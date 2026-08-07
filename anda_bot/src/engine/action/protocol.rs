//! Wire shape of `$action` cards.
//!
//! The engine is the only producer — [`ActionPayload`] pins the schema — while
//! the TUI, the gateway chat view, and the Chrome extension render or merge
//! the payloads, and stored conversations replay them verbatim. Evolve
//! additively: add optional fields, never rename or retype existing ones, and
//! route resolved-state updates through [`update_action_payload_resolution`]
//! so every consumer sees the same `status`/`response`/`responded_at` triple.
//!
//! Consumers must stay tolerant of payloads written by other versions, so the
//! reader-side helpers here operate on raw [`Value`]s (missing `status` counts
//! as pending) instead of requiring a full typed parse.

use anda_core::{ContentPart, Message};
use anda_engine::unix_ms;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const ACTION_MESSAGE_NAME: &str = "$action";
pub(crate) const TOOL_APPROVAL_ACTION: &str = "anda.tool_approval";
pub(crate) const USER_CHOICE_ACTION: &str = "anda.user_choice";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionStatus {
    #[default]
    Pending,
    Approved,
    Denied,
    Selected,
    Expired,
}

impl ActionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Selected => "selected",
            Self::Expired => "expired",
        }
    }
}

/// `tool` is an object for approvals and a bare tool name for choices; both
/// forms are live on the wire.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum ActionToolRef {
    Labeled {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Name(String),
}

impl ActionToolRef {
    pub(crate) fn labeled(name: &str, label: &str) -> Self {
        Self::Labeled {
            name: name.to_string(),
            label: Some(label.to_string()),
        }
    }
}

/// One row in an approval card's detail table. `value` stays free-form: text
/// details carry a string, list details carry an array of strings.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub(crate) struct ActionDetail {
    pub label: String,
    pub value: Value,
    pub format: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ApprovalLabels {
    #[serde(alias = "approveLabel")]
    pub approve_label: String,
    #[serde(alias = "denyLabel")]
    pub deny_label: String,
}

impl ApprovalLabels {
    pub(crate) fn approve_deny() -> Self {
        Self {
            approve_label: "Approve".to_string(),
            deny_label: "Deny".to_string(),
        }
    }
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

/// The `$action` card payload. One struct covers both kinds — approvals fill
/// the `summary`/`details`/`approval` group, choices fill `choices` — and the
/// serialization rules reproduce the historical wire exactly: `message` is
/// always present (`null` for a choice without one) while the other optional
/// fields are omitted when absent.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ActionPayload {
    pub id: String,
    /// `"tool_approval"` or `"choice"`; kept as a string so consumers can
    /// tolerate kinds introduced by other versions.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ActionToolRef>,
    pub agent: String,
    pub conversation: u64,
    pub session: String,
    pub title: String,
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<ActionDetail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalLabels>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<UserChoiceOption>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub status: ActionStatus,
    pub created_at: u64,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responded_at: Option<u64>,
}

impl ActionPayload {
    pub(crate) fn into_value(self) -> Value {
        serde_json::to_value(self).expect("ActionPayload serializes to JSON")
    }
}

/// Result of responding to an action through the `actions_api` tool. The TUI
/// deserializes this from the daemon, so fields stay individually defaulted
/// for cross-version tolerance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ActionApiOutput {
    pub action_id: String,
    pub conversation: u64,
    pub status: String,
    pub response: Value,
    pub responded_at: u64,
}

impl Default for ActionApiOutput {
    fn default() -> Self {
        Self {
            action_id: String::new(),
            conversation: 0,
            status: String::new(),
            response: Value::Null,
            responded_at: 0,
        }
    }
}

pub(crate) fn action_message(name: &str, payload: Value) -> Message {
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

pub(crate) fn approval_detail(label: &str, value: impl Serialize, format: &str) -> ActionDetail {
    ActionDetail {
        label: label.to_string(),
        value: serde_json::to_value(value).expect("approval detail value serializes"),
        format: format.to_string(),
    }
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
        ContentPart::Action { payload, .. } => payload_action_id(payload).map(str::to_string),
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
                    .and_then(payload_action_id)
                    .map(str::to_string)
            })
        })
}

/// Reads the action id from a raw payload.
pub(crate) fn payload_action_id(payload: &Value) -> Option<&str> {
    payload.get("id").and_then(Value::as_str)
}

/// Whether a raw payload is still awaiting a response. A missing `status`
/// counts as pending: older payloads predate the field.
pub(crate) fn payload_is_pending(payload: &Value) -> bool {
    payload
        .get("status")
        .and_then(Value::as_str)
        .is_none_or(|status| status == ActionStatus::Pending.as_str())
}

/// Reads the resolution timestamp from a raw payload, if resolved.
pub(crate) fn payload_responded_at(payload: &Value) -> Option<u64> {
    payload.get("responded_at").and_then(Value::as_u64)
}

/// Sets the resolution triple on a raw payload when its id matches. All
/// resolution writers (engine, TUI) go through here so the triple stays one
/// vocabulary.
pub(crate) fn update_action_payload_resolution(
    payload: &mut Value,
    action_id: &str,
    status: &str,
    response: &Value,
    responded_at: u64,
) -> bool {
    let Some(object) = payload.as_object_mut() else {
        return false;
    };
    if object.get("id").and_then(Value::as_str) != Some(action_id) {
        return false;
    }
    object.insert("status".to_string(), status.into());
    object.insert("response".to_string(), response.clone());
    object.insert("responded_at".to_string(), responded_at.into());
    true
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
        updated |= update_action_payload_resolution(
            payload,
            action_id,
            status.as_str(),
            response,
            responded_at,
        );
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
        let Some(payload) = part.get_mut("payload") else {
            continue;
        };
        updated |= update_action_payload_resolution(
            payload,
            action_id,
            status.as_str(),
            response,
            responded_at,
        );
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn action_payload_serializes_approval_and_choice_wire_shapes() {
        let approval = ActionPayload {
            id: "act_1".to_string(),
            kind: "tool_approval".to_string(),
            tool: Some(ActionToolRef::labeled("shell", "Shell command")),
            agent: "anda".to_string(),
            conversation: 7,
            session: "s1".to_string(),
            title: "Approve shell command".to_string(),
            message: Some("The agent wants to run a local shell command.".to_string()),
            summary: Some("ls".to_string()),
            command: Some("ls".to_string()),
            details: Some(vec![approval_detail("Workspace", "/tmp", "text")]),
            approval: Some(ApprovalLabels::approve_deny()),
            metadata: Some(json!({"command": "ls"})),
            status: ActionStatus::Pending,
            created_at: 1,
            expires_at: 2,
            ..Default::default()
        }
        .into_value();

        assert_eq!(approval["kind"], "tool_approval");
        assert_eq!(approval["tool"]["name"], "shell");
        assert_eq!(approval["tool"]["label"], "Shell command");
        assert_eq!(approval["approval"]["approve_label"], "Approve");
        assert_eq!(approval["details"][0]["label"], "Workspace");
        assert_eq!(approval["details"][0]["format"], "text");
        assert_eq!(approval["status"], "pending");
        // Absent optional groups stay off the wire; a resolution triple is
        // only added by `update_action_payload_resolution`.
        let object = approval.as_object().unwrap();
        assert!(!object.contains_key("choices"));
        assert!(!object.contains_key("response"));
        assert!(!object.contains_key("responded_at"));

        let choice = ActionPayload {
            id: "act_2".to_string(),
            kind: "choice".to_string(),
            tool: Some(ActionToolRef::Name("ask_user_choice".to_string())),
            agent: "anda".to_string(),
            conversation: 7,
            session: "s1".to_string(),
            title: "Pick".to_string(),
            message: None,
            choices: Some(vec![UserChoiceOption {
                id: "a".to_string(),
                label: "A".to_string(),
                value: None,
                description: None,
                input: None,
            }]),
            status: ActionStatus::Pending,
            created_at: 1,
            expires_at: 2,
            ..Default::default()
        }
        .into_value();

        assert_eq!(choice["tool"], "ask_user_choice");
        // A choice without a message keeps the key on the wire as null.
        let object = choice.as_object().unwrap();
        assert!(object.contains_key("message"));
        assert!(object["message"].is_null());
        assert!(!object.contains_key("summary"));
        assert!(!object.contains_key("approval"));
        assert_eq!(choice["choices"][0]["label"], "A");
    }

    #[test]
    fn action_payload_round_trips_and_tolerates_foreign_shapes() {
        let value = json!({
            "id": "act_9",
            "kind": "tool_approval",
            "tool": "shell",
            "approval": {"approveLabel": "Yes", "denyLabel": "No"},
            "status": "approved",
            "responded_at": 42,
        });
        let payload: ActionPayload = serde_json::from_value(value).unwrap();
        assert_eq!(payload.id, "act_9");
        assert!(matches!(payload.tool, Some(ActionToolRef::Name(ref name)) if name == "shell"));
        let approval = payload.approval.unwrap();
        assert_eq!(approval.approve_label, "Yes");
        assert_eq!(approval.deny_label, "No");
        assert_eq!(payload.status, ActionStatus::Approved);
        assert_eq!(payload.responded_at, Some(42));
    }

    #[test]
    fn payload_readers_treat_missing_status_as_pending() {
        let bare = json!({"id": "act_1"});
        assert_eq!(payload_action_id(&bare), Some("act_1"));
        assert!(payload_is_pending(&bare));
        assert_eq!(payload_responded_at(&bare), None);

        let resolved = json!({"id": "act_1", "status": "denied", "responded_at": 5});
        assert!(!payload_is_pending(&resolved));
        assert_eq!(payload_responded_at(&resolved), Some(5));
    }

    #[test]
    fn update_action_payload_resolution_matches_id_only() {
        let mut payload = json!({"id": "act_1", "status": "pending"});
        assert!(!update_action_payload_resolution(
            &mut payload,
            "act_other",
            "approved",
            &json!({"approve": true}),
            9,
        ));
        assert!(update_action_payload_resolution(
            &mut payload,
            "act_1",
            "approved",
            &json!({"approve": true}),
            9,
        ));
        assert_eq!(payload["status"], "approved");
        assert_eq!(payload["response"]["approve"], true);
        assert_eq!(payload["responded_at"], 9);

        assert!(!update_action_payload_resolution(
            &mut json!("not an object"),
            "act_1",
            "approved",
            &Value::Null,
            9,
        ));
    }
}
