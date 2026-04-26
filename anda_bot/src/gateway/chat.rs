use anda_core::{AgentInput, Message, RequestMeta, ToolInput};
use anda_engine::{
    memory::{Conversation, ConversationStatus},
    rfc3339_datetime, unix_ms,
};
use anda_kip::Response as KipResponse;
use serde_json::Map;
use std::time::{Duration, Instant};

use super::Client;
use crate::engine::{ConversationsTool, ConversationsToolArgs};

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    pub role: String,
    pub text: Option<String>,
    pub thoughts: Option<String>,
    pub error: Option<String>,
    #[allow(unused)]
    pub timestamp: Option<String>,
    #[allow(unused)]
    pub conv_id: u64,
}

pub struct ChatSession {
    client: Client,
    pub conv_id: Option<u64>,
    pub conversation: Option<Conversation>,
    pub prev_conversation: Option<Conversation>,
    pub messages: Vec<ChatMessage>,
    pub sending: bool,
    pub errors: Vec<String>,
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
        if text.is_empty() {
            return None;
        }

        let conv_id = self.conv_id.unwrap_or_else(|| {
            self.prev_conversation
                .as_ref()
                .map(|c| c._id)
                .unwrap_or_default()
        });

        self.sending = true;
        let mut input = AgentInput::new(String::new(), text);

        let mut extra = Map::new();
        let work_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .ok();
        let source = if let Some(dir) = &work_dir {
            format!("cli:{dir}")
        } else {
            "cli".to_string()
        };

        extra.insert("conversation".to_string(), conv_id.into());
        extra.insert("source".to_string(), source.into());
        if let Some(work_dir) = work_dir {
            extra.insert("work_dir".to_string(), work_dir.into());
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
                    self.messages.push(ChatMessage {
                        role: "system".to_string(),
                        error: Some(reason.clone()),
                        timestamp: rfc3339_datetime(unix_ms()),
                        conv_id,
                        ..Default::default()
                    });
                }
                output.failed_reason
            }
            Err(err) => {
                self.messages.push(ChatMessage {
                    role: "system".to_string(),
                    error: Some(err.to_string()),
                    timestamp: rfc3339_datetime(unix_ms()),
                    conv_id,
                    ..Default::default()
                });
                Some(format!("Request failed: {err}"))
            }
        };

        self.sending = false;
        notice
    }

    /// Poll the conversation for updates. Returns `true` if new messages were received.
    pub async fn poll(&mut self, latest_conv_id: Option<u64>) -> bool {
        let conv_id = if let Some(id) = latest_conv_id {
            self.conv_id = Some(id);
            id
        } else if let Some(id) = self.conv_id {
            id
        } else {
            return false;
        };

        if !self.is_active()
            || (latest_conv_id.is_none() && self.last_poll.elapsed() < POLL_INTERVAL)
        {
            return false;
        }

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
                        Ok(conv) => return self.apply_conversation_data(conv),
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
        false
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
            self.messages.extend(
                conv.messages
                    .iter()
                    .skip(self.last_msg_offset)
                    .filter_map(|m| match serde_json::from_value::<Message>(m.clone()) {
                        Ok(msg) => Some(ChatMessage {
                            text: msg.text(),
                            thoughts: msg.thoughts(),
                            error: None,
                            timestamp: msg.timestamp.and_then(rfc3339_datetime),
                            role: msg.role,
                            conv_id: conv._id,
                        }),
                        Err(err) => {
                            log::warn!("Failed to parse message for conv_id {}: {err}", conv._id);
                            None
                        }
                    }),
            );
            self.last_msg_offset = conv.messages.len();
            self.conversation = Some(conv);
        } else {
            // should not happen, but just in case, we update prev_conversation to keep the history.
            self.prev_conversation = Some(conv);
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
}
