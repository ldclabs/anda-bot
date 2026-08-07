//! Risk policy for local shell commands.
//!
//! Decides whether a command may run unattended ([`ApprovalDecision::Allow`])
//! or must be confirmed by the user ([`ApprovalDecision::Ask`] with a
//! user-facing, localized reason), given the command, the declared
//! [`ApprovalMode`], and the active workspace. Deterministic and table-driven,
//! except for the optional model-backed classification in
//! [`shell_approval_decision_with_model`] and the launcher UI language hint.

use anda_core::{BoxError, CompletionRequest, ContentPart, ModelEffort, RequestMeta};
use anda_engine::{extension::shell::ExecArgs, model::Models};
use rust_i18n::t;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

use crate::util::request_meta::keys;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ApprovalMode {
    RequestApproval,
    OnRisk,
    FullAccess,
    Custom,
}

impl ApprovalMode {
    pub(super) fn from_meta(meta: &RequestMeta) -> Self {
        match meta
            .get_extra_as::<String>(keys::APPROVAL_MODE)
            .unwrap_or_default()
            .as_str()
        {
            "request_approval" => Self::RequestApproval,
            keys::APPROVAL_MODE_FULL_ACCESS => Self::FullAccess,
            "custom" => Self::Custom,
            _ => Self::OnRisk,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::RequestApproval => "request_approval",
            Self::OnRisk => "on_risk",
            Self::FullAccess => keys::APPROVAL_MODE_FULL_ACCESS,
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ApprovalDecision {
    Allow,
    Ask(String),
}

pub(super) async fn shell_approval_decision_with_model(
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
                "Make the reason plain and non-technical, explaining the real-world risk without assuming the user understands shell commands."
            )
            .to_string(),
            content: vec![ContentPart::Text {
                text: request.to_string(),
            }],
            output_schema: Some(shell_risk_output_schema()),
            effort: Some(ModelEffort::Medium),
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

pub(super) fn shell_risk_language_hint(meta: &RequestMeta) -> Option<String> {
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

pub(super) fn launcher_ui_language_hint(home_dir: &Path) -> Option<String> {
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
        "pwd" | "ls" | "grep" | "cat" | "head" | "tail" | "wc" | "du" | "df" | "ps" | "which"
        | "type" | "uname" | "date" | "whoami" | "dir" | "findstr" | "where" | "hostname"
        | "ver" | "tasklist" | "systeminfo" | "get-childitem" | "gci" | "get-content" | "gc"
        | "select-string" | "get-location" | "gl" | "get-command" | "get-process" | "gps"
        | "get-service" | "get-item" | "gi" | "get-itemproperty" | "gp" | "measure-object" => true,
        "find" => !find_has_side_effect_args(command),
        "fd" => !fd_has_side_effect_args(command),
        "rg" => !rg_has_side_effect_args(command),
        "sed" => sed_invocation_is_read_only(command),
        "awk" => awk_invocation_is_read_only(command),
        _ => false,
    }
}

fn find_has_side_effect_args(command: &str) -> bool {
    shell_tokens(command).iter().any(|token| {
        matches!(
            token.as_str(),
            "-exec" | "-execdir" | "-ok" | "-okdir" | "-delete" | "-fls"
        ) || token.starts_with("-fprint")
    })
}

fn fd_has_side_effect_args(command: &str) -> bool {
    shell_tokens(command).iter().any(|token| {
        let attached_short_exec = token.strip_prefix('-').is_some_and(|short_options| {
            !short_options.starts_with('-')
                && short_options
                    .chars()
                    .any(|option| matches!(option, 'x' | 'X'))
        });
        attached_short_exec
            || matches!(token.as_str(), "--exec" | "--exec-batch")
            || token.starts_with("--exec=")
            || token.starts_with("--exec-batch=")
    })
}

fn rg_has_side_effect_args(command: &str) -> bool {
    shell_tokens(command)
        .iter()
        .any(|token| token == "--pre" || token.starts_with("--pre="))
}

fn awk_invocation_is_read_only(command: &str) -> bool {
    // Options that load code from files or edit in place make the program
    // uninspectable here; those must go through approval.
    if shell_tokens(command).iter().skip(1).any(|token| {
        token.starts_with("-f")
            || token.starts_with("--file")
            || token.starts_with("-i")
            || token.starts_with("--include")
            || token.starts_with("-l")
            || token.starts_with("--load")
            || token.starts_with("-E")
            || token.starts_with("--exec")
    }) {
        return false;
    }
    // `system()` escapes to a shell; `@load`/`@include` pull in external code.
    let compact: String = command
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    !(compact.contains("system(") || compact.contains("@load") || compact.contains("@include"))
}

fn sed_invocation_is_read_only(command: &str) -> bool {
    let tokens = shell_tokens(command);
    let mut scripts: Vec<String> = Vec::new();
    let mut saw_script_operand = false;
    let mut expect_expression = false;
    let mut expect_option_value = false;
    for token in tokens.iter().skip(1) {
        if expect_expression {
            scripts.push(token.clone());
            expect_expression = false;
            continue;
        }
        if expect_option_value {
            expect_option_value = false;
            continue;
        }
        if let Some(expr) = token.strip_prefix("--expression=") {
            scripts.push(expr.to_string());
            continue;
        }
        if token == "-e" || token == "--expression" {
            expect_expression = true;
            continue;
        }
        // In-place editing and script-from-file cannot be verified read-only.
        if token.starts_with("-i")
            || token.starts_with("--in-place")
            || token.starts_with("-f")
            || token.starts_with("--file")
        {
            return false;
        }
        if token.starts_with('-') && token.len() > 1 {
            if token == "-l" {
                expect_option_value = true;
            }
            continue;
        }
        if !saw_script_operand && scripts.is_empty() {
            scripts.push(token.clone());
            saw_script_operand = true;
        }
        // Remaining operands are input files, which are only read.
    }
    !scripts.is_empty() && scripts.iter().all(|script| sed_script_is_safe(script))
}

/// Whether a single sed script consists only of commands that cannot execute
/// programs (`e`, `s///e`) or write files (`w`, `W`, `s///w`).
fn sed_script_is_safe(script: &str) -> bool {
    let rest = sed_skip_addresses(script.trim());
    let mut chars = rest.chars();
    let Some(cmd) = chars.next() else {
        return false;
    };
    let tail = chars.as_str().trim();
    match cmd {
        'p' | 'P' | 'd' | 'D' | '=' | 'n' | 'N' | 'h' | 'H' | 'g' | 'G' | 'x' | 'z' | 'F' => {
            tail.is_empty()
        }
        'q' | 'Q' | 'l' => tail.chars().all(|ch| ch.is_ascii_digit()),
        's' | 'y' => sed_substitution_is_safe(rest),
        _ => false,
    }
}

fn sed_skip_addresses(script: &str) -> &str {
    let mut rest = script.trim_start();
    loop {
        rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix(|ch: char| {
            ch.is_ascii_digit() || matches!(ch, '$' | ',' | '~' | '+' | '!')
        }) {
            rest = stripped;
            continue;
        }
        if let Some(after) = rest.strip_prefix('/') {
            let Some(end) = find_unescaped(after, '/') else {
                return rest;
            };
            rest = &after[end + 1..];
            continue;
        }
        return rest;
    }
}

fn sed_substitution_is_safe(rest: &str) -> bool {
    let mut chars = rest.chars();
    let Some(cmd) = chars.next() else {
        return false;
    };
    let Some(delim) = chars.next() else {
        return false;
    };
    if delim.is_ascii_alphanumeric() || delim == '\\' {
        return false;
    }
    let after = chars.as_str();
    let Some(second) = find_unescaped(after, delim) else {
        return false;
    };
    let after = &after[second + delim.len_utf8()..];
    let Some(third) = find_unescaped(after, delim) else {
        return false;
    };
    let flags = after[third + delim.len_utf8()..].trim();
    if cmd == 'y' {
        return flags.is_empty();
    }
    flags
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, 'g' | 'i' | 'I' | 'm' | 'M' | 'p'))
}

fn find_unescaped(text: &str, needle: char) -> Option<usize> {
    let mut escaped = false;
    for (idx, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == needle {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::{AgentOutput, BoxPinFut};
    use anda_engine::model::{CompletionFeaturesDyn, Model};
    use std::sync::{Arc, Mutex as StdMutex};

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
    fn approval_mode_parses_every_declared_value() {
        for (declared, expected) in [
            ("request_approval", ApprovalMode::RequestApproval),
            ("on_risk", ApprovalMode::OnRisk),
            ("full_access", ApprovalMode::FullAccess),
            ("custom", ApprovalMode::Custom),
            ("nonsense", ApprovalMode::OnRisk),
        ] {
            let mode = ApprovalMode::from_meta(&meta_with(&[("approval_mode", json!(declared))]));
            assert_eq!(mode, expected, "declared mode {declared}");
            assert_eq!(mode.as_str(), expected.as_str());
        }
        assert_eq!(
            ApprovalMode::from_meta(&meta_with(&[])),
            ApprovalMode::OnRisk
        );
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
    fn shell_policy_never_auto_allows_side_effects_via_read_only_whitelist() {
        for command in [
            // find can execute commands or delete/write files
            "find . -exec rm {} +",
            "find . -execdir rm {} +",
            "find . -delete",
            "find . -ok rm {} +",
            "find . -okdir rm {} +",
            "find . -fprintf out.txt %p",
            "find . -fprint out.txt",
            "find . -fls out.txt",
            // fd/rg equivalents
            "fd -x rm",
            "fd -xrm",
            "fd --exec rm",
            "fd -X rm",
            "fd -Xrm",
            "fd -HIx rm",
            "fd --exec-batch rm",
            "rg --pre bash pattern",
            // awk can escape to a shell or load external code
            r#"awk 'BEGIN{system("rm -rf ~")}'"#,
            r#"awk 'BEGIN{system ("id")}'"#,
            r#"awk '@load "extension"' f"#,
            "awk -f prog.awk data.txt",
            "awk -i inplace '{print}' data.txt",
            // sed can execute (e) or write files (w, s///w), or edit in place
            "sed 'e ls' file",
            "sed '1e ls' file",
            "sed 'w out.txt' file",
            "sed 's/a/b/e' file",
            "sed 's/a/b/w out.txt' file",
            "sed --in-place 's/a/b/' file",
            "sed -i.bak 's/a/b/' file",
            "sed -f script.sed file",
            "sed -e 's/a/b/' -e 'w out.txt' file",
        ] {
            let args = ExecArgs {
                command: command.to_string(),
                ..Default::default()
            };
            assert!(
                matches!(
                    shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
                    ApprovalDecision::Ask(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn shell_policy_still_allows_harmless_find_sed_awk() {
        for command in [
            "find . -name file.rs -type f",
            "find . -newer ref.txt -print",
            "fd pattern src",
            "rg pattern src",
            "sed -n '5,10p' file.txt",
            "sed 's/foo/bar/g' file.txt",
            "sed -e 's/foo/bar/' -e '2d' file.txt",
            "sed 'y/abc/xyz/' file.txt",
            "awk '{print $1}' file.txt",
            "awk -F: '{print $1}' file.txt",
        ] {
            let args = ExecArgs {
                command: command.to_string(),
                ..Default::default()
            };
            assert_eq!(
                shell_approval_decision(&args, ApprovalMode::OnRisk, "/tmp/workspace"),
                ApprovalDecision::Allow,
                "{command}"
            );
        }
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
