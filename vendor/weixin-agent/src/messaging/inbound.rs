//! Inbound message parsing and context token management.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;

use crate::error::Result;
use crate::types::{
    CdnMedia, MediaType, MessageItem, MessageItemType, MessageState, MessageType,
    SendTypingRequest, TypingStatus, WeixinMessage,
};
use crate::util::random::generate_id;

/// High-level media information extracted from an inbound message.
#[derive(Debug, Clone)]
pub struct MediaInfo {
    /// Media type category.
    pub media_type: MediaType,
    /// CDN media reference (for download).
    pub cdn_media: Option<CdnMedia>,
    /// Direct URL (if available).
    pub url: Option<String>,
    /// Original file name.
    pub file_name: Option<String>,
    /// Plaintext file size.
    pub file_size: Option<u64>,
    /// AES key for decryption (base64).
    pub aes_key_base64: Option<String>,
}

/// Quoted (referenced) message info.
#[derive(Debug, Clone)]
pub struct RefMessageInfo {
    /// Summary title.
    pub title: Option<String>,
    /// Body text of the quoted message.
    pub body: Option<String>,
}

/// Result of sending a message.
#[derive(Debug, Clone)]
pub struct SendResult {
    /// Client-generated message ID.
    pub message_id: String,
}

/// Internal sender handle for replying from a [`MessageContext`].
pub struct MessageSender {
    pub(crate) api: Arc<crate::api::client::HttpApiClient>,
    pub(crate) cdn_base_url: String,
    pub(crate) config_cache: Arc<crate::api::config_cache::ConfigCache>,
    pub(crate) markdown_filter_enabled: bool,
}

/// Inbound message context passed to the handler.
pub struct MessageContext {
    /// SDK-generated message session ID.
    pub message_id: String,
    /// Server-assigned message ID.
    pub server_message_id: Option<i64>,
    /// Sender user ID.
    pub from: String,
    /// Recipient user ID.
    pub to: String,
    /// Creation timestamp (ms).
    pub timestamp: i64,
    /// Session ID.
    pub session_id: Option<String>,
    /// Context token for replies.
    pub context_token: Option<String>,
    /// Text body (including quoted text).
    pub body: Option<String>,
    /// Media attachment info.
    pub media: Option<MediaInfo>,
    /// Referenced (quoted) message info.
    pub ref_message: Option<RefMessageInfo>,
    /// Internal sender.
    pub(crate) sender: Arc<MessageSender>,
}

impl MessageContext {
    /// Reply with a text message.
    pub async fn reply_text(&self, text: &str) -> Result<SendResult> {
        crate::messaging::send::send_text(
            &self.sender.api,
            &self.from,
            text,
            self.context_token.as_deref(),
            self.sender.markdown_filter_enabled,
            self.sender.api.base_info(),
        )
        .await
    }

    /// Reply with a media file.
    pub async fn reply_media(&self, file_path: &Path) -> Result<SendResult> {
        crate::messaging::send_media::send_media_file(
            &self.sender.api,
            &self.sender.cdn_base_url,
            &self.from,
            file_path,
            "",
            self.context_token.as_deref(),
            self.sender.api.base_info(),
        )
        .await
    }

    /// Download media from this message to a destination path.
    pub async fn download_media(&self, media: &MediaInfo, dest: &Path) -> Result<PathBuf> {
        let data = if let Some(aes_key) = &media.aes_key_base64 {
            let cdn_media = media
                .cdn_media
                .as_ref()
                .ok_or_else(|| crate::error::Error::CdnUpload("no cdn_media".into()))?;
            crate::cdn::download::download_and_decrypt(
                &self.sender.cdn_base_url,
                cdn_media,
                aes_key,
            )
            .await?
        } else if let Some(cdn_media) = &media.cdn_media {
            crate::cdn::download::download_plain(&self.sender.cdn_base_url, cdn_media).await?
        } else {
            return Err(crate::error::Error::CdnUpload(
                "no media source available".into(),
            ));
        };
        tokio::fs::write(dest, &data).await?;
        Ok(dest.to_path_buf())
    }

    /// Send a typing indicator.
    pub async fn send_typing(&self) -> Result<()> {
        let ticket = self
            .sender
            .config_cache
            .get_typing_ticket(&self.from, self.context_token.as_deref())
            .await;
        let req = SendTypingRequest {
            ilink_user_id: self.from.clone(),
            typing_ticket: ticket,
            status: TypingStatus::Typing,
            base_info: self.sender.api.base_info(),
        };
        self.sender.api.send_typing(&req).await
    }

    /// Cancel the typing indicator.
    pub async fn cancel_typing(&self) -> Result<()> {
        let ticket = self
            .sender
            .config_cache
            .get_typing_ticket(&self.from, self.context_token.as_deref())
            .await;
        let req = SendTypingRequest {
            ilink_user_id: self.from.clone(),
            typing_ticket: ticket,
            status: TypingStatus::Cancel,
            base_info: self.sender.api.base_info(),
        };
        self.sender.api.send_typing(&req).await
    }
}

// ── Context token store ─────────────────────────────────────────────

/// In-memory context token store with export/import for persistence.
#[derive(Default)]
pub struct ContextTokenStore {
    tokens: DashMap<String, String>,
}

impl ContextTokenStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a context token for a user.
    pub fn set(&self, user_id: &str, token: &str) {
        self.tokens.insert(user_id.to_owned(), token.to_owned());
    }

    /// Get the context token for a user.
    pub fn get(&self, user_id: &str) -> Option<String> {
        self.tokens.get(user_id).map(|v| v.value().clone())
    }

    /// Export all tokens for caller persistence.
    pub fn export_all(&self) -> HashMap<String, String> {
        self.tokens
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect()
    }

    /// Import tokens (e.g. on startup restore).
    pub fn import(&self, tokens: HashMap<String, String>) {
        for (k, v) in tokens {
            self.tokens.insert(k, v);
        }
    }
}

// ── Message parsing ─────────────────────────────────────────────────

/// Returns `true` if the message item is a media type.
fn is_media_item(item: &MessageItem) -> bool {
    matches!(
        item.item_type,
        Some(
            MessageItemType::Image
                | MessageItemType::Video
                | MessageItemType::File
                | MessageItemType::Voice
        )
    )
}

/// Extract text body from item list (with quoted message handling).
fn body_from_item_list(items: &[MessageItem]) -> String {
    for item in items {
        if item.item_type == Some(MessageItemType::Text) {
            if let Some(text) = item.text_item.as_ref().and_then(|t| t.text.as_deref()) {
                let text = text.to_owned();
                if let Some(ref_msg) = &item.ref_msg {
                    if let Some(ref_item) = &ref_msg.message_item {
                        if is_media_item(ref_item) {
                            return text;
                        }
                    }
                    let mut parts = Vec::new();
                    if let Some(title) = &ref_msg.title {
                        parts.push(title.clone());
                    }
                    if let Some(ref_item) = &ref_msg.message_item {
                        let ref_body = body_from_item_list(&[*ref_item.clone()]);
                        if !ref_body.is_empty() {
                            parts.push(ref_body);
                        }
                    }
                    if parts.is_empty() {
                        return text;
                    }
                    return format!("[引用: {}]\n{text}", parts.join(" | "));
                }
                return text;
            }
        }
        // Voice-to-text
        if item.item_type == Some(MessageItemType::Voice) {
            if let Some(voice_text) = item.voice_item.as_ref().and_then(|v| v.text.as_deref()) {
                return voice_text.to_owned();
            }
        }
    }
    String::new()
}

/// Extract media info from item list. Priority: image > video > file > voice.
fn extract_media(items: &[MessageItem]) -> Option<MediaInfo> {
    // First pass: direct items
    for item in items {
        if let Some(info) = extract_media_from_item(item) {
            return Some(info);
        }
    }
    // Second pass: check ref_msg for media fallback
    for item in items {
        if let Some(ref_msg) = &item.ref_msg {
            if let Some(ref_item) = &ref_msg.message_item {
                if let Some(info) = extract_media_from_item(ref_item) {
                    return Some(info);
                }
            }
        }
    }
    None
}

fn extract_media_from_item(item: &MessageItem) -> Option<MediaInfo> {
    match item.item_type? {
        MessageItemType::Image => {
            let img = item.image_item.as_ref()?;
            let aes_key = if let Some(hex_key) = &img.aeskey {
                // Image: aeskey is hex string → decode to raw bytes → base64
                use base64::Engine;
                let bytes = crate::cdn::aes_ecb::hex_to_bytes(hex_key).ok()?;
                if bytes.len() == 16 {
                    Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
                } else {
                    None
                }
            } else {
                img.media.as_ref().and_then(|m| m.aes_key.clone())
            };
            Some(MediaInfo {
                media_type: MediaType::Image,
                cdn_media: img.media.clone(),
                url: img.url.clone(),
                file_name: None,
                file_size: None,
                aes_key_base64: aes_key,
            })
        }
        MessageItemType::Video => {
            let vid = item.video_item.as_ref()?;
            Some(MediaInfo {
                media_type: MediaType::Video,
                cdn_media: vid.media.clone(),
                url: None,
                file_name: None,
                file_size: vid.video_size.and_then(|s| u64::try_from(s).ok()),
                aes_key_base64: vid.media.as_ref().and_then(|m| m.aes_key.clone()),
            })
        }
        MessageItemType::File => {
            let f = item.file_item.as_ref()?;
            Some(MediaInfo {
                media_type: MediaType::File,
                cdn_media: f.media.clone(),
                url: None,
                file_name: f.file_name.clone(),
                file_size: f.len.as_deref().and_then(|s| s.parse().ok()),
                aes_key_base64: f.media.as_ref().and_then(|m| m.aes_key.clone()),
            })
        }
        MessageItemType::Voice => {
            let v = item.voice_item.as_ref()?;
            // Skip media download if voice has text transcription
            if v.text.is_some() {
                return None;
            }
            Some(MediaInfo {
                media_type: MediaType::Voice,
                cdn_media: v.media.clone(),
                url: None,
                file_name: None,
                file_size: None,
                aes_key_base64: v.media.as_ref().and_then(|m| m.aes_key.clone()),
            })
        }
        _ => None,
    }
}

/// Extract ref message info.
fn extract_ref_message(items: &[MessageItem]) -> Option<RefMessageInfo> {
    for item in items {
        if let Some(ref_msg) = &item.ref_msg {
            return Some(RefMessageInfo {
                title: ref_msg.title.clone(),
                body: ref_msg
                    .message_item
                    .as_ref()
                    .map(|ri| body_from_item_list(&[*ri.clone()])),
            });
        }
    }
    None
}

/// Returns `true` if this message should be processed by the handler.
pub fn should_process(msg: &WeixinMessage) -> bool {
    // Only USER messages
    if msg.message_type != Some(MessageType::User) {
        return false;
    }
    // Skip recalled messages
    if msg.delete_time_ms.unwrap_or(0) > 0 {
        return false;
    }
    // Skip GENERATING state
    if msg.message_state == Some(MessageState::Generating) {
        return false;
    }
    true
}

/// Parse a raw `WeixinMessage` into a `MessageContext`.
pub fn parse_inbound_message(msg: &WeixinMessage, sender: Arc<MessageSender>) -> MessageContext {
    let items = msg.item_list.as_deref().unwrap_or(&[]);
    let body = body_from_item_list(items);

    MessageContext {
        message_id: generate_id("weixin-agent"),
        server_message_id: msg.message_id,
        from: msg.from_user_id.clone().unwrap_or_default(),
        to: msg.to_user_id.clone().unwrap_or_default(),
        timestamp: msg.create_time_ms.unwrap_or(0),
        session_id: msg.session_id.clone(),
        context_token: msg.context_token.clone(),
        body: if body.is_empty() { None } else { Some(body) },
        media: extract_media(items),
        ref_message: extract_ref_message(items),
        sender,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_msg(msg_type: MessageType) -> WeixinMessage {
        WeixinMessage {
            message_type: Some(msg_type),
            ..Default::default()
        }
    }

    #[test]
    fn should_process_user_message() {
        assert!(should_process(&make_msg(MessageType::User)));
    }

    #[test]
    fn should_process_rejects_bot() {
        assert!(!should_process(&make_msg(MessageType::Bot)));
    }

    #[test]
    fn should_process_rejects_deleted() {
        let msg = WeixinMessage {
            message_type: Some(MessageType::User),
            delete_time_ms: Some(1000),
            ..Default::default()
        };
        assert!(!should_process(&msg));
    }

    #[test]
    fn should_process_rejects_generating() {
        let msg = WeixinMessage {
            message_type: Some(MessageType::User),
            message_state: Some(MessageState::Generating),
            ..Default::default()
        };
        assert!(!should_process(&msg));
    }

    #[test]
    fn body_from_text_item() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::Text),
            text_item: Some(TextItem {
                text: Some("hello".into()),
            }),
            ..Default::default()
        }];
        assert_eq!(body_from_item_list(&items), "hello");
    }

    #[test]
    fn body_from_voice_item() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::Voice),
            voice_item: Some(VoiceItem {
                text: Some("voice text".into()),
                ..Default::default()
            }),
            ..Default::default()
        }];
        assert_eq!(body_from_item_list(&items), "voice text");
    }

    #[test]
    fn body_from_ref_message() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::Text),
            text_item: Some(TextItem {
                text: Some("reply".into()),
            }),
            ref_msg: Some(RefMessage {
                title: Some("quoted title".into()),
                message_item: Some(Box::new(MessageItem {
                    item_type: Some(MessageItemType::Text),
                    text_item: Some(TextItem {
                        text: Some("original".into()),
                    }),
                    ..Default::default()
                })),
            }),
            ..Default::default()
        }];
        let body = body_from_item_list(&items);
        assert!(body.contains("引用"));
        assert!(body.contains("reply"));
    }

    #[test]
    fn extract_media_image() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::Image),
            image_item: Some(ImageItem {
                url: Some("https://img.example.com/1.jpg".into()),
                aeskey: Some("0123456789abcdef0123456789abcdef".into()),
                ..Default::default()
            }),
            ..Default::default()
        }];
        let media = extract_media(&items).unwrap();
        assert_eq!(media.media_type, MediaType::Image);
        assert!(media.aes_key_base64.is_some());
    }

    #[test]
    fn extract_media_video() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::Video),
            video_item: Some(VideoItem {
                video_size: Some(1024),
                ..Default::default()
            }),
            ..Default::default()
        }];
        let media = extract_media(&items).unwrap();
        assert_eq!(media.media_type, MediaType::Video);
        assert_eq!(media.file_size, Some(1024));
    }

    #[test]
    fn extract_media_file() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::File),
            file_item: Some(FileItem {
                file_name: Some("doc.pdf".into()),
                len: Some("2048".into()),
                ..Default::default()
            }),
            ..Default::default()
        }];
        let media = extract_media(&items).unwrap();
        assert_eq!(media.media_type, MediaType::File);
        assert_eq!(media.file_name.as_deref(), Some("doc.pdf"));
        assert_eq!(media.file_size, Some(2048));
    }

    #[test]
    fn extract_media_voice_with_text_returns_none() {
        let items = vec![MessageItem {
            item_type: Some(MessageItemType::Voice),
            voice_item: Some(VoiceItem {
                text: Some("transcribed".into()),
                ..Default::default()
            }),
            ..Default::default()
        }];
        assert!(extract_media(&items).is_none());
    }

    #[test]
    fn context_token_store_set_get() {
        let store = ContextTokenStore::new();
        store.set("user1", "token_a");
        assert_eq!(store.get("user1"), Some("token_a".into()));
        assert_eq!(store.get("user2"), None);
    }

    #[test]
    fn context_token_store_export_import() {
        let store = ContextTokenStore::new();
        store.set("u1", "t1");
        store.set("u2", "t2");
        let exported = store.export_all();
        assert_eq!(exported.len(), 2);

        let store2 = ContextTokenStore::new();
        store2.import(exported);
        assert_eq!(store2.get("u1"), Some("t1".into()));
        assert_eq!(store2.get("u2"), Some("t2".into()));
    }
}
