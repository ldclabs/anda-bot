use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use reqwest::{
    Client, RequestBuilder, Response,
    multipart::{Form, Part},
};
use serde_json::Value;
use std::{collections::HashMap, fmt::Write as _, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use super::{
    Channel, ChannelMessage, ChannelWorkspace, SendMessage, file_name_for_resource, is_http_url,
    is_transient_send_error, resource_from_bytes, split_message_on_word_boundaries,
};
use crate::{
    config::{self, normalize_identity},
    util::file_uri::path_from_file_uri_or_path,
};

const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;
const TELEGRAM_CONTINUATION_OVERHEAD: usize = 30;
const TELEGRAM_MAX_FILE_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;
const TELEGRAM_LONG_POLL_TIMEOUT_SECS: u64 = 30;
const TELEGRAM_RETRY_DELAY: Duration = Duration::from_secs(5);
const TELEGRAM_CONFLICT_DELAY: Duration = Duration::from_secs(35);
const TELEGRAM_ACK_REACTIONS: &[&str] = &[
    "\u{26A1}\u{FE0F}",
    "\u{1F44D}",
    "\u{1F440}",
    "\u{1F525}",
    "\u{1F44C}",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct IncomingAttachment {
    file_id: String,
    file_name: Option<String>,
    file_size: Option<u64>,
    mime_type: Option<String>,
    caption: Option<String>,
}

pub fn build_telegram_channels(
    cfg: &[config::TelegramChannelSettings],
    client: Client,
) -> Result<HashMap<String, Arc<dyn Channel>>, BoxError> {
    let mut channels = HashMap::new();

    for (index, telegram_cfg) in cfg.iter().enumerate() {
        if telegram_cfg.is_empty() {
            continue;
        }

        if telegram_cfg.bot_token.trim().is_empty() {
            return Err(format!(
                "Telegram channel '{}' requires bot_token",
                telegram_cfg.label(index)
            )
            .into());
        }

        let channel: Arc<dyn Channel> =
            Arc::new(TelegramChannel::new(telegram_cfg, client.clone()));
        let channel_id = channel.id();
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate Telegram channel id '{channel_id}'").into());
        }
    }

    Ok(channels)
}

pub struct TelegramChannel {
    id: String,
    bot_token: String,
    username: String,
    allowed_users: Vec<String>,
    allow_external_users: bool,
    mention_only: bool,
    api_base: String,
    ack_reactions: bool,
    client: Client,
    workspace: Arc<ChannelWorkspace>,
    bot_username: Mutex<Option<String>>,
    #[allow(dead_code)]
    typing_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TelegramChannel {
    pub fn new(cfg: &config::TelegramChannelSettings, client: Client) -> Self {
        Self {
            id: cfg.channel_id(),
            bot_token: cfg.bot_token.clone(),
            username: cfg
                .username
                .clone()
                .unwrap_or_else(|| "telegram".to_string()),
            allowed_users: cfg
                .allowed_users
                .iter()
                .map(|s| normalize_identity(s))
                .collect(),
            allow_external_users: cfg.allow_external_users,
            mention_only: cfg.mention_only,
            api_base: config::DEFAULT_TELEGRAM_API_BASE.to_string(),
            ack_reactions: cfg.ack_reactions,
            client,
            workspace: Arc::new(ChannelWorkspace::default()),
            bot_username: Mutex::new(None),
            typing_handle: Mutex::new(None),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.api_base, self.bot_token)
    }

    fn file_url(&self, file_path: &str) -> String {
        format!("{}/file/bot{}/{file_path}", self.api_base, self.bot_token)
    }

    /// Redacts the bot token before logging. reqwest errors embed the request URL, which
    /// contains the token (`.../bot<token>/...`), so error messages must be scrubbed to
    /// avoid leaking credentials into logs.
    fn scrub(&self, text: &str) -> String {
        scrub_token(text, &self.bot_token)
    }

    async fn send_request(&self, request: RequestBuilder) -> Result<Response, BoxError> {
        request
            .send()
            .await
            .map_err(|err| -> BoxError { self.scrub(&err.to_string()).into() })
    }

    fn parse_reply_target(reply_target: &str) -> (String, Option<String>) {
        if let Some((chat_id, thread_id)) = reply_target.split_once(':') {
            (chat_id.to_string(), Some(thread_id.to_string()))
        } else {
            (reply_target.to_string(), None)
        }
    }

    fn is_user_allowed(&self, identity: &str) -> bool {
        let identity = normalize_identity(identity);
        self.allowed_users.iter().any(|allowed| {
            allowed == "*"
                || allowed == &identity
                || (!allowed.chars().all(|c| c.is_ascii_digit())
                    && allowed.eq_ignore_ascii_case(&identity))
        })
    }

    fn is_any_user_allowed<'a, I>(&self, identities: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        identities.into_iter().any(|id| self.is_user_allowed(id))
    }

    fn is_telegram_username_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_'
    }

    fn find_bot_mention_spans(text: &str, bot_username: &str) -> Vec<(usize, usize)> {
        let bot_username = bot_username.trim_start_matches('@');
        if bot_username.is_empty() {
            return Vec::new();
        }

        let mut spans = Vec::new();
        for (at_idx, ch) in text.char_indices() {
            if ch != '@' {
                continue;
            }

            if at_idx > 0 {
                let prev = text[..at_idx].chars().next_back().unwrap_or(' ');
                if Self::is_telegram_username_char(prev) {
                    continue;
                }
            }

            let username_start = at_idx + 1;
            let mut username_end = username_start;
            for (rel_idx, candidate_ch) in text[username_start..].char_indices() {
                if Self::is_telegram_username_char(candidate_ch) {
                    username_end = username_start + rel_idx + candidate_ch.len_utf8();
                } else {
                    break;
                }
            }

            if username_end == username_start {
                continue;
            }

            let mention_username = &text[username_start..username_end];
            if mention_username.eq_ignore_ascii_case(bot_username) {
                spans.push((at_idx, username_end));
            }
        }

        spans
    }

    fn contains_bot_mention(text: &str, bot_username: &str) -> bool {
        !Self::find_bot_mention_spans(text, bot_username).is_empty()
    }

    fn normalize_incoming_content(text: &str, bot_username: &str) -> Option<String> {
        let spans = Self::find_bot_mention_spans(text, bot_username);
        if spans.is_empty() {
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            return (!normalized.is_empty()).then_some(normalized);
        }

        let mut normalized = String::with_capacity(text.len());
        let mut cursor = 0;
        for (start, end) in spans {
            normalized.push_str(&text[cursor..start]);
            cursor = end;
        }
        normalized.push_str(&text[cursor..]);

        let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
        (!normalized.is_empty()).then_some(normalized)
    }

    fn is_group_message(message: &Value) -> bool {
        message
            .get("chat")
            .and_then(|chat| chat.get("type"))
            .and_then(Value::as_str)
            .map(|kind| kind == "group" || kind == "supergroup")
            .unwrap_or(false)
    }

    fn extract_sender_info(message: &Value) -> (String, Option<String>, String) {
        let username = message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let sender_id = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string());
        let sender_identity = if username == "unknown" {
            sender_id.clone().unwrap_or_else(|| "unknown".to_string())
        } else {
            username.clone()
        };

        (username, sender_id, sender_identity)
    }

    fn format_forward_attribution(message: &Value) -> Option<String> {
        if let Some(from_chat) = message.get("forward_from_chat") {
            let title = from_chat
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("unknown channel");
            Some(format!("[Forwarded from channel: {title}] "))
        } else if let Some(from_user) = message.get("forward_from") {
            let label = from_user
                .get("username")
                .and_then(Value::as_str)
                .map(|username| format!("@{username}"))
                .or_else(|| {
                    from_user
                        .get("first_name")
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_else(|| "unknown".to_string());
            Some(format!("[Forwarded from {label}] "))
        } else {
            message
                .get("forward_sender_name")
                .and_then(Value::as_str)
                .map(|name| format!("[Forwarded from {name}] "))
        }
    }

    fn extract_reply_context(message: &Value) -> Option<String> {
        let reply = message.get("reply_to_message")?;
        let reply_sender = reply
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(Value::as_str)
            .or_else(|| {
                reply
                    .get("from")
                    .and_then(|from| from.get("first_name"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("unknown");

        let reply_text = if let Some(text) = reply.get("text").and_then(Value::as_str) {
            text.to_string()
        } else if reply.get("photo").is_some() {
            "[Photo]".to_string()
        } else if reply.get("document").is_some() {
            "[Document]".to_string()
        } else if reply.get("voice").is_some() || reply.get("audio").is_some() {
            "[Voice]".to_string()
        } else if reply.get("video").is_some() {
            "[Video]".to_string()
        } else {
            "[Message]".to_string()
        };

        let quoted_lines = reply_text
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        Some(format!("> @{reply_sender}:\n{quoted_lines}"))
    }

    async fn fetch_bot_username(&self) -> Result<String, BoxError> {
        let response = self
            .send_request(self.client.get(self.api_url("getMe")))
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Telegram getMe failed ({status}): {}", self.scrub(&body)).into());
        }

        let data: Value = serde_json::from_str(&body)?;
        let username = data
            .get("result")
            .and_then(|result| result.get("username"))
            .and_then(Value::as_str)
            .ok_or("Telegram getMe response did not include result.username")?;

        Ok(username.to_string())
    }

    async fn get_bot_username(&self) -> Option<String> {
        {
            let cache = self.bot_username.lock().await;
            if let Some(username) = cache.as_ref() {
                return Some(username.clone());
            }
        }

        match self.fetch_bot_username().await {
            Ok(username) => {
                let mut cache = self.bot_username.lock().await;
                *cache = Some(username.clone());
                Some(username)
            }
            Err(err) => {
                log::warn!(
                    "Telegram failed to fetch bot username: {}",
                    self.scrub(&err.to_string())
                );
                None
            }
        }
    }

    async fn parse_update_message(&self, update: &Value) -> Option<ChannelMessage> {
        let message = update.get("message")?;
        let text = message.get("text").and_then(Value::as_str)?;

        let (username, sender_id, sender_identity) = Self::extract_sender_info(message);
        let mut identities = vec![username.as_str()];
        if let Some(sender_id) = sender_id.as_deref() {
            identities.push(sender_id);
        }
        let trusted_user = self.is_any_user_allowed(identities.iter().copied());
        if !trusted_user && !self.allow_external_users {
            self.log_unauthorized(message);
            return None;
        }

        let is_group = Self::is_group_message(message);
        let content = if self.mention_only && is_group {
            let bot_username = self.get_bot_username().await?;
            if !Self::contains_bot_mention(text, &bot_username) {
                return None;
            }
            Self::normalize_incoming_content(text, &bot_username)?
        } else {
            text.to_string()
        };

        self.channel_message_from_parts(
            message,
            sender_identity,
            !trusted_user,
            content,
            Vec::new(),
        )
    }

    fn channel_message_from_parts(
        &self,
        message: &Value,
        sender: String,
        external_user: bool,
        content: String,
        attachments: Vec<Resource>,
    ) -> Option<ChannelMessage> {
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string())?;
        let message_id = message
            .get("message_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let thread = message
            .get("message_thread_id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string());
        let reply_target = if let Some(thread) = &thread {
            format!("{chat_id}:{thread}")
        } else {
            chat_id.clone()
        };

        let content = if let Some(quote) = Self::extract_reply_context(message) {
            format!("{quote}\n\n{content}")
        } else {
            content
        };

        let content = if let Some(attr) = Self::format_forward_attribution(message) {
            format!("{attr}{content}")
        } else {
            content
        };

        let mut extra = std::collections::BTreeMap::new();
        extra.insert("message_id".to_string(), message_id.into());
        extra.insert("chat_id".to_string(), chat_id.clone().into());

        Some(ChannelMessage {
            sender,
            external_user: external_user.then_some(true),
            reply_target,
            content,
            channel: self.id(),
            timestamp: unix_ms(),
            thread,
            attachments,
            extra,
            ..Default::default()
        })
    }

    fn log_unauthorized(&self, message: &Value) {
        let username = message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let sender_id = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        log::warn!(
            "Telegram ignoring message from unauthorized user: username={username}, sender_id={sender_id}"
        );
    }

    fn parse_attachment_metadata(message: &Value) -> Option<IncomingAttachment> {
        if let Some(document) = message.get("document") {
            return Some(IncomingAttachment {
                file_id: document.get("file_id")?.as_str()?.to_string(),
                file_name: document
                    .get("file_name")
                    .and_then(Value::as_str)
                    .map(String::from),
                file_size: document.get("file_size").and_then(Value::as_u64),
                mime_type: document
                    .get("mime_type")
                    .and_then(Value::as_str)
                    .map(String::from),
                caption: message
                    .get("caption")
                    .and_then(Value::as_str)
                    .map(String::from),
            });
        }

        if let Some(photos) = message.get("photo").and_then(Value::as_array) {
            let photo = photos.last()?;
            return Some(IncomingAttachment {
                file_id: photo.get("file_id")?.as_str()?.to_string(),
                file_name: None,
                file_size: photo.get("file_size").and_then(Value::as_u64),
                mime_type: Some("image/jpeg".to_string()),
                caption: message
                    .get("caption")
                    .and_then(Value::as_str)
                    .map(String::from),
            });
        }

        for field in ["voice", "audio", "video"] {
            if let Some(media) = message.get(field) {
                return Some(IncomingAttachment {
                    file_id: media.get("file_id")?.as_str()?.to_string(),
                    file_name: media
                        .get("file_name")
                        .and_then(Value::as_str)
                        .map(String::from),
                    file_size: media.get("file_size").and_then(Value::as_u64),
                    mime_type: media
                        .get("mime_type")
                        .and_then(Value::as_str)
                        .map(String::from),
                    caption: message
                        .get("caption")
                        .and_then(Value::as_str)
                        .map(String::from),
                });
            }
        }

        None
    }

    async fn try_parse_attachment_message(&self, update: &Value) -> Option<ChannelMessage> {
        let message = update.get("message")?;
        let attachment = Self::parse_attachment_metadata(message)?;

        if let Some(size) = attachment.file_size
            && size > TELEGRAM_MAX_FILE_DOWNLOAD_BYTES
        {
            log::warn!(
                "Telegram skipping attachment larger than {} bytes: {size}",
                TELEGRAM_MAX_FILE_DOWNLOAD_BYTES
            );
            return None;
        }

        let (username, sender_id, sender_identity) = Self::extract_sender_info(message);
        let mut identities = vec![username.as_str()];
        if let Some(sender_id) = sender_id.as_deref() {
            identities.push(sender_id);
        }
        let trusted_user = self.is_any_user_allowed(identities.iter().copied());
        if !trusted_user && !self.allow_external_users {
            self.log_unauthorized(message);
            return None;
        }

        let is_group = Self::is_group_message(message);
        let caption = attachment.caption.as_deref().unwrap_or("");
        let caption = if self.mention_only && is_group {
            let bot_username = self.get_bot_username().await?;
            if !Self::contains_bot_mention(caption, &bot_username) {
                return None;
            }
            Self::normalize_incoming_content(caption, &bot_username).unwrap_or_default()
        } else {
            caption.to_string()
        };

        let telegram_path = match self.get_file_path(&attachment.file_id).await {
            Ok(path) => path,
            Err(err) => {
                log::warn!(
                    "Telegram failed to get attachment file path: {}",
                    self.scrub(&err.to_string())
                );
                return None;
            }
        };
        let bytes = match self.download_file(&telegram_path).await {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!(
                    "Telegram failed to download attachment: {}",
                    self.scrub(&err.to_string())
                );
                return None;
            }
        };

        let file_name = attachment
            .file_name
            .clone()
            .or_else(|| telegram_path.rsplit('/').next().map(String::from))
            .unwrap_or_default();
        let mut resource = resource_from_bytes(file_name, bytes, "Telegram attachment");
        let message_key = message
            .get("message_id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string());
        self.workspace
            .store_resource_lossy(&mut resource, message_key.as_deref(), "Telegram attachment")
            .await;

        let content = if caption.trim().is_empty() {
            resource.name.clone()
        } else {
            caption
        };

        self.channel_message_from_parts(
            message,
            sender_identity,
            !trusted_user,
            content,
            vec![resource],
        )
    }

    async fn get_file_path(&self, file_id: &str) -> Result<String, BoxError> {
        let response = self
            .send_request(
                self.client
                    .post(self.api_url("getFile"))
                    .json(&serde_json::json!({ "file_id": file_id })),
            )
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(
                format!("Telegram getFile failed ({status}): {}", self.scrub(&body)).into(),
            );
        }

        let data: Value = serde_json::from_str(&body)?;
        data.get("result")
            .and_then(|result| result.get("file_path"))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| "Telegram getFile response did not include result.file_path".into())
    }

    async fn download_file(&self, file_path: &str) -> Result<Vec<u8>, BoxError> {
        let response = self
            .send_request(self.client.get(self.file_url(file_path)))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Telegram file download failed ({status}): {}",
                self.scrub(&body)
            )
            .into());
        }

        Ok(response
            .bytes()
            .await
            .map_err(|err| -> BoxError { self.scrub(&err.to_string()).into() })?
            .to_vec())
    }

    fn extract_update_message_target(update: &Value) -> Option<(String, i64)> {
        let message = update.get("message")?;
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(Value::as_i64)?
            .to_string();
        let message_id = message.get("message_id").and_then(Value::as_i64)?;
        Some((chat_id, message_id))
    }

    fn try_add_ack_reaction_nonblocking(&self, chat_id: String, message_id: i64) {
        let client = self.client.clone();
        let url = self.api_url("setMessageReaction");
        let token = self.bot_token.clone();
        let emoji = random_telegram_ack_reaction().to_string();
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reaction": [{
                "type": "emoji",
                "emoji": emoji,
            }]
        });

        tokio::spawn(async move {
            let response = match client.post(url).json(&body).send().await {
                Ok(response) => response,
                Err(err) => {
                    log::debug!(
                        "Telegram failed to add ACK reaction: {}",
                        scrub_token(&err.to_string(), &token)
                    );
                    return;
                }
            };

            if !response.status().is_success() {
                log::debug!(
                    "Telegram add ACK reaction failed with status {}",
                    response.status()
                );
            }
        });
    }

    async fn send_chat_action(&self, recipient: &str) -> Result<(), BoxError> {
        let (chat_id, thread_id) = Self::parse_reply_target(recipient);
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        });
        if let Some(thread_id) = thread_id {
            body["message_thread_id"] = Value::String(thread_id);
        }

        let _ = self
            .send_request(self.client.post(self.api_url("sendChatAction")).json(&body))
            .await?;
        Ok(())
    }

    async fn wait_or_cancel(cancel_token: &CancellationToken, delay: Duration) -> bool {
        tokio::select! {
            _ = cancel_token.cancelled() => true,
            _ = tokio::time::sleep(delay) => false,
        }
    }

    async fn send_text_chunks(
        &self,
        message: &str,
        chat_id: &str,
        thread_id: Option<&str>,
    ) -> Result<(), BoxError> {
        let chunks = split_message_for_telegram(message);

        for (index, chunk) in chunks.iter().enumerate() {
            let text = if chunks.len() > 1 {
                if index == 0 {
                    format!("{chunk}\n\n(continues...)")
                } else if index == chunks.len() - 1 {
                    format!("(continued)\n\n{chunk}")
                } else {
                    format!("(continued)\n\n{chunk}\n\n(continues...)")
                }
            } else {
                chunk.to_string()
            };

            let mut html_body = serde_json::json!({
                "chat_id": chat_id,
                "text": Self::markdown_to_telegram_html(&text),
                "parse_mode": "HTML",
            });
            if let Some(thread_id) = thread_id {
                html_body["message_thread_id"] = Value::String(thread_id.to_string());
            }

            match self.post_json_checked("sendMessage", &html_body).await {
                Ok(()) => {}
                Err(markdown_err) => {
                    let mut plain_body = serde_json::json!({
                        "chat_id": chat_id,
                        "text": text,
                    });
                    if let Some(thread_id) = thread_id {
                        plain_body["message_thread_id"] = Value::String(thread_id.to_string());
                    }
                    self.post_json_checked("sendMessage", &plain_body)
                        .await
                        .map_err(|plain_err| {
                            format!(
                                "Telegram sendMessage failed (HTML: {markdown_err}; plain: {plain_err})"
                            )
                        })?;
                }
            }

            if index < chunks.len() - 1 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        Ok(())
    }

    async fn post_json_checked(&self, method: &str, body: &Value) -> Result<(), BoxError> {
        let response = self
            .send_request(self.client.post(self.api_url(method)).json(body))
            .await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("{status}: {}", self.scrub(&text)).into());
        }

        if let Ok(data) = serde_json::from_str::<Value>(&text)
            && !data.get("ok").and_then(Value::as_bool).unwrap_or(true)
        {
            let description = data
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("Telegram API returned ok=false")
                .to_string();
            return Err(self.scrub(&description).into());
        }

        Ok(())
    }

    async fn send_resource(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        resource: &Resource,
    ) -> Result<(), BoxError> {
        if let Some(uri) = resource.uri.as_deref() {
            if is_http_url(uri) {
                return self
                    .send_resource_url(chat_id, thread_id, resource, uri)
                    .await;
            }

            let path = path_from_file_uri_or_path(uri)?;
            if path.exists() {
                let bytes = tokio::fs::read(path).await?;
                return self
                    .send_resource_bytes(chat_id, thread_id, resource, bytes)
                    .await;
            }
        }

        if let Some(blob) = &resource.blob {
            return self
                .send_resource_bytes(chat_id, thread_id, resource, blob.0.clone())
                .await;
        }

        Err(format!("Telegram resource '{}' has no uri or blob", resource.name).into())
    }

    async fn send_resource_url(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        resource: &Resource,
        uri: &str,
    ) -> Result<(), BoxError> {
        let (method, field) = telegram_method_and_field(resource);
        let mut body = serde_json::json!({
            "chat_id": chat_id,
        });
        body[field] = Value::String(uri.to_string());
        if let Some(thread_id) = thread_id {
            body["message_thread_id"] = Value::String(thread_id.to_string());
        }

        self.post_json_checked(method, &body).await
    }

    async fn send_resource_bytes(
        &self,
        chat_id: &str,
        thread_id: Option<&str>,
        resource: &Resource,
        bytes: Vec<u8>,
    ) -> Result<(), BoxError> {
        let (method, field) = telegram_method_and_field(resource);
        let file_name = file_name_for_resource(resource).to_string();
        let part = Part::bytes(bytes).file_name(file_name);
        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part(field.to_string(), part);
        if let Some(thread_id) = thread_id {
            form = form.text("message_thread_id", thread_id.to_string());
        }

        let response = self
            .send_request(self.client.post(self.api_url(method)).multipart(form))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(
                format!("Telegram {method} failed ({status}): {}", self.scrub(&body)).into(),
            );
        }

        Ok(())
    }

    fn markdown_to_telegram_html(text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let mut result_lines: Vec<String> = Vec::new();

        for line in &lines {
            let trimmed_line = line.trim_start();
            if trimmed_line.starts_with("```") {
                result_lines.push(trimmed_line.to_string());
                continue;
            }

            let mut line_out = String::new();
            let stripped = line.trim_start_matches('#');
            let header_level = line.len() - stripped.len();
            if header_level > 0 && line.starts_with('#') && stripped.starts_with(' ') {
                let title = Self::escape_html(stripped.trim());
                result_lines.push(format!("<b>{title}</b>"));
                continue;
            }

            let mut index = 0;
            let bytes = line.as_bytes();
            let len = bytes.len();
            while index < len {
                if index + 1 < len
                    && bytes[index] == b'*'
                    && bytes[index + 1] == b'*'
                    && let Some(end) = line[index + 2..].find("**")
                {
                    let inner = Self::escape_html(&line[index + 2..index + 2 + end]);
                    let _ = write!(line_out, "<b>{inner}</b>");
                    index += 4 + end;
                    continue;
                }
                if index + 1 < len
                    && bytes[index] == b'_'
                    && bytes[index + 1] == b'_'
                    && let Some(end) = line[index + 2..].find("__")
                {
                    let inner = Self::escape_html(&line[index + 2..index + 2 + end]);
                    let _ = write!(line_out, "<b>{inner}</b>");
                    index += 4 + end;
                    continue;
                }
                if bytes[index] == b'*'
                    && (index == 0 || bytes[index - 1] != b'*')
                    && let Some(end) = line[index + 1..].find('*')
                    && end > 0
                {
                    let inner = Self::escape_html(&line[index + 1..index + 1 + end]);
                    let _ = write!(line_out, "<i>{inner}</i>");
                    index += 2 + end;
                    continue;
                }
                if bytes[index] == b'`'
                    && (index == 0 || bytes[index - 1] != b'`')
                    && let Some(end) = line[index + 1..].find('`')
                {
                    let inner = Self::escape_html(&line[index + 1..index + 1 + end]);
                    let _ = write!(line_out, "<code>{inner}</code>");
                    index += 2 + end;
                    continue;
                }
                if bytes[index] == b'['
                    && let Some(bracket_end) = line[index + 1..].find(']')
                {
                    let text_part = &line[index + 1..index + 1 + bracket_end];
                    let after_bracket = index + 1 + bracket_end + 1;
                    if after_bracket < len
                        && bytes[after_bracket] == b'('
                        && let Some(paren_end) = line[after_bracket + 1..].find(')')
                    {
                        let url = &line[after_bracket + 1..after_bracket + 1 + paren_end];
                        if is_http_url(url) {
                            let text_html = Self::escape_html(text_part);
                            let url_html = Self::escape_html(url);
                            let _ = write!(line_out, "<a href=\"{url_html}\">{text_html}</a>");
                            index = after_bracket + 1 + paren_end + 1;
                            continue;
                        }
                    }
                }
                if index + 1 < len
                    && bytes[index] == b'~'
                    && bytes[index + 1] == b'~'
                    && let Some(end) = line[index + 2..].find("~~")
                {
                    let inner = Self::escape_html(&line[index + 2..index + 2 + end]);
                    let _ = write!(line_out, "<s>{inner}</s>");
                    index += 4 + end;
                    continue;
                }

                let ch = line[index..].chars().next().unwrap();
                match ch {
                    '<' => line_out.push_str("&lt;"),
                    '>' => line_out.push_str("&gt;"),
                    '&' => line_out.push_str("&amp;"),
                    '"' => line_out.push_str("&quot;"),
                    '\'' => line_out.push_str("&#39;"),
                    _ => line_out.push(ch),
                }
                index += ch.len_utf8();
            }
            result_lines.push(line_out);
        }

        let joined = result_lines.join("\n");
        let mut final_out = String::with_capacity(joined.len());
        let mut in_code_block = false;
        let mut code_buf = String::new();

        for line in joined.split('\n') {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    let escaped = code_buf.trim_end_matches('\n');
                    let _ = writeln!(final_out, "<pre><code>{escaped}</code></pre>");
                    code_buf.clear();
                } else {
                    in_code_block = true;
                    code_buf.clear();
                }
            } else if in_code_block {
                code_buf.push_str(line);
                code_buf.push('\n');
            } else {
                final_out.push_str(line);
                final_out.push('\n');
            }
        }
        if in_code_block && !code_buf.is_empty() {
            let _ = writeln!(final_out, "<pre><code>{}</code></pre>", code_buf.trim_end());
        }

        final_out.trim_end_matches('\n').to_string()
    }

    fn escape_html(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn id(&self) -> String {
        format!("telegram:{}", self.id)
    }

    fn set_workspace(&self, workspace: PathBuf) {
        self.workspace.set_path(workspace);
    }

    async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
        let (chat_id, thread_id) = Self::parse_reply_target(&message.recipient);
        let thread_id = thread_id.as_deref();

        if !message.content.trim().is_empty() {
            self.send_text_chunks(&message.content, &chat_id, thread_id)
                .await?;
        }

        for resource in &message.attachments {
            self.send_resource(&chat_id, thread_id, resource).await?;
        }

        if message.content.trim().is_empty() && message.attachments.is_empty() {
            self.send_text_chunks(" ", &chat_id, thread_id).await?;
        }

        Ok(())
    }

    fn should_retry_send(&self, error: &str) -> bool {
        is_transient_send_error(error)
    }

    async fn listen(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        let mut offset: i64 = 0;
        if self.mention_only {
            let _ = self.get_bot_username().await;
        }

        log::info!("Telegram channel {} listening for messages", self.id());

        loop {
            if cancel_token.is_cancelled() {
                return Ok(());
            }

            if self.mention_only {
                let missing_username = self.bot_username.lock().await.is_none();
                if missing_username {
                    let _ = self.get_bot_username().await;
                }
            }

            let body = serde_json::json!({
                "offset": offset,
                "timeout": TELEGRAM_LONG_POLL_TIMEOUT_SECS,
                "allowed_updates": ["message"],
            });

            let response = tokio::select! {
                _ = cancel_token.cancelled() => return Ok(()),
                response = self.send_request(self.client.post(self.api_url("getUpdates")).json(&body)) => response,
            };

            let response = match response {
                Ok(response) => response,
                Err(err) => {
                    log::warn!("Telegram poll error: {}", self.scrub(&err.to_string()));
                    if Self::wait_or_cancel(&cancel_token, TELEGRAM_RETRY_DELAY).await {
                        return Ok(());
                    }
                    continue;
                }
            };

            let status = response.status();
            let data = match response.json::<Value>().await {
                Ok(data) => data,
                Err(err) => {
                    log::warn!(
                        "Telegram poll parse error ({status}): {}",
                        self.scrub(&err.to_string())
                    );
                    if Self::wait_or_cancel(&cancel_token, TELEGRAM_RETRY_DELAY).await {
                        return Ok(());
                    }
                    continue;
                }
            };

            let ok = data.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if !status.is_success() || !ok {
                let error_code = data
                    .get("error_code")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let description = data
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown Telegram API error");

                if error_code == 409 {
                    log::warn!(
                        "Telegram polling conflict (409): {description}; ensure only one process uses this bot token"
                    );
                    if Self::wait_or_cancel(&cancel_token, TELEGRAM_CONFLICT_DELAY).await {
                        return Ok(());
                    }
                } else {
                    log::warn!(
                        "Telegram getUpdates failed ({status}, code={error_code}): {description}"
                    );
                    if Self::wait_or_cancel(&cancel_token, TELEGRAM_RETRY_DELAY).await {
                        return Ok(());
                    }
                }
                continue;
            }

            let Some(results) = data.get("result").and_then(Value::as_array) else {
                continue;
            };

            for update in results {
                if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                    offset = update_id + 1;
                }

                let message = if let Some(message) = self.parse_update_message(update).await {
                    message
                } else if let Some(message) = self.try_parse_attachment_message(update).await {
                    message
                } else {
                    continue;
                };

                if self.ack_reactions
                    && let Some((chat_id, message_id)) = Self::extract_update_message_target(update)
                {
                    self.try_add_ack_reaction_nonblocking(chat_id, message_id);
                }

                let _ = self.send_chat_action(&message.reply_target).await;

                if tx.send(message).await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        matches!(
            tokio::time::timeout(
                Duration::from_secs(5),
                self.send_request(self.client.get(self.api_url("getMe")))
            )
            .await,
            Ok(Ok(response)) if response.status().is_success()
        )
    }

    async fn start_typing(&self, recipient: &str) -> Result<(), BoxError> {
        self.stop_typing(recipient).await?;

        let client = self.client.clone();
        let url = self.api_url("sendChatAction");
        let (chat_id, thread_id) = Self::parse_reply_target(recipient);
        let handle = tokio::spawn(async move {
            loop {
                let mut body = serde_json::json!({
                    "chat_id": chat_id,
                    "action": "typing",
                });
                if let Some(thread_id) = &thread_id {
                    body["message_thread_id"] = Value::String(thread_id.clone());
                }
                let _ = client.post(&url).json(&body).send().await;
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        });

        let mut guard = self.typing_handle.lock().await;
        *guard = Some(handle);

        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> Result<(), BoxError> {
        let mut guard = self.typing_handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
        Ok(())
    }
}

/// Replaces every occurrence of the bot token with a placeholder so it never reaches logs.
fn scrub_token(text: &str, token: &str) -> String {
    if token.is_empty() {
        return text.to_string();
    }
    text.replace(token, "<redacted>")
}

fn random_telegram_ack_reaction() -> &'static str {
    let upper = TELEGRAM_ACK_REACTIONS.len() as u64;
    let reject_threshold = (u64::MAX / upper) * upper;

    loop {
        let value = rand::random::<u64>();
        if value < reject_threshold {
            return TELEGRAM_ACK_REACTIONS[(value % upper) as usize];
        }
    }
}

fn split_message_for_telegram(message: &str) -> Vec<String> {
    split_message_on_word_boundaries(
        message,
        TELEGRAM_MAX_MESSAGE_LENGTH,
        TELEGRAM_MAX_MESSAGE_LENGTH - TELEGRAM_CONTINUATION_OVERHEAD,
    )
}

fn telegram_method_and_field(resource: &Resource) -> (&'static str, &'static str) {
    let mime_type = resource.mime_type.as_deref().unwrap_or_default();
    // `infer_resource` records capitalized matcher tags ("Image"/"Video"/...),
    // while callers may set lowercase tags by hand; match case-insensitively.
    let has_tag = |kind: &str| {
        resource
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(kind))
    };
    if has_tag("image") || mime_type.starts_with("image/") {
        ("sendPhoto", "photo")
    } else if has_tag("video") || mime_type.starts_with("video/") {
        ("sendVideo", "video")
    } else if has_tag("audio") || mime_type.starts_with("audio/") {
        ("sendAudio", "audio")
    } else {
        ("sendDocument", "document")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;

    fn test_config() -> config::TelegramChannelSettings {
        config::TelegramChannelSettings {
            id: Some("test".to_string()),
            user: None,
            bot_token: "123:ABC".to_string(),
            username: Some("anda_bot".to_string()),
            allowed_users: vec!["@Alice".to_string(), "12345".to_string()],
            allow_external_users: false,
            mention_only: true,
            ack_reactions: true,
        }
    }

    #[test]
    fn telegram_channel_identity() {
        let channel = TelegramChannel::new(&test_config(), new_reqwest_client());
        assert_eq!(channel.name(), "telegram");
        assert_eq!(channel.username(), "anda_bot");
        assert_eq!(channel.id(), "telegram:test");
    }

    #[test]
    fn scrub_token_redacts_bot_token_from_error_text() {
        let channel = TelegramChannel::new(&test_config(), new_reqwest_client());
        // Mirrors what reqwest's error Display embeds: the full request URL with the token.
        let leaked = format!(
            "error sending request for url (https://api.telegram.org/bot{}/getUpdates)",
            channel.bot_token
        );
        let scrubbed = channel.scrub(&leaked);
        assert!(!scrubbed.contains(&channel.bot_token), "{scrubbed}");
        assert!(scrubbed.contains("<redacted>"), "{scrubbed}");

        // The free function leaves text untouched when no token is configured.
        assert_eq!(scrub_token("nothing to hide", ""), "nothing to hide");
    }

    #[test]
    fn parse_reply_target_supports_threads() {
        assert_eq!(
            TelegramChannel::parse_reply_target("-100123:456"),
            ("-100123".to_string(), Some("456".to_string()))
        );
        assert_eq!(
            TelegramChannel::parse_reply_target("123"),
            ("123".to_string(), None)
        );
    }

    #[test]
    fn allowed_users_match_username_or_numeric_id() {
        let channel = TelegramChannel::new(&test_config(), new_reqwest_client());
        assert!(channel.is_user_allowed("alice"));
        assert!(channel.is_user_allowed("Alice"));
        assert!(channel.is_user_allowed("12345"));
        assert!(!channel.is_user_allowed("bob"));
    }

    #[tokio::test]
    async fn parse_message_marks_non_allowlisted_sender_external_when_enabled() {
        let mut cfg = test_config();
        cfg.allowed_users = vec!["Alice".to_string()];
        cfg.allow_external_users = true;
        cfg.mention_only = false;
        let channel = TelegramChannel::new(&cfg, new_reqwest_client());
        let update = serde_json::json!({
            "message": {
                "message_id": 42,
                "text": "hello",
                "from": { "id": 67890, "username": "Bob" },
                "chat": { "id": 12345, "type": "private" }
            }
        });

        let message = channel.parse_update_message(&update).await.unwrap();

        assert_eq!(message.sender, "Bob");
        assert_eq!(message.content, "hello");
        assert!(message.external_user.unwrap_or_default());
    }

    #[test]
    fn mention_spans_require_username_boundaries() {
        let spans = TelegramChannel::find_bot_mention_spans("hi @Anda_Bot now", "anda_bot");
        assert_eq!(spans, vec![(3, 12)]);
        assert!(TelegramChannel::find_bot_mention_spans("hi x@anda_bot", "anda_bot").is_empty());
    }

    #[test]
    fn normalize_incoming_content_removes_bot_mentions() {
        assert_eq!(
            TelegramChannel::normalize_incoming_content("@anda_bot  hello   world", "anda_bot"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn split_message_for_telegram_chunks_long_text() {
        let text = "a".repeat(TELEGRAM_MAX_MESSAGE_LENGTH + 10);
        let chunks = split_message_for_telegram(&text);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH);
        assert!(chunks[1].chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH);
    }

    #[test]
    fn markdown_to_telegram_html_escapes_links() {
        let rendered = TelegramChannel::markdown_to_telegram_html(
            "**bold** [x](https://example.com?q=\"1\"&v='2')",
        );
        assert_eq!(
            rendered,
            "<b>bold</b> <a href=\"https://example.com?q=&quot;1&quot;&amp;v=&#39;2&#39;\">x</a>"
        );
    }

    #[tokio::test]
    async fn stop_typing_clears_handle() {
        let channel = TelegramChannel::new(&test_config(), new_reqwest_client());
        {
            let mut guard = channel.typing_handle.lock().await;
            *guard = Some(tokio::spawn(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }));
        }

        channel.stop_typing("123").await.unwrap();

        assert!(channel.typing_handle.lock().await.is_none());
    }

    use anda_core::ByteBufB64;
    use axum::{Router, extract::State, routing};
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct MockApi {
        requests: StdMutex<Vec<(String, Value)>>,
        fail_html_send: bool,
        get_updates_calls: StdMutex<u32>,
    }

    impl MockApi {
        fn recorded(&self, method: &str) -> Vec<Value> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, _)| m == method)
                .map(|(_, body)| body.clone())
                .collect()
        }
    }

    async fn handle_method(
        State(state): State<Arc<MockApi>>,
        axum::extract::Path(method): axum::extract::Path<String>,
        body: axum::body::Bytes,
    ) -> axum::Json<Value> {
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        state
            .requests
            .lock()
            .unwrap()
            .push((method.clone(), parsed.clone()));

        let response = match method.as_str() {
            "getMe" => serde_json::json!({
                "ok": true,
                "result": {"username": "anda_bot"},
            }),
            "sendMessage" => {
                if state.fail_html_send && parsed.get("parse_mode").is_some() {
                    serde_json::json!({"ok": false, "description": "can't parse entities"})
                } else {
                    serde_json::json!({"ok": true, "result": {}})
                }
            }
            "getFile" => serde_json::json!({
                "ok": true,
                "result": {"file_path": "photos/file_7.jpg"},
            }),
            "getUpdates" => {
                let calls = {
                    let mut calls = state.get_updates_calls.lock().unwrap();
                    *calls += 1;
                    *calls
                };
                if calls == 1 {
                    serde_json::json!({
                        "ok": true,
                        "result": [{
                            "update_id": 100,
                            "message": {
                                "message_id": 7,
                                "text": "hello bot",
                                "from": {"id": 12345, "username": "Alice"},
                                "chat": {"id": 555, "type": "private"},
                            },
                        }],
                    })
                } else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    serde_json::json!({"ok": true, "result": []})
                }
            }
            _ => serde_json::json!({"ok": true, "result": {}}),
        };
        axum::Json(response)
    }

    async fn spawn_telegram_mock(state: Arc<MockApi>) -> String {
        let app = Router::new()
            .route("/bot123:ABC/{method}", routing::post(handle_method))
            .route("/bot123:ABC/{method}", routing::get(handle_method))
            .route(
                "/file/bot123:ABC/photos/{name}",
                routing::get(|| async { "JPEGDATA" }),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn mock_channel(
        mutate: impl FnOnce(&mut config::TelegramChannelSettings),
        state: Arc<MockApi>,
    ) -> TelegramChannel {
        let mut cfg = test_config();
        mutate(&mut cfg);
        let mut channel = TelegramChannel::new(&cfg, new_reqwest_client());
        channel.api_base = spawn_telegram_mock(state).await;
        channel
    }

    #[tokio::test]
    async fn send_renders_html_and_falls_back_to_plain_text() {
        let state = Arc::new(MockApi {
            fail_html_send: true,
            ..Default::default()
        });
        let channel = mock_channel(|_| {}, state.clone()).await;

        channel
            .send(&SendMessage {
                content: "**bold** text".to_string(),
                recipient: "555:777".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let sends = state.recorded("sendMessage");
        assert_eq!(sends.len(), 2);
        assert_eq!(sends[0]["parse_mode"], "HTML");
        assert_eq!(sends[0]["text"], "<b>bold</b> text");
        assert_eq!(sends[0]["message_thread_id"], "777");
        assert!(sends[1].get("parse_mode").is_none());
        assert_eq!(sends[1]["text"], "**bold** text");
    }

    #[tokio::test]
    async fn send_delivers_attachments_by_url_and_bytes() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        channel
            .send(&SendMessage {
                content: "see attached".to_string(),
                recipient: "555".to_string(),
                attachments: vec![
                    Resource {
                        name: "pic.png".to_string(),
                        tags: vec!["image".to_string()],
                        uri: Some("https://example.com/pic.png".to_string()),
                        ..Default::default()
                    },
                    Resource {
                        name: "notes.txt".to_string(),
                        blob: Some(ByteBufB64(b"file-bytes".to_vec())),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
            .await
            .unwrap();

        let photos = state.recorded("sendPhoto");
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0]["photo"], "https://example.com/pic.png");
        // The blob attachment goes out as multipart (body is not JSON).
        assert_eq!(state.recorded("sendDocument").len(), 1);
    }

    #[tokio::test]
    async fn send_multipart_message_stays_within_telegram_limit() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        // The tail chunk lands in (split_limit, max_len]; before the chunking fix
        // its continuation marker pushed the final message past the 4096 limit.
        let content = format!("{}{}", "a".repeat(4066), "b".repeat(4090));
        channel
            .send(&SendMessage {
                content,
                recipient: "555".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let sends = state.recorded("sendMessage");
        assert!(sends.len() >= 2);
        for body in sends {
            let text = body["text"].as_str().unwrap();
            assert!(
                text.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH,
                "chunk exceeded Telegram limit: {} chars",
                text.chars().count()
            );
        }
    }

    #[tokio::test]
    async fn send_without_content_or_attachments_sends_placeholder() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        channel
            .send(&SendMessage {
                recipient: "555".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(state.recorded("sendMessage").len(), 1);
    }

    #[tokio::test]
    async fn send_resource_reads_local_file_uri_and_rejects_empty() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("report.pdf");
        tokio::fs::write(&file_path, b"pdf-bytes").await.unwrap();

        channel
            .send_resource(
                "555",
                None,
                &Resource {
                    name: "report.pdf".to_string(),
                    uri: Some(format!("file://{}", file_path.display())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(state.recorded("sendDocument").len(), 1);

        let err = channel
            .send_resource("555", None, &Resource::default())
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("has no uri or blob"));
    }

    #[tokio::test]
    async fn bot_username_is_fetched_once_and_cached() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        assert_eq!(
            channel.get_bot_username().await.as_deref(),
            Some("anda_bot")
        );
        assert_eq!(
            channel.get_bot_username().await.as_deref(),
            Some("anda_bot")
        );
        assert_eq!(state.recorded("getMe").len(), 1);
    }

    #[tokio::test]
    async fn health_check_reflects_api_reachability() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state).await;
        assert!(channel.health_check().await);

        let mut dead = TelegramChannel::new(&test_config(), new_reqwest_client());
        dead.api_base = "http://127.0.0.1:1".to_string();
        assert!(!dead.health_check().await);
    }

    #[tokio::test]
    async fn attachment_updates_download_files_into_resources() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(
            |cfg| {
                cfg.mention_only = false;
            },
            state.clone(),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        channel.set_workspace(dir.path().to_path_buf());

        let update = serde_json::json!({
            "message": {
                "message_id": 9,
                "caption": "look at this",
                "photo": [
                    {"file_id": "small", "file_size": 10},
                    {"file_id": "big", "file_size": 100},
                ],
                "from": {"id": 12345, "username": "Alice"},
                "chat": {"id": 555, "type": "private"},
            }
        });

        let message = channel
            .try_parse_attachment_message(&update)
            .await
            .expect("attachment message");

        assert_eq!(message.content, "look at this");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].name, "file_7.jpg");
        // getFile is called with the highest-resolution photo variant.
        assert_eq!(state.recorded("getFile")[0]["file_id"], "big");
    }

    #[tokio::test]
    async fn oversized_attachments_are_skipped() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|cfg| cfg.mention_only = false, state.clone()).await;

        let update = serde_json::json!({
            "message": {
                "message_id": 9,
                "document": {
                    "file_id": "huge",
                    "file_name": "big.bin",
                    "file_size": TELEGRAM_MAX_FILE_DOWNLOAD_BYTES + 1,
                },
                "from": {"id": 12345, "username": "Alice"},
                "chat": {"id": 555, "type": "private"},
            }
        });

        assert!(
            channel
                .try_parse_attachment_message(&update)
                .await
                .is_none()
        );
        assert!(state.recorded("getFile").is_empty());
    }

    #[tokio::test]
    async fn listen_delivers_updates_until_cancelled() {
        let state = Arc::new(MockApi::default());
        let channel = Arc::new(mock_channel(|cfg| cfg.mention_only = false, state.clone()).await);

        let cancel = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(4);
        let listen_channel = channel.clone();
        let listen_cancel = cancel.clone();
        let handle = tokio::spawn(async move { listen_channel.listen(listen_cancel, tx).await });

        let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("listen should deliver a message")
            .expect("channel open");
        assert_eq!(message.sender, "Alice");
        assert_eq!(message.content, "hello bot");
        assert_eq!(message.reply_target, "555");

        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("listen should stop")
            .unwrap()
            .unwrap();

        // The poll acknowledged the update and sent a typing indicator.
        assert!(!state.recorded("sendChatAction").is_empty());
    }

    #[test]
    fn should_retry_send_matches_transient_errors() {
        let channel = TelegramChannel::new(&test_config(), new_reqwest_client());
        assert!(channel.should_retry_send("Connection reset by peer"));
        assert!(channel.should_retry_send("HTTP 429 Too Many Requests"));
        assert!(channel.should_retry_send("upstream 503"));
        assert!(!channel.should_retry_send("400 Bad Request"));
    }

    #[test]
    fn ack_reactions_come_from_known_set() {
        for _ in 0..16 {
            assert!(TELEGRAM_ACK_REACTIONS.contains(&random_telegram_ack_reaction()));
        }
    }

    #[tokio::test]
    async fn start_typing_spawns_keepalive_loop() {
        let state = Arc::new(MockApi::default());
        let channel = mock_channel(|_| {}, state.clone()).await;

        channel.start_typing("555:777").await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        channel.stop_typing("555:777").await.unwrap();

        let actions = state.recorded("sendChatAction");
        assert!(!actions.is_empty());
        assert_eq!(actions[0]["message_thread_id"], "777");
    }

    #[test]
    fn format_forward_attribution_covers_channel_user_and_hidden() {
        use serde_json::json;
        let from_channel = json!({"forward_from_chat": {"title": "News"}});
        assert_eq!(
            TelegramChannel::format_forward_attribution(&from_channel).as_deref(),
            Some("[Forwarded from channel: News] ")
        );

        let from_user = json!({"forward_from": {"username": "bob"}});
        assert_eq!(
            TelegramChannel::format_forward_attribution(&from_user).as_deref(),
            Some("[Forwarded from @bob] ")
        );

        let from_first_name = json!({"forward_from": {"first_name": "Alice"}});
        assert_eq!(
            TelegramChannel::format_forward_attribution(&from_first_name).as_deref(),
            Some("[Forwarded from Alice] ")
        );

        let hidden = json!({"forward_sender_name": "Hidden"});
        assert_eq!(
            TelegramChannel::format_forward_attribution(&hidden).as_deref(),
            Some("[Forwarded from Hidden] ")
        );

        assert!(TelegramChannel::format_forward_attribution(&json!({})).is_none());
    }

    #[test]
    fn extract_reply_context_quotes_text_and_media() {
        use serde_json::json;
        let text_reply = json!({
            "reply_to_message": {"from": {"username": "carol"}, "text": "line one\nline two"}
        });
        let quoted = TelegramChannel::extract_reply_context(&text_reply).unwrap();
        assert!(quoted.contains("> @carol:"));
        assert!(quoted.contains("> line one"));
        assert!(quoted.contains("> line two"));

        for (field, label) in [
            ("photo", "[Photo]"),
            ("document", "[Document]"),
            ("voice", "[Voice]"),
            ("video", "[Video]"),
        ] {
            let reply = json!({"reply_to_message": {"from": {"first_name": "Dave"}, field: {}}});
            assert!(
                TelegramChannel::extract_reply_context(&reply)
                    .unwrap()
                    .contains(label)
            );
        }

        assert!(TelegramChannel::extract_reply_context(&json!({})).is_none());
    }

    #[test]
    fn parse_attachment_metadata_handles_each_media_kind() {
        use serde_json::json;
        let photo = json!({
            "photo": [{"file_id": "small"}, {"file_id": "big", "file_size": 100}],
            "caption": "a pic"
        });
        let meta = TelegramChannel::parse_attachment_metadata(&photo).unwrap();
        assert_eq!(meta.file_id, "big");
        assert_eq!(meta.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(meta.caption.as_deref(), Some("a pic"));

        let voice = json!({"voice": {"file_id": "v1", "mime_type": "audio/ogg", "file_size": 5}});
        let meta = TelegramChannel::parse_attachment_metadata(&voice).unwrap();
        assert_eq!(meta.file_id, "v1");
        assert_eq!(meta.mime_type.as_deref(), Some("audio/ogg"));

        assert!(TelegramChannel::parse_attachment_metadata(&json!({"text": "x"})).is_none());
    }

    #[test]
    fn markdown_to_telegram_html_renders_common_markup() {
        let html = TelegramChannel::markdown_to_telegram_html(
            "# Title\n\n**bold** and *italic* and `code`\n\n- item\n\n[link](https://x.com)",
        );
        assert!(html.contains("<b>") || html.contains("bold"));
        assert!(!html.is_empty());
    }
}
