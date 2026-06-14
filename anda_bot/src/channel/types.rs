use anda_core::{BoxError, Json, Resource};
use anda_db::schema::{
    AndaDBSchema, FieldEntry, FieldKey, FieldType, FieldTyped, Schema, SchemaError,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use tokio_util::sync::CancellationToken;

/// Message to send through a channel
#[derive(Debug, Clone, Default, Deserialize, Serialize, FieldTyped, AndaDBSchema)]
pub struct SendMessage {
    pub content: String,
    pub recipient: String,
    pub subject: Option<String>,
    /// Platform thread identifier for threaded replies (e.g. Slack `thread`).
    pub thread: Option<String>,
    /// File attachments to send with the message.
    /// Channels that don't support attachments ignore this field.
    pub attachments: Vec<Resource>,
}

/// A message received from or sent to a channel
/// version 2: adds `external_user`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, FieldTyped, AndaDBSchema)]
pub struct ChannelMessage {
    pub _id: u64,
    pub sender: String,

    /// True when the sender is accepted as an external untrusted IM user.
    pub external_user: Option<bool>,

    pub reply_target: String,
    pub content: String,
    pub channel: String,
    pub timestamp: u64, // Unix timestamp in milliseconds
    /// Platform thread identifier (e.g. Slack `ts`, Discord thread ID).
    /// When set, replies should be posted as threaded responses.
    pub thread: Option<String>,
    /// Media attachments (audio, images, video) for the media pipeline.
    /// Channels populate this when they receive media alongside a text message.
    /// Defaults to empty — existing channels are unaffected.
    pub attachments: Vec<Resource>,

    /// Extra platform-specific metadata for this message.
    pub extra: BTreeMap<String, Json>,

    // populated when the message is associated with an engine conversation
    pub conversation: Option<u64>,
}

impl SendMessage {
    /// Create a new message with content and recipient
    pub fn new(content: impl Into<String>, recipient: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            recipient: recipient.into(),
            subject: None,
            thread: None,
            attachments: vec![],
        }
    }

    /// Create a new message with content, recipient, and subject
    #[allow(unused)]
    pub fn with_subject(
        content: impl Into<String>,
        recipient: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            content: content.into(),
            recipient: recipient.into(),
            subject: Some(subject.into()),
            thread: None,
            attachments: vec![],
        }
    }

    /// Set the thread identifier for threaded replies.
    pub fn in_thread(mut self, thread: Option<String>) -> Self {
        self.thread = thread;
        self
    }

    /// Attach files to this message.
    pub fn with_attachments(mut self, attachments: Vec<Resource>) -> Self {
        self.attachments = attachments;
        self
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelInitOptions {
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ChannelInitResult {
    pub changed: bool,
    pub message: String,
}

impl ChannelInitResult {
    pub fn changed(message: impl Into<String>) -> Self {
        Self {
            changed: true,
            message: message.into(),
        }
    }

    pub fn unchanged(message: impl Into<String>) -> Self {
        Self {
            changed: false,
            message: message.into(),
        }
    }
}

/// Splits `message` into chunks, preferring to break on newline and then
/// whitespace boundaries.
///
/// A message that already fits within `max_len` is returned as a single chunk
/// verbatim. Once a message has to be split, **every** chunk (including the
/// final one) is kept within `split_limit` so callers always have room to
/// append continuation markers without exceeding `max_len`. Callers must pass
/// `split_limit <= max_len`.
///
/// A newline break is only taken when it falls in the second half of the chunk;
/// otherwise the last space is used, falling back to a hard character split when
/// neither boundary is available.
pub(crate) fn split_message_on_word_boundaries(
    message: &str,
    max_len: usize,
    split_limit: usize,
) -> Vec<String> {
    if message.chars().count() <= max_len {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        // Once we are splitting, cap every chunk at `split_limit` (not `max_len`)
        // so the tail chunk still leaves room for continuation markers.
        if remaining.chars().count() <= split_limit {
            chunks.push(remaining.to_string());
            break;
        }

        let hard_split = remaining
            .char_indices()
            .nth(split_limit)
            .map_or(remaining.len(), |(idx, _)| idx);
        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            match search_area.rfind('\n') {
                Some(pos) if search_area[..pos].chars().count() >= split_limit / 2 => pos + 1,
                _ => search_area.rfind(' ').map_or(hard_split, |pos| pos + 1),
            }
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    chunks
}

/// Returns whether a send error string looks like a transient transport or
/// rate-limit failure that is worth retrying. Channels share this baseline and
/// may layer additional, platform-specific checks on top.
pub(crate) fn is_transient_send_error(error: &str) -> bool {
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

/// Returns the filesystem directory name for a channel workspace.
///
/// Channel ids are stable metadata and routing keys, so they may contain
/// separators like `:`. Use one safe layout on every platform so macOS Finder
/// does not display `:` as `/`, and channel workspaces remain portable.
pub fn channel_workspace_dir_name(channel_id: &str) -> String {
    windows_safe_path_component(channel_id)
}

fn windows_safe_path_component(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "channel".to_string();
    }

    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        if is_windows_safe_path_char(ch) {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }

    while matches!(sanitized.as_bytes().last(), Some(b'.' | b' ')) {
        sanitized.pop();
        sanitized.push('_');
    }

    if is_reserved_windows_name(&sanitized) {
        sanitized.insert(0, '_');
    }

    sanitized
}

pub(crate) fn legacy_percent_encoded_channel_workspace_dir_name(channel_id: &str) -> String {
    let value = channel_id.trim();
    if value.is_empty() {
        return "channel".to_string();
    }

    let mut encoded = String::with_capacity(value.len());
    for ch in value.chars() {
        if is_legacy_percent_encoded_safe_path_char(ch) {
            encoded.push(ch);
        } else {
            push_legacy_percent_encoded_char(&mut encoded, ch);
        }
    }

    while matches!(encoded.as_bytes().last(), Some(b'.' | b' ')) {
        let ch = encoded.pop().expect("path component is not empty");
        push_legacy_percent_encoded_char(&mut encoded, ch);
    }

    if is_reserved_windows_name(&encoded) {
        encoded.insert(0, '_');
    }

    encoded
}

fn is_windows_safe_path_char(ch: char) -> bool {
    !ch.is_control() && !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

fn is_legacy_percent_encoded_safe_path_char(ch: char) -> bool {
    !ch.is_control()
        && !matches!(
            ch,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '%'
        )
}

fn push_legacy_percent_encoded_char(output: &mut String, ch: char) {
    let mut buf = [0_u8; 4];
    for byte in ch.encode_utf8(&mut buf).as_bytes() {
        push_legacy_percent_encoded_byte(output, *byte);
    }
}

fn push_legacy_percent_encoded_byte(output: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    output.push('%');
    output.push(HEX[(byte >> 4) as usize] as char);
    output.push(HEX[(byte & 0x0F) as usize] as char);
}

fn is_reserved_windows_name(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Core channel trait — implement for any messaging platform
#[async_trait]
pub trait Channel: Send + Sync {
    /// Human-readable channel name
    fn name(&self) -> &str;

    fn username(&self) -> &str;

    /// Unique channel identifier for message metadata (e.g. "wechat:personal").
    fn id(&self) -> String;

    /// Set the channel-specific workspace directory managed by ChannelRuntime.
    fn set_workspace(&self, _workspace: PathBuf) {}

    /// Run channel-specific direct initialization from `anda channel init`.
    async fn init(&self, _options: ChannelInitOptions) -> Result<ChannelInitResult, BoxError> {
        Ok(ChannelInitResult::unchanged(format!(
            "{} does not require CLI initialization",
            self.id()
        )))
    }

    /// Send a message through this channel
    async fn send(&self, message: &SendMessage) -> Result<(), BoxError>;

    /// Start listening for incoming messages (long-running)
    async fn listen(
        &self,
        cancel_token: CancellationToken,
        tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError>;

    /// Check if channel is healthy
    async fn health_check(&self) -> bool {
        true
    }

    /// Whether a send error is transient and worth retrying in the runtime.
    /// Implementations can use this to surface reconnect windows or platform-
    /// specific transport failures without forcing protocol logic into runtime.
    fn should_retry_send(&self, _error: &str) -> bool {
        false
    }

    /// Signal that the bot is processing a response (e.g. "typing" indicator).
    /// Implementations should repeat the indicator as needed for their platform.
    #[allow(unused)]
    async fn start_typing(&self, _recipient: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Stop any active typing indicator.
    #[allow(unused)]
    async fn stop_typing(&self, _recipient: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Whether this channel supports progressive message updates via draft edits.
    #[allow(unused)]
    fn supports_draft_updates(&self) -> bool {
        false
    }

    /// Whether this channel supports multi-message streaming delivery, where
    /// the response is sent as multiple separate messages at paragraph
    /// boundaries as tokens arrive from the provider.
    #[allow(unused)]
    fn supports_multi_message_streaming(&self) -> bool {
        false
    }

    /// Minimum delay (ms) between sending each paragraph in multi-message mode.
    /// Channels should override this to avoid platform rate limits.
    #[allow(unused)]
    fn multi_message_delay_ms(&self) -> u64 {
        800
    }

    /// Send an initial draft message. Returns a platform-specific message ID for later edits.
    #[allow(unused)]
    async fn send_draft(&self, _message: &SendMessage) -> Result<Option<String>, BoxError> {
        Ok(None)
    }

    /// Update a previously sent draft message with new accumulated content.
    #[allow(unused)]
    async fn update_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Show a progress/status update (e.g. tool execution status).
    /// Channels can display this in a status bar rather than in the message body.
    /// Default: no-op (progress is ignored).
    #[allow(unused)]
    async fn update_draft_progress(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Finalize a draft with the complete response (e.g. apply Markdown formatting).
    #[allow(unused)]
    async fn finalize_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Cancel and remove a previously sent draft message if the channel supports it.
    #[allow(unused)]
    async fn cancel_draft(&self, _recipient: &str, _message_id: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Add a reaction (emoji) to a message.
    ///
    /// `channel_id` is the platform channel/conversation identifier (e.g. Discord channel ID).
    /// `message_id` is the platform-scoped message identifier (e.g. `discord_<snowflake>`).
    /// `emoji` is the Unicode emoji to react with (e.g. "👀", "✅").
    #[allow(unused)]
    async fn add_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Remove a reaction (emoji) from a message previously added by this bot.
    #[allow(unused)]
    async fn remove_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Pin a message in the channel.
    #[allow(unused)]
    async fn pin_message(&self, _channel_id: &str, _message_id: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Unpin a previously pinned message.
    #[allow(unused)]
    async fn unpin_message(&self, _channel_id: &str, _message_id: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Redact (delete) a message from the channel.
    ///
    /// `channel_id` is the platform channel/conversation identifier.
    /// `message_id` is the platform-scoped message identifier.
    /// `reason` is an optional reason for the redaction (may be visible in audit logs).
    #[allow(unused)]
    async fn redact_message(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _reason: Option<String>,
    ) -> Result<(), BoxError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_message_new_sets_required_fields_only() {
        let message = SendMessage::new("hello", "alice");

        assert_eq!(message.content, "hello");
        assert_eq!(message.recipient, "alice");
        assert_eq!(message.subject, None);
        assert_eq!(message.thread, None);
        assert!(message.attachments.is_empty());
    }

    #[test]
    fn send_message_builders_preserve_subject_thread_and_attachments() {
        let attachment = Resource {
            name: "voice.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            ..Default::default()
        };
        let message = SendMessage::with_subject("report", "ops", "daily")
            .in_thread(Some("thread-42".to_string()))
            .with_attachments(vec![attachment]);

        assert_eq!(message.content, "report");
        assert_eq!(message.recipient, "ops");
        assert_eq!(message.subject.as_deref(), Some("daily"));
        assert_eq!(message.thread.as_deref(), Some("thread-42"));
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].name, "voice.mp3");
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn channel_init_result_constructors_encode_changed_state() {
        let changed = ChannelInitResult::changed("created config");
        assert!(changed.changed);
        assert_eq!(changed.message, "created config");

        let unchanged = ChannelInitResult::unchanged("already configured");
        assert!(!unchanged.changed);
        assert_eq!(unchanged.message, "already configured");
    }

    #[test]
    fn workspace_dir_name_uses_safe_layout_on_all_platforms() {
        assert_eq!(
            channel_workspace_dir_name("wechat:personal"),
            "wechat_personal"
        );
    }

    #[test]
    fn windows_workspace_dir_name_replaces_invalid_path_characters() {
        assert_eq!(
            windows_safe_path_component("wechat:personal"),
            "wechat_personal"
        );
        assert_eq!(
            windows_safe_path_component("telegram:ops/chat?prod*"),
            "telegram_ops_chat_prod_"
        );
        assert_eq!(windows_safe_path_component("discord%prod"), "discord%prod");
    }

    #[test]
    fn windows_workspace_dir_name_handles_reserved_and_trailing_names() {
        assert_eq!(windows_safe_path_component("con"), "_con");
        assert_eq!(windows_safe_path_component("LPT1.log"), "_LPT1.log");
        assert_eq!(windows_safe_path_component("wechat."), "wechat_");
        assert_eq!(windows_safe_path_component("  "), "channel");
    }

    #[test]
    fn legacy_percent_encoding_escapes_unsafe_and_trailing_chars() {
        assert_eq!(
            legacy_percent_encoded_channel_workspace_dir_name("  "),
            "channel"
        );
        assert_eq!(
            legacy_percent_encoded_channel_workspace_dir_name("tele/gram%1"),
            "tele%2Fgram%251"
        );
        assert_eq!(
            legacy_percent_encoded_channel_workspace_dir_name("wechat."),
            "wechat%2E"
        );
        assert_eq!(
            legacy_percent_encoded_channel_workspace_dir_name("con"),
            "_con"
        );
    }

    struct MinimalChannel;

    #[async_trait]
    impl Channel for MinimalChannel {
        fn name(&self) -> &str {
            "minimal"
        }

        fn username(&self) -> &str {
            "minimal-bot"
        }

        fn id(&self) -> String {
            "minimal:test".to_string()
        }

        async fn send(&self, _message: &SendMessage) -> Result<(), BoxError> {
            Ok(())
        }

        async fn listen(
            &self,
            _cancel_token: CancellationToken,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> Result<(), BoxError> {
            Ok(())
        }
    }

    #[test]
    fn shared_split_prefers_newlines_then_spaces_then_hard_breaks() {
        // Short input stays whole.
        assert_eq!(
            split_message_on_word_boundaries("short", 10, 8),
            vec!["short"]
        );

        // A newline in the second half of the chunk wins over later spaces.
        let text = format!("{}\n{} tail", "a".repeat(6), "b".repeat(10));
        let chunks = split_message_on_word_boundaries(&text, 10, 8);
        assert!(chunks[0].ends_with('\n'));

        // Without any boundary the chunk is hard-split at the limit.
        let solid = "c".repeat(25);
        let chunks = split_message_on_word_boundaries(&solid, 10, 8);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 10));
    }

    #[test]
    fn shared_split_keeps_every_multi_chunk_within_split_limit() {
        // Regression: the final chunk used to be capped at `max_len` rather than
        // `split_limit`, so once the tail landed in (split_limit, max_len] the
        // caller's continuation markers pushed it past the platform hard limit.
        // Craft an input whose tail (4090) is exactly in that window for the
        // Telegram constants (max_len 4096, split_limit 4066).
        let text = format!("{}{}", "a".repeat(4066), "b".repeat(4090));
        let chunks = split_message_on_word_boundaries(&text, 4096, 4066);

        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 4066));
    }

    #[test]
    fn shared_transient_error_check_matches_baseline_signatures() {
        assert!(is_transient_send_error("Connection reset"));
        assert!(is_transient_send_error("HTTP 429"));
        assert!(is_transient_send_error("Service Temporarily Unavailable"));
        assert!(!is_transient_send_error("400 bad request"));
    }

    #[tokio::test]
    async fn channel_trait_defaults_are_tolerant_no_ops() {
        let channel = MinimalChannel;

        channel.set_workspace(PathBuf::from("/tmp/anda-min"));
        let result = channel.init(ChannelInitOptions::default()).await.unwrap();
        assert!(!result.changed);
        assert!(result.message.contains("minimal:test"));

        assert!(channel.health_check().await);
        assert!(!channel.should_retry_send("timeout"));
        assert!(!channel.supports_draft_updates());
        assert!(!channel.supports_multi_message_streaming());
        assert_eq!(channel.multi_message_delay_ms(), 800);

        channel.start_typing("alice").await.unwrap();
        channel.stop_typing("alice").await.unwrap();
        assert_eq!(
            channel
                .send_draft(&SendMessage::new("hi", "alice"))
                .await
                .unwrap(),
            None
        );
        channel.update_draft("alice", "m1", "text").await.unwrap();
        channel
            .update_draft_progress("alice", "m1", "running")
            .await
            .unwrap();
        channel.finalize_draft("alice", "m1", "done").await.unwrap();
        channel.cancel_draft("alice", "m1").await.unwrap();
        channel.add_reaction("c1", "m1", "👀").await.unwrap();
        channel.remove_reaction("c1", "m1", "👀").await.unwrap();
        channel.pin_message("c1", "m1").await.unwrap();
        channel.unpin_message("c1", "m1").await.unwrap();
        channel.redact_message("c1", "m1", None).await.unwrap();
    }
}
