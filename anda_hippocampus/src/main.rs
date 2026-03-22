use anda_core::{BoxError, Principal};
use anda_db::{database::DBConfig, storage::StorageConfig};
use anda_engine::{
    management::{BaseManagement, Visibility},
    model::{Models, Proxy, request_client_builder, reqwest},
};
use anda_object_store::MetaStoreBuilder;
use axum::{Router, routing};
use clap::{Parser, Subcommand};
use ic_auth_types::ByteBufB64;
use ic_cose_types::cose::{CborSerializable, CoseKey, ed25519::VerifyingKey, get_cose_key_public};
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
use tower_http::{
    compression::CompressionLayer,
    cors::{AllowHeaders, AllowMethods, CorsLayer},
};

mod agents;
mod handler;
mod model;
mod payload;
mod space;
mod types;

use handler::*;
use space::AppState;

use crate::{model::build_model, types::ModelConfig};

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

    /// AI model family (e.g., "gemini", "anthropic", "openai", "deepseek", "mimo")
    #[arg(long, env = "MODEL_FAMILY", default_value = "gemini")]
    model_family: String,

    /// AI model name (e.g., "gemini-3-flash-preview", "claude-sonnet-4-6")
    #[arg(long, env = "MODEL_NAME", default_value = "gemini-3-flash-preview")]
    model_name: String,

    /// API key for AI model
    #[arg(long, env = "MODEL_API_KEY", default_value = "")]
    model_api_key: String,

    /// API base URL for AI model
    #[arg(
        long,
        env = "MODEL_API_BASE",
        default_value = "https://generativelanguage.googleapis.com/v1beta/models"
    )]
    model_api_base: String,

    /// Optional HTTPS proxy URL (e.g., "http://localhost:8080")
    #[arg(long, env = "HTTPS_PROXY")]
    https_proxy: Option<String>,

    #[arg(long, env = "SHARDING_IDX", default_value_t = 0)]
    sharding_idx: u32,

    /// Manager principal IDs, separated by comma
    #[arg(long, env = "MANAGERS", default_value = "")]
    managers: String,

    /// CORS allowed origins, separated by comma. Use "*" to allow all origins.
    #[arg(long, env = "CORS_ORIGINS", default_value = "")]
    cors_origins: String,

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
    let models = Models::default();
    models.set_model(build_model(
        http_client.clone(),
        ModelConfig {
            family: cli.model_family.clone(),
            model: cli.model_name.clone(),
            api_key: cli.model_api_key.clone(),
            api_base: cli.model_api_base.clone(),
            disabled: cli.model_api_key.is_empty(),
        },
    ));

    let mut db_type = "memory".to_string();
    let object_store: Arc<dyn ObjectStore> = match cli.command {
        Some(Commands::Local { db }) => {
            db_type = "local".to_string();
            let os = LocalFileSystem::new_with_prefix(db)?;
            let os = MetaStoreBuilder::new(os, 100000).build();
            Arc::new(os)
        }
        Some(Commands::Aws { bucket, region }) => {
            db_type = "aws".to_string();
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

    let ed25519_pubkeys = parse_ed25519_pubkeys(&cli.ed25519_pubkeys);
    if ed25519_pubkeys.len() != cli.ed25519_pubkeys.split(',').count() {
        return Err("some ED25519_PUBKEYS entries are invalid".into());
    }

    let app_state = AppState::new(
        object_store,
        Arc::new(db_config),
        management.clone(),
        http_client.clone(),
        Arc::new(models),
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
        .route("/v1/{space_id}/info", routing::get(get_info))
        .route("/v1/{space_id}/status", routing::get(get_info))
        .route(
            "/v1/{space_id}/formation_status",
            routing::get(get_formation_status),
        )
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
            "/v1/{space_id}/management/update_space",
            routing::patch(update_space),
        )
        .route(
            "/v1/{space_id}/management/restart_formation",
            routing::patch(restart_formation),
        )
        .route(
            "/v1/{space_id}/management/space_byok",
            routing::patch(update_byok),
        )
        .route(
            "/v1/{space_id}/management/space_byok",
            routing::get(get_byok),
        )
        .route(
            "/admin/{space_id}/update_space_tier",
            routing::post(update_space_tier),
        )
        .route("/admin/create_space", routing::post(create_space))
        .layer(CompressionLayer::new());

    // Configure CORS
    let cors = if cli.cors_origins.is_empty() {
        CorsLayer::new()
    } else if cli.cors_origins.trim() == "*" {
        CorsLayer::very_permissive()
    } else {
        let origins: Vec<http::HeaderValue> = cli
            .cors_origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_credentials(true)
            .max_age(Duration::from_secs(86400))
            .allow_headers(AllowHeaders::mirror_request())
            .allow_methods(AllowMethods::mirror_request())
    };
    let app = app.layer(cors);
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
        "start service {}@{} on {:?}, sharding: {}, managers: {}, DB type: {}, Model: {}.",
        APP_NAME,
        APP_VERSION,
        addr,
        cli.sharding_idx,
        cli.managers,
        db_type,
        cli.model_name
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

fn parse_ed25519_pubkeys(input: &str) -> Vec<VerifyingKey> {
    input
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }

            match parse_ed25519_pubkey(item) {
                Some(key) => Some(key),
                None => {
                    log::warn!("ignore invalid ED25519_PUBKEYS entry");
                    None
                }
            }
        })
        .collect()
}

fn parse_ed25519_pubkey(input: &str) -> Option<VerifyingKey> {
    let data = ByteBufB64::from_str(input).ok()?;

    if data.len() == 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data);
        return VerifyingKey::from_bytes(&bytes).ok();
    }

    let cose_key = CoseKey::from_slice(data.as_slice()).ok()?;
    let public_key = get_cose_key_public(cose_key).ok()?;
    let bytes: [u8; 32] = public_key.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

async fn create_reuse_port_listener(addr: SocketAddr) -> Result<tokio::net::TcpListener, BoxError> {
    let socket = match &addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };

    #[cfg(unix)]
    let _ = socket.set_reuseport(true);

    socket.bind(addr)?;
    let listener = socket.listen(1024)?;
    Ok(listener)
}
