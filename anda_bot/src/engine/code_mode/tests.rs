use super::*;
use anda_core::{AgentContext, AgentOutput, BoxPinFut, CompletionRequest, ToolCall};
use anda_engine::{
    engine::EngineBuilder,
    model::{CompletionFeaturesDyn, Model},
};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

/// A recorded model turn exercises the public runner, dispatch and worker pipes.
struct FixtureModel(parking_lot::Mutex<Option<ToolCall>>);
impl CompletionFeaturesDyn for FixtureModel {
    fn model_name(&self) -> String {
        "code-mode-fixture".into()
    }
    fn completion(&self, _: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
        let tool_calls = self.0.lock().take().into_iter().collect();
        Box::pin(async move {
            Ok(AgentOutput {
                tool_calls,
                content: "fixture".into(),
                ..Default::default()
            })
        })
    }
}
struct CronRuns(Arc<AtomicUsize>);
impl Tool<BaseCtx> for CronRuns {
    type Args = Value;
    type Output = Value;
    fn name(&self) -> String {
        "list_cron_runs".into()
    }
    fn description(&self) -> String {
        "Recorded cron runs".into()
    }
    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: schema(
                json!({"job_id":{"type":"integer"},"limit":{"type":"integer"},"cursor":{"type":["string","null"]}}),
            ),
            strict: Some(true),
        }
    }
    async fn call(
        &self,
        _: BaseCtx,
        args: Value,
        _: Vec<Resource>,
    ) -> Result<ToolOutput<Value>, BoxError> {
        assert_eq!(args, json!({"job_id":42,"limit":20,"cursor":null}));
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::new(
            json!({"result":[{"error":"a"},{},{"error":"b"}]}),
        ))
    }
}
struct Fixture {
    mode: CodeMode,
    ctx: AgentCtx,
    model: Arc<FixtureModel>,
    calls: Arc<AtomicUsize>,
}

struct RecordedSend;
impl Tool<BaseCtx> for RecordedSend {
    type Args = Value;
    type Output = Value;
    fn name(&self) -> String {
        "send_im_message".into()
    }
    fn description(&self) -> String {
        "Record a send after the normal IM approval.".into()
    }
    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: schema(json!({"recipient":{"type":"string"}})),
            strict: Some(true),
        }
    }
    async fn call(
        &self,
        ctx: BaseCtx,
        args: Value,
        _: Vec<Resource>,
    ) -> Result<ToolOutput<Value>, BoxError> {
        let target = args["recipient"].as_str().unwrap();
        crate::engine::approve_im_send(&ctx, "lark", target, None, "fixture", &[]).await?;
        Ok(ToolOutput::new(json!({"result":{"sent":target}})))
    }
}
impl Fixture {
    async fn open(home: &Path) -> Self {
        let owner = Principal::anonymous();
        let mode = CodeMode::new(
            home.into(),
            home.into(),
            owner,
            CliWorkspaceGrants::new(owner),
            [1; 48],
        );
        let model = Arc::new(FixtureModel(Default::default()));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::new()
            .with_model(Model::new(model.clone()))
            .register_tool(Arc::new(CronRuns(calls.clone())))
            .unwrap()
            .register_tool(Arc::new(RecordedSend))
            .unwrap();
        for name in [EXEC, REGISTER, LIST] {
            let tool = mode.tool(name);
            crate::util::json_schema::assert_openai_strict_parameters(
                &tool.definition().parameters,
            );
            builder = builder.register_tool(Arc::new(tool)).unwrap();
        }
        let ctx = builder.mock_ctx();
        mode.attach(&ctx, ctx.meta()).await.unwrap();
        Self {
            mode,
            ctx,
            model,
            calls,
        }
    }
    async fn call(&self, name: &str, args: Value, allowed: Option<&[&str]>) -> ToolOutput<Value> {
        *self.model.0.lock() = Some(ToolCall {
            name: name.into(),
            args,
            call_id: Some("fixture-call".into()),
            remote_id: None,
            result: None,
        });
        let mut runner = self
            .ctx
            .clone()
            .completion_iter(
                CompletionRequest {
                    prompt: "run recorded call".into(),
                    ..Default::default()
                },
                Vec::new(),
            )
            .with_allowed_callables(
                allowed.map(|names| names.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>()),
            );
        runner.next().await.unwrap().unwrap();
        runner.next().await.unwrap().unwrap().tool_calls[0]
            .result
            .take()
            .unwrap()
    }
}

const SOURCE: &str = "def cron_failure_count(job_id: int, recent: int = 20) -> int:\n    \"\"\"Count recent failed cron runs.\"\"\"\n    page = tools.list_cron_runs(job_id=job_id, cursor=None, limit=recent)\n    return sum('error' in run for run in page['result'])";
fn save(source: &str, revision: Option<&str>) -> Value {
    json!({"action":"save","name":null,"expected_revision":revision,"source":source})
}

#[tokio::test]
async fn code_mode_register_restart_and_reuse_through_runner() {
    let tmp = tempfile::tempdir().unwrap();
    let first = Fixture::open(tmp.path()).await;
    let created = first.call(REGISTER, save(SOURCE, None), None).await;
    assert_eq!(
        created.output["result"]["status"], "created",
        "{:?}",
        created.output
    );
    assert_eq!(first.calls.load(Ordering::SeqCst), 0);
    drop(first);
    let bot = Fixture::open(tmp.path()).await;
    let name = "script__cron_failure_count";
    let listed = bot
        .call(
            LIST,
            json!({"query":name,"cursor":null,"limit":null,"include_source":true}),
            None,
        )
        .await;
    assert_eq!(listed.output["result"][0]["source"], SOURCE);
    let def = bot
        .ctx
        .definitions(Some(&[name.into()]))
        .await
        .pop()
        .unwrap();
    let selected = bot
        .call(
            "tools_select",
            json!({"tools":[name],"query":"","group":"","limit":1}),
            None,
        )
        .await;
    assert_eq!(selected.output["tools"][0]["name"], name);
    assert!(
        selected.output["python_interfaces"][name]
            .as_str()
            .unwrap()
            .contains("recent")
    );
    crate::util::json_schema::assert_openai_strict_parameters(&def.parameters);
    let out = bot.call(name, json!({"job_id":42,"recent":20}), None).await;
    assert_eq!(out.output, json!({"result":2}), "{out:?}");
    let out = bot
        .call(
            EXEC,
            json!({"code":"return tools.script__cron_failure_count(job_id=42)"}),
            None,
        )
        .await;
    assert_eq!(out.output["result"]["value"], 2, "{out:?}");
    assert_eq!(out.tools_usage["list_cron_runs"].requests, 1);
    let denied = bot
        .call(name, json!({"job_id":42,"recent":20}), Some(&[name]))
        .await;
    assert_eq!(denied.output["error"]["code"], "scope_denied", "{denied:?}");
    assert_eq!(bot.calls.load(Ordering::SeqCst), 2);
    bot.ctx
        .base
        .set_state(MemoryPolicy::new(crate::engine::MemoryMode::NoStore));
    assert!(!bot.mode.visible(&bot.ctx.base, REGISTER));
    let denied=bot.call(EXEC,json!({"code":"return tools.code_register(action='remove', name='script__cron_failure_count', source=None, expected_revision='x')"}),None).await;
    assert_eq!(denied.output["error"]["code"], "scope_denied");
}

#[tokio::test]
async fn code_mode_batch_prepares_all_approvals_and_sends_only_selected_items() {
    use crate::engine::{ActionEvent, ActionRuntime, ActionSession, SessionRequestMeta};
    let dir = tempfile::tempdir().unwrap();
    let bot = Fixture::open(dir.path()).await;
    let mut meta = bot.ctx.meta().clone();
    meta.extra
        .insert("approval_mode".into(), json!("request_approval"));
    bot.ctx.base.set_state(SessionRequestMeta::new(meta));
    let runtime = Arc::new(ActionRuntime::new());
    let (tx, mut events) = tokio::sync::mpsc::channel(32);
    let caller = bot.ctx.caller().to_text();
    bot.ctx.base.set_state(ActionSession::new(
        runtime.clone(),
        tx,
        caller.clone(),
        "test-session".into(),
        Arc::new(std::sync::atomic::AtomicU64::new(42)),
        Arc::default(),
        dir.path().into(),
    ));
    let answer = async {
        let mut items = Vec::new();
        while items.len() < 6 {
            if let Some(ActionEvent::Add(message)) = events.recv().await
                && let anda_core::ContentPart::Action { payload, .. } = &message.content[0]
            {
                items.push(payload.clone());
            }
        }
        for card in &items {
            assert_eq!(card["batch_id"], items[0]["batch_id"]);
            assert!(card["parent_run_id"].is_string());
            let approved = matches!(card["child_id"].as_u64(), Some(1 | 3));
            let args =
                serde_json::from_value(json!({"action_id":card["id"],"approve":approved})).unwrap();
            runtime.respond(&caller, 42, args).await.unwrap();
        }
    };
    let code = "results = parallel([('send_im_message', {'recipient':str(i)}) for i in range(6)])\nreturn [v.code if isinstance(v, ToolError) else v['result']['sent'] for v in results]";
    let (output, ()) = tokio::time::timeout(std::time::Duration::from_secs(22), async {
        tokio::join!(bot.call(EXEC, json!({"code":code}), None), answer)
    })
    .await
    .expect("all six cards must be visible before confirmation");
    assert_eq!(
        output.output["result"]["value"],
        json!([
            "0",
            "approval_denied",
            "2",
            "approval_denied",
            "approval_denied",
            "approval_denied"
        ]),
        "{output:?}"
    );
}

#[tokio::test]
async fn code_mode_registry_quota_cas_corruption_and_cursors() {
    let dir = tempfile::tempdir().unwrap();
    let library = Arc::new(Library::open(dir.path().into()).await.unwrap());
    let decl = |n: usize| Declaration {
        name: format!("script__f{n:02}"),
        description: "Fixture".into(),
        signature: "() -> int".into(),
        defaults: Default::default(),
        parameters: schema(json!({})),
    };
    for i in 0..63 {
        library
            .save(format!("source {i}"), decl(i), None)
            .await
            .unwrap();
    }
    let page = |cursor| {
        library.list(library::ListArgs {
            query: None,
            cursor,
            limit: Some(2),
            include_source: false,
        })
    };
    let cursor = page(None).unwrap()["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let (a, b) = tokio::join!(
        library.save("a".into(), decl(63), None),
        library.save("b".into(), decl(64), None)
    );
    assert_eq!([&a, &b].into_iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(a.err().or_else(|| b.err()).unwrap().code, "quota_exceeded");
    assert_eq!(page(Some(cursor)).unwrap_err().code, "stale_cursor");
    let first = library.get("script__f00").unwrap();
    assert_eq!(
        library
            .save("new".into(), decl(0), None)
            .await
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    library
        .remove("script__f00", &first.revision)
        .await
        .unwrap();
    assert_eq!(
        library
            .remove("script__f00", &first.revision)
            .await
            .unwrap()["result"]["status"],
        "absent"
    );
    library.save("new".into(), decl(0), None).await.unwrap();
    assert_eq!(
        library
            .remove("script__f00", &first.revision)
            .await
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    let broken = dir.path().join("script__broken.json");
    tokio::fs::write(&broken, b"broken").await.unwrap();
    let reopened = Library::open(dir.path().into()).await.unwrap();
    assert_eq!(reopened.definitions(None).len(), 64);
    assert_eq!(tokio::fs::read(broken).await.unwrap(), b"broken");
}

#[tokio::test]
async fn code_mode_recovery_reconciles_before_dispatch_and_waits_for_user() {
    let dir = tempfile::tempdir().unwrap();
    let bot = Fixture::open(dir.path()).await;
    let records = bot.mode.home.join("runs/42");
    tokio::fs::create_dir_all(&records).await.unwrap();
    let meta = bot.ctx.meta();
    let receipt = |id, status| json!({"child_id":id,"tool":"send_im_message","status":status,"argument_digest":"fixture"});
    let record = json!({"run_id":"example","conversation_id":42,"caller":bot.ctx.caller().to_text(),"route":route(meta),"outer_call_id":"c","revision":null,
        "calls":[receipt(1,"prepared"),receipt(2,"dispatching"),receipt(3,"completed")]});
    tokio::fs::write(
        records.join("example.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .await
    .unwrap();
    for _ in 0..2 {
        let summary = bot
            .mode
            .recovery(&bot.ctx.base, meta, 42, &[])
            .await
            .unwrap()
            .unwrap();
        for state in ["cancelled", "outcome_unknown", "completed"] {
            assert!(summary.contains(state));
        }
        assert!(!bot.mode.executable(&bot.ctx.base, "send_im_message"));
    }
    assert!(
        bot.mode
            .recovery(&bot.ctx.base, meta, 99, &[])
            .await
            .unwrap()
            .is_none()
    );
    bot.ctx
        .base
        .get_state::<RecoveryHold>()
        .unwrap()
        .user_replied();
    assert!(bot.mode.executable(&bot.ctx.base, "send_im_message"));
}

#[tokio::test]
async fn code_mode_admission_and_oversized_results_preserve_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let bot = Fixture::open(dir.path()).await;
    let occupied = bot.mode.slots.clone().acquire_many_owned(2).await.unwrap();
    let busy = bot.call(EXEC, json!({"code":"return 1"}), None).await;
    assert_eq!(busy.output["error"]["data"]["reason"], "worker_busy");
    drop(occupied);
    let output=bot.call(EXEC,json!({"code":"tools.list_cron_runs(job_id=42, limit=20, cursor=None)\nreturn 'x' * 50000"}),None).await;
    let error = &output.output["error"];
    assert_eq!(error["code"], "resource_limit", "{output:?}");
    assert_eq!(error["data"]["counts"]["completed"], 1);
    assert_eq!(error["data"]["receipts"][0]["tool"], "list_cron_runs");
    assert_eq!(error["data"]["output_shape"]["bytes_at_least"], 32769);
    assert_eq!(bot.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn code_mode_dropping_a_run_expires_its_card_and_reaps_its_worker() {
    use crate::engine::{ActionEvent, ActionRuntime, ActionSession};
    let dir = tempfile::tempdir().unwrap();
    let bot = Fixture::open(dir.path()).await;
    let caller = bot.ctx.caller().to_text();
    let (tx, mut events) = tokio::sync::mpsc::channel(8);
    let runtime = Arc::new(ActionRuntime::new());
    bot.ctx.base.set_state(ActionSession::new(
        runtime.clone(),
        tx,
        caller.clone(),
        "fixture".into(),
        Arc::new(std::sync::atomic::AtomicU64::new(42)),
        Arc::default(),
        dir.path().into(),
    ));
    let id = {
        let call = bot.call(
            EXEC,
            json!({"code":"return tools.send_im_message(recipient='fixture')"}),
            None,
        );
        tokio::pin!(call);
        tokio::select! {
            result=&mut call => panic!("expected approval, received {result:?}"),
            event=tokio::time::timeout(std::time::Duration::from_secs(15),events.recv())=>{
                let Some(ActionEvent::Add(message))=event.unwrap() else{panic!("expected an approval card")};
                let anda_core::ContentPart::Action{payload,..}=&message.content[0] else{panic!("expected an action")};
                payload["id"].as_str().unwrap().to_owned()
            }
        }
    };
    let late = serde_json::from_value(json!({"action_id":id,"approve":true})).unwrap();
    assert!(runtime.respond(&caller, 42, late).await.is_err());
    let permits = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        bot.mode.slots.clone().acquire_many_owned(2),
    )
    .await
    .unwrap()
    .unwrap();
    drop(permits);
}

#[tokio::test]
async fn code_mode_registration_honors_explicit_approval_and_preserves_the_revision() {
    let dir = tempfile::tempdir().unwrap();
    let bot = Fixture::open(dir.path()).await;
    let created = bot.call(REGISTER, save(SOURCE, None), None).await;
    let revision = &created.output["result"]["revision"];
    let mut meta = bot.ctx.meta().clone();
    meta.extra
        .insert("approval_mode".into(), json!("request_approval"));
    bot.ctx
        .base
        .set_state(crate::engine::SessionRequestMeta::new(meta));
    // No card receiver can approve a change in this fixture.
    let unchanged = bot.call(REGISTER, save(SOURCE, None), None).await;
    assert_eq!(unchanged.output["result"]["status"], "unchanged");
    let remove = json!({"action":"remove","name":"script__cron_failure_count","source":null,"expected_revision":revision});
    let rejected = bot.call(REGISTER, remove, None).await;
    assert_eq!(rejected.output["error"]["code"], "approval_required");
    assert_eq!(
        bot.mode
            .library(&bot.ctx.base)
            .unwrap()
            .get("script__cron_failure_count")
            .unwrap()
            .revision,
        revision.as_str().unwrap()
    );
}
