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

#[tokio::test]
async fn memory_record_watch_is_recipient_scoped_and_retries_never_rearm() {
    let keys = [
        Ed25519Key::new([87; 32]),
        Ed25519Key::new([88; 32]),
        Ed25519Key::new([89; 32]),
    ];
    let config = runtime_config(&keys, Some("Does this need an update?"));
    let brain = create(Arc::new(InMemory::new()), &keys, Some(config.clone())).await;
    let host = Host::new(brain.state.clone(), Some(&config)).unwrap();
    let space = brain.state.load_space("anda_bot", true).await.unwrap();
    let result=command(&space,r#"MUTATE {CREATE CONCEPT ?p {TYPE "Person" NAME "Owner"} CREATE CONCEPT ?v {TYPE "Preference" NAME "Brief release notes"} ASSERT ?a (?p,"prefers",?v) {by:?p,mode:"stated"}}"#,json!({})).await;
    let target = result["handles"]["a"].as_str().unwrap().to_string();
    assert!(
        host.watch_record(
            keys[1].id(),
            "watch-other".into(),
            target.clone(),
            "Private memory update".into()
        )
        .await
        .is_err()
    );
    let created = host
        .watch_record(
            keys[0].id(),
            "watch-owner".into(),
            target.clone(),
            "Private memory update".into(),
        )
        .await
        .unwrap();
    assert_eq!(created.state, "armed");
    let cancelled = host
        .cancel_record_watch(keys[0].id(), "watch-owner".into())
        .await
        .unwrap();
    assert_eq!(cancelled.state, "cancelled");
    let retried = host
        .watch_record(
            keys[0].id(),
            "watch-owner".into(),
            target.clone(),
            "Private memory update".into(),
        )
        .await
        .unwrap();
    assert_eq!(retried.state, "cancelled");
    assert_eq!(created.watch_id, retried.watch_id);
    let second = host
        .watch_record(
            keys[0].id(),
            "watch-new".into(),
            target.clone(),
            "Memory changed".into(),
        )
        .await
        .unwrap();
    assert_eq!(second.state, "armed");
    let url = crate::test_support::spawn_http_mock(brain.into_router()).await;
    let client = Client::new(format!("{url}/v1/anda_bot"), Some(token(&keys[0])));
    command(
        &space,
        "TRANSITION :id TO \"retracted\"",
        json!({"id":target}),
    )
    .await;
    let _delivery = delivery(&space, &client).await;
    let page = client.attention(&AttentionQuery::default()).await.unwrap();
    assert!(
        page.items
            .iter()
            .all(|item| item.watch_ref != created.watch_id)
    );
    let question = page
        .items
        .iter()
        .find(|item| item.clarification.is_some())
        .expect("parent item retains the committed question");
    let response = AttentionResponse::Clarification {
        event_key: "product-watch-answer".into(),
        answer: "Please update the preference".into(),
    };
    let receipt = client.respond(&question.id, &response).await.unwrap();
    assert_eq!(
        client
            .respond(&question.id, &response)
            .await
            .unwrap()
            .receipt_id,
        receipt.receipt_id
    );
    space.close().await.unwrap();
}

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

#[tokio::test]
async fn memory_product_mutations_authorize_sources_keep_intents_and_clear_bot_notes() {
    use crate::brain::{
        FormationProvenance, FormationState, FormationSubmission, Journal, MemoryAccess,
        MemoryService, SourceMessageRef,
        activity::ActivityStore,
        mutation::{ChangeRequest, CommitRequest, MutationService},
    };
    use anda_core::{Agent, AgentOutput, Message, RequestMeta};
    use anda_engine::{
        context::AgentCtx,
        engine::{EngineBuilder, EngineRef},
        extension::note::{NoteArgs, NoteTool, load_notes},
        memory::{Conversation, ConversationRef},
    };
    struct Stub;
    impl Agent<AgentCtx> for Stub {
        fn name(&self) -> String {
            crate::engine::AndaBot::NAME.into()
        }
        fn description(&self) -> String {
            "Fixture without model work".into()
        }
        async fn run(
            &self,
            _ctx: AgentCtx,
            _prompt: String,
            _resources: Vec<anda_core::Resource>,
        ) -> Result<AgentOutput, BoxError> {
            Ok(AgentOutput::default())
        }
    }
    let keys = [
        Ed25519Key::new([91; 32]),
        Ed25519Key::new([92; 32]),
        Ed25519Key::new([93; 32]),
    ];
    let config = runtime_config(&keys, None);
    let brain = create(Arc::new(InMemory::new()), &keys, Some(config.clone())).await;
    let host = Host::new(brain.state.clone(), Some(&config)).unwrap();
    let space = brain.state.load_space("anda_bot", true).await.unwrap();
    let db = crate::test_support::memory_db("memory_product_host").await;
    let conversations = Arc::new(
        crate::engine::ConversationsTool::connect(db.clone(), "bot".into(), "/tmp".into())
            .await
            .unwrap(),
    );
    let owner = keys[0].id();
    let message = Message {
        role: "user".into(),
        content: vec!["Keep release notes short".to_string().into()],
        ..Default::default()
    };
    let mut conv = Conversation {
        user: owner,
        ..Default::default()
    };
    conv.append_messages(vec![message.clone()]);
    let conversation = conversations
        .conversations
        .add_conversation(ConversationRef::from(&conv))
        .await
        .unwrap();
    let journal = Journal::new(db.object_store());
    let client =
        Client::new("http://127.0.0.1:0".into(), None).with_host(host.clone(), journal.clone());
    let source = crate::brain::product::source_identity(&owner.to_string(), conversation, None);
    let submission = FormationSubmission {
        bot_conversation: conversation,
        window_start: 0,
        window_end: 1,
        submitted_at: anda_engine::unix_ms(),
        brain_conversation: None,
        state: FormationState::Pending,
        error: None,
        updated_at: None,
        failure_stage: None,
        provenance: Some(FormationProvenance {
            version: 1,
            policy_revision: Some("fixture-standard".into()),
            caller: owner.to_string(),
            session: None,
            source_identity: Some(source),
            source: "cli:fixture".into(),
            reply_target: None,
            thread: None,
            external_user: false,
            counterparty: Some(owner.to_string()),
            source_messages: vec![SourceMessageRef {
                conversation: conversation.to_string(),
                index: "0".into(),
                role: "user".into(),
                content_digest: anda_cognitive_nexus::content_digest(
                    &serde_json::to_value(&message).unwrap(),
                )
                .unwrap(),
                submitted_digest: None,
            }],
            input_digest: None,
        }),
    };
    let timestamp = Some("2026-09-22T00:00:00.000Z".to_string());
    let accepted = client
        .submit_formation_window(
            submission,
            anda_brain::types::FormationInputRef {
                messages: std::slice::from_ref(&message),
                context: &None,
                timestamp: &timestamp,
            },
        )
        .await
        .unwrap();
    let native = accepted.brain_conversation.unwrap();
    let key = format!("formation/{conversation}/0");
    let mut submission = journal
        .read::<FormationSubmission>(&key)
        .await
        .unwrap()
        .unwrap();
    submission.state = FormationState::Completed;
    journal.write(&key, &submission).await.unwrap();
    let created=command(&space,r#"MUTATE {
        CREATE CONCEPT ?owner {TYPE "Person" NAME "Owner" SET FIELDS {key: :owner}}
        CREATE CONCEPT ?value {TYPE "Preference" NAME "Short release notes"}
        CREATE EVIDENCE ?input {CLIENT KEY :key SET FIELDS {evidence_class:"user_statement",payload: :payload,observed_at:"2026-09-22T00:00:00.000Z"}}
        ASSERT ?claim (?owner,"prefers",?value) {by:?owner,mode:"stated",evidence:?input}
    }"#,json!({"owner":owner.to_string(),"key":format!("formation:conversation:{native}:1"),"payload":message})).await;
    let id = created["handles"]["claim"].as_str().unwrap();
    let activity = ActivityStore::connect(db, conversations, journal.clone(), client.clone())
        .await
        .unwrap();
    activity.reconcile().await.unwrap();
    let engine = Arc::new(
        EngineBuilder::new()
            .with_management(Arc::new(anda_engine::management::BaseManagement {
                controller: owner,
                managers: Default::default(),
                visibility: anda_engine::management::Visibility::Public,
            }))
            .register_tool(Arc::new(NoteTool::new()))
            .unwrap()
            .register_agent(Arc::new(Stub), None)
            .unwrap()
            .build(crate::engine::AndaBot::NAME.into())
            .await
            .unwrap(),
    );
    let engine_ref = Arc::new(EngineRef::new());
    engine_ref.bind(Arc::downgrade(&engine));
    let access = Arc::new(MemoryAccess::new(host, journal.clone(), engine_ref, owner));
    let mutations = MutationService::new(access.clone(), journal.clone(), activity.clone());
    let service = MemoryService::new(client)
        .with_activity(activity)
        .with_mutations(mutations.clone());
    let before = service.record(owner, id).await.unwrap();
    assert!(before.sources_complete);
    assert_eq!(
        before.sources[0].text.as_deref(),
        Some("Keep release notes short")
    );
    assert!(service.record(keys[1].id(), id).await.is_err());
    let input = ChangeRequest {
        operation_id: "host-correction".into(),
        record_id: id.into(),
        expected_revision: before.revision,
        kind: anda_brain::product::ChangeKind::Correct,
        new_value: Some("Risks first".into()),
    };
    let preview = service.prepare_change(owner, input.clone()).await.unwrap();
    let ctx = engine
        .ctx_with(
            owner,
            crate::engine::AndaBot::NAME,
            "",
            RequestMeta::default(),
        )
        .unwrap();
    let notes: NoteArgs = serde_json::from_value(
        json!({"op":"set","items":[{"id":"old","content":"Old processing context"}]}),
    )
    .unwrap();
    NoteTool::new()
        .call(ctx.child_base(NoteTool::NAME).unwrap(), notes, vec![])
        .await
        .unwrap();
    let confirmed = service
        .commit_change(
            owner,
            input.operation_id.clone(),
            CommitRequest {
                preview_digest: preview.preview_digest,
            },
        )
        .await
        .unwrap();
    assert_eq!(confirmed.state, "confirmed");
    assert!(load_notes(&ctx).await.unwrap().items.is_empty());
    assert_eq!(
        service
            .prepare_change(owner, input.clone())
            .await
            .unwrap()
            .state,
        "confirmed"
    );
    assert!(
        service
            .change(keys[1].id(), &input.operation_id)
            .await
            .is_err()
    );
    let mut conflict = input.clone();
    conflict.new_value = Some("Different text".into());
    assert!(service.prepare_change(owner, conflict).await.is_err());
    let replacement = service
        .record(owner, confirmed.replacement_record.as_deref().unwrap())
        .await
        .unwrap();
    assert!(replacement.sources_complete);
    assert_eq!(replacement.sources[0].kind, "correction");
    let remove = ChangeRequest {
        operation_id: "host-delete".into(),
        record_id: replacement.id.clone(),
        expected_revision: replacement.revision,
        kind: anda_brain::product::ChangeKind::Delete,
        new_value: None,
    };
    let preview = service.prepare_change(owner, remove.clone()).await.unwrap();
    let native_preview = space
        .product_change(owner, &remove.operation_id)
        .await
        .unwrap();
    assert_eq!(
        space
            .product_commit(
                owner,
                remove.operation_id.clone(),
                native_preview.preview_digest,
            )
            .await
            .unwrap()
            .state,
        "confirmed"
    );
    let notes: NoteArgs = serde_json::from_value(
        json!({"op":"set","items":[{"id":"stale","content":"Must be cleared after deletion"}]}),
    )
    .unwrap();
    NoteTool::new()
        .call(ctx.child_base(NoteTool::NAME).unwrap(), notes, vec![])
        .await
        .unwrap();
    // Simulate a Bot receipt from the old error path: native confirmed, while
    // the Bot still retained its preview and had not reset Notes.
    let key = anda_cognitive_nexus::content_digest(
        &json!({"caller":owner.to_string(),"operation_id":remove.operation_id}),
    )
    .unwrap();
    let path = format!("changes/{}", &key[7..]);
    let mut saved: serde_json::Value = journal.read(&path).await.unwrap().unwrap();
    saved["view"]["state"] = "confirmed".into();
    saved["view"]["error"] = "acceptance_unknown".into();
    journal.write(&path, &saved).await.unwrap();
    let confirmed = service
        .commit_change(
            owner,
            remove.operation_id.clone(),
            CommitRequest {
                preview_digest: preview.preview_digest,
            },
        )
        .await
        .unwrap();
    assert_eq!(confirmed.state, "confirmed");
    assert!(confirmed.before.is_none());
    assert!(confirmed.error.is_none());
    assert!(load_notes(&ctx).await.unwrap().items.is_empty());
    assert!(service.record(owner, &replacement.id).await.is_err());
    // Preparing the same logical operation after deletion returns its receipt,
    // without requiring a record that has intentionally ceased to exist.
    assert_eq!(
        service.prepare_change(owner, remove).await.unwrap().state,
        "confirmed"
    );
    space.close().await.unwrap();
}
