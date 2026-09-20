use super::*;
use anda_core::{BoxPinFut, CompletionRequest, ToolCall, Usage};
use anda_engine::model::{CompletionFeaturesDyn, Model};
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct Fixture {
    calls: AtomicUsize,
    block_business: std::sync::atomic::AtomicBool,
    panic_business: std::sync::atomic::AtomicBool,
    fail_maintenance: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    prompts: std::sync::Mutex<Vec<CompletionRequest>>,
}
impl CompletionFeaturesDyn for Fixture {
    fn model_name(&self) -> String {
        "fixture".into()
    }
    fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let business = req
            .instructions
            .contains("The task runner supplies complete schemas");
        let block = business && self.block_business.load(Ordering::SeqCst);
        let panic_business = business && self.panic_business.load(Ordering::SeqCst);
        let fail = self.fail_maintenance.load(Ordering::SeqCst)
            && req
                .instructions
                .contains("KIP 2.0 Brain — Memory Maintenance");
        if block {
            self.entered.notify_one();
        }
        let budgeted = req.tools.iter().any(|t| t.name == "select_recall_items");
        let tools = req.tools.clone();
        let tool_result = req.prompt.contains("tool_result");
        self.prompts.lock().unwrap().push(req);
        Box::pin(async move {
            assert!(!panic_business, "injected business panic");
            if block {
                std::future::pending::<()>().await;
            }
            if fail {
                return Err("injected maintenance failure".into());
            }
            let mut out = AgentOutput {
                content: if business {
                    if tools.is_empty() {
                        r#"{"type":"structured","value":{"answer":"fixture"}}"#
                    } else {
                        r#"{"type":"final","content":"task done"}"#
                    }
                } else if budgeted {
                    r#"{"selected_ids":[]}"#
                } else {
                    "No additional memory."
                }
                .into(),
                usage: Usage {
                    requests: 1,
                    input_tokens: 7,
                    output_tokens: 3,
                    ..Default::default()
                },
                ..Default::default()
            };
            if business && !tools.is_empty() && !tool_result {
                out.tool_calls.push(ToolCall {
                    name: tools[0].name.clone(),
                    args: json!({}),
                    ..Default::default()
                });
            }
            Ok(out)
        })
    }
}
fn host(mode: Mode) -> (Arc<Host>, Arc<Fixture>) {
    let fixture = Arc::new(Fixture::default());
    let models = Arc::new(Models::default());
    models.set_model(Model::with_completer(fixture.clone()));
    let http = crate::util::http_client::new_reqwest_client();
    let app = AppState::new(
        Arc::new(InMemory::new()),
        Arc::new(DBConfig {
            name: "mib-test".into(),
            description: "test".into(),
            storage: StorageConfig::default(),
            lock: None,
        }),
        Arc::new(BaseManagement {
            controller: SELF_USER_ID,
            managers: Default::default(),
            visibility: Visibility::Protected,
        }),
        http,
        models.clone(),
        Arc::new(vec![]),
        "anda_bot".into(),
        "test".into(),
        0,
    );
    (
        Arc::new(Host {
            app,
            models,
            identity: ExperimentIdentity {
                model_digest: digest(&"fixture"),
                tools_digest: digest(&"fixture-tools"),
                budget_digest: digest(&"fixture-budget"),
            },
            mode,
            timeout: Duration::from_secs(10),
            idle: Duration::from_secs(30),
            max_output: 4096,
            recall_budget: None,
            runs: Mutex::new(HashMap::new()),
            requests: Mutex::new(HashMap::new()),
            tasks: TaskTracker::new(),
            shutdown: CancellationToken::new(),
            epoch: format!("fixture-{}-{}", std::process::id(), unix_ms()),
        }),
        fixture,
    )
}
fn req(op: &str, id: &str, body: Value) -> Request {
    Request {
        mib: "0.1".into(),
        protocol: AGENT.into(),
        run_id: "run".into(),
        request_id: id.into(),
        operation: op.into(),
        virtual_time: Some("2026-09-13T00:00:00Z".into()),
        body,
    }
}
async fn call(h: &Arc<Host>, r: Request) -> Value {
    h.clone().dispatch(r).await
}
fn ok(v: &Value) {
    assert_eq!(v["status"], "ok", "{v}");
}

#[test]
fn observation_roles_preserve_agent_and_tool_provenance() {
    assert_eq!(observation_role("user_message"), "user");
    assert_eq!(observation_role("agent_message"), "assistant");
    assert_eq!(observation_role("action"), "assistant");
    assert_eq!(observation_role("tool_result"), "tool");
}

#[test]
fn model_output_contract_requires_payloads_for_message_and_structured_results() {
    assert!(parse_output(r#"{"type":"message"}"#, &["message"]).is_err());
    assert!(parse_output(r#"{"type":"message","content":null}"#, &["message"]).is_err());
    assert!(parse_output(r#"{"type":"structured"}"#, &["structured"]).is_err());
    assert!(parse_output(r#"{"type":"message","content":"ok"}"#, &["message"]).is_ok());
    assert!(parse_output(r#"{"type":"structured","value":null}"#, &["structured"]).is_ok());
}

#[tokio::test]
async fn duplicate_requests_and_observations_do_not_repeat_formation() {
    let (h, m) = host(Mode::Persistent);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    let r = req(
        "observe",
        "o1",
        json!({"observation":{"observation_id":"o","type":"message","content":"I prefer concise answers"}}),
    );
    let a = call(&h, r.clone()).await;
    ok(&a);
    let calls = m.calls.load(Ordering::SeqCst);
    assert_eq!(a, call(&h, r.clone()).await);
    assert_eq!(calls, m.calls.load(Ordering::SeqCst));
    let mut next = r.clone();
    next.request_id = "o2".into();
    ok(&call(&h, next).await);
    assert_eq!(calls, m.calls.load(Ordering::SeqCst));
    let mut changed = r;
    changed.body["observation"]["content"] = "changed".into();
    assert_eq!(
        call(&h, changed).await["error"]["code"],
        "idempotency_conflict"
    );
    h.close_all().await;
}
#[tokio::test]
async fn no_memory_keeps_current_runner_tool_result_and_clears_at_task_end() {
    let (h, m) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    ok(&call(&h,req("observe","old",json!({"observation":{"observation_id":"old","type":"message","content":"SECRET_HISTORY"}}))).await);
    let task = json!({"task_id":"t","goal":"perform task","constraints":[],"tools":[{"name":"lookup","description":"lookup","input_schema":{"type":"object"}}],"continuation":false});
    let result = call(&h, req("act", "a1", task.clone())).await;
    ok(&result);
    let id = result["body"]["result"]["tool_call_id"].as_str().unwrap();
    ok(&call(&h,req("observe","tool-result",json!({"observation":{"observation_id":"feedback","type":"tool_result","tool":"lookup","tool_call_id":id,"payload":{"result":"CURRENT_TASK"}}}))).await);
    let mut continuation = task;
    continuation["continuation"] = true.into();
    let out = call(&h, req("act", "a2", continuation)).await;
    ok(&out);
    assert_eq!(out["body"]["result"]["type"], "final");
    ok(&call(
        &h,
        req(
            "respond",
            "r1",
            json!({"interaction_id":"i","input":{"content":"hello"}}),
        ),
    )
    .await);
    {
        let prompts = m.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 3);
        let first: Value = serde_json::from_str(&prompts[0].prompt).unwrap();
        let continuation: Value = serde_json::from_str(&prompts[1].prompt).unwrap();
        assert_eq!(first["request"]["continuation"], false);
        assert_eq!(continuation["request"]["continuation"], true);
        assert!(prompts[1].prompt.contains("CURRENT_TASK"));
        assert!(!prompts[2].prompt.contains("CURRENT_TASK"));
        assert!(prompts.iter().all(|p| !p.prompt.contains("SECRET_HISTORY")));
    }
    h.close_all().await;
}
#[tokio::test]
async fn backend_is_read_only_and_namespaced_from_integrated_agent() {
    let (h, m) = host(Mode::Persistent);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    let mut reset = req("reset", "reset", json!({}));
    reset.protocol = MEMORY.into();
    ok(&call(&h, reset).await);
    let mut retrieve = req(
        "retrieve",
        "read",
        json!({"query":"What was recorded?","limit_chars":4}),
    );
    retrieve.protocol = MEMORY.into();
    let v = call(&h, retrieve.clone()).await;
    ok(&v);
    assert_eq!(v["body"]["truncated"], true);
    assert_eq!(v["body"]["items"][0]["content"], "No a");
    assert_eq!(v["body"]["accounting_complete"], false);
    let calls = m.calls.load(Ordering::SeqCst);
    assert_eq!(v, call(&h, retrieve).await);
    assert_eq!(calls, m.calls.load(Ordering::SeqCst));
    assert_eq!(h.runs.lock().await.len(), 2);
    h.close_all().await;
}
#[tokio::test]
async fn close_is_idempotent_and_reuse_of_a_run_id_is_refused() {
    let (h, _) = host(Mode::Persistent);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    let r = req("close", "close", json!({}));
    let a = call(&h, r.clone()).await;
    ok(&a);
    assert_eq!(a, call(&h, r).await);
    ok(&call(&h, req("close", "close-again", json!({}))).await);
    assert_eq!(
        call(&h, req("reset", "reset-again", json!({}))).await["status"],
        "error"
    );
    h.close_all().await;
}

#[tokio::test]
async fn close_interrupts_business_and_retains_unknown_failure_cost() {
    let (h, m) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    m.block_business.store(true, Ordering::SeqCst);
    let waiter = m.entered.notified();
    let hh = h.clone();
    let task = tokio::spawn(async move {
        call(
            &hh,
            req(
                "respond",
                "blocked",
                json!({"interaction_id":"i","input":{"content":"hello"}}),
            ),
        )
        .await
    });
    waiter.await;
    let closed = tokio::time::timeout(
        Duration::from_secs(2),
        call(&h, req("close", "close", json!({}))),
    )
    .await
    .unwrap();
    ok(&closed);
    let failed = task.await.unwrap();
    assert_eq!(failed["status"], "error");
    assert_eq!(failed["extensions"]["cost_scope"], "cumulative_run");
    assert!(failed["extensions"]["costs"].is_object());
    let costs = &closed["body"]["costs"]["receipts"];
    assert!(
        costs
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["stage"] == "business_model"
                && r["failed"] == true
                && r["input_tokens"].is_null()),
        "{closed}"
    );
    h.close_all().await;
}
#[tokio::test]
async fn dropped_http_waiter_does_not_drop_owned_work_and_retry_joins_result() {
    let (h, m) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    let r = req(
        "respond",
        "respond",
        json!({"interaction_id":"i","input":{"content":"hello"}}),
    );
    let tracker = h.tasks.clone();
    let owner = h.clone();
    let rr = r.clone();
    let handle = tracker.spawn(owner.dispatch(rr));
    drop(handle);
    let v = call(&h, r).await;
    ok(&v);
    assert_eq!(m.calls.load(Ordering::SeqCst), 1);
    h.close_all().await;
}
#[tokio::test]
async fn declared_maintenance_failure_invalidates_run_and_cannot_be_replayed() {
    let (h, m) = host(Mode::Persistent);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    m.fail_maintenance.store(true, Ordering::SeqCst);
    let r = req("maintain", "maintenance", json!({"budget":"small"}));
    let failed = call(&h, r.clone()).await;
    assert_eq!(failed["status"], "error", "{failed}");
    let calls = m.calls.load(Ordering::SeqCst);
    assert_eq!(failed, call(&h, r).await);
    assert_eq!(calls, m.calls.load(Ordering::SeqCst));
    assert_eq!(
        call(
            &h,
            req(
                "respond",
                "r",
                json!({"interaction_id":"i","input":{"content":"hello"}})
            )
        )
        .await["status"],
        "error"
    );
    h.close_all().await;
}
#[tokio::test]
async fn backward_time_and_mismatched_tool_feedback_are_execution_failures() {
    let (h, _) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    let mut r = req("session_boundary", "old", json!({}));
    r.virtual_time = Some("2020-01-01T00:00:00Z".into());
    assert_eq!(call(&h, r).await["status"], "error");
    h.close_all().await;
    let (h, _) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    ok(&call(&h,req("act","a",json!({"task_id":"t","goal":"task","tools":[{"name":"lookup","input_schema":{"type":"object"}}]}))).await);
    let r = req(
        "observe",
        "result",
        json!({"observation":{"observation_id":"o","type":"tool_result","tool":"lookup","tool_call_id":"wrong","payload":{}}}),
    );
    assert_eq!(call(&h, r).await["status"], "error");
    h.close_all().await;
}
#[tokio::test]
async fn expired_runs_are_closed_and_do_not_recover_context() {
    let (h, _) = host(Mode::Persistent);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    {
        let runs = h.runs.lock().await;
        let run = runs.values().next().unwrap();
        run.touched.store(1, Ordering::SeqCst);
    }
    h.reap().await;
    assert_eq!(
        call(
            &h,
            req(
                "respond",
                "r",
                json!({"interaction_id":"i","input":{"content":"hello"}})
            )
        )
        .await["status"],
        "error"
    );
    h.close_all().await;
}
#[tokio::test]
async fn conflicting_close_cannot_cancel_an_existing_operation() {
    let (h, _) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "same", json!({}))).await);
    assert_eq!(
        call(&h, req("close", "same", json!({}))).await["error"]["code"],
        "idempotency_conflict"
    );
    ok(&call(
        &h,
        req(
            "respond",
            "r",
            json!({"interaction_id":"i","input":{"content":"hello"}}),
        ),
    )
    .await);
    h.close_all().await;
}

#[tokio::test]
async fn rejected_request_cannot_gain_effect_when_replayed_after_reset() {
    let (h, m) = host(Mode::Persistent);
    let r = req(
        "observe",
        "too-early",
        json!({"observation":{"observation_id":"o","type":"message","content":"test"}}),
    );
    let rejected = call(&h, r.clone()).await;
    assert_eq!(rejected["status"], "error");
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    assert_eq!(call(&h, r).await, rejected);
    assert_eq!(m.calls.load(Ordering::SeqCst), 0);
    h.close_all().await;
}

/// Deliberate local transport fixture for MIB pipeline tests. This never uses a
/// real model or an answer oracle and must not be reported as empirical quality.
#[tokio::test]
#[ignore = "explicit local HTTP fixture; run with MIB_FIXTURE_PORT and --ignored"]
async fn serve_local_transport_fixture() {
    let mode = match std::env::var("MIB_FIXTURE_MODE").as_deref() {
        Ok("no_memory") => Mode::NoMemory,
        Ok("persistent") | Err(std::env::VarError::NotPresent) => Mode::Persistent,
        other => panic!("invalid fixture memory mode: {other:?}"),
    };
    let (mut h, _) = host(mode);
    if std::env::var("MIB_FIXTURE_P5").as_deref() == Ok("1") {
        let host = Arc::get_mut(&mut h).unwrap();
        host.recall_budget = Some(RecallBudget::default());
        host.identity.budget_digest = digest(
            &json!({"fixture":true,"timeout_seconds":10,"max_output_tokens":4096,"recall_budget":host.recall_budget}),
        );
    }
    let port = std::env::var("MIB_FIXTURE_PORT")
        .expect("MIB_FIXTURE_PORT required")
        .parse::<u16>()
        .unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    eprintln!(
        "P2/P6 non-empirical fixture listening on {}",
        listener.local_addr().unwrap()
    );
    let server = axum::serve(listener, router(h.clone())).with_graceful_shutdown(async {
        tokio::select! { _=tokio::time::sleep(Duration::from_secs(600))=>{}, _=tokio::signal::ctrl_c()=>{} }
    });
    server.await.unwrap();
    h.close_all().await;
}

#[derive(Default)]
struct GraphFixture {
    old: std::sync::atomic::AtomicBool,
    updated: std::sync::atomic::AtomicBool,
}
impl CompletionFeaturesDyn for GraphFixture {
    fn model_name(&self) -> String {
        "graph-fixture".into()
    }
    fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
        let mut call = None;
        let serialized = format!("{:?}", req);
        let mut content = "memory workflow complete".to_string();
        if req.instructions.contains("reference Formation policy") {
            let target = if req.prompt.contains("P2_GRAPH_NEW")
                && !self.updated.swap(true, Ordering::SeqCst)
            {
                Some("P2_GRAPH_NEW")
            } else if req.prompt.contains("P2_GRAPH_OLD") && !self.old.swap(true, Ordering::SeqCst)
            {
                Some("P2_GRAPH_OLD")
            } else {
                None
            };
            if let Some(name) = target {
                call = Some((
                    "execute_kip",
                    format!(
                        r#"UPSERT CONCEPT ?p {{MATCH {{type:"Person",key:"p2-subject"}} SET FIELDS {{name:"{name}"}}}}"#
                    ),
                ));
            }
        } else if req.instructions.contains("reference Recall policy") {
            if serialized.contains("P2_GRAPH_NEW") {
                content = "P2_GRAPH_NEW".into();
            } else if serialized.contains("P2_GRAPH_OLD") {
                content = "P2_GRAPH_OLD".into();
            } else {
                call = Some((
                    "execute_kip_readonly",
                    r#"FIND(?p.name) WHERE {?p CONCEPT {type:"Person",key:"p2-subject"}} LIMIT 1"#
                        .into(),
                ));
            }
        }
        Box::pin(async move {
            Ok(AgentOutput {
                content,
                tool_calls: call
                    .into_iter()
                    .map(|(name, command)| ToolCall {
                        name: name.into(),
                        args: json!({"command": command}),
                        ..Default::default()
                    })
                    .collect(),
                usage: Usage {
                    requests: 1,
                    input_tokens: 9,
                    output_tokens: 4,
                    ..Default::default()
                },
                ..Default::default()
            })
        })
    }
}
#[tokio::test]
async fn backend_forms_reads_across_sessions_and_updates_real_nexus() {
    let (h, _) = host(Mode::Persistent);
    h.models
        .set_model(Model::with_completer(Arc::new(GraphFixture::default())));
    let memory_req = |op: &str, id: &str, body: Value| {
        let mut r = req(op, id, body);
        r.protocol = MEMORY.into();
        r
    };
    ok(&call(&h, memory_req("reset", "reset", json!({}))).await);
    for (index, value) in [(1, "P2_GRAPH_OLD"), (2, "P2_GRAPH_NEW")] {
        ok(&call(&h,memory_req("observe",&format!("o{index}"),json!({"observation":{"observation_id":format!("o{index}"),"type":"message","content":value}}))).await);
        ok(&call(
            &h,
            memory_req("session_boundary", &format!("b{index}"), json!({})),
        )
        .await);
        let result = call(
            &h,
            memory_req(
                "retrieve",
                &format!("r{index}"),
                json!({"query":"What is the saved person name?","limit_chars":1000}),
            ),
        )
        .await;
        ok(&result);
        assert_eq!(result["body"]["items"][0]["content"], value, "{result}");
    }
    ok(&call(
        &h,
        memory_req("maintain", "maintain", json!({"budget":"small"})),
    )
    .await);
    h.close_all().await;
}

#[tokio::test]
async fn panicked_request_is_terminal_and_never_reexecutes() {
    let (h, m) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    m.panic_business.store(true, Ordering::SeqCst);
    let r = req(
        "respond",
        "panic",
        json!({"interaction_id":"i","input":{"content":"hello"}}),
    );
    let out = call(&h, r.clone()).await;
    assert_eq!(out["status"], "error");
    assert_eq!(out, call(&h, r).await);
    assert_eq!(m.calls.load(Ordering::SeqCst), 1);
    assert!(
        out["extensions"]["costs"]["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["failed"] == true)
    );
    h.close_all().await;
}

#[tokio::test]
async fn p6_native_audit_is_read_only_and_does_not_claim_unbound_learning() {
    for mode in [Mode::Persistent, Mode::NoMemory] {
        let (h, model) = host(mode);
        let descriptor = h.descriptor(AGENT);
        let extension = &descriptor["extensions"]["mib.learning_longitudinal.v1"];
        assert_eq!(extension["native_comparison_and_review"], false);
        assert_eq!(extension["independent_observer"], false);
        assert_eq!(extension["isolated_trial_application"], false);
        assert_eq!(
            extension["mode"],
            if mode == Mode::NoMemory {
                "no_memory"
            } else {
                "persistent"
            }
        );
        ok(&call(&h, req("reset", "reset", json!({}))).await);
        let first = call(&h, req("learning_audit", "audit1", json!({}))).await;
        ok(&first);
        assert_eq!(first["body"]["complete"], true);
        assert_eq!(first["body"]["counts"]["skills"], 0);
        assert_eq!(first["body"]["run_id"], "run");
        let mut later = req("learning_audit", "audit2", json!({}));
        later.virtual_time = Some("2036-09-13T00:00:00Z".into());
        let second = call(&h, later).await;
        ok(&second);
        assert_eq!(
            first["body"]["state_digest"],
            second["body"]["state_digest"]
        );
        assert_eq!(
            first["body"]["native_sequence"],
            second["body"]["native_sequence"]
        );
        // Audit does not advance the business clock or consume a model call.
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
        ok(&call(&h, req("session_boundary", "session", json!({}))).await);
        let third = call(&h, req("learning_audit", "audit3", json!({}))).await;
        ok(&third);
        assert_eq!(first["body"]["state_digest"], third["body"]["state_digest"]);
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
        h.close_all().await;
    }
}

#[tokio::test]
async fn p6_budgeted_backend_never_slices_a_packet_and_forced_cap_cannot_be_raised() {
    let (mut h, _) = host(Mode::Persistent);
    Arc::get_mut(&mut h).unwrap().recall_budget = Some(RecallBudget::default());
    let mut reset = req("reset", "reset", json!({}));
    reset.protocol = MEMORY.into();
    ok(&call(&h, reset).await);
    let mut retrieve = req(
        "retrieve",
        "retrieve1",
        json!({"query":"memory","limit_chars":1}),
    );
    retrieve.protocol = MEMORY.into();
    let out = call(&h, retrieve).await;
    ok(&out);
    assert_eq!(out["body"]["items"], json!([]));
    assert_eq!(out["body"]["truncated"], true);
    let mut retrieve = req(
        "retrieve",
        "retrieve2",
        json!({"query":"memory","limit_chars":32000}),
    );
    retrieve.protocol = MEMORY.into();
    let out = call(&h, retrieve).await;
    ok(&out);
    let packet = out["body"]["items"][0]["content"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(packet).unwrap()["format"],
        "anda-brain-recall/1"
    );
    assert!(anda_brain::recall_budget::count(packet).unwrap() <= 4096);
    h.close_all().await;

    let (mut h, model) = host(Mode::Persistent);
    Arc::get_mut(&mut h).unwrap().recall_budget = Some(RecallBudget {
        max_tokens: 1,
        context_tokens: 1,
        ..Default::default()
    });
    ok(&call(&h, req("reset", "reset", json!({}))).await);
    let out = call(&h, req("respond", "ask", json!({"interaction_id":"i", "input":{"content":"anything"},"budget":{"max_tokens":65536,"context_tokens":131072}}))).await;
    assert_eq!(out["status"], "error");
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    h.close_all().await;
}

#[tokio::test]
async fn runner_qualified_tools_use_provider_safe_aliases_and_return_runner_names() {
    let (h, fixture) = host(Mode::NoMemory);
    ok(&call(&h, req("reset", "reset-alias", json!({"mode":"fresh"}))).await);
    let schemas = vec![
        json!({"name":"workflow.inspect","description":"Inspect the actual state.","input_schema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"workflow_inspect","description":"A distinct runner operation.","input_schema":{"type":"object","properties":{},"additionalProperties":false}}),
    ];
    let first = call(&h, req("act", "alias-act", json!({"task_id":"alias-task","goal":"Inspect the item.","continuation":false,"tools":schemas}))).await;
    ok(&first);
    assert_eq!(first["body"]["result"]["tool"], "workflow.inspect");
    {
        let prompts = fixture.prompts.lock().unwrap();
        let request = prompts
            .iter()
            .find(|p| {
                p.instructions
                    .contains("The task runner supplies complete schemas")
            })
            .unwrap();
        assert_eq!(request.tools.len(), 2);
        assert_ne!(request.tools[0].name, request.tools[1].name);
        for tool in &request.tools {
            anda_core::validate_function_name(&tool.name).unwrap();
        }
        assert_eq!(request.tools[0].parameters, schemas[0]["input_schema"]);
        assert!(request.tools[0].description.contains("workflow.inspect"));
    }
    let feedback = call(
        &h,
        req(
            "observe",
            "alias-feedback",
            json!({"observation":{
                "observation_id":"alias-result","type":"tool_result","tool":"workflow.inspect",
                "tool_call_id":first["body"]["result"]["tool_call_id"],"payload":{"inspected":true}
            }}),
        ),
    )
    .await;
    ok(&feedback);
    let final_step = call(
        &h,
        req(
            "act",
            "alias-next",
            json!({"task_id":"alias-task","continuation":true}),
        ),
    )
    .await;
    ok(&final_step);
    assert_eq!(final_step["body"]["result"]["type"], "final");
    h.close_all().await;
}
