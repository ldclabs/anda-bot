use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use futures::{SinkExt, StreamExt};
use reqwest::{
    Client,
    multipart::{Form, Part},
};
use serde_json::Value;
use std::{collections::HashMap, fmt::Write as _, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::delivery::{http_send_error, send_step};
use super::{
    Channel, ChannelMessage, ChannelWorkspace, EVENT_DEDUP_WINDOW, RecentEventDedup, SendMessage,
    file_name_for_resource, is_http_url, is_transient_send_error, random_from_pool,
    resource_from_bytes, split_message_on_word_boundaries,
};
use crate::{
    config::{self, normalize_identity},
    util::file_uri::path_from_file_uri_or_path,
};

const DISCORD_MAX_MESSAGE_LENGTH: usize = 2000;
const DISCORD_MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const DISCORD_MAX_FILES_PER_MESSAGE: usize = 10;
const DISCORD_SEND_CHUNK_DELAY: Duration = Duration::from_millis(500);
const DISCORD_GATEWAY_VERSION: u8 = 10;
const DISCORD_INTENTS: u64 = 37_377;
const DISCORD_ACK_REACTIONS: &[&str] = &[
    "\u{26A1}\u{FE0F}",
    "\u{1F44D}",
    "\u{1F440}",
    "\u{1F4AA}",
    "\u{1F44C}",
];
/// Discord requires a `User-Agent: DiscordBot ($url, $version)` header on every
/// HTTP API request; requests without it are blocked at the edge (Cloudflare),
/// which surfaces as the `gateway/bot` lookup failing before the websocket can
/// be opened. See https://discord.com/developers/docs/reference.
const DISCORD_USER_AGENT: &str = concat!(
    "DiscordBot (https://github.com/ldclabs/anda-bot, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct IncomingAttachment {
    url: String,
    file_name: String,
    file_size: Option<u64>,
    mime_type: Option<String>,
}

#[derive(Debug, Clone)]
struct DiscordUpload {
    file_name: String,
    mime_type: Option<String>,
    bytes: Vec<u8>,
}

pub fn build_discord_channels(
    cfg: &[config::DiscordChannelSettings],
    http_client: Client,
) -> Result<HashMap<String, Arc<dyn Channel>>, BoxError> {
    let mut channels = HashMap::new();

    for (index, discord_cfg) in cfg.iter().enumerate() {
        if discord_cfg.is_empty() {
            continue;
        }

        if discord_cfg.bot_token.trim().is_empty() {
            return Err(format!(
                "Discord channel '{}' requires bot_token",
                discord_cfg.label(index)
            )
            .into());
        }

        let channel: Arc<dyn Channel> =
            Arc::new(DiscordChannel::new(discord_cfg, http_client.clone()));
        let channel_id = channel.id();
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate Discord channel id '{channel_id}'").into());
        }
    }

    Ok(channels)
}

#[derive(Clone, Default)]
struct GatewaySession {
    id: Option<String>,
    resume_url: Option<String>,
    sequence: Option<i64>,
}

pub struct DiscordChannel {
    id: String,
    bot_token: String,
    username: String,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    allow_external_users: bool,
    listen_to_bots: bool,
    mention_only: bool,
    api_base: String,
    ack_reactions: bool,
    client: Client,
    workspace: Arc<ChannelWorkspace>,
    dedup: RecentEventDedup,
    bot_user_id: Mutex<Option<String>>,
    gateway_session: Mutex<GatewaySession>,
}

impl DiscordChannel {
    pub fn new(cfg: &config::DiscordChannelSettings, client: Client) -> Self {
        Self {
            id: cfg.channel_id(),
            bot_token: cfg.bot_token.clone(),
            username: cfg
                .username
                .clone()
                .unwrap_or_else(|| "discord".to_string()),
            guild_id: cfg.guild_id.clone(),
            allowed_users: cfg
                .allowed_users
                .iter()
                .map(|s| normalize_identity(s))
                .collect(),
            allow_external_users: cfg.allow_external_users,
            listen_to_bots: cfg.listen_to_bots,
            mention_only: cfg.mention_only,
            api_base: config::DEFAULT_DISCORD_API_BASE.to_string(),
            ack_reactions: cfg.ack_reactions,
            client,
            workspace: Arc::new(ChannelWorkspace::default()),
            dedup: RecentEventDedup::new(EVENT_DEDUP_WINDOW),
            bot_user_id: Mutex::new(None),
            gateway_session: Mutex::new(GatewaySession::default()),
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/{}", self.api_base, path.trim_start_matches('/'))
    }

    /// Adds the headers Discord requires on every REST call: the bot
    /// `Authorization` token and the mandatory `DiscordBot (...)` `User-Agent`.
    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder
            .header("Authorization", format!("Bot {}", self.bot_token))
            .header("User-Agent", DISCORD_USER_AGENT)
    }

    fn message_channel_id(message: &SendMessage) -> &str {
        message.thread.as_deref().unwrap_or(&message.recipient)
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        let user_id = user_id.trim();
        !user_id.is_empty()
            && self
                .allowed_users
                .iter()
                .any(|allowed| allowed == "*" || allowed == user_id)
    }

    fn bot_user_id_from_token(token: &str) -> Option<String> {
        let part = token.split('.').next()?.trim();
        decode_base64_string(part)
    }

    async fn fetch_bot_user_id(&self) -> Result<String, BoxError> {
        let response = self
            .authorized(self.client.get(self.api_url("users/@me")))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Discord users/@me failed ({status}): {body}").into());
        }

        let data: Value = serde_json::from_str(&body)?;
        data.get("id")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| "Discord users/@me response did not include id".into())
    }

    async fn get_bot_user_id(&self) -> Option<String> {
        {
            let cache = self.bot_user_id.lock().await;
            if let Some(user_id) = cache.as_ref() {
                return Some(user_id.clone());
            }
        }

        if let Some(user_id) = Self::bot_user_id_from_token(&self.bot_token) {
            let mut cache = self.bot_user_id.lock().await;
            *cache = Some(user_id.clone());
            return Some(user_id);
        }

        match self.fetch_bot_user_id().await {
            Ok(user_id) => {
                let mut cache = self.bot_user_id.lock().await;
                *cache = Some(user_id.clone());
                Some(user_id)
            }
            Err(err) => {
                log::warn!("Discord failed to fetch bot user id: {err}");
                None
            }
        }
    }

    async fn fetch_gateway_url(&self) -> Result<String, BoxError> {
        let response = self
            .authorized(self.client.get(self.api_url("gateway/bot")))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Discord gateway/bot failed ({status}): {body}").into());
        }

        let data: Value = serde_json::from_str(&body)?;
        Ok(data
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("wss://gateway.discord.gg")
            .to_string())
    }

    fn parse_attachment_metadata(attachment: &Value) -> Option<IncomingAttachment> {
        let url = attachment.get("url").and_then(Value::as_str)?.to_string();
        let file_name = attachment
            .get("filename")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("attachment.bin")
            .to_string();
        let mime_type = attachment
            .get("content_type")
            .and_then(Value::as_str)
            .map(String::from);

        Some(IncomingAttachment {
            url,
            file_name,
            file_size: attachment.get("size").and_then(Value::as_u64),
            mime_type,
        })
    }

    async fn download_attachment_resource(
        &self,
        attachment: &IncomingAttachment,
    ) -> Option<Resource> {
        if let Some(size) = attachment.file_size
            && size > DISCORD_MAX_FILE_BYTES
        {
            log::warn!(
                "Discord skipping attachment larger than {} bytes: {size}",
                DISCORD_MAX_FILE_BYTES
            );
            return None;
        }

        let response = match self.client.get(&attachment.url).send().await {
            Ok(response) => response,
            Err(err) => {
                log::warn!(
                    "Discord failed to download attachment '{}': {err}",
                    attachment.file_name
                );
                return None;
            }
        };
        let status = response.status();
        if !status.is_success() {
            log::warn!(
                "Discord attachment download failed for '{}' ({status})",
                attachment.file_name
            );
            return None;
        }

        let bytes = match crate::util::http_client::read_limited_body(
            response,
            DISCORD_MAX_FILE_BYTES,
            "Discord attachment",
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!(
                    "Discord failed to read attachment '{}': {err}",
                    attachment.file_name
                );
                return None;
            }
        };

        Some(resource_from_bytes(
            attachment.file_name.clone(),
            bytes,
            "Discord attachment",
        ))
    }

    async fn parse_gateway_message(
        &self,
        message: &Value,
        bot_user_id: &str,
    ) -> Option<ChannelMessage> {
        let author = message.get("author")?;
        let author_id = author.get("id").and_then(Value::as_str).unwrap_or("");
        if !bot_user_id.is_empty() && author_id == bot_user_id {
            return None;
        }

        if !self.listen_to_bots && author.get("bot").and_then(Value::as_bool).unwrap_or(false) {
            return None;
        }

        let trusted_user = self.is_user_allowed(author_id);
        if !trusted_user && !self.allow_external_users {
            let username = author
                .get("username")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            log::warn!(
                "Discord ignoring message from unauthorized user: username={username}, user_id={author_id}"
            );
            return None;
        }

        if let Some(guild_id) = &self.guild_id
            && let Some(message_guild_id) = message.get("guild_id").and_then(Value::as_str)
            && message_guild_id != guild_id
        {
            return None;
        }

        let channel_id = message
            .get("channel_id")
            .and_then(Value::as_str)
            .filter(|channel_id| !channel_id.trim().is_empty())?
            .to_string();
        let raw_content = message.get("content").and_then(Value::as_str).unwrap_or("");
        let attachment_values = message
            .get("attachments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let has_attachments = !attachment_values.is_empty();
        let is_dm = message.get("guild_id").is_none();
        let effective_mention_only = self.mention_only && !is_dm;

        let content = if effective_mention_only {
            if !contains_bot_mention(raw_content, bot_user_id) {
                return None;
            }
            normalize_incoming_content(raw_content, true, bot_user_id).unwrap_or_default()
        } else if raw_content.is_empty() {
            String::new()
        } else {
            normalize_incoming_content(raw_content, false, bot_user_id).unwrap_or_default()
        };

        if content.trim().is_empty() && !has_attachments {
            return None;
        }

        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let mut attachments = Vec::new();
        for attachment_value in attachment_values {
            let Some(attachment) = Self::parse_attachment_metadata(&attachment_value) else {
                continue;
            };
            if let Some(resource) = self.download_attachment_resource(&attachment).await {
                attachments.push(resource);
            }
        }
        self.workspace
            .store_resources_lossy(
                &mut attachments,
                (!message_id.is_empty()).then_some(message_id.as_str()),
                "Discord attachment",
            )
            .await;

        let content = content_with_attachment_fallback(content, &attachments);
        if content.trim().is_empty() && attachments.is_empty() {
            return None;
        }

        let mut extra = std::collections::BTreeMap::new();
        extra.insert("channel_id".to_string(), channel_id.clone().into());
        if !message_id.is_empty() {
            extra.insert("message_id".to_string(), message_id.clone().into());
        }
        if let Some(guild_id) = message.get("guild_id").and_then(Value::as_str) {
            extra.insert("guild_id".to_string(), guild_id.to_string().into());
        }

        Some(ChannelMessage {
            sender: author_id.to_string(),
            external_user: (!trusted_user).then_some(true),
            reply_target: channel_id,
            content,
            channel: self.id(),
            timestamp: unix_ms(),
            thread: None,
            attachments,
            extra,
            ..Default::default()
        })
    }

    async fn send_typing_once(&self, channel_id: &str) -> Result<(), BoxError> {
        let response = self
            .authorized(
                self.client
                    .post(self.api_url(&format!("channels/{channel_id}/typing"))),
            )
            .send()
            .await?;
        if !response.status().is_success() {
            log::debug!(
                "Discord typing indicator failed with status {}",
                response.status()
            );
        }
        Ok(())
    }

    fn try_add_ack_reaction_nonblocking(&self, channel_id: String, message_id: String) {
        let client = self.client.clone();
        let bot_token = self.bot_token.clone();
        let url = discord_reaction_url(
            &self.api_base,
            &channel_id,
            &message_id,
            random_from_pool(DISCORD_ACK_REACTIONS),
        );

        tokio::spawn(async move {
            let response = match client
                .put(url)
                .header("Authorization", format!("Bot {bot_token}"))
                .header("User-Agent", DISCORD_USER_AGENT)
                .header("Content-Length", "0")
                .send()
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    log::debug!("Discord failed to add ACK reaction: {err}");
                    return;
                }
            };

            if !response.status().is_success() {
                log::debug!(
                    "Discord add ACK reaction failed with status {}",
                    response.status()
                );
            }
        });
    }

    async fn post_json_checked(&self, path: &str, body: &Value) -> Result<Value, BoxError> {
        let response = self
            .authorized(self.client.post(self.api_url(path)))
            .json(body)
            .send()
            .await?;
        response_json_checked(response, "Discord POST").await
    }

    async fn send_message_json(&self, channel_id: &str, content: &str) -> Result<(), BoxError> {
        self.post_json_checked(
            &format!("channels/{channel_id}/messages"),
            &serde_json::json!({ "content": content }),
        )
        .await?;
        Ok(())
    }

    async fn send_message_with_uploads(
        &self,
        channel_id: &str,
        content: &str,
        uploads: &[DiscordUpload],
    ) -> Result<(), BoxError> {
        let mut form = Form::new().text(
            "payload_json",
            serde_json::json!({ "content": content }).to_string(),
        );

        for (index, upload) in uploads.iter().enumerate() {
            let mut part = Part::bytes(upload.bytes.clone()).file_name(upload.file_name.clone());
            if let Some(mime_type) = upload.mime_type.as_deref() {
                part = part.mime_str(mime_type)?;
            }
            form = form.part(format!("files[{index}]"), part);
        }

        let response = self
            .authorized(
                self.client
                    .post(self.api_url(&format!("channels/{channel_id}/messages"))),
            )
            .multipart(form)
            .send()
            .await?;
        response_unit_checked(response, "Discord send message with files").await
    }

    async fn collect_outgoing_resources(
        &self,
        resources: &[Resource],
    ) -> Result<(Vec<DiscordUpload>, Vec<String>), BoxError> {
        let mut uploads = Vec::new();
        let mut remote_urls = Vec::new();

        for resource in resources {
            if let Some(blob) = &resource.blob {
                let bytes = blob.0.clone();
                if bytes.len() as u64 > DISCORD_MAX_FILE_BYTES {
                    return Err(format!(
                        "Discord resource '{}' exceeds {} bytes",
                        resource.name, DISCORD_MAX_FILE_BYTES
                    )
                    .into());
                }
                uploads.push(DiscordUpload {
                    file_name: file_name_for_resource(resource).to_string(),
                    mime_type: resource.mime_type.clone(),
                    bytes,
                });
                continue;
            }

            if let Some(uri) = resource.uri.as_deref() {
                if is_http_url(uri) {
                    remote_urls.push(uri.to_string());
                    continue;
                }

                let path = path_from_file_uri_or_path(uri)?;
                let bytes = tokio::fs::read(path).await?;
                if bytes.len() as u64 > DISCORD_MAX_FILE_BYTES {
                    return Err(format!(
                        "Discord resource '{}' exceeds {} bytes",
                        resource.name, DISCORD_MAX_FILE_BYTES
                    )
                    .into());
                }
                uploads.push(DiscordUpload {
                    file_name: file_name_for_resource(resource).to_string(),
                    mime_type: resource.mime_type.clone(),
                    bytes,
                });
                continue;
            }

            return Err(format!("Discord resource '{}' has no uri or blob", resource.name).into());
        }

        Ok((uploads, remote_urls))
    }

    async fn send_content_and_uploads(
        &self,
        channel_id: &str,
        content: &str,
        uploads: &[DiscordUpload],
    ) -> Result<(), BoxError> {
        let mut delivered = 0;
        let chunks = if content.trim().is_empty() {
            Vec::new()
        } else {
            split_message_for_discord(content)
        };

        if uploads.is_empty() {
            if chunks.is_empty() {
                send_step(&mut delivered, || self.send_message_json(channel_id, "…")).await?;
            } else {
                for (index, chunk) in chunks.iter().enumerate() {
                    send_step(&mut delivered, || self.send_message_json(channel_id, chunk)).await?;
                    if index < chunks.len() - 1 {
                        tokio::time::sleep(DISCORD_SEND_CHUNK_DELAY).await;
                    }
                }
            }
            return Ok(());
        }

        let first_content = chunks.first().map_or("", String::as_str);
        let mut upload_batches = uploads.chunks(DISCORD_MAX_FILES_PER_MESSAGE);
        if let Some(first_batch) = upload_batches.next() {
            send_step(&mut delivered, || {
                self.send_message_with_uploads(channel_id, first_content, first_batch)
            })
            .await?;
        }

        for batch in upload_batches {
            tokio::time::sleep(DISCORD_SEND_CHUNK_DELAY).await;
            send_step(&mut delivered, || {
                self.send_message_with_uploads(channel_id, "", batch)
            })
            .await?;
        }

        for chunk in chunks.iter().skip(1) {
            tokio::time::sleep(DISCORD_SEND_CHUNK_DELAY).await;
            send_step(&mut delivered, || self.send_message_json(channel_id, chunk)).await?;
        }

        Ok(())
    }
    async fn receive_gateway(
        &self,
        cancel_token: CancellationToken,
        events: mpsc::Sender<Value>,
    ) -> Result<(), BoxError> {
        let session = self.gateway_session.lock().await.clone();
        let gateway_url = match session.resume_url.as_ref() {
            Some(url) => url.clone(),
            None => self.fetch_gateway_url().await?,
        };
        let websocket_url = format!("{gateway_url}/?v={DISCORD_GATEWAY_VERSION}&encoding=json");
        log::info!("Discord channel {} connecting to gateway", self.id());

        let ws_stream = tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            result = super::websocket::connect(&self.client, &websocket_url) => result?,
        };
        let (mut write, mut read) = ws_stream.split();

        let hello = tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            message = read.next() => message.ok_or("Discord gateway closed before hello")??,
        };
        let heartbeat_interval = match hello {
            Message::Text(text) => {
                let data: Value = serde_json::from_str(text.as_ref())?;
                data.get("d")
                    .and_then(|data| data.get("heartbeat_interval"))
                    .and_then(Value::as_u64)
                    .unwrap_or(41_250)
            }
            _ => return Err("Discord gateway did not send hello text frame".into()),
        };

        let identify = if let Some(id) = session.id {
            serde_json::json!({"op": 6, "d": {"token": self.bot_token, "session_id": id, "seq": session.sequence}})
        } else {
            serde_json::json!({
                "op": 2,
                "d": {
                    "token": self.bot_token,
                    "intents": DISCORD_INTENTS,
                    "properties": {
                        "os": std::env::consts::OS,
                        "browser": "anda_bot",
                        "device": "anda_bot"
                    }
                }
            })
        };
        tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            result = write.send(Message::Text(identify.to_string().into())) => result?,
        }

        log::info!("Discord channel {} listening for messages", self.id());

        let mut sequence = session.sequence;
        let heartbeat_interval = heartbeat_interval.max(1);
        let first_tick = tokio::time::Instant::now()
            + Duration::from_millis(rand::random_range(0..heartbeat_interval));
        let mut heartbeat =
            tokio::time::interval_at(first_tick, Duration::from_millis(heartbeat_interval));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut awaiting_ack = false;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => return Ok(()),
                _ = events.closed() => return Ok(()),
                _ = heartbeat.tick() => {
                    if awaiting_ack { return Err("Discord heartbeat ACK missing; reconnecting".into()); }
                    awaiting_ack = true;
                    let heartbeat_payload = serde_json::json!({"op": 1, "d": sequence});
                    if write.send(Message::Text(heartbeat_payload.to_string().into())).await.is_err() {
                        return Ok(());
                    }
                }
                message = read.next() => {
                    let message = match message {
                        Some(Ok(message)) => message,
                        Some(Err(err)) => {
                            log::warn!("Discord gateway read error: {err}");
                            return Ok(());
                        }
                        None => return Ok(()),
                    };

                    let text = match message {
                        Message::Text(text) => text,
                        Message::Ping(payload) => {
                            if write.send(Message::Pong(payload)).await.is_err() {
                                return Ok(());
                            }
                            continue;
                        }
                        Message::Close(frame) => {
                            if frame.is_some_and(|frame| matches!(u16::from(frame.code), 4007 | 4009)) {
                                *self.gateway_session.lock().await = GatewaySession::default();
                            }
                            return Ok(());
                        }
                        _ => continue,
                    };

                    let event: Value = match serde_json::from_str(text.as_ref()) {
                        Ok(event) => event,
                        Err(err) => {
                            log::debug!("Discord gateway JSON parse error: {err}");
                            continue;
                        }
                    };

                    match event.get("op").and_then(Value::as_u64) {
                        Some(11) => { awaiting_ack = false; }
                        Some(1) => {
                            awaiting_ack = true;
                            write.send(Message::Text(serde_json::json!({"op": 1, "d": sequence}).to_string().into())).await?;
                        }
                        Some(7) => return Ok(()),
                        Some(9) => {
                            if event.get("d").and_then(Value::as_bool) != Some(true) {
                                *self.gateway_session.lock().await = GatewaySession::default();
                            }
                            return Ok(());
                        }
                        Some(0) => {
                            if let Some(data) = event.get("d") {
                                match event.get("t").and_then(Value::as_str) {
                                    Some("READY") => {
                                        let mut session = self.gateway_session.lock().await;
                                        session.id = data.get("session_id").and_then(Value::as_str).map(str::to_owned);
                                        session.resume_url = data.get("resume_gateway_url").and_then(Value::as_str).map(str::to_owned);
                                    }
                                    Some("MESSAGE_CREATE") => {
                                        // Do not advance the resumable sequence if the queue rejects the event.
                                        events.try_send(data.clone()).map_err(|_| "Discord inbound event queue unavailable")?;
                                    }
                                    _ => {}
                                }
                            }
                            if let Some(next) = event.get("s").and_then(Value::as_i64) {
                                sequence = Some(next);
                                self.gateway_session.lock().await.sequence = sequence;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn id(&self) -> String {
        format!("discord:{}", self.id)
    }

    fn set_workspace(&self, workspace: PathBuf) {
        self.workspace.set_path(workspace);
    }

    async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
        let channel_id = Self::message_channel_id(message);
        let (uploads, remote_urls) = self
            .collect_outgoing_resources(&message.attachments)
            .await?;
        let content = with_inline_resource_urls(&message.content, &remote_urls);
        self.send_content_and_uploads(channel_id, &content, &uploads)
            .await
    }

    fn should_retry_send(&self, error: &str) -> bool {
        is_transient_send_error(error)
    }

    async fn listen(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
        let (events, mut incoming) = mpsc::channel::<Value>(64);
        let receive = self.receive_gateway(cancel_token.clone(), events);
        let process = async {
            while let Some(data) = incoming.recv().await {
                if let Some(id) = data.get("id").and_then(Value::as_str)
                    && self.dedup.is_duplicate(id).await
                {
                    continue;
                }
                let Some(message) = self.parse_gateway_message(&data, &bot_user_id).await else {
                    continue;
                };
                if self.ack_reactions
                    && let Some(id) = data.get("id").and_then(Value::as_str)
                {
                    self.try_add_ack_reaction_nonblocking(
                        message.reply_target.clone(),
                        id.to_string(),
                    );
                }
                let _ = self.send_typing_once(&message.reply_target).await;
                if tx.send(message).await.is_err() {
                    break;
                }
            }
        };
        let process = async {
            tokio::select! { _ = cancel_token.cancelled() => {}, _ = process => {} }
        };
        let (result, ()) = tokio::join!(receive, process);
        result
    }
}

async fn response_json_checked(
    response: reqwest::Response,
    _context: &str,
) -> Result<Value, BoxError> {
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(http_send_error(status, &headers, &text));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn response_unit_checked(
    response: reqwest::Response,
    _context: &str,
) -> Result<(), BoxError> {
    let status = response.status();
    if !status.is_success() {
        let headers = response.headers().clone();
        let text = response.text().await?;
        return Err(http_send_error(status, &headers, &text));
    }
    Ok(())
}

fn decode_base64_string(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }

    URL_SAFE_NO_PAD
        .decode(input)
        .or_else(|_| URL_SAFE.decode(input))
        .or_else(|_| STANDARD_NO_PAD.decode(input))
        .or_else(|_| STANDARD.decode(input))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|value| !value.is_empty())
}

fn mention_tags(bot_user_id: &str) -> [String; 2] {
    [format!("<@{bot_user_id}>"), format!("<@!{bot_user_id}>")]
}

fn contains_bot_mention(content: &str, bot_user_id: &str) -> bool {
    if bot_user_id.is_empty() {
        return false;
    }
    let tags = mention_tags(bot_user_id);
    content.contains(&tags[0]) || content.contains(&tags[1])
}

fn normalize_incoming_content(
    content: &str,
    mention_only: bool,
    bot_user_id: &str,
) -> Option<String> {
    if content.is_empty() {
        return None;
    }

    if mention_only && !contains_bot_mention(content, bot_user_id) {
        return None;
    }

    let mut normalized = content.to_string();
    if mention_only {
        for tag in mention_tags(bot_user_id) {
            normalized = normalized.replace(&tag, " ");
        }
    }

    let normalized = normalized.trim().to_string();
    (!normalized.is_empty()).then_some(normalized)
}

fn content_with_attachment_fallback(content: String, attachments: &[Resource]) -> String {
    if !content.trim().is_empty() || attachments.is_empty() {
        return content;
    }

    attachments
        .iter()
        .map(|resource| format!("[Attachment: {}]", resource.name))
        .collect::<Vec<_>>()
        .join("\n")
}

fn with_inline_resource_urls(content: &str, remote_urls: &[String]) -> String {
    if remote_urls.is_empty() {
        return content.to_string();
    }

    let mut lines = Vec::new();
    if !content.trim().is_empty() {
        lines.push(content.trim().to_string());
    }
    lines.extend(remote_urls.iter().cloned());
    lines.join("\n")
}

fn split_message_for_discord(message: &str) -> Vec<String> {
    split_message_on_word_boundaries(
        message,
        DISCORD_MAX_MESSAGE_LENGTH,
        DISCORD_MAX_MESSAGE_LENGTH,
    )
}

fn encode_emoji_for_discord(emoji: &str) -> String {
    if emoji.contains(':') {
        return emoji.to_string();
    }

    let mut encoded = String::new();
    for byte in emoji.as_bytes() {
        let _ = write!(encoded, "%{byte:02X}");
    }
    encoded
}

fn raw_discord_message_id(message_id: &str) -> &str {
    message_id.strip_prefix("discord_").unwrap_or(message_id)
}

fn discord_reaction_url(api_base: &str, channel_id: &str, message_id: &str, emoji: &str) -> String {
    let encoded_emoji = encode_emoji_for_discord(emoji);
    format!(
        "{}/channels/{channel_id}/messages/{}/reactions/{encoded_emoji}/@me",
        api_base.trim_end_matches('/'),
        raw_discord_message_id(message_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;

    fn test_config() -> config::DiscordChannelSettings {
        config::DiscordChannelSettings {
            id: Some("test".to_string()),
            user: None,
            bot_token: "MTIzNDU2.fake.hmac".to_string(),
            username: Some("anda-discord".to_string()),
            guild_id: Some("987".to_string()),
            allowed_users: vec!["111".to_string(), "*".to_string()],
            allow_external_users: false,
            listen_to_bots: false,
            mention_only: true,
            ack_reactions: true,
        }
    }

    #[test]
    fn discord_channel_identity() {
        let channel = DiscordChannel::new(&test_config(), new_reqwest_client());
        assert_eq!(channel.name(), "discord");
        assert_eq!(channel.username(), "anda-discord");
        assert_eq!(channel.id(), "discord:test");
    }

    #[test]
    fn bot_user_id_extraction() {
        assert_eq!(
            DiscordChannel::bot_user_id_from_token("MTIzNDU2.fake.hmac"),
            Some("123456".to_string())
        );
    }

    #[test]
    fn allowed_users_match_exact_id_or_wildcard() {
        let channel = DiscordChannel::new(&test_config(), new_reqwest_client());
        assert!(channel.is_user_allowed("111"));
        assert!(channel.is_user_allowed("222"));
        assert!(!channel.is_user_allowed(""));
    }

    #[tokio::test]
    async fn parse_gateway_message_marks_non_allowlisted_sender_external_when_enabled() {
        let mut cfg = test_config();
        cfg.allowed_users = vec!["111".to_string()];
        cfg.allow_external_users = true;
        cfg.mention_only = false;
        let channel = DiscordChannel::new(&cfg, new_reqwest_client());
        let payload = serde_json::json!({
            "id": "msg_1",
            "channel_id": "chan_1",
            "guild_id": "987",
            "content": "hello",
            "author": { "id": "222", "username": "bob", "bot": false },
            "attachments": []
        });

        let message = channel
            .parse_gateway_message(&payload, "999")
            .await
            .unwrap();

        assert_eq!(message.sender, "222");
        assert_eq!(message.content, "hello");
        assert!(message.external_user.unwrap_or_default());
    }

    #[test]
    fn normalize_incoming_content_strips_mentions() {
        assert_eq!(
            normalize_incoming_content("  <@!123456> run status  ", true, "123456"),
            Some("run status".to_string())
        );
        assert!(normalize_incoming_content("hello", true, "123456").is_none());
    }

    #[test]
    fn split_message_for_discord_chunks_long_text() {
        let text = "a".repeat(DISCORD_MAX_MESSAGE_LENGTH + 10);
        let chunks = split_message_for_discord(&text);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].chars().count() <= DISCORD_MAX_MESSAGE_LENGTH);
        assert!(chunks[1].chars().count() <= DISCORD_MAX_MESSAGE_LENGTH);
    }

    #[test]
    fn encode_reaction_url_escapes_unicode_emoji() {
        let url = discord_reaction_url(
            config::DEFAULT_DISCORD_API_BASE,
            "123",
            "discord_456",
            "\u{1F440}",
        );
        assert_eq!(
            url,
            "https://discord.com/api/v10/channels/123/messages/456/reactions/%F0%9F%91%80/@me"
        );
    }

    use anda_core::ByteBufB64;
    use axum::{
        Router,
        extract::{Path as AxumPath, State},
        routing,
    };
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct MockApi {
        requests: StdMutex<Vec<(String, String, Value)>>,
        user_agents: StdMutex<Vec<(String, Option<String>)>>,
        gateway_url: StdMutex<Option<String>>,
    }

    impl MockApi {
        fn record(&self, method: &str, path: String, body: Value) {
            self.requests
                .lock()
                .unwrap()
                .push((method.to_string(), path, body));
        }

        fn recorded(&self, method: &str, path_part: &str) -> Vec<Value> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, p, _)| m == method && p.contains(path_part))
                .map(|(_, _, body)| body.clone())
                .collect()
        }

        fn user_agent(&self, path_part: &str) -> Option<String> {
            self.user_agents
                .lock()
                .unwrap()
                .iter()
                .find(|(path, _)| path.contains(path_part))
                .and_then(|(_, ua)| ua.clone())
        }
    }

    async fn record_handler(
        method: http::Method,
        state: Arc<MockApi>,
        path: String,
        body: axum::body::Bytes,
    ) -> axum::Json<Value> {
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        state.record(method.as_str(), path.clone(), parsed);

        let response = if path == "users/@me" {
            serde_json::json!({"id": "999", "username": "anda"})
        } else if path == "gateway/bot" {
            serde_json::json!({"url": state.gateway_url.lock().unwrap().as_deref().unwrap_or("wss://gateway.example")})
        } else if path.ends_with("/messages") {
            serde_json::json!({"id": "msg_100"})
        } else {
            serde_json::json!({})
        };
        axum::Json(response)
    }

    async fn spawn_discord_mock(state: Arc<MockApi>) -> String {
        async fn handle(
            method: http::Method,
            State(state): State<Arc<MockApi>>,
            AxumPath(path): AxumPath<String>,
            headers: axum::http::HeaderMap,
            body: axum::body::Bytes,
        ) -> axum::Json<Value> {
            let user_agent = headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            state
                .user_agents
                .lock()
                .unwrap()
                .push((path.clone(), user_agent));
            record_handler(method, state, path, body).await
        }

        let app = Router::new()
            .route("/{*path}", routing::any(handle))
            .with_state(state);
        crate::test_support::spawn_http_mock(app).await
    }

    async fn mock_channel(
        mutate: impl FnOnce(&mut config::DiscordChannelSettings),
        state: Arc<MockApi>,
    ) -> DiscordChannel {
        let mut cfg = test_config();
        mutate(&mut cfg);
        let mut channel = DiscordChannel::new(&cfg, new_reqwest_client());
        channel.api_base = spawn_discord_mock(state).await;
        channel
    }

    #[tokio::test]
    async fn send_combines_text_uploads_and_remote_urls() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("notes.txt");
        tokio::fs::write(&local, b"local-bytes").await.unwrap();

        channel
            .send(&SendMessage {
                content: "see files".to_string(),
                recipient: "chan_1".to_string(),
                attachments: vec![
                    Resource {
                        name: "pic.png".to_string(),
                        uri: Some("https://cdn.example.com/pic.png".to_string()),
                        ..Default::default()
                    },
                    Resource {
                        name: "blob.bin".to_string(),
                        blob: Some(ByteBufB64(b"blob-bytes".to_vec())),
                        ..Default::default()
                    },
                    Resource {
                        name: "notes.txt".to_string(),
                        uri: Some(format!("file://{}", local.display())),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
            .await
            .unwrap();

        // Uploads go out as one multipart message (recorded with a null body).
        let sends = state.recorded("POST", "channels/chan_1/messages");
        assert_eq!(sends.len(), 1);
        assert!(sends[0].is_null());
    }

    #[tokio::test]
    async fn send_text_only_and_placeholder_messages() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        channel
            .send(&SendMessage {
                content: "plain message".to_string(),
                recipient: "chan_2".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let sends = state.recorded("POST", "channels/chan_2/messages");
        assert_eq!(sends[0]["content"], "plain message");

        // Thread overrides the recipient channel.
        channel
            .send(&SendMessage {
                content: "threaded".to_string(),
                recipient: "chan_2".to_string(),
                thread: Some("thread_9".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            state.recorded("POST", "channels/thread_9/messages").len(),
            1
        );

        // Empty content and no attachments sends a placeholder space.
        channel
            .send(&SendMessage {
                recipient: "chan_3".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            state.recorded("POST", "channels/chan_3/messages")[0]["content"],
            "…"
        );
    }

    #[tokio::test]
    async fn collect_outgoing_resources_rejects_invalid_entries() {
        let channel = DiscordChannel::new(&test_config(), new_reqwest_client());

        let err = channel
            .collect_outgoing_resources(&[Resource::default()])
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("has no uri or blob"));
    }

    #[tokio::test]
    async fn bot_user_id_falls_back_to_api_when_token_is_opaque() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(
            |cfg| cfg.bot_token = "!!.fake.hmac".to_string(),
            state.clone(),
        )
        .await;

        assert_eq!(channel.get_bot_user_id().await.as_deref(), Some("999"));
        // Cached on the second call.
        assert_eq!(channel.get_bot_user_id().await.as_deref(), Some("999"));
        assert_eq!(state.recorded("GET", "users/@me").len(), 1);

        let url = channel.fetch_gateway_url().await.unwrap();
        assert_eq!(url, "wss://gateway.example");
    }

    #[tokio::test]
    async fn rest_requests_send_discord_bot_user_agent() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        // Discord blocks requests without a `DiscordBot (...)` User-Agent at the
        // edge, which previously made the gateway/bot lookup fail.
        channel.fetch_gateway_url().await.unwrap();
        channel.send_message_json("chan_1", "hi").await.unwrap();

        let gateway_ua = state
            .user_agent("gateway/bot")
            .expect("gateway request recorded");
        assert_eq!(gateway_ua, DISCORD_USER_AGENT);
        assert!(gateway_ua.starts_with("DiscordBot ("));
        assert_eq!(
            state.user_agent("channels/chan_1/messages").as_deref(),
            Some(DISCORD_USER_AGENT)
        );
    }

    #[tokio::test]
    async fn gateway_messages_filter_by_author_guild_and_mentions() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|cfg| cfg.mention_only = true, state.clone()).await;

        // Own messages are ignored.
        let own = serde_json::json!({
            "id": "m1", "channel_id": "c1", "content": "hi",
            "author": {"id": "999"}, "attachments": [],
        });
        assert!(channel.parse_gateway_message(&own, "999").await.is_none());

        // Bot authors are ignored unless listen_to_bots is set.
        let from_bot = serde_json::json!({
            "id": "m2", "channel_id": "c1", "content": "hi",
            "author": {"id": "111", "bot": true}, "attachments": [],
        });
        assert!(
            channel
                .parse_gateway_message(&from_bot, "999")
                .await
                .is_none()
        );

        // Messages from other guilds are ignored.
        let other_guild = serde_json::json!({
            "id": "m3", "channel_id": "c1", "guild_id": "654", "content": "<@999> hi",
            "author": {"id": "111"}, "attachments": [],
        });
        assert!(
            channel
                .parse_gateway_message(&other_guild, "999")
                .await
                .is_none()
        );

        // Guild messages require a mention; the mention is stripped.
        let no_mention = serde_json::json!({
            "id": "m4", "channel_id": "c1", "guild_id": "987", "content": "hi",
            "author": {"id": "111"}, "attachments": [],
        });
        assert!(
            channel
                .parse_gateway_message(&no_mention, "999")
                .await
                .is_none()
        );

        let mentioned = serde_json::json!({
            "id": "m5", "channel_id": "c1", "guild_id": "987", "content": "<@999> do it",
            "author": {"id": "111"}, "attachments": [],
        });
        let message = channel
            .parse_gateway_message(&mentioned, "999")
            .await
            .expect("mentioned message");
        assert_eq!(message.content, "do it");
        assert_eq!(message.reply_target, "c1");
        assert_eq!(message.sender, "111");

        // DMs bypass mention_only.
        let dm = serde_json::json!({
            "id": "m6", "channel_id": "c1", "content": "direct",
            "author": {"id": "111"}, "attachments": [],
        });
        let message = channel.parse_gateway_message(&dm, "999").await.unwrap();
        assert_eq!(message.content, "direct");
    }

    #[tokio::test]
    async fn gateway_messages_download_attachments() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|cfg| cfg.mention_only = false, state.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        channel.set_workspace(dir.path().to_path_buf());

        // Serve the attachment bytes from a tiny static server.
        let files = Router::new().route("/img.png", routing::get(|| async { "PNGDATA" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let files_addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, files).await.unwrap();
        });

        let payload = serde_json::json!({
            "id": "m7", "channel_id": "c1", "content": "",
            "author": {"id": "111"},
            "attachments": [
                {
                    "url": format!("http://{files_addr}/img.png"),
                    "filename": "img.png",
                    "size": 7,
                    "content_type": "image/png",
                },
                // Oversized attachments are skipped without failing the message.
                {
                    "url": format!("http://{files_addr}/img.png"),
                    "filename": "huge.bin",
                    "size": DISCORD_MAX_FILE_BYTES + 1,
                },
            ],
        });

        let message = channel
            .parse_gateway_message(&payload, "999")
            .await
            .expect("attachment message");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].name, "img.png");
        assert_eq!(message.content, "[Attachment: img.png]");
    }

    #[test]
    fn pure_helpers_normalize_content_and_ids() {
        assert_eq!(decode_base64_string(""), None);
        assert_eq!(decode_base64_string("!!"), None);
        assert_eq!(decode_base64_string("MTIzNDU2").as_deref(), Some("123456"));

        assert!(contains_bot_mention("<@!999> hi", "999"));
        assert!(!contains_bot_mention("hi", "999"));
        assert!(!contains_bot_mention("<@999>", ""));

        assert_eq!(normalize_incoming_content("", false, "999"), None);
        assert_eq!(
            normalize_incoming_content("<@999>  hi", true, "999").as_deref(),
            Some("hi")
        );
        assert_eq!(normalize_incoming_content("<@999>", true, "999"), None);

        assert_eq!(content_with_attachment_fallback(String::new(), &[]), "");
        assert_eq!(
            with_inline_resource_urls("text", &["https://a".to_string()]),
            "text\nhttps://a"
        );
        assert_eq!(with_inline_resource_urls("text", &[]), "text");

        assert_eq!(raw_discord_message_id("discord_5"), "5");
        assert_eq!(raw_discord_message_id("5"), "5");

        assert!(DISCORD_ACK_REACTIONS.contains(&random_from_pool(DISCORD_ACK_REACTIONS)));
        assert_eq!(encode_emoji_for_discord("custom:123"), "custom:123");
    }
    #[tokio::test]
    async fn gateway_keeps_heartbeats_during_download_and_resumes_after_reconnect() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let media = Router::new().route(
            "/image.png",
            routing::get({
                let started = started.clone();
                let release = release.clone();
                move || {
                    let started = started.clone();
                    let release = release.clone();
                    async move {
                        started.notify_one();
                        release.notified().await;
                        "PNGDATA"
                    }
                }
            }),
        );
        let media_url = format!(
            "{}/image.png",
            crate::test_support::spawn_http_mock(media).await
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway = format!("ws://{}", listener.local_addr().unwrap());
        let state = Arc::new(MockApi::default());
        *state.gateway_url.lock().unwrap() = Some(gateway.clone());
        let channel = Arc::new(mock_channel(|c| c.mention_only = false, state).await);
        let server = tokio::spawn(async move {
            for connection in 0..2 {
                let (socket, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
                ws.send(Message::Text(
                    serde_json::json!({"op":10,"d":{"heartbeat_interval":100}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
                let handshake: Value =
                    serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                assert_eq!(handshake["op"], if connection == 0 { 2 } else { 6 });
                if connection == 0 {
                    ws.send(Message::Text(serde_json::json!({"op":0,"t":"READY","s":1,"d":{"session_id":"session-1","resume_gateway_url":gateway}}).to_string().into())).await.unwrap();
                    ws.send(Message::Text(serde_json::json!({"op":0,"t":"MESSAGE_CREATE","s":2,"d":{"id":"media-message","channel_id":"c1","author":{"id":"111"},"attachments":[{"url":media_url,"filename":"image.png"}]}}).to_string().into())).await.unwrap();
                    started.notified().await;
                    ws.send(Message::Ping(b"probe".to_vec().into()))
                        .await
                        .unwrap();
                    let mut pong = false;
                    let mut heartbeats = 0;
                    while !pong || heartbeats < 2 {
                        let frame = tokio::time::timeout(Duration::from_secs(2), ws.next())
                            .await
                            .unwrap()
                            .unwrap()
                            .unwrap();
                        match frame {
                            Message::Pong(bytes) => {
                                assert_eq!(bytes.as_ref(), b"probe");
                                pong = true;
                            }
                            Message::Text(text) => {
                                let event: Value = serde_json::from_str(&text).unwrap();
                                if event["op"] == 1 {
                                    heartbeats += 1;
                                    ws.send(Message::Text("{\"op\":11}".into())).await.unwrap();
                                }
                            }
                            other => panic!("unexpected frame: {other:?}"),
                        }
                    }
                    ws.send(Message::Text("{\"op\":7}".into())).await.unwrap();
                    release.notify_one();
                } else {
                    assert_eq!(handshake["d"]["session_id"], "session-1");
                    assert_eq!(handshake["d"]["seq"], 2);
                    ws.send(Message::Text(serde_json::json!({"op":0,"t":"MESSAGE_CREATE","s":3,"d":{"id":"missed-message","channel_id":"c1","author":{"id":"111"},"content":"missed during disconnect","attachments":[]}}).to_string().into())).await.unwrap();
                    ws.send(Message::Text("{\"op\":7}".into())).await.unwrap();
                }
            }
        });
        let (tx, mut rx) = mpsc::channel(4);
        let run_channel = channel.clone();
        let run = tokio::spawn(async move {
            run_channel
                .listen(CancellationToken::new(), tx.clone())
                .await
                .unwrap();
            run_channel
                .listen(CancellationToken::new(), tx)
                .await
                .unwrap();
        });
        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.attachments.len(), 1);
        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.content, "missed during disconnect");
        run.await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn gateway_missing_ack_forces_reconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            ws.send(Message::Text(
                "{\"op\":10,\"d\":{\"heartbeat_interval\":20}}".into(),
            ))
            .await
            .unwrap();
            while let Some(Ok(_)) = ws.next().await {}
        });
        let channel = DiscordChannel::new(&test_config(), new_reqwest_client());
        channel.gateway_session.lock().await.resume_url = Some(gateway);
        let (events, _rx) = mpsc::channel(4);
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            channel.receive_gateway(CancellationToken::new(), events),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("ACK missing"));
        server.await.unwrap();
    }
}
