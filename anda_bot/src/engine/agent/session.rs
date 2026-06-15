//! Active session state shared between the agent entry points, the session
//! runner, and background task hooks.

use anda_brain::types::InputContext;
use anda_core::{
    AgentOutput, CompletionRequest, RequestMeta, Resource, StateFeatures, ToolOutput, Usage,
};
use anda_engine::{
    context::{AgentCtx, BaseCtx},
    extension::shell::{ExecArgs, ExecOutput, ShellTool},
    hook::{AgentHook, ToolHook},
};
use async_trait::async_trait;
use futures::future::join_all;
use ic_auth_types::Xid;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::engine::{
    CompletionHook,
    goal::{self, GoalStateSnapshot},
    prompt::PromptCommand,
    system::system_runtime_prompt,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub caller: String,
    pub workspace: String,
    pub source: String,
    pub conversation_id: u64,
    pub active_at: u64,
    pub idle_ms: u64,
    pub has_goal: bool,
    pub background_task_count: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionFormationContext {
    pub counterparty: Option<String>,
    pub agent: Option<String>,
    pub source: Option<String>,
    pub topic: Option<String>,
}

impl From<&InputContext> for SessionFormationContext {
    fn from(context: &InputContext) -> Self {
        Self {
            counterparty: context.counterparty.clone(),
            agent: context.agent.clone(),
            source: context.source.clone(),
            topic: context.topic.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionState {
    pub summary: SessionSummary,
    pub formation_context: Option<SessionFormationContext>,
    pub goal: Option<GoalStateSnapshot>,
    pub background_tasks: HashMap<String, BackgroundTaskInfo>,
    pub submit_formation_at: u64,
}

pub(super) struct Session {
    pub(super) id: Xid,
    pub(super) caller: String,
    pub(super) workspace: String,
    pub(super) source_key: String,
    pub(super) conversation_id: AtomicU64,
    pub(super) sender: tokio::sync::mpsc::Sender<ConversationInput>,
    // task_id -> BackgroundTaskInfo
    pub(super) background_tasks: Arc<RwLock<HashMap<String, BackgroundTaskInfo>>>,
    pub(super) background_progress_outputs: Arc<RwLock<HashMap<String, String>>>,
    pub(super) goal: Arc<RwLock<Option<goal::GoalState>>>,
    pub(super) request_meta: SessionRequestMeta,
    pub(super) completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    pub(super) submit_formation_at: AtomicU64,
    // Unix ms before which formation submissions are skipped (set on failure).
    pub(super) formation_backoff_until: AtomicU64,
    // Unix ms before which goal supervisor checks are skipped (set on failure).
    pub(super) goal_check_backoff_until: AtomicU64,
    pub(super) active_at: Arc<AtomicU64>,
    pub(super) finish_when_idle: AtomicBool,
    // Mirrors CompletionRunner::is_idle of this session's runner; the runner
    // is owned by the session task, so the idle monitor reads this flag.
    pub(super) runner_idle: AtomicBool,
    pub(super) formation_context: Option<InputContext>,
}

#[derive(Clone)]
pub struct SessionRequestMeta {
    meta: Arc<RwLock<RequestMeta>>,
}

impl SessionRequestMeta {
    pub fn new(meta: RequestMeta) -> Self {
        Self {
            meta: Arc::new(RwLock::new(meta)),
        }
    }

    pub fn get(&self) -> RequestMeta {
        self.meta.read().clone()
    }

    pub(super) fn set(&self, meta: RequestMeta) {
        *self.meta.write() = meta;
    }
}

fn subagent_background_output_key(session_id: &str) -> String {
    format!("subagent:{session_id}")
}

fn shell_background_output_key(task_id: &str) -> String {
    format!("shell:{task_id}")
}

fn subagent_final_output_prompt(
    session_id: &str,
    output: &AgentOutput,
    last_progress_content: Option<&str>,
) -> String {
    let message = if !output.content.is_empty() {
        if last_progress_content == Some(output.content.as_str()) {
            format!(
                "Subagent session {session_id} completed; final output is unchanged from the latest intermediate output."
            )
        } else {
            format!(
                "Subagent session {session_id} final output:\n\n{}",
                output.content
            )
        }
    } else if let Some(failed_reason) = output.failed_reason.as_ref() {
        format!(
            "Subagent session {session_id} failed with reason: {:?}",
            failed_reason
        )
    } else {
        format!("Subagent session {session_id} completed")
    };

    system_runtime_prompt("subagent final output", message)
}

fn background_shell_output_json(output: &ExecOutput) -> String {
    serde_json::to_string(output).unwrap_or_default()
}

fn background_shell_end_prompt(
    task_id: &str,
    output_json: &str,
    last_progress_output: Option<&str>,
) -> String {
    let message = if last_progress_output == Some(output_json) {
        format!(
            "Background task {task_id} completed; final output is unchanged from the latest intermediate output."
        )
    } else {
        format!("Background task {task_id} completed:\n\n{output_json}")
    };

    system_runtime_prompt("background shell", message)
}

impl Session {
    // A live session is idle when its completion runner has no pending work
    // and no background tasks are running.
    pub(super) fn is_idle(&self) -> bool {
        self.runner_idle.load(Ordering::SeqCst) && !self.has_running_background_tasks()
    }

    pub(super) fn has_running_background_tasks(&self) -> bool {
        self.background_tasks
            .read()
            .values()
            .any(|task| !task.stopped)
    }

    fn running_background_task_count(&self) -> usize {
        self.background_tasks
            .read()
            .values()
            .filter(|task| !task.stopped)
            .count()
    }

    fn is_background_task_stopped(&self, task_id: &str) -> bool {
        self.background_tasks
            .read()
            .get(task_id)
            .is_some_and(|task| task.stopped)
    }

    pub(super) fn stop_background_tasks(&self) {
        let mut progress_outputs = self.background_progress_outputs.write();
        for (task_id, task) in self.background_tasks.write().iter_mut() {
            task.stopped = true;
            progress_outputs.remove(&subagent_background_output_key(task_id));
            progress_outputs.remove(&shell_background_output_key(task_id));
        }
    }

    pub(super) fn summary(&self, now_ms: u64) -> SessionSummary {
        let active_at = self.active_at.load(Ordering::SeqCst);
        SessionSummary {
            id: self.id.to_string(),
            caller: self.caller.clone(),
            workspace: self.workspace.clone(),
            source: self.source_key.clone(),
            conversation_id: self.conversation_id.load(Ordering::SeqCst),
            active_at,
            idle_ms: now_ms.saturating_sub(active_at),
            has_goal: self.goal.read().is_some(),
            background_task_count: self.running_background_task_count() as u64,
        }
    }

    pub(super) fn state(&self, now_ms: u64) -> SessionState {
        SessionState {
            summary: self.summary(now_ms),
            formation_context: self
                .formation_context
                .as_ref()
                .map(SessionFormationContext::from),
            goal: self.goal.read().as_ref().map(|goal| goal.snapshot()),
            background_tasks: self.background_tasks.read().clone(),
            submit_formation_at: self.submit_formation_at.load(Ordering::SeqCst),
        }
    }
}

#[async_trait]
impl CompletionHook for Session {
    async fn on_completion(&self, _ctx: &AgentCtx, _output: &AgentOutput) {
        join_all(
            self.completion_hooks
                .iter()
                .map(|hook| hook.on_completion(_ctx, _output)),
        )
        .await;
    }
}

#[async_trait]
impl AgentHook for Session {
    async fn on_background_start(
        &self,
        ctx: &AgentCtx,
        session_id: &str,
        _req: &CompletionRequest,
    ) {
        self.background_tasks.write().insert(
            session_id.to_string(),
            BackgroundTaskInfo {
                agent_name: ctx.base.agent.clone(),
                tool_name: None,
                progress_message: None,
                stopped: false,
            },
        );
        self.background_progress_outputs
            .write()
            .remove(&subagent_background_output_key(session_id));
    }

    async fn on_background_progress(
        &self,
        ctx: &AgentCtx,
        session_id: String,
        output: AgentOutput,
    ) {
        if self.is_background_task_stopped(&session_id) {
            return;
        }

        let prompt = if !output.content.is_empty() {
            self.background_progress_outputs.write().insert(
                subagent_background_output_key(&session_id),
                output.content.clone(),
            );
            system_runtime_prompt(
                "subagent progress",
                format!(
                    "Subagent session {session_id} intermediate output:\n\n{}",
                    output.content
                ),
            )
        } else if let Some(failed_reason) = output.failed_reason {
            system_runtime_prompt(
                "subagent progress",
                format!(
                    "Subagent session {session_id} failed with reason: {:?}",
                    failed_reason
                ),
            )
        } else {
            return;
        };
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain { prompt },
                resources: output.artifacts,
                extra: ctx.meta().extra.clone(),
                usage: output.usage,
            })
            .await
            .ok();
    }

    async fn on_background_end(&self, ctx: &AgentCtx, session_id: String, output: AgentOutput) {
        let stopped = self
            .background_tasks
            .write()
            .remove(&session_id)
            .is_some_and(|task| task.stopped);
        let last_progress_content = self
            .background_progress_outputs
            .write()
            .remove(&subagent_background_output_key(&session_id));
        if stopped {
            return;
        }

        let prompt =
            subagent_final_output_prompt(&session_id, &output, last_progress_content.as_deref());
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain { prompt },
                resources: output.artifacts,
                extra: ctx.meta().extra.clone(),
                usage: output.usage,
            })
            .await
            .ok();
    }
}

#[async_trait]
impl ToolHook<ExecArgs, ExecOutput> for Session {
    async fn on_background_start(&self, ctx: &BaseCtx, task_id: &str, _args: &ExecArgs) {
        self.background_tasks.write().insert(
            task_id.to_string(),
            BackgroundTaskInfo {
                agent_name: ctx.agent.clone(),
                tool_name: Some(ShellTool::NAME.to_string()),
                progress_message: None,
                stopped: false,
            },
        );
        self.background_progress_outputs
            .write()
            .remove(&shell_background_output_key(task_id));
    }

    async fn on_background_progress(
        &self,
        ctx: &BaseCtx,
        task_id: String,
        output: ToolOutput<ExecOutput>,
    ) {
        if self.is_background_task_stopped(&task_id) {
            return;
        }

        let output_json = background_shell_output_json(&output.output);
        self.background_progress_outputs
            .write()
            .insert(shell_background_output_key(&task_id), output_json.clone());
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain {
                    prompt: system_runtime_prompt(
                        "background shell",
                        format!(
                            "Background task {task_id} intermediate output:\n\n{}",
                            output_json
                        ),
                    ),
                },
                usage: output.usage,
                extra: ctx.meta().extra.clone(),
                resources: output.artifacts,
            })
            .await
            .ok();
    }

    async fn on_background_end(
        &self,
        ctx: &BaseCtx,
        task_id: String,
        output: ToolOutput<ExecOutput>,
    ) {
        let stopped = self
            .background_tasks
            .write()
            .remove(&task_id)
            .is_some_and(|task| task.stopped);
        let output_json = background_shell_output_json(&output.output);
        let last_progress_output = self
            .background_progress_outputs
            .write()
            .remove(&shell_background_output_key(&task_id));
        if stopped {
            return;
        }

        let prompt =
            background_shell_end_prompt(&task_id, &output_json, last_progress_output.as_deref());
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain { prompt },
                usage: output.usage,
                extra: ctx.meta().extra.clone(),
                resources: output.artifacts,
            })
            .await
            .ok();
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct BackgroundTaskInfo {
    pub agent_name: String,
    pub tool_name: Option<String>,
    pub progress_message: Option<String>,
    #[serde(default)]
    pub stopped: bool,
}

#[derive(Default, Clone)]
pub(super) struct ConversationInput {
    pub(super) command: PromptCommand,
    pub(super) resources: Vec<Resource>,
    pub(super) extra: Map<String, Value>,
    pub(super) usage: Usage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_is_idle_requires_idle_runner_and_no_background_tasks() {
        let (sender, _rx) = tokio::sync::mpsc::channel(1);
        let session = Session {
            id: Xid::new(),
            caller: "caller".to_string(),
            workspace: "/tmp".to_string(),
            source_key: "test".to_string(),
            conversation_id: AtomicU64::new(1),
            sender,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            background_progress_outputs: Arc::new(RwLock::new(HashMap::new())),
            goal: Arc::new(RwLock::new(None)),
            request_meta: SessionRequestMeta::new(RequestMeta::default()),
            completion_hooks: Arc::new(Vec::new()),
            submit_formation_at: AtomicU64::new(0),
            formation_backoff_until: AtomicU64::new(0),
            goal_check_backoff_until: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(0)),
            finish_when_idle: AtomicBool::new(false),
            runner_idle: AtomicBool::new(false),
            formation_context: None,
        };

        // A fresh session is busy: its runner is about to process a prompt.
        assert!(!session.is_idle());

        session.runner_idle.store(true, Ordering::SeqCst);
        assert!(session.is_idle());

        session
            .background_tasks
            .write()
            .insert("task-1".to_string(), BackgroundTaskInfo::default());
        assert!(!session.is_idle());

        session.background_progress_outputs.write().insert(
            shell_background_output_key("task-1"),
            "old output".to_string(),
        );
        session.stop_background_tasks();

        assert!(session.is_idle());
        assert_eq!(session.summary(0).background_task_count, 0);
        assert!(
            !session
                .background_progress_outputs
                .read()
                .contains_key(&shell_background_output_key("task-1"))
        );
    }

    #[test]
    fn subagent_final_output_prompt_omits_duplicate_progress_content() {
        let output = AgentOutput {
            content: "same final body".to_string(),
            ..Default::default()
        };

        let prompt = subagent_final_output_prompt("session-1", &output, Some("same final body"));

        assert!(prompt.contains("Subagent session session-1 completed"));
        assert!(prompt.contains("unchanged from the latest intermediate output"));
        assert!(!prompt.contains("same final body"));
    }

    #[test]
    fn subagent_final_output_prompt_keeps_new_final_content() {
        let output = AgentOutput {
            content: "new final body".to_string(),
            ..Default::default()
        };

        let prompt = subagent_final_output_prompt("session-1", &output, Some("old body"));

        assert!(prompt.contains("Subagent session session-1 final output"));
        assert!(prompt.contains("new final body"));
    }

    #[test]
    fn background_shell_end_prompt_omits_duplicate_progress_output() {
        let output_json = r#"{"stdout":"same final body"}"#;

        let prompt = background_shell_end_prompt("task-1", output_json, Some(output_json));

        assert!(prompt.contains("Background task task-1 completed"));
        assert!(prompt.contains("unchanged from the latest intermediate output"));
        assert!(!prompt.contains("same final body"));
    }

    #[test]
    fn background_shell_end_prompt_keeps_new_final_output() {
        let output_json = r#"{"stdout":"new final body"}"#;

        let prompt = background_shell_end_prompt("task-1", output_json, Some(r#"{"stdout":""}"#));

        assert!(prompt.contains("Background task task-1 completed"));
        assert!(prompt.contains("new final body"));
    }
}
