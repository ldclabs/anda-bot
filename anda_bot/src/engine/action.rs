use anda_core::{
    BoxError, FunctionDefinition, Message, RequestMeta, Resource, StateFeatures, Tool, ToolOutput,
};
use anda_engine::{
    context::BaseCtx,
    extension::shell::{CommandArgs, ShellTool},
    model::Models,
    unix_ms,
};
use ic_auth_types::Xid;
use parking_lot::Mutex;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

use super::{agent::SessionRequestMeta, goal::GoalToolState};
use crate::util::request_meta::keys;

mod shell_policy;

pub(crate) use shell_policy::ApprovalMode;
use shell_policy::{
    ApprovalDecision, ShellRiskCache, shell_approval_decision_with_model, shell_risk_language_hint,
};

mod protocol;

pub(crate) use protocol::{
    ActionApiOutput, ActionDetail, ActionStatus, ApprovalLabels, TOOL_APPROVAL_ACTION,
    USER_CHOICE_ACTION, UserChoiceInput, UserChoiceOption, action_id_from_message,
    action_id_from_message_value, action_message, apply_action_resolution_to_chat_message,
    apply_action_resolution_to_message, approval_detail, is_action_message_value,
    payload_action_id, payload_is_pending, payload_responded_at, update_action_payload_resolution,
};
use protocol::{ActionPayload, ActionToolRef};

/// How long an approval, or a choice without a default, waits for the user.
const ACTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// A choice with a default waits less: the agent then proceeds with it.
const CHOICE_DEFAULT_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// Marks a choice the user answered by writing in chat instead.
const ANSWERED_IN_CHAT: &str = "answered_in_chat";

impl ApprovalMode {
    fn from_ctx(ctx: &BaseCtx, meta: &RequestMeta) -> Self {
        // Unattended runs have nobody on the other end of an approval card:
        // it would sit pending until ACTION_RESPONSE_TIMEOUT and then fail the
        // whole task. Grant full access instead so scheduled and autonomous
        // work can complete. Runs started by MCP events are not elevated:
        // the events are untrusted, so whatever needs approval is refused.
        let declared = Self::from_meta(meta);
        if let Some(reason) = elevated_run_reason(ctx, meta) {
            if declared != Self::FullAccess {
                log::debug!(
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

/// Returns why the current run is unattended and runs with full access, or
/// `None`.
fn elevated_run_reason(ctx: &BaseCtx, meta: &RequestMeta) -> Option<&'static str> {
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

/// Returns why the current run is unattended, or `None` when a human can answer.
fn unattended_run_reason(ctx: &BaseCtx, meta: &RequestMeta) -> Option<&'static str> {
    elevated_run_reason(ctx, meta).or_else(|| {
        meta.get_extra_as::<u64>(keys::MCP_TRIGGER_ID)
            .is_some()
            .then_some("MCP event automation")
    })
}

/// Returns why nobody can answer a choice card here, or `None` when one can be
/// shown. IM channels relay replies as text and never render action cards.
fn choice_unanswerable_reason(ctx: &BaseCtx, meta: &RequestMeta) -> Option<&'static str> {
    unattended_run_reason(ctx, meta).or_else(|| {
        meta.get_extra_as::<String>(keys::REPLY_TARGET)
            .is_some()
            .then_some("IM channel")
    })
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

    fn insert(&self, pending: PendingAction) {
        self.pending
            .lock()
            .insert(pending.action_id.clone(), pending);
    }

    fn take(&self, action_id: &str) -> Option<PendingAction> {
        self.pending.lock().remove(action_id)
    }

    /// Removes the pending actions of `session` whose kind matches `filter`.
    fn take_session(
        &self,
        session: &str,
        mut filter: impl FnMut(&PendingActionKind) -> bool,
    ) -> Vec<PendingAction> {
        self.pending
            .lock()
            .extract_if(|_, action| action.session == session && filter(&action.kind))
            .map(|(_, action)| action)
            .collect()
    }

    /// Denies every pending action of `session` and returns the resolutions
    /// for the caller to apply: it is the runner draining the event channel.
    fn cancel_session(&self, session: &str) -> Vec<ActionEvent> {
        self.take_session(session, |_| true)
            .into_iter()
            .map(|action| {
                let response =
                    ActionResponse::new(ActionStatus::Denied, json!({"reason": "task stopped"}));
                let event = response.event(action.action_id);
                let _ = action.tx.send(response);
                event
            })
            .collect()
    }

    pub(crate) async fn respond(
        &self,
        caller: &str,
        conversation: u64,
        args: ActionResponseArgs,
    ) -> Result<ActionApiOutput, BoxError> {
        let (pending, response) = {
            let mut pending_actions = self.pending.lock();
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
            let pending = pending_actions
                .remove(&args.action_id)
                .expect("pending action exists");
            (pending, response)
        };

        let output = ActionApiOutput {
            action_id: pending.action_id.clone(),
            conversation: pending.conversation,
            status: response.status.as_str().to_string(),
            response: response.payload.clone(),
            responded_at: response.responded_at,
        };
        pending.resolve(response).await;
        Ok(output)
    }
}

#[derive(Clone)]
pub(crate) struct ActionSession {
    runtime: Arc<ActionRuntime>,
    event_sender: mpsc::Sender<ActionEvent>,
    caller: String,
    session_id: String,
    conversation_id: Arc<AtomicU64>,
    models: Arc<Models>,
    home_dir: PathBuf,
    risk_cache: Arc<ShellRiskCache>,
}

impl ActionSession {
    pub(crate) fn new(
        runtime: Arc<ActionRuntime>,
        event_sender: mpsc::Sender<ActionEvent>,
        caller: String,
        session_id: String,
        conversation_id: Arc<AtomicU64>,
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
            risk_cache: Arc::default(),
        }
    }

    pub(crate) fn cancel_pending(&self) -> Vec<ActionEvent> {
        self.runtime.cancel_session(&self.session_id)
    }

    /// The user wrote in chat while a choice card was waiting. Release the
    /// waiting tool now, so the agent reads that message instead of the card
    /// running out its timeout. Approvals keep waiting for an explicit answer.
    pub(crate) async fn answer_choices_in_chat(&self) {
        let answered = self.runtime.take_session(&self.session_id, |kind| {
            matches!(kind, PendingActionKind::Choice { .. })
        });
        for action in answered {
            action
                .resolve(ActionResponse::new(
                    ActionStatus::Expired,
                    json!({
                        "reason": "answered in chat",
                        ANSWERED_IN_CHAT: true,
                        "note": "The user replied in chat instead of picking an option. Their message reaches you after this step: do not pick an option yourself; stop here and follow that message.",
                    }),
                ))
                .await;
        }
    }

    fn conversation(&self) -> u64 {
        self.conversation_id.load(Ordering::SeqCst)
    }

    /// The fields every card shares; callers fill in their kind-specific ones.
    fn new_payload(&self, ctx: &BaseCtx, kind: &PendingActionKind, title: String) -> ActionPayload {
        let now_ms = unix_ms();
        ActionPayload {
            id: next_action_id(),
            kind: kind.payload_kind().to_string(),
            agent: ctx.agent.clone(),
            conversation: self.conversation(),
            session: self.session_id.clone(),
            title,
            status: ActionStatus::Pending,
            created_at: now_ms,
            expires_at: now_ms + kind.timeout().as_millis() as u64,
            ..Default::default()
        }
    }

    pub(crate) async fn request_shell_approval(
        &self,
        ctx: &BaseCtx,
        args: CommandArgs,
    ) -> Result<CommandArgs, BoxError> {
        let meta = live_request_meta(ctx);
        let approval_mode = ApprovalMode::from_ctx(ctx, &meta);
        if approval_mode == ApprovalMode::FullAccess {
            return Ok(args);
        }
        let workspace = meta
            .get_extra_as::<String>(keys::WORKSPACE)
            .unwrap_or_default();
        let language_hint = shell_risk_language_hint(&meta)
            .or_else(|| crate::util::locale::persisted_ui_language(&self.home_dir));
        let approval_reason = match shell_approval_decision_with_model(
            &args,
            approval_mode,
            &workspace,
            self.models.as_ref(),
            language_hint.as_deref(),
            &self.risk_cache,
        )
        .await
        {
            ApprovalDecision::Allow => return Ok(args),
            ApprovalDecision::Ask(reason) => reason,
        };
        if let Some(reason) = unattended_run_reason(ctx, &meta) {
            return Err(format!(
                "this shell command needs the user's approval ({approval_reason}), which nobody \
                 can give in this {reason}, so it was NOT run"
            )
            .into());
        }
        let approval_locale = language_hint.as_deref().unwrap_or("en");
        let mut details = Vec::new();
        if !workspace.is_empty() {
            details.push(approval_detail("Workspace", workspace, "text"));
        }
        if let Some(cwd) = &args.cwd {
            details.push(approval_detail("Working directory", cwd, "text"));
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

        let kind = PendingActionKind::Approval {
            approved_payload: json!({
                "tool": ShellTool::NAME,
                "command": &args.command,
            }),
            rememberable: false,
        };
        let payload = ActionPayload {
            tool: Some(ActionToolRef::labeled(ShellTool::NAME, "Shell command")),
            message: Some("The agent wants to run a local shell command.".to_string()),
            summary: Some(args.command.clone()),
            command: Some(args.command.clone()),
            details: Some(details),
            approval: Some(ApprovalLabels::approve_deny()),
            metadata: Some(json!({
                "command": &args.command,
                "cwd": &args.cwd,
                "env_keys": &args.env_keys,
                "background": args.background,
                "approval_mode": approval_mode.as_str(),
                "approval_reason": &approval_reason,
            })),
            ..self.new_payload(ctx, &kind, "Approve shell command".to_string())
        };
        self.request_approval(payload, kind, "shell command")
            .await
            .map(|_| args)
    }

    /// Shows an MCP approval card and waits for the answer: one for adding,
    /// connecting or changing a server ([`require_mcp_approval`]), or for one
    /// call of a server's tool ([`request_mcp_tool_approval`]). A card with a
    /// `remember_label` offers to stop asking; the result says whether the
    /// user chose that.
    async fn request_mcp_approval(
        &self,
        ctx: &BaseCtx,
        tool: ActionToolRef,
        title: String,
        card: McpApprovalCard,
        remember_label: Option<&str>,
    ) -> Result<bool, BoxError> {
        let McpApprovalCard {
            message,
            summary,
            details,
            metadata,
        } = card;
        let tool_name = match &tool {
            ActionToolRef::Labeled { name, .. } | ActionToolRef::Name(name) => name.clone(),
        };
        let kind = PendingActionKind::Approval {
            approved_payload: json!({
                "tool": tool_name,
                "summary": &summary,
            }),
            rememberable: remember_label.is_some(),
        };
        let payload = ActionPayload {
            tool: Some(tool),
            message: Some(message),
            summary: Some(summary),
            details: Some(details),
            approval: Some(match remember_label {
                Some(label) => ApprovalLabels::approve_deny_remember(label),
                None => ApprovalLabels::approve_deny(),
            }),
            metadata: Some(metadata),
            ..self.new_payload(ctx, &kind, title)
        };
        self.request_approval(payload, kind, "MCP server").await
    }

    /// Waits for an approval; `Ok(true)` when the user also asked not to be
    /// asked again.
    async fn request_approval(
        &self,
        payload: ActionPayload,
        kind: PendingActionKind,
        what: &str,
    ) -> Result<bool, BoxError> {
        let response = self.publish_and_wait(payload, kind, what).await?;
        match response.status {
            ActionStatus::Approved => Ok(response.payload["remember"] == true),
            // Approvals never pass by default: an unanswered one is refused,
            // and the error says so plainly so the model neither mistakes it
            // for a failed run nor queues the same card again.
            ActionStatus::Expired => Err(format!(
                "approval for the {what} expired: the user did not respond within {} minutes, \
                 so it was NOT carried out. Do not retry the same request now; continue another \
                 way, or tell the user what needs approval.",
                ACTION_RESPONSE_TIMEOUT.as_secs() / 60
            )
            .into()),
            _ => Err(action_denied_error(&response.payload)),
        }
    }

    /// Publishes an action card and waits for it to resolve. A card nobody
    /// answers in time resolves through [`PendingActionKind::unanswered`].
    async fn publish_and_wait(
        &self,
        payload: ActionPayload,
        kind: PendingActionKind,
        what: &str,
    ) -> Result<ActionResponse, BoxError> {
        let action_id = payload.id.clone();
        let conversation = payload.conversation;
        let timeout = kind.timeout();
        let message = action_message(kind.message_name(), payload.into_value());
        let (tx, mut rx) = oneshot::channel();
        self.runtime.insert(PendingAction {
            session: self.session_id.clone(),
            action_id: action_id.clone(),
            caller: self.caller.clone(),
            conversation,
            kind,
            event_sender: self.event_sender.clone(),
            tx,
        });
        if self
            .event_sender
            .send(ActionEvent::Add(message))
            .await
            .is_err()
        {
            self.runtime.take(&action_id);
            return Err(format!("failed to publish {what} request").into());
        }

        let cancelled = || -> BoxError { format!("{what} was cancelled").into() };
        match tokio::time::timeout(timeout, &mut rx).await {
            Ok(response) => response.map_err(|_| cancelled()),
            Err(_) => match self.runtime.take(&action_id) {
                Some(pending) => {
                    let response = pending.kind.unanswered(&format!(
                        "no response within {} minutes",
                        timeout.as_secs() / 60
                    ));
                    let _ = self.event_sender.send(response.event(action_id)).await;
                    Ok(response)
                }
                // An answer or a stop took the action right at the deadline;
                // its response is already on the way.
                None => rx.await.map_err(|_| cancelled()),
            },
        }
    }

    async fn request_choice(
        &self,
        ctx: &BaseCtx,
        mut args: UserChoiceArgs,
    ) -> Result<Value, BoxError> {
        normalize_choice_args(&mut args)?;
        let kind = PendingActionKind::Choice {
            choices: args.choices.clone(),
            default_choice_id: args.default_choice_id.clone(),
            limit: None,
        };
        let meta = live_request_meta(ctx);
        if let Some(reason) = choice_unanswerable_reason(ctx, &meta) {
            return choice_result(
                kind.unanswered(&format!("nobody can answer choice cards in this {reason}")),
            );
        }

        let payload = ActionPayload {
            tool: Some(ActionToolRef::Name(AskUserChoiceTool::NAME.to_string())),
            message: args.message,
            choices: Some(args.choices),
            default_choice_id: args.default_choice_id,
            ..self.new_payload(ctx, &kind, args.title)
        };
        choice_result(self.publish_and_wait(payload, kind, "user choice").await?)
    }

    /// Shows one card of an MCP server's request for input and waits, at
    /// most `card.timeout`, for the user's pick. Where no choice card can be
    /// answered (automations, scheduled jobs, IM chats) nothing is shown.
    pub(crate) async fn request_mcp_input(
        &self,
        ctx: &BaseCtx,
        card: McpInputCard,
    ) -> Result<McpInputAnswer, BoxError> {
        let meta = live_request_meta(ctx);
        if let Some(reason) = choice_unanswerable_reason(ctx, &meta) {
            return Ok(McpInputAnswer::Unanswered(format!(
                "nobody can answer it in this {reason}"
            )));
        }
        let kind = PendingActionKind::Choice {
            choices: card.choices.clone(),
            default_choice_id: None,
            limit: Some(card.timeout),
        };
        let payload = ActionPayload {
            tool: Some(ActionToolRef::labeled(&card.tool, &card.tool_label)),
            message: card.message,
            details: (!card.details.is_empty()).then_some(card.details),
            choices: Some(card.choices),
            metadata: Some(card.metadata),
            ..self.new_payload(ctx, &kind, card.title)
        };
        // The engine drops this wait when the server's deadline passes or the
        // call ends; the card must close with it.
        let _close = AbandonGuard {
            runtime: &self.runtime,
            action_id: payload.id.clone(),
            event_sender: &self.event_sender,
        };
        let response = self
            .publish_and_wait(payload, kind, "MCP server input")
            .await?;
        let text = |key: &str| {
            response
                .payload
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        Ok(match response.status {
            ActionStatus::Selected => McpInputAnswer::Choice {
                id: text("choice_id").unwrap_or_default(),
                text: text("choice_text"),
            },
            ActionStatus::Expired if response.payload[ANSWERED_IN_CHAT] == true => {
                McpInputAnswer::InChat
            }
            _ => McpInputAnswer::Unanswered(
                text("reason").unwrap_or_else(|| "no answer".to_string()),
            ),
        })
    }
}

/// One card of an MCP server's request for input (elicitation).
pub(crate) struct McpInputCard {
    pub tool: String,
    pub tool_label: String,
    pub title: String,
    pub message: Option<String>,
    pub details: Vec<ActionDetail>,
    pub choices: Vec<UserChoiceOption>,
    pub metadata: Value,
    /// How long the server waits for the answer.
    pub timeout: Duration,
}

/// What the user did with an [`McpInputCard`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum McpInputAnswer {
    /// Picked an option, with the text typed into it.
    Choice { id: String, text: Option<String> },
    /// Wrote in chat instead.
    InChat,
    /// Nobody could or did answer; the reason.
    Unanswered(String),
}

/// Resolves an MCP input card whose waiter went away, cancelled or timed out
/// with its call, so the card does not stay open with nobody to take the
/// answer. A card answered, expired or stopped is no longer pending, and this
/// does nothing. Other cards wait for their session: stopping it resolves
/// them as stopped.
struct AbandonGuard<'a> {
    runtime: &'a ActionRuntime,
    action_id: String,
    event_sender: &'a mpsc::Sender<ActionEvent>,
}

impl Drop for AbandonGuard<'_> {
    fn drop(&mut self) {
        if self.runtime.take(&self.action_id).is_some() {
            let response = ActionResponse::new(
                ActionStatus::Expired,
                json!({ "reason": "the request was cancelled" }),
            );
            let _ = self
                .event_sender
                .try_send(response.event(std::mem::take(&mut self.action_id)));
        }
    }
}

struct PendingAction {
    session: String,
    action_id: String,
    caller: String,
    conversation: u64,
    kind: PendingActionKind,
    event_sender: mpsc::Sender<ActionEvent>,
    tx: oneshot::Sender<ActionResponse>,
}

impl PendingAction {
    /// Publishes the resolution, then releases the waiting tool. The event goes
    /// first so the card is resolved before the tool result reaches the model.
    async fn resolve(self, response: ActionResponse) {
        let _ = self.event_sender.send(response.event(self.action_id)).await;
        let _ = self.tx.send(response);
    }
}

enum PendingActionKind {
    Approval {
        approved_payload: Value,
        /// The card offers to approve without asking again.
        rememberable: bool,
    },
    Choice {
        choices: Vec<UserChoiceOption>,
        default_choice_id: Option<String>,
        /// Waits this long instead of the usual timeout.
        limit: Option<Duration>,
    },
}

impl PendingActionKind {
    fn payload_kind(&self) -> &'static str {
        match self {
            Self::Approval { .. } => "tool_approval",
            Self::Choice { .. } => "choice",
        }
    }

    fn message_name(&self) -> &'static str {
        match self {
            Self::Approval { .. } => TOOL_APPROVAL_ACTION,
            Self::Choice { .. } => USER_CHOICE_ACTION,
        }
    }

    fn timeout(&self) -> Duration {
        match self {
            Self::Choice {
                limit: Some(limit), ..
            } => *limit,
            Self::Choice {
                default_choice_id: Some(_),
                ..
            } => CHOICE_DEFAULT_TIMEOUT,
            _ => ACTION_RESPONSE_TIMEOUT,
        }
    }

    fn response_from_args(&self, args: &ActionResponseArgs) -> Result<ActionResponse, BoxError> {
        match self {
            Self::Approval {
                approved_payload,
                rememberable,
            } => {
                if !args.approve.ok_or("approve is required")? {
                    return Ok(ActionResponse::new(
                        ActionStatus::Denied,
                        json!({ "approve": false }),
                    ));
                }
                let mut payload = approved_payload.clone();
                payload["approve"] = true.into();
                // Only a card that offered it can be remembered.
                if *rememberable && args.remember == Some(true) {
                    payload["remember"] = true.into();
                }
                Ok(ActionResponse::new(ActionStatus::Approved, payload))
            }
            Self::Choice { choices, .. } => {
                let choice_id = args
                    .choice_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|choice_id| !choice_id.is_empty())
                    .ok_or("choice_id is required")?;
                select_choice(choices, choice_id, args.choice_text.as_deref())
            }
        }
    }

    /// What an action resolves to when nobody answers it: a choice with a
    /// default takes that default, everything else expires.
    fn unanswered(&self, reason: &str) -> ActionResponse {
        if let Self::Choice {
            choices,
            default_choice_id: Some(choice_id),
            ..
        } = self
            && let Ok(mut response) = select_choice(choices, choice_id, None)
        {
            response.payload["auto_selected"] = true.into();
            response.payload["reason"] = reason.into();
            return response;
        }
        ActionResponse::new(ActionStatus::Expired, json!({ "reason": reason }))
    }
}

fn select_choice(
    choices: &[UserChoiceOption],
    choice_id: &str,
    choice_text: Option<&str>,
) -> Result<ActionResponse, BoxError> {
    let Some(choice) = choices.iter().find(|choice| choice.id == choice_id) else {
        return Err("unknown choice_id".into());
    };
    let choice_text = choice_text
        .filter(|_| choice.input.is_some())
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if choice.input.as_ref().is_some_and(|input| input.required) && choice_text.is_none() {
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
    if let Some(choice_text) = choice_text {
        payload["choice_text"] = choice_text.into();
    }
    Ok(ActionResponse::new(ActionStatus::Selected, payload))
}

/// Maps a choice resolution to the tool result. An option picked, by the user
/// or by default, and a reply in chat are answers; anything else fails the call.
fn choice_result(response: ActionResponse) -> Result<Value, BoxError> {
    let answered_in_chat = response
        .payload
        .get(ANSWERED_IN_CHAT)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match response.status {
        ActionStatus::Selected => Ok(response.payload),
        ActionStatus::Expired if answered_in_chat => Ok(response.payload),
        ActionStatus::Expired => {
            let reason = response
                .payload
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("no response");
            Err(format!(
                "user choice got no answer ({reason}) and has no default_choice_id. \
                 Do not guess: tell the user which decision you need, then end your turn."
            )
            .into())
        }
        _ => Err(action_denied_error(&response.payload)),
    }
}

#[derive(Clone, Debug)]
struct ActionResponse {
    status: ActionStatus,
    payload: Value,
    responded_at: u64,
}

impl ActionResponse {
    fn new(status: ActionStatus, payload: Value) -> Self {
        Self {
            status,
            payload,
            responded_at: unix_ms(),
        }
    }

    fn event(&self, action_id: String) -> ActionEvent {
        ActionEvent::Resolve {
            action_id,
            status: self.status,
            response: self.payload.clone(),
            responded_at: self.responded_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub(crate) enum ActionsToolArgs {
    RespondAction(ActionResponseArgs),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ActionResponseArgs {
    pub(crate) action_id: String,
    #[serde(default)]
    pub(crate) approve: Option<bool>,
    #[serde(default)]
    pub(crate) choice_id: Option<String>,
    #[serde(default)]
    pub(crate) choice_text: Option<String>,
    /// With `approve: true`, on a card that offers it: stop asking. Left
    /// off the wire when unset, as older daemons never sent it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) remember: Option<bool>,
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
        let ActionsToolArgs::RespondAction(args) = args;
        let conversation = ctx
            .meta()
            .get_extra_as::<u64>(keys::CONVERSATION)
            .unwrap_or(0);
        let caller = ctx.caller().to_text();
        let output = self.runtime.respond(&caller, conversation, args).await?;
        Ok(ToolOutput::new(output))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct UserChoiceArgs {
    pub title: String,
    #[serde(default)]
    pub message: Option<String>,
    pub choices: Vec<UserChoiceOption>,
    /// The option taken when the user does not answer in time or cannot be
    /// asked; `None` makes the question blocking.
    #[serde(default)]
    pub default_choice_id: Option<String>,
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
        "Ask the user to choose one option from a small set of suggested next actions. Use this when user intent is ambiguous or confirmation should be collected with buttons instead of free-form text. A choice can include an input field when the selected option needs the user to type details. Set `default_choice_id` to the option you recommend whenever a reasonable default exists: if the user does not answer within a few minutes, or cannot be asked, it is selected for them, the result carries `auto_selected: true`, and you continue the task with it. If the result carries `answered_in_chat: true`, the user replied in chat instead: stop this step and follow their message.".to_string()
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
        Ok(ToolOutput::new(output))
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
            },
            "remember": {
                "type": ["boolean", "null"],
                "description": "With approve true on an approval card that has a remember_label: true also stops asking for this from now on. Null otherwise."
            }
        },
        "required": ["type", "action_id", "approve", "choice_id", "choice_text", "remember"],
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
            },
            "default_choice_id": {
                "type": ["string", "null"],
                "description": "Id of the choice you recommend. It is selected automatically when the user does not answer within a few minutes or cannot be asked (scheduled or unattended runs, IM chats). It cannot be a choice whose input is required. Use null only when continuing without the user's own decision would be unsafe or meaningless; the question then blocks until it expires."
            }
        },
        "required": ["title", "message", "choices", "default_choice_id"],
        "additionalProperties": false
    })
}

fn next_action_id() -> String {
    format!("act_{}", Xid::new())
}

/// What an MCP approval card asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum McpApprovalKind {
    /// Adding or connecting a server.
    Connect,
    /// Enabling, disabling, removing or signing out of one.
    Change,
    /// Creating or changing an automation that runs on a server's events.
    Automation,
}

/// The body of an MCP approval card.
pub(crate) struct McpApprovalCard {
    pub message: String,
    pub summary: String,
    pub details: Vec<ActionDetail>,
    pub metadata: Value,
}

/// Fail-closed approval gate for the MCP server tools: outside FullAccess mode
/// the user must confirm, and when no [`ActionSession`] is available in the
/// context (so no approval card can be shown) the call is rejected.
pub(crate) async fn require_mcp_approval(
    ctx: &BaseCtx,
    kind: McpApprovalKind,
    tool_name: &str,
    summary: String,
    details: Vec<ActionDetail>,
    metadata: Value,
) -> Result<(), BoxError> {
    let meta = live_request_meta(ctx);
    if ApprovalMode::from_ctx(ctx, &meta) == ApprovalMode::FullAccess {
        return Ok(());
    }
    if let Some(reason) = unattended_run_reason(ctx, &meta) {
        return Err(format!(
            "{tool_name} needs the user's approval, which nobody can give in this {reason}"
        )
        .into());
    }
    let Some(session) = ctx.get_state::<ActionSession>() else {
        let what = match kind {
            McpApprovalKind::Connect => "adding or connecting an MCP server",
            McpApprovalKind::Change => "changing an MCP server",
            McpApprovalKind::Automation => "creating or changing an MCP event automation",
        };
        return Err(format!(
            "{what} requires user approval, which is not available in this context"
        )
        .into());
    };
    let (title, message) = match kind {
        McpApprovalKind::Connect => (
            "Approve MCP server connection",
            "The agent wants to connect an MCP server, which can run a local program or reach a remote endpoint.",
        ),
        McpApprovalKind::Change => (
            "Approve MCP server change",
            "The agent wants to change an MCP server you configured.",
        ),
        McpApprovalKind::Automation => (
            "Approve MCP event automation",
            "The agent wants to run on its own whenever an MCP server reports an event. Event data comes from the server and is untrusted; those runs cannot use tools that need approval.",
        ),
    };
    session
        .request_mcp_approval(
            ctx,
            ActionToolRef::labeled(tool_name, "MCP server"),
            title.to_string(),
            McpApprovalCard {
                message: message.to_string(),
                summary,
                details,
                metadata,
            },
            None,
        )
        .await
        .map(|_| ())
}

/// Who answers an approval for the request being served.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ApprovalScope {
    /// The approval mode; unattended runs (cron, goals) have full access.
    pub mode: ApprovalMode,
    /// The request comes from an external IM user, who cannot approve on
    /// the owner's behalf.
    pub external_user: bool,
    /// Why nobody can answer an approval card here, when nobody can.
    pub unanswerable: Option<&'static str>,
}

pub(crate) fn approval_scope(ctx: &BaseCtx) -> ApprovalScope {
    let meta = live_request_meta(ctx);
    let mode = ApprovalMode::from_ctx(ctx, &meta);
    // Either the request or the session it joined: an IM thread can carry
    // the owner and external users alike.
    let external_user = [&meta, ctx.meta()]
        .iter()
        .any(|meta| meta.get_extra_as::<bool>(keys::EXTERNAL_USER) == Some(true));
    let unanswerable = if external_user {
        Some("request from an external user")
    } else {
        choice_unanswerable_reason(ctx, &meta).or_else(|| {
            ctx.get_state::<ActionSession>()
                .is_none()
                .then_some("context without approval cards")
        })
    };
    ApprovalScope {
        mode,
        external_user,
        unanswerable,
    }
}

/// Asks the user before the agent calls `tool_name`, a tool of an MCP
/// server. The caller decides that it must ask and checks with
/// [`approval_scope`] that someone can answer. `Ok(true)`: approved, and the
/// user chose to always allow the tool.
pub(crate) async fn request_mcp_tool_approval(
    ctx: &BaseCtx,
    tool_name: &str,
    title: String,
    card: McpApprovalCard,
) -> Result<bool, BoxError> {
    let session = ctx
        .get_state::<ActionSession>()
        .ok_or("calling this MCP tool requires user approval, which is not available here")?;
    session
        .request_mcp_approval(
            ctx,
            ActionToolRef::labeled(tool_name, "MCP tool"),
            title,
            card,
            Some("Always allow"),
        )
        .await
}

/// Asks the user to let the agent read from an MCP server's resources.
pub(crate) async fn request_mcp_resource_approval(
    ctx: &BaseCtx,
    title: String,
    card: McpApprovalCard,
) -> Result<(), BoxError> {
    let session = ctx
        .get_state::<ActionSession>()
        .ok_or("reading MCP resources requires user approval, which is not available here")?;
    session
        .request_mcp_approval(
            ctx,
            ActionToolRef::labeled("mcp_resources", "MCP resources"),
            title,
            card,
            None,
        )
        .await
        .map(|_| ())
}

/// Validates the choices and trims the ids the model wrote, so a response
/// (which is trimmed too) and the default match them exactly.
fn normalize_choice_args(args: &mut UserChoiceArgs) -> Result<(), BoxError> {
    if args.title.trim().is_empty() {
        return Err("title is required".into());
    }
    if args.choices.is_empty() || args.choices.len() > 6 {
        return Err("choices must contain 1 to 6 items".into());
    }
    let mut seen = HashSet::new();
    for choice in &mut args.choices {
        choice.id = choice.id.trim().to_string();
        if choice.id.is_empty() {
            return Err("choice id is required".into());
        }
        if choice.label.trim().is_empty() {
            return Err("choice label is required".into());
        }
        if !seen.insert(choice.id.clone()) {
            return Err("choice ids must be unique".into());
        }
        // Links are the runtime's: the model cannot make a card open one.
        choice.url = None;
    }
    args.default_choice_id = args
        .default_choice_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    if let Some(default_id) = &args.default_choice_id {
        let Some(choice) = args.choices.iter().find(|choice| &choice.id == default_id) else {
            return Err("default_choice_id must be one of the choice ids".into());
        };
        if choice.input.as_ref().is_some_and(|input| input.required) {
            return Err("default_choice_id cannot be a choice whose input is required".into());
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

    fn approval_mode(ctx: &BaseCtx) -> ApprovalMode {
        ApprovalMode::from_ctx(ctx, &live_request_meta(ctx))
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
        assert_eq!(approval_mode(&ctx), ApprovalMode::OnRisk);

        ctx.set_state(SessionRequestMeta::new(meta_with(&[(
            "approval_mode",
            json!("full_access"),
        )])));
        assert_eq!(approval_mode(&ctx), ApprovalMode::FullAccess);
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

        assert_eq!(approval_mode(&ctx), ApprovalMode::FullAccess);
    }

    #[tokio::test]
    async fn event_automation_runs_are_unattended_without_full_access() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        ctx.set_state(SessionRequestMeta::new(meta_with(&[(
            keys::MCP_TRIGGER_ID,
            json!(3u64),
        )])));
        // Untrusted events must not buy full access; asks fail at once.
        assert_eq!(approval_mode(&ctx), ApprovalMode::OnRisk);
        assert_eq!(
            approval_scope(&ctx).unanswerable,
            Some("MCP event automation")
        );

        let (event_sender, mut event_rx) = mpsc::channel(4);
        let session = ActionSession::new(
            Arc::new(ActionRuntime::new()),
            event_sender,
            ctx.caller().to_text(),
            "session_1".to_string(),
            Arc::new(AtomicU64::new(1)),
            Arc::new(Models::default()),
            std::env::temp_dir(),
        );
        let err = session
            .request_shell_approval(
                &ctx,
                CommandArgs {
                    command: "rm -rf target".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("NOT run"), "{err}");
        ctx.set_state(session);
        let err = require_mcp_approval(
            &ctx,
            McpApprovalKind::Change,
            "manage_mcp_server",
            "Remove MCP server x".to_string(),
            Vec::new(),
            json!({}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("nobody can give"), "{err}");
        assert!(event_rx.try_recv().is_err(), "no card was shown");
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
            Arc::new(AtomicU64::new(0)),
        ));
        assert_eq!(approval_mode(&ctx), ApprovalMode::RequestApproval);

        *goal.write() = Some(crate::engine::goal::GoalState::new("ship it".to_string()));
        assert_eq!(approval_mode(&ctx), ApprovalMode::FullAccess);

        // Completing the objective hands control back to the declared mode.
        *goal.write() = None;
        assert_eq!(approval_mode(&ctx), ApprovalMode::RequestApproval);
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
        let conversation_id = Arc::new(AtomicU64::new(42));
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
        let args = CommandArgs {
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
                    remember: None,
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
            McpApprovalKind::Connect,
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
        let conversation_id = Arc::new(AtomicU64::new(7));
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
                McpApprovalKind::Connect,
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
                    remember: None,
                },
            )
            .await
            .unwrap();
        assert!(request.await.unwrap().is_err());
    }

    fn option(id: &str, input: Option<UserChoiceInput>) -> UserChoiceOption {
        UserChoiceOption {
            id: id.to_string(),
            label: id.to_uppercase(),
            value: None,
            description: None,
            input,
            url: None,
        }
    }

    fn required_input() -> Option<UserChoiceInput> {
        Some(UserChoiceInput {
            placeholder: None,
            required: true,
            multiline: false,
        })
    }

    fn choice_args(choices: Vec<UserChoiceOption>, default: Option<&str>) -> UserChoiceArgs {
        UserChoiceArgs {
            title: "Pick".to_string(),
            message: None,
            choices,
            default_choice_id: default.map(str::to_string),
        }
    }

    #[test]
    fn choice_args_validate_ids_and_default() {
        let mut args = choice_args(vec![option(" a ", None)], Some(" a"));
        normalize_choice_args(&mut args).unwrap();
        assert_eq!(args.choices[0].id, "a");
        assert_eq!(args.default_choice_id.as_deref(), Some("a"));

        // A blank default from a strict-schema model means "no default".
        let mut args = choice_args(vec![option("a", None)], Some("  "));
        normalize_choice_args(&mut args).unwrap();
        assert_eq!(args.default_choice_id, None);

        let mut args = choice_args(vec![option("a", None)], Some("b"));
        assert!(normalize_choice_args(&mut args).is_err());

        // Nobody can type the required text on the user's behalf.
        let mut args = choice_args(vec![option("a", required_input())], Some("a"));
        assert!(normalize_choice_args(&mut args).is_err());

        let mut args = choice_args(vec![option("a", None), option(" a", None)], None);
        assert!(normalize_choice_args(&mut args).is_err());
    }

    #[test]
    fn unanswered_choice_takes_its_default_or_expires() {
        let with_default = PendingActionKind::Choice {
            choices: vec![option("a", None), option("b", None)],
            default_choice_id: Some("b".to_string()),
            limit: None,
        };
        assert_eq!(with_default.timeout(), CHOICE_DEFAULT_TIMEOUT);
        let response = with_default.unanswered("no response within 3 minutes");
        assert_eq!(response.status, ActionStatus::Selected);
        assert_eq!(response.payload["choice_id"], "b");
        assert_eq!(response.payload["value"], "B");
        assert_eq!(response.payload["auto_selected"], true);
        assert_eq!(response.payload["reason"], "no response within 3 minutes");
        assert_eq!(choice_result(response).unwrap()["choice_id"], "b");

        let blocking = PendingActionKind::Choice {
            choices: vec![option("a", None)],
            default_choice_id: None,
            limit: None,
        };
        assert_eq!(blocking.timeout(), ACTION_RESPONSE_TIMEOUT);
        let response = blocking.unanswered("no response within 10 minutes");
        assert_eq!(response.status, ActionStatus::Expired);
        let err = choice_result(response).unwrap_err().to_string();
        assert!(err.contains("no default_choice_id"), "{err}");

        // Approvals never pass by default.
        let approval = PendingActionKind::Approval {
            approved_payload: json!({"tool": "shell"}),
            rememberable: false,
        };
        assert_eq!(approval.unanswered("late").status, ActionStatus::Expired);
    }

    fn test_session(
        ctx: &BaseCtx,
    ) -> (
        Arc<ActionRuntime>,
        ActionSession,
        mpsc::Receiver<ActionEvent>,
    ) {
        let runtime = Arc::new(ActionRuntime::new());
        let (event_sender, event_rx) = mpsc::channel(4);
        let session = ActionSession::new(
            runtime.clone(),
            event_sender,
            ctx.caller().to_text(),
            "session_1".to_string(),
            Arc::new(AtomicU64::new(9)),
            Arc::new(Models::default()),
            std::env::temp_dir(),
        );
        (runtime, session, event_rx)
    }

    #[tokio::test]
    async fn choices_nobody_can_answer_resolve_without_a_card() {
        for (key, value) in [
            ("cron_job_id", json!(7u64)),
            ("reply_target", json!("chat_1")),
        ] {
            let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
            ctx.set_state(SessionRequestMeta::new(meta_with(&[(key, value)])));
            let (_runtime, session, mut event_rx) = test_session(&ctx);

            let output = session
                .request_choice(
                    &ctx,
                    choice_args(vec![option("a", None), option("b", None)], Some("a")),
                )
                .await
                .unwrap();
            assert_eq!(output["choice_id"], "a");
            assert_eq!(output["auto_selected"], true);

            let err = session
                .request_choice(&ctx, choice_args(vec![option("a", None)], None))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("nobody can answer"), "{err}");
            // No card was published for either question.
            assert!(event_rx.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn a_chat_reply_releases_a_waiting_choice_but_not_an_approval() {
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        let (runtime, session, mut event_rx) = test_session(&ctx);

        let choice_session = session.clone();
        let choice_ctx = ctx.clone();
        let choice = tokio::spawn(async move {
            choice_session
                .request_choice(
                    &choice_ctx,
                    choice_args(vec![option("a", None), option("b", None)], Some("a")),
                )
                .await
        });
        let Some(ActionEvent::Add(message)) = event_rx.recv().await else {
            panic!("expected choice action");
        };
        let Some(ContentPart::Action { payload, .. }) = message.content.first() else {
            panic!("expected action payload");
        };
        assert_eq!(payload["default_choice_id"], "a");
        let choice_id = payload["id"].as_str().unwrap().to_string();
        let (approval_tx, _approval_rx) = oneshot::channel();
        runtime.insert(PendingAction {
            session: "session_1".to_string(),
            action_id: "act_approval".to_string(),
            caller: "caller".to_string(),
            conversation: 9,
            kind: PendingActionKind::Approval {
                approved_payload: json!({}),
                rememberable: false,
            },
            event_sender: mpsc::channel(1).0,
            tx: approval_tx,
        });

        session.answer_choices_in_chat().await;

        let Some(ActionEvent::Resolve {
            action_id, status, ..
        }) = event_rx.recv().await
        else {
            panic!("expected resolve event");
        };
        assert_eq!(action_id, choice_id);
        assert_eq!(status, ActionStatus::Expired);
        let output = choice.await.unwrap().unwrap();
        assert_eq!(output[ANSWERED_IN_CHAT], true);
        let pending = runtime.pending.lock();
        assert_eq!(pending.len(), 1);
        assert!(pending.contains_key("act_approval"));
    }

    #[test]
    fn actions_tool_args_keep_the_respond_action_wire_shape() {
        let wire = json!({
            "type": "RespondAction",
            "action_id": "act_1",
            "approve": null,
            "choice_id": "a",
            "choice_text": null,
        });
        let args: ActionsToolArgs = serde_json::from_value(wire.clone()).unwrap();
        let ActionsToolArgs::RespondAction(response) = &args;
        assert_eq!(response.action_id, "act_1");
        assert_eq!(response.choice_id.as_deref(), Some("a"));
        assert_eq!(serde_json::to_value(&args).unwrap(), wire);
    }

    #[test]
    fn choice_response_returns_selected_value() {
        let kind = PendingActionKind::Choice {
            default_choice_id: None,
            limit: None,
            choices: vec![UserChoiceOption {
                id: "a".to_string(),
                label: "Option A".to_string(),
                value: Some("value-a".to_string()),
                description: None,
                input: None,
                url: None,
            }],
        };

        let response = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: Some("a".to_string()),
                choice_text: None,
                remember: None,
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
            default_choice_id: None,
            limit: None,
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
                url: None,
            }],
        };

        let response = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: Some("custom".to_string()),
                choice_text: Some("Please focus on the UI state.".to_string()),
                remember: None,
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
            default_choice_id: None,
            limit: None,
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
                url: None,
            }],
        };

        let err = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: Some("custom".to_string()),
                choice_text: Some("   ".to_string()),
                remember: None,
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
            rememberable: false,
        };

        let response = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: Some(true),
                choice_id: None,
                choice_text: None,
                remember: None,
            })
            .unwrap();

        assert_eq!(response.status, ActionStatus::Approved);
        assert_eq!(response.payload["approve"], true);
        assert_eq!(response.payload["tool"], "payments");
        assert_eq!(response.payload["payment_id"], "pay_1");
    }

    #[test]
    fn only_a_card_that_offers_it_remembers_an_approval() {
        let answer = |rememberable: bool, approve: bool| {
            PendingActionKind::Approval {
                approved_payload: json!({ "tool": "mcp_docs_search" }),
                rememberable,
            }
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: Some(approve),
                choice_id: None,
                choice_text: None,
                remember: Some(true),
            })
            .unwrap()
            .payload
        };
        assert_eq!(answer(true, true)["remember"], true);
        assert!(answer(false, true).get("remember").is_none());
        // Denying is never remembered.
        assert_eq!(answer(true, false), json!({ "approve": false }));
    }

    #[test]
    fn approval_response_requires_explicit_decision() {
        let kind = PendingActionKind::Approval {
            approved_payload: json!({"tool": "payments"}),
            rememberable: false,
        };

        let err = kind
            .response_from_args(&ActionResponseArgs {
                action_id: "act_1".to_string(),
                approve: None,
                choice_id: None,
                choice_text: None,
                remember: None,
            })
            .unwrap_err();

        assert_eq!(err.to_string(), "approve is required");
    }

    #[tokio::test]
    async fn invalid_action_response_keeps_pending_for_retry() {
        let runtime = ActionRuntime::new();
        let (event_sender, mut event_rx) = mpsc::channel(4);
        let action_id = "act_retry".to_string();
        let (tx, rx) = oneshot::channel();
        runtime.insert(PendingAction {
            session: "test".to_string(),
            action_id: action_id.clone(),
            caller: "caller".to_string(),
            conversation: 42,
            kind: PendingActionKind::Choice {
                default_choice_id: None,
                limit: None,
                choices: vec![UserChoiceOption {
                    id: "a".to_string(),
                    label: "Option A".to_string(),
                    value: None,
                    description: None,
                    input: None,
                    url: None,
                }],
            },
            event_sender,
            tx,
        });

        let err = runtime
            .respond(
                "caller",
                42,
                ActionResponseArgs {
                    action_id: action_id.clone(),
                    approve: None,
                    choice_id: Some("missing".to_string()),
                    choice_text: None,
                    remember: None,
                },
            )
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "unknown choice_id");
        assert!(runtime.pending.lock().contains_key(&action_id));

        let output = runtime
            .respond(
                "caller",
                42,
                ActionResponseArgs {
                    action_id: action_id.clone(),
                    approve: None,
                    choice_id: Some("a".to_string()),
                    choice_text: None,
                    remember: None,
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

    #[tokio::test]
    async fn cancelling_session_resolves_pending_approval_and_rejects_late_response() {
        let runtime = ActionRuntime::new();
        let (event_sender, _event_rx) = mpsc::channel(4);
        let (tx, rx) = oneshot::channel();
        runtime.insert(PendingAction {
            session: "cancelled-session".into(),
            action_id: "cancelled-action".into(),
            caller: "caller".into(),
            conversation: 1,
            kind: PendingActionKind::Approval {
                approved_payload: json!({}),
                rememberable: false,
            },
            event_sender,
            tx,
        });
        let events = runtime.cancel_session("cancelled-session");
        assert_eq!(events.len(), 1);
        assert_eq!(rx.await.unwrap().status, ActionStatus::Denied);
        assert!(
            runtime
                .respond(
                    "caller",
                    1,
                    ActionResponseArgs {
                        action_id: "cancelled-action".into(),
                        approve: Some(true),
                        choice_id: None,
                        choice_text: None,
                        remember: None,
                    }
                )
                .await
                .is_err()
        );
        assert!(runtime.pending.lock().is_empty());
    }
}
