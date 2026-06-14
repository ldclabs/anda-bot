use anda_core::BoxError;
use anda_db::{database::DBConfig, storage::StorageConfig, unix_ms};
use anda_engine::{
    management::{BaseManagement, Visibility},
    model::Models,
};
use axum::{Router, routing};
use object_store::ObjectStore;
use std::sync::Arc;

use crate::{
    config,
    util::{http_client::build_http_client, key::Ed25519PubKey},
};
use anda_brain::{agents::SELF_USER_ID, handler::*, space::AppState};

pub struct BrainConfig {
    pub managers: Vec<Ed25519PubKey>,
    pub https_proxy: Option<String>,
    pub models: Arc<Models>,
}

pub struct Brain {
    pub state: AppState,
}

impl Brain {
    pub async fn new(
        object_store: Arc<dyn ObjectStore>,
        cfg: BrainConfig,
    ) -> Result<Self, BoxError> {
        let http_client = build_http_client(cfg.https_proxy.clone(), |client| client)?;
        let management = Arc::new(BaseManagement {
            controller: SELF_USER_ID,
            managers: cfg.managers.iter().map(|k| k.id()).collect(),
            visibility: Visibility::Protected,
        });

        let db_config = DBConfig {
            name: "brain_db".to_string(),
            description: "Anda Brain database".to_string(),
            storage: StorageConfig {
                cache_max_capacity: 100000,
                compress_level: 3,
                object_chunk_size: 256 * 1024,
                bucket_overload_size: 1024 * 1024,
                max_small_object_size: 1024 * 1024 * 10,
            },
            lock: None,
        };

        let admin = cfg
            .managers
            .first()
            .map(|k| k.id())
            .ok_or("At least one manager is required")?;
        let app_state = AppState::new(
            object_store,
            Arc::new(db_config),
            management.clone(),
            http_client.clone(),
            cfg.models,
            Arc::new(cfg.managers.into_iter().map(|k| k.into()).collect()),
            config::APP_NAME.to_string(),
            config::APP_VERSION.to_string(),
            0,
        );

        let _ = match app_state.load_space(config::ANDA_BOT_SPACE_ID, true).await {
            Ok(space) => space,
            Err(e) => {
                if e.to_string().contains("not found") {
                    log::warn!(
                        target: "brain",
                        name = "brain";
                        "Space '{}' not found, creating a new one",
                        config::ANDA_BOT_SPACE_ID
                    );

                    let _ = app_state
                        .admin_create_space(
                            admin,
                            admin,
                            config::ANDA_BOT_SPACE_ID.to_string(),
                            7,
                            unix_ms(),
                        )
                        .await?;
                    log::warn!(
                        target: "brain",
                        name = "brain";
                        "Space '{}' created successfully",
                        config::ANDA_BOT_SPACE_ID
                    );
                    app_state
                        .load_space(config::ANDA_BOT_SPACE_ID, true)
                        .await?
                } else {
                    return Err(e);
                }
            }
        };
        Ok(Self { state: app_state })
    }

    pub fn into_router(self) -> Router<()> {
        let app: Router<()> = Router::new()
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
                "/v1/{space_id}/get_or_init_user",
                routing::post(get_or_init_user),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;
    use crate::util::key::Ed25519Key;
    use anda_engine::model::ModelConfig;
    use object_store::memory::InMemory;

    fn brain_models() -> Arc<Models> {
        let models = Models::from_configs(
            &[ModelConfig {
                family: "openai".to_string(),
                model: "test-model".to_string(),
                api_base: "https://api.example.test/v1".to_string(),
                api_key: "sk-test".to_string(),
                ..Default::default()
            }],
            new_reqwest_client(),
        );
        let model = models.get("test-model").expect("test model");
        models.set_model(model);
        Arc::new(models)
    }

    #[tokio::test]
    async fn brain_creates_missing_space_and_builds_router() {
        let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let manager = Ed25519Key::new([9; 32]).pubkey();

        let brain = Brain::new(
            object_store,
            BrainConfig {
                managers: vec![manager],
                https_proxy: None,
                models: brain_models(),
            },
        )
        .await
        .unwrap();

        // The space was created on first load and the router registers the
        // public API routes without panicking.
        let _router = brain.into_router();
    }

    #[tokio::test]
    async fn brain_requires_at_least_one_manager() {
        let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());

        let err = Brain::new(
            object_store,
            BrainConfig {
                managers: Vec::new(),
                https_proxy: None,
                models: brain_models(),
            },
        )
        .await
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("At least one manager"));
    }
}
