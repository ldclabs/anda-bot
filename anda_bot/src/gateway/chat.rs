use crate::util::tool_response::ToolResponse;
use anda_core::{AgentInput, AgentOutput, BoxError, ContentPart, Message, RequestMeta, ToolInput};
use anda_engine::{
    memory::{Conversation, ConversationStatus},
    unix_ms,
};
use serde_json::Map;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

use super::{Client, MAX_CONVERSATION_CHAIN, is_terminal_conversation_status};
use crate::engine::{
    ConversationsTool, ConversationsToolArgs, PromptCommand, SourceState, payload_action_id,
    payload_is_pending, payload_responded_at,
};
use crate::util::request_meta::keys;

const POLL_INTERVAL: Duration = Duration::from_millis(2000);
const PING_INTERVAL: Duration = Duration::from_secs(60);
// Poll requests run in a background task. Timeouts also bound each outstanding
// request so a stalled daemon does not prevent subsequent refreshes.
const PING_TIMEOUT: Duration = Duration::from_secs(30);
const CONVERSATION_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Build a synthetic system message (used for local notices / errors that
/// aren't part of the persisted conversation history).
fn system_message(text: impl Into<String>) -> Message {
    Message {
        role: "system".to_string(),
        content: vec![ContentPart::Text { text: text.into() }],
        name: None,
        user: None,
        timestamp: Some(unix_ms()),
    }
}

fn user_message(text: impl Into<String>) -> Message {
    Message {
        role: "user".to_string(),
        content: vec![ContentPart::Text { text: text.into() }],
        name: None,
        user: None,
        timestamp: Some(unix_ms()),
    }
}

fn assistant_message(text: impl Into<String>) -> Message {
    Message {
        role: "assistant".to_string(),
        content: vec![ContentPart::Text { text: text.into() }],
        name: None,
        user: None,
        timestamp: Some(unix_ms()),
    }
}

fn current_request_meta(conversation: u64, full_access: bool) -> RequestMeta {
    let mut extra = Map::new();
    let workspace = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok();
    let source = if let Some(dir) = &workspace {
        format!("cli:{dir}")
    } else {
        "cli".to_string()
    };

    extra.insert(keys::CONVERSATION.to_string(), conversation.into());
    extra.insert(keys::SOURCE.to_string(), source.into());
    if let Some(workspace) = workspace {
        extra.insert(keys::WORKSPACE.to_string(), workspace.into());
    };
    if full_access {
        // Runs shell commands and MCP connections without approval cards.
        // Mirrors the Chrome extension's `full_access` setting.
        extra.insert(
            keys::APPROVAL_MODE.to_string(),
            keys::APPROVAL_MODE_FULL_ACCESS.into(),
        );
    }

    RequestMeta {
        engine: None,
        user: None,
        extra,
    }
}

type SendResult = Result<AgentOutput, String>;
type PollResult = Vec<Conversation>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct NewPromptCommand {
    prompt: Option<String>,
}

fn same_display_message(left: &Message, right: &Message) -> bool {
    left.role == right.role && left.content == right.content
}

fn displayed_suffix_prefix_overlap(displayed: &[Message], incoming: &[Message]) -> usize {
    let max = displayed.len().min(incoming.len());
    for len in (1..=max).rev() {
        let displayed_suffix = &displayed[displayed.len() - len..];
        let incoming_prefix = &incoming[..len];
        if displayed_suffix
            .iter()
            .zip(incoming_prefix)
            .all(|(left, right)| same_display_message(left, right))
        {
            return len;
        }
    }
    0
}

fn changed_message_values<'a>(
    previous: &'a [serde_json::Value],
    incoming: &'a [serde_json::Value],
) -> impl Iterator<Item = (usize, &'a serde_json::Value)> {
    incoming
        .iter()
        .enumerate()
        .filter(|(index, value)| previous.get(*index) != Some(*value))
}

fn merge_action_payload_updates(displayed: &mut [Message], incoming: &[Message]) -> bool {
    let incoming_actions = incoming
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|part| match part {
            ContentPart::Action { payload, .. } => {
                payload_action_id(payload).map(|id| (id.to_string(), payload.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if incoming_actions.is_empty() {
        return false;
    }

    let mut changed = false;
    for (action_id, incoming_payload) in incoming_actions {
        for message in displayed.iter_mut() {
            for part in &mut message.content {
                let ContentPart::Action { payload, .. } = part else {
                    continue;
                };
                if payload_action_id(payload) != Some(action_id.as_str()) {
                    continue;
                }
                changed |= merge_action_payload(payload, &incoming_payload);
            }
        }
    }
    changed
}

fn merge_action_payload(target: &mut serde_json::Value, incoming: &serde_json::Value) -> bool {
    if incoming_action_is_stale(target, incoming) {
        return false;
    }

    if let (Some(target), Some(incoming)) = (target.as_object_mut(), incoming.as_object()) {
        let mut changed = false;
        for (key, value) in incoming {
            if target.get(key) == Some(value) {
                continue;
            }
            target.insert(key.clone(), value.clone());
            changed = true;
        }
        return changed;
    }

    if target != incoming {
        *target = incoming.clone();
        true
    } else {
        false
    }
}

fn incoming_action_is_stale(target: &serde_json::Value, incoming: &serde_json::Value) -> bool {
    let target_responded_at = payload_responded_at(target);
    let incoming_responded_at = payload_responded_at(incoming);
    if let (Some(target_at), Some(incoming_at)) = (target_responded_at, incoming_responded_at) {
        return incoming_at < target_at;
    }

    if target_responded_at.is_some() && incoming_responded_at.is_none() {
        return true;
    }

    !payload_is_pending(target) && payload_is_pending(incoming)
}

pub struct ChatSession {
    client: Client,
    pub conv_id: Option<u64>,
    pub conversation: Option<Conversation>,
    pub prev_conversation: Option<Conversation>,
    pub messages: Vec<Message>,
    pub sending: bool,
    pub errors: Vec<String>,
    awaiting_response: bool,
    last_ping: Instant,
    last_poll: Instant,
    last_msg_offset: usize,
    pending_send: Option<oneshot::Receiver<SendResult>>,
    pending_new_command: Option<NewPromptCommand>,
    full_access: bool,
    pending_poll: Option<oneshot::Receiver<PollResult>>,
    poll_requested: bool,
    revision: u64,
}

impl ChatSession {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            conv_id: None,
            prev_conversation: None,
            conversation: None,
            messages: Vec::new(),
            sending: false,
            errors: Vec::new(),
            awaiting_response: false,
            last_ping: Instant::now(),
            last_poll: Instant::now(),
            last_msg_offset: 0,
            pending_send: None,
            pending_new_command: None,
            full_access: false,
            pending_poll: None,
            poll_requested: false,
            revision: 0,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn mark_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Tags every request from this session with the `full_access` approval
    /// mode, so shell commands and MCP connections run without approval cards.
    pub fn with_full_access(mut self, full_access: bool) -> Self {
        self.full_access = full_access;
        self
    }

    fn request_meta(&self, conversation: u64) -> RequestMeta {
        current_request_meta(conversation, self.full_access)
    }

    fn status(&self) -> Option<&ConversationStatus> {
        self.conversation.as_ref().map(|c| &c.status)
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.status(),
            Some(ConversationStatus::Submitted)
                | Some(ConversationStatus::Working)
                | Some(ConversationStatus::Idle)
                | None
        )
    }

    pub fn is_thinking(&self) -> bool {
        self.sending
            || self.awaiting_response
            || matches!(
                self.status(),
                Some(ConversationStatus::Submitted) | Some(ConversationStatus::Working)
            )
    }

    pub fn status_label(&self) -> &'static str {
        match self.status() {
            None => "idle",
            Some(ConversationStatus::Idle) => "idle",
            Some(ConversationStatus::Submitted) => "submitted",
            Some(ConversationStatus::Working) => "working…",
            Some(ConversationStatus::Completed) => "completed",
            Some(ConversationStatus::Cancelled) => "cancelled",
            Some(ConversationStatus::Failed) => "failed",
        }
    }

    #[allow(unused)]
    pub fn reset(&mut self) {
        self.conv_id = None;
        self.conversation = None;
        self.prev_conversation = None;
        self.messages.clear();
        self.last_msg_offset = 0;
        self.sending = false;
        self.pending_send = None;
        self.pending_new_command = None;
        self.awaiting_response = false;
        self.errors.clear();
        self.pending_poll = None;
        self.poll_requested = false;
        self.mark_changed();
    }

    /// Start sending a user message without blocking the UI loop.
    pub fn start_send(&mut self, text: String) -> Option<String> {
        if self.sending {
            return None;
        }

        let text = text.trim().to_owned();
        if text.is_empty() {
            return None;
        }

        let conv_id = self.conv_id.unwrap_or_else(|| {
            self.prev_conversation
                .as_ref()
                .map(|c| c._id)
                .unwrap_or_default()
        });
        let new_command = new_prompt_command(&text);

        if let Some(command) = &new_command {
            self.clear_display_for_new_command();
            if command.prompt.is_some() {
                self.messages.push(user_message(text.clone()));
            }
        } else {
            self.messages.push(user_message(text.clone()));
        }

        let mut input = AgentInput::new(String::new(), text);
        input.meta = Some(self.request_meta(conv_id));

        let client = self.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(
                client
                    .agent_run(&input)
                    .await
                    .map_err(|err| err.to_string()),
            );
        });

        self.sending = true;
        self.awaiting_response = new_command
            .as_ref()
            .map(|command| command.prompt.is_some())
            .unwrap_or(true);
        self.pending_send = Some(rx);
        self.pending_new_command = new_command;
        self.mark_changed();
        None
    }

    /// Collect the result of a pending send if it has finished.
    pub fn finish_pending_send(&mut self) -> Option<String> {
        let rx = self.pending_send.as_mut()?;

        match rx.try_recv() {
            Ok(result) => {
                self.pending_send = None;
                self.apply_send_result(result)
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_send = None;
                self.apply_send_result(Err("request task cancelled".to_string()))
            }
        }
    }

    #[allow(unused)]
    pub async fn send(&mut self, text: String) -> Option<String> {
        if self.sending {
            return None;
        }
        self.start_send(text);
        let rx = self.pending_send.take()?;
        let result = rx
            .await
            .unwrap_or_else(|_| Err("request task cancelled".to_string()));
        let error = self.apply_send_result(result);
        if let Some(rx) = self.pending_poll.take()
            && let Ok(conversations) = rx.await
        {
            self.apply_poll_result(conversations);
        }
        error
    }

    fn apply_send_result(&mut self, result: SendResult) -> Option<String> {
        self.sending = false;
        self.mark_changed();
        let pending_new_command = self.pending_new_command.take();

        match result {
            Ok(mut output) => {
                if !output.content.trim().is_empty() {
                    self.messages
                        .push(assistant_message(output.content.clone()));
                    self.awaiting_response = false;
                }

                if pending_new_command
                    .as_ref()
                    .map(|command| command.prompt.is_some())
                    .unwrap_or(true)
                {
                    // Poll immediately to get the new conversation data.
                    self.start_poll(output.conversation);
                } else {
                    self.clear_display_for_new_command();
                    self.awaiting_response = false;
                }
                if let Some(reason) = output.failed_reason.take() {
                    self.awaiting_response = false;
                    self.errors.push(reason.clone());
                    self.messages.push(system_message(reason.clone()));
                    Some(reason)
                } else {
                    None
                }
            }
            Err(msg) => {
                self.awaiting_response = false;
                self.messages.push(system_message(msg.clone()));
                Some(format!("Request failed: {msg}"))
            }
        }
    }

    pub async fn restore_source_conversation(&mut self) -> Result<bool, BoxError> {
        let mut input = ToolInput::new(
            ConversationsTool::NAME.to_string(),
            ConversationsToolArgs::GetSourceState {},
        );
        input.meta = Some(self.request_meta(0));

        let output = self
            .client
            .tool_call_with_timeout::<ConversationsToolArgs, ToolResponse>(
                &input,
                CONVERSATION_FETCH_TIMEOUT,
            )
            .await?;

        let state = match output.output {
            ToolResponse::Ok { result, .. } => serde_json::from_value::<SourceState>(result)?,
            other => return Err(format!("conversation API returned an error: {other:?}").into()),
        };
        if state.conv_id == 0 {
            return Ok(false);
        }

        let conversations = self.fetch_conversation_chain(state.conv_id).await?;
        if !conversations
            .last()
            .is_some_and(|conv| should_restore_conversation_status(&conv.status))
        {
            return Ok(false);
        }

        self.conv_id = None;
        self.conversation = None;
        self.prev_conversation = None;
        self.messages.clear();
        self.last_msg_offset = 0;

        for conv in conversations {
            let child = conv.child;
            self.conv_id = Some(conv._id);
            self.apply_conversation_data(conv);
            if let Some(id) = child {
                self.conv_id = Some(id);
            }
        }

        Ok(true)
    }

    /// Start at most one poll, retaining an explicit refresh requested while
    /// an earlier poll is in flight (for example after an approval response).
    pub fn start_poll(&mut self, latest_conv_id: Option<u64>) {
        if let Some(id) = latest_conv_id {
            if self.conv_id != Some(id) {
                self.pending_poll = None;
                self.conv_id = Some(id);
                self.mark_changed();
            }
            self.poll_requested = true;
        }
        if self.pending_poll.is_some() {
            return;
        }
        let force = self.poll_requested;
        let fetch = self.conv_id.filter(|id| {
            (force || self.last_poll.elapsed() >= POLL_INTERVAL)
                && (self.is_active()
                    || self
                        .conversation
                        .as_ref()
                        .is_some_and(|conv| conv._id != *id))
        });
        let ping = self.last_ping.elapsed() >= PING_INTERVAL;
        if fetch.is_none() && !ping {
            return;
        }
        self.poll_requested = false;
        if fetch.is_some() {
            self.last_poll = Instant::now();
        }
        if ping {
            self.last_ping = Instant::now();
        }
        let client = self.client.clone();
        let meta = self.request_meta(self.conv_id.unwrap_or_default());
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            // A slow keepalive must not delay fetching an approval card.
            let keepalive = async {
                if ping {
                    let mut input = AgentInput::new(String::new(), String::new());
                    input.meta = Some(meta);
                    let _ = client.agent_run_with_timeout(&input, PING_TIMEOUT).await;
                }
            };
            let fetches = async {
                let mut conversations: Vec<Conversation> = Vec::new();
                let mut next = fetch;
                while let Some(id) = next {
                    if conversations.len() >= MAX_CONVERSATION_CHAIN
                        || conversations.iter().any(|conv| conv._id == id)
                    {
                        break;
                    }
                    match client
                        .get_conversation_with_timeout(id, CONVERSATION_FETCH_TIMEOUT)
                        .await
                    {
                        Ok(conv) => {
                            next = conv.child;
                            conversations.push(conv);
                        }
                        Err(err) => {
                            log::warn!("Poll conversation {id} failed: {err}");
                            break;
                        }
                    }
                }
                let _ = tx.send(conversations);
            };
            tokio::join!(keepalive, fetches);
        });
        self.pending_poll = Some(rx);
    }

    pub fn finish_pending_poll(&mut self) -> bool {
        let Some(rx) = self.pending_poll.as_mut() else {
            return false;
        };
        match rx.try_recv() {
            Ok(conversations) => {
                self.pending_poll = None;
                self.apply_poll_result(conversations)
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_poll = None;
                false
            }
        }
    }

    fn apply_poll_result(&mut self, conversations: PollResult) -> bool {
        let mut changed = false;
        for conv in conversations {
            let child = conv.child;
            self.conv_id = Some(conv._id);
            changed |= self.apply_conversation_data(conv);
            if let Some(child) = child {
                self.conv_id = Some(child);
            }
        }
        changed
    }

    #[cfg(test)]
    pub async fn poll(&mut self, latest_conv_id: Option<u64>) -> bool {
        self.start_poll(latest_conv_id);
        let Some(rx) = self.pending_poll.take() else {
            return false;
        };
        match rx.await {
            Ok(conversations) => self.apply_poll_result(conversations),
            Err(_) => false,
        }
    }

    fn apply_conversation_data(&mut self, conv: Conversation) -> bool {
        let old_len = self.messages.len();
        let mut changed = self
            .conversation
            .as_ref()
            .is_none_or(|previous| previous._id != conv._id || previous.status != conv.status);
        if self.conv_id.is_none() {
            self.conv_id = Some(conv._id);
        }

        if self.conv_id == Some(conv._id) {
            if self.conversation.as_ref().map(|c| c._id) != Some(conv._id) {
                self.prev_conversation = self.conversation.take();
                self.last_msg_offset = 0;
            }
            // Snapshots include edits to old approval cards. Only deserialize
            // new or changed entries; unchanged history needs no cloned Message.
            let previous = self
                .conversation
                .as_ref()
                .map(|conversation| conversation.messages.as_slice())
                .unwrap_or_default();
            let mut parsed_messages = Vec::new();
            for (index, value) in changed_message_values(previous, &conv.messages) {
                match serde_json::from_value::<Message>(value.clone()) {
                    Ok(message) => {
                        changed |= merge_action_payload_updates(
                            &mut self.messages,
                            std::slice::from_ref(&message),
                        );
                        if index >= self.last_msg_offset {
                            parsed_messages.push(message);
                        }
                    }
                    Err(err) => {
                        log::warn!("Failed to parse message for conv_id {}: {err}", conv._id)
                    }
                }
            }
            let has_assistant_message = parsed_messages.iter().any(|msg| msg.role == "assistant");
            let overlap = displayed_suffix_prefix_overlap(&self.messages, &parsed_messages);
            self.messages
                .extend(parsed_messages.into_iter().skip(overlap));
            self.last_msg_offset = conv.messages.len();
            if has_assistant_message || is_terminal_conversation_status(&conv.status) {
                self.awaiting_response = false;
            }
            self.conversation = Some(conv);
        } else {
            // should not happen, but just in case, we update prev_conversation to keep the history.
        }

        changed |= self.messages.len() != old_len;
        if changed {
            self.mark_changed();
        }
        changed
    }

    fn clear_display_for_new_command(&mut self) {
        self.conv_id = None;
        self.conversation = None;
        self.prev_conversation = None;
        self.messages.clear();
        self.last_msg_offset = 0;
        self.errors.clear();
        self.pending_poll = None;
        self.poll_requested = false;
        self.mark_changed();
    }

    async fn fetch_conversation(&self, conv_id: u64) -> Result<Conversation, BoxError> {
        self.client
            .get_conversation_with_timeout(conv_id, CONVERSATION_FETCH_TIMEOUT)
            .await
    }

    async fn fetch_conversation_chain(&self, conv_id: u64) -> Result<Vec<Conversation>, BoxError> {
        let mut conversations = Vec::new();
        let mut next_id = Some(conv_id);

        while let Some(conv_id) = next_id {
            if conversations
                .iter()
                .any(|conv: &Conversation| conv._id == conv_id)
            {
                log::warn!("Conversation child chain contains a cycle at {conv_id}");
                break;
            }
            if conversations.len() >= MAX_CONVERSATION_CHAIN {
                log::warn!("Conversation child chain is too long starting at {conv_id}");
                break;
            }

            let conv = self.fetch_conversation(conv_id).await?;
            next_id = conv.child;
            conversations.push(conv);
        }

        Ok(conversations)
    }
}

pub fn is_new_conversation_command(text: &str) -> bool {
    new_prompt_command(text).is_some()
}

fn new_prompt_command(text: &str) -> Option<NewPromptCommand> {
    match PromptCommand::from(text.to_string()) {
        PromptCommand::New { prompt } => Some(NewPromptCommand { prompt }),
        _ => None,
    }
}

fn should_restore_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted
            | ConversationStatus::Working
            | ConversationStatus::Idle
            | ConversationStatus::Failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> Client {
        Client::new("http://127.0.0.1:8042".to_string(), String::new())
    }

    #[test]
    fn full_access_sessions_declare_the_approval_mode_in_request_meta() {
        let default = ChatSession::new(test_client()).request_meta(7);
        assert_eq!(default.get_extra_as::<String>("approval_mode"), None);
        assert_eq!(default.get_extra_as::<u64>("conversation"), Some(7));

        let full_access = ChatSession::new(test_client())
            .with_full_access(true)
            .request_meta(7);
        assert_eq!(
            full_access.get_extra_as::<String>("approval_mode"),
            Some("full_access".to_string())
        );
    }

    fn session_with_status(status: ConversationStatus) -> ChatSession {
        let mut session = ChatSession::new(test_client());
        session.conversation = Some(Conversation {
            status,
            ..Default::default()
        });
        session
    }

    #[test]
    fn status_label_defaults_to_idle_without_conversation() {
        let session = ChatSession::new(test_client());

        assert_eq!(session.status_label(), "idle");
    }

    #[test]
    fn is_thinking_for_running_or_pending_send() {
        assert!(!ChatSession::new(test_client()).is_thinking());
        assert!(!session_with_status(ConversationStatus::Idle).is_thinking());
        assert!(session_with_status(ConversationStatus::Submitted).is_thinking());
        assert!(session_with_status(ConversationStatus::Working).is_thinking());
        assert!(!session_with_status(ConversationStatus::Completed).is_thinking());
        assert!(!session_with_status(ConversationStatus::Cancelled).is_thinking());
        assert!(!session_with_status(ConversationStatus::Failed).is_thinking());

        let mut sending = ChatSession::new(test_client());
        sending.sending = true;
        assert!(sending.is_thinking());

        let mut awaiting = session_with_status(ConversationStatus::Idle);
        awaiting.awaiting_response = true;
        assert!(awaiting.is_thinking());
    }

    #[test]
    fn restore_source_conversation_statuses_match_active_terminal_states() {
        assert!(should_restore_conversation_status(
            &ConversationStatus::Submitted
        ));
        assert!(should_restore_conversation_status(
            &ConversationStatus::Working
        ));
        assert!(should_restore_conversation_status(
            &ConversationStatus::Idle
        ));
        assert!(should_restore_conversation_status(
            &ConversationStatus::Failed
        ));
        assert!(!should_restore_conversation_status(
            &ConversationStatus::Completed
        ));
        assert!(!should_restore_conversation_status(
            &ConversationStatus::Cancelled
        ));
    }

    #[test]
    fn apply_conversation_data_merges_action_status_updates() {
        let mut session = ChatSession::new(test_client());
        session.conv_id = Some(55);
        let pending = Message {
            role: "assistant".to_string(),
            name: Some("$action".to_string()),
            content: vec![ContentPart::Action {
                name: "anda.tool_approval".to_string(),
                payload: serde_json::json!({
                    "id": "act_1",
                    "kind": "tool_approval",
                    "title": "Approve shell command",
                    "status": "pending",
                    "details": [{"label": "Command", "value": "cargo test"}]
                }),
                recipients: None,
                signature: None,
            }],
            ..Default::default()
        };
        session.apply_conversation_data(Conversation {
            _id: 55,
            status: ConversationStatus::Working,
            messages: vec![serde_json::to_value(&pending).unwrap()],
            ..Default::default()
        });

        let mut resolved = pending;
        if let ContentPart::Action { payload, .. } = &mut resolved.content[0] {
            let object = payload.as_object_mut().unwrap();
            object.insert("status".to_string(), "approved".into());
            object.insert("response".to_string(), serde_json::json!({"approve": true}));
            object.insert("responded_at".to_string(), 123.into());
        }
        session.apply_conversation_data(Conversation {
            _id: 55,
            status: ConversationStatus::Working,
            messages: vec![serde_json::to_value(&resolved).unwrap()],
            ..Default::default()
        });

        assert_eq!(session.messages.len(), 1);
        let ContentPart::Action { payload, .. } = &session.messages[0].content[0] else {
            panic!("expected action part");
        };
        assert_eq!(payload["status"], "approved");
        assert_eq!(payload["response"]["approve"], true);
        assert_eq!(payload["details"][0]["value"], "cargo test");
    }

    #[test]
    fn apply_conversation_data_does_not_revert_resolved_action_to_stale_pending() {
        let mut session = ChatSession::new(test_client());
        session.conv_id = Some(55);
        let pending = Message {
            role: "assistant".to_string(),
            name: Some("$action".to_string()),
            content: vec![ContentPart::Action {
                name: "anda.user_choice".to_string(),
                payload: serde_json::json!({
                    "id": "act_1",
                    "kind": "choice",
                    "title": "Choose",
                    "status": "pending",
                    "choices": [{"id": "ship", "label": "Ship it"}]
                }),
                recipients: None,
                signature: None,
            }],
            ..Default::default()
        };
        session.apply_conversation_data(Conversation {
            _id: 55,
            status: ConversationStatus::Working,
            messages: vec![serde_json::to_value(&pending).unwrap()],
            ..Default::default()
        });

        let ContentPart::Action { payload, .. } = &mut session.messages[0].content[0] else {
            panic!("expected action part");
        };
        let object = payload.as_object_mut().unwrap();
        object.insert("status".to_string(), "selected".into());
        object.insert(
            "response".to_string(),
            serde_json::json!({"choice_id": "ship"}),
        );
        object.insert("responded_at".to_string(), 200.into());

        session.apply_conversation_data(Conversation {
            _id: 55,
            status: ConversationStatus::Working,
            messages: vec![serde_json::to_value(&pending).unwrap()],
            ..Default::default()
        });

        let ContentPart::Action { payload, .. } = &session.messages[0].content[0] else {
            panic!("expected action part");
        };
        assert_eq!(payload["status"], "selected");
        assert_eq!(payload["response"]["choice_id"], "ship");
        assert_eq!(payload["responded_at"], 200);
    }

    #[test]
    fn new_conversation_command_detects_prompt_and_alias() {
        assert_eq!(
            new_prompt_command(" /NEW fresh start "),
            Some(NewPromptCommand {
                prompt: Some("/NEW fresh start".to_string())
            })
        );
        assert_eq!(
            new_prompt_command("/clear"),
            Some(NewPromptCommand { prompt: None })
        );
        assert_eq!(new_prompt_command("/tmp/workspace"), None);
    }

    #[test]
    fn apply_conversation_data_dedupes_local_user_echo() {
        let mut session = ChatSession::new(test_client());
        session.messages.push(user_message("hello"));

        let assistant = Message {
            role: "assistant".to_string(),
            content: vec![ContentPart::Text {
                text: "hi".to_string(),
            }],
            ..Default::default()
        };
        let conv = Conversation {
            _id: 42,
            status: ConversationStatus::Completed,
            messages: vec![
                serde_json::json!(user_message("hello")),
                serde_json::json!(assistant),
            ],
            ..Default::default()
        };

        assert!(session.apply_conversation_data(conv));

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[1].role, "assistant");
    }

    #[test]
    fn apply_conversation_data_clears_awaiting_response_on_reply_or_terminal_status() {
        let mut session = ChatSession::new(test_client());
        session.awaiting_response = true;

        let conv = Conversation {
            _id: 42,
            status: ConversationStatus::Idle,
            messages: vec![serde_json::json!(Message {
                role: "assistant".to_string(),
                content: vec![ContentPart::Text {
                    text: "done".to_string(),
                }],
                ..Default::default()
            })],
            ..Default::default()
        };

        assert!(session.apply_conversation_data(conv));
        assert!(!session.awaiting_response);

        session.awaiting_response = true;
        let conv = Conversation {
            _id: 42,
            status: ConversationStatus::Failed,
            messages: vec![],
            ..Default::default()
        };

        assert!(session.apply_conversation_data(conv));
        assert!(!session.awaiting_response);
    }

    use anda_core::ByteBufB64;
    use axum::{Router, extract::State, routing};
    use std::{collections::HashMap, sync::Arc};

    struct ChatGateway {
        conversations: HashMap<u64, Conversation>,
        agent_output: Result<AgentOutput, ()>,
        source_state: serde_json::Value,
    }

    async fn chat_gateway_handler(
        State(state): State<Arc<ChatGateway>>,
        axum::Json(request): axum::Json<anda_core::http::RPCRequest>,
    ) -> axum::Json<serde_json::Value> {
        let rpc: anda_core::http::RPCResponse = if request.method == "agent_run" {
            match &state.agent_output {
                Ok(output) => Ok(ByteBufB64(serde_json::to_vec(output).unwrap())),
                Err(()) => Err("agent unavailable".to_string()),
            }
        } else {
            let (input,): (ToolInput<serde_json::Value>,) =
                serde_json::from_slice(&request.params).unwrap();
            let response = match input.args["type"].as_str() {
                Some("GetSourceState") => ToolResponse::Ok {
                    result: state.source_state.clone(),
                    next_cursor: None,
                },
                Some("GetConversation") => {
                    let id = input.args["_id"].as_u64().unwrap_or_default();
                    match state.conversations.get(&id) {
                        Some(conv) => ToolResponse::Ok {
                            result: serde_json::to_value(conv).unwrap(),
                            next_cursor: None,
                        },
                        None => ToolResponse::Err {
                            error: crate::util::tool_response::ToolError::new(
                                "KIP_404",
                                format!("conversation {id} not found"),
                            ),
                            result: None,
                        },
                    }
                }
                other => panic!("unexpected tool args type: {other:?}"),
            };
            let output: anda_core::ToolOutput<ToolResponse> = anda_core::ToolOutput::new(response);
            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()))
        };

        axum::Json(serde_json::to_value(&rpc).unwrap())
    }

    async fn spawn_chat_gateway(state: ChatGateway) -> Client {
        let app = Router::new()
            .route("/engine/default", routing::post(chat_gateway_handler))
            .with_state(Arc::new(state));
        let base_url = crate::test_support::spawn_http_mock(app).await;
        Client::new(base_url, "token".to_string())
    }

    fn conversation(id: u64, status: ConversationStatus, child: Option<u64>) -> Conversation {
        Conversation {
            _id: id,
            status,
            child,
            messages: vec![
                serde_json::to_value(user_message(format!("question {id}"))).unwrap(),
                serde_json::to_value(assistant_message(format!("answer {id}"))).unwrap(),
            ],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn send_round_trip_applies_reply_and_polls_conversation() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::from([(
                101,
                conversation(101, ConversationStatus::Working, None),
            )]),
            agent_output: Ok(AgentOutput {
                content: "assistant reply".to_string(),
                conversation: Some(101),
                ..Default::default()
            }),
            source_state: serde_json::json!({"c": 0}),
        })
        .await;
        let mut session = ChatSession::new(client);

        // Guards reject empty input and double sends.
        assert!(session.start_send("   ".to_string()).is_none());
        assert!(session.send("hello there".to_string()).await.is_none());

        assert_eq!(session.conv_id, Some(101));
        assert!(!session.sending);
        // The fetched conversation is still Working, so the session reports
        // thinking via the conversation status.
        assert!(session.is_thinking());
        assert_eq!(session.status_label(), "working…");
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.text().is_some_and(|t| t == "assistant reply"))
        );
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.text().is_some_and(|t| t == "answer 101"))
        );

        // Identical poll data does not invalidate rendering or action caches.
        let revision = session.revision();
        assert!(!session.poll(Some(101)).await);
        assert_eq!(session.revision(), revision);

        session.reset();
        assert!(session.conv_id.is_none());
        assert!(session.messages.is_empty());
    }

    #[tokio::test]
    async fn failed_agent_output_records_error_message() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::new(),
            agent_output: Ok(AgentOutput {
                content: String::new(),
                failed_reason: Some("model exploded".to_string()),
                ..Default::default()
            }),
            source_state: serde_json::json!({"c": 0}),
        })
        .await;
        let mut session = ChatSession::new(client);

        let error = session.send("hello".to_string()).await;
        assert_eq!(error.as_deref(), Some("model exploded"));
        assert_eq!(session.errors, vec!["model exploded".to_string()]);

        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::new(),
            agent_output: Err(()),
            source_state: serde_json::json!({"c": 0}),
        })
        .await;
        let mut session = ChatSession::new(client);
        let error = session.send("hello".to_string()).await;
        assert!(error.is_some_and(|message| message.starts_with("Request failed:")));
    }

    #[tokio::test]
    async fn new_command_clears_display_before_sending() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::new(),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 0}),
        })
        .await;
        let mut session = ChatSession::new(client);
        session.messages.push(user_message("old"));
        session.conv_id = Some(7);

        // A bare /new clears the transcript and does not await a reply.
        session.send("/new".to_string()).await;
        assert!(session.conv_id.is_none());
        assert!(session.messages.is_empty());
        assert!(!session.awaiting_response);

        assert!(is_new_conversation_command("/new"));
        assert!(is_new_conversation_command("/new start fresh"));
        assert!(!is_new_conversation_command("hello"));
    }

    #[tokio::test]
    async fn restore_source_conversation_replays_active_chains() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::from([
                (
                    200,
                    conversation(200, ConversationStatus::Completed, Some(201)),
                ),
                (201, conversation(201, ConversationStatus::Idle, None)),
            ]),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 200}),
        })
        .await;
        let mut session = ChatSession::new(client);

        let restored = session.restore_source_conversation().await.unwrap();
        assert!(restored);
        assert_eq!(session.conv_id, Some(201));
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.text().is_some_and(|t| t == "answer 200"))
        );
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.text().is_some_and(|t| t == "answer 201"))
        );
    }

    #[tokio::test]
    async fn restore_source_conversation_skips_empty_and_finished_state() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::new(),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 0}),
        })
        .await;
        let mut session = ChatSession::new(client);
        assert!(!session.restore_source_conversation().await.unwrap());

        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::from([(
                300,
                conversation(300, ConversationStatus::Completed, None),
            )]),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 300}),
        })
        .await;
        let mut session = ChatSession::new(client);
        assert!(!session.restore_source_conversation().await.unwrap());
    }

    #[tokio::test]
    async fn conversation_chains_stop_on_cycles() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::from([(
                400,
                conversation(400, ConversationStatus::Idle, Some(400)),
            )]),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 400}),
        })
        .await;
        let session = ChatSession::new(client);

        let chain = session.fetch_conversation_chain(400).await.unwrap();
        assert_eq!(chain.len(), 1);
    }

    #[tokio::test]
    async fn poll_skips_when_idle_or_finished() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::new(),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 0}),
        })
        .await;
        let mut session = ChatSession::new(client);

        // No conversation id: nothing to poll.
        assert!(!session.poll(None).await);

        // A finished conversation is not re-polled.
        session.conv_id = Some(7);
        session.conversation = Some(Conversation {
            _id: 7,
            status: ConversationStatus::Completed,
            ..Default::default()
        });
        assert!(!session.poll(Some(7)).await);
    }
    #[tokio::test]
    async fn pending_poll_is_applied_without_waiting_and_discarded_on_reset() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::new(),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c":0}),
        })
        .await;
        let mut session = ChatSession::new(client);
        session.conv_id = Some(7);
        let (tx, rx) = oneshot::channel();
        session.pending_poll = Some(rx);
        assert!(!session.finish_pending_poll());
        tx.send(vec![conversation(7, ConversationStatus::Working, None)])
            .unwrap();
        assert!(session.finish_pending_poll());
        assert_eq!(session.messages.len(), 2);
        assert!(!session.finish_pending_poll());

        let (tx, rx) = oneshot::channel();
        session.pending_poll = Some(rx);
        session.reset();
        assert!(
            tx.send(vec![conversation(7, ConversationStatus::Working, None)])
                .is_err()
        );
        assert!(!session.finish_pending_poll());
        assert!(session.messages.is_empty());
    }

    #[tokio::test]
    async fn send_completion_schedules_refresh_without_waiting_for_http() {
        let base = crate::test_support::spawn_http_mock(Router::new().route(
            "/engine/default",
            routing::post(|| async { std::future::pending::<String>().await }),
        ))
        .await;
        let mut session = ChatSession::new(Client::new(base, String::new()));
        let (tx, rx) = oneshot::channel();
        session.pending_send = Some(rx);
        session.sending = true;
        tx.send(Ok(AgentOutput {
            conversation: Some(7),
            ..Default::default()
        }))
        .unwrap();
        assert!(session.finish_pending_send().is_none());
        assert!(!session.sending);
        assert_eq!(session.conv_id, Some(7));
        assert!(session.pending_poll.is_some());
        assert!(!session.finish_pending_poll());
        // An approval during that fetch requests another poll afterwards.
        session.start_poll(Some(7));
        assert!(session.poll_requested);
    }

    #[tokio::test]
    async fn slow_keepalive_does_not_hold_ready_conversation_data() {
        let conv = conversation(7, ConversationStatus::Working, None);
        let base = crate::test_support::spawn_http_mock(Router::new().route(
            "/engine/default",
            routing::post(
                move |axum::Json(request): axum::Json<anda_core::http::RPCRequest>| {
                    let conv = conv.clone();
                    async move {
                        if request.method == "agent_run" {
                            return std::future::pending::<axum::Json<serde_json::Value>>().await;
                        }
                        let output = anda_core::ToolOutput::new(ToolResponse::Ok {
                            result: serde_json::to_value(conv).unwrap(),
                            next_cursor: None,
                        });
                        let rpc: anda_core::http::RPCResponse =
                            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()));
                        axum::Json(serde_json::to_value(rpc).unwrap())
                    }
                },
            ),
        ))
        .await;
        let mut session = ChatSession::new(Client::new(base, String::new()));
        session.last_ping = Instant::now() - PING_INTERVAL;
        session.start_poll(Some(7));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if session.finish_pending_poll() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("conversation fetch must not wait for keepalive");
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn unchanged_history_is_not_deserialized_again() {
        let previous = (0..1000)
            .map(|index| {
                serde_json::to_value(assistant_message(format!("message {index}"))).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(changed_message_values(&previous, &previous).count(), 0);
        let mut incoming = previous.clone();
        incoming[400] = serde_json::to_value(assistant_message("updated")).unwrap();
        incoming.push(serde_json::to_value(assistant_message("new")).unwrap());
        assert_eq!(
            changed_message_values(&previous, &incoming)
                .map(|(index, _)| index)
                .collect::<Vec<_>>(),
            vec![400, 1000]
        );
    }

    #[test]
    fn malformed_history_entry_does_not_shift_new_message_offsets() {
        let mut session = ChatSession::new(test_client());
        let messages = vec![
            serde_json::json!({"invalid":true}),
            serde_json::to_value(assistant_message("first")).unwrap(),
        ];
        session.apply_conversation_data(Conversation {
            _id: 1,
            messages: messages.clone(),
            ..Default::default()
        });
        let mut next = messages;
        next.push(serde_json::to_value(assistant_message("second")).unwrap());
        session.apply_conversation_data(Conversation {
            _id: 1,
            messages: next,
            ..Default::default()
        });
        assert!(
            session
                .messages
                .iter()
                .any(|message| message.text().as_deref() == Some("second"))
        );
    }
}
