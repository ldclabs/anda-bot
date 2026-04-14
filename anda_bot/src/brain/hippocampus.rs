use anda_core::{BoxError, Principal};
use anda_db::{
    database::{AndaDB, DBConfig},
    storage::StorageConfig,
    unix_ms,
};
use anda_engine::{
    management::{BaseManagement, Visibility},
    model::Models,
};
use axum::{Router, routing};
use ic_cose_types::cose::ed25519::VerifyingKey;
use object_store::ObjectStore;
use std::{collections::BTreeSet, sync::Arc};

use crate::util::http_client::build_http_client;
use anda_hippocampus::{handler::*, model::build_model, space::AppState, types::ModelConfig};

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub static ANDA_BOT_SPACE_ID: &str = "anda_bot";

pub struct HippocampusConfig {
    pub ed25519_pubkey: [u8; 32],
    pub https_proxy: Option<String>,
    pub managers: BTreeSet<Principal>,
    pub model: ModelConfig,
}

pub struct Hippocampus {
    pub state: AppState,
    pub db: Arc<AndaDB>,
}

impl Hippocampus {
    pub async fn new(
        object_store: Arc<dyn ObjectStore>,
        cfg: HippocampusConfig,
    ) -> Result<Self, BoxError> {
        let http_client = build_http_client(cfg.https_proxy.clone(), |client| client)?;
        let management = Arc::new(BaseManagement {
            controller: Principal::management_canister(),
            managers: cfg.managers.clone(),
            visibility: Visibility::Protected,
        });

        // Configure AI model
        let models = Models::default();
        models.set_model(build_model(http_client.clone(), cfg.model.clone()));

        let db_config = DBConfig {
            name: "brain_db".to_string(),
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

        let ed25519_pubkeys = vec![VerifyingKey::from_bytes(&cfg.ed25519_pubkey)?];
        let app_state = AppState::new(
            object_store,
            Arc::new(db_config),
            management.clone(),
            http_client.clone(),
            Arc::new(models),
            ed25519_pubkeys,
            APP_NAME.to_string(),
            APP_VERSION.to_string(),
            0,
        );

        let space = match app_state.load_space(ANDA_BOT_SPACE_ID, true).await {
            Ok(space) => space,
            Err(e) => {
                if e.to_string().contains("not found") {
                    log::warn!(
                        name = "brain";
                        "Space '{}' not found, creating a new one",
                        ANDA_BOT_SPACE_ID
                    );
                    let admin = cfg
                        .managers
                        .iter()
                        .next()
                        .cloned()
                        .ok_or("At least one manager is required")?;
                    let _ = app_state
                        .admin_create_space(
                            admin,
                            admin,
                            ANDA_BOT_SPACE_ID.to_string(),
                            7,
                            unix_ms(),
                        )
                        .await?;
                    log::warn!(
                        name = "brain";
                        "Space '{}' created successfully",
                        ANDA_BOT_SPACE_ID
                    );
                    app_state.load_space(ANDA_BOT_SPACE_ID, true).await?
                } else {
                    return Err(e);
                }
            }
        };
        Ok(Self {
            state: app_state,
            db: space.db.clone(),
        })
    }

    pub fn into_router(self) -> Router<()> {
        let app: Router<()> = Router::new()
            .route("/", routing::get(get_information))
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
                "/v1/{space_id}/execute_kip_readonly",
                routing::post(execute_kip_readonly),
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
            .with_state(self.state);
        app
    }
}
