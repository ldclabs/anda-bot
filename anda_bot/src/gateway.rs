use anda_core::BoxError;
use anda_db::database::AndaDB;
use anda_engine::engine::EngineRef;
use anda_engine_server::create_reuse_port_listener;
use axum::Router;
use std::{net::SocketAddr, sync::Arc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;

use crate::{brain, cron, engine};

mod chat;
mod client;
pub use chat::*;
pub use client::*;

pub async fn serve(
    cancel_token: CancellationToken,
    db: Arc<AndaDB>,
    addr: String,
    brain_cfg: brain::HippocampusConfig,
    engine_cfg: engine::EngineConfig,
    engine_ref: Arc<EngineRef>,
    cron: Arc<cron::Cron>,
) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
    let hippocampus = brain::Hippocampus::new(db.object_store(), brain_cfg).await?;
    let hippocampus_state = hippocampus.state.clone();
    let engines = engine::Engines::new(engine_cfg, db, engine_ref, cron).await?;

    let addr: SocketAddr = addr.parse()?;
    let listener = create_reuse_port_listener(addr).await?;
    let server_cancel_token = cancel_token.clone();
    let background_cancel_token = cancel_token.clone();
    let app = Router::new()
        .merge(engines.into_router())
        .merge(hippocampus.into_router())
        .layer(CompressionLayer::new());

    log::warn!(
        name = "gateway";
        "start service {}@{} on {:?}.",
        engine::APP_NAME,
        engine::APP_VERSION,
        addr,
    );

    Ok(tokio::spawn(async move {
        let server_handle = tokio::spawn(
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    server_cancel_token.cancelled_owned().await;
                    log::warn!(
                        name = "gateway";
                        "received cancellation signal, starting graceful shutdown"
                    );
                })
                .into_future(),
        );

        let background_tasks_handle = tokio::spawn(async move {
            hippocampus_state
                .start_background_tasks(background_cancel_token)
                .await;
        });

        let (server_result, background_result) =
            tokio::join!(server_handle, background_tasks_handle);
        background_result?;
        server_result??;
        Ok(())
    }))
}
