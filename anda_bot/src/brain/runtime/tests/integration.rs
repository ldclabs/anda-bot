use super::*;
use crate::brain::{
    AttentionQuery, AttentionResponse, Client, Host, HttpError, RuntimeOperation, RuntimeTool,
};
use crate::{
    engine::SessionRequestMeta,
    util::{request_meta::keys as meta_keys, tool_response::ToolResponse},
};
use anda_brain::{consequence::*, runtime_api::config::RuntimeConfig};
use anda_core::{RequestMeta, Tool};
use anda_kip::{Executor, Request};
use serde_json::{Value, json};

const READER: &str = "kip:principal:bot-reader";
const OTHER: &str = "kip:principal:bot-other";
const OBSERVER: &str = "kip:principal:bot-observer";

fn runtime_config(keys: &[Ed25519Key], question: Option<&str>) -> RuntimeConfig {
    serde_json::from_value(json!({"format":"anda-brain:runtime-api-v1","spaces":{"anda_bot":{
        "bootstrap":true,
        "subjects": keys.iter().zip([READER, OTHER, OBSERVER]).map(|(key, principal)| json!({"credential":{"kind":"cwt_subject","subject":key.id().to_text()},"principal":principal,"observer":principal==OBSERVER})).collect::<Vec<_>>(),
        "audience":[READER, OTHER, OBSERVER],
        "observers":[contract()],
        "adapter":{"id":"attention_inbox_v1","controller_principal":"kip:principal:bot-controller","recipient_principal":READER,"message":"Local memory reminder","question":question,"reply_timeout_ms":60000,"context":null}
    }}})).unwrap()
}
fn contract() -> ObserverContract {
    ObserverContract {
        principal_id: OBSERVER.into(),
        configuration_digest: anda_cognitive_nexus::content_digest(
            &json!({"instrument":"durable-readback-test"}),
        )
        .unwrap(),
        control_domain: "independent-test-instrument".into(),
        task_family: "memory.attention.v1".into(),
        metric: "delivery".into(),
        window: "durable_inbox_v1".into(),
        maximum_delay_ms: 3600000,
    }
}
fn token(key: &Ed25519Key) -> String {
    let mut claims = crate::identity::expiring_claims(std::time::Duration::from_secs(60)).unwrap();
    claims.audience = Some("*".into());
    claims
        .extra
        .insert(crate::identity::iana::CWTClaimScope, "*");
    key.sign_cwt(claims).unwrap()
}
async fn create(
    store: Arc<dyn ObjectStore>,
    keys: &[Ed25519Key],
    config: Option<RuntimeConfig>,
) -> Brain {
    Brain::new(
        store,
        BrainConfig {
            managers: keys.iter().map(Ed25519Key::pubkey).collect(),
            models: brain_models(),
            https_proxy: None,
            runtime_config: config,
        },
    )
    .await
    .unwrap()
}
async fn command(space: &anda_brain::space::Space, text: &str, parameters: Value) -> Value {
    let mut request = Request::single(text);
    request.parameters = Some(parameters.as_object().unwrap().clone());
    let nexus = space.memory_runtime().unwrap().nexus();
    let response = nexus
        .system_session()
        .execute(
            anda_kip::parse_kip(text).unwrap(),
            &request,
            &request.operations[0],
        )
        .await;
    assert_eq!(
        response.status,
        anda_kip::TopLevelStatus::Succeeded,
        "{response:?}"
    );
    response.first_result().unwrap().clone()
}
async fn fire(space: &anda_brain::space::Space) {
    let target = command(
        space,
        r#"CREATE CONCEPT ?item {TYPE "Person" NAME "before"}"#,
        json!({}),
    )
    .await["handles"]["item"]
        .as_str()
        .unwrap()
        .to_string();
    command(space, r#"MUTATE {CREATE CONCEPT ?p {TYPE "Preference" NAME "coordinate"} ENSURE PROPOSITION ?item (:target,"prefers",?p)}"#, json!({"target":target})).await;
    let watch = command(space, r#"CREATE CONCEPT ?item {TYPE "Watch" SET ATTRIBUTES {watch_class:"delta",summary:"Bot retained reminder",status:"disarmed",condition:{element: :target}}}"#, json!({"target":target})).await["handles"]["item"].as_str().unwrap().to_string();
    space.attention().arm_watch(watch, 1).await.unwrap();
    command(
        space,
        r#"UPDATE :id SET FIELDS {name:"after"}"#,
        json!({"id":target}),
    )
    .await;
}
async fn delivery(
    space: &anda_brain::space::Space,
    client: &Client,
) -> anda_brain::runtime_api::AttentionItem {
    for _ in 0..20 {
        let report = space.attention().tick().await.unwrap();
        assert!(report.error.is_none(), "{report:?}");
        if let Some(item) = client
            .attention(&AttentionQuery::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.delivery.is_some() && i.attempt_ref.is_some())
        {
            return item;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("missing native inbox delivery")
}
fn status(error: BoxError) -> http::StatusCode {
    error.downcast_ref::<HttpError>().unwrap().status
}

#[tokio::test]
async fn configured_inbox_signed_callers_independent_outcomes_and_restart() {
    let keys = [
        Ed25519Key::new([51; 32]),
        Ed25519Key::new([52; 32]),
        Ed25519Key::new([53; 32]),
    ];
    let config = runtime_config(&keys, None);
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let brain = create(store.clone(), &keys, Some(config.clone())).await;
    let host = Host::new(brain.state.clone(), Some(&config)).unwrap();
    for operation in [
        RuntimeOperation::Attention,
        RuntimeOperation::Respond,
        RuntimeOperation::Status,
    ] {
        crate::util::json_schema::assert_openai_strict_parameters(
            &RuntimeTool::new(host.clone(), operation)
                .definition()
                .parameters,
        );
    }
    let trusted_ctx = anda_engine::engine::EngineBuilder::new()
        .mock_ctx()
        .base
        .with_caller(keys[0].id());
    let output = RuntimeTool::new(host.clone(), RuntimeOperation::Status)
        .call(trusted_ctx, json!({}), vec![])
        .await
        .unwrap();
    assert!(matches!(output.output, ToolResponse::Ok { .. }));

    let external_ctx = anda_engine::engine::EngineBuilder::new()
        .mock_ctx()
        .base
        .with_caller(keys[0].id());
    let mut live_meta = RequestMeta::default();
    live_meta
        .extra
        .insert(meta_keys::EXTERNAL_USER.into(), true.into());
    external_ctx.set_state(SessionRequestMeta::new(live_meta));
    assert!(
        RuntimeTool::new(host.clone(), RuntimeOperation::Status)
            .call(external_ctx, json!({}), vec![])
            .await
            .unwrap_err()
            .to_string()
            .contains("external users")
    );
    let space = brain.state.load_space("anda_bot", true).await.unwrap();
    let url = crate::test_support::spawn_http_mock(brain.into_router()).await;
    let reader = Client::new(format!("{url}/v1/anda_bot"), Some(token(&keys[0])));
    let other = reader.with_auth_token(token(&keys[1]));
    let observer = reader.with_auth_token(token(&keys[2]));
    let anonymous = reader.with_auth_token(String::new());
    assert!(reader.runtime_status().await.unwrap().configured);
    assert_eq!(
        status(anonymous.runtime_status().await.unwrap_err()),
        http::StatusCode::UNAUTHORIZED
    );
    assert!(host.status(keys[0].id()).await.unwrap().configured);
    assert!(host.status(Ed25519Key::new([54; 32]).id()).await.is_err());
    fire(&space).await;
    let item = delivery(&space, &reader).await;
    assert!(
        other
            .attention(&AttentionQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(other.runtime_status().await.unwrap().visible_items, 0);
    let page = reader
        .attention(&AttentionQuery {
            limit: Some(1),
            cursor: None,
        })
        .await
        .unwrap();
    let cursor = page
        .next_cursor
        .expect("parent and delivery are separately visible native items");
    assert!(
        other
            .attention(&AttentionQuery {
                limit: Some(1),
                cursor: Some(cursor)
            })
            .await
            .is_err()
    );
    let statement = AttentionResponse::AgentStatement {
        event_key: "statement-1".into(),
        statement: "I saw the reminder".into(),
    };
    assert_eq!(
        status(other.respond(&item.id, &statement).await.unwrap_err()),
        http::StatusCode::NOT_FOUND
    );
    let receipt = reader.respond(&item.id, &statement).await.unwrap();
    assert_eq!(
        reader
            .respond(&item.id, &statement)
            .await
            .unwrap()
            .receipt_id,
        receipt.receipt_id
    );
    let conflict = AttentionResponse::AgentStatement {
        event_key: "statement-1".into(),
        statement: "different".into(),
    };
    assert_eq!(
        status(reader.respond(&item.id, &conflict).await.unwrap_err()),
        http::StatusCode::CONFLICT
    );
    let runtime = space.memory_runtime().unwrap();
    let input = OutcomeInput {
        utility: None,
        space_instance: runtime.scope().space_instance.clone(),
        attempt_ref: item.attempt_ref.clone().unwrap(),
        observer_configuration_digest: contract().configuration_digest,
        event_key: "observed-delivery-1".into(),
        observed_at: anda_cognitive_nexus::time::now(),
        metric: "delivery".into(),
        window: "durable_inbox_v1".into(),
        observation: Observation::Measurement {
            terminal: true,
            outcome_status: OutcomeStatus::Success,
            magnitude: None,
            payload: json!({"delivery_digest":item.delivery.as_ref().unwrap()["request_digest"]}),
        },
        correction_of: None,
        safety_signal: None,
    };
    assert_eq!(
        status(reader.submit_outcome(&input).await.unwrap_err()),
        http::StatusCode::FORBIDDEN
    );
    let outcome = observer.submit_outcome(&input).await.unwrap();
    assert!(outcome.native_committed);
    assert!(!outcome.learning_eligible);
    assert_eq!(
        observer.submit_outcome(&input).await.unwrap().outcome_ref,
        outcome.outcome_ref
    );
    let nexus = runtime.nexus();
    let grant = nexus
        .governance()
        .grants_for(anda_cognitive_nexus::nexus::DEFAULT_SPACE, OBSERVER, &[])
        .await
        .unwrap()
        .into_iter()
        .find(|g| g.actions.contains(&"record_outcome".into()))
        .unwrap();
    nexus
        .system_session()
        .revoke_grant(anda_cognitive_nexus::nexus::DEFAULT_SPACE, grant._id)
        .await
        .unwrap();
    assert_eq!(
        status(observer.submit_outcome(&input).await.unwrap_err()),
        http::StatusCode::FORBIDDEN
    );
    space.close().await.unwrap();
    drop(host);
    drop(space);
    drop(runtime);
    let restarted = create(store, &keys, Some(config)).await;
    let space = restarted.state.load_space("anda_bot", true).await.unwrap();
    let url = crate::test_support::spawn_http_mock(restarted.into_router()).await;
    let reader = Client::new(format!("{url}/v1/anda_bot"), Some(token(&keys[0])));
    let observer = reader.with_auth_token(token(&keys[2]));
    assert_eq!(
        status(observer.submit_outcome(&input).await.unwrap_err()),
        http::StatusCode::FORBIDDEN
    );

    for _ in 0..3 {
        space.attention().tick().await.unwrap();
    }
    let page = reader.attention(&AttentionQuery::default()).await.unwrap();
    assert_eq!(page.items.iter().filter(|i| i.id == item.id).count(), 1);
    assert_eq!(
        reader
            .respond(&item.id, &statement)
            .await
            .unwrap()
            .receipt_id,
        receipt.receipt_id
    );
    space.close().await.unwrap();
}

#[tokio::test]
async fn unconfigured_routes_are_present_and_never_anonymous_runtime() {
    let keys = [Ed25519Key::new([61; 32])];
    let brain = create(Arc::new(InMemory::new()), &keys, None).await;
    let space = brain.state.load_space("anda_bot", true).await.unwrap();
    let url = crate::test_support::spawn_http_mock(brain.into_router()).await;
    let client = Client::new(format!("{url}/v1/anda_bot"), Some(token(&keys[0])));
    let state = client.runtime_status().await.unwrap();
    assert!(!state.configured && !state.actions_enabled);
    assert_eq!(state.learning["compiled"], cfg!(feature = "learning"));
    assert_eq!(
        status(
            client
                .attention(&AttentionQuery::default())
                .await
                .unwrap_err()
        ),
        http::StatusCode::SERVICE_UNAVAILABLE
    );
    let http = new_reqwest_client();
    for path in [
        "attention/unknown/responses",
        "outcomes",
        "recall_structured",
    ] {
        let response = http
            .post(format!("{url}/v1/anda_bot/{path}"))
            .bearer_auth(token(&keys[0]))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            http::StatusCode::NOT_FOUND,
            "missing route {path}"
        );
    }
    space.close().await.unwrap();
}

#[tokio::test]
async fn startup_rejects_unsupported_channel_adapters_and_missing_secrets() {
    let keys = [
        Ed25519Key::new([71; 32]),
        Ed25519Key::new([72; 32]),
        Ed25519Key::new([73; 32]),
    ];
    let config = runtime_config(&keys, None);
    for variant in ["adapter", "secret", "principal"] {
        let mut config = config.clone();
        let space = config.spaces.get_mut("anda_bot").unwrap();
        match variant {
            "adapter" => space.adapter.as_mut().unwrap().id = "telegram_send_result_unit".into(),
            "secret" => {
                space.subjects[0].credential =
                    anda_brain::runtime_api::config::ConfigCredential::SpaceTokenEnv {
                        variable: "ANDA_BOT_TEST_UNSET_SECRET".into(),
                    }
            }
            _ => space.adapter.as_mut().unwrap().controller_principal = READER.into(),
        }
        let result = Brain::new(
            Arc::new(InMemory::new()),
            BrainConfig {
                managers: keys.iter().map(Ed25519Key::pubkey).collect(),
                models: brain_models(),
                https_proxy: None,
                runtime_config: Some(config),
            },
        )
        .await;
        assert!(result.is_err(), "{variant}");
    }
}

#[cfg(feature = "learning")]
#[tokio::test]
async fn learning_http_bindings_probe_separate_services_but_do_not_run_without_switches() {
    use anda_brain::learning::workflow_http::WorkflowCapabilities;
    use axum::{Json, Router, extract::State, routing};
    let key = Ed25519Key::new([81; 32]);
    let mut config: RuntimeConfig = serde_json::from_str(include_str!(
        "../../../../assets/brain-learning.example.json"
    ))
    .unwrap();
    let space = config.spaces.get_mut("anda_bot").unwrap();
    space.bootstrap = true;
    space.subjects[0].credential = anda_brain::runtime_api::config::ConfigCredential::CwtSubject {
        subject: key.id().to_string(),
    };
    let http_config: anda_brain::learning::workflow_http::WorkflowHttpConfig =
        serde_json::from_value(space.learning.clone().unwrap()).unwrap();
    let capabilities = json!(WorkflowCapabilities {
        format: "anda-brain:workflow-http-v1".into(),
        identity: http_config.identity,
        reset_isolated: true,
        request_idempotency: true,
        authoritative_status: true,
        fence_and_deadline_enforced: true,
        cancellation: true,
        complete_instrumented_journal: true,
        calibration_digest: http_config.registration.calibration_digest,
    });
    let observer = json!(http_config.registration.observer);
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let server = Router::new().route("/{role}/{op}", routing::any(move |State(seen): State<Arc<std::sync::Mutex<Vec<String>>>>, axum::extract::Path((role,op)):axum::extract::Path<(String,String)>, headers:http::HeaderMap| {
        let capabilities = capabilities.clone(); let observer = observer.clone();
        async move {
            assert_eq!(headers["authorization"], format!("Bearer test-{role}"));
            seen.lock().unwrap().push(format!("{role}/{op}"));
            match (role.as_str(),op.as_str()) {
                ("executor","capabilities") => Json(capabilities),
                ("observer","identity") => Json(observer),
                _ => panic!("disabled learning must not execute, observe, or enroll: {role}/{op}"),
            }
        }
    })).with_state(seen.clone());
    let url = crate::test_support::spawn_http_mock(server).await;
    let learning = space.learning.as_mut().unwrap();
    for role in ["executor", "observer", "source"] {
        learning[format!("{role}_endpoint")] = json!(format!("{url}/{role}/"));
    }
    let brain = Brain::new_with_secrets(
        Arc::new(InMemory::new()),
        BrainConfig {
            managers: vec![key.pubkey()],
            models: brain_models(),
            https_proxy: None,
            runtime_config: Some(config),
        },
        |name| match name {
            "WORKFLOW_EXECUTOR_TOKEN" => Some("test-executor".into()),
            "WORKFLOW_OBSERVER_TOKEN" => Some("test-observer".into()),
            "WORKFLOW_SOURCE_TOKEN" => Some("test-source".into()),
            _ => None,
        },
    )
    .await
    .unwrap();
    let space = brain.state.load_space("anda_bot", true).await.unwrap();
    let url = crate::test_support::spawn_http_mock(brain.into_router()).await;
    let status = Client::new(format!("{url}/v1/anda_bot"), Some(token(&key)))
        .runtime_status()
        .await
        .unwrap();
    assert_eq!(status.learning["compiled"], true);
    assert_eq!(status.learning["bindings_ready"], true);
    assert_eq!(status.learning["automation"]["trials"], false);
    assert!(space.learning().jobs().await.unwrap().is_empty());
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|s| s == "executor/capabilities")
    );
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|s| s == "observer/identity")
    );
    space.close().await.unwrap();
}

#[cfg(not(feature = "learning"))]
#[tokio::test]
async fn lean_build_rejects_learning_configuration() {
    let config = serde_json::from_str(include_str!(
        "../../../../assets/brain-learning.example.json"
    ))
    .unwrap();
    let result = Brain::new(
        Arc::new(InMemory::new()),
        BrainConfig {
            managers: vec![Ed25519Key::new([82; 32]).pubkey()],
            models: brain_models(),
            https_proxy: None,
            runtime_config: Some(config),
        },
    )
    .await;
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("learning Cargo feature")
    );
}

#[tokio::test]
async fn real_structured_recall_and_tool_keep_off_graph_delivery_association() {
    use anda_core::Tool;
    use futures::TryStreamExt;
    use object_store::path::Path;
    let key = Ed25519Key::new([91; 32]);
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let models = Arc::new(Models::default());
    models.set_model(anda_engine::model::Model::mock_implemented());
    let brain = Brain::new(
        store.clone(),
        BrainConfig {
            managers: vec![key.pubkey()],
            models,
            https_proxy: None,
            runtime_config: None,
        },
    )
    .await
    .unwrap();
    let space = brain.state.load_space("anda_bot", true).await.unwrap();
    let host = Host::new(brain.state.clone(), None).unwrap();
    let url = crate::test_support::spawn_http_mock(brain.into_router()).await;
    let journal = crate::brain::Journal::new(store.clone());
    let client = Client::new(format!("{url}/v1/anda_bot"), Some(token(&key)))
        .with_host(host, journal.clone());
    let input = crate::brain::RecallInput {
        query: "What do you remember?".into(),
        context: None,
        budget: None,
    };
    let structured = client.recall_structured(&input).await.unwrap();
    assert!(structured.conversation.is_some());
    assert!(structured.recall_receipt.is_some());
    let ctx = anda_engine::engine::EngineBuilder::new()
        .mock_ctx()
        .base
        .with_caller(key.id());
    let trace = crate::brain::RecallTurn::default();
    trace.prepare(
        42,
        &[anda_core::ToolCall {
            name: Client::NAME.into(),
            args: json!(input),
            call_id: Some("recall-call-1".into()),
            ..Default::default()
        }],
    );
    ctx.set_state(trace);
    let output = client.call(ctx, input, vec![]).await.unwrap();
    assert_ne!(output.is_error, Some(true), "{}", output.output);
    assert!(output.artifacts.is_empty());
    let rows: Vec<_> = store
        .list(Some(&Path::from("bot-brain/v1/recall")))
        .try_collect()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let invocation = rows[0].location.filename().unwrap();
    let saved: crate::brain::RecallDelivery = journal
        .read(&format!("recall/{invocation}"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.bot_conversation, Some(42));
    assert_eq!(saved.tool_call.as_deref(), Some("recall-call-1"));
    assert!(saved.receipt.is_some());
    assert!(!saved.accounting_complete);
    let budget = anda_brain::recall_budget::RecallBudget {
        max_tokens: 32,
        ..Default::default()
    };
    let structured = client
        .recall_structured(&crate::brain::RecallInput {
            query: "Recall within a small budget".into(),
            context: None,
            budget: Some(budget),
        })
        .await
        .unwrap();
    assert_eq!(structured.memory_budget.unwrap().token_limit, 32);
    assert!(anda_brain::recall_budget::count(&structured.answer).unwrap() <= 32);
    space.close().await.unwrap();
}
