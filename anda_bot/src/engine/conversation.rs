use anda_core::{
    BoxError, FunctionDefinition, RequestMeta, Resource, StateFeatures, Tool, ToolOutput, Usage,
};
use anda_db::schema::Fv;
use anda_engine::{context::BaseCtx, memory::Conversations};
use anda_kip::Response;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// Arguments for "conversation_api" tool
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ConversationsToolArgs {
    /// Get the source-bound conversation state from request metadata
    GetSourceState {},
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SourceState {
    #[serde(rename = "c")]
    pub conv_id: u64,
}

#[derive(Debug, Clone)]
pub struct RequestState {
    pub workspace: String,
    pub source: String,
    pub source_key: String,
    pub source_state: SourceState,
    pub conversation: u64,
    #[allow(unused)]
    pub reply_target: Option<String>,
    #[allow(unused)]
    pub thread: Option<String>,
}

/// A tool for conversation API
#[derive(Debug)]
pub struct ConversationsTool {
    pub conversations: Conversations,
    default_workspace: String,
    tools_usage: RwLock<HashMap<String, Usage>>,
    source_conversation: RwLock<HashMap<String, SourceState>>,
}

impl ConversationsTool {
    pub const NAME: &'static str = "conversations_api";

    /// Creates a new ConversationTool instance
    pub fn new(conversations: Conversations, default_workspace: String) -> Self {
        Self {
            conversations,
            default_workspace,
            tools_usage: RwLock::new(HashMap::new()),
            source_conversation: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_source_state(&self, source: &str) -> Option<SourceState> {
        self.source_conversation.read().get(source).cloned()
    }

    pub fn state_from_meta(&self, meta: &RequestMeta) -> RequestState {
        let workspace = meta
            .get_extra_as::<String>("workspace")
            .unwrap_or_else(|| self.default_workspace.clone());
        let source = meta
            .get_extra_as::<String>("source")
            .unwrap_or_else(|| format!("cli:{workspace}"));
        let reply_target = meta.get_extra_as::<String>("reply_target");
        let thread = meta.get_extra_as::<String>("thread");
        let source_key =
            source_conversation_key(&source, reply_target.as_deref(), thread.as_deref());
        let source_state = self.get_source_state(&source_key).unwrap_or_default();
        let conversation = meta
            .get_extra_as::<u64>("conversation")
            .filter(|conv_id| *conv_id > 0)
            .unwrap_or(source_state.conv_id);
        RequestState {
            workspace,
            source,
            source_key,
            source_state,
            conversation,
            reply_target,
            thread,
        }
    }

    pub async fn update_source_state(
        &self,
        source: String,
        state: SourceState,
    ) -> Result<(), BoxError> {
        let fv = {
            let mut map = self.source_conversation.write();
            map.insert(source, state);
            Fv::serialized(&*map, None)
        }?;
        self.conversations
            .conversations
            .save_extension("source_conversation".to_string(), fv)
            .await?;
        Ok(())
    }

    #[allow(unused)]
    pub fn tools_usage(&self) -> HashMap<String, Usage> {
        self.tools_usage.read().clone()
    }

    pub fn tool_usage_with<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&HashMap<String, Usage>) -> R,
    {
        f(&self.tools_usage.read())
    }

    pub async fn accumulate_tool_usage(
        &self,
        tools_usage_delta: HashMap<String, Usage>,
    ) -> Result<(), BoxError> {
        if tools_usage_delta.is_empty() {
            return Ok(());
        }

        let tools_usage = {
            let mut tools_usage = self.tools_usage.write();
            for (tool, usage) in tools_usage_delta.into_iter() {
                let entry = tools_usage.entry(tool.clone()).or_default();
                entry.accumulate(&usage);
            }
            Fv::serialized(&*tools_usage, None)
        }?;
        self.conversations
            .conversations
            .save_extension("tools_usage".to_string(), tools_usage)
            .await?;
        Ok(())
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
                        "GetSourceState",
                        "GetConversation",
                        "GetConversationDelta",
                        "ListPrevConversations",
                        "SearchConversations"
                    ],
                    "description": "The type of conversation operation to perform. GetSourceState uses request metadata to return the conversation associated with the current source."
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

    async fn init(&self, _ctx: BaseCtx) -> Result<(), BoxError> {
        {
            let source_conversation: HashMap<String, SourceState> = self
                .conversations
                .conversations
                .get_extension_as("source_conversation")
                .unwrap_or_default();

            *self.source_conversation.write() = source_conversation;
        }
        {
            let tools_usage: HashMap<String, Usage> = self
                .conversations
                .conversations
                .get_extension_as("tools_usage")
                .unwrap_or_default();

            *self.tools_usage.write() = tools_usage;
        }

        Ok(())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        match args {
            ConversationsToolArgs::GetSourceState {} => {
                let state = self.state_from_meta(ctx.meta());
                Ok(ToolOutput::new(Response::Ok {
                    result: json!(state.source_state),
                    next_cursor: None,
                }))
            }
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

pub fn source_conversation_key(
    source: &str,
    reply_target: Option<&str>,
    thread: Option<&str>,
) -> String {
    match reply_target {
        Some(reply_target) => format!(
            "{source}:reply_target:{reply_target}:thread:{}",
            thread.unwrap_or_default()
        ),
        None => source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_tool_args_parse_tagged_variants() {
        let args: ConversationsToolArgs = serde_json::from_value(json!({
            "type": "GetSourceState",
        }))
        .expect("source state variant should parse");

        assert_eq!(args, ConversationsToolArgs::GetSourceState {});

        let args: ConversationsToolArgs = serde_json::from_value(json!({
            "type": "GetConversationDelta",
            "_id": 42,
            "messages_offset": 3,
            "artifacts_offset": 5,
        }))
        .expect("tagged variant should parse");

        assert_eq!(
            args,
            ConversationsToolArgs::GetConversationDelta {
                _id: 42,
                messages_offset: 3,
                artifacts_offset: 5,
            }
        );
    }

    #[test]
    fn conversation_tool_args_default_optional_list_fields() {
        let args: ConversationsToolArgs = serde_json::from_value(json!({
            "type": "ListPrevConversations",
        }))
        .expect("missing optional list fields should parse");

        assert_eq!(
            args,
            ConversationsToolArgs::ListPrevConversations {
                cursor: None,
                limit: None,
            }
        );
    }

    #[test]
    fn conversation_tool_args_reject_missing_required_variant_fields() {
        let err = serde_json::from_value::<ConversationsToolArgs>(json!({
            "type": "SearchConversations",
        }))
        .expect_err("search query is required");

        assert!(err.to_string().contains("query"));
    }

    #[test]
    fn source_conversation_key_is_route_aware_for_channels() {
        assert_eq!(
            source_conversation_key("cli:/tmp/app", None, None),
            "cli:/tmp/app"
        );
        assert_eq!(
            source_conversation_key("telegram", Some("chat-1"), None),
            "telegram:reply_target:chat-1:thread:"
        );
        assert_ne!(
            source_conversation_key("telegram", Some("chat-1"), Some("thread-a")),
            source_conversation_key("telegram", Some("chat-1"), Some("thread-b"))
        );
    }
}
