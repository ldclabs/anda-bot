use crate::util::tool_response::ToolResponse;
use anda_core::{AgentInput, AgentOutput, BoxError, ContentPart, Message, RequestMeta, ToolInput};
use anda_engine::{
    memory::{Conversation, ConversationDelta, ConversationStatus},
    unix_ms,
};
use serde::Deserialize;
use serde_json::Map;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

use super::{Client, MAX_CONVERSATION_CHAIN, is_terminal_conversation_status, tool_result};
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

/// Build a local text message. System messages carry notices and errors
/// that are not part of the persisted conversation history.
fn text_message(role: &str, text: impl Into<String>) -> Message {
    Message {
        role: role.to_string(),
        content: vec![ContentPart::Text { text: text.into() }],
        name: None,
        user: None,
        timestamp: Some(unix_ms()),
    }
}

type SendResult = Result<AgentOutput, String>;
type PollResult = Vec<Fetched>;

/// One conversation of a polled chain. The head arrives as a delta against
/// the stored snapshot when nothing on display can still change.
enum Fetched {
    Full(Box<Conversation>),
    Delta {
        messages_offset: usize,
        delta: ConversationDelta,
    },
}

/// Whether a conversation can still change: it is running, or idle and
/// waiting for follow-up input.
fn is_live(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted | ConversationStatus::Working | ConversationStatus::Idle
    )
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

fn parse_message(value: &serde_json::Value, conv_id: u64) -> Option<Message> {
    Message::deserialize(value)
        .inspect_err(|err| log::warn!("Failed to parse message for conv_id {conv_id}: {err}"))
        .ok()
}

fn has_pending_action(messages: &[Message]) -> bool {
    messages.iter().flat_map(|message| &message.content).any(
        |part| matches!(part, ContentPart::Action { payload, .. } if payload_is_pending(payload)),
    )
}

/// Apply the action resolutions in an updated history entry to the cards on
/// display. A card can sit at another index there (local echoes, earlier
/// conversations of the chain), so cards are matched by action id.
fn merge_action_payload_updates(displayed: &mut [Message], incoming: &Message) -> bool {
    let mut changed = false;
    for part in &incoming.content {
        let ContentPart::Action {
            payload: incoming_payload,
            ..
        } = part
        else {
            continue;
        };
        let Some(action_id) = payload_action_id(incoming_payload) else {
            continue;
        };
        for message in displayed.iter_mut() {
            for part in &mut message.content {
                if let ContentPart::Action { payload, .. } = part
                    && payload_action_id(payload) == Some(action_id)
                {
                    changed |= merge_action_payload(payload, incoming_payload);
                }
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

/// Walk a conversation's child chain from `head`. With `head_delta`, the
/// head is read as a delta from those (messages, artifacts) offsets. Only a
/// failure on the head is an error; a later one ends the chain there.
async fn fetch_chain(
    client: &Client,
    head: u64,
    mut head_delta: Option<(usize, usize)>,
) -> Result<Vec<Fetched>, BoxError> {
    let mut fetched = Vec::new();
    let mut seen = Vec::new();
    let mut next = Some(head);
    while let Some(id) = next {
        if seen.contains(&id) {
            log::warn!("Conversation child chain contains a cycle at {id}");
            break;
        }
        if seen.len() >= MAX_CONVERSATION_CHAIN {
            log::warn!("Conversation child chain is too long starting at {head}");
            break;
        }

        let item = match head_delta.take() {
            Some((messages_offset, artifacts_offset)) => client
                .conversations::<ConversationDelta>(
                    ConversationsToolArgs::GetConversationDelta {
                        _id: id,
                        messages_offset,
                        artifacts_offset,
                    },
                    Some(CONVERSATION_FETCH_TIMEOUT),
                )
                .await
                .map(|delta| {
                    let child = delta.child;
                    (
                        child,
                        Fetched::Delta {
                            messages_offset,
                            delta,
                        },
                    )
                }),
            None => client
                .conversations::<Conversation>(
                    ConversationsToolArgs::GetConversation { _id: id },
                    Some(CONVERSATION_FETCH_TIMEOUT),
                )
                .await
                .map(|conv| (conv.child, Fetched::Full(Box::new(conv)))),
        };
        match item {
            Ok((child, item)) => {
                seen.push(id);
                next = child;
                fetched.push(item);
            }
            Err(err) if fetched.is_empty() => return Err(err),
            Err(err) => {
                log::warn!("Conversation {id} fetch failed: {err}");
                break;
            }
        }
    }
    Ok(fetched)
}

pub struct ChatSession {
    client: Client,
    /// The launch directory, read once: the session's source and its shell
    /// workspace stay the same for as long as the TUI runs.
    workspace: Option<String>,
    conv_id: Option<u64>,
    pub conversation: Option<Conversation>,
    pub messages: Vec<Message>,
    pub sending: bool,
    awaiting_response: bool,
    last_ping: Instant,
    last_poll: Instant,
    pending_send: Option<oneshot::Receiver<SendResult>>,
    /// `Some(has_prompt)` while a `/new` command is in flight.
    pending_new_command: Option<bool>,
    full_access: bool,
    pending_poll: Option<oneshot::Receiver<PollResult>>,
    poll_requested: bool,
    revision: u64,
}

impl ChatSession {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            workspace: std::env::current_dir()
                .ok()
                .map(|dir| dir.to_string_lossy().into_owned()),
            conv_id: None,
            conversation: None,
            messages: Vec::new(),
            sending: false,
            awaiting_response: false,
            last_ping: Instant::now(),
            last_poll: Instant::now(),
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
        let mut extra = Map::new();
        extra.insert(keys::CONVERSATION.to_string(), conversation.into());
        let source = match &self.workspace {
            Some(dir) => format!("cli:{dir}"),
            None => "cli".to_string(),
        };
        extra.insert(keys::SOURCE.to_string(), source.into());
        if let Some(workspace) = &self.workspace {
            extra.insert(keys::WORKSPACE.to_string(), workspace.clone().into());
        }
        if self.full_access {
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

    fn status(&self) -> Option<&ConversationStatus> {
        self.conversation.as_ref().map(|c| &c.status)
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

    #[cfg(test)]
    pub fn reset(&mut self) {
        self.clear_display_for_new_command();
        self.sending = false;
        self.pending_send = None;
        self.pending_new_command = None;
        self.awaiting_response = false;
    }

    /// Start sending a user message without blocking the UI loop.
    pub fn start_send(&mut self, text: String) {
        if self.sending {
            return;
        }

        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }

        let conv_id = self.conv_id.unwrap_or_default();
        let new_command = parse_new_command(&text);
        if new_command.is_some() {
            self.clear_display_for_new_command();
        }
        // A bare `/new` only clears the display; it gets no reply.
        let expects_reply = new_command != Some(false);
        if expects_reply {
            self.messages.push(text_message("user", text.clone()));
        }

        let mut input = AgentInput::new(String::new(), text);
        input.meta = Some(self.request_meta(conv_id));

        let client = self.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(
                client
                    .agent_run_in_cli_workspace(&input)
                    .await
                    .map_err(|err| err.to_string()),
            );
        });

        self.sending = true;
        self.awaiting_response = expects_reply;
        self.pending_send = Some(rx);
        self.pending_new_command = new_command;
        self.mark_changed();
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

    #[cfg(test)]
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
            && let Ok(fetched) = rx.await
        {
            self.apply_poll_result(fetched);
        }
        error
    }

    fn apply_send_result(&mut self, result: SendResult) -> Option<String> {
        self.sending = false;
        self.mark_changed();
        let new_command = self.pending_new_command.take();

        let output = match result {
            Ok(output) => output,
            Err(msg) => {
                self.awaiting_response = false;
                self.messages.push(text_message("system", msg.clone()));
                return Some(format!("Request failed: {msg}"));
            }
        };
        if !output.content.trim().is_empty() {
            self.messages
                .push(text_message("assistant", output.content));
            self.awaiting_response = false;
        }
        if new_command == Some(false) {
            self.clear_display_for_new_command();
            self.awaiting_response = false;
        } else {
            // Poll immediately to get the new conversation data.
            self.start_poll(output.conversation);
        }
        let reason = output.failed_reason?;
        self.awaiting_response = false;
        self.messages.push(text_message("system", reason.clone()));
        Some(reason)
    }

    pub async fn restore_source_conversation(&mut self) -> Result<bool, BoxError> {
        let mut input = ToolInput::new(
            ConversationsTool::NAME.to_string(),
            ConversationsToolArgs::GetSourceState {},
        );
        input.meta = Some(self.request_meta(0));

        let output = self
            .client
            .tool_call_with_timeout::<_, ToolResponse>(&input, CONVERSATION_FETCH_TIMEOUT)
            .await?;
        let state: SourceState = tool_result(output.output)?;
        if state.conv_id == 0 {
            return Ok(false);
        }

        let fetched = fetch_chain(&self.client, state.conv_id, None).await?;
        if !matches!(
            fetched.last(),
            Some(Fetched::Full(conv)) if should_restore_conversation_status(&conv.status)
        ) {
            return Ok(false);
        }

        self.conv_id = None;
        self.conversation = None;
        self.messages.clear();
        self.apply_poll_result(fetched);
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
        let due = self.poll_requested || self.last_poll.elapsed() >= POLL_INTERVAL;
        let stored = self.conversation.as_ref();
        let fetch = self
            .conv_id
            .filter(|id| due && stored.is_none_or(|conv| conv._id != *id || is_live(&conv.status)));
        // The keepalive holds a live session open. Without one the daemon
        // would build a whole system prompt only to reject the empty prompt.
        let ping = stored.is_some_and(|conv| is_live(&conv.status))
            && self.last_ping.elapsed() >= PING_INTERVAL;
        if fetch.is_none() && !ping {
            return;
        }
        // Only action cards change after they are written. While one on
        // display is pending, read the whole snapshot to see it resolved.
        let head_delta = fetch
            .and_then(|id| stored.filter(|conv| conv._id == id))
            .filter(|_| !has_pending_action(&self.messages))
            .map(|conv| (conv.messages.len(), conv.artifacts.len()));
        let ping_meta = ping.then(|| self.request_meta(self.conv_id.unwrap_or_default()));
        self.poll_requested = false;
        if fetch.is_some() {
            self.last_poll = Instant::now();
        }
        if ping {
            self.last_ping = Instant::now();
        }
        let client = self.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            // A slow keepalive must not delay fetching an approval card.
            let keepalive = async {
                if let Some(meta) = ping_meta {
                    let mut input = AgentInput::new(String::new(), String::new());
                    input.meta = Some(meta);
                    let _ = client.agent_run_with_timeout(&input, PING_TIMEOUT).await;
                }
            };
            let fetches = async {
                let fetched = match fetch {
                    Some(id) => fetch_chain(&client, id, head_delta)
                        .await
                        .unwrap_or_else(|err| {
                            log::warn!("Poll conversation {id} failed: {err}");
                            Vec::new()
                        }),
                    None => Vec::new(),
                };
                let _ = tx.send(fetched);
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
            Ok(fetched) => {
                self.pending_poll = None;
                self.apply_poll_result(fetched)
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_poll = None;
                false
            }
        }
    }

    fn apply_poll_result(&mut self, fetched: PollResult) -> bool {
        let mut changed = false;
        for item in fetched {
            // Follow the chain: the next poll starts at the newest child.
            changed |= match item {
                Fetched::Full(conv) => {
                    self.conv_id = Some(conv.child.unwrap_or(conv._id));
                    self.apply_conversation_data(*conv)
                }
                Fetched::Delta {
                    messages_offset,
                    delta,
                } => {
                    self.conv_id = Some(delta.child.unwrap_or(delta._id));
                    self.apply_conversation_delta(messages_offset, delta)
                }
            };
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
            Ok(fetched) => self.apply_poll_result(fetched),
            Err(_) => false,
        }
    }

    fn apply_conversation_data(&mut self, conv: Conversation) -> bool {
        let old_len = self.messages.len();
        let stored = self
            .conversation
            .as_ref()
            .filter(|stored| stored._id == conv._id);
        let mut changed = stored.is_none_or(|stored| stored.status != conv.status);
        // Snapshots include edits to old approval cards. Only parse changed
        // entries; unchanged history needs no cloned Message.
        let previous = stored
            .map(|stored| stored.messages.as_slice())
            .unwrap_or_default();
        let known = previous.len();
        for (old, value) in previous.iter().zip(&conv.messages) {
            if old != value
                && let Some(message) = parse_message(value, conv._id)
            {
                changed |= merge_action_payload_updates(&mut self.messages, &message);
            }
        }
        self.append_new_messages(
            conv.messages.get(known..).unwrap_or_default(),
            conv._id,
            &conv.status,
        );
        self.conversation = Some(conv);

        changed |= self.messages.len() != old_len;
        if changed {
            self.mark_changed();
        }
        changed
    }

    /// Append a delta read against the stored snapshot. A delta for a
    /// snapshot replaced while it was in flight no longer lines up and is
    /// dropped.
    fn apply_conversation_delta(
        &mut self,
        messages_offset: usize,
        delta: ConversationDelta,
    ) -> bool {
        let Some(mut conv) = self
            .conversation
            .take_if(|conv| conv._id == delta._id && conv.messages.len() == messages_offset)
        else {
            return false;
        };
        let old_len = self.messages.len();
        let mut changed = conv.status != delta.status;
        conv.status = delta.status;
        conv.child = delta.child;
        conv.failed_reason = delta.failed_reason;
        conv.usage = delta.usage;
        conv.updated_at = delta.updated_at;
        conv.artifacts.extend(delta.artifacts);
        conv.messages.extend(delta.messages);
        self.append_new_messages(&conv.messages[messages_offset..], conv._id, &conv.status);
        self.conversation = Some(conv);

        changed |= self.messages.len() != old_len;
        if changed {
            self.mark_changed();
        }
        changed
    }

    /// Display entries appended to the conversation, skipping the prefix
    /// that repeats what is already shown (this session's own user echo).
    fn append_new_messages(
        &mut self,
        values: &[serde_json::Value],
        conv_id: u64,
        status: &ConversationStatus,
    ) {
        let parsed: Vec<Message> = values
            .iter()
            .filter_map(|value| parse_message(value, conv_id))
            .collect();
        if parsed.iter().any(|message| message.role == "assistant")
            || is_terminal_conversation_status(status)
        {
            self.awaiting_response = false;
        }
        let overlap = displayed_suffix_prefix_overlap(&self.messages, &parsed);
        self.messages.extend(parsed.into_iter().skip(overlap));
    }

    fn clear_display_for_new_command(&mut self) {
        self.conv_id = None;
        self.conversation = None;
        self.messages.clear();
        self.pending_poll = None;
        self.poll_requested = false;
        self.mark_changed();
    }
}

pub fn is_new_conversation_command(text: &str) -> bool {
    parse_new_command(text).is_some()
}

/// `Some(has_prompt)` when `text` starts a new conversation.
fn parse_new_command(text: &str) -> Option<bool> {
    match PromptCommand::from(text.to_string()) {
        PromptCommand::New { prompt } => Some(prompt.is_some()),
        _ => None,
    }
}

fn should_restore_conversation_status(status: &ConversationStatus) -> bool {
    is_live(status) || *status == ConversationStatus::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> Client {
        Client::new("http://127.0.0.1:8042".to_string(), String::new())
    }

    fn user_message(text: impl Into<String>) -> Message {
        text_message("user", text)
    }

    fn assistant_message(text: impl Into<String>) -> Message {
        text_message("assistant", text)
    }

    fn pending_action_message(id: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            name: Some("$action".to_string()),
            content: vec![ContentPart::Action {
                name: "anda.tool_approval".to_string(),
                payload: serde_json::json!({
                    "id": id,
                    "kind": "tool_approval",
                    "title": "Approve shell command",
                    "status": "pending",
                    "details": [{"label": "Command", "value": "cargo test"}]
                }),
                recipients: None,
                signature: None,
            }],
            ..Default::default()
        }
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
        let pending = pending_action_message("act_1");
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
        assert_eq!(parse_new_command(" /NEW fresh start "), Some(true));
        assert_eq!(parse_new_command("/clear"), Some(false));
        assert_eq!(parse_new_command("/tmp/workspace"), None);
    }

    #[test]
    fn apply_conversation_data_dedupes_local_user_echo() {
        let mut session = ChatSession::new(test_client());
        session.messages.push(user_message("hello"));

        let conv = Conversation {
            _id: 42,
            status: ConversationStatus::Completed,
            messages: vec![
                serde_json::json!(user_message("hello")),
                serde_json::json!(assistant_message("hi")),
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
            messages: vec![serde_json::json!(assistant_message("done"))],
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

    #[test]
    fn conversation_delta_extends_the_stored_snapshot() {
        let mut session = ChatSession::new(test_client());
        let conv = conversation(7, ConversationStatus::Working, None);
        session.apply_conversation_data(conv.clone());
        session.awaiting_response = true;

        let mut next = conv;
        next.messages
            .push(serde_json::to_value(assistant_message("more")).unwrap());
        next.status = ConversationStatus::Completed;
        let revision = session.revision();
        assert!(session.apply_conversation_delta(2, next.to_delta(2, 0)));

        assert_eq!(session.messages.len(), 3);
        assert_eq!(session.messages[2].text().as_deref(), Some("more"));
        let stored = session.conversation.as_ref().unwrap();
        assert_eq!(stored.messages.len(), 3);
        assert_eq!(stored.status, ConversationStatus::Completed);
        assert!(!session.awaiting_response);
        assert_ne!(session.revision(), revision);

        // The snapshot has grown since: the old offset no longer lines up.
        assert!(!session.apply_conversation_delta(2, next.to_delta(2, 0)));
        assert_eq!(session.messages.len(), 3);
        // Nor does a delta of another conversation.
        let mut other = next.to_delta(3, 0);
        other._id = 8;
        assert!(!session.apply_conversation_delta(3, other));
    }

    #[tokio::test]
    async fn keepalive_ping_needs_a_live_conversation() {
        let mut session = ChatSession::new(test_client());
        session.last_ping = Instant::now() - PING_INTERVAL;
        // Nothing to fetch and no session to keep alive: no request at all.
        session.start_poll(None);
        assert!(session.pending_poll.is_none());

        session.conv_id = Some(7);
        session.conversation = Some(Conversation {
            _id: 7,
            status: ConversationStatus::Completed,
            ..Default::default()
        });
        session.start_poll(None);
        assert!(session.pending_poll.is_none());
    }

    use anda_core::ByteBufB64;
    use axum::{Router, extract::State, routing};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    struct ChatGateway {
        conversations: HashMap<u64, Conversation>,
        agent_output: Result<AgentOutput, ()>,
        source_state: serde_json::Value,
    }

    fn rpc_ok<T: serde::Serialize>(value: &T) -> serde_json::Value {
        let rpc: anda_core::http::RPCResponse = Ok(ByteBufB64(serde_json::to_vec(value).unwrap()));
        serde_json::to_value(&rpc).unwrap()
    }

    fn tool_ok<T: serde::Serialize>(value: &T) -> serde_json::Value {
        rpc_ok(&anda_core::ToolOutput::new(ToolResponse::Ok {
            result: serde_json::to_value(value).unwrap(),
            next_cursor: None,
        }))
    }

    /// Answers a conversations tool call from `conversations`, as a whole
    /// conversation or as a delta from the requested offsets.
    fn conversation_reply(
        conversations: &HashMap<u64, Conversation>,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = args["_id"].as_u64().unwrap_or_default();
        let Some(conv) = conversations.get(&id) else {
            let output = anda_core::ToolOutput::new(ToolResponse::Err {
                error: crate::util::tool_response::ToolError::new(
                    "KIP_404",
                    format!("conversation {id} not found"),
                ),
                result: None,
            });
            return rpc_ok(&output);
        };
        match args["type"].as_str() {
            Some("GetConversation") => tool_ok(conv),
            Some("GetConversationDelta") => tool_ok(&conv.to_delta(
                args["messages_offset"].as_u64().unwrap_or_default() as usize,
                args["artifacts_offset"].as_u64().unwrap_or_default() as usize,
            )),
            other => panic!("unexpected tool args type: {other:?}"),
        }
    }

    async fn chat_gateway_handler(
        State(state): State<Arc<ChatGateway>>,
        axum::Json(request): axum::Json<anda_core::http::RPCRequest>,
    ) -> axum::Json<serde_json::Value> {
        if request.method == "agent_run" {
            return axum::Json(match &state.agent_output {
                Ok(output) => rpc_ok(output),
                Err(()) => {
                    let rpc: anda_core::http::RPCResponse = Err("agent unavailable".to_string());
                    serde_json::to_value(&rpc).unwrap()
                }
            });
        }
        let (input,): (ToolInput<serde_json::Value>,) =
            serde_json::from_slice(&request.params).unwrap();
        axum::Json(match input.args["type"].as_str() {
            Some("GetSourceState") => tool_ok(&state.source_state),
            _ => conversation_reply(&state.conversations, &input.args),
        })
    }

    async fn spawn_chat_gateway(state: ChatGateway) -> Client {
        spawn_recording_chat_gateway(state, Default::default()).await
    }

    /// Records each directory the session registers before its prompts.
    async fn spawn_recording_chat_gateway(
        state: ChatGateway,
        registered: Arc<Mutex<Vec<String>>>,
    ) -> Client {
        let app = Router::new()
            .route("/engine/default", routing::post(chat_gateway_handler))
            .with_state(Arc::new(state))
            .route(
                "/daemon/cli-workspace",
                routing::post(
                    move |axum::Json(request): axum::Json<serde_json::Value>| async move {
                        let workspace = request["workspace"].as_str().unwrap_or_default();
                        registered.lock().unwrap().push(workspace.to_string());
                        axum::Json(request)
                    },
                ),
            );
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
        session.start_send("   ".to_string());
        assert!(!session.sending);
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
        let last = session.messages.last().unwrap();
        assert_eq!(last.role, "system");
        assert_eq!(last.text().as_deref(), Some("model exploded"));

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
    async fn every_prompt_registers_the_launch_directory_again() {
        let registered = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_recording_chat_gateway(
            ChatGateway {
                conversations: HashMap::new(),
                agent_output: Ok(AgentOutput::default()),
                source_state: serde_json::json!({"c": 0}),
            },
            registered.clone(),
        )
        .await;
        let mut session = ChatSession::new(client);

        // A daemon restarted since the CLI connected has forgotten the
        // directory, so each prompt registers it again; stopping does not.
        assert!(session.send("hello".to_string()).await.is_none());
        assert!(session.send("again".to_string()).await.is_none());
        assert!(session.send("/stop".to_string()).await.is_none());
        let launch_dir = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(
            *registered.lock().unwrap(),
            [launch_dir.clone(), launch_dir]
        );
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
    async fn conversation_chains_stop_on_cycles_and_keep_what_was_read() {
        let client = spawn_chat_gateway(ChatGateway {
            conversations: HashMap::from([
                (400, conversation(400, ConversationStatus::Idle, Some(400))),
                (
                    500,
                    conversation(500, ConversationStatus::Completed, Some(501)),
                ),
            ]),
            agent_output: Ok(AgentOutput::default()),
            source_state: serde_json::json!({"c": 400}),
        })
        .await;

        let chain = fetch_chain(&client, 400, None).await.unwrap();
        assert_eq!(chain.len(), 1);
        // A missing child ends the chain; only a missing head is an error.
        let chain = fetch_chain(&client, 500, None).await.unwrap();
        assert_eq!(chain.len(), 1);
        assert!(fetch_chain(&client, 501, None).await.is_err());
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
    async fn polls_read_deltas_unless_a_displayed_card_is_pending() {
        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let observed = requests.clone();
        let conversations =
            HashMap::from([(7, conversation(7, ConversationStatus::Working, None))]);
        let base = crate::test_support::spawn_http_mock(Router::new().route(
            "/engine/default",
            routing::post(
                move |axum::Json(request): axum::Json<anda_core::http::RPCRequest>| {
                    let observed = observed.clone();
                    let conversations = conversations.clone();
                    async move {
                        let (input,): (ToolInput<serde_json::Value>,) =
                            serde_json::from_slice(&request.params).unwrap();
                        let kind = input.args["type"].as_str().unwrap_or_default();
                        observed.lock().unwrap().push(kind.to_string());
                        axum::Json(conversation_reply(&conversations, &input.args))
                    }
                },
            ),
        ))
        .await;
        let mut session = ChatSession::new(Client::new(base, String::new()));

        // No snapshot yet, then nothing new since it.
        assert!(session.poll(Some(7)).await);
        assert!(!session.poll(Some(7)).await);
        assert_eq!(session.messages.len(), 2);
        // A pending card on display may still be resolved in place.
        session.messages.push(pending_action_message("act_1"));
        session.poll(Some(7)).await;
        assert_eq!(
            *requests.lock().unwrap(),
            ["GetConversation", "GetConversationDelta", "GetConversation"]
        );
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
        assert!(
            tx.send(vec![Fetched::Full(Box::new(conversation(
                7,
                ConversationStatus::Working,
                None,
            )))])
            .is_ok()
        );
        assert!(session.finish_pending_poll());
        assert_eq!(session.messages.len(), 2);
        assert!(!session.finish_pending_poll());

        let (tx, rx) = oneshot::channel();
        session.pending_poll = Some(rx);
        session.reset();
        assert!(
            tx.send(vec![Fetched::Full(Box::new(conversation(
                7,
                ConversationStatus::Working,
                None,
            )))])
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
        let conversations =
            HashMap::from([(7, conversation(7, ConversationStatus::Working, None))]);
        let base = crate::test_support::spawn_http_mock(Router::new().route(
            "/engine/default",
            routing::post(
                move |axum::Json(request): axum::Json<anda_core::http::RPCRequest>| {
                    let conversations = conversations.clone();
                    async move {
                        if request.method == "agent_run" {
                            return std::future::pending::<axum::Json<serde_json::Value>>().await;
                        }
                        let (input,): (ToolInput<serde_json::Value>,) =
                            serde_json::from_slice(&request.params).unwrap();
                        axum::Json(conversation_reply(&conversations, &input.args))
                    }
                },
            ),
        ))
        .await;
        let mut session = ChatSession::new(Client::new(base, String::new()));
        // A live conversation is due for its keepalive.
        session.conv_id = Some(7);
        session.conversation = Some(Conversation {
            _id: 7,
            status: ConversationStatus::Working,
            ..Default::default()
        });
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
