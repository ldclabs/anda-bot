use anda_core::{AgentInput, AgentOutput, BoxError, ContentPart, Message, RequestMeta, ToolInput};
use anda_engine::{
    memory::{Conversation, ConversationStatus},
    unix_ms,
};
use anda_kip::Response as KipResponse;
use serde_json::Map;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

use super::Client;
use crate::engine::{ConversationsTool, ConversationsToolArgs, PromptCommand, SourceState};

const POLL_INTERVAL: Duration = Duration::from_millis(2000);
const PING_INTERVAL: Duration = Duration::from_secs(60);
// The keepalive ping and conversation fetches run inline in the poll loop;
// keep their timeouts short so an unresponsive daemon cannot stall the UI for
// the HTTP client's full default timeout.
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

fn current_request_meta(conversation: u64) -> RequestMeta {
    let mut extra = Map::new();
    let workspace = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok();
    let source = if let Some(dir) = &workspace {
        format!("cli:{dir}")
    } else {
        "cli".to_string()
    };

    extra.insert("conversation".to_string(), conversation.into());
    extra.insert("source".to_string(), source.into());
    if let Some(workspace) = workspace {
        extra.insert("workspace".to_string(), workspace.into());
    };

    RequestMeta {
        engine: None,
        user: None,
        extra,
    }
}

type SendResult = Result<AgentOutput, String>;

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
        input.meta = Some(current_request_meta(conv_id));

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
        None
    }

    /// Collect the result of a pending send if it has finished.
    pub async fn finish_pending_send(&mut self) -> Option<String> {
        let rx = self.pending_send.as_mut()?;

        match rx.try_recv() {
            Ok(result) => {
                self.pending_send = None;
                self.apply_send_result(result).await
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.pending_send = None;
                self.apply_send_result(Err("request task cancelled".to_string()))
                    .await
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
        self.apply_send_result(result).await
    }

    async fn apply_send_result(&mut self, result: SendResult) -> Option<String> {
        self.sending = false;
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
                    self.poll(output.conversation).await;
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
        input.meta = Some(current_request_meta(0));

        let output = self
            .client
            .tool_call_with_timeout::<ConversationsToolArgs, KipResponse>(
                &input,
                CONVERSATION_FETCH_TIMEOUT,
            )
            .await?;

        let state = match output.output {
            KipResponse::Ok { result, .. } => serde_json::from_value::<SourceState>(result)?,
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

    async fn ping(&mut self) {
        if self.last_ping.elapsed() < PING_INTERVAL {
            return;
        }

        self.last_ping = Instant::now();
        let mut input = AgentInput::new(String::new(), String::new());
        input.meta = Some(current_request_meta(self.conv_id.unwrap_or_default()));
        let _ = self.client.agent_run_with_timeout(&input, PING_TIMEOUT).await;
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
            match self.fetch_conversation(conv_id).await {
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

        true
    }

    fn clear_display_for_new_command(&mut self) {
        self.conv_id = None;
        self.conversation = None;
        self.prev_conversation = None;
        self.messages.clear();
        self.last_msg_offset = 0;
        self.errors.clear();
    }

    async fn fetch_conversation(&self, conv_id: u64) -> Result<Conversation, BoxError> {
        let output = self
            .client
            .tool_call_with_timeout::<ConversationsToolArgs, KipResponse>(
                &ToolInput::new(
                    ConversationsTool::NAME.to_string(),
                    ConversationsToolArgs::GetConversation { _id: conv_id },
                ),
                CONVERSATION_FETCH_TIMEOUT,
            )
            .await?;

        match output.output {
            KipResponse::Ok { result, .. } => Ok(serde_json::from_value::<Conversation>(result)?),
            other => Err(format!("conversation API returned an error: {other:?}").into()),
        }
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
            if conversations.len() >= 64 {
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

fn is_terminal_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Completed | ConversationStatus::Cancelled | ConversationStatus::Failed
    )
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
    use base64::Engine;
    use std::{collections::HashMap, sync::Arc};

    struct ChatGateway {
        conversations: HashMap<u64, Conversation>,
        agent_output: Result<AgentOutput, ()>,
        source_state: serde_json::Value,
    }

    async fn chat_gateway_handler(
        State(state): State<Arc<ChatGateway>>,
        axum::Json(request): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        let method = request["method"].as_str().unwrap_or_default().to_string();
        let params = base64::engine::general_purpose::STANDARD
            .decode(request["params"].as_str().unwrap_or_default())
            .unwrap_or_default();

        let rpc: anda_core::http::RPCResponse = if method == "agent_run" {
            match &state.agent_output {
                Ok(output) => Ok(ByteBufB64(serde_json::to_vec(output).unwrap())),
                Err(()) => Err("agent unavailable".to_string()),
            }
        } else {
            let (input,): (ToolInput<serde_json::Value>,) =
                serde_json::from_slice(&params).unwrap();
            let response = match input.args["type"].as_str() {
                Some("GetSourceState") => KipResponse::Ok {
                    result: state.source_state.clone(),
                    next_cursor: None,
                },
                Some("GetConversation") => {
                    let id = input.args["_id"].as_u64().unwrap_or_default();
                    match state.conversations.get(&id) {
                        Some(conv) => KipResponse::Ok {
                            result: serde_json::to_value(conv).unwrap(),
                            next_cursor: None,
                        },
                        None => KipResponse::Err {
                            error: anda_kip::ErrorObject {
                                code: "KIP_404".to_string(),
                                message: format!("conversation {id} not found"),
                                hint: None,
                                data: None,
                            },
                            result: None,
                        },
                    }
                }
                other => panic!("unexpected tool args type: {other:?}"),
            };
            let output: anda_core::ToolOutput<KipResponse> = anda_core::ToolOutput::new(response);
            Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()))
        };

        axum::Json(serde_json::to_value(&rpc).unwrap())
    }

    async fn spawn_chat_gateway(state: ChatGateway) -> Client {
        let app = Router::new()
            .route("/engine/default", routing::post(chat_gateway_handler))
            .with_state(Arc::new(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Client::new(format!("http://{addr}"), "token".to_string())
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

        // Polling again while active refreshes without errors.
        assert!(session.poll(Some(101)).await);

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
                (200, conversation(200, ConversationStatus::Completed, Some(201))),
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
}
