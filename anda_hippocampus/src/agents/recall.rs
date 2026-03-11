use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionFeatures, CompletionRequest,
    FunctionDefinition, Message, Resource,
};
use anda_engine::{
    context::AgentCtx,
    memory::{MemoryReadonly, SearchConversationsTool},
    rfc3339_datetime, unix_ms,
};
use serde_json::json;
use std::sync::LazyLock;

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
    #[allow(dead_code)]
    max_input_tokens: usize,
}

impl RecallAgent {
    pub const NAME: &'static str = "recall_memory";
    pub fn new(max_input_tokens: usize) -> Self {
        Self { max_input_tokens }
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
        let now_ms = unix_ms();
        let msg = Message {
            role: "user".into(),
            content: vec![
                format!(
                    "Current datetime: {}",
                    rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
                )
                .into(),
            ],
            ..Default::default()
        };

        ctx.completion(
            CompletionRequest {
                instructions: SELF_INSTRUCTIONS.to_string(),
                prompt,
                chat_history: vec![msg],
                tools: ctx
                    .tool_definitions(Some(&[MemoryReadonly::NAME, SearchConversationsTool::NAME])),
                tool_choice_required: true,
                max_output_tokens: Some(20000),
                ..Default::default()
            },
            vec![],
        )
        .await
    }
}
