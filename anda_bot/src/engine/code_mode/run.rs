use super::{
    CodeMode, Failure, route,
    worker::{ARG, FRAME_LIMIT, SOURCE_LIMIT},
};
use crate::{
    engine::{
        action::live_request_meta, agent::memory_policy::MemoryPolicy,
        write_daemon_config_atomically,
    },
    util::request_meta::{keys, request_meta_extra_as},
};
use anda_core::{
    BoxError, ContentPart, Message, Principal, RequestMeta, Resource, StateFeatures, ToolOutput,
    Usage,
};
use anda_engine::{
    context::{BaseCtx, CallableDispatch},
    memory::Conversation,
};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{ChildStdin, ChildStdout, Command},
    sync::{Mutex, OwnedSemaphorePermit},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize, Deserialize)]
struct Receipt {
    child_id: u32,
    tool: String,
    status: String,
    argument_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Record {
    run_id: String,
    conversation_id: u64,
    caller: String,
    route: Value,
    outer_call_id: Option<String>,
    calls: Vec<Receipt>,
    revision: Option<String>,
}
impl Record {
    fn reconcile(&mut self) {
        for call in &mut self.calls {
            match call.status.as_str() {
                "dispatching" => call.status = "outcome_unknown".into(),
                "prepared" => call.status = "cancelled".into(),
                _ => {}
            }
        }
    }
    fn summary(&self) -> Value {
        let mut counts = BTreeMap::new();
        for call in &self.calls {
            *counts.entry(call.status.as_str()).or_insert(0u32) += 1;
        }
        const MAX_SHOWN: usize = 48;
        let mut calls: Vec<_> = self.calls.iter().collect();
        calls.sort_by_key(|c| match c.status.as_str() {
            "outcome_unknown" | "dispatching" => 0,
            "completed" => 1,
            _ => 2,
        });
        let omitted = calls.len().saturating_sub(MAX_SHOWN);
        calls.truncate(MAX_SHOWN);
        let receipts: Vec<_> = calls
            .iter()
            .map(|c| json!({"child_id":c.child_id,"tool":c.tool,"status":c.status,"target":c.target}))
            .collect();
        json!({"run_id":self.run_id,"counts":counts,"receipts":receipts,"omitted":omitted})
    }
}

struct Journal {
    path: Option<PathBuf>,
    record: Mutex<Record>,
    digest_key: [u8; 48],
}
impl Journal {
    async fn persist(&self, record: &Record) -> Result<(), Failure> {
        if let Some(p) = &self.path {
            let parent = p
                .parent()
                .expect("run records have a conversation directory");
            tokio::fs::create_dir_all(parent).await?;
            write_daemon_config_atomically(p, &serde_json::to_vec(record)?)
                .await
                .map_err(Failure::io)?;
            #[cfg(unix)]
            tokio::fs::File::open(parent).await?.sync_all().await?;
        }
        Ok(())
    }
    async fn start(&self, name: &str, args: &Value) -> Result<u32, Failure> {
        let mut record = self.record.lock().await;
        if record.calls.len() >= 128 {
            return Err(Failure::new("resource_limit", "128 child calls per script"));
        }
        let id = record.calls.len() as u32 + 1;
        let mut canonical = args.clone();
        canonical.sort_all_objects();
        let mut bytes = self.digest_key.to_vec();
        serde_json::to_writer(&mut bytes, &(name, canonical))?;
        let argument_digest = crate::engine::app_protocol::hash(&bytes);
        let target = if name.eq_ignore_ascii_case("send_im_message") {
            let mut bytes = self.digest_key.to_vec();
            serde_json::to_writer(
                &mut bytes,
                &(&args["channel"], &args["recipient"], &args["thread"]),
            )?;
            Some(crate::engine::app_protocol::hash(&bytes)[..16].to_owned())
        } else {
            None
        };
        record.calls.push(Receipt {
            child_id: id,
            tool: name.into(),
            status: "prepared".into(),
            argument_digest,
            target,
        });
        self.persist(&record).await?;
        Ok(id)
    }
    async fn status(&self, id: u32, status: &str) -> Result<(), Failure> {
        let mut record = self.record.lock().await;
        if let Some(call) = record.calls.get_mut(id.saturating_sub(1) as usize) {
            call.status = status.into();
        }
        self.persist(&record).await
    }
}

#[derive(Default)]
struct Accounting {
    usage: Usage,
    tools: HashMap<String, Usage>,
    artifacts: Vec<Resource>,
}

struct Pipes {
    input: ChildStdin,
    output: ChildStdout,
    aborted: Option<Failure>,
}
impl Pipes {
    async fn send(&mut self, value: &Value) -> Result<(), Failure> {
        if let Some(err) = &self.aborted {
            return Err(err.clone());
        }
        let bytes = serde_json::to_vec(value)?;
        if bytes.len() > FRAME_LIMIT {
            return Err(Failure::new("resource_limit", "worker frame exceeds 1 MiB"));
        }
        self.input.write_u8(super::worker::VERSION).await?;
        self.input.write_u32(bytes.len() as u32).await?;
        self.input.write_all(&bytes).await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn read(&mut self) -> Result<Value, Failure> {
        let version =
            self.output.read_u8().await.map_err(|e| {
                Failure::new("worker_exited", format!("worker closed its pipe: {e}"))
            })?;
        if version != super::worker::VERSION {
            return Err(Failure::new(
                "protocol_error",
                "unsupported Code Mode protocol version",
            ));
        }
        let len =
            self.output.read_u32().await.map_err(|e| {
                Failure::new("worker_exited", format!("worker closed its pipe: {e}"))
            })? as usize;
        if len > FRAME_LIMIT {
            return Err(Failure::new("resource_limit", "worker frame exceeds 1 MiB"));
        }
        let mut bytes = vec![0; len];
        self.output.read_exact(&mut bytes).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}
#[derive(Clone)]
struct WorkerRun {
    id: String,
    agent: String,
    owner: Principal,
    route: Value,
    lanes: Arc<tokio::sync::Semaphore>,
    pipes: Arc<Mutex<Pipes>>,
    journal: Arc<Journal>,
    accounting: Arc<Mutex<Accounting>>,
}
#[derive(Clone)]
pub(crate) struct CallSite {
    run: WorkerRun,
    pub(crate) batch_id: Option<String>,
    pub(crate) child_id: u32,
    siblings: Arc<Mutex<()>>,
    permit: Arc<parking_lot::Mutex<Option<OwnedSemaphorePermit>>>,
    stack: Vec<String>,
}
impl CallSite {
    pub(crate) fn is_direct(&self, ctx: &BaseCtx) -> bool {
        self.run.agent == ctx.agent
    }

    pub(crate) fn root_id(&self) -> &str {
        &self.run.id
    }
    pub(crate) fn release(&self) {
        self.permit.lock().take();
    }
    pub(crate) async fn resume(&self, ctx: &BaseCtx) -> Result<(), BoxError> {
        let permit = self.run.lanes.clone().acquire_owned().await?;
        if self.run.owner != *ctx.caller() || self.run.route != route(&live_request_meta(ctx)) {
            return Err("script request identity or route changed".into());
        }
        *self.permit.lock() = Some(permit);
        self.mark("dispatching").await
    }
    pub(crate) async fn mark(&self, status: &str) -> Result<(), BoxError> {
        self.run
            .journal
            .status(self.child_id, status)
            .await
            .map_err(|e| e.message.into())
    }
}

impl CodeMode {
    /// Check the actual worker image and OS policy before advertising tools.
    pub(crate) async fn verify_platform(&self) -> Result<(), BoxError> {
        let sentinel = tempfile::NamedTempFile::new()?;
        let cancel = CancellationToken::new();
        let _guard = cancel.clone().drop_guard();
        let permit = self.slots.clone().acquire_owned().await?;
        let check = async {
            let mut pipes = spawn(permit, cancel).await?;
            pipes
                .send(&json!({"op":"check_isolation","file":sentinel.path()}))
                .await?;
            let result = pipes.read().await?;
            if result["op"] == "failed" {
                return Err(Failure::new(
                    "isolation_failed",
                    result["message"]
                        .as_str()
                        .unwrap_or("worker sandbox unavailable"),
                ));
            }
            if result != json!({"op":"isolated"}) {
                return Err(Failure::new(
                    "isolation_failed",
                    "worker could access a host resource",
                ));
            }
            Ok(())
        };
        tokio::time::timeout(Duration::from_secs(15), check)
            .await?
            .map_err(|e| format!("Code Mode isolation check failed: {}", e.message).into())
    }

    pub(super) async fn run(
        &self,
        ctx: &BaseCtx,
        request: Value,
    ) -> Result<ToolOutput<Value>, Failure> {
        let source = request["source"].as_str().unwrap_or_default();
        if source.trim().is_empty() || source.len() > SOURCE_LIMIT {
            return Err(Failure::new(
                "invalid_source",
                "provide 1..65536 bytes of Python",
            ));
        }
        let dispatch = ctx.get_state::<CallableDispatch>().ok_or_else(|| {
            Failure::new(
                "scope_denied",
                "Code Mode requires an active agent tool turn",
            )
        })?;
        let site = ctx.get_state::<CallSite>();
        if let Some(site) = &site {
            site.release();
        }
        let name = request["name"].as_str().map(str::to_owned);
        if let Some(site) = &site
            && site.is_direct(ctx)
            && site.run.owner == *ctx.caller()
            && request["op"] != "exec"
        {
            if name.as_ref().is_some_and(|n| site.stack.contains(n)) {
                let mut chain = site.stack.clone();
                chain.extend(name.clone());
                return Err(Failure {
                    code: "script_cycle".into(),
                    message: "recursive saved script call".into(),
                    data: json!({"call_chain":chain}),
                });
            }
            let _lane = site.siblings.lock().await;
            let mut stack = site.stack.clone();
            stack.extend(name);
            let result = Box::pin(site.run.evaluate(ctx, &dispatch, request, stack)).await?;
            return Ok(ToolOutput::new(json!({"result":result})));
        }
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Failure {
                code: "resource_limit".into(),
                message: "Code Mode workers are busy".into(),
                data: json!({"reason":"worker_busy"}),
            })?;
        let run_ctx = ctx.cancellable_child();
        let cancel = run_ctx.cancellation_token();
        let _guard = cancel.clone().drop_guard();
        let meta = live_request_meta(ctx);
        let conv = request_meta_extra_as::<u64>(&meta, keys::CONVERSATION).unwrap_or_default();
        let id = ic_auth_types::Xid::new().to_string();
        let path = (conv > 0 && MemoryPolicy::current(ctx).may_write()).then(|| {
            self.home
                .join("runs")
                .join(conv.to_string())
                .join(format!("{id}.json"))
        });
        let journal = site
            .as_ref()
            .map(|s| s.run.journal.clone())
            .unwrap_or_else(|| {
                Arc::new(Journal {
                    path,
                    digest_key: self.digest_key,
                    record: Mutex::new(Record {
                        run_id: id.clone(),
                        conversation_id: conv,
                        caller: ctx.caller().to_text(),
                        route: route(&meta),
                        outer_call_id: dispatch.call_id.clone(),
                        calls: Vec::new(),
                        revision: request["revision"].as_str().map(str::to_owned),
                    }),
                })
            });
        let pipes = spawn(permit, cancel.clone()).await?;
        let lanes = site
            .as_ref()
            .map(|s| s.run.lanes.clone())
            .unwrap_or_else(|| Arc::new(tokio::sync::Semaphore::new(4)));
        let root_id = site.as_ref().map(|s| s.run.id.clone()).unwrap_or(id);
        let run = WorkerRun {
            id: root_id,
            agent: ctx.agent.clone(),
            owner: *ctx.caller(),
            route: route(&meta),
            lanes,
            pipes: Arc::new(Mutex::new(pipes)),
            journal,
            accounting: Default::default(),
        };
        let result = tokio::select! {
            _=cancel.cancelled()=>Err(Failure::new("cancelled","script cancelled")),
            res=tokio::time::timeout(Duration::from_secs(20*60),Box::pin(run.evaluate(&run_ctx,&dispatch,request,name.into_iter().collect())))=>res.unwrap_or_else(|_|Err(Failure::new("resource_limit","script wall time exceeded"))),
        };
        let mut output = match result {
            Ok(value) => ToolOutput::new(json!({"result":value})),
            Err(mut e) => {
                let mut record = run.journal.record.lock().await;
                if site.is_none() {
                    record.reconcile();
                }
                let mut data = record.summary();
                if let Some(shape) = e.data.get("output_shape") {
                    data["output_shape"] = shape.clone();
                }
                if !e.data.is_null() {
                    data["details"] = e.data;
                }
                e.data = data;
                e.output()
            }
        };
        let accounting = std::mem::take(&mut *run.accounting.lock().await);
        output.usage = accounting.usage;
        output.tools_usage = accounting.tools;
        output.artifacts = accounting.artifacts;
        // This marker allows the conversation writer to retire the journal only
        // after the outer result is durable.
        if site.is_none()
            && let Some(path) = &run.journal.path
            && let Some(done) = ctx.get_state::<FinishedRuns>()
        {
            done.track(
                path.clone(),
                RunCompletion::ToolResult(dispatch.call_id.clone()),
            );
        }
        Ok(output)
    }

    pub(crate) async fn prepare_recovery(
        &self,
        ctx: &BaseCtx,
        meta: &RequestMeta,
        conversation: &Conversation,
    ) -> Option<String> {
        ctx.set_state(FinishedRuns::default());
        // The replay history drops unanswered calls. Read the saved messages
        // so recovery can still match the original call IDs.
        let recorded: Vec<Message> = conversation
            .messages
            .iter()
            .filter_map(|message| Message::deserialize(message).ok())
            .collect();
        match self.recovery(ctx, meta, conversation._id, &recorded).await {
            Ok(summary) => summary,
            Err(err) => {
                log::warn!(conversation=conversation._id; "Code Mode receipts could not be read: {err}");
                ctx.set_state(RecoveryHold::default());
                Some("Code Mode receipts could not be read. Ask the user to check the outcome before starting new operations.".into())
            }
        }
    }

    pub(super) async fn recovery(
        &self,
        ctx: &BaseCtx,
        meta: &RequestMeta,
        conv: u64,
        history: &[Message],
    ) -> Result<Option<String>, BoxError> {
        if !MemoryPolicy::current(ctx).may_write() {
            let mut pending = std::collections::BTreeSet::new();
            for part in history.iter().flat_map(|m| &m.content) {
                match part {
                    ContentPart::ToolCall { name, call_id, .. }
                        if name == super::EXEC || name.starts_with("script__") =>
                    {
                        pending.insert((name, call_id));
                    }
                    ContentPart::ToolOutput { name, call_id, .. } => {
                        pending.remove(&(name, call_id));
                    }
                    _ => {}
                }
            }
            if pending.is_empty() {
                return Ok(None);
            }
            ctx.set_state(RecoveryHold::default());
            return Ok(Some("This conversation does not retain Code Mode receipts. Ask the user to reconcile any interrupted operations before continuing.".into()));
        }
        let path = self.home.join("runs").join(conv.to_string());
        let mut entries = match tokio::fs::read_dir(path).await {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut summaries = Vec::new();
        while let Some(e) = entries.next_entry().await? {
            if e.path().extension().is_none_or(|v| v != "json") || !e.file_type().await?.is_file() {
                continue;
            }
            let mut rec: Record = serde_json::from_slice(&tokio::fs::read(e.path()).await?)?;
            if rec.conversation_id != conv
                || rec.caller != ctx.caller().to_text()
                || rec.route != route(meta)
            {
                continue;
            }
            if rec.outer_call_id.as_ref().is_some_and(|id| {
                history.iter().flat_map(|m| &m.content).any(|p| {
                    matches!(p, ContentPart::ToolOutput { call_id: Some(call), .. } if call == id)
                })
            }) {
                remove_receipt(e.path()).await;
                continue;
            }
            rec.reconcile();
            summaries.push(rec.summary());
            if let Some(done) = ctx.get_state::<FinishedRuns>() {
                done.track(e.path(), RunCompletion::UserReply);
            }
            if summaries.len() >= 16 {
                break;
            }
        }
        if summaries.is_empty() {
            return Ok(None);
        }
        ctx.set_state(RecoveryHold::default());
        Ok(Some(format!(
            "Interrupted Code Mode operations. Report completed calls and reconcile unknown outcomes with the user before starting new actions. {}",
            serde_json::to_string(&summaries)?
        )))
    }
}
#[derive(Clone, Default)]
pub(crate) struct FinishedRuns(Arc<parking_lot::Mutex<Vec<FinishedRun>>>);
struct FinishedRun {
    path: PathBuf,
    completion: RunCompletion,
}
enum RunCompletion {
    ToolResult(Option<String>),
    UserReply,
}
impl FinishedRuns {
    fn track(&self, path: PathBuf, completion: RunCompletion) {
        self.0.lock().push(FinishedRun { path, completion });
    }
    pub(crate) async fn acknowledge(&self, messages: &[Value], user_replied: bool) {
        let paths: Vec<_> = self
            .0
            .lock()
            .extract_if(.., |run| match &run.completion {
                RunCompletion::UserReply => user_replied,
                RunCompletion::ToolResult(None) => true,
                RunCompletion::ToolResult(Some(id)) => messages
                    .iter()
                    .filter_map(|m| m["content"].as_array())
                    .flatten()
                    .any(|p| p["type"] == "ToolOutput" && p["callId"].as_str() == Some(id)),
            })
            .map(|run| run.path)
            .collect();
        for p in paths {
            remove_receipt(p).await;
        }
    }
}

async fn remove_receipt(path: PathBuf) {
    if let Err(err) = tokio::fs::remove_file(path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("retiring Code Mode run: {err}");
    }
}

#[derive(Clone, Default)]
pub(crate) struct RecoveryHold(Arc<std::sync::atomic::AtomicBool>);
impl RecoveryHold {
    pub(crate) fn waiting(&self) -> bool {
        !self.0.load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn user_replied(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

async fn spawn(permit: OwnedSemaphorePermit, cancel: CancellationToken) -> Result<Pipes, Failure> {
    let empty = tempfile::tempdir()?;
    let executable = std::env::current_exe()?;
    // Cargo builds the application alongside its integration tests. Unit
    // tests run from the test harness in deps/, which has a different main.
    #[cfg(test)]
    let executable = {
        let profile = executable.parent().unwrap().parent().unwrap();
        let name = format!("anda{}", std::env::consts::EXE_SUFFIX);
        let adjacent = profile.join(&name);
        if adjacent.is_file() {
            adjacent
        } else {
            let target = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
            target.join(profile.file_name().unwrap()).join(name)
        }
    };
    let mut cmd = Command::new(executable);
    cmd.arg(ARG)
        .env_clear()
        // mimalloc normally reserves a 1 GiB arena before Rust main. Direct
        // mappings keep each guest allocation inside the OS address-space limit.
        .env(super::worker::ALLOCATOR_OPTION, "1")
        .current_dir(empty.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let pipes = Pipes {
        input: child.stdin.take().unwrap(),
        output: child.stdout.take().unwrap(),
        aborted: None,
    };
    tokio::spawn(async move {
        let _empty = empty;
        let _permit = permit;
        tokio::select! {_=child.wait()=>{},_=cancel.cancelled()=>{let _=child.kill().await;let _=child.wait().await;}}
    });
    Ok(pipes)
}

impl WorkerRun {
    async fn evaluate(
        &self,
        ctx: &BaseCtx,
        dispatch: &CallableDispatch,
        request: Value,
        stack: Vec<String>,
    ) -> Result<Value, Failure> {
        let inspect = request["op"] == "inspect";
        let reply = match self.exchange(ctx, dispatch, request, stack).await {
            Ok(reply) => reply,
            Err(err) => {
                // A nested invocation shares the pipe with its parent. If
                // the host stopped mid-exchange, the worker is still waiting
                // for a reply to that invocation and cannot resume its parent.
                self.pipes.lock().await.aborted = Some(err.clone());
                return Err(err);
            }
        };
        if reply["op"] == "failed" {
            return Err(Failure {
                code: reply["code"].as_str().unwrap_or("script_error").into(),
                message: reply["message"].as_str().unwrap_or("worker failed").into(),
                data: reply
                    .get("output_shape")
                    .map(|shape| json!({"output_shape":shape}))
                    .unwrap_or_else(|| reply["data"].clone()),
            });
        }
        let value = &reply["value"];
        if !inspect {
            let bytes = serde_json::to_vec(&value["value"])?.len();
            if bytes > super::worker::OUTPUT_LIMIT
                || value["stdout"].as_str().is_none_or(|s| s.len() > 4096)
            {
                return Err(Failure {
                    code: "resource_limit".into(),
                    message: "worker result exceeds its output budget".into(),
                    data: json!({"output_shape":{"bytes":bytes}}),
                });
            }
        }
        Ok(value.clone())
    }

    async fn exchange(
        &self,
        ctx: &BaseCtx,
        dispatch: &CallableDispatch,
        request: Value,
        stack: Vec<String>,
    ) -> Result<Value, Failure> {
        let inspect = request["op"] == "inspect";
        self.pipes.lock().await.send(&request).await?;
        loop {
            let message = self.pipes.lock().await.read().await?;
            match message["op"].as_str() {
                Some("done" | "failed") => return Ok(message),
                Some("calls") if !inspect => {
                    let calls: Vec<(String, Value)> =
                        serde_json::from_value(message["calls"].clone())?;
                    if calls.len() > 128 {
                        return Err(Failure::new(
                            "resource_limit",
                            "batch exceeds root call budget",
                        ));
                    }
                    let batch = message["batch"].as_bool() == Some(true);
                    if !batch && calls.len() != 1 {
                        return Err(Failure::new("invalid_arguments", "invalid sequential call"));
                    }
                    let batch_id = batch.then(|| ic_auth_types::Xid::new().to_string());
                    let siblings = Arc::new(Mutex::new(()));
                    let mut jobs = Vec::with_capacity(calls.len());
                    if calls
                        .iter()
                        .any(|(name, args)| name.is_empty() || name.len() > 64 || !args.is_object())
                    {
                        return Err(Failure::new(
                            "invalid_arguments",
                            "every call needs a tool name and a named argument object",
                        ));
                    }
                    for (name, args) in calls {
                        let id = self.journal.start(&name, &args).await?;
                        let site = CallSite {
                            run: self.clone(),
                            batch_id: batch_id.clone(),
                            child_id: id,
                            siblings: siblings.clone(),
                            permit: Default::default(),
                            stack: stack.clone(),
                        };
                        jobs.push(self.child(ctx, dispatch, name, args, site));
                    }
                    // Poll all approvals. Each call separately takes one of the four
                    // execution permits and releases it while a card is waiting.
                    let items: Vec<_> = stream::iter(jobs).buffered(128).collect().await;
                    let reply = json!({"op":"results","items":items});
                    self.pipes.lock().await.send(&reply).await?;
                }
                _ => {
                    return Err(Failure::new(
                        "protocol_error",
                        "unexpected worker operation",
                    ));
                }
            }
        }
    }

    async fn child(
        &self,
        ctx: &BaseCtx,
        dispatch: &CallableDispatch,
        name: String,
        args: Value,
        site: CallSite,
    ) -> Value {
        let id = site.child_id;
        let result = self.dispatch_child(ctx, dispatch, &name, args, site).await;
        let status = match &result {
            Ok(_) => "completed",
            Err(e)
                if matches!(
                    e.code.as_str(),
                    "scope_denied" | "approval_denied" | "approval_required" | "external_user"
                ) =>
            {
                "denied"
            }
            Err(_) => "failed",
        };
        if let Err(e) = self.journal.status(id, status).await {
            return json!({"error":e});
        }
        match result {
            Ok(v) => json!({"value":v}),
            Err(e) => json!({"error":e}),
        }
    }
    async fn dispatch_child(
        &self,
        ctx: &BaseCtx,
        dispatch: &CallableDispatch,
        name: &str,
        args: Value,
        site: CallSite,
    ) -> Result<Value, Failure> {
        let name = name.to_ascii_lowercase();
        if matches!(name.as_str(), super::EXEC | "tools_select" | "tools_search")
            || !dispatch.permits(ctx, &name)
        {
            return Err(Failure::new(
                "scope_denied",
                "tool is not permitted in this script",
            ));
        }
        let child = ctx.with_caller(*ctx.caller());
        site.resume(&child)
            .await
            .map_err(|err| Failure::new("scope_denied", err.to_string()))?;
        child.set_state(site.clone());
        let result = Box::pin(dispatch.call(&child, name.clone(), args)).await;
        // Background work may keep a context clone alive after the tool has
        // returned. The execution lane belongs to this dispatch only.
        site.release();
        let mut output = result.map_err(|err| {
            let message = err.to_string();
            let code = if message.contains("action denied:")
                || (message.contains("approval for the") && message.contains("expired:"))
            {
                "approval_denied"
            } else if message.contains("approval_required")
                || message.contains("nobody can")
                || message.contains("requires user approval")
            {
                "approval_required"
            } else if message.contains("external users may only") {
                "scope_denied"
            } else {
                "tool_error"
            };
            Failure::new(code, message)
        })?;
        {
            let mut accounting = self.accounting.lock().await;
            accounting.usage.accumulate(&output.usage);
            let mut usage = output.usage.clone();
            usage.requests = 1;
            accounting
                .tools
                .entry(name.clone())
                .or_default()
                .accumulate(&usage);
            for (tool, usage) in &output.tools_usage {
                accounting
                    .tools
                    .entry(tool.clone())
                    .or_default()
                    .accumulate(usage);
            }
            accounting.artifacts.append(&mut output.artifacts);
        }
        let error = &output.output["error"];
        if output.is_error == Some(true) || !error.is_null() {
            return Err(Failure {
                code: error["code"].as_str().unwrap_or("tool_error").into(),
                message: error["message"]
                    .as_str()
                    .or_else(|| error.as_str())
                    .or_else(|| output.output.as_str())
                    .unwrap_or("tool failed")
                    .into(),
                data: json!({"details":error.get("data"),"partial_result":output.output.get("result")}),
            });
        }
        if name.starts_with("script__") {
            Ok(output.output["result"].take())
        } else {
            Ok(output.output)
        }
    }
}
