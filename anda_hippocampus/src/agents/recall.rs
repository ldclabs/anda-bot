use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionFeatures, CompletionRequest,
    FunctionDefinition, Message, Resource, StateFeatures,
};
use anda_db::collection::Collection;
use anda_engine::{
    context::AgentCtx,
    memory::{
        Conversation, ConversationRef, ConversationStatus, MemoryManagement, MemoryReadonly,
        SearchConversationsTool,
    },
    rfc3339_datetime, unix_ms,
};
use serde_json::json;
use std::sync::{Arc, LazyLock};

use super::AgentHook;

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
                "required": [
                "query"
                ]
            }
        })).unwrap()
});

#[derive(Clone)]
pub struct RecallAgent {
    pub conversations: Arc<Collection>,
    memory: Arc<MemoryManagement>,
    hook: Arc<dyn AgentHook>,
    #[allow(dead_code)]
    max_input_tokens: usize,
}

impl RecallAgent {
    pub const NAME: &'static str = "recall_memory";
    pub fn new(
        memory: Arc<MemoryManagement>,
        conversations: Arc<Collection>,
        hook: Arc<dyn AgentHook>,
        max_input_tokens: usize,
    ) -> Self {
        Self {
            conversations,
            memory,
            hook,
            max_input_tokens,
        }
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
        vec![
            MemoryReadonly::NAME.to_string(),
            SearchConversationsTool::NAME.to_string(),
        ]
    }

    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String, // RecallInput serialized as JSON string
        _resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let caller = ctx.caller();
        let now_ms = unix_ms();

        let primer = self.memory.describe_primer().await.unwrap_or_default();
        let chat_history = vec![
            Message {
                role: "user".into(),
                content: vec![format!("`DESCRIBE PRIMER` result:\n{}", primer).into()],
                ..Default::default()
            },
            Message {
                role: "user".into(),
                content: vec![
                    format!(
                        "Current datetime: {}",
                        rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
                    )
                    .into(),
                ],
                ..Default::default()
            },
        ];

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
            steering_messages: Some(vec![prompt.clone()]), // 原始输入作为 steering message，供 process_loop 处理
            label: Some("recall".to_string()),
            ..Default::default()
        };

        let id = self
            .conversations
            .add_from(&ConversationRef::from(&conversation))
            .await?;
        self.conversations.flush(now_ms).await?;
        conversation._id = id;

        match ctx
            .completion(
                CompletionRequest {
                    instructions: SELF_INSTRUCTIONS.to_string(),
                    prompt,
                    chat_history,
                    tools: ctx.tool_definitions(Some(&[
                        MemoryReadonly::NAME,
                        SearchConversationsTool::NAME,
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
                }

                if let Ok(changes) = conversation.to_changes() {
                    let _ = self.conversations.update(conversation._id, changes).await;
                    self.conversations.flush(conversation.updated_at).await?;
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
                    let _ = self.conversations.update(conversation._id, changes).await;
                }
                if let Ok(changes) = conversation.to_changes() {
                    let _ = self.conversations.update(conversation._id, changes).await;
                    self.conversations.flush(conversation.updated_at).await?;
                }
                self.hook
                    .on_conversation_end(Self::NAME, &conversation)
                    .await;
                Err(err)
            }
        }
    }
}
