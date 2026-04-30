use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use reqwest::{
    Client,
    multipart::{Form, Part},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use super::{
    Channel, ChannelMessage, ChannelWorkspace, SendMessage, file_name_for_resource, is_http_url,
    resource_from_bytes,
};
use crate::config;

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
            allowed_users: cfg.allowed_users.clone(),
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
        let response = self.client.get(self.api_url("getMe")).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Telegram getMe failed ({status}): {body}").into());
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
                log::warn!("Telegram failed to fetch bot username: {err}");
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
        if !self.is_any_user_allowed(identities.iter().copied()) {
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

        self.channel_message_from_parts(message, sender_identity, content, Vec::new())
    }

    fn channel_message_from_parts(
        &self,
        message: &Value,
        sender: String,
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
        if !self.is_any_user_allowed(identities.iter().copied()) {
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
                log::warn!("Telegram failed to get attachment file path: {err}");
                return None;
            }
        };
        let bytes = match self.download_file(&telegram_path).await {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!("Telegram failed to download attachment: {err}");
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

        self.channel_message_from_parts(message, sender_identity, content, vec![resource])
    }

    async fn get_file_path(&self, file_id: &str) -> Result<String, BoxError> {
        let response = self
            .client
            .post(self.api_url("getFile"))
            .json(&serde_json::json!({ "file_id": file_id }))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Telegram getFile failed ({status}): {body}").into());
        }

        let data: Value = serde_json::from_str(&body)?;
        data.get("result")
            .and_then(|result| result.get("file_path"))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| "Telegram getFile response did not include result.file_path".into())
    }

    async fn download_file(&self, file_path: &str) -> Result<Vec<u8>, BoxError> {
        let response = self.client.get(self.file_url(file_path)).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Telegram file download failed ({status}): {body}").into());
        }

        Ok(response.bytes().await?.to_vec())
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
                    log::debug!("Telegram failed to add ACK reaction: {err}");
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
            .client
            .post(self.api_url("sendChatAction"))
            .json(&body)
            .send()
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
            .client
            .post(self.api_url(method))
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("{status}: {text}").into());
        }

        if let Ok(data) = serde_json::from_str::<Value>(&text)
            && !data.get("ok").and_then(Value::as_bool).unwrap_or(true)
        {
            return Err(data
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("Telegram API returned ok=false")
                .to_string()
                .into());
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

            let path = uri.strip_prefix("file://").unwrap_or(uri);
            if Path::new(path).exists() {
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
            .client
            .post(self.api_url(method))
            .multipart(form)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Telegram {method} failed ({status}): {body}").into());
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
                response = self.client.post(self.api_url("getUpdates")).json(&body).send() => response,
            };

            let response = match response {
                Ok(response) => response,
                Err(err) => {
                    log::warn!("Telegram poll error: {err}");
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
                    log::warn!("Telegram poll parse error ({status}): {err}");
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
            tokio::time::timeout(Duration::from_secs(5), self.client.get(self.api_url("getMe")).send()).await,
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

fn normalize_identity(value: &str) -> String {
    value.trim().trim_start_matches('@').to_string()
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
    if message.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;
    let chunk_limit = TELEGRAM_MAX_MESSAGE_LENGTH - TELEGRAM_CONTINUATION_OVERHEAD;

    while !remaining.is_empty() {
        if remaining.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH {
            chunks.push(remaining.to_string());
            break;
        }

        let hard_split = remaining
            .char_indices()
            .nth(chunk_limit)
            .map_or(remaining.len(), |(idx, _)| idx);
        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            if let Some(pos) = search_area.rfind('\n') {
                if search_area[..pos].chars().count() >= chunk_limit / 2 {
                    pos + 1
                } else {
                    search_area.rfind(' ').map_or(hard_split, |pos| pos + 1)
                }
            } else {
                search_area.rfind(' ').map_or(hard_split, |pos| pos + 1)
            }
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    chunks
}

fn telegram_method_and_field(resource: &Resource) -> (&'static str, &'static str) {
    let mime_type = resource.mime_type.as_deref().unwrap_or_default();
    if resource.tags.iter().any(|tag| tag == "image") || mime_type.starts_with("image/") {
        ("sendPhoto", "photo")
    } else if resource.tags.iter().any(|tag| tag == "video") || mime_type.starts_with("video/") {
        ("sendVideo", "video")
    } else if resource.tags.iter().any(|tag| tag == "audio") || mime_type.starts_with("audio/") {
        ("sendAudio", "audio")
    } else {
        ("sendDocument", "document")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> config::TelegramChannelSettings {
        config::TelegramChannelSettings {
            id: Some("test".to_string()),
            bot_token: "123:ABC".to_string(),
            username: Some("anda_bot".to_string()),
            allowed_users: vec!["@Alice".to_string(), "12345".to_string()],
            mention_only: true,
            ack_reactions: true,
        }
    }

    #[test]
    fn telegram_channel_identity() {
        let channel = TelegramChannel::new(&test_config(), Client::new());
        assert_eq!(channel.name(), "telegram");
        assert_eq!(channel.username(), "anda_bot");
        assert_eq!(channel.id(), "telegram:test");
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
        let channel = TelegramChannel::new(&test_config(), Client::new());
        assert!(channel.is_user_allowed("alice"));
        assert!(channel.is_user_allowed("Alice"));
        assert!(channel.is_user_allowed("12345"));
        assert!(!channel.is_user_allowed("bob"));
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
        let channel = TelegramChannel::new(&test_config(), Client::new());
        {
            let mut guard = channel.typing_handle.lock().await;
            *guard = Some(tokio::spawn(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }));
        }

        channel.stop_typing("123").await.unwrap();

        assert!(channel.typing_handle.lock().await.is_none());
    }
}
