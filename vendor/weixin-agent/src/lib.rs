//! `weixin-agent` — Pure protocol SDK for the Weixin iLink Bot API.
//!
//! This crate handles HTTP API calls, CDN upload/download, AES encryption,
//! long-poll monitoring, and message parsing. It does **not** manage state
//! persistence — that is the caller's responsibility.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use weixin_agent::{WeixinClient, WeixinConfig, MessageHandler, MessageContext, Result};
//!
//! struct EchoBot;
//!
//! #[async_trait::async_trait]
//! impl MessageHandler for EchoBot {
//!     async fn on_message(&self, ctx: &MessageContext) -> Result<()> {
//!         if let Some(text) = &ctx.body {
//!             ctx.reply_text(text).await?;
//!         }
//!         Ok(())
//!     }
//! }
//! ```

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]

pub mod api;
pub mod cdn;
pub mod config;
pub mod error;
pub mod media;
pub mod messaging;
pub mod monitor;
pub mod qr_login;
pub mod types;
pub mod util;

mod client;

// ── Public re-exports ───────────────────────────────────────────────

pub use client::{WeixinClient, WeixinClientBuilder};
pub use config::{WeixinConfig, WeixinConfigBuilder};
pub use error::{Error, Result};
pub use messaging::inbound::{
    ContextTokenStore, MediaInfo, MessageContext, RefMessageInfo, SendResult,
};
pub use messaging::markdown_filter::{StreamingMarkdownFilter, filter_markdown};
pub use monitor::poll_loop::MessageHandler;
pub use qr_login::login::{LoginStatus, QrLoginApi, QrLoginSession, StandaloneQrLogin};
pub use types::{
    CdnMedia, MediaType, MessageItemType, MessageState, MessageType, TypingStatus, UploadMediaType,
};
