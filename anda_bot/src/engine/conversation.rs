use anda_core::{
    BoxError, Document, FunctionDefinition, RequestMeta, Resource, StateFeatures, Tool, ToolOutput,
    Usage,
};
use anda_db::schema::Fv;
use anda_engine::{
    context::BaseCtx,
    memory::{ConversationStatus, Conversations},
    rfc3339_datetime,
};
use anda_kip::Response;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::util::request_meta::request_meta_extra_as;

/// Arguments for "conversation_api" tool
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ConversationsToolArgs {
    /// Get the source-bound conversation state from request metadata
    GetSourceState {},
    /// List the state of all conversations associated with sources.
    ListSourceState {},
    /// Delete the state of a source-bound conversation without deleting conversation records.
    DeleteSourceState {
        /// The source key to delete
        source: String,
    },
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
    BatchGetConversations {
        /// The IDs of the conversations to get
        ids: Vec<u64>,
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
    #[serde(rename = "c", alias = "conv_id")]
    pub conv_id: u64,
    #[serde(default, rename = "s", alias = "status")]
    pub status: ConversationStatus,
    #[serde(default, rename = "t", alias = "timestamp")]
    pub timestamp: u64,
}

#[derive(Serialize)]
pub struct SourceStateDisplay {
    pub conv_id: u64,
    pub status: ConversationStatus,
    pub timestamp: String,
}

impl From<SourceState> for SourceStateDisplay {
    fn from(state: SourceState) -> Self {
        Self {
            conv_id: state.conv_id,
            status: state.status,
            timestamp: rfc3339_datetime(state.timestamp).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct AgentInfo {
    #[allow(unused)]
    pub name: String,
}

/// A tool for conversation API
#[derive(Debug)]
pub struct ConversationsTool {
    pub conversations: Conversations,
    default_workspace: String,
    tools_usage: RwLock<HashMap<String, Usage>>,
    source_conversation: RwLock<HashMap<String, SourceState>>,
    // Serializes extension persistence so concurrent updates cannot save
    // snapshots out of order: a stale snapshot written last would win on the
    // next daemon start.
    extension_save_lock: tokio::sync::Mutex<()>,
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
            extension_save_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn get_source_state(&self, source: &str) -> Option<SourceState> {
        self.source_conversation.read().get(source).cloned()
    }

    pub fn source_conversations(&self) -> HashMap<String, SourceState> {
        self.source_conversation.read().clone()
    }

    pub fn state_from_meta(&self, meta: &RequestMeta) -> RequestState {
        let (workspace, source) = match (
            request_meta_extra_as::<String>(meta, "workspace"),
            request_meta_extra_as::<String>(meta, "source"),
        ) {
            (Some(workspace), Some(source)) => (workspace, source),
            (Some(workspace), None) => (workspace.clone(), format!("cli:{workspace}")),
            (None, Some(source)) => {
                let workspace = if let Some(v) = source.strip_prefix("cli:") {
                    v.to_string()
                } else {
                    self.default_workspace.clone()
                };
                (workspace, source)
            }
            (None, None) => {
                let workspace = self.default_workspace.clone();
                let source = format!("cli:{workspace}");
                (workspace, source)
            }
        };

        let reply_target = request_meta_extra_as::<String>(meta, "reply_target");
        let thread = request_meta_extra_as::<String>(meta, "thread");
        let source_key =
            source_conversation_key(&source, reply_target.as_deref(), thread.as_deref());
        let source_state = self.get_source_state(&source_key).unwrap_or_default();
        let conversation = request_meta_extra_as::<u64>(meta, "conversation")
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
        let _guard = self.extension_save_lock.lock().await;
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

    pub async fn delete_source_state(&self, source: &str) -> Result<Option<SourceState>, BoxError> {
        let _guard = self.extension_save_lock.lock().await;
        let (removed, fv) = {
            let mut map = self.source_conversation.write();
            let removed = map.remove(source);
            if removed.is_none() {
                return Ok(None);
            }
            (removed, Fv::serialized(&*map, None)?)
        };
        self.conversations
            .conversations
            .save_extension("source_conversation".to_string(), fv)
            .await?;
        Ok(removed)
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

        let _guard = self.extension_save_lock.lock().await;
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

fn conversations_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": [
                    "GetSourceState",
                    "ListSourceState",
                    "DeleteSourceState",
                    "GetConversation",
                    "GetConversationDelta",
                    "BatchGetConversations",
                    "ListPrevConversations",
                    "SearchConversations"
                ],
                "description": "Conversation operation to perform. Prefer ListSourceState to inspect all source-bound conversation states and discover conv_id values, DeleteSourceState to remove a source binding without deleting conversations, GetConversation to load a full conversation by _id, and SearchConversations to locate history by keyword when the _id is unknown."
            },
            "source": {
                "type": ["string", "null"],
                "description": "Source key to delete. Only for DeleteSourceState; use a key returned by ListSourceState."
            },
            "_id": {
                "type": ["integer", "null"],
                "description": "Conversation ID to load. Use the conv_id returned by GetSourceState or ListSourceState. For GetConversation, _id = 0 resolves to the caller's latest conversation."
            },
            "ids": {
                "type": ["array", "null"],
                "items": { "type": "integer" },
                "description": "The IDs of the conversations to get. Only for BatchGetConversations."
            },
            "messages_offset": {
                "type": ["integer", "null"],
                "description": "Only for GetConversationDelta. Number of messages already known to the caller; use 0 to return from the beginning."
            },
            "artifacts_offset": {
                "type": ["integer", "null"],
                "description": "Only for GetConversationDelta. Number of artifacts already known to the caller; use 0 to return from the beginning."
            },
            "cursor": {
                "type": ["string", "null"],
                "description": "Pagination cursor from a previous ListPrevConversations response. Omit for the first page."
            },
            "limit": {
                "type": ["integer", "null"],
                "description": "Optional maximum number of conversations to return for ListPrevConversations or SearchConversations. Defaults to 10."
            },
            "query": {
                "type": ["string", "null"],
                "description": "Keyword, phrase, participant, or topic to search in historical conversations. Required for SearchConversations when the conversation _id is unknown."
            }
        },
        "required": ["type", "source", "_id", "ids", "messages_offset", "artifacts_offset", "cursor", "limit", "query"],
        "additionalProperties": false
    })
}

impl Tool<BaseCtx> for ConversationsTool {
    type Args = ConversationsToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Read the caller's conversation state and conversation history. ",
            "Use ListSourceState to inspect all tracked conversation sources and discover each source's current conversation _id. ",
            "Use GetConversation to load the full contents of one conversation when you already have its _id. ",
            "Use DeleteSourceState to remove a source binding without deleting conversation records. ",
            "Use SearchConversations to find earlier conversations by keyword, topic, or phrase when the _id is unknown. "
        ).to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: conversations_tool_parameters(),
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
        let is_agent = ctx.get_state::<AgentInfo>().is_some();
        match args {
            ConversationsToolArgs::GetSourceState {} => {
                let state = self.state_from_meta(ctx.meta());
                let result = if is_agent {
                    json!(SourceStateDisplay::from(state.source_state))
                } else {
                    json!(state.source_state)
                };

                Ok(ToolOutput::new(Response::Ok {
                    result,
                    next_cursor: None,
                }))
            }
            ConversationsToolArgs::ListSourceState {} => {
                let states = self.source_conversations();
                let result = if is_agent {
                    json!(
                        states
                            .into_iter()
                            .map(|(source, state)| (source, SourceStateDisplay::from(state)))
                            .collect::<HashMap<_, _>>()
                    )
                } else {
                    json!(states)
                };

                Ok(ToolOutput::new(Response::Ok {
                    result,
                    next_cursor: None,
                }))
            }
            ConversationsToolArgs::DeleteSourceState { source } => {
                let source = source.trim();
                if source.is_empty() {
                    return Err("source is required".into());
                }

                let removed = self.delete_source_state(source).await?;
                let deleted = removed.is_some();
                let result = if is_agent {
                    json!({
                        "source": source,
                        "deleted": deleted,
                        "state": removed.map(SourceStateDisplay::from),
                    })
                } else {
                    json!({
                        "source": source,
                        "deleted": deleted,
                        "state": removed,
                    })
                };

                Ok(ToolOutput::new(Response::Ok {
                    result,
                    next_cursor: None,
                }))
            }
            ConversationsToolArgs::GetConversation { _id } => {
                let _id = if _id == 0 {
                    self.conversations
                        .conversations
                        .latest_document_id()
                        .unwrap_or_default()
                } else {
                    _id
                };
                let conversation = self.conversations.get_conversation(_id).await?;
                if &conversation.user != ctx.caller() {
                    return Err("permission denied".into());
                }

                let result = if is_agent {
                    let doc = Document::from(conversation);
                    json!(doc)
                } else {
                    json!(conversation)
                };

                Ok(ToolOutput::new(Response::Ok {
                    result,
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
            ConversationsToolArgs::BatchGetConversations { ids } => {
                let result = self
                    .conversations
                    .batch_get_conversations(ctx.caller(), ids)
                    .await?;

                Ok(ToolOutput::new(Response::Ok {
                    result: json!(result),
                    next_cursor: None,
                }))
            }
            ConversationsToolArgs::ListPrevConversations { cursor, limit } => {
                let (conversations, next_cursor) = self
                    .conversations
                    .list_conversations_by_user(ctx.caller(), cursor, Some(limit.unwrap_or(10)))
                    .await?;

                let result = if is_agent {
                    let docs = conversations
                        .into_iter()
                        .map(Document::from)
                        .collect::<Vec<_>>();
                    json!(docs)
                } else {
                    json!(conversations)
                };

                Ok(ToolOutput::new(Response::Ok {
                    result,
                    next_cursor,
                }))
            }
            ConversationsToolArgs::SearchConversations { query, limit } => {
                let conversations = self
                    .conversations
                    .search_conversations(ctx.caller(), query, Some(limit.unwrap_or(10)))
                    .await?;

                let result = if is_agent {
                    let docs = conversations
                        .into_iter()
                        .map(Document::from)
                        .collect::<Vec<_>>();
                    json!(docs)
                } else {
                    json!(conversations)
                };

                Ok(ToolOutput::new(Response::Ok {
                    result,
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
    use crate::util::json_schema::assert_openai_strict_parameters;

    #[test]
    fn conversations_api_schema_is_openai_strict() {
        assert_openai_strict_parameters(&conversations_tool_parameters());
    }

    #[test]
    fn conversation_tool_args_parse_tagged_variants() {
        let args: ConversationsToolArgs = serde_json::from_value(json!({
            "type": "GetSourceState",
            "source": null,
            "_id": null,
            "ids": null,
            "messages_offset": null,
            "artifacts_offset": null,
            "cursor": null,
            "limit": null,
            "query": null,
        }))
        .expect("source state variant should parse");

        assert_eq!(args, ConversationsToolArgs::GetSourceState {});

        let args: ConversationsToolArgs = serde_json::from_value(json!({
            "type": "DeleteSourceState",
            "source": "browser:chrome:123",
        }))
        .expect("delete source state variant should parse");

        assert_eq!(
            args,
            ConversationsToolArgs::DeleteSourceState {
                source: "browser:chrome:123".to_string(),
            }
        );

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

        let err = serde_json::from_value::<ConversationsToolArgs>(json!({
            "type": "DeleteSourceState",
        }))
        .expect_err("delete source state requires a source key");

        assert!(err.to_string().contains("source"));
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

    use anda_core::Principal;
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
    };
    use anda_engine::{
        engine::EngineBuilder,
        memory::{Conversation, ConversationRef},
    };
    use object_store::memory::InMemory;
    use std::sync::Arc;

    async fn test_tool() -> ConversationsTool {
        let object_store: Arc<dyn object_store::ObjectStore> = Arc::new(InMemory::new());
        let db = AndaDB::connect(
            object_store,
            DBConfig {
                name: "conversations_test_db".to_string(),
                description: "conversations test db".to_string(),
                storage: StorageConfig {
                    cache_max_capacity: 1024,
                    compress_level: 1,
                    object_chunk_size: 256 * 1024,
                    bucket_overload_size: 256 * 1024,
                    max_small_object_size: 1024 * 1024,
                },
                lock: None,
            },
        )
        .await
        .unwrap();
        let conversations = Conversations::connect(Arc::new(db), "conversations".to_string())
            .await
            .unwrap();
        ConversationsTool::new(conversations, "/tmp/default-ws".to_string())
    }

    fn meta_with_extra(entries: &[(&str, Value)]) -> RequestMeta {
        let mut extra = serde_json::Map::new();
        for (key, value) in entries {
            extra.insert((*key).to_string(), value.clone());
        }
        RequestMeta {
            extra,
            ..Default::default()
        }
    }

    fn ok_result(output: ToolOutput<Response>) -> Value {
        match output.output {
            Response::Ok { result, .. } => result,
            other => panic!("expected ok response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn state_from_meta_resolves_workspace_and_source() {
        let tool = test_tool().await;

        let state = tool.state_from_meta(&meta_with_extra(&[
            ("workspace", json!("/tmp/ws")),
            ("source", json!("telegram")),
            ("reply_target", json!("chat-1")),
            ("thread", json!("topic-2")),
            ("conversation", json!(7)),
        ]));
        assert_eq!(state.workspace, "/tmp/ws");
        assert_eq!(state.source, "telegram");
        assert_eq!(
            state.source_key,
            "telegram:reply_target:chat-1:thread:topic-2"
        );
        assert_eq!(state.conversation, 7);

        let state = tool.state_from_meta(&meta_with_extra(&[("workspace", json!("/tmp/ws"))]));
        assert_eq!(state.source, "cli:/tmp/ws");

        let state = tool.state_from_meta(&meta_with_extra(&[("source", json!("cli:/tmp/other"))]));
        assert_eq!(state.workspace, "/tmp/other");

        let state = tool.state_from_meta(&meta_with_extra(&[("source", json!("discord"))]));
        assert_eq!(state.workspace, "/tmp/default-ws");
        assert_eq!(state.source, "discord");

        let state = tool.state_from_meta(&RequestMeta::default());
        assert_eq!(state.workspace, "/tmp/default-ws");
        assert_eq!(state.source, "cli:/tmp/default-ws");
        assert_eq!(state.conversation, 0);
    }

    #[tokio::test]
    async fn source_state_round_trips_through_extension_storage() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;

        tool.update_source_state(
            "telegram".to_string(),
            SourceState {
                conv_id: 11,
                status: ConversationStatus::Working,
                timestamp: 1_750_000_000_000,
            },
        )
        .await
        .unwrap();

        assert_eq!(tool.get_source_state("telegram").unwrap().conv_id, 11);
        assert_eq!(tool.source_conversations().len(), 1);

        // init() reloads the persisted map after the in-memory copy is lost.
        tool.source_conversation.write().clear();
        tool.init(ctx).await.unwrap();
        assert_eq!(tool.get_source_state("telegram").unwrap().conv_id, 11);

        let removed = tool.delete_source_state("telegram").await.unwrap();
        assert_eq!(removed.map(|state| state.conv_id), Some(11));
        assert!(
            tool.delete_source_state("telegram")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn tool_usage_accumulates_and_persists() {
        let tool = test_tool().await;

        tool.accumulate_tool_usage(HashMap::new()).await.unwrap();
        assert!(tool.tools_usage().is_empty());

        let delta = HashMap::from([(
            "shell".to_string(),
            Usage {
                input_tokens: 5,
                output_tokens: 3,
                cached_tokens: 1,
                requests: 1,
            },
        )]);
        tool.accumulate_tool_usage(delta.clone()).await.unwrap();
        tool.accumulate_tool_usage(delta).await.unwrap();

        let total = tool.tool_usage_with(|usage| usage.get("shell").cloned()).unwrap();
        assert_eq!(total.input_tokens, 10);
        assert_eq!(total.requests, 2);
    }

    #[tokio::test]
    async fn tool_call_reads_source_states_and_conversations() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;

        // GetSourceState falls back to an empty default state.
        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::GetSourceState {},
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["c"], 0);

        tool.update_source_state(
            "telegram".to_string(),
            SourceState {
                conv_id: 11,
                status: ConversationStatus::Idle,
                timestamp: 1_750_000_000_000,
            },
        )
        .await
        .unwrap();

        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::ListSourceState {},
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["telegram"]["c"], 11);

        // The agent-facing variant renders display-friendly fields.
        let agent_ctx = ctx.clone();
        agent_ctx.set_state(AgentInfo {
            name: "anda".to_string(),
        });
        let result = ok_result(
            tool.call(
                agent_ctx.clone(),
                ConversationsToolArgs::ListSourceState {},
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["telegram"]["conv_id"], 11);
        assert!(result["telegram"]["timestamp"].is_string());

        let err = tool
            .call(
                ctx.clone(),
                ConversationsToolArgs::DeleteSourceState {
                    source: "  ".to_string(),
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("source is required"));

        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::DeleteSourceState {
                    source: "telegram".to_string(),
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["deleted"], true);

        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::DeleteSourceState {
                    source: "telegram".to_string(),
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["deleted"], false);
    }

    #[tokio::test]
    async fn tool_call_enforces_conversation_ownership() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;

        let mine = Conversation {
            user: Principal::anonymous(),
            messages: vec![json!({"role": "user", "content": "hello world"})],
            ..Default::default()
        };
        let my_id = tool
            .conversations
            .add_conversation(ConversationRef::from(&mine))
            .await
            .unwrap();

        let theirs = Conversation {
            user: Principal::management_canister(),
            ..Default::default()
        };
        let their_id = tool
            .conversations
            .add_conversation(ConversationRef::from(&theirs))
            .await
            .unwrap();

        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::GetConversation { _id: my_id },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["_id"], my_id);

        let err = tool
            .call(
                ctx.clone(),
                ConversationsToolArgs::GetConversation { _id: their_id },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("permission denied"));

        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::GetConversationDelta {
                    _id: my_id,
                    messages_offset: 0,
                    artifacts_offset: 0,
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["messages"].as_array().map(Vec::len), Some(1));

        let err = tool
            .call(
                ctx.clone(),
                ConversationsToolArgs::GetConversationDelta {
                    _id: their_id,
                    messages_offset: 0,
                    artifacts_offset: 0,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("permission denied"));

        // Batch get only returns the caller's conversations.
        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::BatchGetConversations {
                    ids: vec![my_id, their_id],
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result.as_array().map(Vec::len), Some(1));

        let result = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::ListPrevConversations {
                    cursor: None,
                    limit: None,
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result.as_array().map(Vec::len), Some(1));

        let result = ok_result(
            tool.call(
                ctx,
                ConversationsToolArgs::SearchConversations {
                    query: "hello".to_string(),
                    limit: None,
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert!(result.is_array());
    }
}
