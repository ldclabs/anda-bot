use anda_core::BoxError;
use anda_engine_server::create_reuse_port_listener;
use axum::Router;
use object_store::ObjectStore;
use std::{net::SocketAddr, sync::Arc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;

use crate::{brain, engine};

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn serve(
    cancel_token: CancellationToken,
    object_store: Arc<dyn ObjectStore>,
    addr: String,
    brain_cfg: brain::HippocampusConfig,
    engine_cfg: engine::EngineConfig,
) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
    let hippocampus = brain::Hippocampus::new(object_store.clone(), brain_cfg).await?;
    let hippocampus_state = hippocampus.state.clone();
    let engine_cfg = engine::EngineConfig {
        brain_base_url: format!("http://{}/v1/{}", addr, brain::ANDA_BOT_SPACE_ID),
        ..engine_cfg
    };
    let engines = engine::Engines::new(engine_cfg, hippocampus.db.clone()).await?;

    let addr: SocketAddr = addr.parse()?;
    let listener = create_reuse_port_listener(addr).await?;
    let server_cancel_token = cancel_token.clone();
    let background_cancel_token = cancel_token.clone();
    let app = Router::new()
        .nest("/bot", engines.into_router())
        .nest("/brain", hippocampus.into_router())
        .layer(CompressionLayer::new());

    log::warn!(
        name = "gateway";
        "start service {}@{} on {:?}.",
        APP_NAME,
        APP_VERSION,
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
