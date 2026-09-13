use anda_core::BoxError;
use anda_db::{database::DBConfig, storage::StorageConfig, unix_ms};
use anda_engine::{
    management::{BaseManagement, Visibility},
    model::Models,
};
use axum::{Router, routing};
use object_store::ObjectStore;
use std::sync::Arc;

use crate::{config, identity::Ed25519PubKey, util::http_client::build_http_client};
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
                cache_max_bytes: None,
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
    use crate::identity::Ed25519Key;
    use crate::util::http_client::new_reqwest_client;
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
    async fn embedded_brain_kip2_client_and_graph_queries_use_real_nexus_shapes() {
        let key = Ed25519Key::new([17; 32]);
        let brain = Brain::new(
            Arc::new(InMemory::new()),
            BrainConfig {
                managers: vec![key.pubkey()],
                https_proxy: None,
                models: brain_models(),
            },
        )
        .await
        .unwrap();
        let mut claims =
            crate::identity::expiring_claims(std::time::Duration::from_secs(60)).unwrap();
        claims.audience = Some("*".into());
        claims
            .extra
            .insert(crate::identity::iana::CWTClaimScope, "*");
        let token = key.sign_cwt(claims).unwrap();
        let space = brain
            .state
            .load_space(config::ANDA_BOT_SPACE_ID, true)
            .await
            .unwrap();
        let base_url = crate::test_support::spawn_http_mock(brain.into_router()).await;
        let client = crate::brain::Client::new(format!("{base_url}/v1/anda_bot"), Some(token));
        let primer = client.describe_primer().await.unwrap();
        assert!(primer.get("cognitive_identity").is_some());
        let person = client
            .user_info("contract-person".into(), Some("Contract Person".into()))
            .await
            .unwrap();
        assert_eq!(person["kind"], "concept");
        assert!(person["schema_ref"].as_str().unwrap().ends_with("/Person"));
        let id = person["id"].as_str().unwrap();
        // The browser submits these together. Single-operation checks cannot
        // catch a missing mandatory execution mode on a real KIP 2.0 batch.
        let mut batch = serde_json::json!({
            "kip": "2.0",
            "operations": [
                {"command": "LIST TYPES LIMIT 500"},
                {"command": "LIST PREDICATES LIMIT 500"}
            ]
        });
        let invalid: anda_kip::Request = serde_json::from_value(batch.clone()).unwrap();
        assert!(invalid.validate().is_err());
        batch["execution"] = serde_json::json!({"mode": "independent"});
        let valid: anda_kip::Request = serde_json::from_value(batch).unwrap();
        valid.validate().unwrap();
        let response = client.execute_kip_readonly(valid).await.unwrap();
        assert_eq!(response.status, anda_kip::TopLevelStatus::Succeeded);
        assert_eq!(response.results.len(), 2);
        for result in response.results {
            assert_eq!(result.status, anda_kip::OperationStatus::Succeeded);
            assert!(result.result.unwrap().is_array());
        }
        for command in [
            "LIST TYPES LIMIT 500".to_string(),
            "LIST PREDICATES LIMIT 500".to_string(),
            r#"FIND(?node) WHERE { ?node CONCEPT {type: "Person"} } LIMIT 12"#.to_string(),
            format!(
                r#"FIND(?link, ?o) WHERE {{ ?node CONCEPT {{id: "{id}"}} ?link (?node, ?predicate, ?o) }} LIMIT 180"#
            ),
            format!(
                r#"FIND(?s, ?link) WHERE {{ ?node CONCEPT {{id: "{id}"}} ?link (?s, ?predicate, ?node) }} LIMIT 180"#
            ),
            r#"SEARCH CONCEPT "Contract Person" LIMIT 32"#.to_string(),
        ] {
            let response = client
                .execute_kip_readonly(anda_kip::Request::single(&command))
                .await
                .unwrap();
            assert_eq!(
                response.status,
                anda_kip::TopLevelStatus::Succeeded,
                "{command}: {response:?}"
            );
            assert_eq!(
                response.results[0].status,
                anda_kip::OperationStatus::Succeeded
            );
            let value = response.first_result().unwrap();
            if command.starts_with("LIST") {
                assert!(
                    value
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|row| row["ref"].is_string() && row["local_name"].is_string())
                );
            } else if command.starts_with("SEARCH") {
                assert!(value["hits"].is_array());
                assert!(
                    value["hits"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|hit| hit["element"]["id"] == id)
                );
                assert!(value.get("search_context").is_some());
            }
        }
        space.close().await.unwrap();
    }

    #[tokio::test]
    async fn nonempty_graph_expansion_projects_real_nexus_tuples() {
        use anda_cognitive_nexus::{
            CognitiveNexus,
            nexus::DEFAULT_SPACE,
            schema::{PackageState, SchemaLock, SchemaPackage},
        };
        use anda_kip::{Executor, Request, TopLevelStatus};
        let db = crate::test_support::memory_db("graph_wire_contract").await;
        let nexus = CognitiveNexus::connect(db.clone()).await.unwrap();
        nexus
            .install_package(
                &SchemaPackage::parse(anda_cognitive_nexus::profiles::COGNITIVE_MEMORY).unwrap(),
                "test",
            )
            .await
            .unwrap();
        let mut lock = SchemaLock::default();
        lock.packages
            .insert("kip://profiles/cognitive-memory".into(), "2.1.0".into());
        lock.states.insert(
            "kip://profiles/cognitive-memory".into(),
            PackageState::Active,
        );
        nexus.activate_schema(DEFAULT_SPACE, lock).await.unwrap();
        let commands = [
            r#"MUTATE {
                CREATE CONCEPT ?person { TYPE "Person" NAME "Graph Person" }
                CREATE CONCEPT ?preference { TYPE "Preference" NAME "Dark mode" }
                ENSURE PROPOSITION ?p (?person, "prefers", ?preference)
            }"#,
            r#"FIND(?link, ?o) WHERE { ?node CONCEPT {name: "Graph Person"} ?link (?node, ?predicate, ?o) } LIMIT 180"#,
            r#"FIND(?s, ?link) WHERE { ?node CONCEPT {name: "Dark mode"} ?link (?s, ?predicate, ?node) } LIMIT 180"#,
        ];
        for (index, command) in commands.iter().enumerate() {
            let request = Request::single(*command);
            let response = nexus
                .execute(
                    anda_kip::parse_kip(command).unwrap(),
                    &request,
                    &request.operations[0],
                )
                .await;
            assert_eq!(
                response.status,
                TopLevelStatus::Succeeded,
                "{command}: {response:?}"
            );
            if index > 0 {
                let rows = response.first_result().unwrap().as_array().unwrap();
                assert_eq!(rows.len(), 1);
                let tuple = rows[0].as_array().unwrap();
                assert_eq!(tuple.len(), 2);
                let proposition = &tuple[if index == 1 { 0 } else { 1 }];
                let concept = &tuple[if index == 1 { 1 } else { 0 }];
                assert_eq!(proposition["kind"], "proposition");
                assert!(
                    proposition["predicate_ref"]
                        .as_str()
                        .unwrap()
                        .ends_with("/prefers")
                );
                assert!(proposition["subject"]["id"].is_string());
                assert!(proposition["object"]["id"].is_string());
                assert_eq!(concept["kind"], "concept");
                assert!(concept["schema_ref"].is_string());
            }
        }
        db.close().await.unwrap();
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
