use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{
    Client,
    multipart::{Form, Part},
};
use serde_json::Value;
use std::{collections::HashMap, fmt::Write as _, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use super::{
    Channel, ChannelMessage, ChannelWorkspace, SendMessage, file_name_for_resource, is_http_url,
    resource_from_bytes,
};
use crate::config;

const DISCORD_MAX_MESSAGE_LENGTH: usize = 2000;
const DISCORD_MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const DISCORD_MAX_FILES_PER_MESSAGE: usize = 10;
#[allow(dead_code)]
const DISCORD_TYPING_INTERVAL: Duration = Duration::from_secs(8);
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

pub struct DiscordChannel {
    id: String,
    bot_token: String,
    username: String,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    listen_to_bots: bool,
    mention_only: bool,
    api_base: String,
    ack_reactions: bool,
    client: Client,
    workspace: Arc<ChannelWorkspace>,
    bot_user_id: Mutex<Option<String>>,
    #[allow(dead_code)]
    typing_handles: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
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
            allowed_users: cfg.allowed_users.clone(),
            listen_to_bots: cfg.listen_to_bots,
            mention_only: cfg.mention_only,
            api_base: config::DEFAULT_DISCORD_API_BASE.to_string(),
            ack_reactions: cfg.ack_reactions,
            client,
            workspace: Arc::new(ChannelWorkspace::default()),
            bot_user_id: Mutex::new(None),
            typing_handles: Mutex::new(HashMap::new()),
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/{}", self.api_base, path.trim_start_matches('/'))
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
            .client
            .get(self.api_url("users/@me"))
            .header("Authorization", format!("Bot {}", self.bot_token))
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
            .client
            .get(self.api_url("gateway/bot"))
            .header("Authorization", format!("Bot {}", self.bot_token))
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

        let bytes = match response.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(err) => {
                log::warn!(
                    "Discord failed to read attachment '{}': {err}",
                    attachment.file_name
                );
                return None;
            }
        };
        if bytes.len() as u64 > DISCORD_MAX_FILE_BYTES {
            log::warn!(
                "Discord skipping downloaded attachment larger than {} bytes: {}",
                DISCORD_MAX_FILE_BYTES,
                attachment.file_name
            );
            return None;
        }

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

        if !self.is_user_allowed(author_id) {
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
            .client
            .post(self.api_url(&format!("channels/{channel_id}/typing")))
            .header("Authorization", format!("Bot {}", self.bot_token))
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
            random_discord_ack_reaction(),
        );

        tokio::spawn(async move {
            let response = match client
                .put(url)
                .header("Authorization", format!("Bot {bot_token}"))
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
            .client
            .post(self.api_url(path))
            .header("Authorization", format!("Bot {}", self.bot_token))
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

    #[allow(dead_code)]
    async fn send_message_json_with_id(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<String, BoxError> {
        let response = self
            .post_json_checked(
                &format!("channels/{channel_id}/messages"),
                &serde_json::json!({ "content": content }),
            )
            .await?;
        response
            .get("id")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| "Discord send response missing id".into())
    }

    #[allow(dead_code)]
    async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<(), BoxError> {
        let response = self
            .client
            .patch(self.api_url(&format!(
                "channels/{channel_id}/messages/{}",
                raw_discord_message_id(message_id)
            )))
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await?;
        response_json_checked(response, "Discord edit message").await?;
        Ok(())
    }

    #[allow(dead_code)]
    async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<(), BoxError> {
        let response = self
            .client
            .delete(self.api_url(&format!(
                "channels/{channel_id}/messages/{}",
                raw_discord_message_id(message_id)
            )))
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await?;
        response_unit_checked(response, "Discord delete message").await
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
            .client
            .post(self.api_url(&format!("channels/{channel_id}/messages")))
            .header("Authorization", format!("Bot {}", self.bot_token))
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

                let path = uri.strip_prefix("file://").unwrap_or(uri);
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
        let chunks = if content.trim().is_empty() {
            Vec::new()
        } else {
            split_message_for_discord(content)
        };

        if uploads.is_empty() {
            if chunks.is_empty() {
                self.send_message_json(channel_id, " ").await?;
            } else {
                for (index, chunk) in chunks.iter().enumerate() {
                    self.send_message_json(channel_id, chunk).await?;
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
            self.send_message_with_uploads(channel_id, first_content, first_batch)
                .await?;
        }

        for batch in upload_batches {
            tokio::time::sleep(DISCORD_SEND_CHUNK_DELAY).await;
            self.send_message_with_uploads(channel_id, "", batch)
                .await?;
        }

        for chunk in chunks.iter().skip(1) {
            tokio::time::sleep(DISCORD_SEND_CHUNK_DELAY).await;
            self.send_message_json(channel_id, chunk).await?;
        }

        Ok(())
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
        let error = error.to_ascii_lowercase();
        error.contains("timeout")
            || error.contains("connection")
            || error.contains("temporarily")
            || error.contains("too many requests")
            || error.contains("429")
            || error.contains("502")
            || error.contains("503")
            || error.contains("504")
    }

    async fn listen(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
        if self.mention_only && bot_user_id.is_empty() {
            log::warn!("Discord mention_only is enabled but bot user id is unavailable");
        }

        let gateway_url = self.fetch_gateway_url().await?;
        let websocket_url = format!("{gateway_url}/?v={DISCORD_GATEWAY_VERSION}&encoding=json");
        log::info!("Discord channel {} connecting to gateway", self.id());

        let (ws_stream, _) = tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            result = connect_async(&websocket_url) => result?,
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

        let identify = serde_json::json!({
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
        });
        tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            result = write.send(Message::Text(identify.to_string().into())) => result?,
        }

        log::info!("Discord channel {} listening for messages", self.id());

        let mut sequence: Option<i64> = None;
        let mut heartbeat = tokio::time::interval(Duration::from_millis(heartbeat_interval));
        heartbeat.tick().await;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => return Ok(()),
                _ = heartbeat.tick() => {
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
                        Message::Close(_) => return Ok(()),
                        _ => continue,
                    };

                    let event: Value = match serde_json::from_str(text.as_ref()) {
                        Ok(event) => event,
                        Err(err) => {
                            log::debug!("Discord gateway JSON parse error: {err}");
                            continue;
                        }
                    };

                    if let Some(next_sequence) = event.get("s").and_then(Value::as_i64) {
                        sequence = Some(next_sequence);
                    }

                    match event.get("op").and_then(Value::as_u64).unwrap_or_default() {
                        1 => {
                            let heartbeat_payload = serde_json::json!({"op": 1, "d": sequence});
                            if write.send(Message::Text(heartbeat_payload.to_string().into())).await.is_err() {
                                return Ok(());
                            }
                            continue;
                        }
                        7 | 9 => return Ok(()),
                        _ => {}
                    }

                    if event.get("t").and_then(Value::as_str) != Some("MESSAGE_CREATE") {
                        continue;
                    }

                    let Some(data) = event.get("d") else {
                        continue;
                    };

                    let Some(channel_message) = self.parse_gateway_message(data, &bot_user_id).await else {
                        continue;
                    };

                    if self.ack_reactions
                        && let Some(message_id) = data.get("id").and_then(Value::as_str)
                    {
                        self.try_add_ack_reaction_nonblocking(
                            channel_message.reply_target.clone(),
                            message_id.to_string(),
                        );
                    }

                    let _ = self.send_typing_once(&channel_message.reply_target).await;

                    if tx.send(channel_message).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        matches!(
            tokio::time::timeout(
                Duration::from_secs(5),
                self.client
                    .get(self.api_url("users/@me"))
                    .header("Authorization", format!("Bot {}", self.bot_token))
                    .send(),
            )
            .await,
            Ok(Ok(response)) if response.status().is_success()
        )
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), BoxError> {
        self.stop_typing(recipient).await?;

        let client = self.client.clone();
        let url = self.api_url(&format!("channels/{recipient}/typing"));
        let bot_token = self.bot_token.clone();
        let handle = tokio::spawn(async move {
            loop {
                let _ = client
                    .post(&url)
                    .header("Authorization", format!("Bot {bot_token}"))
                    .send()
                    .await;
                tokio::time::sleep(DISCORD_TYPING_INTERVAL).await;
            }
        });

        let mut guard = self.typing_handles.lock().await;
        guard.insert(recipient.to_string(), handle);

        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> Result<(), BoxError> {
        let mut guard = self.typing_handles.lock().await;
        if let Some(handle) = guard.remove(recipient) {
            handle.abort();
        }
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        true
    }

    async fn send_draft(&self, message: &SendMessage) -> Result<Option<String>, BoxError> {
        let channel_id = Self::message_channel_id(message);
        let content = if message.content.trim().is_empty() {
            "..."
        } else {
            message.content.as_str()
        };
        self.send_message_json_with_id(channel_id, content)
            .await
            .map(Some)
    }

    async fn update_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), BoxError> {
        let text = truncate_for_discord(text);
        self.edit_message(recipient, message_id, text).await
    }

    async fn update_draft_progress(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), BoxError> {
        self.update_draft(recipient, message_id, text).await
    }

    async fn finalize_draft(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), BoxError> {
        if text.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH {
            return self.edit_message(recipient, message_id, text).await;
        }

        let _ = self.delete_message(recipient, message_id).await;
        for (index, chunk) in split_message_for_discord(text).iter().enumerate() {
            self.send_message_json(recipient, chunk).await?;
            if index < split_message_for_discord(text).len() - 1 {
                tokio::time::sleep(DISCORD_SEND_CHUNK_DELAY).await;
            }
        }
        Ok(())
    }

    async fn cancel_draft(&self, recipient: &str, message_id: &str) -> Result<(), BoxError> {
        let _ = self.stop_typing(recipient).await;
        self.delete_message(recipient, message_id).await
    }

    async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<(), BoxError> {
        let response = self
            .client
            .put(discord_reaction_url(
                &self.api_base,
                channel_id,
                message_id,
                emoji,
            ))
            .header("Authorization", format!("Bot {}", self.bot_token))
            .header("Content-Length", "0")
            .send()
            .await?;
        response_unit_checked(response, "Discord add reaction").await
    }

    async fn remove_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<(), BoxError> {
        let response = self
            .client
            .delete(discord_reaction_url(
                &self.api_base,
                channel_id,
                message_id,
                emoji,
            ))
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await?;
        response_unit_checked(response, "Discord remove reaction").await
    }

    async fn pin_message(&self, channel_id: &str, message_id: &str) -> Result<(), BoxError> {
        let response = self
            .client
            .put(self.api_url(&format!(
                "channels/{channel_id}/pins/{}",
                raw_discord_message_id(message_id)
            )))
            .header("Authorization", format!("Bot {}", self.bot_token))
            .header("Content-Length", "0")
            .send()
            .await?;
        response_unit_checked(response, "Discord pin message").await
    }

    async fn unpin_message(&self, channel_id: &str, message_id: &str) -> Result<(), BoxError> {
        let response = self
            .client
            .delete(self.api_url(&format!(
                "channels/{channel_id}/pins/{}",
                raw_discord_message_id(message_id)
            )))
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await?;
        response_unit_checked(response, "Discord unpin message").await
    }

    async fn redact_message(
        &self,
        channel_id: &str,
        message_id: &str,
        _reason: Option<String>,
    ) -> Result<(), BoxError> {
        self.delete_message(channel_id, message_id).await
    }
}

async fn response_json_checked(
    response: reqwest::Response,
    context: &str,
) -> Result<Value, BoxError> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{context} failed ({status}): {text}").into());
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn response_unit_checked(response: reqwest::Response, context: &str) -> Result<(), BoxError> {
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("{context} failed ({status}): {text}").into());
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
    if message.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        if remaining.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH {
            chunks.push(remaining.to_string());
            break;
        }

        let hard_split = remaining
            .char_indices()
            .nth(DISCORD_MAX_MESSAGE_LENGTH)
            .map_or(remaining.len(), |(index, _)| index);
        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            if let Some(position) = search_area.rfind('\n') {
                if search_area[..position].chars().count() >= DISCORD_MAX_MESSAGE_LENGTH / 2 {
                    position + 1
                } else {
                    search_area
                        .rfind(' ')
                        .map_or(hard_split, |position| position + 1)
                }
            } else {
                search_area
                    .rfind(' ')
                    .map_or(hard_split, |position| position + 1)
            }
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    chunks
}

#[allow(dead_code)]
fn truncate_for_discord(text: &str) -> &str {
    if text.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH {
        return text;
    }

    let end = text
        .char_indices()
        .nth(DISCORD_MAX_MESSAGE_LENGTH)
        .map_or(text.len(), |(index, _)| index);
    &text[..end]
}

fn random_discord_ack_reaction() -> &'static str {
    let upper = DISCORD_ACK_REACTIONS.len() as u64;
    let reject_threshold = (u64::MAX / upper) * upper;

    loop {
        let value = rand::random::<u64>();
        if value < reject_threshold {
            return DISCORD_ACK_REACTIONS[(value % upper) as usize];
        }
    }
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

    fn test_config() -> config::DiscordChannelSettings {
        config::DiscordChannelSettings {
            id: Some("test".to_string()),
            bot_token: "MTIzNDU2.fake.hmac".to_string(),
            username: Some("anda-discord".to_string()),
            guild_id: Some("987".to_string()),
            allowed_users: vec!["111".to_string(), "*".to_string()],
            listen_to_bots: false,
            mention_only: true,
            ack_reactions: true,
        }
    }

    #[test]
    fn discord_channel_identity() {
        let channel = DiscordChannel::new(&test_config(), Client::new());
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
        let channel = DiscordChannel::new(&test_config(), Client::new());
        assert!(channel.is_user_allowed("111"));
        assert!(channel.is_user_allowed("222"));
        assert!(!channel.is_user_allowed(""));
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

    #[tokio::test]
    async fn stop_typing_clears_handle() {
        let channel = DiscordChannel::new(&test_config(), Client::new());
        {
            let mut guard = channel.typing_handles.lock().await;
            guard.insert(
                "123".to_string(),
                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }),
            );
        }

        channel.stop_typing("123").await.unwrap();

        assert!(channel.typing_handles.lock().await.is_empty());
    }
}
