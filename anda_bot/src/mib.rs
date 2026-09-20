//! Trusted, local-only MIB host. No production paths or normal Bot services are
//! opened here. A request remains owned by the host if its HTTP waiter drops.
use crate::engine::AndaBot;
use anda_brain::{
    agents::SELF_USER_ID,
    experiments::{CostStage, Experiment, ExperimentIdentity, MemoryMode, StageCost},
    recall_budget::RecallBudget,
    space::{AppState, ProcessingState},
    types::{FormationInput, InputContext, MaintenanceInput, MaintenanceScope, RecallInput},
};
use anda_core::{AgentOutput, BoxError, FunctionDefinition, Message};
use anda_db::{database::DBConfig, storage::StorageConfig};
use anda_engine::{
    management::{BaseManagement, Visibility},
    model::{ModelConfig, Models},
    unix_ms,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    routing::get,
};
use clap::{Args, ValueEnum};
use futures::FutureExt;
use object_store::memory::InMemory;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const AGENT: &str = "mib-agent/0.1";
const MEMORY: &str = "mib-memory-backend/0.1";
const MAX_REQUESTS: usize = 10_000;
const MAX_LEDGER_BYTES: usize = 32 * 1024 * 1024;
const MAX_TRANSIENT_BYTES: usize = 1024 * 1024;
const MAX_RUNS: usize = 1024;
const MAX_ACTIVE_RUNS: usize = 8;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Mode {
    Persistent,
    NoMemory,
}

#[derive(Args)]
pub struct MibCommand {
    /// JSON Anda Engine ModelConfig; API key may be supplied through --api-key-env.
    #[arg(long)]
    pub model_config: PathBuf,
    #[arg(long, default_value = "MIB_MODEL_API_KEY")]
    pub api_key_env: String,
    #[arg(long, default_value = "127.0.0.1:8043")]
    pub listen: SocketAddr,
    #[arg(long, value_enum, default_value = "persistent")]
    pub memory_mode: Mode,
    #[arg(long, default_value_t = 180)]
    pub timeout_seconds: u64,
    #[arg(long, default_value_t = 900)]
    pub idle_seconds: u64,
    #[arg(long, default_value_t = 4096)]
    pub max_output_tokens: usize,
    /// Force the Recall memory packet cap; supply both Recall limits together.
    #[arg(long, requires = "recall_context_tokens")]
    pub recall_max_tokens: Option<u32>,
    /// Cumulative normalized Recall planner input cap, using the fixed Brain codec.
    #[arg(long, requires = "recall_max_tokens")]
    pub recall_context_tokens: Option<u32>,
}
impl MibCommand {
    fn recall_budget(&self) -> Result<Option<RecallBudget>, BoxError> {
        match (self.recall_max_tokens, self.recall_context_tokens) {
            (None, None) => Ok(None),
            (Some(max_tokens), Some(context_tokens)) => {
                let budget = RecallBudget {
                    max_tokens,
                    context_tokens,
                    ..Default::default()
                };
                budget.validate()?;
                Ok(Some(budget))
            }
            _ => Err("both Recall budget limits are required".into()),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    mib: String,
    protocol: String,
    run_id: String,
    request_id: String,
    operation: String,
    #[serde(default)]
    virtual_time: Option<String>,
    #[serde(default)]
    body: Value,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    observation_id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    virtual_time: Option<String>,
    #[serde(default)]
    actor: Option<Value>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    tool: Option<String>,
}

type RequestKey = (String, String, String);
struct RequestEntry {
    digest: String,
    result: tokio::sync::OnceCell<Value>,
}

struct Stored {
    digest: String,
    response: Value,
}
#[derive(Default)]
struct RunState {
    ledger: BTreeMap<String, Stored>,
    ledger_bytes: usize,
    invalid: bool,
    closed: bool,
    transient: Vec<Value>,
    active_task: Option<Value>,
    pending_tool: Option<(String, String)>,
    seen_observations: BTreeMap<String, String>,
    costs: Value,
    brain_costs: Value,
    business_time: u64,
}
struct Run {
    brain: Arc<Experiment>,
    state: Mutex<RunState>,
    cancel: CancellationToken,
    touched: AtomicU64,
    closed: AtomicBool,
    business_costs: std::sync::Mutex<Vec<StageCost>>,
}
struct Host {
    app: AppState,
    models: Arc<Models>,
    identity: ExperimentIdentity,
    mode: Mode,
    timeout: Duration,
    idle: Duration,
    max_output: usize,
    recall_budget: Option<RecallBudget>,
    runs: Mutex<HashMap<(String, String), Arc<Run>>>,
    requests: Mutex<HashMap<RequestKey, Arc<RequestEntry>>>,
    tasks: TaskTracker,
    shutdown: CancellationToken,
    epoch: String,
}

fn digest(value: &impl Serialize) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON serialization"))
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn brain_prompts_digest() -> String {
    use anda_brain::agents::prompts::{PromptTarget, active_prompt, mode_reference};
    digest(
        &[
            PromptTarget::Formation,
            PromptTarget::Recall,
            PromptTarget::Maintenance,
        ]
        .map(|p| (mode_reference(p), active_prompt(p).to_string())),
    )
}
fn envelope(req: &Request, body: Value) -> Value {
    json!({"mib":"0.1","protocol":req.protocol,"run_id":req.run_id,"request_id":req.request_id,"status":"ok","body":body})
}
fn failure(req: &Request, code: &str, message: impl ToString) -> Value {
    json!({"mib":"0.1","protocol":req.protocol,"run_id":req.run_id,"request_id":req.request_id,"status":"error","error":{"code":code,"message":message.to_string(),"retryable":false}})
}
fn timestamp(time: Option<&str>) -> Result<u64, BoxError> {
    let Some(time) = time else {
        return Ok(0);
    };
    let ms = chrono::DateTime::parse_from_rfc3339(time)?.timestamp_millis();
    u64::try_from(ms).map_err(Into::into)
}
fn time_string(ms: u64) -> Result<String, BoxError> {
    Ok(chrono::DateTime::from_timestamp_millis(i64::try_from(ms)?)
        .ok_or("invalid business time")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
fn required<'a>(body: &'a Value, key: &str) -> Result<&'a str, BoxError> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing or invalid {key}").into())
}

fn observation_role(kind: &str) -> &'static str {
    match kind {
        "conversation" | "agent_message" | "action" => "assistant",
        "tool_result" => "tool",
        _ => "user",
    }
}

impl Host {
    fn descriptor(&self, protocol: &str) -> Value {
        let memory = protocol == MEMORY;
        let capabilities = if memory {
            json!({"observe":true,"retrieve":true,"maintenance":true,"session_boundary":true,"virtual_time":true,"run_isolation":true})
        } else {
            json!({"observe":true,"respond":true,"act":true,"spontaneous_emissions":false,"maintenance":true,"runner_managed_tools":true,"structured_output":true,"virtual_time":true,"seedable":false,"session_boundary":true})
        };
        json!({"protocol":protocol,"implementation":{"name":if memory {"anda-brain"} else {"anda-bot-with-anda-brain"},"version":crate::config::APP_VERSION,"vendor":"ldclabs"},
            "identity":{"model_digest":self.identity.model_digest,"tools_digest":self.identity.tools_digest,"budget_digest":self.identity.budget_digest,"prompts_digest":brain_prompts_digest()},"configuration_digest":digest(&json!({"runtime":self.identity,"prompts":brain_prompts_digest(),"memory_mode":format!("{:?}",self.mode),"recall_budget":self.recall_budget})),
            "track_support":if memory {vec!["memory_system"]} else {vec!["integrated_agent"]},"capabilities":capabilities,
            "state":{"run_isolation":"hard","observe_visibility":"read_after_write","request_idempotency":true},
            "extensions":{"memory_backend":{"protocol":MEMORY,"http_prefix":"/mib-memory/v0.1"},
                "mib.learning_longitudinal.v1":{
                    "mode":if self.mode==Mode::NoMemory {"no_memory"}else{"persistent"},
                    "configuration_digest":digest(&json!({"identity":self.identity,"mode":format!("{:?}",self.mode),"recall_budget":self.recall_budget})),
                    "business_identity":{"model_digest":self.identity.model_digest,"tools_digest":self.identity.tools_digest,"budget_digest":self.identity.budget_digest,
                        "business_prompt_digest":digest(&AndaBot::runner_managed_request(&json!({}), "", &[], vec![], "", self.max_output).instructions),"decoding_digest":digest(&json!({"temperature":0.0,"max_output_tokens":self.max_output}))},
                    "execution_guard_digest":digest(&"anda_bot_runner_managed_v1:runner_schemas_only;one_pending_tool;matching_feedback;no_local_effects;native_learning_unbound"),
                    "native_comparison_and_review":false,"independent_observer":false,"isolated_trial_application":false,
                    "native_task_family":null,"workflow_contract_digest":null,
                    "cross_task_state":self.mode==Mode::Persistent,"audit_operation":"learning_audit","recall_budget":self.recall_budget,
                    "accounting_complete":false},
                "anda_brain.host.v1":{"epoch":self.epoch,"memory_mode":if self.mode==Mode::Persistent {"persistent"}else{"no_memory"},"identity":self.identity,"execution_profile":"anda_bot_runner_managed","maintenance_policy":"fixed quick maintenance; requested budget is a recorded hint, not a token proof","learning":false,"idempotency_scope":"server process; run IDs cannot be reused","idle_seconds":self.idle.as_secs(),"max_output_tokens":self.max_output,"recall_budget":self.recall_budget,"business_prompt_digest":digest(&AndaBot::runner_managed_request(&json!({}), "", &[], vec![], "", self.max_output).instructions),"accounting_complete":false}}})
    }

    // This future is host-owned, not tied to the HTTP connection's lifetime.
    async fn dispatch(self: Arc<Self>, req: Request) -> Value {
        if req.operation == "describe" {
            return self.dispatch_inner(req).await;
        }
        let entry = {
            let mut requests = self.requests.lock().await;
            let key = (
                req.protocol.clone(),
                req.run_id.clone(),
                req.request_id.clone(),
            );
            let fp = digest(&req);
            if let Some(entry) = requests.get(&key) {
                if entry.digest != fp {
                    return failure(
                        &req,
                        "idempotency_conflict",
                        "request_id reused with different content",
                    );
                }
                entry.clone()
            } else {
                if requests.len() >= 100_000 && req.operation != "close" {
                    return failure(&req, "capacity", "host request capacity reached");
                }
                let entry = Arc::new(RequestEntry {
                    digest: fp,
                    result: tokio::sync::OnceCell::new(),
                });
                requests.insert(key, entry.clone());
                entry
            }
        };
        entry.result.get_or_init(|| async {
            let recovery_host=self.clone();
            let recovery_req=req.clone();
            match std::panic::AssertUnwindSafe(self.dispatch_inner(req)).catch_unwind().await {
                Ok(response)=>response,
                Err(_)=> {
                    let run=recovery_host.runs.lock().await.get(&(recovery_req.protocol.clone(),recovery_req.run_id.clone())).cloned();
                    let mut response=failure(&recovery_req,"execution_failure","host request panicked; run invalid and outcome unknown");
                    if let Some(run)=run {
                        run.cancel.cancel();
                        let mut state=run.state.lock().await;
                        state.invalid=true;
                        if let Ok(mut costs)=run.brain.cost_summary().await {
                            costs.receipts.extend(run.business_costs.lock().unwrap().iter().cloned());
                            costs.accounting_complete=false;
                            state.costs=json!(costs);
                        }
                        let cleanup=Self::close_brain(&run).await.err().map(|e|e.to_string());
                        response["extensions"]=json!({"costs":state.costs,"cost_scope":"cumulative_run","cleanup_error":cleanup});
                    }
                    response
                }
            }
        }).await.clone()
    }

    async fn dispatch_inner(self: Arc<Self>, req: Request) -> Value {
        if self.shutdown.is_cancelled() {
            return failure(&req, "closed", "host is closing");
        }
        if !req.body.is_object()
            || req.mib != "0.1"
            || ![AGENT, MEMORY].contains(&req.protocol.as_str())
            || req.run_id.is_empty()
            || req.run_id.len() > 256
            || req.request_id.is_empty()
            || req.request_id.len() > 256
        {
            return failure(
                &req,
                "invalid_request",
                "invalid protocol identity or correlation ID",
            );
        }
        if req.operation == "describe" {
            return envelope(&req, self.descriptor(&req.protocol));
        }
        let key = (req.protocol.clone(), req.run_id.clone());
        let run = {
            let mut runs = self.runs.lock().await;
            if let Some(run) = runs.get(&key) {
                run.clone()
            } else if req.operation != "reset" && req.operation != "close" {
                return failure(&req, "invalid_state", "run has not been reset");
            } else {
                if runs.len() >= MAX_RUNS
                    || (req.operation != "close"
                        && runs
                            .values()
                            .filter(|r| !r.closed.load(Ordering::SeqCst))
                            .count()
                            >= MAX_ACTIVE_RUNS)
                {
                    return failure(
                        &req,
                        "capacity",
                        "run capacity reached; close active runs or restart the host",
                    );
                }
                if req
                    .body
                    .get("mode")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "fresh")
                {
                    return failure(&req, "invalid_request", "only fresh reset is supported");
                }
                let now = match timestamp(
                    req.body
                        .get("virtual_time")
                        .and_then(Value::as_str)
                        .or(req.virtual_time.as_deref()),
                ) {
                    Ok(t) => t,
                    Err(e) => return failure(&req, "invalid_request", e),
                };
                let mode = if self.mode == Mode::NoMemory {
                    MemoryMode::SessionOnly
                } else {
                    MemoryMode::Persistent
                };
                let created = match &self.recall_budget {
                    Some(budget) => {
                        Experiment::create_with_recall_budget(
                            &self.app,
                            mode,
                            self.identity.clone(),
                            now,
                            budget.clone(),
                        )
                        .await
                    }
                    None => Experiment::create(&self.app, mode, self.identity.clone(), now).await,
                };
                let brain = match created {
                    Ok(b) => b,
                    Err(e) => return failure(&req, "execution_failure", e),
                };
                let run = Arc::new(Run {
                    brain: Arc::new(brain),
                    state: Mutex::new(RunState {
                        business_time: now,
                        ..Default::default()
                    }),
                    cancel: CancellationToken::new(),
                    touched: AtomicU64::new(unix_ms()),
                    closed: AtomicBool::new(false),
                    business_costs: std::sync::Mutex::new(vec![]),
                });
                runs.insert(key, run.clone());
                run
            }
        };
        run.touched.store(unix_ms(), Ordering::SeqCst);
        {
            if let Ok(state) = run.state.try_lock()
                && let Some(old) = state.ledger.get(&req.request_id)
            {
                return if digest(&req) == old.digest {
                    old.response.clone()
                } else {
                    failure(
                        &req,
                        "idempotency_conflict",
                        "request_id reused with different content",
                    )
                };
            }
        }
        if req.operation == "close" {
            run.cancel.cancel();
            // Signal Experiment before waiting for the request lock: a blocked
            // Recall/Formation waiter must be interrupted to release it.
            if let Err(e) = Self::close_brain(&run).await {
                return failure(&req, "cleanup_failure", e);
            }
        }
        let mut state = run.state.lock().await;
        let fp = digest(&req);
        if let Some(old) = state.ledger.get(&req.request_id) {
            return if fp == old.digest {
                old.response.clone()
            } else {
                failure(
                    &req,
                    "idempotency_conflict",
                    "request_id reused with different content",
                )
            };
        }
        if state.ledger.len() >= MAX_REQUESTS || state.ledger_bytes >= MAX_LEDGER_BYTES {
            state.invalid = true;
            run.cancel.cancel();
            let _ = Self::close_brain(&run).await;
            return failure(&req, "capacity", "request ledger exhausted; run invalid");
        }
        let started = Instant::now();
        let result = if req.operation == "close" {
            state.closed = true;
            state.transient.clear();
            state.active_task = None;
            state.pending_tool = None;
            Ok(json!({"closed":true,"costs":state.costs}))
        } else if state.closed || state.invalid || run.cancel.is_cancelled() {
            Err("run is closed or invalid; use a new run_id".into())
        } else if req.operation == "reset" && !state.ledger.is_empty() {
            Err("run_id cannot be reset twice; use a new run_id".into())
        } else {
            tokio::select! { biased;
                _ = run.cancel.cancelled() => Err("run cancelled; outcome unknown".into()),
                out = tokio::time::timeout(self.timeout,self.operate(&run,&mut state,&req)) => match out {Ok(v)=>v,Err(_)=>Err("request timed out; outcome unknown; run invalid".into())}
            }
        };
        if !state.closed
            && let Ok(costs) = run.brain.cost_summary().await
        {
            state.brain_costs = json!(costs);
        }
        state.costs = state.brain_costs.clone();
        if state.costs.is_null() {
            state.costs = json!({"receipts":[],"unreported_stages":["formation","maintenance","recall","business_model","tools","observer"],"accounting_complete":false});
        }
        let business = run.business_costs.lock().unwrap().clone();
        if !business.is_empty() {
            state.costs["receipts"]
                .as_array_mut()
                .unwrap()
                .extend(business.into_iter().map(|c| json!(c)));
            state.costs["unreported_stages"]
                .as_array_mut()
                .unwrap()
                .retain(|v| v != "business_model");
        }
        let response = match result {
            Ok(mut body) => {
                body["costs"] = state.costs.clone();
                body["cost_scope"] = "cumulative_run".into();
                body["extensions"] = json!({"anda_brain.costs.v1":{"summary":state.costs,"scope":"cumulative_run","request_elapsed_ms":started.elapsed().as_millis() as u64,"input_bytes":serde_json::to_vec(&req).unwrap().len(),"accounting_complete":false}});
                envelope(&req, body)
            }
            Err(e) => {
                state.invalid = true;
                run.cancel.cancel();
                let cleanup = Self::close_brain(&run).await.err().map(|e| e.to_string());
                let mut response = failure(&req, "execution_failure", e);
                response["extensions"] = json!({"anda_brain.costs.v1":{"summary":state.costs,"accounting_complete":false},"costs":state.costs,"cost_scope":"cumulative_run","cleanup_error":cleanup});
                response
            }
        };
        state.ledger_bytes += serde_json::to_vec(&response).unwrap().len();
        state.ledger.insert(
            req.request_id.clone(),
            Stored {
                digest: fp,
                response: response.clone(),
            },
        );
        run.touched.store(unix_ms(), Ordering::SeqCst);
        response
    }

    async fn operate(
        &self,
        run: &Run,
        state: &mut RunState,
        req: &Request,
    ) -> Result<Value, BoxError> {
        if req.operation == "learning_audit" {
            if !req.body.as_object().is_some_and(|v| v.is_empty()) {
                return Err("learning_audit accepts only an empty body".into());
            }
            let mut audit = serde_json::to_value(run.brain.audit_procedures().await?)?;
            audit["run_id"] = req.run_id.clone().into();
            audit["mode"] = if self.mode == Mode::NoMemory {
                "no_memory"
            } else {
                "persistent"
            }
            .into();
            return Ok(audit);
        }
        if let Some(t) = req.virtual_time.as_deref() {
            let now = timestamp(Some(t))?;
            run.brain.advance_to(now).await?;
            state.business_time = now;
        }
        match req.operation.as_str() {
            "reset" => Ok(json!({"accepted":true})),
            "observe" => {
                let obs: Observation = serde_json::from_value(
                    req.body
                        .get("observation")
                        .ok_or("missing observation")?
                        .clone(),
                )?;
                if obs.observation_id.is_empty() {
                    return Err("empty observation id".into());
                }
                if obs.virtual_time.is_some()
                    && req.virtual_time.is_some()
                    && timestamp(obs.virtual_time.as_deref())?
                        != timestamp(req.virtual_time.as_deref())?
                {
                    return Err("observation time differs from envelope".into());
                }
                if let Some(t) = obs.virtual_time.as_deref() {
                    let now = timestamp(Some(t))?;
                    run.brain.advance_to(now).await?;
                    state.business_time = now;
                }
                let fp = digest(&obs);
                if let Some(old) = state.seen_observations.get(&obs.observation_id) {
                    if old != &fp {
                        return Err("observation_id reused with different content".into());
                    }
                    return Ok(json!({"accepted":true,"emissions":[]}));
                }
                if obs.tool_call_id.is_some() && state.active_task.is_some() {
                    let expected = state
                        .pending_tool
                        .as_ref()
                        .ok_or("unexpected tool result")?;
                    if obs.tool_call_id.as_ref() != Some(&expected.0)
                        || obs.tool.as_ref() != Some(&expected.1)
                    {
                        return Err("tool result does not match pending call".into());
                    }
                    state.transient.push(json!({"tool_result":obs}));
                    state.pending_tool = None;
                    run.brain
                        .record_external_cost(StageCost {
                            stage: CostStage::Tools,
                            conversation: None,
                            failed: obs
                                .payload
                                .as_ref()
                                .and_then(|p| p.get("error"))
                                .is_some_and(|v| !v.is_null()),
                            requests: Some(1),
                            input_tokens: None,
                            output_tokens: None,
                            elapsed_ms: None,
                            accounting_complete: false,
                        })
                        .await?;
                } else if self.mode == Mode::Persistent {
                    self.form(run, &obs).await?;
                }
                Self::check_transient(state)?;
                state.seen_observations.insert(obs.observation_id, fp);
                Ok(json!({"accepted":true,"emissions":[]}))
            }
            "maintain" => {
                self.finish_task(run, state).await?;
                let report = run
                    .brain
                    .maintain(
                        MaintenanceInput {
                            scope: MaintenanceScope::Quick,
                            timestamp: req.virtual_time.clone(),
                            ..Default::default()
                        },
                        self.timeout,
                    )
                    .await?;
                if report.timed_out || report.report.state != ProcessingState::Completed {
                    return Err(format!(
                        "maintenance did not complete: {}",
                        serde_json::to_string(&report)?
                    )
                    .into());
                }
                Ok(json!({"accepted":true,"processing":report}))
            }
            "session_boundary" => {
                self.finish_task(run, state).await?;
                if self.mode == Mode::Persistent {
                    run.brain.session_boundary().await?;
                }
                Ok(json!({"accepted":true,"transient_cleared":true}))
            }
            "retrieve" if req.protocol == MEMORY => {
                let query = required(&req.body, "query")?;
                let limit = match req.body.get("limit_chars") {
                    None | Some(Value::Null) => 32_000,
                    Some(v) => v
                        .as_u64()
                        .filter(|n| *n > 0 && *n <= 1_000_000)
                        .ok_or("invalid limit_chars")? as usize,
                };
                let text = self.recall(run, query).await?;
                let truncated = text.chars().count() > limit;
                // A bounded packet is one semantic unit. A second character
                // transport cap must not cut off its warnings or corrupt JSON.
                let text = if self.recall_budget.is_some() && truncated {
                    String::new()
                } else {
                    text.chars().take(limit).collect::<String>()
                };
                Ok(
                    json!({"items":if text.is_empty(){vec![]}else{vec![json!({"id":format!("recall:{}",req.request_id),"content":text})]},"truncated":truncated,"accounting_complete":false,"costs":run.brain.cost_summary().await?}),
                )
            }
            "respond" if req.protocol == AGENT => {
                self.finish_task(run, state).await?;
                let input = req
                    .body
                    .get("input")
                    .filter(|v| v.is_object())
                    .ok_or("invalid input")?;
                let memory = self.recall(run, &input.to_string()).await?;
                let out = self
                    .business(run, state.business_time, input, &memory, &[], vec![])
                    .await?;
                if !out.tool_calls.is_empty() {
                    return Err("respond requested an unavailable tool".into());
                }
                let value = parse_output(&out.content, &["message", "structured", "abstention"])?;
                Ok(json!({"interaction_id":required(&req.body,"interaction_id")?,"output":value}))
            }
            "act" if req.protocol == AGENT => {
                let continuation = req
                    .body
                    .get("continuation")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let task = required(&req.body, "task_id")?;
                if !continuation {
                    self.finish_task(run, state).await?;
                    state.active_task = Some(req.body.clone());
                } else if state
                    .active_task
                    .as_ref()
                    .and_then(|v| v.get("task_id"))
                    .and_then(Value::as_str)
                    != Some(task)
                {
                    return Err("continuation has no matching task".into());
                }
                if state.pending_tool.is_some() {
                    return Err("pending runner tool result has not arrived".into());
                }
                let mut active = state.active_task.as_ref().ok_or("missing task")?.clone();
                active["continuation"] = continuation.into();
                let mut tools = Vec::new();
                let mut runner_names = BTreeMap::new();
                for t in active
                    .get("tools")
                    .and_then(Value::as_array)
                    .ok_or("missing tools")?
                {
                    let runner_name = required(t, "name")?;
                    if runner_names.values().any(|name| name == runner_name) {
                        return Err("duplicate task tool name".into());
                    }
                    // Runner names are qualified (e.g. workflow.inspect), but
                    // provider-native function names cannot contain a period.
                    // Index aliases are bounded, collision-free and stable for
                    // every continuation of this frozen task tool list.
                    let function_name = format!("mib_tool_{}", tools.len());
                    runner_names.insert(function_name.clone(), runner_name.to_string());
                    tools.push(FunctionDefinition {
                        name: function_name,
                        description: format!(
                            "Runner tool {runner_name}. {}",
                            t.get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        ),
                        parameters: t.get("input_schema").ok_or("missing input_schema")?.clone(),
                        strict: None,
                    });
                }
                if tools.len() > 128 {
                    return Err("too many task tools".into());
                }
                let memory = self
                    .recall(
                        run,
                        &json!({"goal":active.get("goal"),"constraints":active.get("constraints")})
                            .to_string(),
                    )
                    .await?;
                let out = self
                    .business(
                        run,
                        state.business_time,
                        &active,
                        &memory,
                        &state.transient,
                        tools.clone(),
                    )
                    .await?;
                let result = if out.tool_calls.len() == 1 {
                    let call = &out.tool_calls[0];
                    let runner_name = runner_names
                        .get(&call.name)
                        .ok_or("model requested unregistered tool")?;
                    let id = format!(
                        "bot_{}",
                        &digest(&(req.run_id.clone(), req.request_id.clone()))[7..31]
                    );
                    state.pending_tool = Some((id.clone(), runner_name.clone()));
                    json!({"type":"tool_call","tool_call_id":id,"tool":runner_name,"arguments":call.args})
                } else if !out.tool_calls.is_empty() {
                    return Err("model requested multiple tools in one step".into());
                } else {
                    parse_output(&out.content, &["final", "abstention"])?
                };
                state.transient.push(json!({"action":result}));
                Self::check_transient(state)?;
                if result["type"] != "tool_call" {
                    self.finish_task(run, state).await?;
                }
                Ok(json!({"result":result}))
            }
            _ => Err("unsupported operation for this protocol".into()),
        }
    }
    fn check_transient(state: &RunState) -> Result<(), BoxError> {
        if serde_json::to_vec(&state.transient)?.len() > MAX_TRANSIENT_BYTES {
            return Err("current task context capacity exceeded".into());
        }
        Ok(())
    }
    async fn form(&self, run: &Run, obs: &Observation) -> Result<(), BoxError> {
        let context = InputContext {
            counterparty: obs
                .actor
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string),
            agent: Some("anda_bot".into()),
            source: Some("mib_public_observation".into()),
            topic: None,
        };
        let report = run
            .brain
            .observe(
                FormationInput {
                    messages: vec![Message {
                        role: observation_role(&obs.kind).into(),
                        content: vec![serde_json::to_string(obs)?.into()],
                        ..Default::default()
                    }],
                    context: Some(context),
                    timestamp: obs.virtual_time.clone(),
                },
                self.timeout,
            )
            .await?;
        if report.timed_out || report.report.state != ProcessingState::Completed {
            return Err(format!(
                "formation did not complete: {}",
                serde_json::to_string(&report)?
            )
            .into());
        }
        Ok(())
    }
    async fn finish_task(&self, run: &Run, state: &mut RunState) -> Result<(), BoxError> {
        if state.pending_tool.is_some() {
            return Err("cannot cross task boundary with an unresolved tool call".into());
        }
        if self.mode == Mode::Persistent
            && state.active_task.is_some()
            && !state.transient.is_empty()
        {
            let obs = Observation {
                observation_id: format!("task:{}", digest(&state.active_task)),
                kind: "conversation".into(),
                virtual_time: None,
                actor: None,
                content: None,
                payload: Some(json!({"task":state.active_task,"public_steps":state.transient})),
                tool_call_id: None,
                tool: None,
            };
            self.form(run, &obs).await?;
        }
        state.transient.clear();
        state.active_task = None;
        state.pending_tool = None;
        if self.mode == Mode::NoMemory {
            run.brain.session_boundary().await?;
        }
        Ok(())
    }
    async fn recall(&self, run: &Run, query: &str) -> Result<String, BoxError> {
        if self.mode == Mode::NoMemory {
            return Ok(String::new());
        }
        let out = run
            .brain
            .recall(RecallInput {
                budget: None,
                query: query.into(),
                context: Some(InputContext {
                    agent: Some("anda_bot".into()),
                    ..Default::default()
                }),
            })
            .await?;
        if let Some(reason) = out.failed_reason {
            return Err(reason.into());
        }
        Ok(out.content)
    }
    async fn business(
        &self,
        run: &Run,
        now_ms: u64,
        input: &Value,
        memory: &str,
        transient: &[Value],
        tools: Vec<FunctionDefinition>,
    ) -> Result<AgentOutput, BoxError> {
        let model = self
            .models
            .get_model()
            .ok_or("business model unavailable")?;
        let now = time_string(now_ms)?;
        let mut input = input.clone();
        // Run/request/interaction/task identifiers are host correlation, never a
        // source of model hints. Preserve public goal/constraints/tool schemas.
        if let Some(v) = input.as_object_mut() {
            v.remove("task_id");
            v.remove("interaction_id");
        }
        let request = AndaBot::runner_managed_request(
            &input,
            memory,
            transient,
            tools,
            &now,
            self.max_output,
        );
        let mut receipt = BusinessReceipt {
            costs: &run.business_costs,
            start: Instant::now(),
            completed: false,
        };
        let out = model.completion(request).await;
        let usage = out.as_ref().ok().map(|o| &o.usage);
        receipt.finish(StageCost {
            stage: CostStage::BusinessModel,
            conversation: None,
            failed: out.as_ref().map_or(true, |o| o.failed_reason.is_some()),
            requests: Some(1),
            input_tokens: usage
                .filter(|u| u.requests > 0 || u.input_tokens > 0)
                .map(|u| u.input_tokens),
            output_tokens: usage
                .filter(|u| u.requests > 0 || u.output_tokens > 0)
                .map(|u| u.output_tokens),
            elapsed_ms: Some(receipt.start.elapsed().as_millis() as u64),
            accounting_complete: false,
        });
        let out = out?;
        if let Some(reason) = out.failed_reason.as_ref() {
            return Err(reason.clone().into());
        }
        Ok(out)
    }
    async fn close_brain(run: &Run) -> Result<(), BoxError> {
        run.cancel.cancel();
        run.brain.close().await?;
        run.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn close_all(&self) {
        self.shutdown.cancel();
        let runs = self.runs.lock().await.values().cloned().collect::<Vec<_>>();
        for run in runs {
            run.cancel.cancel();
            if let Err(e) = Self::close_brain(&run).await {
                eprintln!("MIB cleanup failed: {e}");
            }
        }
        self.tasks.close();
        self.tasks.wait().await;
    }
    async fn reap(&self) {
        let cutoff = unix_ms().saturating_sub(self.idle.as_millis() as u64);
        let runs = self
            .runs
            .lock()
            .await
            .values()
            .filter(|r| {
                !r.closed.load(Ordering::SeqCst) && r.touched.load(Ordering::SeqCst) < cutoff
            })
            .cloned()
            .collect::<Vec<_>>();
        for run in runs {
            run.cancel.cancel();
            if let Err(e) = Self::close_brain(&run).await {
                eprintln!("MIB idle cleanup failed: {e}");
            }
            let mut state = run.state.lock().await;
            state.closed = true;
            state.transient.clear();
            state.active_task = None;
            state.pending_tool = None;
        }
    }
}

// This guard also records hard cancellation/timeout: dropping the model future
// cannot erase the fact that a billable request was dispatched. Provider-internal
// retries and missing token counts remain explicitly unknown.
struct BusinessReceipt<'a> {
    costs: &'a std::sync::Mutex<Vec<StageCost>>,
    start: Instant,
    completed: bool,
}
impl BusinessReceipt<'_> {
    fn finish(&mut self, cost: StageCost) {
        self.costs.lock().unwrap().push(cost);
        self.completed = true;
    }
}
impl Drop for BusinessReceipt<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.costs.lock().unwrap().push(StageCost {
                stage: CostStage::BusinessModel,
                conversation: None,
                failed: true,
                requests: Some(1),
                input_tokens: None,
                output_tokens: None,
                elapsed_ms: Some(self.start.elapsed().as_millis() as u64),
                accounting_complete: false,
            });
        }
    }
}

fn parse_output(text: &str, allowed: &[&str]) -> Result<Value, BoxError> {
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .and_then(|t| t.strip_suffix("```"))
        .unwrap_or(text)
        .trim();
    let value: Value = serde_json::from_str(text)?;
    if !value.is_object()
        || !value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| allowed.contains(&t))
    {
        return Err("model output does not match response contract".into());
    }
    if value
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !["type", "content", "value", "attribution"].contains(&k.as_str()))
    {
        return Err("unexpected model output field".into());
    }
    if value
        .get("content")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        return Err("invalid response content".into());
    }
    match value["type"].as_str().unwrap() {
        "message" if !value.get("content").is_some_and(Value::is_string) => {
            return Err("message output requires string content".into());
        }
        "structured" if value.get("value").is_none() => {
            return Err("structured output requires value".into());
        }
        _ => {}
    }
    Ok(value)
}

async fn post_agent(
    State(host): State<Arc<Host>>,
    Path(op): Path<String>,
    Json(value): Json<Value>,
) -> Json<Value> {
    post(host, op, value, AGENT).await
}
async fn post_memory(
    State(host): State<Arc<Host>>,
    Path(op): Path<String>,
    Json(value): Json<Value>,
) -> Json<Value> {
    post(host, op, value, MEMORY).await
}
async fn post(host: Arc<Host>, op: String, value: Value, protocol: &str) -> Json<Value> {
    let req: Request = match serde_json::from_value(value.clone()) {
        Ok(req) => req,
        Err(e) => {
            return Json(
                json!({"mib":"0.1","protocol":protocol,"run_id":value.get("run_id"),"request_id":value.get("request_id"),"status":"error","error":{"code":"invalid_request","message":e.to_string(),"retryable":false}}),
            );
        }
    };
    if req.operation != op || req.protocol != protocol {
        return Json(failure(
            &req,
            "invalid_request",
            "route and envelope differ",
        ));
    }
    let fallback = failure(
        &req,
        "execution_failure",
        "host task terminated; outcome unknown",
    );
    let tracker = host.tasks.clone();
    Json(tracker.spawn(host.dispatch(req)).await.unwrap_or(fallback))
}
async fn get_agent(State(host): State<Arc<Host>>, Path(op): Path<String>) -> Json<Value> {
    get_descriptor(host, &op, AGENT)
}
async fn get_memory(State(host): State<Arc<Host>>, Path(op): Path<String>) -> Json<Value> {
    get_descriptor(host, &op, MEMORY)
}
fn get_descriptor(host: Arc<Host>, op: &str, protocol: &str) -> Json<Value> {
    let req = Request {
        mib: "0.1".into(),
        protocol: protocol.into(),
        run_id: "descriptor".into(),
        request_id: "describe".into(),
        operation: op.into(),
        virtual_time: None,
        body: json!({}),
    };
    Json(if op == "describe" {
        envelope(&req, host.descriptor(protocol))
    } else {
        failure(
            &req,
            "invalid_request",
            "GET is only supported for describe",
        )
    })
}
fn router(host: Arc<Host>) -> Router {
    Router::new()
        .route(
            "/mib-agent/v0.1/{operation}",
            get(get_agent).post(post_agent),
        )
        .route(
            "/mib-memory/v0.1/{operation}",
            get(get_memory).post(post_memory),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(host)
}

pub async fn serve(cmd: &MibCommand) -> Result<(), BoxError> {
    if !cmd.listen.ip().is_loopback() {
        return Err("MIB host must bind a loopback address".into());
    }
    if !(1..=3600).contains(&cmd.timeout_seconds)
        || cmd.idle_seconds <= cmd.timeout_seconds
        || !(1..=65536).contains(&cmd.max_output_tokens)
    {
        return Err("invalid MIB timeout, idle timeout or output budget".into());
    }
    let recall_budget = cmd.recall_budget()?;
    let mut cfg: ModelConfig = serde_json::from_slice(&tokio::fs::read(&cmd.model_config).await?)?;
    if cfg.api_key.is_empty() {
        cfg.api_key = std::env::var(&cmd.api_key_env).unwrap_or_default();
    }
    if cfg.model.is_empty() || cfg.disabled {
        return Err("a model must be configured and enabled".into());
    }
    let mut public = serde_json::to_value(&cfg)?;
    public.as_object_mut().unwrap().remove("api_key");
    let http = crate::util::http_client::new_reqwest_client();
    let models = Models::from_configs(std::slice::from_ref(&cfg), http.clone());
    models.set_model(
        models
            .get(&cfg.model)
            .or_else(|| cfg.labels.iter().find_map(|label| models.get(label)))
            .ok_or("model configuration unavailable")?,
    );
    let models = Arc::new(models);
    let app = AppState::new(
        Arc::new(InMemory::new()),
        Arc::new(DBConfig {
            name: "mib".into(),
            description: "isolated MIB template".into(),
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
        crate::config::APP_VERSION.into(),
        0,
    );
    let host = Arc::new(Host {
        app,
        models,
        identity: ExperimentIdentity {
            model_digest: digest(&public),
            tools_digest: digest(&"MIB runner schemas per task; no local effects"),
            budget_digest: digest(
                &json!({"timeout":cmd.timeout_seconds,"output_tokens":cmd.max_output_tokens,"temperature":0.0,"recall_budget":recall_budget}),
            ),
        },
        mode: cmd.memory_mode,
        timeout: Duration::from_secs(cmd.timeout_seconds),
        idle: Duration::from_secs(cmd.idle_seconds),
        max_output: cmd.max_output_tokens,
        recall_budget,
        runs: Mutex::new(HashMap::new()),
        requests: Mutex::new(HashMap::new()),
        tasks: TaskTracker::new(),
        shutdown: CancellationToken::new(),
        epoch: format!("{}-{}", std::process::id(), unix_ms()),
    });
    let listener = tokio::net::TcpListener::bind(cmd.listen).await?;
    eprintln!("MIB host listening on {}", listener.local_addr()?);
    let reaper_host = host.clone();
    let reaper = tokio::spawn(async move {
        loop {
            tokio::select! {_ = reaper_host.shutdown.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(10))=>reaper_host.reap().await}
        }
    });
    let shutdown_host = host.clone();
    let result = axum::serve(listener, router(host.clone()))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_host.close_all().await;
        })
        .await;
    host.close_all().await;
    let _ = reaper.await;
    result.map_err(Into::into)
}

#[cfg(test)]
mod tests;
