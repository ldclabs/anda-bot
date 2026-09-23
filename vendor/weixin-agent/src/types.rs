//! Protocol types mirroring the Weixin iLink Bot API.

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

// ── Protocol constants ──────────────────────────────────────────────

/// iLink-App-Id header value.
pub const ILINK_APP_ID: &str = "bot";
/// Channel version sent in `base_info`.
pub const CHANNEL_VERSION: &str = "2.4.3";
/// Fixed QR code base URL.
pub const QR_CODE_BASE_URL: &str = "https://ilinkai.weixin.qq.com/";
/// Default bot type for QR login.
pub const DEFAULT_ILINK_BOT_TYPE: &str = "3";
/// Session-expired error code from server.
pub const SESSION_EXPIRED_ERRCODE: i32 = -14;
/// Text chunk limit (characters).
pub const TEXT_CHUNK_LIMIT: usize = 4000;

// ── Timing constants (ms) ───────────────────────────────────────────

/// Long-poll timeout.
pub const DEFAULT_LONG_POLL_TIMEOUT_MS: u64 = 35_000;
/// Regular API timeout.
pub const DEFAULT_API_TIMEOUT_MS: u64 = 15_000;
/// Config/typing API timeout.
pub const DEFAULT_CONFIG_TIMEOUT_MS: u64 = 10_000;
/// Session pause after expiry.
pub const SESSION_PAUSE_DURATION_MS: u64 = 3_600_000;
/// Max consecutive poll failures before backoff.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Backoff delay after max failures.
pub const BACKOFF_DELAY_MS: u64 = 30_000;
/// Normal retry delay.
pub const RETRY_DELAY_MS: u64 = 2_000;
/// CDN upload max retries.
pub const UPLOAD_MAX_RETRIES: u32 = 3;
/// Config cache TTL.
pub const CONFIG_CACHE_TTL_MS: u64 = 86_400_000;
/// Max QR refresh count.
pub const MAX_QR_REFRESH_COUNT: u32 = 3;
/// QR poll timeout.
pub const DEFAULT_QR_POLL_TIMEOUT_MS: u64 = 35_000;

// ── Enums ───────────────────────────────────────────────────────────

/// CDN upload media type.
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum UploadMediaType {
    /// Image upload.
    Image = 1,
    /// Video upload.
    Video = 2,
    /// Generic file upload.
    File = 3,
    /// Voice upload.
    Voice = 4,
}

/// Message sender type.
#[derive(Debug, Clone, Copy, Default, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// Unset.
    #[default]
    None = 0,
    /// From a human user.
    User = 1,
    /// From a bot.
    Bot = 2,
}

/// Message item content type.
#[derive(Debug, Clone, Copy, Default, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageItemType {
    /// Unset.
    #[default]
    None = 0,
    /// Text content.
    Text = 1,
    /// Image content.
    Image = 2,
    /// Voice content.
    Voice = 3,
    /// File attachment.
    File = 4,
    /// Video content.
    Video = 5,
}

/// Message generation state.
#[derive(Debug, Clone, Copy, Default, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageState {
    /// New / finished.
    #[default]
    New = 0,
    /// Still generating (streaming).
    Generating = 1,
    /// Generation complete.
    Finish = 2,
}

/// Typing indicator status.
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr, PartialEq, Eq)]
#[repr(u8)]
pub enum TypingStatus {
    /// Currently typing.
    Typing = 1,
    /// Cancel typing indicator.
    Cancel = 2,
}

/// High-level media type for inbound messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    /// Image media.
    Image,
    /// Video media.
    Video,
    /// Voice media.
    Voice,
    /// Generic file.
    File,
}

// ── BaseInfo ────────────────────────────────────────────────────────

/// Metadata attached to every outgoing API request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaseInfo {
    /// Channel version string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_version: Option<String>,
    /// Bot agent UA string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_agent: Option<String>,
}

/// Build a `BaseInfo` with the current channel version and bot agent.
pub fn build_base_info_with_agent(bot_agent: &str) -> BaseInfo {
    BaseInfo {
        channel_version: Some(CHANNEL_VERSION.to_owned()),
        bot_agent: Some(bot_agent.to_owned()),
    }
}

/// Build a `BaseInfo` with the current channel version (legacy, prefer `build_base_info_with_agent`).
pub fn build_base_info() -> BaseInfo {
    build_base_info_with_agent("weixin-agent-rs")
}

// ── CDN / Media sub-structures ──────────────────────────────────────

/// CDN media reference.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CdnMedia {
    /// Encrypted query parameter for CDN download.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypt_query_param: Option<String>,
    /// AES key (base64-encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aes_key: Option<String>,
    /// Encrypt type: 0 = fileid only, 1 = packed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypt_type: Option<i32>,
    /// Full download URL from server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_url: Option<String>,
}

/// Text item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextItem {
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Image item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageItem {
    /// Original image CDN reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    /// Thumbnail CDN reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_media: Option<CdnMedia>,
    /// Raw AES key as hex string (preferred for inbound decryption).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aeskey: Option<String>,
    /// Image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Mid-size ciphertext bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mid_size: Option<i64>,
    /// Thumbnail size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_size: Option<i64>,
    /// Thumbnail height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_height: Option<i64>,
    /// Thumbnail width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_width: Option<i64>,
    /// HD size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hd_size: Option<i64>,
}

/// Voice item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceItem {
    /// Voice CDN reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    /// Encoding type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encode_type: Option<i32>,
    /// Bits per sample.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits_per_sample: Option<i32>,
    /// Sample rate (Hz).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,
    /// Duration in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playtime: Option<i64>,
    /// Speech-to-text result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// File item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileItem {
    /// File CDN reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    /// Original file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// File MD5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    /// Plaintext file size as string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub len: Option<String>,
}

/// Video item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoItem {
    /// Video CDN reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    /// Video ciphertext size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_size: Option<i64>,
    /// Play length in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_length: Option<i64>,
    /// Video MD5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_md5: Option<String>,
    /// Thumbnail CDN reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_media: Option<CdnMedia>,
    /// Thumbnail size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_size: Option<i64>,
    /// Thumbnail height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_height: Option<i64>,
    /// Thumbnail width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_width: Option<i64>,
}

/// Reference (quoted) message.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefMessage {
    /// Quoted message item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_item: Option<Box<MessageItem>>,
    /// Summary title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A single content item within a message.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageItem {
    /// Item type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub item_type: Option<MessageItemType>,
    /// Creation timestamp (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time_ms: Option<i64>,
    /// Update timestamp (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time_ms: Option<i64>,
    /// Whether generation is complete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_completed: Option<bool>,
    /// Item-level message ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    /// Referenced (quoted) message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_msg: Option<RefMessage>,
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_item: Option<TextItem>,
    /// Image content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_item: Option<ImageItem>,
    /// Voice content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_item: Option<VoiceItem>,
    /// File content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_item: Option<FileItem>,
    /// Video content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_item: Option<VideoItem>,
}

// ── WeixinMessage ───────────────────────────────────────────────────

/// Unified message from `getUpdates`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeixinMessage {
    /// Sequence number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    /// Server-assigned message ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<i64>,
    /// Sender user ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_user_id: Option<String>,
    /// Recipient user ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_user_id: Option<String>,
    /// Client-generated message ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Creation timestamp (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time_ms: Option<i64>,
    /// Update timestamp (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time_ms: Option<i64>,
    /// Deletion timestamp (ms); >0 means recalled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_time_ms: Option<i64>,
    /// Session ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Group ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Sender type (user / bot).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_type: Option<MessageType>,
    /// Generation state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_state: Option<MessageState>,
    /// Content items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_list: Option<Vec<MessageItem>>,
    /// Context token for replies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
}

// ── API request / response types ────────────────────────────────────

/// `getUpdates` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetUpdatesRequest {
    /// Full context buf from previous response.
    pub get_updates_buf: String,
    /// Metadata.
    pub base_info: BaseInfo,
}

/// `getUpdates` response body.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetUpdatesResponse {
    /// Return code (0 = success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ret: Option<i32>,
    /// Error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errcode: Option<i32>,
    /// Error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errmsg: Option<String>,
    /// Inbound messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msgs: Option<Vec<WeixinMessage>>,
    /// Legacy sync buf (compat).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_buf: Option<String>,
    /// New context buf to cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get_updates_buf: Option<String>,
    /// Server-suggested next poll timeout (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longpolling_timeout_ms: Option<u64>,
}

/// `sendMessage` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    /// The message to send.
    pub msg: WeixinMessage,
    /// Metadata.
    pub base_info: BaseInfo,
}

/// `getUploadUrl` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetUploadUrlRequest {
    /// Random file key (32 hex chars).
    pub filekey: String,
    /// Upload media type.
    pub media_type: UploadMediaType,
    /// Recipient user ID.
    pub to_user_id: String,
    /// Plaintext file size.
    pub rawsize: u64,
    /// Plaintext file MD5 hex.
    pub rawfilemd5: String,
    /// Ciphertext file size.
    pub filesize: u64,
    /// Whether thumbnail is not needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_need_thumb: Option<bool>,
    /// Thumbnail plaintext size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_rawsize: Option<u64>,
    /// Thumbnail plaintext MD5 hex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_rawfilemd5: Option<String>,
    /// Thumbnail ciphertext size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_filesize: Option<u64>,
    /// AES key hex string.
    pub aeskey: String,
    /// Metadata.
    pub base_info: BaseInfo,
}

/// `getUploadUrl` response body.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetUploadUrlResponse {
    /// Upload encrypted parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_param: Option<String>,
    /// Thumbnail upload parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_upload_param: Option<String>,
    /// Full upload URL from server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_full_url: Option<String>,
}

/// `getConfig` request body (internal).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GetConfigRequest {
    /// User ID to get config for.
    pub ilink_user_id: String,
    /// Optional context token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
    /// Metadata.
    pub base_info: BaseInfo,
}

/// `getConfig` response body.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetConfigResponse {
    /// Return code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ret: Option<i32>,
    /// Error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errmsg: Option<String>,
    /// Typing ticket (base64).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typing_ticket: Option<String>,
}

/// `sendTyping` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTypingRequest {
    /// Target user ID.
    pub ilink_user_id: String,
    /// Typing ticket from `getConfig`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typing_ticket: Option<String>,
    /// Typing status.
    pub status: TypingStatus,
    /// Metadata.
    pub base_info: BaseInfo,
}

// ── QR login types ──────────────────────────────────────────────────

/// QR code response from server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrCodeResponse {
    /// QR code token string.
    pub qrcode: String,
    /// QR code image URL.
    pub qrcode_img_content: String,
}

/// QR status response from server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QrStatusResponse {
    /// Current status.
    pub status: String,
    /// Bot token (on confirmed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    /// Bot ID (on confirmed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ilink_bot_id: Option<String>,
    /// Base URL (on confirmed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseurl: Option<String>,
    /// User ID who scanned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ilink_user_id: Option<String>,
    /// Redirect host for IDC redirect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_host: Option<String>,
}
