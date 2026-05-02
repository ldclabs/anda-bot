use anda_core::{BoxError, FunctionDefinition, Resource, StateFeatures, Tool, ToolOutput};
use anda_engine::{context::BaseCtx, memory::Conversations};
use anda_kip::Response;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Arguments for "conversation_api" tool
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ConversationsToolArgs {
    /// Get a conversation by ID
    GetConversation {
        /// The ID of the conversation to get
        _id: u64,
    },
    GetConversationDelta {
        /// The ID of the conversation to get
        _id: u64,
        /// The messages offset for the conversation delta
        #[serde(default)]
        messages_offset: usize,
        /// The artifacts offset for the conversation delta
        #[serde(default)]
        artifacts_offset: usize,
    },
    /// List previous conversations
    ListPrevConversations {
        /// The cursor for pagination
        cursor: Option<String>,
        /// The limit for pagination, default to 10
        limit: Option<usize>,
    },
    /// Search conversations
    SearchConversations {
        /// The query string to search
        query: String,
        /// The max number of conversations to return, default to 10
        limit: Option<usize>,
    },
}

/// A tool for conversation API
#[derive(Debug, Clone)]
pub struct ConversationsTool {
    conversations: Conversations,
}

impl ConversationsTool {
    pub const NAME: &'static str = "conversations_api";

    /// Creates a new ConversationTool instance
    pub fn new(conversations: Conversations) -> Self {
        Self { conversations }
    }
}

impl Tool<BaseCtx> for ConversationsTool {
    type Args = ConversationsToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "A unified API for managing conversations. Supports retrieving conversation details, listing previous conversations with pagination, searching conversation history by keyword.".to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
              "type": "object",
              "properties": {
                "type": {
                    "type": "string",
                    "enum": [
                        "GetConversation",
                        "GetConversationDelta",
                        "ListPrevConversations",
                        "SearchConversations"
                    ],
                    "description": "The type of conversation operation to perform."
                },
                "_id": {
                    "type": "integer",
                    "description": "The ID of the conversation to retrieve. Required for GetConversation and GetConversationDelta."
                },
                "messages_offset": {
                    "type": ["integer", "null"],
                    "description": "The messages offset for the conversation delta. Required for GetConversationDelta."
                },
                "artifacts_offset": {
                    "type": ["integer", "null"],
                    "description": "The artifacts offset for the conversation delta. Required for GetConversationDelta."
                },
                "cursor": {
                    "type": ["string", "null"],
                    "description": "The cursor for pagination. Optional for ListPrevConversations."
                },
                "limit": {
                    "type": ["integer", "null"],
                    "description": "The maximum number of conversations to return. Optional for ListPrevConversations and SearchConversations, defaults to 10."
                },
                "query": {
                    "type": ["string", "null"],
                    "description": "The query string to search conversations. Required for SearchConversations."
                }
              },
              "required": ["type"]
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        match args {
            ConversationsToolArgs::GetConversation { _id } => {
                let conversation = self.conversations.get_conversation(_id).await?;
                if &conversation.user != ctx.caller() {
                    return Err("permission denied".into());
                }

                Ok(ToolOutput::new(Response::Ok {
                    result: json!(conversation),
                    next_cursor: None,
                }))
            }
            ConversationsToolArgs::GetConversationDelta {
                _id,
                messages_offset,
                artifacts_offset,
            } => {
                let conversation = self.conversations.get_conversation(_id).await?;
                if &conversation.user != ctx.caller() {
                    return Err("permission denied".into());
                }

                Ok(ToolOutput::new(Response::Ok {
                    result: json!(conversation.into_delta(messages_offset, artifacts_offset)),
                    next_cursor: None,
                }))
            }
            ConversationsToolArgs::ListPrevConversations { cursor, limit } => {
                let (conversations, next_cursor) = self
                    .conversations
                    .list_conversations_by_user(ctx.caller(), cursor, limit)
                    .await?;

                Ok(ToolOutput::new(Response::Ok {
                    result: json!(conversations),
                    next_cursor,
                }))
            }
            ConversationsToolArgs::SearchConversations { query, limit } => {
                let conversations = self
                    .conversations
                    .search_conversations(ctx.caller(), query, limit)
                    .await?;

                Ok(ToolOutput::new(Response::Ok {
                    result: json!(conversations),
                    next_cursor: None,
                }))
            }
        }
    }
}
