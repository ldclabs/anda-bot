use anda_core::{AgentInput, Json, RequestMeta, ToolInput};
use anda_engine::memory::ConversationStatus;
use anda_kip::Response as KipResponse;
use serde_json::{Map, json};
use std::time::{Duration, Instant};

use super::Client;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Clone)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
}

pub struct ChatSession {
    client: Client,
    pub conversation_id: Option<u64>,
    pub conv_status: Option<ConversationStatus>,
    pub messages: Vec<ChatMessage>,
    pub sending: bool,
    pub failed_reason: Option<String>,
    last_poll: Instant,
    last_msg_count: usize,
}

impl ChatSession {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            conversation_id: None,
            conv_status: None,
            messages: Vec::new(),
            sending: false,
            failed_reason: None,
            last_poll: Instant::now(),
            last_msg_count: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.conv_status,
            Some(ConversationStatus::Submitted) | Some(ConversationStatus::Working)
        )
    }

    pub fn status_label(&self) -> &'static str {
        match &self.conv_status {
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
        self.conversation_id = None;
        self.conv_status = None;
        self.messages.clear();
        self.last_msg_count = 0;
        self.sending = false;
        self.failed_reason = None;
    }

    /// Send a user message. Returns an optional error notice for the UI.
    pub async fn send(&mut self, text: String) -> Option<String> {
        if text.is_empty() {
            return None;
        }

        let previous_conversation_id = self.conversation_id;
        let continuing_existing_conversation =
            previous_conversation_id.is_some() && self.is_active();

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text: text.clone(),
        });

        self.sending = true;
        let mut input = AgentInput::new(String::new(), text);

        if let Some(conv_id) = self.conversation_id
            && self.is_active()
        {
            let mut extra = Map::new();
            extra.insert("conversation".to_string(), json!(conv_id));
            input.meta = Some(RequestMeta {
                engine: None,
                user: None,
                extra,
            });
        }

        let notice = match self.client.agent_run(&input).await {
            Ok(output) => {
                if let Some(id) = output.conversation {
                    let continuing_same_conversation =
                        continuing_existing_conversation && previous_conversation_id == Some(id);
                    self.conversation_id = Some(id);
                    self.conv_status = Some(ConversationStatus::Working);
                    self.last_poll = Instant::now()
                        .checked_sub(POLL_INTERVAL)
                        .unwrap_or_else(Instant::now);
                    self.last_msg_count = if continuing_same_conversation {
                        self.last_msg_count.saturating_add(1)
                    } else {
                        1
                    };
                }
                if let Some(reason) = output.failed_reason {
                    self.messages.push(ChatMessage {
                        role: "system".to_string(),
                        text: format!("Error: {reason}"),
                    });
                    self.conv_status = Some(ConversationStatus::Failed);
                }
                None
            }
            Err(err) => {
                self.messages.push(ChatMessage {
                    role: "system".to_string(),
                    text: format!("Request failed: {err}"),
                });
                Some(format!("Request failed: {err}"))
            }
        };

        self.sending = false;
        notice
    }

    /// Poll the conversation for updates. Returns `true` if new messages were received.
    pub async fn poll(&mut self) -> bool {
        let Some(conv_id) = self.conversation_id else {
            return false;
        };
        if !self.is_active() || self.last_poll.elapsed() < POLL_INTERVAL {
            return false;
        }
        self.last_poll = Instant::now();

        let tool_input = ToolInput::new(
            "conversations_api".to_string(),
            json!({ "type": "GetConversation", "_id": conv_id }),
        );

        match self
            .client
            .tool_call::<Json, KipResponse>(&tool_input)
            .await
        {
            Ok(output) => {
                if let KipResponse::Ok { result, .. } = output.output {
                    return self.apply_conversation_data(&result);
                }
            }
            Err(err) => {
                log::warn!("Poll conversation {conv_id} failed: {err}");
            }
        }
        false
    }

    fn apply_conversation_data(&mut self, data: &Json) -> bool {
        if let Some(status_str) = data.get("status").and_then(|v| v.as_str()) {
            self.conv_status = Some(match status_str {
                "submitted" => ConversationStatus::Submitted,
                "working" => ConversationStatus::Working,
                "completed" => ConversationStatus::Completed,
                "cancelled" => ConversationStatus::Cancelled,
                "failed" => ConversationStatus::Failed,
                _ => ConversationStatus::Working,
            });
        }

        if let Some(reason) = data.get("failed_reason").and_then(|v| v.as_str())
            && !reason.is_empty()
        {
            self.failed_reason = Some(reason.to_string());
        }

        let mut has_new = false;
        if let Some(msgs) = data.get("messages").and_then(|v| v.as_array())
            && msgs.len() > self.last_msg_count
        {
            for msg_json in &msgs[self.last_msg_count..] {
                if let Some(cm) = parse_message(msg_json) {
                    self.messages.push(cm);
                }
            }
            self.last_msg_count = msgs.len();
            has_new = true;
        }
        has_new
    }
}

fn parse_message(msg_json: &Json) -> Option<ChatMessage> {
    let role = msg_json.get("role")?.as_str()?.to_string();
    if role == "system" {
        return None;
    }

    let content = msg_json.get("content")?;
    let text = match content {
        Json::String(s) => s.clone(),
        Json::Array(parts) => {
            let mut texts = Vec::new();
            for part in parts {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if part_type.is_empty()
                        || part_type == "Text"
                        || part_type == "Reasoning"
                        || part_type == "Action"
                    {
                        texts.push(t.to_string());
                    }
                }
            }
            if texts.is_empty() {
                return None;
            }
            texts.join("\n")
        }
        _ => return None,
    };

    if text.trim().is_empty() {
        return None;
    }

    Some(ChatMessage { role, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> Client {
        Client::new("http://127.0.0.1:8042".to_string(), String::new())
    }

    #[test]
    fn apply_conversation_data_skips_locally_echoed_user_message() {
        let mut session = ChatSession::new(test_client());
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            text: "hello".to_string(),
        });
        session.last_msg_count = 1;

        let has_new = session.apply_conversation_data(&json!({
            "status": "working",
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                },
                {
                    "role": "assistant",
                    "content": "world"
                }
            ]
        }));

        assert!(has_new);
        assert_eq!(session.last_msg_count, 2);
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[1].role, "assistant");
        assert_eq!(session.messages[1].text, "world");
    }
}
