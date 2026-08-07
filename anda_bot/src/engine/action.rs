use anda_core::{
    BoxError, FunctionDefinition, Message, RequestMeta, Resource, StateFeatures, Tool, ToolOutput,
    Usage,
};
use anda_engine::{
    context::BaseCtx,
    extension::shell::{ExecArgs, ShellTool},
    model::Models,
    unix_ms,
};
use ic_auth_types::Xid;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc, oneshot};

use super::{agent::SessionRequestMeta, goal::GoalToolState};
use crate::util::request_meta::keys;

mod shell_policy;

use shell_policy::{
    ApprovalDecision, ApprovalMode, launcher_ui_language_hint, shell_approval_decision_with_model,
    shell_risk_language_hint,
};

mod protocol;

pub(crate) use protocol::{
    ActionApiOutput, ActionDetail, ActionStatus, ApprovalLabels, TOOL_APPROVAL_ACTION,
    USER_CHOICE_ACTION, UserChoiceOption, action_id_from_message, action_id_from_message_value,
    action_message, apply_action_resolution_to_chat_message, apply_action_resolution_to_message,
    approval_detail, is_action_message, is_action_message_value, payload_action_id,
    payload_is_pending, payload_responded_at, update_action_payload_resolution,
};
use protocol::{ActionPayload, ActionToolRef};

const ACTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

impl ApprovalMode {
    fn from_ctx(ctx: &BaseCtx) -> Self {
        let meta = live_request_meta(ctx);
        // Unattended runs have nobody on the other end of an approval card:
        // it would sit pending until ACTION_RESPONSE_TIMEOUT and then fail the
        // whole task. Grant full access instead so scheduled and autonomous
        // work can complete.
        let declared = Self::from_meta(&meta);
        if let Some(reason) = unattended_run_reason(ctx, &meta) {
            if declared != Self::FullAccess {
                log::info!(
                    "Approval elevated from {} to full_access for an unattended run ({reason}); agent {}",
                    declared.as_str(),
                    ctx.agent
                );
            }
            return Self::FullAccess;
        }

        declared
    }
}

/// Metadata of the request currently being served.
///
/// A session's [`BaseCtx`] metadata is frozen when the session is created, but
/// later requests join the running session and only refresh
/// [`SessionRequestMeta`]. Approval decisions must follow the live request (a
/// cron job firing into an existing chat session, a CLI launched with
/// `--full-access`), so prefer it and fall back to the context metadata.
fn live_request_meta(ctx: &BaseCtx) -> RequestMeta {
    ctx.get_state::<SessionRequestMeta>()
        .map(|meta| meta.get())
        .unwrap_or_else(|| ctx.meta().clone())
}

/// Returns why the current run is unattended, or `None` when a human can answer.
fn unattended_run_reason(ctx: &BaseCtx, meta: &RequestMeta) -> Option<&'static str> {
    if meta.get_extra_as::<u64>(keys::CRON_JOB_ID).is_some() {
        return Some("cron job");
    }
    if ctx
        .get_state::<GoalToolState>()
        .is_some_and(|goal| goal.is_active())
    {
        return Some("goal mode");
    }
    None
}

#[derive(Clone, Debug)]
pub(crate) enum ActionEvent {
    Add(Message),
    Resolve {
        action_id: String,
        status: ActionStatus,
        response: Value,
        responded_at: u64,
    },
}

#[derive(Clone)]
pub(crate) struct ActionRuntime {
    pending: Arc<Mutex<HashMap<String, PendingAction>>>,
}

impl ActionRuntime {
    pub(crate) fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn register(&self, pending: PendingAction) -> oneshot::Receiver<ActionResponse> {
        let action_id = pending.action_id.clone();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(action_id, PendingAction { tx, ..pending });
        rx
    }

    async fn expire(&self, action_id: &str) -> Option<PendingAction> {
        self.pending.lock().await.remove(action_id)
    }

    pub(crate) async fn respond(
        &self,
        caller: &str,
        conversation: u64,
        args: ActionResponseArgs,
    ) -> Result<ActionApiOutput, BoxError> {
        let (pending, response) = {
            let mut pending_actions = self.pending.lock().await;
            let pending = pending_actions
                .get(&args.action_id)
                .ok_or_else(|| format!("action {} is not pending", args.action_id))?;
            if pending.caller != caller {
                return Err("permission denied".into());
            }
            if conversation > 0 && pending.conversation != conversation {
                return Err("action belongs to a different conversation".into());
            }
            let response = pending.kind.response_from_args(&args)?;
            pending_actions
                .remove(&args.action_id)
                .map(|pending| (pending, response))
                .expect("pending action exists")
        };

        let status = response.status;
        let responded_at = unix_ms();
        let event = ActionEvent::Resolve {
            action_id: pending.action_id.clone(),
            status,
            response: response.payload.clone(),
            responded_at,
        };
        let _ = pending.event_sender.send(event).await;
        let _ = pending.tx.send(response.clone());

        Ok(ActionApiOutput {
            action_id: pending.action_id,
            conversation: pending.conversation,
            status: status.as_str().to_string(),
            response: response.payload,
            responded_at,
        })
    }
}

#[derive(Clone)]
pub(crate) struct ActionSession {
    runtime: Arc<ActionRuntime>,
    event_sender: mpsc::Sender<ActionEvent>,
    caller: String,
    session_id: String,
    conversation_id: Arc<std::sync::atomic::AtomicU64>,
    models: Arc<Models>,
    home_dir: PathBuf,
}

impl ActionSession {
    pub(crate) fn new(
        runtime: Arc<ActionRuntime>,
        event_sender: mpsc::Sender<ActionEvent>,
        caller: String,
        session_id: String,
        conversation_id: Arc<std::sync::atomic::AtomicU64>,
        models: Arc<Models>,
        home_dir: PathBuf,
    ) -> Self {
        Self {
            runtime,
            event_sender,
            caller,
            session_id,
            conversation_id,
            models,
            home_dir,
        }
    }

    pub(crate) async fn request_shell_approval(
        &self,
        ctx: &BaseCtx,
        args: ExecArgs,
    ) -> Result<ExecArgs, BoxError> {
        let conversation = self
            .conversation_id
            .load(std::sync::atomic::Ordering::SeqCst);
        let workspace = ctx
            .meta()
            .get_extra_as::<String>(keys::WORKSPACE)
            .unwrap_or_default();
        let approval_mode = ApprovalMode::from_ctx(ctx);
        let language_hint = shell_risk_language_hint(ctx.meta())
            .or_else(|| launcher_ui_language_hint(&self.home_dir));
        let approval_reason = match shell_approval_decision_with_model(
            &args,
            approval_mode,
            &workspace,
            self.models.as_ref(),
            language_hint.as_deref(),
        )
        .await
        {
            ApprovalDecision::Allow => return Ok(args),
            ApprovalDecision::Ask(reason) => reason,
        };
        let approval_locale = language_hint.as_deref().unwrap_or("en");
        let mut details = Vec::new();
        if !workspace.is_empty() {
            details.push(approval_detail("Workspace", workspace, "text"));
        }
        let approval_reason_label = t!(
            "shell_approval.detail.approval_reason",
            locale = approval_locale
        )
        .into_owned();
        details.push(approval_detail(
            &approval_reason_label,
            &approval_reason,
            "text",
        ));
        details.push(approval_detail(
            "Mode",
            if args.background {
                "background"
            } else {
                "foreground"
            },
            "text",
        ));
        if !args.env_keys.is_empty() {
            details.push(approval_detail("Environment keys", &args.env_keys, "list"));
        }

        let now_ms = unix_ms();
        let action_id = next_action_id();
        let payload = ActionPayload {
            id: action_id.clone(),
            kind: "tool_approval".to_string(),
            tool: Some(ActionToolRef::labeled(ShellTool::NAME, "Shell command")),
            agent: ctx.agent.clone(),
            conversation,
            session: self.session_id.clone(),
            title: "Approve shell command".to_string(),
            message: Some("The agent wants to run a local shell command.".to_string()),
            summary: Some(args.command.clone()),
            command: Some(args.command.clone()),
            details: Some(details),
            approval: Some(ApprovalLabels::approve_deny()),
            metadata: Some(json!({
                "command": &args.command,
                "env_keys": &args.env_keys,
                "background": args.background,
                "approval_mode": approval_mode.as_str(),
                "approval_reason": &approval_reason,
            })),
            status: ActionStatus::Pending,
            created_at: now_ms,
            expires_at: now_ms + ACTION_RESPONSE_TIMEOUT.as_millis() as u64,
            ..Default::default()
        };
        let approved_payload = json!({
            "tool": ShellTool::NAME,
            "command": &args.command,
        });
        self.publish_approval_and_wait(
            action_id,
            conversation,
            approved_payload,
            payload.into_value(),
            "shell command",
        )
        .await
        .map(|()| args)
    }

    /// Requests user approval before an MCP server is added or connected.
    /// Unlike shell commands there is no risk classification: outside
    /// FullAccess mode these tools always require explicit confirmation,
    /// because they spawn local processes or open connections to arbitrary
    /// endpoints.
    pub(crate) async fn request_mcp_approval(
        &self,
        ctx: &BaseCtx,
        tool_name: &str,
        summary: String,
        details: Vec<ActionDetail>,
        metadata: Value,
    ) -> Result<(), BoxError> {
        if ApprovalMode::from_ctx(ctx) == ApprovalMode::FullAccess {
            return Ok(());
        }
        let conversation = self
            .conversation_id
            .load(std::sync::atomic::Ordering::SeqCst);
        let now_ms = unix_ms();
        let action_id = next_action_id();
        let payload = ActionPayload {
            id: action_id.clone(),
            kind: "tool_approval".to_string(),
            tool: Some(ActionToolRef::labeled(tool_name, "MCP server")),
            agent: ctx.agent.clone(),
            conversation,
            session: self.session_id.clone(),
            title: "Approve MCP server connection".to_string(),
            message: Some(
                "The agent wants to connect an MCP server, which can run a local program or reach a remote endpoint."
                    .to_string(),
            ),
            summary: Some(summary.clone()),
            details: Some(details),
            approval: Some(ApprovalLabels::approve_deny()),
            metadata: Some(metadata),
            status: ActionStatus::Pending,
            created_at: now_ms,
            expires_at: now_ms + ACTION_RESPONSE_TIMEOUT.as_millis() as u64,
            ..Default::default()
        };
        let approved_payload = json!({
            "tool": tool_name,
            "summary": summary,
        });
        self.publish_approval_and_wait(
            action_id,
            conversation,
            approved_payload,
            payload.into_value(),
            "MCP server",
        )
        .await
    }

    async fn publish_approval_and_wait(
        &self,
        action_id: String,
        conversation: u64,
        approved_payload: Value,
        payload: Value,
        what: &str,
    ) -> Result<(), BoxError> {
        let message = action_message(TOOL_APPROVAL_ACTION, payload);
        let rx = self
            .runtime
            .register(PendingAction {
                action_id: action_id.clone(),
                caller: self.caller.clone(),
                conversation,
                kind: PendingActionKind::Approval { approved_payload },
                event_sender: self.event_sender.clone(),
                tx: oneshot::channel().0,
            })
            .await;
        if self
            .event_sender
            .send(ActionEvent::Add(message))
            .await
            .is_err()
        {
            self.runtime.expire(&action_id).await;
            return Err(format!("failed to publish {what} approval request").into());
        }

        match tokio::time::timeout(ACTION_RESPONSE_TIMEOUT, rx).await {
            Ok(Ok(response)) if response.status == ActionStatus::Approved => Ok(()),
            Ok(Ok(response)) => Err(action_denied_error(&response.payload)),
            Ok(Err(_)) => Err(format!("{what} approval was cancelled").into()),
            Err(_) => {
                if let Some(pending) = self.runtime.expire(&action_id).await {
                    let response = json!({"reason": "approval timed out"});
                    let _ = pending
                        .event_sender
                        .send(ActionEvent::Resolve {
                            action_id,
                            status: ActionStatus::Expired,
                            response,
                            responded_at: unix_ms(),
                        })
                        .await;
                }
                Err(format!("{what} approval timed out").into())
            }
        }
    }

    async fn request_choice(&self, ctx: &BaseCtx, args: UserChoiceArgs) -> Result<Value, BoxError> {
        validate_choice_args(&args)?;
        let action_id = next_action_id();
        let now_ms = unix_ms();
        let conversation = self
            .conversation_id
            .load(std::sync::atomic::Ordering::SeqCst);
        let choices = args.choices.clone();
        let payload = ActionPayload {
            id: action_id.clone(),
            kind: "choice".to_string(),
            tool: Some(ActionToolRef::Name(AskUserChoiceTool::NAME.to_string())),
            agent: ctx.agent.clone(),
            conversation,
            session: self.session_id.clone(),
            title: args.title.clone(),
            message: args.message.clone(),
            choices: Some(args.choices),
            status: ActionStatus::Pending,
            created_at: now_ms,
            expires_at: now_ms + ACTION_RESPONSE_TIMEOUT.as_millis() as u64,
            ..Default::default()
        };
        let message = action_message(USER_CHOICE_ACTION, payload.into_value());
        let rx = self
            .runtime
            .register(PendingAction {
                action_id: action_id.clone(),
                caller: self.caller.clone(),
                conversation,
                kind: PendingActionKind::Choice { choices },
                event_sender: self.event_sender.clone(),
                tx: oneshot::channel().0,
            })
            .await;
        if self
            .event_sender
            .send(ActionEvent::Add(message))
            .await
            .is_err()
        {
            self.runtime.expire(&action_id).await;
            return Err("failed to publish user choice request".into());
        }

        match tokio::time::timeout(ACTION_RESPONSE_TIMEOUT, rx).await {
            Ok(Ok(response)) if response.status == ActionStatus::Selected => Ok(response.payload),
            Ok(Ok(response)) => Err(action_denied_error(&response.payload)),
            Ok(Err(_)) => Err("user choice was cancelled".into()),
            Err(_) => {
                if let Some(pending) = self.runtime.expire(&action_id).await {
                    let response = json!({"reason": "choice timed out"});
                    let _ = pending
                        .event_sender
                        .send(ActionEvent::Resolve {
                            action_id,
                            status: ActionStatus::Expired,
                            response,
                            responded_at: unix_ms(),
                        })
                        .await;
                }
                Err("user choice timed out".into())
            }
        }
    }
}

struct PendingAction {
    action_id: String,
    caller: String,
    conversation: u64,
    kind: PendingActionKind,
    event_sender: mpsc::Sender<ActionEvent>,
    tx: oneshot::Sender<ActionResponse>,
}

enum PendingActionKind {
    Approval { approved_payload: Value },
    Choice { choices: Vec<UserChoiceOption> },
}

impl PendingActionKind {
    fn response_from_args(&self, args: &ActionResponseArgs) -> Result<ActionResponse, BoxError> {
        match self {
            Self::Approval { approved_payload } => {
                let approved = args.approve.ok_or("approve is required")?;
                let status = if approved {
                    ActionStatus::Approved
                } else {
                    ActionStatus::Denied
                };
                let payload = if approved {
                    merge_approval_payload(approved_payload.clone(), true)
                } else {
                    json!({ "approve": false })
                };
                Ok(ActionResponse { status, payload })
            }
            Self::Choice { choices } => {
                let choice_id = args
                    .choice_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|choice_id| !choice_id.is_empty())
                    .ok_or("choice_id is required")?;
                let Some(choice) = choices.iter().find(|choice| choice.id == choice_id) else {
                    return Err("unknown choice_id".into());
                };
                let choice_text = if choice.input.is_some() {
                    args.choice_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                } else {
                    None
                };
                if choice.input.as_ref().is_some_and(|input| input.required)
                    && choice_text.is_none()
                {
                    return Err("choice_text is required".into());
                }
                let value = choice_text
                    .or(choice.value.as_deref())
                    .unwrap_or(&choice.label);
                let mut payload = json!({
                    "choice_id": choice_id,
                    "label": &choice.label,
                    "value": value,
                });
                if let Some(choice_text) = choice_text
                    && let Some(object) = payload.as_object_mut()
                {
                    object.insert("choice_text".to_string(), choice_text.into());
                }
                Ok(ActionResponse {
                    status: ActionStatus::Selected,
                    payload,
                })
            }
        }
    }
}

fn merge_approval_payload(payload: Value, approved: bool) -> Value {
    match payload {
        Value::Object(mut object) => {
            object.insert("approve".to_string(), approved.into());
            Value::Object(object)
        }
        value => json!({
            "approve": approved,
            "value": value,
        }),
    }
}

#[derive(Clone, Debug)]
struct ActionResponse {
    status: ActionStatus,
    payload: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub(crate) enum ActionsToolArgs {
    RespondAction {
        action_id: String,
        #[serde(default)]
        approve: Option<bool>,
        #[serde(default)]
        choice_id: Option<String>,
        #[serde(default)]
        choice_text: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ActionResponseArgs {
    pub(crate) action_id: String,
    pub(crate) approve: Option<bool>,
    pub(crate) choice_id: Option<String>,
    pub(crate) choice_text: Option<String>,
}

impl From<ActionsToolArgs> for ActionResponseArgs {
    fn from(value: ActionsToolArgs) -> Self {
        match value {
            ActionsToolArgs::RespondAction {
                action_id,
                approve,
                choice_id,
                choice_text,
            } => Self {
                action_id,
                approve,
                choice_id,
                choice_text,
            },
        }
    }
}

pub(crate) struct ActionsTool {
    runtime: Arc<ActionRuntime>,
}

impl ActionsTool {
    pub(crate) const NAME: &'static str = "actions_api";

    pub(crate) fn new(runtime: Arc<ActionRuntime>) -> Self {
        Self { runtime }
    }
}

impl Tool<BaseCtx> for ActionsTool {
    type Args = ActionsToolArgs;
    type Output = ActionApiOutput;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Respond to pending user action cards such as shell approvals and user choices.".to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: actions_tool_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        if ctx.get_state::<ActionSession>().is_some() {
            return Err("actions_api cannot be called from an active agent session".into());
        }
        let conversation = ctx.meta().get_extra_as::<u64>("conversation").unwrap_or(0);
        let caller = ctx.caller().to_text();
        let output = self
            .runtime
            .respond(&caller, conversation, args.into())
            .await?;
        Ok(ToolOutput::new(output))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct UserChoiceArgs {
    pub title: String,
    #[serde(default)]
    pub message: Option<String>,
    pub choices: Vec<UserChoiceOption>,
}

pub(crate) struct AskUserChoiceTool;

impl AskUserChoiceTool {
    pub(crate) const NAME: &'static str = "ask_user_choice";
}

impl Tool<BaseCtx> for AskUserChoiceTool {
    type Args = UserChoiceArgs;
    type Output = Value;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Ask the user to choose one option from a small set of suggested next actions. Use this when user intent is ambiguous or confirmation should be collected with buttons instead of free-form text. A choice can include an input field when the selected option needs the user to type details.".to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: user_choice_tool_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let Some(action_session) = ctx.get_state::<ActionSession>() else {
            return Err("user choice actions require an active session".into());
        };
        let output = action_session.request_choice(&ctx, args).await?;
        Ok(ToolOutput {
            output,
            artifacts: Vec::new(),
            usage: Usage::default(),
            tools_usage: HashMap::new(),
            is_error: None,
        })
    }
}

fn actions_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": ["RespondAction"],
                "description": "Action API operation."
            },
            "action_id": {
                "type": "string",
                "description": "The pending action id from the action card payload."
            },
            "approve": {
                "type": ["boolean", "null"],
                "description": "For shell approvals, true approves and false denies. Null for choice cards."
            },
            "choice_id": {
                "type": ["string", "null"],
                "description": "For choice cards, the selected choice id. Null for shell approvals."
            },
            "choice_text": {
                "type": ["string", "null"],
                "description": "For choice cards with an input field, the user-entered text. Null otherwise."
            }
        },
        "required": ["type", "action_id", "approve", "choice_id", "choice_text"],
        "additionalProperties": false
    })
}

fn user_choice_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "description": "Short card title shown to the user."
            },
            "message": {
                "type": ["string", "null"],
                "description": "Optional short explanation shown above the choices."
            },
            "choices": {
                "type": "array",
                "minItems": 1,
                "maxItems": 6,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Stable choice id."
                        },
                        "label": {
                            "type": "string",
                            "description": "Button label shown to the user."
                        },
                        "value": {
                            "type": ["string", "null"],
                            "description": "Optional value returned to the model. Defaults to label."
                        },
                        "description": {
                            "type": ["string", "null"],
                            "description": "Optional short helper text shown under the label."
                        },
                        "input": {
                            "type": ["object", "null"],
                            "description": "Input configuration when selecting this option should ask the user to type extra content. Use null for a plain button option.",
                            "properties": {
                                "placeholder": {
                                    "type": ["string", "null"],
                                    "description": "Optional placeholder shown in the text field."
                                },
                                "required": {
                                    "type": "boolean",
                                    "description": "Whether the user must type non-empty text before selecting this option."
                                },
                                "multiline": {
                                    "type": "boolean",
                                    "description": "Whether to show a multiline text area instead of a single-line input."
                                }
                            },
                            "required": ["placeholder", "required", "multiline"],
                            "additionalProperties": false
                        }
                    },
                    "required": ["id", "label", "value", "description", "input"],
                    "additionalProperties": false
                },
                "description": "The choices to show. Keep this list small and concrete. Set `input` when an option needs the user to fill in details before submitting."
            }
        },
        "required": ["title", "message", "choices"],
        "additionalProperties": false
    })
}

fn next_action_id() -> String {
    format!("act_{}", Xid::new())
}

/// Fail-closed approval gate for the MCP server tools: outside FullAccess mode
/// the user must confirm, and when no [`ActionSession`] is available in the
/// context (so no approval card can be shown) the call is rejected.
pub(crate) async fn require_mcp_approval(
    ctx: &BaseCtx,
    tool_name: &str,
    summary: String,
    details: Vec<ActionDetail>,
    metadata: Value,
) -> Result<(), BoxError> {
    if ApprovalMode::from_ctx(ctx) == ApprovalMode::FullAccess {
        return Ok(());
    }
    let Some(session) = ctx.get_state::<ActionSession>() else {
        return Err(
            "adding or connecting an MCP server requires user approval, \
             which is not available in this context"
                .into(),
        );
    };
    session
        .request_mcp_approval(ctx, tool_name, summary, details, metadata)
        .await
}

fn validate_choice_args(args: &UserChoiceArgs) -> Result<(), BoxError> {
    if args.title.trim().is_empty() {
        return Err("title is required".into());
    }
    if args.choices.is_empty() || args.choices.len() > 6 {
        return Err("choices must contain 1 to 6 items".into());
    }
    let mut seen = std::collections::HashSet::new();
    for choice in &args.choices {
        if choice.id.trim().is_empty() {
            return Err("choice id is required".into());
        }
        if choice.label.trim().is_empty() {
            return Err("choice label is required".into());
        }
        if !seen.insert(choice.id.trim().to_string()) {
            return Err("choice ids must be unique".into());
        }
    }
    Ok(())
}

fn action_denied_error(payload: &Value) -> BoxError {
    let reason = payload
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or("denied by user");
    format!("action denied: {reason}").into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::ContentPart;
    use protocol::UserChoiceInput;

    #[test]
    fn action_tool_schemas_are_strict() {
        assert_openai_strict_parameters(&actions_tool_parameters());
        assert_openai_strict_parameters(&user_choice_tool_parameters());
    }

    #[test]
    fn action_message_helpers_find_and_update_action() {
        let mut message = json!(action_message(
            USER_CHOICE_ACTION,
            json!({"id": "act_1", "status": "pending"})
        ));

        assert!(is_action_message_value(&message));
        assert_eq!(
            action_id_from_message_value(&message).as_deref(),
            Some("act_1")
        );

        assert!(apply_action_resolution_to_message(
            &mut message,
            "act_1",
            ActionStatus::Selected,
            &json!({"choice_id": "a"}),
            10
        ));
        assert_eq!(message["content"][0]["payload"]["status"], "selected");
        assert_eq!(
            message["content"][0]["payload"]["response"]["choice_id"],
            "a"
        );
    }

    fn meta_with(entries: &[(&str, Value)]) -> RequestMeta {
        let mut extra = serde_json::Map::new();
        for (key, value) in entries {
            extra.insert((*key).to_string(), value.clone());
        }
        RequestMeta {
            extra,
            ..Default::default()
        }
    }

    #[test]
    fn approval_mode_follows_the_live_session_request_meta() {
        // The context metadata of a running session is frozen at creation, so a
        // later request that joins it (a CLI started with --full-access) only
        // shows up in SessionRequestMeta.
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        assert_eq!(ApprovalMode::from_ctx(&ctx), ApprovalMode::OnRisk);

        ctx.set_state(SessionRequestMeta::new(meta_with(&[(
            "approval_mode",
            json!("full_access"),
        )])));
        assert_eq!(ApprovalMode::from_ctx(&ctx), ApprovalMode::FullAccess);
    }

    #[test]
    fn cron_runs_are_unattended_and_get_full_access() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        // Nobody can answer an approval card for a scheduled job, so the
        // declared mode must not be able to stall it until it expires.
        ctx.set_state(SessionRequestMeta::new(meta_with(&[
            ("cron_job_id", json!(7u64)),
            ("approval_mode", json!("request_approval")),
        ])));

        assert_eq!(ApprovalMode::from_ctx(&ctx), ApprovalMode::FullAccess);
    }

    #[test]
    fn goal_mode_gets_full_access_only_while_an_objective_is_active() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        ctx.set_state(SessionRequestMeta::new(meta_with(&[(
            "approval_mode",
            json!("request_approval"),
        )])));

        let goal = Arc::new(parking_lot::RwLock::new(None));
        ctx.set_state(GoalToolState::new(
            goal.clone(),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        ));
        assert_eq!(ApprovalMode::from_ctx(&ctx), ApprovalMode::RequestApproval);

        *goal.write() = Some(crate::engine::goal::GoalState::new("ship it".to_string()));
        assert_eq!(ApprovalMode::from_ctx(&ctx), ApprovalMode::FullAccess);

        // Completing the objective hands control back to the declared mode.
        *goal.write() = None;
        assert_eq!(ApprovalMode::from_ctx(&ctx), ApprovalMode::RequestApproval);
    }

    #[test]
    fn tool_approval_payload_uses_generic_fields() {
        let message = json!(action_message(
            TOOL_APPROVAL_ACTION,
            json!({
                "id": "act_1",
                "kind": "tool_approval",
                "tool": {"name": "payments", "label": "Payment"},
                "title": "Approve payment",
                "summary": "Pay $10.00",
                "details": [approval_detail("Amount", "$10.00", "text")],
                "approval": {"approve_label": "Pay", "deny_label": "Cancel"},
                "status": "pending"
            })
        ));

        assert_eq!(message["content"][0]["name"], TOOL_APPROVAL_ACTION);
        assert_eq!(message["content"][0]["payload"]["kind"], "tool_approval");
        assert_eq!(message["content"][0]["payload"]["tool"]["name"], "payments");
        assert_eq!(
            message["content"][0]["payload"]["details"][0]["label"],
            "Amount"
        );
    }

    #[tokio::test]
    async fn shell_approval_payload_avoids_duplicate_command_and_localizes_reason() {
        let home = tempfile::tempdir().unwrap();
        let launcher_dir = home.path().join("launcher");
        std::fs::create_dir_all(&launcher_dir).unwrap();
        std::fs::write(launcher_dir.join("ui.json"), r#"{"language":"zh-Hans"}"#).unwrap();

        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        let caller = ctx.caller().to_text();
        let conversation_id = Arc::new(std::sync::atomic::AtomicU64::new(42));
        let runtime = Arc::new(ActionRuntime::new());
        let (event_sender, mut event_rx) = mpsc::channel(4);
        let session = ActionSession::new(
            runtime.clone(),
            event_sender,
            caller.clone(),
            "session_1".to_string(),
            conversation_id,
            Arc::new(Models::default()),
            home.path().to_path_buf(),
        );
        let args = ExecArgs {
            command: "rm -rf target".to_string(),
            ..Default::default()
        };

        let request = tokio::spawn(async move { session.request_shell_approval(&ctx, args).await });
        let Some(ActionEvent::Add(message)) = event_rx.recv().await else {
            panic!("expected shell approval action");
        };
        let Some(ContentPart::Action { payload, .. }) = message.content.first() else {
            panic!("expected action payload");
        };

        assert_eq!(payload["summary"], "rm -rf target");
        assert_eq!(payload["command"], "rm -rf target");
        let details = payload["details"]
            .as_array()
            .expect("details should be array");
        assert!(
            !details
                .iter()
                .any(|detail| detail["label"].as_str() == Some("Command"))
        );
        let reason = details
            .iter()
            .find(|detail| detail["label"].as_str() == Some("审批原因"))
            .expect("approval reason detail");
        assert_eq!(
            reason["value"],
            "该命令可能会访问网络、写入文件或更改系统状态。"
        );

        runtime
            .respond(
                &caller,
                42,
                ActionResponseArgs {
                    action_id: payload["id"].as_str().unwrap().to_string(),
                    approve: Some(true),
                    choice_id: None,
                    choice_text: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            request.await.unwrap().unwrap().command,
            "rm -rf target".to_string()
        );
    }

    #[tokio::test]
    async fn mcp_approval_gate_fails_closed_and_requires_confirmation() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;

        // Without an ActionSession in the context (and outside FullAccess
        // mode), the gate must fail closed instead of letting the tool run.
        let err = require_mcp_approval(
            &ctx,
            "add_mcp_server",
            "Run local MCP server: npx server".to_string(),
            vec![],
            json!({}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("approval"));

        // With a session, an approval card is published; a deny resolves to an
        // error and the tool must not proceed.
        let caller = ctx.caller().to_text();
        let conversation_id = Arc::new(std::sync::atomic::AtomicU64::new(7));
        let runtime = Arc::new(ActionRuntime::new());
        let (event_sender, mut event_rx) = mpsc::channel(4);
        let session = ActionSession::new(
            runtime.clone(),
            event_sender,
            caller.clone(),
            "session_1".to_string(),
            conversation_id,
            Arc::new(Models::default()),
            std::env::temp_dir(),
        );
        ctx.set_state(session);

        let ctx2 = ctx.clone();
        let request = tokio::spawn(async move {
            require_mcp_approval(
                &ctx2,
                "add_mcp_server",
                "Run local MCP server: npx server".to_string(),
                vec![approval_detail("Command", "npx", "text")],
                json!({"server_id": "srv"}),
            )
            .await
        });
        let Some(ActionEvent::Add(message)) = event_rx.recv().await else {
            panic!("expected MCP approval action");
        };
        let Some(ContentPart::Action { payload, .. }) = message.content.first() else {
            panic!("expected action payload");
        };
        assert_eq!(payload["kind"], "tool_approval");
        assert_eq!(payload["tool"]["name"], "add_mcp_server");
        assert_eq!(payload["status"], "pending");

        runtime
            .respond(
                &caller,
                7,
                ActionResponseArgs {
                    action_id: payload["id"].as_str().unwrap().to_string(),
                    approve: Some(false),
                    choice_id: None,
                    choice_text: None,
                },
            )
            .await
            .unwrap();
        assert!(request.await.unwrap().is_err());
    }

    #[test]
    fn choice_args_validate_ids() {
        let args = UserChoiceArgs {
            title: "Pick".to_string(),
            message: None,
            choices: vec![UserChoiceOption {
                id: "a".to_string(),
                label: "A".to_string(),
                value: None,
                description: None,
                input: None,
            }],
        };
        assert!(validate_choice_args(&args).is_ok());
    }

    #[test]
    fn choice_response_returns_selected_value() {
        let kind = PendingActionKind::Choice {
            choices: vec![UserChoiceOption {
                id: "a".to_string(),
                label: "Option A".to_string(),
                value: Some("value-a".to_string()),
                description: None,
                input: None,
            }],
        };

        let response = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: Some("a".to_string()),
                choice_text: None,
            })
            .unwrap();

        assert_eq!(response.status, ActionStatus::Selected);
        assert_eq!(response.payload["choice_id"], "a");
        assert_eq!(response.payload["label"], "Option A");
        assert_eq!(response.payload["value"], "value-a");
    }

    #[test]
    fn choice_response_returns_entered_text() {
        let kind = PendingActionKind::Choice {
            choices: vec![UserChoiceOption {
                id: "custom".to_string(),
                label: "Custom".to_string(),
                value: None,
                description: None,
                input: Some(UserChoiceInput {
                    placeholder: Some("Describe it".to_string()),
                    required: true,
                    multiline: true,
                }),
            }],
        };

        let response = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: Some("custom".to_string()),
                choice_text: Some("Please focus on the UI state.".to_string()),
            })
            .unwrap();

        assert_eq!(response.status, ActionStatus::Selected);
        assert_eq!(response.payload["choice_id"], "custom");
        assert_eq!(response.payload["label"], "Custom");
        assert_eq!(response.payload["value"], "Please focus on the UI state.");
        assert_eq!(
            response.payload["choice_text"],
            "Please focus on the UI state."
        );
    }

    #[test]
    fn choice_response_rejects_missing_required_text() {
        let kind = PendingActionKind::Choice {
            choices: vec![UserChoiceOption {
                id: "custom".to_string(),
                label: "Custom".to_string(),
                value: None,
                description: None,
                input: Some(UserChoiceInput {
                    placeholder: None,
                    required: true,
                    multiline: false,
                }),
            }],
        };

        let err = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: Some("custom".to_string()),
                choice_text: Some("   ".to_string()),
            })
            .unwrap_err();

        assert_eq!(err.to_string(), "choice_text is required");
    }

    #[test]
    fn approval_response_preserves_tool_payload() {
        let kind = PendingActionKind::Approval {
            approved_payload: json!({
                "tool": "payments",
                "payment_id": "pay_1"
            }),
        };

        let response = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: Some(true),
                choice_id: None,
                choice_text: None,
            })
            .unwrap();

        assert_eq!(response.status, ActionStatus::Approved);
        assert_eq!(response.payload["approve"], true);
        assert_eq!(response.payload["tool"], "payments");
        assert_eq!(response.payload["payment_id"], "pay_1");
    }

    #[test]
    fn approval_response_requires_explicit_decision() {
        let kind = PendingActionKind::Approval {
            approved_payload: json!({"tool": "payments"}),
        };

        let err = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: None,
                choice_text: None,
            })
            .unwrap_err();

        assert_eq!(err.to_string(), "approve is required");
    }

    #[tokio::test]
    async fn invalid_action_response_keeps_pending_for_retry() {
        let runtime = ActionRuntime::new();
        let (event_sender, mut event_rx) = mpsc::channel(4);
        let action_id = "act_retry".to_string();
        let rx = runtime
            .register(PendingAction {
                action_id: action_id.clone(),
                caller: "caller".to_string(),
                conversation: 42,
                kind: PendingActionKind::Choice {
                    choices: vec![UserChoiceOption {
                        id: "a".to_string(),
                        label: "Option A".to_string(),
                        value: None,
                        description: None,
                        input: None,
                    }],
                },
                event_sender,
                tx: oneshot::channel().0,
            })
            .await;

        let err = runtime
            .respond(
                "caller",
                42,
                ActionResponseArgs {
                    action_id: action_id.clone(),
                    approve: None,
                    choice_id: Some("missing".to_string()),
                    choice_text: None,
                },
            )
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "unknown choice_id");
        assert!(runtime.pending.lock().await.contains_key(&action_id));

        let output = runtime
            .respond(
                "caller",
                42,
                ActionResponseArgs {
                    action_id: action_id.clone(),
                    approve: None,
                    choice_id: Some("a".to_string()),
                    choice_text: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(output.action_id, action_id);
        assert_eq!(output.conversation, 42);
        assert_eq!(output.status, "selected");
        assert_eq!(output.response["choice_id"], "a");
        assert!(output.responded_at > 0);

        let response = rx.await.unwrap();
        assert_eq!(response.status, ActionStatus::Selected);
        let Some(ActionEvent::Resolve {
            action_id, status, ..
        }) = event_rx.recv().await
        else {
            panic!("expected resolve event");
        };
        assert_eq!(action_id, "act_retry");
        assert_eq!(status, ActionStatus::Selected);
    }
}
