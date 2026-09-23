//! Unified error types for the weixin-agent SDK.

/// All errors produced by this SDK.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// HTTP transport error.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Server returned a non-zero error code.
    #[error("API error: errcode={errcode}, errmsg={errmsg}")]
    Api {
        /// Server error code.
        errcode: i32,
        /// Server error message.
        errmsg: String,
    },

    /// Session expired; the monitor will pause for one hour.
    #[error("Session expired for account, paused until cooldown")]
    SessionExpired,

    /// CDN upload failure.
    #[error("CDN upload failed: {0}")]
    CdnUpload(String),

    /// AES encryption/decryption failure.
    #[error("Encryption error: {0}")]
    Crypto(String),

    /// Invalid configuration.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Operation timed out.
    #[error("Timeout: {0}")]
    Timeout(String),
}

/// Convenience alias used throughout the SDK.
pub type Result<T> = std::result::Result<T, Error>;
