use anda_core::BoxError;
use anda_engine_server::shutdown_signal;
use anda_object_store::MetaStoreBuilder;
use object_store::{ObjectStore, local::LocalFileSystem};
use std::{path::PathBuf, sync::Arc};
use structured_logger::{Builder, async_json::new_writer, get_env_level};
use tokio_util::sync::CancellationToken;

use crate::{brain, cron, engine, gateway};

pub async fn serve(
    workspace: PathBuf,
    gateway_addr: String,
    brain_cfg: brain::HippocampusConfig,
    engine_cfg: engine::EngineConfig,
) -> Result<(), BoxError> {
    // Initialize structured logging with JSON format
    Builder::with_level(&get_env_level().to_string())
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    // Create global cancellation token for graceful shutdown
    let global_cancel_token = CancellationToken::new();

    let object_store: Arc<dyn ObjectStore> = {
        let db_path = workspace.join("db");
        let os = LocalFileSystem::new_with_prefix(db_path)?;
        let os = MetaStoreBuilder::new(os, 100000).build();
        Arc::new(os)
    };

    let cron_handle = cron::serve(global_cancel_token.child_token()).await?;
    let gateway_handle = gateway::serve(
        global_cancel_token.child_token(),
        object_store,
        gateway_addr,
        brain_cfg,
        engine_cfg,
    )
    .await?;

    let terminate_handle = shutdown_signal(global_cancel_token);
    let _ = tokio::join!(cron_handle, gateway_handle, terminate_handle);

    Ok(())
}
