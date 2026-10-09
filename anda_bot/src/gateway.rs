use anda_core::BoxError;
use anda_db::database::AndaDB;
use anda_engine::engine::EngineRef;
use axum::{Router, serve::ListenerExt};
use std::sync::Arc;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tower_http::{CompressionLevel, compression::CompressionLayer};

use crate::{brain, channel, config, cron, engine};

mod chat;
mod client;
pub use chat::*;
pub use client::*;

#[allow(clippy::too_many_arguments)]
pub async fn serve(
    cancel_token: CancellationToken,
    db: Arc<AndaDB>,
    brain_cfg: brain::BrainConfig,
    engine_cfg: engine::EngineConfig,
    engine_ref: Arc<EngineRef>,
    cron: Arc<cron::CronRuntime>,
    completion_hooks: Vec<Arc<dyn engine::CompletionHook>>,
    channel_sender: channel::ChannelSender,
) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
    let addr = engine_cfg.gateway_addr;
    let runtime_config = brain_cfg.runtime_config.clone();
    let brain = brain::Brain::new(db.object_store(), brain_cfg).await?;
    let brain_state = brain.state.clone();
    let brain_host = brain::Host::new(brain_state.clone(), runtime_config.as_ref())?
        .with_journal(brain::Journal::new(db.object_store()));
    let engines = engine::Engines::new(
        engine_cfg,
        db,
        brain_host,
        engine_ref,
        cron,
        completion_hooks,
        channel_sender,
    )
    .await?;

    // Streamed WebSocket frames are small; send each one without waiting on
    // the previous frame's ACK.
    let listener = tokio::net::TcpListener::bind(addr).await?.tap_io(|tcp| {
        if let Err(err) = tcp.set_nodelay(true) {
            log::warn!(name = "gateway"; "failed to set TCP_NODELAY: {err}");
        }
    });
    let memory = engines.memory.clone();
    let brain_admission = engines.brain_admission_state();
    let plan_access = engines.plan_access_state();
    let app = Router::new()
        .merge(engines.into_router(cancel_token.clone()))
        .merge(
            brain
                .into_router()
                .layer(axum::middleware::from_fn_with_state(
                    brain_admission,
                    engine::brain_admission,
                ))
                .layer(axum::middleware::from_fn_with_state(
                    plan_access,
                    engine::plan_access,
                )),
        )
        // Clients are almost always on loopback, where a tighter level only
        // costs CPU on both ends.
        .layer(CompressionLayer::new().quality(CompressionLevel::Fastest));

    log::warn!(
        name = "gateway";
        "start service {}@{} on {:?}.",
        config::APP_NAME,
        config::APP_VERSION,
        addr,
    );

    Ok(tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        let shutdown = cancel_token.clone().cancelled_owned();
        tasks.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown.await;
                    log::warn!(
                        name = "gateway";
                        "received cancellation signal, starting graceful shutdown"
                    );
                })
                .await
                .map_err(BoxError::from)
        });

        let background = cancel_token.clone();
        tasks.spawn(async move {
            brain_state.start_background_tasks(background).await;
            Ok(())
        });
        let background = cancel_token.clone();
        tasks.spawn(async move {
            memory.run_background(background).await;
            Ok(())
        });
        supervise(tasks, cancel_token).await
    }))
}

async fn supervise(
    mut tasks: JoinSet<Result<(), BoxError>>,
    cancel: CancellationToken,
) -> Result<(), BoxError> {
    let mut error: Option<BoxError> = None;
    while let Some(result) = tasks.join_next().await {
        // The first exit must stop its peers before we wait for them.
        cancel.cancel();
        let result = result
            .map_err(|err| -> BoxError { err.into() })
            .and_then(|result| result);
        if let Err(err) = result {
            if error.is_none() {
                error = Some(err);
            } else {
                log::error!("gateway task failed during shutdown: {err}");
            }
        }
    }
    error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn first_exit_cancels_and_drains_gateway_peers() {
        for fail in [false, true] {
            let cancel = CancellationToken::new();
            let peer = cancel.clone();
            let mut tasks = JoinSet::new();
            tasks.spawn(async move {
                if fail {
                    Err("gateway failure".into())
                } else {
                    Ok(())
                }
            });
            tasks.spawn(async move {
                peer.cancelled().await;
                Ok(())
            });
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                supervise(tasks, cancel.clone()),
            )
            .await
            .unwrap();
            assert_eq!(result.is_err(), fail);
            assert!(cancel.is_cancelled());
        }
    }
}
