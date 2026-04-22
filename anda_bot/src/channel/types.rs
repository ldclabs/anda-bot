use anda_core::{BoxError, Json, Resource};
use anda_db::schema::{
    AndaDBSchema, FieldEntry, FieldKey, FieldType, FieldTyped, Schema, SchemaError,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
#[derive(Debug, Clone, Default, Deserialize, Serialize, FieldTyped, AndaDBSchema)]
pub struct ChannelMessage {
    pub _id: u64,
    pub sender: String,
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

/// Core channel trait — implement for any messaging platform
#[async_trait]
pub trait Channel: Send + Sync {
    /// Human-readable channel name
    fn name(&self) -> &str;

    fn username(&self) -> &str;

    /// Unique channel identifier for message metadata (e.g. "irc:irc.libera.chat").
    fn id(&self) -> String;

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

    /// Signal that the bot is processing a response (e.g. "typing" indicator).
    /// Implementations should repeat the indicator as needed for their platform.
    async fn start_typing(&self, _recipient: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Stop any active typing indicator.
    async fn stop_typing(&self, _recipient: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Whether this channel supports progressive message updates via draft edits.
    fn supports_draft_updates(&self) -> bool {
        false
    }

    /// Whether this channel supports multi-message streaming delivery, where
    /// the response is sent as multiple separate messages at paragraph
    /// boundaries as tokens arrive from the provider.
    fn supports_multi_message_streaming(&self) -> bool {
        false
    }

    /// Minimum delay (ms) between sending each paragraph in multi-message mode.
    /// Channels should override this to avoid platform rate limits.
    fn multi_message_delay_ms(&self) -> u64 {
        800
    }

    /// Send an initial draft message. Returns a platform-specific message ID for later edits.
    async fn send_draft(&self, _message: &SendMessage) -> Result<Option<String>, BoxError> {
        Ok(None)
    }

    /// Update a previously sent draft message with new accumulated content.
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
    async fn update_draft_progress(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Finalize a draft with the complete response (e.g. apply Markdown formatting).
    async fn finalize_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Cancel and remove a previously sent draft message if the channel supports it.
    async fn cancel_draft(&self, _recipient: &str, _message_id: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Add a reaction (emoji) to a message.
    ///
    /// `channel_id` is the platform channel/conversation identifier (e.g. Discord channel ID).
    /// `message_id` is the platform-scoped message identifier (e.g. `discord_<snowflake>`).
    /// `emoji` is the Unicode emoji to react with (e.g. "👀", "✅").
    async fn add_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Remove a reaction (emoji) from a message previously added by this bot.
    async fn remove_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> Result<(), BoxError> {
        Ok(())
    }

    /// Pin a message in the channel.
    async fn pin_message(&self, _channel_id: &str, _message_id: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Unpin a previously pinned message.
    async fn unpin_message(&self, _channel_id: &str, _message_id: &str) -> Result<(), BoxError> {
        Ok(())
    }

    /// Redact (delete) a message from the channel.
    ///
    /// `channel_id` is the platform channel/conversation identifier.
    /// `message_id` is the platform-scoped message identifier.
    /// `reason` is an optional reason for the redaction (may be visible in audit logs).
    async fn redact_message(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _reason: Option<String>,
    ) -> Result<(), BoxError> {
        Ok(())
    }
}
