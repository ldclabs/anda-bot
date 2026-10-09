use crate::util::tool_response::ToolResponse as Response;
use anda_core::{
    BoxError, Document, FunctionDefinition, Principal, RequestMeta, Resource, StateFeatures, Tool,
    ToolOutput, Usage,
};
use anda_db::{collection::Collection, database::AndaDB, schema::Fv};
use anda_engine::{
    context::BaseCtx,
    memory::{Conversation, ConversationStatus, Conversations},
    rfc3339_datetime,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::util::request_meta::{keys, request_meta_extra_as};

/// Key of [`ContextUsage`] in a conversation's `extra` and in the
/// `GetConversation` and `GetConversationDelta` results.
pub const CONTEXT_USAGE_KEY: &str = "context_usage";

/// The context the conversation's latest model request filled. A
/// conversation's `usage` sums every request, recall runs included, so it says
/// what the conversation consumed, not how full the context is.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ContextUsage {
    /// Input and output tokens of the latest model request.
    pub tokens: u64,
    /// The model's context window; 0 when it is not configured.
    pub window: u64,
}

impl ContextUsage {
    pub fn of(conversation: &Conversation) -> Option<Self> {
        let value = conversation.extra.as_ref()?.get(CONTEXT_USAGE_KEY)?;
        serde_json::from_value(value.clone()).ok()
    }

    pub fn save(self, conversation: &mut Conversation) {
        let value = json!(self);
        match &mut conversation.extra {
            Some(Value::Object(extra)) => {
                extra.insert(CONTEXT_USAGE_KEY.to_string(), value);
            }
            extra @ None => *extra = Some(json!({ CONTEXT_USAGE_KEY: value })),
            Some(_) => {}
        }
    }
}

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
    /// Owner of the bound conversation, so ownership checks need not load it.
    /// States saved by earlier releases lack it until the startup scan
    /// records it; until then they belong to nobody.
    #[serde(default, rename = "u", skip_serializing_if = "Option::is_none")]
    pub user: Option<Principal>,
}

impl SourceState {
    pub fn owned_by(&self, caller: &Principal) -> bool {
        self.user.as_ref() == Some(caller)
    }
}

/// What the startup scan read for a source's conversation, applied only while
/// the source still holds the binding it observed.
pub struct SourceStateRepair {
    pub observed: SourceState,
    pub status: ConversationStatus,
    pub user: Principal,
}

/// Source bindings kept at most; the least recently rebound are dropped
/// first. Every IM thread and CLI workspace is a source, so the map would
/// otherwise grow for the daemon's lifetime, and it is saved whole.
const MAX_SOURCE_STATES: usize = 1024;

fn prune_source_states(states: &mut HashMap<String, SourceState>) {
    while states.len() > MAX_SOURCE_STATES {
        let Some(oldest) = states
            .iter()
            .min_by_key(|(_, state)| state.timestamp)
            .map(|(source, _)| source.clone())
        else {
            break;
        };
        states.remove(&oldest);
    }
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

#[derive(Debug, Clone)]
pub struct RequestState {
    pub workspace: String,
    pub source: String,
    pub source_key: String,
    pub source_state: SourceState,
    pub conversation: u64,
}

/// Marks a context whose tool calls come from an agent rather than a client,
/// which gets display-friendly and memory-policy-filtered results.
#[derive(Debug, Clone)]
pub struct AgentCaller;

/// A tool for conversation API
pub struct ConversationsTool {
    pub events: Arc<super::app_protocol::AppEvents>,
    memory_host: Option<crate::brain::Host>,
    pub conversations: Conversations,
    // The collection `conversations` wraps. anda_engine keeps its own handle
    // private, so this second handle — the same cached `Arc` AndaDB hands out
    // for the name — is how the tool reaches the collection-level extension
    // slots it persists its state in, and the document count for the status
    // report.
    store: Arc<Collection>,
    default_workspace: String,
    tools_usage: RwLock<HashMap<String, Usage>>,
    source_conversation: RwLock<HashMap<String, SourceState>>,
    // Serializes extension persistence so concurrent updates cannot save
    // snapshots out of order: a stale snapshot written last would win on the
    // next daemon start.
    extension_save_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for ConversationsTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConversationsTool")
            .field("default_workspace", &self.default_workspace)
            .finish_non_exhaustive()
    }
}

impl ConversationsTool {
    pub const NAME: &'static str = "conversations_api";

    /// Opens the named conversations collection and creates the tool over it.
    pub async fn connect(
        db: Arc<AndaDB>,
        name: String,
        default_workspace: String,
    ) -> Result<Self, BoxError> {
        let conversations = Conversations::connect(db.clone(), name.clone()).await?;
        // `Conversations::connect` has already registered the collection, so
        // this returns the very handle it holds rather than a second instance.
        let store = db.open_collection(name, async |_| Ok(())).await?;
        Ok(Self {
            events: Arc::new(super::app_protocol::AppEvents::default()),
            memory_host: None,
            conversations,
            store,
            default_workspace,
            tools_usage: RwLock::new(HashMap::new()),
            source_conversation: RwLock::new(HashMap::new()),
            extension_save_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn with_memory_host(mut self, host: crate::brain::Host) -> Self {
        self.memory_host = Some(host);
        self
    }

    pub(crate) async fn may_reuse_memory(
        &self,
        conversation: &Conversation,
    ) -> Result<bool, BoxError> {
        let space = self.memory_space().await?;
        memory_reusable(space.as_deref(), conversation)
    }

    pub(crate) async fn filter_memory_sources(
        &self,
        conversations: Vec<Conversation>,
    ) -> Result<Vec<Conversation>, BoxError> {
        let space = self.memory_space().await?;
        let mut visible = Vec::with_capacity(conversations.len());
        for conversation in conversations {
            if memory_reusable(space.as_deref(), &conversation)? {
                visible.push(conversation)
            }
        }
        Ok(visible)
    }

    /// The Space whose source controls decide memory reuse, when a memory
    /// host is attached.
    async fn memory_space(&self) -> Result<Option<Arc<anda_brain::space::Space>>, BoxError> {
        match &self.memory_host {
            Some(host) => Ok(Some(
                host.state
                    .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
                    .await?,
            )),
            None => Ok(None),
        }
    }

    /// Number of stored conversations.
    pub fn conversations_len(&self) -> usize {
        self.store.len()
    }

    pub fn get_source_state(&self, source: &str) -> Option<SourceState> {
        self.source_conversation.read().get(source).cloned()
    }

    pub fn source_conversations(&self) -> HashMap<String, SourceState> {
        self.source_conversation.read().clone()
    }

    pub fn state_from_meta(&self, meta: &RequestMeta) -> RequestState {
        let (workspace, source) = match (
            request_meta_extra_as::<String>(meta, keys::WORKSPACE),
            request_meta_extra_as::<String>(meta, keys::SOURCE),
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

        let reply_target = request_meta_extra_as::<String>(meta, keys::REPLY_TARGET);
        let thread = request_meta_extra_as::<String>(meta, keys::THREAD);
        let source_key =
            source_conversation_key(&source, reply_target.as_deref(), thread.as_deref());
        let source_state = self.get_source_state(&source_key).unwrap_or_default();
        let conversation = request_meta_extra_as::<u64>(meta, keys::CONVERSATION)
            .filter(|conv_id| *conv_id > 0)
            .unwrap_or(source_state.conv_id);
        RequestState {
            workspace,
            source,
            source_key,
            source_state,
            conversation,
        }
    }

    pub async fn update_source_state(
        &self,
        source: String,
        state: SourceState,
    ) -> Result<(), BoxError> {
        let user = state.user;
        let _guard = self.extension_save_lock.lock().await;
        let fv = {
            let mut map = self.source_conversation.write();
            map.insert(source, state);
            prune_source_states(&mut map);
            Fv::serialized(&*map, None)
        }?;
        self.store
            .save_extension("source_conversation".to_string(), fv)
            .await?;
        if let Some(user) = user {
            self.events.changed(&user.to_string());
        }
        Ok(())
    }

    /// Records `conversation`'s status in the sources bound to it, so clients
    /// can show a channel's state without loading its conversation.
    pub async fn sync_source_status(&self, conversation: &Conversation) -> Result<(), BoxError> {
        let stale = |state: &SourceState| {
            state.conv_id == conversation._id && state.status != conversation.status
        };
        // Most saves keep the status; they need no save lock.
        if self.source_conversation.read().values().any(stale) {
            self.update_source_states(|_, state| {
                if !stale(state) {
                    return false;
                }
                state.status = conversation.status.clone();
                true
            })
            .await?;
        }
        Ok(())
    }

    /// Applies what the startup scan read: the owner of states saved before
    /// owners were recorded, and statuses recorded before they were kept in
    /// sync. A source rebound, or a status recorded, since the scan read it
    /// is newer and kept.
    pub async fn repair_source_states(
        &self,
        repairs: HashMap<String, SourceStateRepair>,
    ) -> Result<(), BoxError> {
        if repairs.is_empty() {
            return Ok(());
        }
        let changed = self
            .update_source_states(|source, state| {
                let Some(repair) = repairs.get(source) else {
                    return false;
                };
                if state.conv_id != repair.observed.conv_id {
                    return false;
                }
                let mut changed = false;
                if state.user.is_none() {
                    state.user = Some(repair.user);
                    changed = true;
                }
                if state.status == repair.observed.status && state.status != repair.status {
                    state.status = repair.status.clone();
                    changed = true;
                }
                changed
            })
            .await?;
        if changed {
            let users: HashSet<_> = repairs.values().map(|repair| repair.user).collect();
            for user in users {
                self.events.changed(&user.to_string());
            }
        }
        Ok(())
    }

    /// Runs `update` over every source binding and saves them once if it
    /// changed any. Returns whether it did.
    async fn update_source_states(
        &self,
        mut update: impl FnMut(&str, &mut SourceState) -> bool,
    ) -> Result<bool, BoxError> {
        let _guard = self.extension_save_lock.lock().await;
        let fv = {
            let mut map = self.source_conversation.write();
            let mut changed = false;
            for (source, state) in map.iter_mut() {
                changed |= update(source, state);
            }
            if !changed {
                return Ok(false);
            }
            Fv::serialized(&*map, None)?
        };
        self.store
            .save_extension("source_conversation".to_string(), fv)
            .await?;
        Ok(true)
    }

    pub async fn delete_source_state(
        &self,
        source: &str,
        caller: &Principal,
    ) -> Result<Option<SourceState>, BoxError> {
        let _guard = self.extension_save_lock.lock().await;
        let (removed, fv) = {
            let mut map = self.source_conversation.write();
            match map.get(source) {
                None => return Ok(None),
                Some(state) if !state.owned_by(caller) => {
                    return Err("permission denied".into());
                }
                Some(_) => {}
            }
            (map.remove(source), Fv::serialized(&*map, None)?)
        };
        self.store
            .save_extension("source_conversation".to_string(), fv)
            .await?;
        self.events.changed(&caller.to_string());
        Ok(removed)
    }

    fn caller_source_states(&self, caller: &Principal) -> HashMap<String, SourceState> {
        self.source_conversation
            .read()
            .iter()
            .filter(|(_, state)| state.owned_by(caller))
            .map(|(source, state)| (source.clone(), state.clone()))
            .collect()
    }

    /// Fails unless `caller` owns `conversation` and, when an agent asks,
    /// memory policy lets it be reused.
    async fn check_readable(
        &self,
        conversation: &Conversation,
        caller: &Principal,
        is_agent: bool,
    ) -> Result<(), BoxError> {
        if &conversation.user != caller {
            return Err("permission denied".into());
        }
        if is_agent && !self.may_reuse_memory(conversation).await? {
            return Err("This conversation is excluded from automatic memory reuse.".into());
        }
        Ok(())
    }

    /// Agents get the conversations memory policy lets them reuse, as pruned
    /// documents; clients get the records as stored.
    async fn render_conversations(
        &self,
        conversations: Vec<Conversation>,
        is_agent: bool,
    ) -> Result<Value, BoxError> {
        if !is_agent {
            return Ok(json!(conversations));
        }
        let docs = self
            .filter_memory_sources(conversations)
            .await?
            .into_iter()
            .map(Document::from)
            .collect::<Vec<_>>();
        Ok(json!(docs))
    }

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
            for (tool, usage) in tools_usage_delta {
                tools_usage.entry(tool).or_default().accumulate(&usage);
            }
            Fv::serialized(&*tools_usage, None)
        }?;
        self.store
            .save_extension("tools_usage".to_string(), tools_usage)
            .await?;
        Ok(())
    }
}

/// Whether memory policy and, when there is one, the memory Space's source
/// controls let `conversation` be reused.
fn memory_reusable(
    space: Option<&anda_brain::space::Space>,
    conversation: &Conversation,
) -> Result<bool, BoxError> {
    if !crate::engine::MemoryPolicy::from_conversation(conversation)?.may_write() {
        return Ok(false);
    }
    let Some(space) = space else {
        return Ok(true);
    };
    let session = conversation.thread.as_ref().map(ToString::to_string);
    let mut source = crate::brain::product::source_identity(
        &conversation.user.to_string(),
        conversation._id,
        session.as_deref(),
    );
    if let Some(parents) = conversation
        .extra
        .as_ref()
        .and_then(|value| value.get("memory_source_parents"))
    {
        source.parents.extend(Vec::<String>::deserialize(parents)?);
        source.parents.sort();
        source.parents.dedup();
    }
    Ok(space.product_source_allowed(&source))
}

/// Lifts [`ContextUsage`] to the top level, beside `usage`, where a client
/// polling deltas finds it.
fn with_context_usage(mut result: Value, context: Option<ContextUsage>) -> Value {
    if let (Some(context), Some(result)) = (context, result.as_object_mut()) {
        result.insert(CONTEXT_USAGE_KEY.to_string(), json!(context));
    }
    result
}

fn ok(result: Value) -> ToolOutput<Response> {
    ToolOutput::new(Response::Ok {
        result,
        next_cursor: None,
    })
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
        let mut source_conversation: HashMap<String, SourceState> = self
            .store
            .get_extension_as("source_conversation")
            .unwrap_or_default();
        prune_source_states(&mut source_conversation);
        *self.source_conversation.write() = source_conversation;

        *self.tools_usage.write() = self
            .store
            .get_extension_as("tools_usage")
            .unwrap_or_default();
        Ok(())
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let is_agent = ctx.get_state::<AgentCaller>().is_some();
        let caller = ctx.caller();
        match args {
            ConversationsToolArgs::GetSourceState {} => {
                let mut state = self.state_from_meta(ctx.meta()).source_state;
                if !state.owned_by(caller) {
                    state = SourceState::default();
                }
                Ok(ok(if is_agent {
                    json!(SourceStateDisplay::from(state))
                } else {
                    json!(state)
                }))
            }
            ConversationsToolArgs::ListSourceState {} => {
                let states = self.caller_source_states(caller);
                Ok(ok(if is_agent {
                    json!(
                        states
                            .into_iter()
                            .map(|(source, state)| (source, SourceStateDisplay::from(state)))
                            .collect::<HashMap<_, _>>()
                    )
                } else {
                    json!(states)
                }))
            }
            ConversationsToolArgs::DeleteSourceState { source } => {
                let source = source.trim();
                if source.is_empty() {
                    return Err("source is required".into());
                }

                let removed = self.delete_source_state(source, caller).await?;
                let deleted = removed.is_some();
                let state = if is_agent {
                    json!(removed.map(SourceStateDisplay::from))
                } else {
                    json!(removed)
                };
                Ok(ok(json!({
                    "source": source,
                    "deleted": deleted,
                    "state": state,
                })))
            }
            ConversationsToolArgs::GetConversation { _id } => {
                // `_id == 0` means "the caller's latest conversation" — the
                // globally latest document may belong to another manager and
                // would make this stably fail with "permission denied".
                let conversation = if _id == 0 {
                    let (conversations, _) = self
                        .conversations
                        .list_conversations_by_user(caller, None, Some(1))
                        .await?;
                    conversations
                        .into_iter()
                        .next()
                        .ok_or("no conversations found")?
                } else {
                    self.conversations.get_conversation(_id).await?
                };
                self.check_readable(&conversation, caller, is_agent).await?;

                Ok(ok(if is_agent {
                    json!(Document::from(conversation))
                } else {
                    with_context_usage(json!(conversation), ContextUsage::of(&conversation))
                }))
            }
            ConversationsToolArgs::GetConversationDelta {
                _id,
                messages_offset,
                artifacts_offset,
            } => {
                let conversation = self.conversations.get_conversation(_id).await?;
                self.check_readable(&conversation, caller, is_agent).await?;

                let context = ContextUsage::of(&conversation);
                Ok(ok(with_context_usage(
                    json!(conversation.into_delta(messages_offset, artifacts_offset)),
                    context,
                )))
            }
            ConversationsToolArgs::BatchGetConversations { ids } => {
                let conversations = self
                    .conversations
                    .batch_get_conversations(caller, ids)
                    .await?;
                Ok(ok(self
                    .render_conversations(conversations, is_agent)
                    .await?))
            }
            ConversationsToolArgs::ListPrevConversations { cursor, limit } => {
                let (conversations, next_cursor) = self
                    .conversations
                    .list_conversations_by_user(caller, cursor, Some(limit.unwrap_or(10)))
                    .await?;
                Ok(ToolOutput::new(Response::Ok {
                    result: self.render_conversations(conversations, is_agent).await?,
                    next_cursor,
                }))
            }
            ConversationsToolArgs::SearchConversations { query, limit } => {
                let conversations = self
                    .conversations
                    .search_conversations(caller, query, Some(limit.unwrap_or(10)))
                    .await?;
                Ok(ok(self
                    .render_conversations(conversations, is_agent)
                    .await?))
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
    use anda_engine::{
        engine::EngineBuilder,
        memory::{Conversation, ConversationRef},
    };

    async fn test_tool() -> ConversationsTool {
        let db = crate::test_support::memory_db("conversations").await;
        ConversationsTool::connect(
            db,
            "conversations".to_string(),
            "/tmp/default-ws".to_string(),
        )
        .await
        .unwrap()
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
        let conversation = Conversation {
            user: *ctx.caller(),
            ..Default::default()
        };
        let id = tool
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await
            .unwrap();

        tool.update_source_state(
            "telegram".to_string(),
            SourceState {
                conv_id: id,
                status: ConversationStatus::Working,
                timestamp: 1_750_000_000_000,
                user: Some(*ctx.caller()),
            },
        )
        .await
        .unwrap();

        assert_eq!(tool.get_source_state("telegram").unwrap().conv_id, id);
        assert_eq!(tool.source_conversations().len(), 1);

        // init() reloads the persisted map after the in-memory copy is lost.
        tool.source_conversation.write().clear();
        tool.init(ctx.clone()).await.unwrap();
        assert_eq!(tool.get_source_state("telegram").unwrap().conv_id, id);

        let removed = tool
            .delete_source_state("telegram", ctx.caller())
            .await
            .unwrap();
        assert_eq!(removed.map(|state| state.conv_id), Some(id));
        assert!(
            tool.delete_source_state("telegram", ctx.caller())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn source_states_record_owners_and_stay_bounded() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        // Owner-bound states answer ownership without loading a conversation.
        for (source, user) in [
            ("mine", *ctx.caller()),
            ("theirs", Principal::management_canister()),
        ] {
            tool.update_source_state(
                source.into(),
                SourceState {
                    conv_id: 7,
                    user: Some(user),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let states = tool.caller_source_states(ctx.caller());
        assert!(states.contains_key("mine"));
        assert!(!states.contains_key("theirs"));
        let theirs = tool.get_source_state("theirs").unwrap();
        assert!(!theirs.owned_by(ctx.caller()));

        let mut states = (0..MAX_SOURCE_STATES as u64 + 2)
            .map(|i| {
                (
                    format!("source-{i}"),
                    SourceState {
                        conv_id: i,
                        timestamp: i,
                        ..Default::default()
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        prune_source_states(&mut states);
        assert_eq!(states.len(), MAX_SOURCE_STATES);
        assert!(!states.contains_key("source-0"));
        assert!(!states.contains_key("source-1"));
        assert!(states.contains_key("source-2"));
    }

    #[tokio::test]
    async fn source_statuses_follow_their_conversation() {
        let tool = test_tool().await;
        for (source, conv_id) in [("a", 7), ("b", 7), ("c", 8)] {
            tool.update_source_state(
                source.into(),
                SourceState {
                    conv_id,
                    status: ConversationStatus::Submitted,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        // The startup scan reads the bindings before a turn records a status.
        let observed_a = tool.get_source_state("a").unwrap();
        let observed_c = tool.get_source_state("c").unwrap();

        let conversation = Conversation {
            _id: 7,
            status: ConversationStatus::Idle,
            ..Default::default()
        };
        tool.sync_source_status(&conversation).await.unwrap();
        let status = |source| tool.get_source_state(source).unwrap().status;
        assert_eq!(status("a"), ConversationStatus::Idle);
        assert_eq!(status("b"), ConversationStatus::Idle);
        assert_eq!(status("c"), ConversationStatus::Submitted);

        let repair = |observed, status| SourceStateRepair {
            observed,
            status,
            user: Principal::anonymous(),
        };
        tool.repair_source_states(HashMap::from([
            (
                "a".to_string(),
                repair(observed_a, ConversationStatus::Working),
            ),
            (
                "c".to_string(),
                repair(observed_c, ConversationStatus::Completed),
            ),
        ]))
        .await
        .unwrap();
        // A repair must not undo a status recorded after it was read, but
        // still records the owner the binding was saved without.
        assert_eq!(status("a"), ConversationStatus::Idle);
        assert_eq!(status("c"), ConversationStatus::Completed);
        let user = |source| tool.get_source_state(source).unwrap().user;
        assert_eq!(user("a"), Some(Principal::anonymous()));
        assert_eq!(user("b"), None);
        assert_eq!(user("c"), Some(Principal::anonymous()));

        let saved: HashMap<String, SourceState> =
            tool.store.get_extension_as("source_conversation").unwrap();
        assert_eq!(saved["b"].status, ConversationStatus::Idle);
        assert_eq!(saved["c"].status, ConversationStatus::Completed);
        assert_eq!(saved["a"].user, Some(Principal::anonymous()));

        // A source rebound since the scan read it is left alone.
        tool.update_source_state(
            "b".into(),
            SourceState {
                conv_id: 9,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut stale = tool.get_source_state("b").unwrap();
        stale.conv_id = 7;
        tool.repair_source_states(HashMap::from([(
            "b".to_string(),
            repair(stale, ConversationStatus::Failed),
        )]))
        .await
        .unwrap();
        let b = tool.get_source_state("b").unwrap();
        assert_eq!(b.conv_id, 9);
        assert_eq!(b.user, None);
        assert_eq!(b.status, ConversationStatus::default());
    }

    #[tokio::test]
    async fn conversations_len_sees_writes_made_through_conversations() {
        let tool = test_tool().await;
        assert_eq!(tool.conversations_len(), 0);

        let conv = Conversation {
            user: Principal::anonymous(),
            ..Default::default()
        };
        tool.conversations
            .add_conversation(ConversationRef::from(&conv))
            .await
            .unwrap();

        // The extension handle and `Conversations` must be the one collection
        // instance AndaDB registered for the name: a second, independently
        // loaded instance would keep its own document index and report 0.
        assert_eq!(tool.conversations_len(), 1);
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

        let total = tool
            .tool_usage_with(|usage| usage.get("shell").cloned())
            .unwrap();
        assert_eq!(total.input_tokens, 10);
        assert_eq!(total.requests, 2);
    }

    #[tokio::test]
    async fn tool_call_reads_source_states_and_conversations() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        let conversation = Conversation {
            user: *ctx.caller(),
            ..Default::default()
        };
        let id = tool
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await
            .unwrap();

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
                conv_id: id,
                status: ConversationStatus::Idle,
                timestamp: 1_750_000_000_000,
                user: Some(*ctx.caller()),
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
        assert_eq!(result["telegram"]["c"], id);

        // The agent-facing variant renders display-friendly fields.
        let agent_ctx = ctx.clone();
        agent_ctx.set_state(AgentCaller);
        let result = ok_result(
            tool.call(
                agent_ctx.clone(),
                ConversationsToolArgs::ListSourceState {},
                Vec::new(),
            )
            .await
            .unwrap(),
        );
        assert_eq!(result["telegram"]["conv_id"], id);
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

        let mut mine = Conversation {
            user: Principal::anonymous(),
            messages: vec![json!({"role": "user", "content": "hello world"})],
            extra: Some(json!({"workspace": "/work"})),
            ..Default::default()
        };
        let context = ContextUsage {
            tokens: 47_279,
            window: 400_000,
        };
        context.save(&mut mine);
        assert_eq!(ContextUsage::of(&mine), Some(context));
        assert_eq!(mine.extra.as_ref().unwrap()["workspace"], "/work");
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
        assert_eq!(result[CONTEXT_USAGE_KEY], json!(context));

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
        assert_eq!(result[CONTEXT_USAGE_KEY], json!(context));

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

    #[tokio::test]
    async fn conversation_tool_get_delta_batch_and_list_variants() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        let caller = *ctx.caller();

        let conv = Conversation {
            user: caller,
            messages: vec![json!(anda_core::Message {
                role: "user".to_string(),
                content: vec![anda_core::ContentPart::Text {
                    text: "hi".to_string()
                }],
                ..Default::default()
            })],
            created_at: 1,
            updated_at: 1,
            ..Default::default()
        };
        let id = tool
            .conversations
            .add_conversation(ConversationRef::from(&conv))
            .await
            .unwrap();

        // Fetch by explicit id and by 0 (latest).
        tool.call(
            ctx.clone(),
            ConversationsToolArgs::GetConversation { _id: id },
            Vec::new(),
        )
        .await
        .unwrap();
        tool.call(
            ctx.clone(),
            ConversationsToolArgs::GetConversation { _id: 0 },
            Vec::new(),
        )
        .await
        .unwrap();

        // Delta, batch, and list variants.
        tool.call(
            ctx.clone(),
            ConversationsToolArgs::GetConversationDelta {
                _id: id,
                messages_offset: 0,
                artifacts_offset: 0,
            },
            Vec::new(),
        )
        .await
        .unwrap();
        tool.call(
            ctx.clone(),
            ConversationsToolArgs::BatchGetConversations { ids: vec![id] },
            Vec::new(),
        )
        .await
        .unwrap();
        tool.call(
            ctx.clone(),
            ConversationsToolArgs::ListPrevConversations {
                cursor: None,
                limit: Some(10),
            },
            Vec::new(),
        )
        .await
        .unwrap();

        // Agent-facing variants render Document/display forms.
        let agent_ctx = ctx.clone();
        agent_ctx.set_state(AgentCaller);
        tool.call(
            agent_ctx.clone(),
            ConversationsToolArgs::GetConversation { _id: id },
            Vec::new(),
        )
        .await
        .unwrap();
        tool.call(
            agent_ctx,
            ConversationsToolArgs::ListPrevConversations {
                cursor: None,
                limit: Some(5),
            },
            Vec::new(),
        )
        .await
        .unwrap();

        // A conversation owned by someone else is permission-denied.
        let other = Conversation {
            user: anda_core::Principal::management_canister(),
            created_at: 1,
            updated_at: 1,
            ..Default::default()
        };
        let other_id = tool
            .conversations
            .add_conversation(ConversationRef::from(&other))
            .await
            .unwrap();
        assert!(
            tool.call(
                ctx,
                ConversationsToolArgs::GetConversation { _id: other_id },
                Vec::new()
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn source_state_api_isolates_callers_and_preserves_other_bindings() {
        let tool = test_tool().await;
        let ctx = EngineBuilder::new().mock_ctx().base;
        for (source, user) in [
            ("mine", *ctx.caller()),
            ("cli:/tmp/default-ws", Principal::management_canister()),
        ] {
            let conversation = Conversation {
                user,
                ..Default::default()
            };
            let id = tool
                .conversations
                .add_conversation(ConversationRef::from(&conversation))
                .await
                .unwrap();
            tool.update_source_state(
                source.into(),
                SourceState {
                    conv_id: id,
                    user: Some(user),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        // A binding saved before owners were recorded belongs to nobody
        // until the startup scan records its owner.
        tool.update_source_state(
            "legacy".into(),
            SourceState {
                conv_id: tool.get_source_state("mine").unwrap().conv_id,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        tool.init(ctx.clone()).await.unwrap();
        let listed = ok_result(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::ListSourceState {},
                vec![],
            )
            .await
            .unwrap(),
        );
        assert!(listed.get("mine").is_some());
        assert!(listed.get("cli:/tmp/default-ws").is_none());
        assert!(listed.get("legacy").is_none());
        assert!(
            tool.delete_source_state("legacy", ctx.caller())
                .await
                .is_err()
        );
        let foreign_ctx = ctx.clone();
        let state = ok_result(
            tool.call(
                foreign_ctx,
                ConversationsToolArgs::GetSourceState {},
                vec![],
            )
            .await
            .unwrap(),
        );
        assert_eq!(state["c"], 0);
        assert!(
            tool.call(
                ctx.clone(),
                ConversationsToolArgs::DeleteSourceState {
                    source: "cli:/tmp/default-ws".into()
                },
                vec![]
            )
            .await
            .is_err()
        );
        assert!(tool.get_source_state("cli:/tmp/default-ws").is_some());
        assert!(
            tool.call(
                ctx,
                ConversationsToolArgs::DeleteSourceState {
                    source: "mine".into()
                },
                vec![]
            )
            .await
            .is_ok()
        );
    }
}
