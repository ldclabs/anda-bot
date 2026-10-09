use anda_core::{BoxError, Json, Resource};
use anda_db::schema::{AndaDBSchema, FieldTyped};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// Message to send through a channel
#[derive(Debug, Clone, Default)]
pub struct SendMessage {
    pub content: String,
    pub recipient: String,
    /// Platform thread identifier for threaded replies (e.g. Slack `thread`).
    pub thread: Option<String>,
    /// File attachments to send with the message.
    /// Unsupported attachments must produce an explicit error.
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

/// How long a platform event id is remembered for duplicate suppression.
pub(crate) const EVENT_DEDUP_WINDOW: Duration = Duration::from_secs(30 * 60);

/// Suppresses redelivered platform events (websocket resumes, webhook
/// retries, long-poll rewinds) by remembering event ids for a bounded window.
/// In-memory only: a restart forgets history, which matches the reconnect
/// windows this protects.
pub(crate) struct RecentEventDedup {
    window: Duration,
    seen: Mutex<(HashMap<String, Instant>, Instant)>,
}

impl RecentEventDedup {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            window,
            seen: Mutex::new((HashMap::new(), Instant::now())),
        }
    }

    /// Returns true when `event_id` was already observed inside the window,
    /// recording it otherwise. Empty ids are never treated as duplicates.
    pub(crate) fn is_duplicate(&self, event_id: &str) -> bool {
        if event_id.trim().is_empty() {
            return false;
        }

        let now = Instant::now();
        let mut state = self.seen.lock();
        let (seen, last_cleanup) = &mut *state;
        if now.duration_since(*last_cleanup) >= self.window.min(Duration::from_secs(60)) {
            seen.retain(|_, instant| now.duration_since(*instant) < self.window);
            *last_cleanup = now;
        }
        if seen
            .get(event_id)
            .is_some_and(|instant| now.duration_since(*instant) < self.window)
        {
            return true;
        }
        seen.insert(event_id.to_string(), now);
        false
    }
}

/// Wraps a multi-chunk message with continuation markers so readers can tell
/// a split reply from unrelated consecutive messages. A single chunk passes
/// through verbatim. Chunks must already leave room for the markers (see
/// `split_message_on_word_boundaries`'s `split_limit`).
pub(crate) fn apply_continuation_markers(chunks: &[String]) -> Vec<String> {
    if chunks.len() <= 1 {
        return chunks.to_vec();
    }

    let last = chunks.len() - 1;
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            if index == 0 {
                format!("{chunk}\n\n(continues...)")
            } else if index == last {
                format!("(continued)\n\n{chunk}")
            } else {
                format!("(continued)\n\n{chunk}\n\n(continues...)")
            }
        })
        .collect()
}

/// Picks a uniformly random entry from a static pool (e.g. ACK reactions).
pub(crate) fn random_from_pool(pool: &'static [&'static str]) -> &'static str {
    pool[rand::random_range(0..pool.len())]
}

// Room to close a code fence at the end of a chunk ("\n```") and reopen it
// at the start of the next one (a fence line of at most `MAX_REOPENED_FENCE`
// chars plus "\n").
const CODE_FENCE_RESERVE: usize = 24;
const MAX_REOPENED_FENCE: usize = 16;

/// Splits `message` into chunks, preferring to break on newline and then
/// whitespace boundaries.
///
/// A message that already fits within `max_len` is returned as a single chunk
/// verbatim. Once a message has to be split, **every** chunk (including the
/// final one) is kept within `split_limit` so callers always have room to
/// append continuation markers without exceeding `max_len`. Callers must pass
/// `split_limit <= max_len`.
///
/// A newline or space break is only taken when it falls in the second half of
/// the chunk, so text with few breaks (e.g. CJK) is not cut into slivers; a
/// hard character split is used otherwise. A code block cut by a split is
/// closed at the end of its chunk and reopened at the start of the next one.
pub(crate) fn split_message_on_word_boundaries(
    message: &str,
    max_len: usize,
    split_limit: usize,
) -> Vec<String> {
    if message.chars().count() <= max_len {
        return vec![message.to_string()];
    }

    let has_fences = message.contains("```");
    let body_limit = if has_fences && split_limit > 2 * CODE_FENCE_RESERVE {
        split_limit - CODE_FENCE_RESERVE
    } else {
        split_limit
    };

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        // Once we are splitting, cap every chunk at `split_limit` (not `max_len`)
        // so the tail chunk still leaves room for continuation markers.
        let hard_split = remaining
            .char_indices()
            .nth(body_limit)
            .map_or(remaining.len(), |(idx, _)| idx);
        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            let in_second_half =
                |pos: &usize| search_area[..*pos].chars().count() >= body_limit / 2;
            search_area
                .rfind('\n')
                .filter(in_second_half)
                .or_else(|| search_area.rfind(' ').filter(in_second_half))
                .map_or(hard_split, |pos| pos + 1)
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    if has_fences {
        balance_code_fences(&mut chunks);
    }
    chunks
}

fn balance_code_fences(chunks: &mut [String]) {
    let mut open_fence: Option<String> = None;
    for chunk in chunks {
        let reopened = open_fence.clone();
        for line in chunk.split('\n') {
            let line = line.trim();
            if line.starts_with("```") {
                open_fence = match open_fence {
                    Some(_) => None,
                    None if line.chars().count() <= MAX_REOPENED_FENCE => Some(line.to_string()),
                    None => Some("```".to_string()),
                };
            }
        }
        if let Some(fence) = reopened {
            chunk.insert_str(0, &format!("{fence}\n"));
        }
        if open_fence.is_some() {
            if !chunk.ends_with('\n') {
                chunk.push('\n');
            }
            chunk.push_str("```");
        }
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_message_new_sets_required_fields_only() {
        let message = SendMessage::new("hello", "alice");

        assert_eq!(message.content, "hello");
        assert_eq!(message.recipient, "alice");
        assert_eq!(message.thread, None);
        assert!(message.attachments.is_empty());
    }

    #[test]
    fn send_message_builders_preserve_thread_and_attachments() {
        let attachment = Resource {
            name: "voice.mp3".to_string(),
            mime_type: Some("audio/mpeg".to_string()),
            ..Default::default()
        };
        let message = SendMessage::new("report", "ops")
            .in_thread(Some("thread-42".to_string()))
            .with_attachments(vec![attachment]);

        assert_eq!(message.content, "report");
        assert_eq!(message.recipient, "ops");
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

    #[test]
    fn recent_event_dedup_suppresses_repeats_and_ignores_empty_ids() {
        let dedup = RecentEventDedup::new(Duration::from_secs(60));
        assert!(!dedup.is_duplicate("evt_1"));
        assert!(dedup.is_duplicate("evt_1"));
        assert!(!dedup.is_duplicate("evt_2"));
        assert!(!dedup.is_duplicate(""));
        assert!(!dedup.is_duplicate("  "));
    }

    struct MinimalChannel;

    #[async_trait]
    impl Channel for MinimalChannel {
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
    fn shared_split_does_not_break_on_an_early_space() {
        // CJK text has few spaces: an early one must not leave a sliver chunk.
        let text = format!("好的 {}", "字".repeat(30));
        let chunks = split_message_on_word_boundaries(&text, 20, 16);
        assert_eq!(chunks[0].chars().count(), 16);
        assert!(chunks.iter().all(|chunk| !chunk.trim().is_empty()));

        // A space in the second half is still preferred over a hard split.
        let text = format!("{} {}", "a".repeat(12), "b".repeat(30));
        let chunks = split_message_on_word_boundaries(&text, 20, 16);
        assert_eq!(chunks[0], format!("{} ", "a".repeat(12)));
    }

    #[test]
    fn shared_split_closes_and_reopens_cut_code_blocks() {
        let code = (0..200)
            .map(|i| format!("let value_{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!("Intro\n```rust\n{code}\n```\nDone");
        let chunks = split_message_on_word_boundaries(&text, 2000, 2000);

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 2000);
            let fences = chunk
                .lines()
                .filter(|line| line.trim().starts_with("```"))
                .count();
            assert_eq!(fences % 2, 0, "unbalanced chunk: {chunk}");
        }
        assert!(chunks[1].starts_with("```rust\n"));
        assert!(chunks.last().unwrap().ends_with("Done"));
    }

    #[tokio::test]
    async fn channel_trait_defaults_are_tolerant_no_ops() {
        let channel = MinimalChannel;

        channel.set_workspace(PathBuf::from("/tmp/anda-min"));
        let result = channel.init(ChannelInitOptions::default()).await.unwrap();
        assert!(!result.changed);
        assert!(result.message.contains("minimal:test"));
    }
}
