use anda_core::{
    BoxError, CompletionRequest, ContentPart, FunctionDefinition, Message, ModelEffort,
    RequestMeta, Resource, StateFeatures, Tool, ToolOutput, Usage,
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
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub(crate) const ACTION_MESSAGE_NAME: &str = "$action";
pub(crate) const TOOL_APPROVAL_ACTION: &str = "anda.tool_approval";
pub(crate) const USER_CHOICE_ACTION: &str = "anda.user_choice";
const ACTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const APPROVAL_MODE_META_KEY: &str = "approval_mode";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApprovalMode {
    RequestApproval,
    OnRisk,
    FullAccess,
    Custom,
}

impl ApprovalMode {
    fn from_ctx(ctx: &BaseCtx) -> Self {
        match ctx
            .meta()
            .get_extra_as::<String>(APPROVAL_MODE_META_KEY)
            .unwrap_or_default()
            .as_str()
        {
            "request_approval" => Self::RequestApproval,
            "full_access" => Self::FullAccess,
            "custom" => Self::Custom,
            _ => Self::OnRisk,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ApprovalDecision {
    Allow,
    Ask(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionStatus {
    Pending,
    Approved,
    Denied,
    Selected,
    Expired,
}

impl ActionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Selected => "selected",
            Self::Expired => "expired",
        }
    }
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
            .get_extra_as::<String>("workspace")
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
        let payload = json!({
            "id": action_id,
            "kind": "tool_approval",
            "tool": {
                "name": ShellTool::NAME,
                "label": "Shell command"
            },
            "agent": &ctx.agent,
            "conversation": conversation,
            "session": self.session_id,
            "title": "Approve shell command",
            "message": "The agent wants to run a local shell command.",
            "summary": &args.command,
            "command": &args.command,
            "details": details,
            "approval": {
                "approve_label": "Approve",
                "deny_label": "Deny"
            },
            "metadata": {
                "command": &args.command,
                "env_keys": &args.env_keys,
                "background": args.background,
                "approval_mode": ctx
                    .meta()
                    .get_extra_as::<String>(APPROVAL_MODE_META_KEY)
                    .unwrap_or_else(|| "on_risk".to_string()),
                "approval_reason": &approval_reason,
            },
            "status": ActionStatus::Pending.as_str(),
            "created_at": now_ms,
            "expires_at": now_ms + ACTION_RESPONSE_TIMEOUT.as_millis() as u64,
        });
        let message = action_message(TOOL_APPROVAL_ACTION, payload);
        let rx = self
            .runtime
            .register(PendingAction {
                action_id: action_id.clone(),
                caller: self.caller.clone(),
                conversation,
                kind: PendingActionKind::Approval {
                    approved_payload: json!({
                        "tool": ShellTool::NAME,
                        "command": &args.command,
                    }),
                },
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
            return Err("failed to publish shell approval request".into());
        }

        match tokio::time::timeout(ACTION_RESPONSE_TIMEOUT, rx).await {
            Ok(Ok(response)) if response.status == ActionStatus::Approved => Ok(args),
            Ok(Ok(response)) => Err(action_denied_error(&response.payload)),
            Ok(Err(_)) => Err("shell command approval was cancelled".into()),
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
                Err("shell command approval timed out".into())
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
        let payload = json!({
            "id": action_id,
            "kind": "choice",
            "tool": AskUserChoiceTool::NAME,
            "agent": &ctx.agent,
            "conversation": conversation,
            "session": self.session_id,
            "title": args.title,
            "message": args.message,
            "choices": args.choices,
            "status": ActionStatus::Pending.as_str(),
            "created_at": now_ms,
            "expires_at": now_ms + ACTION_RESPONSE_TIMEOUT.as_millis() as u64,
        });
        let message = action_message(USER_CHOICE_ACTION, payload);
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

async fn shell_approval_decision_with_model(
    args: &ExecArgs,
    mode: ApprovalMode,
    workspace: &str,
    models: &Models,
    language_hint: Option<&str>,
) -> ApprovalDecision {
    match mode {
        ApprovalMode::FullAccess => return ApprovalDecision::Allow,
        ApprovalMode::RequestApproval => {
            return localize_shell_approval_decision(
                ApprovalDecision::Ask("approval mode requires confirmation".to_string()),
                language_hint,
            );
        }
        ApprovalMode::OnRisk | ApprovalMode::Custom => {}
    }

    let command = args.command.trim();
    if command.is_empty() {
        return localize_shell_approval_decision(
            ApprovalDecision::Ask("empty command".to_string()),
            language_hint,
        );
    }

    match shell_approval_decision(args, mode, workspace) {
        ApprovalDecision::Allow => return ApprovalDecision::Allow,
        ApprovalDecision::Ask(_) => {}
    }

    match model_shell_approval_decision(args, workspace, models, language_hint).await {
        Ok(decision) => localize_shell_approval_decision(decision, language_hint),
        Err(err) => {
            log::warn!(
                "Shell approval risk model unavailable or invalid; falling back to static policy: {err:?}"
            );
            localize_shell_approval_decision(
                shell_approval_decision(args, mode, workspace),
                language_hint,
            )
        }
    }
}

async fn model_shell_approval_decision(
    args: &ExecArgs,
    workspace: &str,
    models: &Models,
    language_hint: Option<&str>,
) -> Result<ApprovalDecision, BoxError> {
    let Some(model) = shell_risk_model(models) else {
        return Err("no model available for shell risk classification".into());
    };

    let request = json!({
        "command": args.command,
        "workspace": workspace,
        "background": args.background,
        "env_keys": args.env_keys,
        "user_language_hint": language_hint.unwrap_or("unknown"),
    });
    let output = model
        .completion(CompletionRequest {
            instructions: concat!(
                "You are a local shell command risk classifier for an AI agent approval system. ",
                "Decide whether the command can run without asking the user. ",
                "Return strict JSON only with fields `decision` and `reason`. ",
                "`decision` must be `allow` or `ask`. ",
                "Default to `allow` for ordinary local development work confined to the active workspace or common OS temporary directories, ",
                "including reading files, searching, editing or generating project files, formatting, running tests/builds, ",
                "writing caches or logs, and local git operations like add/commit/status/diff/log/show. ",
                "Creating, overwriting, or reading temporary files under paths like /tmp, /private/tmp, /var/tmp, or platform temp directories is low risk by itself; do not ask solely because a temp file is outside the workspace. ",
                "Use `ask` only for high-risk operations: destructive or hard-to-reverse deletes/overwrites, ",
                "git reset --hard/clean or history rewrites, publishing/pushing/uploading data, network downloads that execute code, ",
                "installing or changing global/system software, sudo/admin/system-service changes, broad permission changes, ",
                "touching credentials/secrets/keychains, non-temporary paths outside the workspace, or background/long-running processes. ",
                "Do not mark shell syntax like &&, pipes, or redirection as risky by itself; judge the actual operations. ",
                "The `reason` is shown directly to the user only when `decision` is `ask`; write it in the user's current conversation language or the supplied `user_language_hint`. ",
                "If `user_language_hint` starts with `zh` or says Chinese, write the reason in Simplified Chinese. ",
                "Make the reason plain and non-technical, explaining the real-world risk without assuming the user understands shell commands."
            )
            .to_string(),
            content: vec![ContentPart::Text {
                text: request.to_string(),
            }],
            output_schema: Some(shell_risk_output_schema()),
            effort: Some(ModelEffort::Low),
            ..Default::default()
        })
        .await?;

    parse_shell_risk_decision(&output.content)
}

fn shell_risk_model(models: &Models) -> Option<anda_engine::model::Model> {
    models
        .get("lite")
        .or_else(|| models.get("flash"))
        .or_else(|| models.get_model())
}

fn shell_risk_language_hint(meta: &RequestMeta) -> Option<String> {
    ["ui_language", "language", "locale", "lang"]
        .iter()
        .find_map(|key| meta.get_extra_as::<String>(key))
        .map(|hint| hint.trim().to_ascii_lowercase())
        .filter(|hint| !hint.is_empty())
        .map(|lang| {
            if lang.starts_with("zh") || lang.starts_with("cn") {
                "zh-Hans".to_string()
            } else {
                lang
            }
        })
}

fn launcher_ui_language_hint(home_dir: &Path) -> Option<String> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct LauncherUiSettings {
        language: String,
    }

    let content = std::fs::read_to_string(home_dir.join("launcher").join("ui.json")).ok()?;
    let settings = serde_json::from_str::<LauncherUiSettings>(&content).ok()?;
    let language = settings.language.trim();
    (!language.is_empty()).then(|| language.to_string())
}

fn localize_shell_approval_decision(
    decision: ApprovalDecision,
    language_hint: Option<&str>,
) -> ApprovalDecision {
    match decision {
        ApprovalDecision::Ask(reason) => {
            ApprovalDecision::Ask(localize_shell_approval_reason(&reason, language_hint))
        }
        ApprovalDecision::Allow => ApprovalDecision::Allow,
    }
}

fn localize_shell_approval_reason(reason: &str, language_hint: Option<&str>) -> String {
    let locale = language_hint.unwrap_or("en");
    match reason {
        "approval mode requires confirmation" => {
            t!("shell_approval.reason.approval_required", locale = locale).into_owned()
        }
        "empty command" => t!("shell_approval.reason.empty_command", locale = locale).into_owned(),
        "background command" => {
            t!("shell_approval.reason.background_command", locale = locale).into_owned()
        }
        "complex shell syntax" => t!(
            "shell_approval.reason.complex_shell_syntax",
            locale = locale
        )
        .into_owned(),
        "sensitive path or secret-like argument" => t!(
            "shell_approval.reason.sensitive_path_or_secret",
            locale = locale
        )
        .into_owned(),
        "path outside the active workspace" => t!(
            "shell_approval.reason.path_outside_workspace",
            locale = locale
        )
        .into_owned(),
        "unknown command" => {
            t!("shell_approval.reason.unknown_command", locale = locale).into_owned()
        }
        "network, write, or system-changing command" => t!(
            "shell_approval.reason.network_write_or_system_change",
            locale = locale
        )
        .into_owned(),
        "git command may change state or access the network" => t!(
            "shell_approval.reason.git_state_or_network",
            locale = locale
        )
        .into_owned(),
        "unclassified command" => t!(
            "shell_approval.reason.unclassified_command",
            locale = locale
        )
        .into_owned(),
        "model classified the command as risky" => t!(
            "shell_approval.reason.model_classified_risky",
            locale = locale
        )
        .into_owned(),
        _ => reason.to_string(),
    }
}

fn shell_risk_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "decision": {
                "type": "string",
                "enum": ["allow", "ask"]
            },
            "reason": {
                "type": "string"
            }
        },
        "required": ["decision", "reason"],
        "additionalProperties": false
    })
}

fn parse_shell_risk_decision(content: &str) -> Result<ApprovalDecision, BoxError> {
    let Some(json_text) = extract_json_object(content) else {
        return Err("shell risk model did not return JSON".into());
    };
    let value: Value = serde_json::from_str(json_text)?;
    let decision = value
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("model classified the command as risky");

    match decision.as_str() {
        "allow" => Ok(ApprovalDecision::Allow),
        "ask" => Ok(ApprovalDecision::Ask(reason.to_string())),
        _ => Err(format!("unknown shell risk decision: {decision}").into()),
    }
}

fn extract_json_object(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&trimmed[start..=end])
}

fn shell_approval_decision(
    args: &ExecArgs,
    mode: ApprovalMode,
    workspace: &str,
) -> ApprovalDecision {
    match mode {
        ApprovalMode::FullAccess => return ApprovalDecision::Allow,
        ApprovalMode::RequestApproval => {
            return ApprovalDecision::Ask("approval mode requires confirmation".to_string());
        }
        ApprovalMode::OnRisk | ApprovalMode::Custom => {}
    }

    if args.background {
        return ApprovalDecision::Ask("background command".to_string());
    }

    let command = args.command.trim();
    if command.is_empty() {
        return ApprovalDecision::Ask("empty command".to_string());
    }

    if has_risky_shell_syntax(command) {
        return ApprovalDecision::Ask("complex shell syntax".to_string());
    }
    if references_sensitive_path(command) {
        return ApprovalDecision::Ask("sensitive path or secret-like argument".to_string());
    }
    if references_external_path(command, workspace) {
        return ApprovalDecision::Ask("path outside the active workspace".to_string());
    }

    let Some(program) = shell_program(command) else {
        return ApprovalDecision::Ask("unknown command".to_string());
    };
    if is_network_or_write_program(&program) {
        return ApprovalDecision::Ask("network, write, or system-changing command".to_string());
    }
    if program == "git" {
        return git_approval_decision(command);
    }
    if is_read_only_program(&program, command) {
        return ApprovalDecision::Allow;
    }

    ApprovalDecision::Ask("unclassified command".to_string())
}

fn shell_program(command: &str) -> Option<String> {
    effective_program_from_tokens(&shell_tokens(command))
}

fn effective_program_from_tokens(tokens: &[String]) -> Option<String> {
    let first = normalize_program_token(tokens.first()?);
    match first.as_str() {
        "cmd" => {
            let command_index = tokens.iter().position(|token| {
                let token = token.to_ascii_lowercase();
                token == "/c" || token == "/k"
            })?;
            tokens.get(command_index + 1).and_then(|token| {
                shell_program(token).or_else(|| Some(normalize_program_token(token)))
            })
        }
        "powershell" | "pwsh" => powershell_command_token(tokens)
            .and_then(|token| shell_program(token).or_else(|| Some(normalize_program_token(token))))
            .or(Some(first)),
        _ => Some(first),
    }
}

fn powershell_command_token(tokens: &[String]) -> Option<&str> {
    tokens
        .iter()
        .position(|token| {
            let token = token.to_ascii_lowercase();
            token == "-command" || token == "-c"
        })
        .and_then(|index| tokens.get(index + 1))
        .map(String::as_str)
}

fn shell_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in command.chars() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }

        if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn normalize_program_token(token: &str) -> String {
    let token = trim_shell_token(token);
    let basename = token
        .rsplit(|ch| ['/', '\\'].contains(&ch))
        .next()
        .unwrap_or(token)
        .to_ascii_lowercase();
    for suffix in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(stripped) = basename.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    basename
}

fn has_risky_shell_syntax(command: &str) -> bool {
    [
        "&&", "||", "|", "&", ";", ">", "<", "`", "$(", "^", "\n", "\r",
    ]
    .iter()
    .any(|token| command.contains(token))
}

fn references_sensitive_path(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let normalized = normalize_path_separators(&lower);
    if normalized.contains("appdata") && !normalized.contains("/appdata/local/temp") {
        return true;
    }
    [
        ".env",
        ".ssh",
        ".gnupg",
        ".aws",
        ".kube",
        "id_rsa",
        "id_ed25519",
        "keychain",
        "secret",
        "token",
        "password",
        "credential",
        "programdata",
        "ntuser.dat",
        "consolehost_history.txt",
        "system32\\config",
        "system32/config",
        "dpapi",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn references_external_path(command: &str, workspace: &str) -> bool {
    let workspace = workspace.trim();
    for token in shell_tokens(command) {
        let token = trim_shell_token(&token);
        if is_windows_switch_token(token) {
            continue;
        }
        if token.chars().any(char::is_whitespace) && references_external_path(token, workspace) {
            return true;
        }
        let path = normalize_path_separators(token);
        if path == ".." || path.starts_with("../") || path.contains("/../") {
            return true;
        }
        if path.starts_with("~/") || path.starts_with("%") {
            return true;
        }
        if is_absolute_path(token) {
            if path_is_known_temp_path(token) {
                continue;
            }
            if workspace.is_empty() {
                return true;
            }
            if !path_is_within_workspace(token, workspace) {
                return true;
            }
        }
    }
    false
}

fn is_windows_switch_token(token: &str) -> bool {
    if !token.starts_with('/') || token.starts_with("//") {
        return false;
    }
    let switch = &token[1..];
    !switch.is_empty()
        && switch.len() <= 3
        && switch
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '?')
}

fn trim_shell_token(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        matches!(
            ch,
            '\'' | '"' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    })
}

fn normalize_path_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with('\\')
        || path.starts_with("//")
        || path.starts_with("\\\\")
        || path
            .as_bytes()
            .get(0..2)
            .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':')
}

fn path_is_within_workspace(path: &str, workspace: &str) -> bool {
    let path = normalize_path_for_compare(path);
    let workspace = normalize_path_for_compare(workspace);
    if path == workspace {
        return true;
    }
    path.strip_prefix(&workspace)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn path_is_known_temp_path(path: &str) -> bool {
    let path = normalize_path_for_compare(path);
    if path == "/tmp"
        || path.starts_with("/tmp/")
        || path == "/private/tmp"
        || path.starts_with("/private/tmp/")
        || path == "/var/tmp"
        || path.starts_with("/var/tmp/")
        || path == "/private/var/tmp"
        || path.starts_with("/private/var/tmp/")
    {
        return true;
    }

    let macos_var_folders = path.strip_prefix("/private").unwrap_or(path.as_str());
    if let Some(rest) = macos_var_folders.strip_prefix("/var/folders/") {
        let mut parts = rest.split('/');
        if parts.next().is_some()
            && parts.next().is_some()
            && parts
                .next()
                .is_some_and(|part| part.eq_ignore_ascii_case("t"))
        {
            return true;
        }
    }

    let windows_path = path.to_ascii_lowercase();
    let Some((_, suffix)) = windows_path.split_once(":/") else {
        return false;
    };
    suffix == "tmp"
        || suffix.starts_with("tmp/")
        || suffix == "temp"
        || suffix.starts_with("temp/")
        || suffix == "windows/temp"
        || suffix.starts_with("windows/temp/")
        || suffix.ends_with("/appdata/local/temp")
        || suffix.contains("/appdata/local/temp/")
}

fn normalize_path_for_compare(path: &str) -> String {
    let mut path = normalize_path_separators(path);
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    if path.starts_with("//")
        || path
            .as_bytes()
            .get(0..2)
            .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':')
    {
        path = path.to_ascii_lowercase();
    }
    path
}

fn is_network_or_write_program(program: &str) -> bool {
    matches!(
        program,
        "rm" | "rmdir"
            | "mv"
            | "cp"
            | "mkdir"
            | "touch"
            | "chmod"
            | "chown"
            | "sudo"
            | "kill"
            | "pkill"
            | "curl"
            | "wget"
            | "ssh"
            | "scp"
            | "rsync"
            | "brew"
            | "npm"
            | "pnpm"
            | "yarn"
            | "pip"
            | "pip3"
            | "uv"
            | "make"
            | "cargo"
            | "python"
            | "python3"
            | "node"
            | "del"
            | "erase"
            | "rd"
            | "ren"
            | "rename"
            | "move"
            | "copy"
            | "xcopy"
            | "robocopy"
            | "md"
            | "mklink"
            | "setx"
            | "attrib"
            | "icacls"
            | "takeown"
            | "taskkill"
            | "reg"
            | "net"
            | "netsh"
            | "sc"
            | "schtasks"
            | "winget"
            | "choco"
            | "scoop"
            | "msiexec"
            | "powershell"
            | "pwsh"
            | "remove-item"
            | "ri"
            | "set-content"
            | "new-item"
            | "copy-item"
            | "move-item"
            | "rename-item"
            | "invoke-webrequest"
            | "iwr"
            | "invoke-restmethod"
            | "irm"
            | "start-process"
            | "stop-process"
            | "restart-service"
            | "set-itemproperty"
            | "new-itemproperty"
            | "remove-itemproperty"
    )
}

fn git_approval_decision(command: &str) -> ApprovalDecision {
    let subcommand = shell_tokens(command).get(1).cloned().unwrap_or_default();
    if matches!(
        subcommand.as_str(),
        "status" | "diff" | "log" | "show" | "branch" | "rev-parse" | "ls-files"
    ) {
        ApprovalDecision::Allow
    } else {
        ApprovalDecision::Ask("git command may change state or access the network".to_string())
    }
}

fn is_read_only_program(program: &str, command: &str) -> bool {
    match program {
        "pwd" | "ls" | "find" | "fd" | "rg" | "grep" | "cat" | "head" | "tail" | "wc" | "du"
        | "df" | "ps" | "which" | "type" | "uname" | "date" | "whoami" | "dir" | "findstr"
        | "where" | "hostname" | "ver" | "tasklist" | "systeminfo" | "get-childitem" | "gci"
        | "get-content" | "gc" | "select-string" | "get-location" | "gl" | "get-command"
        | "get-process" | "gps" | "get-service" | "get-item" | "gi" | "get-itemproperty" | "gp"
        | "measure-object" => true,
        "sed" => !shell_tokens(command)
            .iter()
            .any(|token| token.starts_with("-i")),
        "awk" => true,
        _ => false,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ActionApiOutput {
    pub action_id: String,
    pub conversation: u64,
    pub status: String,
    pub response: Value,
    pub responded_at: u64,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct UserChoiceOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Option<UserChoiceInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct UserChoiceInput {
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiline: bool,
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

fn action_message(name: &str, payload: Value) -> Message {
    Message {
        role: "assistant".to_string(),
        content: vec![ContentPart::Action {
            name: name.to_string(),
            payload,
            recipients: None,
            signature: None,
        }],
        name: Some(ACTION_MESSAGE_NAME.to_string()),
        timestamp: Some(unix_ms()),
        ..Default::default()
    }
}

fn approval_detail(label: &str, value: impl Serialize, format: &str) -> Value {
    json!({
        "label": label,
        "value": value,
        "format": format,
    })
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

pub(crate) fn is_action_message_value(value: &Value) -> bool {
    value
        .get("name")
        .and_then(|name| name.as_str())
        .is_some_and(|name| name == ACTION_MESSAGE_NAME)
}

pub(crate) fn is_action_message(message: &Message) -> bool {
    message
        .name
        .as_deref()
        .is_some_and(|name| name == ACTION_MESSAGE_NAME)
}

pub(crate) fn action_id_from_message(message: &Message) -> Option<String> {
    message.content.iter().find_map(|part| match part {
        ContentPart::Action { payload, .. } => payload
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_string),
        _ => None,
    })
}

pub(crate) fn action_id_from_message_value(value: &Value) -> Option<String> {
    value
        .get("content")
        .and_then(|content| content.as_array())
        .and_then(|parts| {
            parts.iter().find_map(|part| {
                if part.get("type").and_then(|value| value.as_str()) != Some("Action") {
                    return None;
                }
                part.get("payload")
                    .and_then(|payload| payload.get("id"))
                    .and_then(|id| id.as_str())
                    .map(str::to_string)
            })
        })
}

pub(crate) fn apply_action_resolution_to_chat_message(
    message: &mut Message,
    action_id: &str,
    status: ActionStatus,
    response: &Value,
    responded_at: u64,
) -> bool {
    let mut updated = false;
    for part in &mut message.content {
        let ContentPart::Action { payload, .. } = part else {
            continue;
        };
        let Some(object) = payload.as_object_mut() else {
            continue;
        };
        if object.get("id").and_then(|id| id.as_str()) != Some(action_id) {
            continue;
        }
        object.insert("status".to_string(), status.as_str().into());
        object.insert("response".to_string(), response.clone());
        object.insert("responded_at".to_string(), responded_at.into());
        updated = true;
    }
    updated
}

pub(crate) fn apply_action_resolution_to_message(
    value: &mut Value,
    action_id: &str,
    status: ActionStatus,
    response: &Value,
    responded_at: u64,
) -> bool {
    let Some(parts) = value
        .get_mut("content")
        .and_then(|content| content.as_array_mut())
    else {
        return false;
    };
    let mut updated = false;
    for part in parts {
        if part.get("type").and_then(|value| value.as_str()) != Some("Action") {
            continue;
        }
        let Some(payload) = part
            .get_mut("payload")
            .and_then(|payload| payload.as_object_mut())
        else {
            continue;
        };
        if payload.get("id").and_then(|id| id.as_str()) != Some(action_id) {
            continue;
        }
        payload.insert("status".to_string(), status.as_str().into());
        payload.insert("response".to_string(), response.clone());
        payload.insert("responded_at".to_string(), responded_at.into());
        updated = true;
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::{AgentOutput, BoxPinFut};
    use anda_engine::model::{CompletionFeaturesDyn, Model};
    use std::sync::Mutex as StdMutex;

    struct RecordingCompleter {
        requests: Arc<StdMutex<Vec<CompletionRequest>>>,
        response: String,
        name: &'static str,
    }

    impl CompletionFeaturesDyn for RecordingCompleter {
        fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            self.requests.lock().unwrap().push(req);
            let content = self.response.clone();
            Box::pin(async move {
                Ok(AgentOutput {
                    content,
                    ..Default::default()
                })
            })
        }

        fn model_name(&self) -> String {
            self.name.to_string()
        }
    }

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
            "该命令可能访问网络、写入文件或更改系统状态。"
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

    #[tokio::test]
    async fn shell_policy_uses_lite_model_for_complex_shell_syntax() {
        let lite_requests = Arc::new(StdMutex::new(Vec::new()));
        let flash_requests = Arc::new(StdMutex::new(Vec::new()));
        let models = Models::default();
        models.set(
            "flash".to_string(),
            Model::with_completer(Arc::new(RecordingCompleter {
                requests: flash_requests.clone(),
                response: r#"{"decision":"ask","reason":"flash fallback"}"#.to_string(),
                name: "flash-recorder",
            })),
        );
        models.set(
            "lite".to_string(),
            Model::with_completer(Arc::new(RecordingCompleter {
                requests: lite_requests.clone(),
                response: r#"{"decision":"allow","reason":"read-only inspection"}"#.to_string(),
                name: "lite-recorder",
            })),
        );
        let args = ExecArgs {
            command: "pwd && rg approval anda_bot/src".to_string(),
            ..Default::default()
        };

        let decision = shell_approval_decision_with_model(
            &args,
            ApprovalMode::OnRisk,
            "/tmp/workspace",
            &models,
            Some("zh-CN"),
        )
        .await;

        assert_eq!(decision, ApprovalDecision::Allow);
        let lite_requests = lite_requests.lock().unwrap();
        assert_eq!(lite_requests.len(), 1);
        assert_eq!(flash_requests.lock().unwrap().len(), 0);
        assert!(
            lite_requests[0]
                .instructions
                .contains("Do not mark shell syntax")
        );
        assert!(
            lite_requests[0]
                .instructions
                .contains("ordinary local development work")
        );
        assert!(
            lite_requests[0]
                .instructions
                .contains("common OS temporary directories")
        );
        assert!(
            lite_requests[0]
                .instructions
                .contains("current conversation language")
        );
        match &lite_requests[0].content[0] {
            ContentPart::Text { text } => {
                assert!(text.contains("&&"));
                assert!(text.contains(r#""user_language_hint":"zh-CN""#));
            }
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn shell_policy_uses_plain_localized_model_reason_for_high_risk_decision() {
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let models = Models::default();
        models.set(
            "lite".to_string(),
            Model::with_completer(Arc::new(RecordingCompleter {
                requests,
                response:
                    r#"{"decision":"ask","reason":"这个命令会删除项目文件，删除后可能很难恢复。"}"#
                        .to_string(),
                name: "lite-recorder",
            })),
        );
        let args = ExecArgs {
            command: "rm -rf anda_bot/src/engine".to_string(),
            ..Default::default()
        };

        assert_eq!(
            shell_approval_decision_with_model(
                &args,
                ApprovalMode::OnRisk,
                "/tmp/workspace",
                &models,
                Some("zh-CN")
            )
            .await,
            ApprovalDecision::Ask("这个命令会删除项目文件，删除后可能很难恢复。".to_string())
        );
    }

    #[tokio::test]
    async fn shell_policy_allows_ordinary_workspace_writes_when_model_allows() {
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let models = Models::default();
        models.set(
            "lite".to_string(),
            Model::with_completer(Arc::new(RecordingCompleter {
                requests,
                response: r#"{"decision":"allow","reason":"ordinary workspace write"}"#.to_string(),
                name: "lite-recorder",
            })),
        );
        let args = ExecArgs {
            command: "git add anda_bot/src/engine/action.rs".to_string(),
            ..Default::default()
        };

        assert_eq!(
            shell_approval_decision_with_model(
                &args,
                ApprovalMode::OnRisk,
                "/tmp/workspace",
                &models,
                None
            )
            .await,
            ApprovalDecision::Allow
        );
    }

    #[tokio::test]
    async fn shell_policy_falls_back_to_static_rules_when_model_output_is_invalid() {
        let models = Models::default();
        models.set(
            "lite".to_string(),
            Model::with_completer(Arc::new(RecordingCompleter {
                requests: Arc::new(StdMutex::new(Vec::new())),
                response: "not json".to_string(),
                name: "lite-recorder",
            })),
        );
        let args = ExecArgs {
            command: "pwd && rg approval anda_bot/src".to_string(),
            ..Default::default()
        };

        assert_eq!(
            shell_approval_decision_with_model(
                &args,
                ApprovalMode::OnRisk,
                "/tmp/workspace",
                &models,
                None
            )
            .await,
            ApprovalDecision::Ask(
                "This command uses complex shell syntax, so you need to confirm what will run."
                    .to_string()
            )
        );
    }

    #[test]
    fn shell_policy_allows_low_risk_read_commands() {
        let args = ExecArgs {
            command: "rg approval anda_bot/src".to_string(),
            ..Default::default()
        };
        assert_eq!(
            shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
            ApprovalDecision::Allow
        );

        let args = ExecArgs {
            command: "git diff --stat".to_string(),
            ..Default::default()
        };
        assert_eq!(
            shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
            ApprovalDecision::Allow
        );

        for command in [
            r"dir C:\workspace",
            r"cmd.exe /C type C:\workspace\README.md",
            r#"powershell -NoProfile -Command "Get-ChildItem C:\workspace""#,
            r#"pwsh -Command "Select-String TODO C:\workspace\README.md""#,
        ] {
            let args = ExecArgs {
                command: command.to_string(),
                ..Default::default()
            };
            assert_eq!(
                shell_approval_decision(&args, ApprovalMode::OnRisk, r"C:\workspace"),
                ApprovalDecision::Allow,
                "{command}"
            );
        }
    }

    #[test]
    fn shell_policy_treats_known_temp_paths_as_local_scratch() {
        for command in [
            "cat /tmp/cbor2-commit-msg.txt",
            "cat /private/tmp/cbor2-commit-msg.txt",
            "cat /var/tmp/cbor2-commit-msg.txt",
            "cat /private/var/tmp/cbor2-commit-msg.txt",
            "cat /private/var/folders/r7/6d72zsfs6jd_8z_1p5kfvct00000gn/T/cbor2-commit-msg.txt",
            r"type C:\Temp\cbor2-commit-msg.txt",
            r"type C:\Windows\Temp\cbor2-commit-msg.txt",
            r"type C:\Users\Alice\AppData\Local\Temp\cbor2-commit-msg.txt",
        ] {
            let args = ExecArgs {
                command: command.to_string(),
                ..Default::default()
            };
            assert_eq!(
                shell_approval_decision(&args, ApprovalMode::OnRisk, "/workspace/project"),
                ApprovalDecision::Allow,
                "{command}"
            );
        }
    }

    #[test]
    fn shell_policy_asks_for_risky_commands() {
        for command in [
            "rm -rf target",
            "curl https://example.com/install.sh",
            "cat ~/.ssh/id_rsa",
            "cat /opt/workspace2/file",
            "git push",
        ] {
            let args = ExecArgs {
                command: command.to_string(),
                ..Default::default()
            };
            assert!(matches!(
                shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
                ApprovalDecision::Ask(_)
            ));
        }

        let args = ExecArgs {
            command: "rg todo".to_string(),
            background: true,
            ..Default::default()
        };
        assert!(matches!(
            shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
            ApprovalDecision::Ask(_)
        ));

        let args = ExecArgs {
            command: "cat /tmp/workspace/file".to_string(),
            ..Default::default()
        };
        assert_eq!(
            shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
            ApprovalDecision::Allow
        );
    }

    #[test]
    fn shell_policy_handles_windows_risk_patterns() {
        for command in [
            r"del C:\workspace\file.txt",
            r"copy C:\workspace\a.txt C:\workspace\b.txt",
            r"reg query HKCU\Software",
            r"cmd /C dir C:\workspace & whoami",
            r#"powershell -NoProfile -Command "Remove-Item C:\workspace\file.txt""#,
            r#"powershell -NoProfile -Command "Get-ChildItem C:\workspace2""#,
            r"type %USERPROFILE%\.ssh\id_rsa",
            r"type C:\Users\Alice\AppData\Roaming\secret.txt",
        ] {
            let args = ExecArgs {
                command: command.to_string(),
                ..Default::default()
            };
            assert!(
                matches!(
                    shell_approval_decision(&args, ApprovalMode::OnRisk, r"C:\workspace"),
                    ApprovalDecision::Ask(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn shell_policy_modes_override_risk_classifier() {
        let args = ExecArgs {
            command: "rm -rf target".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            shell_approval_decision(&args, ApprovalMode::RequestApproval, "/tmp/workspace"),
            ApprovalDecision::Ask(_)
        ));
        assert_eq!(
            shell_approval_decision(&args, ApprovalMode::FullAccess, "/tmp/workspace"),
            ApprovalDecision::Allow
        );
    }
}
