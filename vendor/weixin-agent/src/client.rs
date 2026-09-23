//! SDK entry point: [`WeixinClient`] and its builder.

use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::api::client::HttpApiClient;
use crate::api::config_cache::ConfigCache;
use crate::api::session_guard::SessionGuard;
use crate::config::WeixinConfig;
use crate::error::{Error, Result};
use crate::messaging::inbound::{ContextTokenStore, SendResult};
use crate::monitor::poll_loop::MessageHandler;
use crate::qr_login::login::QrLoginApi;

/// The main SDK client.
pub struct WeixinClient {
    config: Arc<WeixinConfig>,
    handler: Arc<dyn MessageHandler>,
    api: Arc<HttpApiClient>,
    session_guard: Arc<SessionGuard>,
    config_cache: Arc<ConfigCache>,
    context_tokens: Arc<ContextTokenStore>,
    cancel: CancellationToken,
}

/// Builder for [`WeixinClient`].
#[must_use]
pub struct WeixinClientBuilder {
    config: WeixinConfig,
    handler: Option<Arc<dyn MessageHandler>>,
    cancel: CancellationToken,
}

impl WeixinClient {
    /// Create a new builder.
    pub fn builder(config: WeixinConfig) -> WeixinClientBuilder {
        WeixinClientBuilder {
            config,
            handler: None,
            cancel: CancellationToken::new(),
        }
    }

    /// Start the long-poll monitor loop. Blocks until shutdown.
    ///
    /// `initial_sync_buf` should be loaded from your persistence layer (or `None` for fresh start).
    pub async fn start(&self, initial_sync_buf: Option<String>) -> Result<()> {
        if let Err(e) = self.api.notify_start().await {
            tracing::warn!(error = %e, "notify_start failed");
        }

        crate::monitor::poll_loop::run_monitor(
            Arc::clone(&self.api),
            self.config.cdn_base_url.clone(),
            Arc::clone(&self.handler),
            Arc::clone(&self.session_guard),
            Arc::clone(&self.config_cache),
            Arc::clone(&self.context_tokens),
            initial_sync_buf,
            self.config.long_poll_timeout,
            self.cancel.clone(),
            self.config.markdown_filter_enabled,
        )
        .await
    }

    /// Gracefully shut down the monitor loop.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }

    /// Send a text message to a user.
    pub async fn send_text(
        &self,
        to: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<SendResult> {
        crate::messaging::send::send_text(
            &self.api,
            to,
            text,
            context_token,
            self.config.markdown_filter_enabled,
            self.api.base_info(),
        )
        .await
    }

    /// Send a media file to a user.
    pub async fn send_media(
        &self,
        to: &str,
        file_path: &Path,
        context_token: Option<&str>,
    ) -> Result<SendResult> {
        crate::messaging::send_media::send_media_file(
            &self.api,
            &self.config.cdn_base_url,
            to,
            file_path,
            "",
            context_token,
            self.api.base_info(),
        )
        .await
    }

    /// Get a QR login API handle.
    pub fn qr_login(&self) -> QrLoginApi<'_> {
        QrLoginApi::new(&self.api)
    }

    /// Access the context token store (for export/import).
    pub fn context_tokens(&self) -> &ContextTokenStore {
        &self.context_tokens
    }
}

impl WeixinClientBuilder {
    /// Set the message handler.
    pub fn on_message(mut self, handler: impl MessageHandler + 'static) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Optionally set a cancellation token for the monitor loop (for advanced users).
    pub fn with_cancel_token(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<WeixinClient> {
        let handler = self
            .handler
            .ok_or_else(|| Error::Config("message handler is required".into()))?;
        let api = Arc::new(HttpApiClient::new(&self.config));
        let config_cache = Arc::new(ConfigCache::new(Arc::clone(&api)));
        Ok(WeixinClient {
            config: Arc::new(self.config),
            handler,
            api,
            session_guard: Arc::new(SessionGuard::new()),
            config_cache,
            context_tokens: Arc::new(ContextTokenStore::new()),
            cancel: self.cancel,
        })
    }
}
