//! Long-poll `getUpdates` loop with error handling, backoff, and session guard.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::api::client::HttpApiClient;
use crate::api::config_cache::ConfigCache;
use crate::api::session_guard::SessionGuard;
use crate::error::Result;
use crate::messaging::inbound::{self, ContextTokenStore, MessageSender};
use crate::types::{
    BACKOFF_DELAY_MS, GetUpdatesRequest, MAX_CONSECUTIVE_FAILURES, RETRY_DELAY_MS,
    SESSION_EXPIRED_ERRCODE,
};

/// The handler trait users implement to receive messages.
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync {
    /// Called for each inbound user message.
    async fn on_message(&self, ctx: &inbound::MessageContext) -> Result<()>;

    /// Called when `get_updates_buf` changes — persist it here.
    async fn on_sync_buf_updated(&self, _sync_buf: &str) -> Result<()> {
        Ok(())
    }

    /// Lifecycle hook: called before the poll loop starts.
    async fn on_start(&self) -> Result<()> {
        Ok(())
    }

    /// Lifecycle hook: called after the poll loop ends.
    async fn on_shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Run the long-poll monitor loop. Blocks until cancellation.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) async fn run_monitor(
    api: Arc<HttpApiClient>,
    cdn_base_url: String,
    handler: Arc<dyn MessageHandler>,
    session_guard: Arc<SessionGuard>,
    config_cache: Arc<ConfigCache>,
    context_tokens: Arc<ContextTokenStore>,
    initial_sync_buf: Option<String>,
    initial_timeout: Duration,
    cancel: CancellationToken,
    markdown_filter_enabled: bool,
) -> Result<()> {
    handler.on_start().await?;

    let mut get_updates_buf = initial_sync_buf.unwrap_or_default();
    let mut next_timeout = initial_timeout;
    let mut consecutive_failures: u32 = 0;

    let sender = Arc::new(MessageSender {
        api: Arc::clone(&api),
        cdn_base_url: cdn_base_url.clone(),
        config_cache: Arc::clone(&config_cache),
        markdown_filter_enabled,
    });

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // Check session guard
        if session_guard.is_paused() {
            let remaining = session_guard.remaining_ms();
            tracing::info!(remaining_ms = remaining, "session paused, sleeping");
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(Duration::from_millis(remaining)) => continue,
            }
        }

        let req = GetUpdatesRequest {
            get_updates_buf: get_updates_buf.clone(),
            base_info: api.base_info(),
        };

        let resp = tokio::select! {
            () = cancel.cancelled() => break,
            result = api.get_updates(&req, next_timeout) => {
                match result {
                    Ok(r) => r,
                    Err(e) => {
                        consecutive_failures += 1;
                        tracing::error!(
                            error = %e,
                            failures = consecutive_failures,
                            "getUpdates error"
                        );
                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            consecutive_failures = 0;
                            sleep_or_cancel(Duration::from_millis(BACKOFF_DELAY_MS), &cancel).await;
                        } else {
                            sleep_or_cancel(Duration::from_millis(RETRY_DELAY_MS), &cancel).await;
                        }
                        continue;
                    }
                }
            }
        };

        // Update dynamic timeout
        if let Some(t) = resp.longpolling_timeout_ms {
            if t > 0 {
                next_timeout = Duration::from_millis(t);
            }
        }

        // Check API-level errors
        let is_error = resp.ret.unwrap_or(0) != 0 || resp.errcode.unwrap_or(0) != 0;
        if is_error {
            let errcode = resp.errcode.or(resp.ret).unwrap_or(0);
            if errcode == SESSION_EXPIRED_ERRCODE {
                session_guard.pause();
                consecutive_failures = 0;
                let remaining = session_guard.remaining_ms();
                tracing::error!(
                    errcode,
                    remaining_min = remaining / 60_000,
                    "session expired, pausing"
                );
                sleep_or_cancel(Duration::from_millis(remaining), &cancel).await;
                continue;
            }

            consecutive_failures += 1;
            tracing::error!(
                ret = resp.ret,
                errcode = resp.errcode,
                errmsg = resp.errmsg.as_deref().unwrap_or(""),
                failures = consecutive_failures,
                "getUpdates API error"
            );
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                consecutive_failures = 0;
                sleep_or_cancel(Duration::from_millis(BACKOFF_DELAY_MS), &cancel).await;
            } else {
                sleep_or_cancel(Duration::from_millis(RETRY_DELAY_MS), &cancel).await;
            }
            continue;
        }

        // Success
        consecutive_failures = 0;

        // Update sync buf (prefer get_updates_buf, fall back to deprecated sync_buf)
        let new_buf = resp
            .get_updates_buf
            .as_ref()
            .or(resp.sync_buf.as_ref())
            .filter(|b| !b.is_empty());
        if let Some(new_buf) = new_buf {
            get_updates_buf.clone_from(new_buf);
            if let Err(e) = handler.on_sync_buf_updated(new_buf).await {
                tracing::error!(error = %e, "on_sync_buf_updated failed");
            }
        }

        // Process messages
        let msgs = resp.msgs.unwrap_or_default();
        for msg in &msgs {
            if !inbound::should_process(msg) {
                continue;
            }

            // Update context token store
            if let (Some(from), Some(token)) = (&msg.from_user_id, &msg.context_token) {
                context_tokens.set(from, token);
            }

            let ctx = inbound::parse_inbound_message(msg, Arc::clone(&sender));
            if let Err(e) = handler.on_message(&ctx).await {
                tracing::error!(
                    error = %e,
                    from = %ctx.from,
                    message_id = %ctx.message_id,
                    "on_message handler error"
                );
            }
        }
    }

    if let Err(e) = api.notify_stop().await {
        tracing::warn!(error = %e, "notify_stop failed");
    }

    handler.on_shutdown().await?;
    tracing::info!("monitor loop ended");
    Ok(())
}

async fn sleep_or_cancel(duration: Duration, cancel: &CancellationToken) {
    tokio::select! {
        () = cancel.cancelled() => {},
        () = tokio::time::sleep(duration) => {},
    }
}
