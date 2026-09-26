use anda_core::BoxError;
use anda_db::database::AndaDB;
use anda_engine::engine::EngineRef;
use axum::Router;
use std::{net::SocketAddr, sync::Arc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;

use crate::{brain, channel, config, cron, engine};

mod chat;
mod client;
pub use chat::*;
pub use client::*;

#[allow(clippy::too_many_arguments)]
pub async fn serve(
    cancel_token: CancellationToken,
    db: Arc<AndaDB>,
    addr: String,
    brain_cfg: brain::BrainConfig,
    engine_cfg: engine::EngineConfig,
    engine_ref: Arc<EngineRef>,
    cron: Arc<cron::CronRuntime>,
    completion_hooks: Vec<Arc<dyn engine::CompletionHook>>,
    channel_sender: channel::ChannelSender,
) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
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

    let addr: SocketAddr = addr.parse()?;
    // create_reuse_port_listener(addr).await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let server_cancel_token = cancel_token.clone();
    let background_cancel_token = cancel_token.clone();
    let memory = engines.memory.clone();
    let memory_cancel_token = cancel_token.clone();
    let brain_admission = engines.brain_admission_state();
    let app = Router::new()
        .merge(engines.into_router(cancel_token.clone()))
        .merge(
            brain
                .into_router()
                .layer(axum::middleware::from_fn_with_state(
                    brain_admission,
                    engine::brain_admission,
                )),
        )
        .layer(CompressionLayer::new());

    log::warn!(
        name = "gateway";
        "start service {}@{} on {:?}.",
        config::APP_NAME,
        config::APP_VERSION,
        addr,
    );

    Ok(tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    server_cancel_token.cancelled_owned().await;
                    log::warn!(
                        name = "gateway";
                        "received cancellation signal, starting graceful shutdown"
                    );
                })
                .await
                .map_err(|error| -> BoxError { error.into() })
        });

        tasks.spawn(async move {
            brain_state
                .start_background_tasks(background_cancel_token)
                .await;
            Ok(())
        });
        tasks.spawn(async move {
            memory.run_background(memory_cancel_token).await;
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
