use anda_core::{BoxError, Principal};
use anda_db::{database::DBConfig, storage::StorageConfig};
use anda_engine::{
    management::{BaseManagement, Visibility},
    model::{Model, Proxy, gemini, request_client_builder, reqwest},
};
use anda_object_store::MetaStoreBuilder;
use axum::{Router, routing};
use clap::{Parser, Subcommand};
use ic_auth_types::ByteArrayB64;
use ic_cose_types::cose::ed25519::VerifyingKey;
use object_store::{
    ObjectStore,
    aws::{AmazonS3Builder, S3CopyIfNotExists},
    local::LocalFileSystem,
    memory::InMemory,
};
use std::{collections::BTreeSet, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use structured_logger::{Builder, async_json::new_writer, get_env_level};
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;

mod agents;
mod handler;
mod payload;
mod space;
mod types;

use handler::*;
use space::AppState;

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Port to listen on
    #[clap(long, env = "LISTEN_ADDR", default_value = "127.0.0.1:8042")]
    addr: String,

    /// API key
    #[arg(long, env = "ED25519_PUBKEYS", default_value = "")]
    ed25519_pubkeys: String,

    /// Gemini API key for AI model
    #[arg(long, env = "GEMINI_API_KEY", default_value = "")]
    gemini_api_key: String,

    #[arg(
        long,
        env = "GEMINI_API_BASE",
        default_value = "https://generativelanguage.googleapis.com/v1beta/models"
    )]
    gemini_api_base: String,

    #[arg(long, env = "GEMINI_MODEL", default_value = "gemini-3-flash-preview")]
    gemini_model: String,

    #[arg(long, env = "HTTPS_PROXY")]
    https_proxy: Option<String>,

    #[arg(long, env = "SHARDING_IDX", default_value_t = 0)]
    sharding_idx: u32,

    /// Manager principal IDs, separated by comma
    #[arg(long, env = "MANAGERS", default_value = "")]
    managers: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    Local {
        #[clap(long, env = "LOCAL_DB_PATH", default_value = "./db")]
        db: String,
    },
    Aws {
        #[arg(long, env = "AWS_BUCKET")]
        bucket: String,

        #[arg(long, env = "AWS_REGION")]
        region: String,
    },
}

#[derive(Clone, Copy, Debug)]
struct AnyHost;

impl PartialEq<&str> for AnyHost {
    fn eq(&self, _other: &&str) -> bool {
        true
    }
}

/// ```bash
/// cargo run -p anda_hippocampus
/// ```
#[tokio::main]
async fn main() -> Result<(), BoxError> {
    dotenv::dotenv().ok();
    let cli = Cli::parse();

    // Initialize structured logging with JSON format
    Builder::with_level(&get_env_level().to_string())
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    // Create global cancellation token for graceful shutdown
    let global_cancel_token = CancellationToken::new();

    let mut http_client = request_client_builder()
        .timeout(Duration::from_secs(600))
        .retry(
            reqwest::retry::for_host(AnyHost)
                .max_retries_per_request(2)
                .classify_fn(|req_rep| {
                    if req_rep.error().is_some() {
                        return req_rep.retryable();
                    }

                    match req_rep.status() {
                        Some(
                            http::StatusCode::REQUEST_TIMEOUT
                            | http::StatusCode::TOO_MANY_REQUESTS
                            | http::StatusCode::BAD_GATEWAY
                            | http::StatusCode::SERVICE_UNAVAILABLE
                            | http::StatusCode::GATEWAY_TIMEOUT,
                        ) => req_rep.retryable(),
                        _ => req_rep.success(),
                    }
                }),
        );
    if let Some(proxy) = &cli.https_proxy {
        http_client = http_client.proxy(Proxy::all(proxy)?);
    }
    let http_client = http_client.build()?;

    let mut managers = BTreeSet::new();
    if !cli.managers.is_empty() {
        for id in cli.managers.split(',') {
            let id = Principal::from_text(id)?;
            managers.insert(id);
        }
    }
    let management = Arc::new(BaseManagement {
        controller: Principal::management_canister(),
        managers,
        visibility: Visibility::Public,
    });

    // Configure AI model

    // Gemini
    let gemini_default = Model::with_completer(Arc::new(
        gemini::Client::new(&cli.gemini_api_key, Some(cli.gemini_api_base.clone()))
            .with_client(http_client.clone())
            .completion_model(&cli.gemini_model),
    ));

    let gemini_flash = Model::with_completer(Arc::new(
        gemini::Client::new(&cli.gemini_api_key, Some(cli.gemini_api_base))
            .with_client(http_client.clone())
            .completion_model("gemini-3-flash-preview"),
    ));

    let object_store: Arc<dyn ObjectStore> = match cli.command {
        Some(Commands::Local { db }) => {
            let os = LocalFileSystem::new_with_prefix(db)?;
            let os = MetaStoreBuilder::new(os, 100000).build();
            Arc::new(os)
        }
        Some(Commands::Aws { bucket, region }) => {
            let os = AmazonS3Builder::from_env()
                .with_bucket_name(bucket)
                .with_region(region)
                .with_copy_if_not_exists(S3CopyIfNotExists::Multipart)
                .build()?;
            Arc::new(os)
        }
        None => Arc::new(InMemory::new()),
    };

    let db_config = DBConfig {
        name: "test".to_string(),
        description: "Anda Hippocampus database".to_string(),
        storage: StorageConfig {
            cache_max_capacity: 100000,
            compress_level: 3,
            object_chunk_size: 256 * 1024,
            bucket_overload_size: 1024 * 1024,
            max_small_object_size: 1024 * 1024 * 10,
        },
        lock: None,
    };

    let ed25519_pubkeys = cli
        .ed25519_pubkeys
        .split(',')
        .filter_map(|s| {
            ByteArrayB64::from_str(s)
                .ok()
                .and_then(|d| VerifyingKey::from_bytes(&d).ok())
        })
        .collect();

    let app_state = AppState::new(
        object_store,
        Arc::new(db_config),
        management.clone(),
        gemini_default,
        gemini_flash,
        ed25519_pubkeys,
        APP_NAME.to_string(),
        APP_VERSION.to_string(),
        cli.sharding_idx,
    );

    let app: Router<AppState> = Router::new()
        .route("/", routing::get(get_website))
        .route("/favicon.ico", routing::get(favicon))
        .route("/apple-touch-icon.webp", routing::get(apple_touch_icon))
        .route("/info", routing::get(get_information))
        .route("/SKILL.md", routing::get(get_skill))
        .route("/v1/{space_id}/status", routing::get(get_status))
        .route("/v1/{space_id}/formation", routing::post(post_formation))
        .route("/v1/{space_id}/recall", routing::post(post_recall))
        .route(
            "/v1/{space_id}/maintenance",
            routing::post(post_maintenance),
        )
        .route(
            "/v1/{space_id}/conversations/{conversation_id}",
            routing::get(get_conversation),
        )
        .route(
            "/v1/{space_id}/conversations",
            routing::get(list_conversations),
        )
        .route(
            "/v1/{space_id}/management/space_tokens",
            routing::get(list_space_tokens),
        )
        .route(
            "/v1/{space_id}/management/add_space_token",
            routing::post(add_space_token),
        )
        .route(
            "/v1/{space_id}/management/revoke_space_token",
            routing::post(revoke_space_token),
        )
        .route(
            "/v1/{space_id}/management/set_public",
            routing::post(set_public),
        )
        .route("/admin/create_space", routing::post(create_space))
        .route("/admin/update_space_tier", routing::post(update_space_tier))
        .layer(CompressionLayer::new());

    let app = app.with_state(app_state.clone());

    let addr: SocketAddr = cli.addr.parse()?;
    let listener = create_reuse_port_listener(addr).await?;
    let shutdown_token = global_cancel_token.clone();
    let server_handle = tokio::spawn(
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(shutdown_token))
            .into_future(),
    );

    let cancel_token = global_cancel_token.clone();
    let spaces_handle = tokio::spawn(async move {
        app_state.start_background_tasks(cancel_token).await;
    });

    log::warn!(
        "start service {}@{} on {:?}, sharding_idx: {}, managers: {}.",
        APP_NAME,
        APP_VERSION,
        addr,
        cli.sharding_idx,
        cli.managers
    );

    let _ = tokio::join!(server_handle, spaces_handle);
    Ok(())
}

async fn shutdown_signal(cancel_token: CancellationToken) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    log::warn!("received termination signal, starting graceful shutdown");
    cancel_token.cancel();
}

async fn create_reuse_port_listener(addr: SocketAddr) -> Result<tokio::net::TcpListener, BoxError> {
    let socket = match &addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };

    socket.set_reuseport(true)?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;
    Ok(listener)
}
