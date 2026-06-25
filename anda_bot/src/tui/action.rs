use std::time::Duration;

use anda_core::{ContentPart, Message, ToolInput};
use ratatui::text::{Line, Span};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    text::{compact_cjk_spacing, truncate_visual},
    theme,
};

pub(super) const ACTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const ACTIONS_TOOL_NAME: &str = "actions_api";
const PENDING_STATUS: &str = "pending";
const MAX_CHOICE_KEYS: usize = 6;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TuiAction {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) kind: Option<String>,
    pub(super) status: String,
    pub(super) tool: Option<TuiActionTool>,
    pub(super) title: Option<String>,
    pub(super) message: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) details: Vec<TuiActionDetail>,
    pub(super) approval: Option<TuiActionApproval>,
    pub(super) command: Option<String>,
    pub(super) workspace: Option<String>,
    pub(super) background: Option<bool>,
    pub(super) choices: Vec<TuiActionChoice>,
    pub(super) response: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionTool {
    pub(super) name: String,
    pub(super) label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TuiActionDetail {
    pub(super) label: String,
    pub(super) value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionApproval {
    pub(super) approve_label: Option<String>,
    pub(super) deny_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionChoice {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) description: Option<String>,
    pub(super) input: Option<TuiActionChoiceInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionChoiceInput {
    pub(super) placeholder: Option<String>,
    pub(super) required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionChoiceDraft {
    pub(super) action_id: String,
    pub(super) choice_id: String,
    pub(super) label: String,
    pub(super) placeholder: Option<String>,
    pub(super) required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionState {
    pub(super) id: String,
    pub(super) status: String,
    pub(super) response: String,
}

impl TuiActionChoiceDraft {
    pub(super) fn placeholder(&self) -> String {
        self.placeholder
            .clone()
            .unwrap_or_else(|| format!("Type response for {}.", self.label))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct TuiActionApiOutput {
    #[serde(default)]
    pub(super) action_id: String,
    #[serde(default)]
    pub(super) conversation: u64,
    #[serde(default)]
    pub(super) status: String,
    #[serde(default)]
    pub(super) response: Value,
    #[serde(default)]
    pub(super) responded_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TuiActionResponseRequest {
    pub(super) action_id: String,
    pub(super) approve: Option<bool>,
    pub(super) choice_id: Option<String>,
    pub(super) choice_text: Option<String>,
}

impl TuiActionResponseRequest {
    pub(super) fn approve(action_id: String, approve: bool) -> Self {
        Self {
            action_id,
            approve: Some(approve),
            choice_id: None,
            choice_text: None,
        }
    }

    pub(super) fn choice(
        action_id: String,
        choice_id: String,
        choice_text: Option<String>,
    ) -> Self {
        Self {
            action_id,
            approve: None,
            choice_id: Some(choice_id),
            choice_text,
        }
    }

    pub(super) fn tool_input(&self) -> ToolInput<Value> {
        ToolInput::new(
            ACTIONS_TOOL_NAME.to_string(),
            json!({
                "type": "RespondAction",
                "action_id": self.action_id,
                "approve": self.approve,
                "choice_id": self.choice_id,
                "choice_text": self.choice_text,
            }),
        )
    }
}

impl TuiAction {
    pub(super) fn is_pending(&self) -> bool {
        self.status == PENDING_STATUS
    }

    pub(super) fn is_approval(&self) -> bool {
        matches!(
            self.kind.as_deref(),
            Some("tool_approval") | Some("shell_command")
        )
    }

    pub(super) fn display_title(&self) -> String {
        if self.is_shell_approval()
            && self
                .title
                .as_deref()
                .is_none_or(|title| title == "Approve shell command")
        {
            return "Approve shell command".to_string();
        }

        self.title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| self.kind_label())
    }

    pub(super) fn footer_text(&self) -> Option<String> {
        if !self.is_pending() {
            return None;
        }

        let title = self.display_title();
        if self.is_approval() {
            return Some(format!(
                "{title} · y {} · n {}",
                self.approve_label(),
                self.deny_label()
            ));
        }

        if self.choices.is_empty() {
            return None;
        }

        let choices = self
            .choices
            .iter()
            .take(MAX_CHOICE_KEYS)
            .enumerate()
            .map(|(index, choice)| format!("{} {}", index + 1, choice.label))
            .collect::<Vec<_>>()
            .join(" · ");
        Some(format!("{title} · {choices}"))
    }

    pub(super) fn choice_for_key(&self, key: char) -> Option<&TuiActionChoice> {
        let digit = key.to_digit(10)? as usize;
        if digit == 0 || digit > MAX_CHOICE_KEYS {
            return None;
        }
        self.choices.get(digit - 1)
    }

    pub(super) fn choice_draft(&self, choice: &TuiActionChoice) -> TuiActionChoiceDraft {
        TuiActionChoiceDraft {
            action_id: self.id.clone(),
            choice_id: choice.id.clone(),
            label: choice.label.clone(),
            placeholder: choice
                .input
                .as_ref()
                .and_then(|input| input.placeholder.clone()),
            required: choice
                .input
                .as_ref()
                .map(|input| input.required)
                .unwrap_or(false),
        }
    }

    fn kind_label(&self) -> String {
        if self.is_approval() {
            return format!("Approval: {}", self.tool_label());
        }
        if self.kind.as_deref() == Some("choice") {
            return "Choice".to_string();
        }
        self.kind.clone().unwrap_or_else(|| "Action".to_string())
    }

    fn tool_label(&self) -> String {
        if self.is_shell_approval() {
            return "Shell command".to_string();
        }
        self.tool
            .as_ref()
            .and_then(|tool| tool.label.clone().or_else(|| Some(tool.name.clone())))
            .unwrap_or_else(|| "tool".to_string())
    }

    fn is_shell_approval(&self) -> bool {
        let tool = self
            .tool
            .as_ref()
            .map(|tool| tool.name.to_ascii_lowercase())
            .unwrap_or_default();
        self.kind.as_deref() == Some("shell_command") || tool == "shell" || tool.contains("shell")
    }

    fn approve_label(&self) -> String {
        self.approval
            .as_ref()
            .and_then(|approval| approval.approve_label.clone())
            .filter(|label| label != "Approve")
            .unwrap_or_else(|| "Approve".to_string())
    }

    fn deny_label(&self) -> String {
        self.approval
            .as_ref()
            .and_then(|approval| approval.deny_label.clone())
            .filter(|label| label != "Deny")
            .unwrap_or_else(|| "Deny".to_string())
    }

    fn status_label(&self) -> String {
        match self.status.as_str() {
            "pending" => "pending".to_string(),
            "approved" => "approved".to_string(),
            "denied" => "denied".to_string(),
            "selected" => "selected".to_string(),
            "expired" => "expired".to_string(),
            other if !other.is_empty() => other.to_string(),
            _ => "unknown".to_string(),
        }
    }

    fn response_label(&self) -> Option<String> {
        if self.status == "selected" {
            let choice_id = self
                .response
                .as_ref()
                .and_then(|response| response.get("choice_id"))
                .and_then(Value::as_str)?;
            return Some(
                self.choices
                    .iter()
                    .find(|choice| choice.id == choice_id)
                    .map(|choice| choice.label.clone())
                    .unwrap_or_else(|| choice_id.to_string()),
            );
        }

        None
    }
}

pub(super) fn active_pending_action(messages: &[Message]) -> Option<TuiAction> {
    messages
        .iter()
        .rev()
        .flat_map(actions_from_message)
        .find(TuiAction::is_pending)
}

pub(super) fn actions_from_message(message: &Message) -> Vec<TuiAction> {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Action { name, payload, .. } => action_from_payload(name, payload),
            _ => None,
        })
        .collect()
}

pub(super) fn action_state_snapshot(messages: &[Message]) -> Vec<TuiActionState> {
    messages
        .iter()
        .flat_map(actions_from_message)
        .map(|action| {
            let response = action
                .response
                .as_ref()
                .map(Value::to_string)
                .unwrap_or_default();
            let status = action.status_label();
            TuiActionState {
                id: action.id,
                status,
                response: compact_cjk_spacing(&response).into_owned(),
            }
        })
        .collect()
}

pub(super) fn existing_action_state_changed(
    before: &[TuiActionState],
    after: &[TuiActionState],
) -> bool {
    before.iter().any(|before_action| {
        after
            .iter()
            .find(|after_action| after_action.id == before_action.id)
            .is_some_and(|after_action| after_action != before_action)
    })
}

pub(super) fn action_footer_line(action: &TuiAction, width: usize) -> Option<Line<'static>> {
    let text = action.footer_text()?;
    Some(Line::from(vec![
        Span::styled("ACTION ", theme::accent_style()),
        Span::styled(
            truncate_visual(&compact_cjk_spacing(&text), width.saturating_sub(7)),
            theme::subtle_style(),
        ),
    ]))
}

pub(super) fn action_transcript_text(action: &TuiAction) -> String {
    let mut lines = vec![format!(
        "⚡ {} · {}",
        action.display_title(),
        action.status_label()
    )];
    if let Some(response) = action.response_label() {
        lines[0].push_str(" · ");
        lines[0].push_str(&response);
    }

    if let Some(message) = action
        .message
        .as_deref()
        .filter(|message| !message.is_empty())
    {
        lines.push(message.to_string());
    }
    if let Some(summary) = action
        .summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
    {
        lines.push(summary.to_string());
    }

    if action.details.is_empty() {
        if let Some(command) = action.command.as_deref() {
            lines.push(format!("Command: {command}"));
        }
        if let Some(workspace) = action.workspace.as_deref() {
            let suffix = if action.background.unwrap_or(false) {
                " · background"
            } else {
                ""
            };
            lines.push(format!("Workspace: {workspace}{suffix}"));
        }
    } else {
        for detail in &action.details {
            lines.push(format!(
                "{}: {}",
                detail.label,
                detail_value_text(&detail.value)
            ));
        }
    }

    if action.is_pending() && action.is_approval() {
        lines.push(format!(
            "[y] {}  [n] {}",
            action.approve_label(),
            action.deny_label()
        ));
    } else if action.is_pending() && !action.choices.is_empty() {
        for (index, choice) in action.choices.iter().take(MAX_CHOICE_KEYS).enumerate() {
            let mut line = format!("[{}] {}", index + 1, choice.label);
            if let Some(description) = choice.description.as_deref() {
                line.push_str(" — ");
                line.push_str(description);
            }
            if choice.input.is_some() {
                line.push_str(" (text)");
            }
            lines.push(line);
        }
    }

    compact_cjk_spacing(&lines.join("\n")).into_owned()
}

pub(super) fn action_response_notice(output: &TuiActionApiOutput) -> String {
    let status = if output.status.is_empty() {
        "updated"
    } else {
        output.status.as_str()
    };
    format!("Action {} {status}.", output.action_id)
}

pub(super) fn apply_action_response_to_messages(
    messages: &mut [Message],
    output: &TuiActionApiOutput,
) -> bool {
    let mut updated = false;
    for message in messages {
        for part in &mut message.content {
            let ContentPart::Action { payload, .. } = part else {
                continue;
            };
            updated |= apply_action_response_to_payload(payload, output);
        }
    }
    updated
}

pub(super) fn apply_action_response_to_message_value(
    value: &mut Value,
    output: &TuiActionApiOutput,
) -> bool {
    let Some(parts) = value
        .get_mut("content")
        .and_then(|content| content.as_array_mut())
    else {
        return false;
    };

    let mut updated = false;
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("Action") {
            continue;
        }
        let Some(payload) = part.get_mut("payload") else {
            continue;
        };
        updated |= apply_action_response_to_payload(payload, output);
    }
    updated
}

fn apply_action_response_to_payload(payload: &mut Value, output: &TuiActionApiOutput) -> bool {
    if output.action_id.is_empty()
        || payload.get("id").and_then(Value::as_str) != Some(output.action_id.as_str())
    {
        return false;
    }
    let Some(object) = payload.as_object_mut() else {
        return false;
    };

    object.insert("status".to_string(), output.status.clone().into());
    object.insert("response".to_string(), output.response.clone());
    object.insert("responded_at".to_string(), output.responded_at.into());
    true
}

pub(super) fn action_from_payload(name: &str, payload: &Value) -> Option<TuiAction> {
    let id = string_field(payload, "id").unwrap_or_else(|| name.to_string());
    if id.trim().is_empty() {
        return None;
    }

    Some(TuiAction {
        id,
        name: name.to_string(),
        kind: string_field(payload, "kind"),
        status: string_field(payload, "status").unwrap_or_else(|| PENDING_STATUS.to_string()),
        tool: tool_from_value(payload.get("tool")),
        title: string_field(payload, "title"),
        message: string_field(payload, "message"),
        summary: string_field(payload, "summary"),
        details: details_from_value(payload.get("details")),
        approval: approval_from_value(payload.get("approval")),
        command: string_field(payload, "command")
            .or_else(|| nested_string_field(payload, "metadata", "command")),
        workspace: string_field(payload, "workspace")
            .or_else(|| nested_string_field(payload, "metadata", "workspace")),
        background: bool_field(payload, "background")
            .or_else(|| nested_bool_field(payload, "metadata", "background")),
        choices: choices_from_value(payload.get("choices")),
        response: payload.get("response").cloned(),
    })
}

fn tool_from_value(value: Option<&Value>) -> Option<TuiActionTool> {
    match value {
        Some(Value::String(name)) if !name.trim().is_empty() => Some(TuiActionTool {
            name: name.trim().to_string(),
            label: None,
        }),
        Some(Value::Object(object)) => {
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())?;
            Some(TuiActionTool {
                name: name.to_string(),
                label: object
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|label| !label.is_empty())
                    .map(str::to_string),
            })
        }
        _ => None,
    }
}

fn details_from_value(value: Option<&Value>) -> Vec<TuiActionDetail> {
    let Some(details) = value.and_then(Value::as_array) else {
        return Vec::new();
    };

    details
        .iter()
        .filter_map(|detail| {
            let object = detail.as_object()?;
            let label = object
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|label| !label.is_empty())?;
            Some(TuiActionDetail {
                label: label.to_string(),
                value: object.get("value").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

fn approval_from_value(value: Option<&Value>) -> Option<TuiActionApproval> {
    let object = value.and_then(Value::as_object)?;
    Some(TuiActionApproval {
        approve_label: object
            .get("approve_label")
            .or_else(|| object.get("approveLabel"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string),
        deny_label: object
            .get("deny_label")
            .or_else(|| object.get("denyLabel"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string),
    })
}

fn choices_from_value(value: Option<&Value>) -> Vec<TuiActionChoice> {
    let Some(choices) = value.and_then(Value::as_array) else {
        return Vec::new();
    };

    choices
        .iter()
        .filter_map(|choice| {
            let object = choice.as_object()?;
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?;
            let label = object
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|label| !label.is_empty())?;
            Some(TuiActionChoice {
                id: id.to_string(),
                label: label.to_string(),
                description: object
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|description| !description.is_empty())
                    .map(str::to_string),
                input: choice_input_from_value(object.get("input")),
            })
        })
        .collect()
}

fn choice_input_from_value(value: Option<&Value>) -> Option<TuiActionChoiceInput> {
    let object = value.and_then(Value::as_object)?;
    Some(TuiActionChoiceInput {
        placeholder: object
            .get("placeholder")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|placeholder| !placeholder.is_empty())
            .map(str::to_string),
        required: object
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn detail_value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        Value::Array(items)
            if items
                .iter()
                .all(|item| item.as_str().is_some_and(|text| !text.is_empty())) =>
        {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        }
        value => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn nested_string_field(value: &Value, object_key: &str, key: &str) -> Option<String> {
    value
        .get(object_key)
        .and_then(|object| string_field(object, key))
}

fn bool_field(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn nested_bool_field(value: &Value, object_key: &str, key: &str) -> Option<bool> {
    value
        .get(object_key)
        .and_then(|object| bool_field(object, key))
}
