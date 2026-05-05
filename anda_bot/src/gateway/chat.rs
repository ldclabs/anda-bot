use anda_core::{AgentInput, ContentPart, Message, RequestMeta, ToolInput};
use anda_engine::{
    memory::{Conversation, ConversationStatus},
    unix_ms,
};
use anda_kip::Response as KipResponse;
use serde_json::Map;
use std::time::{Duration, Instant};

use super::Client;
use crate::engine::{ConversationsTool, ConversationsToolArgs};

const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const PING_INTERVAL: Duration = Duration::from_secs(60);

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

pub struct ChatSession {
    client: Client,
    pub conv_id: Option<u64>,
    pub conversation: Option<Conversation>,
    pub prev_conversation: Option<Conversation>,
    pub messages: Vec<Message>,
    pub sending: bool,
    pub errors: Vec<String>,
    last_ping: Instant,
    last_poll: Instant,
    last_msg_offset: usize,
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
            last_ping: Instant::now(),
            last_poll: Instant::now(),
            last_msg_offset: 0,
        }
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
        matches!(
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
        self.errors.clear();
    }

    /// Send a user message. Returns an optional error notice for the UI.
    pub async fn send(&mut self, text: String) -> Option<String> {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return None;
        }

        self.messages.push(user_message(text.clone()));

        let conv_id = self.conv_id.unwrap_or_else(|| {
            self.prev_conversation
                .as_ref()
                .map(|c| c._id)
                .unwrap_or_default()
        });

        self.sending = true;
        let mut input = AgentInput::new(String::new(), text);

        let mut extra = Map::new();
        let workspace = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .ok();
        let source = if let Some(dir) = &workspace {
            format!("cli:{dir}")
        } else {
            "cli".to_string()
        };

        extra.insert("conversation".to_string(), conv_id.into());
        extra.insert("source".to_string(), source.into());
        if let Some(workspace) = workspace {
            extra.insert("workspace".to_string(), workspace.into());
        };

        input.meta = Some(RequestMeta {
            engine: None,
            user: None,
            extra,
        });

        let notice = match self.client.agent_run(&input).await {
            Ok(output) => {
                // Poll immediately to get the new conversation data
                self.poll(output.conversation).await;
                if let Some(reason) = &output.failed_reason {
                    self.errors.push(reason.clone());
                    self.messages.push(system_message(reason.clone()));
                }
                output.failed_reason
            }
            Err(err) => {
                let msg = err.to_string();
                self.messages.push(system_message(msg.clone()));
                Some(format!("Request failed: {msg}"))
            }
        };

        self.sending = false;
        notice
    }

    async fn ping(&mut self) {
        if self.last_ping.elapsed() < PING_INTERVAL {
            return;
        }

        self.last_ping = Instant::now();
        let _ = self
            .client
            .agent_run(&AgentInput::new(String::new(), String::new()))
            .await;
    }

    /// Poll the conversation for updates. Returns `true` if new messages were received.
    pub async fn poll(&mut self, latest_conv_id: Option<u64>) -> bool {
        self.ping().await;

        let mut conv_id = if let Some(id) = latest_conv_id {
            self.conv_id = Some(id);
            id
        } else if let Some(id) = self.conv_id {
            id
        } else {
            return false;
        };

        if latest_conv_id.is_none() && self.last_poll.elapsed() < POLL_INTERVAL {
            return false;
        }

        if let Some(conv) = &self.conversation
            && conv_id == conv._id
            && !self.is_active()
        {
            return false;
        }

        let mut received = false;
        loop {
            self.last_poll = Instant::now();
            match self
                .client
                .tool_call::<ConversationsToolArgs, KipResponse>(&ToolInput::new(
                    ConversationsTool::NAME.to_string(),
                    ConversationsToolArgs::GetConversation { _id: conv_id },
                ))
                .await
            {
                Ok(output) => {
                    if let KipResponse::Ok { result, .. } = output.output {
                        match serde_json::from_value::<Conversation>(result) {
                            Ok(conv) => {
                                let child = conv.child;
                                self.apply_conversation_data(conv);
                                received = true;

                                if self.conv_id != child
                                    && let Some(id) = child
                                {
                                    self.conv_id = Some(id);
                                    conv_id = id;
                                    continue;
                                }

                                return received;
                            }
                            Err(err) => {
                                log::warn!(
                                    "Failed to parse conversation data for conv_id {conv_id}: {err}"
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    log::warn!("Poll conversation {conv_id} failed: {err}");
                }
            }

            return received;
        }
    }

    fn apply_conversation_data(&mut self, conv: Conversation) -> bool {
        if self.conv_id.is_none() {
            self.conv_id = Some(conv._id);
        }

        if self.conv_id == Some(conv._id) {
            if self.conversation.as_ref().map(|c| c._id) != Some(conv._id) {
                self.prev_conversation = self.conversation.take();
                self.last_msg_offset = 0;
            }
            let parsed_messages: Vec<Message> = conv
                .messages
                .iter()
                .skip(self.last_msg_offset)
                .filter_map(|m| match serde_json::from_value::<Message>(m.clone()) {
                    Ok(msg) => Some(msg),
                    Err(err) => {
                        log::warn!("Failed to parse message for conv_id {}: {err}", conv._id);
                        None
                    }
                })
                .collect();
            let overlap = displayed_suffix_prefix_overlap(&self.messages, &parsed_messages);
            self.messages
                .extend(parsed_messages.into_iter().skip(overlap));
            self.last_msg_offset = conv.messages.len();
            self.conversation = Some(conv);
        } else {
            // should not happen, but just in case, we update prev_conversation to keep the history.
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> Client {
        Client::new("http://127.0.0.1:8042".to_string(), String::new())
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
    fn is_thinking_only_for_submitted_and_working() {
        assert!(!ChatSession::new(test_client()).is_thinking());
        assert!(!session_with_status(ConversationStatus::Idle).is_thinking());
        assert!(session_with_status(ConversationStatus::Submitted).is_thinking());
        assert!(session_with_status(ConversationStatus::Working).is_thinking());
        assert!(!session_with_status(ConversationStatus::Completed).is_thinking());
        assert!(!session_with_status(ConversationStatus::Cancelled).is_thinking());
        assert!(!session_with_status(ConversationStatus::Failed).is_thinking());
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
}
