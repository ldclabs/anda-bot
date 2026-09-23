use anda_core::{
    AgentContext, AgentInput, BoxError, Principal, RequestMeta, StateFeatures, ToolInput,
    ToolOutput,
};
use anda_db::{database::AndaDB, unix_ms};
use anda_engine::{
    context::BaseCtx,
    engine::{Engine, EngineRef},
    extension::shell::{ExecArgs, ExecOutput, ShellTool},
    hook::{BackgroundHandle, DynToolJsonHook, ToolBackgroundHook},
};
use async_trait::async_trait;
use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

use super::{
    execution::{AgentSubmission, CronWorkspaceGrant},
    store::CronStore,
    types::*,
};
use crate::{engine::system_runtime_prompt, util::request_meta::keys};

const DEFAULT_POLL_SECS: u64 = 5;
const MAX_CONCURRENT_JOBS: usize = 8;

#[derive(Clone)]
pub struct CronRuntime {
    pub store: CronStore,
    engine: Arc<EngineRef>,
}

impl CronRuntime {
    pub async fn connect(engine: Arc<EngineRef>, db: Arc<AndaDB>) -> Result<Self, BoxError> {
        Ok(Self {
            store: CronStore::connect(db).await?,
            engine,
        })
    }

    async fn process_due_jobs_once(
        &self,
        engine: Arc<Engine>,
        in_flight: &mut JoinSet<u64>,
        running_ids: &mut HashSet<u64>,
        cancel: &CancellationToken,
    ) -> Result<usize, BoxError> {
        let available = MAX_CONCURRENT_JOBS.saturating_sub(running_ids.len());
        let jobs = self
            .store
            .due_jobs(unix_ms(), available, running_ids)
            .await?;
        let mut started = 0;
        for job in jobs {
            if cancel.is_cancelled() {
                break;
            }
            let Some((job, run)) = self.store.claim_job(job._id, unix_ms()).await? else {
                continue;
            };
            running_ids.insert(job._id);
            let this = self.clone();
            let engine = engine.clone();
            let cancel = cancel.clone();
            in_flight.spawn(async move {
                let id = job._id;
                this.process_due_job(engine, job, run, cancel).await;
                id
            });
            started += 1;
        }
        Ok(started)
    }

    async fn process_due_job(
        &self,
        engine: Arc<Engine>,
        job: CronJob,
        run: CronRun,
        cancel: CancellationToken,
    ) {
        // Cancellation/panic affects execution only; never drop a database mutation midway.
        let result = match AssertUnwindSafe(self.execute_job(engine, &job, run._id, &cancel))
            .catch_unwind()
            .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => err.into(),
            Err(_) => BoxError::from("Cron job execution panicked").into(),
        };
        let run_id = run._id;
        if let Err(err) = self.store.job_finish(&job, run, unix_ms(), result).await {
            log::error!(name = "cron"; "failed to finish cron job {} (run {run_id}): {err}", job._id);
        }
    }

    async fn execute_job(
        &self,
        engine: Arc<Engine>,
        job: &CronJob,
        run_id: u64,
        cancel: &CancellationToken,
    ) -> Result<CronJobResult, BoxError> {
        if job
            .origin
            .as_ref()
            .is_some_and(|origin| origin.external_user == Some(true))
        {
            return Err("External IM users cannot execute scheduled jobs".into());
        }
        let caller = job
            .origin
            .as_ref()
            .and_then(CronJobOrigin::caller_principal)
            .unwrap_or(Principal::management_canister());
        let mut meta = job.request_meta().unwrap_or_default();
        meta.extra.insert(keys::CRON_JOB_ID.into(), job._id.into());
        meta.extra.insert(keys::CRON_RUN_ID.into(), run_id.into());
        match job.job_kind {
            JobKind::Agent => {
                let prompt = system_runtime_prompt(
                    "cron agent job",
                    format!(
                        "Scheduled agent job is running. Execute the following instructions and produce a helpful result for the user.\n\nJob id: {}\nJob name: {}\nRun id: {}\nInstructions:\n{}",
                        job._id,
                        job.name.as_deref().unwrap_or("unnamed"),
                        run_id,
                        job.job
                    ),
                );
                self.run_agent(&engine, job, caller, meta, prompt, cancel)
                    .await
            }
            JobKind::Shell => {
                let mut result = match run_shell(&engine, job, caller, meta.clone(), cancel).await {
                    Ok(output) => shell_result(output),
                    Err(err) => err.into(),
                };
                if job.origin.is_some() && !cancel.is_cancelled() {
                    let notification = self
                        .run_agent(
                            &engine,
                            job,
                            caller,
                            meta,
                            cron_shell_result_prompt(job, run_id, &result),
                            cancel,
                        )
                        .await
                        .unwrap_or_else(Into::into);
                    result.conversation_id = notification.conversation_id;
                    if let Some(error) = notification.error {
                        result.error = Some(format!(
                            "{}Notification failed: {error}",
                            result.error.map(|e| e + "; ").unwrap_or_default()
                        ));
                    }
                }
                Ok(result)
            }
        }
    }

    async fn run_agent(
        &self,
        engine: &Engine,
        job: &CronJob,
        caller: Principal,
        meta: RequestMeta,
        prompt: String,
        cancel: &CancellationToken,
    ) -> Result<CronJobResult, BoxError> {
        let name = engine.default_agent();
        let ctx = engine.ctx_with(caller, &name, &name, meta)?;
        let context_cancel = ctx.base.cancellation_token();
        let cancel_on_drop = context_cancel.clone().drop_guard();
        install_workspace_grant(&ctx.base, job, caller);
        let (submission, receiver) = AgentSubmission::new();
        let stop_on_drop = submission.cancellation_token().drop_guard();
        ctx.base.set_state(submission.clone());
        let (output, _) = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err("Cron scheduler stopped".into()),
            output = ctx.agent_run(AgentInput::new(name, prompt)) => output?,
        };
        let result = if submission.was_claimed() {
            wait_for_completion(receiver, cancel, || {
                submission.cancellation_token().cancel();
                context_cancel.cancel();
            })
            .await
        } else {
            // Other synchronous agents already return their final output.
            Ok(output.into())
        };
        stop_on_drop.disarm();
        cancel_on_drop.disarm();
        result
    }

    pub async fn serve(
        self,
        cancel: CancellationToken,
    ) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
        Ok(tokio::spawn(async move {
            log::warn!(name = "cron"; "cron scheduler started");
            let mut interval = tokio::time::interval(Duration::from_secs(DEFAULT_POLL_SECS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut in_flight = JoinSet::new();
            let mut running_ids = HashSet::new();
            let mut failure: Option<BoxError> = None;
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    result = in_flight.join_next(), if !in_flight.is_empty() => {
                        if let Some(result) = result {
                            match result {
                                Ok(id) => { running_ids.remove(&id); }
                                Err(err) => { failure = Some(err.into()); break; }
                            }
                            if let Err(err) = self.store.flush(unix_ms()).await {
                                failure = Some(err);
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {}
                }
                if let Some(engine) = self.engine.get()
                    && let Err(err) = self
                        .process_due_jobs_once(engine, &mut in_flight, &mut running_ids, &cancel)
                        .await
                {
                    log::error!(name = "cron"; "cron tick failed: {err}");
                }
            }
            cancel.cancel();
            while let Some(result) = in_flight.join_next().await {
                if let Err(err) = result {
                    log::error!(name = "cron"; "cron task failed during shutdown: {err}");
                }
            }
            self.store.flush(unix_ms()).await?;
            log::warn!(name = "cron"; "cron scheduler stopped");
            match failure {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }))
    }
}

fn install_workspace_grant(ctx: &BaseCtx, job: &CronJob, caller: Principal) {
    if let Some(path) = job
        .origin
        .as_ref()
        .and_then(|origin| origin.workspace_grant.as_ref())
    {
        ctx.set_state(CronWorkspaceGrant {
            caller,
            path: path.into(),
        });
    }
}

struct ShellCompletion {
    started: AtomicBool,
    sender: Mutex<Option<oneshot::Sender<ToolOutput<Value>>>>,
}

#[async_trait]
impl ToolBackgroundHook for ShellCompletion {
    async fn on_background_start(&self, _: &BaseCtx, _: BackgroundHandle, _: Value) {
        self.started.store(true, Ordering::SeqCst);
    }
    async fn on_background_end(&self, _: &BaseCtx, _: String, output: ToolOutput<Value>) {
        if let Some(sender) = self.sender.lock().take() {
            let _ = sender.send(output);
        }
    }
}

async fn run_shell(
    engine: &Engine,
    job: &CronJob,
    caller: Principal,
    meta: RequestMeta,
    cancel: &CancellationToken,
) -> Result<ToolOutput<Value>, BoxError> {
    if cancel.is_cancelled() {
        return Err("Cron scheduler stopped".into());
    }
    let name = engine.default_agent();
    // Validate direct-tool visibility before using local dispatch to carry the host hook.
    engine.base_ctx_with(caller, &name, ShellTool::NAME, meta.clone())?;
    let ctx = engine.ctx_with(caller, &name, &name, meta)?;
    let context_cancel = ctx.base.cancellation_token();
    let _cancel_on_drop = context_cancel.clone().drop_guard();
    install_workspace_grant(&ctx.base, job, caller);
    let (sender, receiver) = oneshot::channel();
    let hook = Arc::new(ShellCompletion {
        started: AtomicBool::new(false),
        sender: Mutex::new(Some(sender)),
    });
    ctx.base.set_state(DynToolJsonHook::new(hook.clone()));
    let (output, _) = ctx
        .tool_call(ToolInput {
            name: ShellTool::NAME.into(),
            args: json!(ExecArgs {
                command: job.job.clone(),
                background: true,
                ..Default::default()
            }),
            ..Default::default()
        })
        .await?;
    if hook.started.load(Ordering::SeqCst) {
        wait_for_completion(receiver, cancel, || context_cancel.cancel()).await
    } else {
        Ok(output)
    }
}

// A cancellation request is followed by bounded cleanup, so scheduler shutdown
// drains native process exits and accepted agent turns before flushing history.
async fn wait_for_completion<T>(
    mut receiver: oneshot::Receiver<T>,
    cancel: &CancellationToken,
    stop: impl FnOnce(),
) -> Result<T, BoxError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            stop();
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut receiver).await;
            Err("Cron scheduler stopped".into())
        }
        result = &mut receiver => result.map_err(|_| "Scheduled execution completion channel closed".into()),
    }
}

fn shell_result(output: ToolOutput<Value>) -> CronJobResult {
    let parsed = serde_json::from_value::<ExecOutput>(output.output.clone());
    let mut result: CronJobResult = output.into();
    match parsed {
        Ok(output)
            if matches!(
                output.exit_status.as_deref(),
                Some("exit status: 0" | "exit code: 0")
            ) => {}
        Ok(output) => {
            result.error.get_or_insert_with(|| {
                format!(
                    "Shell command failed ({}): {}",
                    output.exit_status.as_deref().unwrap_or("no exit status"),
                    result.result.as_deref().unwrap_or_default()
                )
            });
        }
        Err(err) => {
            result
                .error
                .get_or_insert_with(|| format!("Invalid shell result: {err}"));
        }
    }
    result
}

fn cron_shell_result_prompt(job: &CronJob, run_id: u64, result: &CronJobResult) -> String {
    let outcome = if let Some(error) = &result.error {
        format!("Shell command failed:\n\n{error}")
    } else if let Some(result) = &result.result {
        format!("Shell command completed:\n\n{result}")
    } else {
        "Shell command completed without a textual result.".to_string()
    };
    system_runtime_prompt(
        "cron shell job result",
        format!(
            "Scheduled shell job completed. Incorporate this result into the current conversation and tell the originating user the useful outcome succinctly.\n\nJob id: {}\nJob name: {}\nRun id: {}\nCommand:\n{}\n\n{}",
            job._id,
            job.name.as_deref().unwrap_or("unnamed"),
            run_id,
            job.job,
            outcome
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_result_prompt_reports_each_outcome() {
        let job = CronJob {
            _id: 5,
            origin: None,
            job_kind: JobKind::Shell,
            job: "echo hi".to_string(),
            schedule_kind: ScheduleKind::Every,
            schedule: "60".to_string(),
            tz: None,
            name: Some("heartbeat".to_string()),
            created_at: 0,
            updated_at: 0,
            next_run: 0,
            last_finished_at: None,
            last_result: None,
            last_error: None,
            last_conversation_id: None,
        };

        let failed = cron_shell_result_prompt(
            &job,
            9,
            &CronJobResult {
                error: Some("exit 1".to_string()),
                ..Default::default()
            },
        );
        assert!(failed.contains("Shell command failed"));
        assert!(failed.contains("heartbeat"));

        let ok = cron_shell_result_prompt(
            &job,
            9,
            &CronJobResult {
                result: Some("hi".to_string()),
                ..Default::default()
            },
        );
        assert!(ok.contains("Shell command completed"));

        let silent = cron_shell_result_prompt(&job, 9, &CronJobResult::default());
        assert!(silent.contains("without a textual result"));
    }

    #[tokio::test]
    async fn serve_skips_ticks_without_engine_and_stops_on_cancel() {
        use anda_engine::engine::EngineRef;

        let db = crate::test_support::memory_db("cron_serve").await;
        let runtime = CronRuntime::connect(Arc::new(EngineRef::new()), db)
            .await
            .unwrap();

        let cancel = CancellationToken::new();
        let handle = runtime.serve(cancel.clone()).await.unwrap();

        // The first tick fires immediately and is skipped because the engine
        // reference is unbound; cancellation then stops the scheduler.
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("scheduler should stop")
            .unwrap()
            .unwrap();
    }

    use anda_core::{
        Agent, AgentOutput, BoxError as CoreBoxError, FunctionDefinition, Json, Resource, Tool,
        ToolOutput,
    };
    use anda_engine::{
        context::{AgentCtx, BaseCtx},
        engine::AgentInfo,
        management::{BaseManagement, Visibility},
    };
    use std::collections::BTreeSet;

    struct EchoAgent;

    impl Agent<AgentCtx> for EchoAgent {
        fn name(&self) -> String {
            "echo_agent".to_string()
        }

        fn description(&self) -> String {
            "Echoes the prompt".to_string()
        }

        async fn run(
            &self,
            _ctx: AgentCtx,
            prompt: String,
            _resources: Vec<Resource>,
        ) -> Result<AgentOutput, CoreBoxError> {
            Ok(AgentOutput {
                content: format!("echo:{prompt}"),
                conversation: Some(77),
                ..Default::default()
            })
        }
    }

    struct FakeShellTool;

    impl Tool<BaseCtx> for FakeShellTool {
        type Args = Json;
        type Output = Json;

        fn name(&self) -> String {
            ShellTool::NAME.to_string()
        }

        fn description(&self) -> String {
            "Echoes shell args".to_string()
        }

        fn definition(&self) -> FunctionDefinition {
            FunctionDefinition {
                name: self.name(),
                description: self.description(),
                parameters: json!({"type": "object"}),
                strict: Some(false),
            }
        }

        async fn call(
            &self,
            _ctx: BaseCtx,
            args: Self::Args,
            _resources: Vec<Resource>,
        ) -> Result<ToolOutput<Self::Output>, CoreBoxError> {
            Ok(ToolOutput {
                output: json!({"stdout": args["command"], "exit_status": "exit status: 0"}),
                ..Default::default()
            })
        }
    }

    async fn test_engine() -> Arc<Engine> {
        engine_with(EchoAgent, FakeShellTool).await
    }

    async fn engine_with<A: Agent<AgentCtx> + 'static, T: Tool<BaseCtx> + 'static>(
        agent: A,
        tool: T,
    ) -> Arc<Engine> {
        Arc::new(
            Engine::builder()
                .with_info(AgentInfo {
                    handle: "cron_test".to_string(),
                    name: "Cron Test Engine".to_string(),
                    description: "Test engine".to_string(),
                    endpoint: "https://example.com/engine".to_string(),
                    ..Default::default()
                })
                .with_management(Arc::new(BaseManagement {
                    controller: Principal::management_canister(),
                    managers: BTreeSet::new(),
                    visibility: Visibility::Public,
                }))
                .register_tool(Arc::new(tool))
                .unwrap()
                .register_agent(Arc::new(agent), None)
                .unwrap()
                .export_tools(vec![ShellTool::NAME.to_string()])
                .build("echo_agent".to_string())
                .await
                .unwrap(),
        )
    }

    async fn test_runtime() -> CronRuntime {
        use anda_engine::engine::EngineRef;

        let db = crate::test_support::memory_db("cron_engine").await;
        CronRuntime::connect(Arc::new(EngineRef::new()), db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn process_due_job_runs_shell_and_agent_jobs() {
        let engine = test_engine().await;
        let runtime = test_runtime().await;

        // Nothing is due yet: the tick is a no-op.
        assert_eq!(
            runtime
                .process_due_jobs_once(
                    engine.clone(),
                    &mut JoinSet::new(),
                    &mut HashSet::new(),
                    &CancellationToken::new()
                )
                .await
                .unwrap(),
            0
        );

        let shell_job = runtime
            .store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Shell,
                    job: "echo hi".to_string(),
                    schedule_kind: ScheduleKind::Every,
                    schedule: "60".to_string(),
                    name: Some("shell-job".to_string()),
                    tz: None,
                },
                Some(CronJobOrigin {
                    source: Some("cli:/tmp/ws".to_string()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let run = runtime
            .store
            .job_start(shell_job._id, unix_ms())
            .await
            .unwrap();
        runtime
            .process_due_job(
                engine.clone(),
                shell_job.clone(),
                run,
                CancellationToken::new(),
            )
            .await;

        let finished = runtime.store.get_job(shell_job._id).await.unwrap();
        assert!(finished.last_finished_at.is_some());
        assert!(
            finished
                .last_result
                .as_deref()
                .is_some_and(|result| result.contains("echo hi")),
            "got: {:?}",
            finished.last_result
        );
        assert!(finished.last_error.is_none());
        // The shell result notification ran through the agent and recorded
        // the conversation id it returned.
        assert_eq!(finished.last_conversation_id, Some(77));

        let agent_job = runtime
            .store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Agent,
                    job: "summarize the day".to_string(),
                    schedule_kind: ScheduleKind::Every,
                    schedule: "60".to_string(),
                    name: Some("agent-job".to_string()),
                    tz: None,
                },
                None,
            )
            .await
            .unwrap();
        let run = runtime
            .store
            .job_start(agent_job._id, unix_ms())
            .await
            .unwrap();
        runtime
            .process_due_job(engine, agent_job.clone(), run, CancellationToken::new())
            .await;

        let finished = runtime.store.get_job(agent_job._id).await.unwrap();
        assert!(
            finished
                .last_result
                .as_deref()
                .is_some_and(|result| result.contains("echo:")),
            "got: {:?}",
            finished.last_result
        );
        assert_eq!(finished.last_conversation_id, Some(77));
    }

    struct DeferredAgent {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl Agent<AgentCtx> for DeferredAgent {
        fn name(&self) -> String {
            "echo_agent".into()
        }
        fn description(&self) -> String {
            "Defers actual work after accepting a request".into()
        }
        async fn run(
            &self,
            ctx: AgentCtx,
            _: String,
            _: Vec<Resource>,
        ) -> Result<AgentOutput, BoxError> {
            let receipt = ctx
                .base
                .get_state::<AgentSubmission>()
                .unwrap()
                .take()
                .unwrap();
            let release = self.release.clone();
            let token = ctx.base.cancellation_token();
            tokio::spawn(async move {
                tokio::select! {
                    _ = release.notified() => receipt.finish(CronJobResult { result: Some("actual completion".into()), conversation_id: Some(77), error: None }),
                    _ = token.cancelled() => drop(receipt),
                }
            });
            self.started.notify_one();
            Ok(AgentOutput {
                conversation: Some(77),
                ..Default::default()
            })
        }
    }

    async fn insert_due_agent(runtime: &CronRuntime) -> CronJob {
        let at = (unix_ms() / 1000 + 1) * 1000;
        let job = runtime
            .store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Agent,
                    job: "work".into(),
                    schedule_kind: ScheduleKind::At,
                    schedule: chrono::DateTime::from_timestamp_millis(at as i64)
                        .unwrap()
                        .to_rfc3339(),
                    name: None,
                    tz: None,
                },
                None,
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(at.saturating_sub(unix_ms()) + 5)).await;
        job
    }

    #[tokio::test]
    async fn cron_waits_for_agent_completion_and_shutdown_drains_owned_runs() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let engine = engine_with(
            DeferredAgent {
                started: started.clone(),
                release: release.clone(),
            },
            FakeShellTool,
        )
        .await;
        let runtime = test_runtime().await;
        let job = insert_due_agent(&runtime).await;
        let mut tasks = JoinSet::new();
        let mut running = HashSet::new();
        let cancel = CancellationToken::new();
        assert_eq!(
            runtime
                .process_due_jobs_once(engine.clone(), &mut tasks, &mut running, &cancel)
                .await
                .unwrap(),
            1
        );
        started.notified().await;
        assert!(
            runtime
                .store
                .get_job(job._id)
                .await
                .unwrap()
                .last_finished_at
                .is_none()
        );
        assert_eq!(
            runtime
                .process_due_jobs_once(engine.clone(), &mut tasks, &mut running, &cancel)
                .await
                .unwrap(),
            0
        );
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), tasks.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            runtime
                .store
                .get_job(job._id)
                .await
                .unwrap()
                .last_result
                .as_deref(),
            Some("actual completion")
        );

        let job = insert_due_agent(&runtime).await;
        runtime.engine.bind(Arc::downgrade(&engine));
        let handle = runtime.clone().serve(cancel.clone()).await.unwrap();
        started.notified().await;
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let job = runtime.store.get_job(job._id).await.unwrap();
        assert!(job.last_finished_at.is_some());
        assert!(job.last_error.unwrap().contains("stopped"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cron_shell_waits_for_background_exit_and_records_failures() {
        use anda_engine::extension::shell::NativeRuntime;
        let dir = tempfile::tempdir().unwrap();
        let tool = ShellTool::new(
            Arc::new(NativeRuntime::new(dir.path().into())),
            Default::default(),
            None,
        );
        let engine = engine_with(EchoAgent, tool).await;
        let runtime = test_runtime().await;
        for (command, fails) in [
            ("/bin/sleep 0.1; echo final-output", false),
            ("echo failed-output >&2; exit 7", true),
        ] {
            let job = runtime
                .store
                .insert_job(
                    CreateCronJobArgs {
                        job_kind: JobKind::Shell,
                        job: command.into(),
                        schedule_kind: ScheduleKind::Every,
                        schedule: "60".into(),
                        name: None,
                        tz: None,
                    },
                    None,
                )
                .await
                .unwrap();
            let output = run_shell(
                &engine,
                &job,
                Principal::management_canister(),
                RequestMeta::default(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert!(output.output["exit_status"].is_string());
            let result = shell_result(output);
            assert_eq!(result.error.is_some(), fails);
            if !fails {
                assert!(result.result.unwrap().contains("final-output"));
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cron_cancels_and_drains_a_running_shell_process() {
        use anda_engine::extension::shell::NativeRuntime;
        let dir = tempfile::tempdir().unwrap();
        let tool = ShellTool::new(
            Arc::new(NativeRuntime::new(dir.path().into())),
            Default::default(),
            None,
        );
        let engine = engine_with(EchoAgent, tool).await;
        let runtime = test_runtime().await;
        let job = runtime
            .store
            .insert_job(
                CreateCronJobArgs {
                    job_kind: JobKind::Shell,
                    job: "echo started > started; /bin/sleep 10; echo unexpected > leaked".into(),
                    schedule_kind: ScheduleKind::Every,
                    schedule: "60".into(),
                    name: None,
                    tz: None,
                },
                None,
            )
            .await
            .unwrap();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            run_shell(
                &engine,
                &job,
                Principal::management_canister(),
                RequestMeta::default(),
                &task_cancel,
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !dir.path().join("started").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        let error = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("scheduler stopped"));
        assert!(!dir.path().join("leaked").exists());
    }
}
