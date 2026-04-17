use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionFeatures, CompletionRequest, Document,
    Documents, FunctionDefinition, Message, Principal, Resource, StateFeatures,
};
use anda_engine::{
    context::AgentCtx,
    extension::note::{NoteTool, load_notes},
    memory::{
        Conversation, ConversationRef, ConversationStatus, Conversations, MemoryManagement,
        MemoryReadonly,
    },
    rfc3339_datetime, unix_ms,
};
use parking_lot::RwLock;
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock},
};

use super::{HippocampusHook, SYSTEM_PROMPT_DYNAMIC_BOUNDARY};

const SELF_INSTRUCTIONS: &str = include_str!("../../assets/HippocampusRecall.md");

pub static FUNCTION_DEFINITION: LazyLock<FunctionDefinition> = LazyLock::new(|| {
    serde_json::from_value(json!({
        "name": "recall_memory",
        "description": "Recall information from your long-term memory (Cognitive Nexus). Send a natural language query describing what you want to remember or look up — the memory system will search and return relevant knowledge, including facts, preferences, relationships, past events, and any other stored information. Use this whenever you need context from previous interactions or stored knowledge to answer the user's question.",
        "parameters": {
            "type": "object",
            "properties": {
            "query": {
                "type": "string",
                "description": "A natural language question or description of what information to retrieve from memory. Be specific and include relevant context. Examples: 'What are Alice's communication preferences?', 'What happened in our last discussion about Project Aurora?', 'Who are the members of the engineering team?', 'What decisions were made about the pricing strategy?'"
            },
            "context": {
                "type": "object",
                "description": "Optional current conversational context to help narrow the search. Provide any relevant identifiers or topic hints that could improve retrieval accuracy.",
                "properties": {
                "user": {
                    "type": "string",
                    "description": "The identifier of the user currently being interacted with, if applicable."
                },
                "agent": {
                    "type": "string",
                    "description": "The identifier of the calling business agent, if applicable."
                },
                "topic": {
                    "type": "string",
                    "description": "The topic of the current conversation, to help disambiguate the query."
                }
                }
            }
            },
            "required": ["query"]
        },
        "strict": true
    })).unwrap()
});

#[derive(Clone)]
pub struct RecallAgent {
    pub conversations: Conversations,
    memory: Arc<MemoryManagement>,
    hook: Arc<dyn HippocampusHook>,
    history: Arc<RwLock<VecDeque<Document>>>,
    #[allow(dead_code)]
    max_input_tokens: usize,
}

impl RecallAgent {
    pub const NAME: &'static str = "recall_memory";
    pub fn new(
        memory: Arc<MemoryManagement>,
        conversations: Conversations,
        hook: Arc<dyn HippocampusHook>,
        max_input_tokens: usize,
    ) -> Self {
        Self {
            conversations,
            memory,
            hook,
            history: Arc::new(RwLock::new(VecDeque::new())),
            max_input_tokens,
        }
    }

    pub async fn init(&self) -> Result<(), BoxError> {
        let (conversations, _) = self
            .conversations
            .list_conversations_by_user(&Principal::anonymous(), None, Some(3))
            .await?;
        *self.history.write() = conversations
            .into_iter()
            .map(Document::from)
            .collect();
        Ok(())
    }
}

/// Implementation of the [`Agent`] trait for RecallAgent.
impl Agent<AgentCtx> for RecallAgent {
    /// Returns the agent's name identifier
    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    /// Returns a description of the agent's purpose and capabilities.
    fn description(&self) -> String {
        FUNCTION_DEFINITION.description.clone()
    }

    fn definition(&self) -> FunctionDefinition {
        FUNCTION_DEFINITION.clone()
    }

    /// Returns a list of tool names that this agent depends on
    fn tool_dependencies(&self) -> Vec<String> {
        vec![MemoryReadonly::NAME.to_string(), NoteTool::NAME.to_string()]
    }

    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String, // RecallInput serialized as JSON string
        _resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let caller = ctx.caller();
        let now_ms = unix_ms();

        // add history conversations to provide more context for recall
        let chat_history: Vec<Document> = { self.history.read().iter().cloned().collect() };

        let chat_history = if chat_history.is_empty() {
            vec![]
        } else {
            vec![Message {
                role: "user".into(),
                content: vec![
                    Documents::new("history_recall".to_string(), chat_history)
                        .to_string()
                        .into(),
                ],
                name: Some("$system".into()),
                timestamp: Some(now_ms),
                ..Default::default()
            }]
        };

        let primer = self.memory.describe_primer().await.unwrap_or_default();
        let mut conversation = Conversation {
            user: *caller,
            messages: vec![serde_json::json!(Message {
                role: "user".into(),
                content: vec![prompt.clone().into()],
                timestamp: Some(now_ms),
                ..Default::default()
            })],
            status: ConversationStatus::Working,
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            label: Some("recall".to_string()),
            ..Default::default()
        };

        let id = self
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = id;
        let notes = load_notes(&ctx).await.unwrap_or_default();
        match ctx
            .completion(
                CompletionRequest {
                    instructions: format!(
                        "{}\n\n{}\n\n---\n\n# `DESCRIBE PRIMER` Result:\n{}\n\n---\n\n# Your notes:\n{}\n\n# Current datetime: {}",
                        SELF_INSTRUCTIONS,
                        SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
                        primer,
                        serde_json::to_string(&notes.notes).unwrap_or_default(),
                        rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
                    ),
                    prompt,
                    chat_history,
                    tools: ctx.tool_definitions(Some(&[
                        MemoryReadonly::NAME.to_string(),
                    ])),
                    tool_choice_required: true,
                    max_output_tokens: Some(8192),
                    ..Default::default()
                },
                vec![],
            )
            .await
        {
            Ok(mut output) => {
                // Mark conversation as completed successfully
                conversation.messages.clear();
                conversation.append_messages(output.chat_history.clone());
                conversation.status = if output.failed_reason.is_some() {
                    ConversationStatus::Failed
                } else {
                    ConversationStatus::Completed
                };
                conversation.usage = output.usage.clone();
                conversation.updated_at = now_ms;

                if let Some(ref failed_reason) = output.failed_reason {
                    conversation.failed_reason = Some(failed_reason.clone());
                } else {
                    let doc: Document = conversation.clone().into();
                    let mut history = self.history.write();
                    history.push_back(doc);
                    let len = history.len();
                    if len > 3 {
                        history.drain(0..(len - 3));
                    }
                }

                if let Ok(changes) = conversation.to_changes() {
                    let _ = self.conversations.update_conversation(conversation._id, changes).await;
                }
                self.hook
                    .on_conversation_end(Self::NAME, &conversation)
                    .await;
                output.conversation = Some(conversation._id);
                Ok(output)
            }
            Err(err) => {
                conversation.status = ConversationStatus::Failed;
                conversation.failed_reason = Some(err.to_string());
                conversation.updated_at = unix_ms();
                if let Ok(changes) = conversation.to_changes() {
                    let _ = self.conversations.update_conversation(conversation._id, changes).await;
                }
                self.hook
                    .on_conversation_end(Self::NAME, &conversation)
                    .await;
                Err(err)
            }
        }
    }
}
