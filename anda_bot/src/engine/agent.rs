use anda_brain::types::{FormationInputRef, InputContext};
use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, Document, Documents,
    FunctionDefinition, Message, Principal, Resource, StateFeatures, Tool, ToolOutput, Usage,
};
use anda_db_utils::UniqueVec;
use anda_engine::{
    ANONYMOUS,
    context::{
        AgentCtx, BaseCtx, CompletionRunner, TOOLS_GROUPS_NAME, TOOLS_SEARCH_NAME,
        TOOLS_SELECT_NAME,
    },
    extension::{
        fs::{EditFileTool, ReadFileTool, SearchFileTool, WriteFileTool},
        note::NoteTool,
        shell::{ShellTool, ShellToolHook},
        skill::SkillManager,
        todo::TodoTool,
    },
    hook::DynAgentHook,
    memory::{Conversation, ConversationRef, ConversationStatus},
    subagent::SubAgentManager,
    unix_ms,
};
use anda_kip::Response;
use futures::future::join_all;
use ic_auth_types::Xid;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

mod instructions;
mod meta;
mod runner;
mod session;
mod startup;

pub use session::{SessionRequestMeta, SessionState, SessionSummary};

use instructions::available_tool_names;
use meta::{
    conversation_chat_history, conversation_extra_without_id, is_terminal_conversation_status,
    request_meta_for_conversation, scoped_external_user_name_from_meta,
    should_continue_conversation,
};
use session::{ConversationInput, Session};

use super::{
    ActionEvent, ActionRuntime, ActionSession, AskUserChoiceTool, CompletionHook, McpServerTool,
    browser::ChromeBrowserTool,
    conversation::{AgentInfo, ConversationsTool, RequestState, SourceState},
    goal::{self, GoalTool, GoalToolState},
    idle::{IDLE_CHECK_INTERVAL, IDLE_HOOK_THRESHOLD_MS, IdleHook, IdleTracker},
    multimodal,
    prompt::{PromptCommand, skill_subagent},
    resources::ResourceStore,
    side,
    skill_library::SkillLibrary,
    system::{SYSTEM_PERSON_NAME, system_extra_user_context},
};
use crate::{
    brain, channel, cron, transcription::TranscriptionManager, tts::TtsManager,
    util::request_meta::request_meta_extra_as,
};

#[derive(Clone)]
pub struct AndaBot {
    inner: Arc<AndaBotInner>,
}

struct AndaBotInner {
    brain: brain::Client,
    actions: Arc<ActionRuntime>,
    conversations: Arc<ConversationsTool>,
    resource_store: Arc<ResourceStore>,
    tool_dependencies: Vec<String>,
    tools: Vec<String>,
    sessions: ActiveSessions,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    idle_hooks: Vec<Arc<dyn IdleHook>>,
    home_dir: PathBuf,
    skill_library: Arc<SkillLibrary>,
    browser_manager: Arc<ChromeBrowserTool>,
    transcription_manager: Option<Arc<TranscriptionManager>>,
    active_im_channels: HashSet<String>,
    merge_discovered_tools_cache: RwLock<HashMap<String, bool>>,
    session_creation_lock: tokio::sync::Mutex<()>,
}

type ActiveSessions = RwLock<HashMap<Xid, Arc<Session>>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AndaBotStatus {
    pub conversations: u64,
    pub memory_nodes: u64,
    pub memory_links: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum AndaBotToolArgs {
    /// List currently active in-memory sessions.
    ListSessions {},
    /// Get one currently active in-memory session by session id.
    GetSession { session_id: String },
    /// List all available skills.
    ListSkills {},
}

fn anda_bot_tool_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "enum": ["ListSessions", "GetSession", "ListSkills"],
                "description": "The API operation to perform. Use ListSessions to list active sessions, GetSession to inspect one session, or ListSkills to list available skills."
            },
            "session_id": {
                "type": ["string", "null"],
                "description": "The active session id to inspect. Required for GetSession."
            }
        },
        "required": ["type", "session_id"],
        "additionalProperties": false
    })
}

fn base_tool_dependencies() -> Vec<String> {
    let mut tools = vec![
        brain::Client::NAME.to_string(),
        NoteTool::NAME.to_string(),
        GoalTool::NAME.to_string(),
        TOOLS_SEARCH_NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        TOOLS_GROUPS_NAME.to_string(),
        ShellTool::NAME.to_string(),
        AskUserChoiceTool::NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
        EditFileTool::NAME.to_string(),
        WriteFileTool::NAME.to_string(),
        TodoTool::NAME.to_string(),
        McpServerTool::NAME.to_string(),
        SubAgentManager::NAME.to_string(),
        SkillManager::NAME.to_string(),
        cron::CreateCronTool::NAME.to_string(),
        cron::UpdateCronJobTool::NAME.to_string(),
        cron::ManageCronJobTool::NAME.to_string(),
        cron::ListCronJobsTool::NAME.to_string(),
        cron::ListCronRunsTool::NAME.to_string(),
    ];
    tools.extend(
        ChromeBrowserTool::dependency_tool_names()
            .into_iter()
            .map(str::to_string),
    );
    tools.extend(multimodal::media_agent_names());
    tools
}

fn base_tools() -> Vec<String> {
    vec![
        brain::Client::NAME.to_string(),
        NoteTool::NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        TOOLS_GROUPS_NAME.to_string(),
        TodoTool::NAME.to_string(),
        ShellTool::NAME.to_string(),
        AskUserChoiceTool::NAME.to_string(),
        SubAgentManager::NAME.to_string(),
        SkillManager::NAME.to_string(),
    ]
}

impl AndaBot {
    pub const NAME: &'static str = "anda_bot";

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        brain: brain::Client,
        home_dir: PathBuf,
        conversations: Arc<ConversationsTool>,
        resource_store: Arc<ResourceStore>,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
        idle_hooks: Vec<Arc<dyn IdleHook>>,
        skill_library: Arc<SkillLibrary>,
        browser_manager: Arc<ChromeBrowserTool>,
        tts_manager: Option<Arc<TtsManager>>,
        transcription_manager: Option<Arc<TranscriptionManager>>,
        active_im_channels: Vec<String>,
    ) -> Self {
        let mut tool_dependencies = base_tool_dependencies();
        if tts_manager.is_some() {
            tool_dependencies.push(TtsManager::NAME.to_string());
        }
        if transcription_manager.is_some() {
            tool_dependencies.push(TranscriptionManager::NAME.to_string());
        }
        if !active_im_channels.is_empty() {
            tool_dependencies.push(channel::SendImMessageTool::NAME.to_string());
            tool_dependencies.push(channel::ListImChannelsTool::NAME.to_string());
        }
        let actions = Arc::new(ActionRuntime::new());

        Self {
            inner: Arc::new(AndaBotInner {
                brain,
                actions,
                home_dir,
                conversations,
                resource_store,
                tool_dependencies,
                tools: base_tools(),
                sessions: RwLock::new(HashMap::new()),
                completion_hooks: Arc::new(completion_hooks),
                idle_hooks,
                skill_library,
                browser_manager,
                transcription_manager,
                active_im_channels: active_im_channels.into_iter().collect(),
                merge_discovered_tools_cache: RwLock::new(HashMap::new()),
                session_creation_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    pub async fn status(&self) -> Result<AndaBotStatus, BoxError> {
        let conversations = self.inner.conversations.conversations.conversations.len() as u64;
        let bs = self.inner.brain.brain_status().await?;
        Ok(AndaBotStatus {
            conversations,
            memory_nodes: bs.concepts as u64,
            memory_links: bs.propositions as u64,
        })
    }

    pub(crate) fn action_runtime(&self) -> Arc<ActionRuntime> {
        self.inner.actions.clone()
    }

    fn insert_session(&self, task: Arc<Session>) {
        self.inner.sessions.write().insert(task.id.clone(), task);
    }

    fn get_session(&self, key: &Xid) -> Option<Arc<Session>> {
        let mut sessions = self.inner.sessions.write();
        sessions.retain(|_, task| !task.sender.is_closed());
        sessions.get(key).cloned()
    }

    fn get_session_by_source(&self, source_key: &str) -> Option<Arc<Session>> {
        let mut sessions = self.inner.sessions.write();
        sessions.retain(|_, task| !task.sender.is_closed());
        sessions
            .values()
            .find(|session| session.source_key == source_key)
            .cloned()
    }

    fn detach_session(&self, key: &Xid) -> Option<Arc<Session>> {
        self.inner.sessions.write().remove(key)
    }

    // A live session can itself be idle: its completion runner has no
    // pending work and no background tasks are running. The bot is busy only
    // while some session has work in flight.
    fn has_busy_sessions(&self) -> bool {
        let mut sessions = self.inner.sessions.write();
        sessions.retain(|_, session| !session.sender.is_closed());
        sessions.values().any(|session| !session.is_idle())
    }

    // Samples the sessions and invokes the idle hooks once the bot has been
    // fully idle for the threshold: every live session is idle, so no
    // foreground turns and no background tasks are running.
    fn spawn_idle_monitor(&self) {
        if self.inner.idle_hooks.is_empty() {
            return;
        }

        let this = self.clone();
        tokio::spawn(async move {
            let mut tracker = IdleTracker::new(IDLE_HOOK_THRESHOLD_MS);
            loop {
                tokio::time::sleep(IDLE_CHECK_INTERVAL).await;
                if let Some(idle_ms) = tracker.observe(this.has_busy_sessions(), unix_ms()) {
                    for hook in this.inner.idle_hooks.iter() {
                        hook.on_idle(idle_ms).await;
                    }
                }
            }
        });
    }

    fn active_sessions(&self) -> Vec<Arc<Session>> {
        let mut sessions = self.inner.sessions.write();
        sessions.retain(|_, session| !session.sender.is_closed());
        // Snapshot `active_at` once per session up front. It is a shared atomic that running
        // session tasks update concurrently; reading it inside the comparator would let the
        // observed order change mid-sort, which makes the comparison a non-total order and
        // panics with "comparison function does not correctly implement a total order".
        let mut active = sessions
            .values()
            .map(|session| (session.active_at.load(Ordering::SeqCst), session.clone()))
            .collect::<Vec<_>>();
        drop(sessions);

        active.sort_by(|(a_active_at, a), (b_active_at, b)| {
            b_active_at.cmp(a_active_at).then_with(|| a.id.cmp(&b.id))
        });
        active.into_iter().map(|(_, session)| session).collect()
    }

    fn session_summaries(&self, now_ms: u64) -> Vec<SessionSummary> {
        self.active_sessions()
            .into_iter()
            .map(|session| session.summary(now_ms))
            .collect()
    }

    fn session_state_by_id(&self, session_id: &str, now_ms: u64) -> Option<SessionState> {
        self.active_sessions()
            .into_iter()
            .find(|session| session.id.to_string() == session_id)
            .map(|session| session.state(now_ms))
    }

    async fn persist_conversation_state(&self, conversation: &Conversation) {
        match conversation.to_changes() {
            Ok(changes) => {
                let _ = self
                    .inner
                    .conversations
                    .conversations
                    .update_conversation(conversation._id, changes)
                    .await;
            }
            Err(err) => {
                log::error!(
                    "Failed to serialize conversation {} changes: {:?}",
                    conversation._id,
                    err
                );
            }
        }
    }

    async fn persist_resources_for_message(
        &self,
        user: &Principal,
        resources: Vec<Resource>,
    ) -> Result<Vec<Resource>, BoxError> {
        self.inner
            .resource_store
            .persist_resources(user, resources)
            .await
    }

    async fn complete_conversation_if_unfinished(
        &self,
        conversation: &mut Conversation,
        now_ms: u64,
    ) {
        if is_terminal_conversation_status(&conversation.status) {
            return;
        }

        conversation.status = ConversationStatus::Completed;
        conversation.updated_at = now_ms;
        self.persist_conversation_state(conversation).await;
    }

    async fn submit_formation(
        &self,
        messages: &[Message],
        context: &Option<InputContext>,
        timestamp: &Option<String>,
    ) -> Result<(), BoxError> {
        if messages.is_empty() {
            return Ok(());
        }

        let _ = self
            .inner
            .brain
            .formation(FormationInputRef {
                messages,
                context,
                timestamp,
            })
            .await?;
        Ok(())
    }

    async fn run_side_command(
        &self,
        ctx: &AgentCtx,
        instructions: String,
        prompt: String,
        resources: Vec<Resource>,
        conversation: Option<u64>,
    ) -> Result<AgentOutput, BoxError> {
        let subagent = side::side_agent(instructions);
        let (resources, media_usage) = multimodal::understand_media_resources(ctx, resources).await;
        let mut output = subagent
            .run(
                ctx.child(&subagent.name, super::ACTIVE_MODEL_LABEL)?,
                prompt,
                resources,
            )
            .await?;

        output.usage.accumulate(&media_usage);
        output.conversation = conversation;
        self.dispatch_direct_output(ctx, &output).await;
        Ok(output)
    }

    async fn dispatch_direct_output(&self, ctx: &AgentCtx, output: &AgentOutput) {
        if output.conversation.is_none() || output.content.is_empty() {
            return;
        }

        join_all(
            self.inner
                .completion_hooks
                .iter()
                .map(|hook| hook.on_completion(ctx, output)),
        )
        .await;
    }

    async fn latest_conversation_in_chain(
        &self,
        conv_id: u64,
        user: Option<Principal>,
    ) -> Result<Conversation, BoxError> {
        let mut seen = HashSet::new();
        let mut next_id = Some(conv_id);
        let mut latest = None;

        while let Some(id) = next_id {
            if !seen.insert(id) {
                log::warn!(conversation = id; "conversation child chain contains a cycle");
                break;
            }
            if seen.len() > 256 {
                log::warn!(conversation = conv_id; "conversation child chain is too long");
                break;
            }

            let conversation = self
                .inner
                .conversations
                .conversations
                .get_conversation(id)
                .await?;
            if let Some(u) = &user
                && &conversation.user != u
            {
                break;
            }
            next_id = conversation.child;
            latest = Some(conversation);
        }

        latest.ok_or_else(|| format!("conversation not found: {conv_id}").into())
    }
}

impl Tool<BaseCtx> for AndaBot {
    type Args = AndaBotToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        "anda_bot_api".to_string()
    }

    fn description(&self) -> String {
        "Client API for inspecting currently active AndaBot sessions, including goals and background tasks."
            .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: Tool::name(self),
            description: Tool::description(self),
            parameters: anda_bot_tool_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let now_ms = unix_ms();
        let result = match args {
            AndaBotToolArgs::ListSessions {} => json!(self.session_summaries(now_ms)),
            AndaBotToolArgs::GetSession { session_id } => {
                let Some(state) = self.session_state_by_id(&session_id, now_ms) else {
                    return Err(format!("session not found: {session_id}").into());
                };
                json!(state)
            }
            AndaBotToolArgs::ListSkills {} => {
                json!(self.inner.skill_library.prompt_skills())
            }
        };

        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

/// Implementation of the [`Agent`] trait for AndaBot.
impl Agent<AgentCtx> for AndaBot {
    /// Returns the agent's name identifier
    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    /// Returns a description of the agent's purpose and capabilities.
    fn description(&self) -> String {
        "anda_bot".to_string()
    }

    /// Returns a list of tool names that this agent depends on
    fn tool_dependencies(&self) -> Vec<String> {
        self.inner.tool_dependencies.clone()
    }

    fn supported_resource_tags(&self) -> Vec<String> {
        let mut tags = vec!["text".to_string(), "md".to_string()];
        tags.extend(multimodal::supported_media_resource_tags());
        if self.inner.transcription_manager.is_some() {
            tags.extend(crate::transcription::supported_audio_resource_tags());
        }
        tags
    }

    async fn init(&self, ctx: AgentCtx) -> Result<(), BoxError> {
        self.spawn_idle_monitor();

        let this = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            if let Err(err) = this.startup_self_check(ctx).await {
                log::error!("startup self-check failed: {err}");
            }
        });
        Ok(())
    }

    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String,
        resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let caller = ctx.caller();
        if caller == &ANONYMOUS {
            return Err("anonymous caller not allowed".into());
        }

        let has_resources = !resources.is_empty();
        let command = match PromptCommand::from(prompt) {
            PromptCommand::Ping if has_resources => PromptCommand::Plain {
                prompt: String::new(),
            },
            PromptCommand::New { prompt: None } if has_resources => PromptCommand::New {
                prompt: Some(String::new()),
            },
            command => command,
        };
        if let PromptCommand::Invalid { reason } = &command {
            return Err(reason.clone().into());
        }

        let now_ms = unix_ms();
        let home_dir = self.inner.home_dir.to_string_lossy().to_string();
        let available_tools = available_tool_names(&ctx).await;

        ctx.base.set_state(AgentInfo {
            name: Self::NAME.to_string(),
        });

        if let PromptCommand::Side { prompt } = &command {
            let RequestState {
                workspace,
                conversation: maybe_conv_id,
                ..
            } = self.inner.conversations.state_from_meta(ctx.meta());
            let instructions = self
                .build_system_instructions(&ctx, &home_dir, &workspace, &available_tools, now_ms)
                .await?;
            let side_conversation_id = if maybe_conv_id > 0 {
                self.latest_conversation_in_chain(maybe_conv_id, Some(*caller))
                    .await
                    .ok()
                    .map(|conv| conv._id)
            } else {
                None
            };
            return self
                .run_side_command(
                    &ctx,
                    instructions,
                    prompt.clone(),
                    resources,
                    side_conversation_id,
                )
                .await;
        }

        let RequestState {
            workspace,
            source_key,
            source_state,
            conversation: maybe_conv_id,
            ..
        } = self.inner.conversations.state_from_meta(ctx.meta());

        let mut ancestors: Option<Vec<u64>> = None;
        let mut current_conversation = if maybe_conv_id > 0 {
            self.latest_conversation_in_chain(maybe_conv_id, Some(*caller))
                .await
                .ok()
        } else {
            None
        };

        if let Some(conv) = &current_conversation {
            let mut ids = conv.ancestors.clone().unwrap_or_default();
            ids.push(conv._id);
            if ids.len() > 10 {
                ids.drain(0..ids.len() - 10);
            }
            ancestors = Some(ids);
        }

        let mut input = ConversationInput {
            command,
            resources,
            extra: ctx.meta().extra.clone(),
            usage: Usage::default(),
        };
        let current_conversation_id = current_conversation.as_ref().map(|conv| conv._id);
        if let Some(id) = current_conversation_id {
            input.extra.insert("conversation".to_string(), id.into());
        }

        let mut sess_id = current_conversation
            .as_ref()
            .and_then(|conv| conv.thread.clone())
            .unwrap_or_else(Xid::new);
        let mut detached_existing_session = false;
        let mut detached_conversation_id = current_conversation_id.unwrap_or_default();

        // The brain lookups behind build_system_instructions (primer + user
        // profile) are network calls and can be slow, so they must not run
        // while holding the session creation lock: that would block message
        // delivery to every already-running session. Check for a joinable
        // session under the lock, release it to build instructions, then
        // re-check before creating the session so concurrent requests for the
        // same source cannot create duplicate sessions.
        let mut instructions: Option<String> = None;
        let _session_creation_guard = loop {
            let guard = self.inner.session_creation_lock.lock().await;
            let active_session = self
                .get_session(&sess_id)
                .or_else(|| self.get_session_by_source(&source_key));
            if let Some(session) = active_session {
                if matches!(input.command, PromptCommand::New { .. }) {
                    detached_conversation_id = session.conversation_id.load(Ordering::SeqCst);
                    if let Some(session) = self.detach_session(&session.id) {
                        session.finish_when_idle.store(true, Ordering::SeqCst);
                        detached_existing_session = true;
                    }
                    if Some(detached_conversation_id) != current_conversation_id {
                        // fetch the latest ancestors in session for /new command
                        if let Ok(conv) = self
                            .latest_conversation_in_chain(detached_conversation_id, Some(*caller))
                            .await
                        {
                            let mut ids = conv.ancestors.clone().unwrap_or_default();
                            ids.push(conv._id);
                            if ids.len() > 10 {
                                ids.drain(0..ids.len() - 10);
                            }
                            ancestors = Some(ids);
                        }
                    }
                } else {
                    // Join existing conversation session if it's active.
                    // Release the lock first: enqueueing can wait on a full
                    // channel and must not stall unrelated requests.
                    drop(guard);
                    let response_conversation_id = session.conversation_id.load(Ordering::SeqCst);
                    let meta = request_meta_for_conversation(ctx.meta(), response_conversation_id);
                    session.request_meta.set(meta);
                    match session.sender.send(input).await {
                        Ok(_) => {
                            return Ok(AgentOutput {
                                conversation: (response_conversation_id > 0)
                                    .then_some(response_conversation_id),
                                session: Some(session.id.to_string()),
                                ..Default::default()
                            });
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to enqueue prompt for processing conversation {}",
                                maybe_conv_id,
                            );
                            self.detach_session(&session.id);
                            input = err.0;
                        }
                    }
                    continue;
                }
            }

            if instructions.is_none() {
                drop(guard);
                instructions = Some(
                    self.build_system_instructions(
                        &ctx,
                        &home_dir,
                        &workspace,
                        &available_tools,
                        now_ms,
                    )
                    .await?,
                );
                continue;
            }

            break guard;
        };

        // If the conversation session is not active, start a new session and process the prompt
        let ConversationInput {
            command,
            resources,
            extra,
            ..
        } = input;
        let mut instructions =
            instructions.expect("system instructions are built before session creation");

        let mut initial_goal = None;
        let mut tools = UniqueVec::from(self.inner.tools.clone());
        let mut force_standalone_conversation = false;
        let prompt = match command {
            PromptCommand::Plain { prompt }
            | PromptCommand::Steer { prompt }
            | PromptCommand::Loop { prompt } => prompt,
            PromptCommand::Goal { prompt } => {
                initial_goal = Some(prompt.clone());
                tools.push(GoalTool::NAME.to_string());
                prompt
            }
            PromptCommand::Ping => return Err("prompt cannot be empty".into()),
            PromptCommand::Stop { .. } => {
                return Err("/stop requires an active conversation".into());
            }
            PromptCommand::Cancel { .. } => {
                return Err("/cancel requires an active conversation".into());
            }
            PromptCommand::Skill { skill, prompt } => {
                if let Some(subagent) = skill_subagent(self.inner.skill_library.as_ref(), &skill) {
                    instructions = format!(
                        "{instructions}\n\nUse the {} skill to handle user's request",
                        subagent.name
                    );
                    tools.push(subagent.name);
                }

                prompt
            }
            PromptCommand::Invalid { reason } => return Err(reason.into()),
            PromptCommand::Side { .. } => unreachable!(),
            PromptCommand::New { prompt } => {
                if !detached_existing_session
                    && let Some(conversation) = current_conversation.as_mut()
                {
                    self.complete_conversation_if_unfinished(conversation, now_ms)
                        .await;
                }

                let Some(prompt) = prompt else {
                    if detached_conversation_id > 0
                        && source_state.conv_id != detached_conversation_id
                        && let Err(err) = self
                            .inner
                            .conversations
                            .update_source_state(
                                source_key.clone(),
                                SourceState {
                                    conv_id: detached_conversation_id,
                                    status: ConversationStatus::Cancelled,
                                    timestamp: now_ms,
                                },
                            )
                            .await
                    {
                        log::error!("Failed to update_source_state: {:?}", err);
                    }

                    return Ok(AgentOutput {
                        conversation: (detached_conversation_id > 0)
                            .then_some(detached_conversation_id),
                        ..Default::default()
                    });
                };

                force_standalone_conversation = true;
                current_conversation = None;
                sess_id = Xid::new();
                prompt
            }
        };

        let mut chat_history: Vec<Message> = Vec::new();
        let mut reserve_chat_history: Vec<Message> = Vec::new();
        let mut new_chat_history_message = Message {
            role: "user".into(),
            content: vec![],
            name: Some(SYSTEM_PERSON_NAME.into()),
            timestamp: Some(now_ms),
            ..Default::default()
        };

        let should_continue = !force_standalone_conversation
            && current_conversation
                .as_ref()
                .map(|conv| should_continue_conversation(&conv.status))
                .unwrap_or(false);

        let conversation = if should_continue && let Some(conv) = current_conversation {
            // 如果 conversation 已经存在，允许 prompt 为空（会进入等待模式）
            reserve_chat_history = conversation_chat_history(&conv);
            chat_history = reserve_chat_history.clone();
            conv
        } else {
            if prompt.trim().is_empty() && !has_resources {
                return Err("prompt cannot be empty".into());
            }

            if !force_standalone_conversation {
                let (mut history_conversations, _) = self
                    .inner
                    .conversations
                    .conversations
                    .list_conversations_by_user(caller, None, Some(2))
                    .await?;

                if let Some(conv) = &current_conversation
                    && !history_conversations.iter().any(|c| c._id == conv._id)
                {
                    history_conversations.push(conv.clone());
                }

                if !history_conversations.is_empty() {
                    new_chat_history_message.content.push(
                        Documents::new(
                            "user_history_conversations".to_string(),
                            history_conversations
                                .into_iter()
                                .map(Document::from)
                                .collect(),
                        )
                        .to_string()
                        .into(),
                    );
                }
            }

            let mut conv = Conversation {
                user: *caller,
                thread: Some(sess_id.clone()),
                messages: vec![],
                ancestors,
                resources: vec![],
                period: now_ms / 3600 / 1000,
                created_at: now_ms,
                updated_at: now_ms,
                extra: Some(if force_standalone_conversation {
                    json!(conversation_extra_without_id(ctx.meta()))
                } else {
                    json!(ctx.meta().extra)
                }),
                ..Default::default()
            };

            let conv_id = self
                .inner
                .conversations
                .conversations
                .add_conversation(ConversationRef::from(&conv))
                .await?;

            if !force_standalone_conversation
                && let Some(mut conversation) = current_conversation
                && conversation.child.is_none()
            {
                conversation.child = Some(conv_id);
                conversation.updated_at = now_ms;
                if conversation.status == ConversationStatus::Failed {
                    conversation.status = ConversationStatus::Completed;
                }
                self.persist_conversation_state(&conversation).await;
            }

            conv._id = conv_id;
            conv
        };

        if source_state.conv_id != conversation._id {
            // Update the mapping of source to conv_id if it's different from the current one.
            if let Err(err) = self
                .inner
                .conversations
                .update_source_state(
                    source_key.clone(),
                    SourceState {
                        conv_id: conversation._id,
                        status: conversation.status.clone(),
                        timestamp: now_ms,
                    },
                )
                .await
            {
                log::error!("Failed to update_source_state: {:?}", err);
            }
        }

        let res = AgentOutput {
            conversation: Some(conversation._id),
            ..Default::default()
        };

        let (sender, rx) = tokio::sync::mpsc::channel::<ConversationInput>(42);
        let (action_sender, action_rx) = tokio::sync::mpsc::channel::<ActionEvent>(42);
        let session_request_meta =
            SessionRequestMeta::new(request_meta_for_conversation(ctx.meta(), conversation._id));
        let external_user =
            request_meta_extra_as::<bool>(ctx.meta(), "external_user").unwrap_or(false);
        let formation_counterparty = if external_user {
            Some(scoped_external_user_name_from_meta(ctx.meta()))
        } else {
            Some(caller.to_string())
        };

        let conversation_id = Arc::new(AtomicU64::new(conversation._id));
        let session_id = sess_id.to_string();
        let session = Arc::new(Session {
            id: sess_id,
            caller: caller.to_string(),
            workspace,
            source_key: source_key.clone(),
            conversation_id: conversation_id.clone(),
            sender,
            actions: ActionSession::new(
                self.inner.actions.clone(),
                action_sender,
                caller.to_string(),
                session_id,
                conversation_id,
            ),
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            background_progress_outputs: Arc::new(RwLock::new(HashMap::new())),
            goal: Arc::new(RwLock::new(initial_goal.map(goal::GoalState::new))),
            request_meta: session_request_meta.clone(),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            formation_backoff_until: AtomicU64::new(0),
            goal_check_backoff_until: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(unix_ms())),
            finish_when_idle: AtomicBool::new(false),
            runner_idle: AtomicBool::new(false),
            formation_context: Some(InputContext {
                counterparty: formation_counterparty,
                agent: Some(AndaBot::NAME.to_string()),
                source: Some(source_key),
                topic: None,
            }),
        });

        ctx.base.set_state(GoalToolState::new(
            session.goal.clone(),
            session.active_at.clone(),
        ));
        ctx.base.set_state(session_request_meta);
        ctx.base.set_state(session.actions.clone());

        let agent_hook = DynAgentHook::new(session.clone());
        ctx.base.set_state(agent_hook);

        let shell_hook = ShellToolHook::new(session.clone());
        ctx.base.set_state(shell_hook);

        self.insert_session(session.clone());

        let assistant = self.clone();
        if !new_chat_history_message.content.is_empty() {
            chat_history.push(new_chat_history_message);
        };

        if assistant.inner.browser_manager.is_active() {
            tools.extend(
                ChromeBrowserTool::active_tool_names()
                    .into_iter()
                    .map(str::to_string),
            );
        }

        tools.extend(
            assistant.inner.conversations.tool_usage_with(|usage| {
                select_most_used_tools(&available_tools, &tools, usage, 3)
            }),
        );
        let req = CompletionRequest {
            instructions,
            prompt,
            chat_history,
            tools: ctx.definitions(Some(&tools)).await,
            tool_choice_required: false,
            ..Default::default()
        };

        assistant.spawn_session_runner(
            ctx,
            req,
            resources,
            reserve_chat_history,
            session,
            conversation,
            rx,
            action_rx,
            system_extra_user_context(&extra),
        );
        Ok(res)
    }
}

impl AndaBotInner {
    fn apply_merge_discovered_tools(&self, runner: &mut CompletionRunner) {
        if let Some(merge_discovered_tools) =
            self.merge_discovered_tools_for_model(&runner.model().model_name())
        {
            runner.set_merge_discovered_tools(Some(merge_discovered_tools));
        }
    }

    fn cache_merge_discovered_tools(&self, runner: &CompletionRunner) {
        let Some(merge_discovered_tools) = runner.merge_discovered_tools() else {
            return;
        };
        let Some(model_name) = merge_discovered_tools_model_key(&runner.model().model_name())
        else {
            return;
        };

        self.merge_discovered_tools_cache
            .write()
            .insert(model_name, merge_discovered_tools);
    }

    fn merge_discovered_tools_for_model(&self, model_name: &str) -> Option<bool> {
        merge_discovered_tools_for_model_cache(&self.merge_discovered_tools_cache, model_name)
    }
}

fn merge_discovered_tools_for_model_cache(
    cache: &RwLock<HashMap<String, bool>>,
    model_name: &str,
) -> Option<bool> {
    known_merge_discovered_tools_for_model(model_name).or_else(|| {
        let key = merge_discovered_tools_model_key(model_name)?;
        cache.read().get(&key).copied()
    })
}

fn known_merge_discovered_tools_for_model(model_name: &str) -> Option<bool> {
    let model_name = merge_discovered_tools_model_key(model_name)?;
    if model_name.contains("deepseek") {
        Some(false)
    } else if model_name.starts_with("gpt")
        || model_name.contains("/gpt")
        || model_name.contains("chatgpt")
    {
        Some(true)
    } else {
        None
    }
}

fn merge_discovered_tools_model_key(model_name: &str) -> Option<String> {
    let model_name = model_name.trim().to_ascii_lowercase();
    if model_name.is_empty() {
        None
    } else {
        Some(model_name)
    }
}

fn select_most_used_tools(
    available_tools: &[String],
    base_tools: &[String],
    tools_usage: &HashMap<String, Usage>,
    limit: usize,
) -> Vec<String> {
    let available: HashSet<&str> = available_tools.iter().map(String::as_str).collect();
    let existing: HashSet<&str> = base_tools.iter().map(String::as_str).collect();
    let mut ranked: Vec<(&String, &Usage)> = tools_usage
        .iter()
        .filter(|(tool, _)| {
            let tool = tool.as_str();
            available.contains(tool) && !existing.contains(tool)
        })
        .collect();

    ranked.sort_unstable_by(|(tool_a, usage_a), (tool_b, usage_b)| {
        usage_b
            .requests
            .cmp(&usage_a.requests)
            .then_with(|| tool_a.cmp(tool_b))
    });

    ranked
        .into_iter()
        .take(limit)
        .map(|(tool, _)| tool.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::Usage;
    use std::collections::HashMap;

    #[test]
    fn anda_bot_api_schema_is_openai_strict() {
        assert_openai_strict_parameters(&anda_bot_tool_parameters());
    }

    #[test]
    fn anda_bot_tool_args_parse_tagged_variants() {
        let args: AndaBotToolArgs = serde_json::from_value(serde_json::json!({
            "type": "ListSessions",
            "session_id": null,
        }))
        .expect("list sessions variant should parse");

        assert_eq!(args, AndaBotToolArgs::ListSessions {});

        let args: AndaBotToolArgs = serde_json::from_value(serde_json::json!({
            "type": "ListSkills",
        }))
        .expect("list skills variant should parse");

        assert_eq!(args, AndaBotToolArgs::ListSkills {});

        let args: AndaBotToolArgs = serde_json::from_value(serde_json::json!({
            "type": "GetSession",
            "session_id": "session-1",
        }))
        .expect("get session variant should parse");

        assert_eq!(
            args,
            AndaBotToolArgs::GetSession {
                session_id: "session-1".to_string(),
            }
        );
    }

    #[test]
    fn anda_bot_tool_args_reject_missing_session_id() {
        let err = serde_json::from_value::<AndaBotToolArgs>(serde_json::json!({
            "type": "GetSession",
        }))
        .expect_err("get session requires session_id");

        assert!(err.to_string().contains("session_id"));
    }

    #[test]
    fn base_agent_tools_include_some_tools() {
        assert!(base_tool_dependencies().contains(&GoalTool::NAME.to_string()));
        assert!(!base_tools().contains(&GoalTool::NAME.to_string()));
        assert!(base_tool_dependencies().contains(&McpServerTool::NAME.to_string()));
        assert!(base_tools().contains(&McpServerTool::NAME.to_string()));
    }

    #[test]
    fn known_merge_discovered_tools_policy_covers_deepseek_and_gpt_models() {
        assert_eq!(
            known_merge_discovered_tools_for_model("deepseek-v4-pro"),
            Some(false)
        );
        assert_eq!(
            known_merge_discovered_tools_for_model("openai/gpt-5.4"),
            Some(true)
        );
        assert_eq!(
            known_merge_discovered_tools_for_model("chatgpt-codex"),
            Some(true)
        );
        assert_eq!(known_merge_discovered_tools_for_model("gemini-3-pro"), None);
    }

    #[test]
    fn merge_discovered_tools_cache_reuses_unknown_model_probe_result() {
        let cache = RwLock::new(HashMap::from([("custom-model".to_string(), true)]));

        assert_eq!(
            merge_discovered_tools_for_model_cache(&cache, "CUSTOM-MODEL"),
            Some(true)
        );
        assert_eq!(
            merge_discovered_tools_for_model_cache(&cache, "unknown-model"),
            None
        );
    }

    #[test]
    fn select_most_used_tools_prefers_high_request_tools() {
        let available_tools = vec![
            "shell".to_string(),
            "read_file".to_string(),
            "todo".to_string(),
            "search".to_string(),
        ];
        let base_tools = vec!["shell".to_string()];
        let tools_usage = HashMap::from([
            (
                "shell".to_string(),
                Usage {
                    requests: 99,
                    ..Default::default()
                },
            ),
            (
                "todo".to_string(),
                Usage {
                    requests: 8,
                    ..Default::default()
                },
            ),
            (
                "read_file".to_string(),
                Usage {
                    requests: 10,
                    ..Default::default()
                },
            ),
            (
                "unavailable".to_string(),
                Usage {
                    requests: 100,
                    ..Default::default()
                },
            ),
        ]);

        let selected = select_most_used_tools(&available_tools, &base_tools, &tools_usage, 2);

        assert_eq!(selected, vec!["read_file".to_string(), "todo".to_string()]);
    }

    use crate::engine::ACTIVE_MODEL_LABEL;
    use crate::engine::browser::BrowserBridge;
    use crate::engine::multimodal::MediaUnderstandingAgent;
    use crate::engine::resources::ResourceStore;
    use crate::util::http_client::new_reqwest_client;
    use anda_core::{AgentInput, RequestMeta};
    use anda_db::{database::DBConfig, storage::StorageConfig};
    use anda_engine::{
        engine::{AgentInfo, Engine, EngineRef},
        management::{BaseManagement, Visibility},
        memory::Conversations,
        model::Model,
    };
    use anda_kip::Response as KipResp;
    use axum::{Router, routing};
    use object_store::memory::InMemory;
    use std::collections::BTreeSet;

    struct FakeShellTool;

    impl Tool<BaseCtx> for FakeShellTool {
        type Args = Value;
        type Output = Value;

        fn name(&self) -> String {
            ShellTool::NAME.to_string()
        }

        fn description(&self) -> String {
            "fake shell".to_string()
        }

        fn definition(&self) -> FunctionDefinition {
            FunctionDefinition {
                name: self.name(),
                description: self.description(),
                parameters: json!({"type": "object"}),
                strict: Some(false),
            }
        }

        async fn call(
            &self,
            _ctx: BaseCtx,
            _args: Self::Args,
            _resources: Vec<Resource>,
        ) -> Result<ToolOutput<Self::Output>, BoxError> {
            Ok(ToolOutput::new(json!({"ok": true})))
        }
    }

    async fn build_test_db() -> Arc<anda_db::database::AndaDB> {
        let object_store: Arc<dyn object_store::ObjectStore> = Arc::new(InMemory::new());
        Arc::new(
            anda_db::database::AndaDB::connect(
                object_store,
                DBConfig {
                    name: "anda_bot_run_test".to_string(),
                    description: "run test".to_string(),
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
            .unwrap(),
        )
    }

    async fn spawn_brain_mock() -> String {
        let app = Router::new()
            .route(
                "/v1/anda_bot/execute_kip_readonly",
                routing::post(|| async {
                    axum::Json(
                        serde_json::to_value(KipResp::ok(json!({"identity": "panda"}))).unwrap(),
                    )
                }),
            )
            .route(
                "/v1/anda_bot/get_or_init_user",
                routing::post(|| async { axum::Json(json!({"name": "tester"})) }),
            )
            .route(
                "/v1/anda_bot/formation",
                routing::post(|| async { axum::Json(json!({"result": {"content": ""}})) }),
            )
            .route(
                "/v1/anda_bot/formation_status",
                routing::get(|| async {
                    axum::Json(json!({
                        "result": {
                            "id": "anda_bot",
                            "concepts": 3,
                            "propositions": 5,
                            "conversations": 2,
                            "formation_processing": false,
                            "maintenance_processing": false,
                            "formation_processed_id": 0,
                            "maintenance_processed_id": 0,
                            "maintenance_at": {"daydream": 0, "full": 0, "quick": 0, "start_at": 0}
                        }
                    }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/v1/anda_bot")
    }

    async fn build_bot_engine(home: PathBuf) -> (Arc<Engine>, Arc<AndaBot>) {
        let db = build_test_db().await;
        let brain_url = spawn_brain_mock().await;
        let brain_client = brain::Client::new(brain_url, Some("token".to_string()))
            .with_http_client(new_reqwest_client());

        let conversations = Conversations::connect(db.clone(), "bot".to_string())
            .await
            .unwrap();
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let conversations_tool = Arc::new(ConversationsTool::new(
            conversations,
            home.to_string_lossy().to_string(),
        ));
        let bridge = Arc::new(BrowserBridge::new());
        let skills = SkillLibrary::for_test(home.clone());
        let mcp_provider =
            Arc::new(anda_engine::extension::mcp::McpToolProvider::new(Vec::new()).unwrap());
        let add_mcp_server = Arc::new(McpServerTool::new(
            mcp_provider.clone(),
            home.clone(),
            Some(home.join("workspace")),
            crate::config::McpSettings::file_path(&home),
            Arc::new(tokio::sync::Mutex::new(())),
        ));
        let cron_runtime = Arc::new(
            crate::cron::CronRuntime::connect(Arc::new(EngineRef::new()), db.clone())
                .await
                .unwrap(),
        );

        let bot = Arc::new(AndaBot::new(
            brain_client.clone(),
            home.clone(),
            conversations_tool.clone(),
            resource_store.clone(),
            vec![],
            vec![],
            skills.clone(),
            Arc::new(ChromeBrowserTool::tabs(bridge.clone())),
            None,
            None,
            vec![],
        ));

        let image = Arc::new(MediaUnderstandingAgent::image(vec![]));
        let audio = Arc::new(MediaUnderstandingAgent::audio(vec![]));
        let video = Arc::new(MediaUnderstandingAgent::video(vec![]));
        let other = Arc::new(MediaUnderstandingAgent::other(vec![]));

        let engine = Engine::builder()
            .with_info(AgentInfo {
                handle: "anda".to_string(),
                name: "Anda".to_string(),
                description: "test".to_string(),
                endpoint: "https://example.com/engine".to_string(),
                ..Default::default()
            })
            .with_management(Arc::new(BaseManagement {
                controller: Principal::management_canister(),
                managers: BTreeSet::new(),
                visibility: Visibility::Public,
            }))
            .with_model(Model::mock_implemented())
            .register_tool(Arc::new(brain_client.clone()))
            .unwrap()
            .register_tool(Arc::new(FakeShellTool))
            .unwrap()
            .register_tool(Arc::new(crate::engine::ActionsTool::new(
                bot.action_runtime(),
            )))
            .unwrap()
            .register_tool(Arc::new(AskUserChoiceTool))
            .unwrap()
            .register_tool(Arc::new(NoteTool::new()))
            .unwrap()
            .register_tool(Arc::new(GoalTool::new()))
            .unwrap()
            .register_tool(Arc::new(TodoTool::new()))
            .unwrap()
            .register_tool(Arc::new(ReadFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(SearchFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(EditFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(WriteFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(cron::CreateCronTool::new(cron_runtime.clone())))
            .unwrap()
            .register_tool(Arc::new(cron::ListCronJobsTool::new(cron_runtime.clone())))
            .unwrap()
            .register_tool(Arc::new(cron::UpdateCronJobTool::new(cron_runtime.clone())))
            .unwrap()
            .register_tool(Arc::new(cron::ManageCronJobTool::new(cron_runtime.clone())))
            .unwrap()
            .register_tool(Arc::new(cron::ListCronRunsTool::new(cron_runtime.clone())))
            .unwrap()
            .register_tool(Arc::new(ChromeBrowserTool::tabs(bridge.clone())))
            .unwrap()
            .register_tool(Arc::new(ChromeBrowserTool::page(bridge.clone())))
            .unwrap()
            .register_tool(Arc::new(ChromeBrowserTool::input(bridge.clone())))
            .unwrap()
            .register_tool(Arc::new(ChromeBrowserTool::script(bridge.clone())))
            .unwrap()
            .register_tool(skills.skill_manager())
            .unwrap()
            .register_tool(skills.clone())
            .unwrap()
            .register_tool(add_mcp_server)
            .unwrap()
            .register_tool(resource_store.clone())
            .unwrap()
            .register_tool(conversations_tool.clone())
            .unwrap()
            .register_tool(bot.clone())
            .unwrap()
            .register_tool_provider(mcp_provider)
            .unwrap()
            .register_agent(image, Some("image".to_string()))
            .unwrap()
            .register_agent(audio, Some("audio".to_string()))
            .unwrap()
            .register_agent(video, Some("video".to_string()))
            .unwrap()
            .register_agent(other, Some("other".to_string()))
            .unwrap()
            .register_agent(bot.clone(), Some(ACTIVE_MODEL_LABEL.to_string()))
            .unwrap()
            .export_tools(vec![
                ConversationsTool::NAME.to_string(),
                crate::engine::ActionsTool::NAME.to_string(),
                ResourceStore::NAME.to_string(),
                Tool::name(bot.as_ref()),
            ])
            .build(AndaBot::NAME.to_string())
            .await
            .unwrap();

        (Arc::new(engine), bot)
    }

    fn test_caller() -> Principal {
        Principal::from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9])
    }

    #[tokio::test]
    async fn anda_bot_run_creates_conversation_via_full_engine() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        // A non-anonymous caller drives the full AndaBot::run path: building
        // system instructions (brain mock), conversation creation (in-memory
        // DB), and spawning the session runner. The detached runner uses the
        // deterministic mock model.
        let input = AgentInput::new(AndaBot::NAME.to_string(), "hello there".to_string());
        let output = engine.agent_run(test_caller(), input).await.unwrap();
        assert!(output.conversation.is_some() || output.session.is_some());
    }

    #[tokio::test]
    async fn anda_bot_run_rejects_anonymous_caller() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let input = AgentInput::new(AndaBot::NAME.to_string(), "hi".to_string());
        let err = engine
            .agent_run(ANONYMOUS, input)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("anonymous"));
    }

    #[tokio::test]
    async fn anda_bot_run_handles_command_variants() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();

        // Each prompt exercises a different command branch of AndaBot::run.
        for prompt in [
            "/goal finish the report",
            "/side quick aside",
            "plain message",
        ] {
            let mut input = AgentInput::new(AndaBot::NAME.to_string(), prompt.to_string());
            // Distinct source keys avoid joining the same in-memory session.
            let mut meta = RequestMeta::default();
            meta.extra
                .insert("source".to_string(), json!(format!("cli:{prompt}")));
            input.meta = Some(meta);
            let result = engine.agent_run(caller, input).await;
            assert!(result.is_ok(), "command {prompt} failed: {result:?}");
        }
    }

    #[tokio::test]
    async fn anda_bot_run_rejects_control_commands_without_active_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();

        for prompt in ["/stop", "/cancel"] {
            let mut input = AgentInput::new(AndaBot::NAME.to_string(), prompt.to_string());
            let mut meta = RequestMeta::default();
            meta.extra
                .insert("source".to_string(), json!(format!("cli:ctrl:{prompt}")));
            input.meta = Some(meta);
            let err = engine
                .agent_run(caller, input)
                .await
                .map(|_| ())
                .unwrap_err();
            assert!(err.to_string().contains("requires an active conversation"));
        }
    }

    #[tokio::test]
    async fn anda_bot_status_and_api_tool_report_state() {
        let dir = tempfile::tempdir().unwrap();
        let (_engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;

        // status() queries the brain mock for memory counts.
        let status = bot.status().await.unwrap();
        assert_eq!(status.memory_nodes, 3);
        assert_eq!(status.memory_links, 5);

        // The anda_bot_api tool surfaces sessions and skills.
        let ctx = anda_engine::engine::EngineBuilder::new().mock_ctx().base;
        let sessions = Tool::call(
            bot.as_ref(),
            ctx.clone(),
            AndaBotToolArgs::ListSessions {},
            vec![],
        )
        .await
        .unwrap();
        assert!(matches!(sessions.output, Response::Ok { .. }));

        let skills = Tool::call(
            bot.as_ref(),
            ctx.clone(),
            AndaBotToolArgs::ListSkills {},
            vec![],
        )
        .await
        .unwrap();
        assert!(matches!(skills.output, Response::Ok { .. }));

        // A missing session id is reported as an error.
        let missing = Tool::call(
            bot.as_ref(),
            ctx,
            AndaBotToolArgs::GetSession {
                session_id: "nope".to_string(),
            },
            vec![],
        )
        .await;
        assert!(missing.is_err());
    }

    fn input_for_source(prompt: &str, source: &str) -> AgentInput {
        let mut input = AgentInput::new(AndaBot::NAME.to_string(), prompt.to_string());
        let mut meta = RequestMeta::default();
        meta.extra.insert("source".to_string(), json!(source));
        input.meta = Some(meta);
        input
    }

    #[tokio::test]
    async fn anda_bot_run_joins_session_and_handles_new_command() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();

        // First message creates a session for this source.
        let first = engine
            .agent_run(caller, input_for_source("first message", "cli:join"))
            .await
            .unwrap();
        assert!(first.conversation.is_some());

        // Give the detached session runner a moment to process a round so the
        // runner loop, formation submission, and persistence paths execute.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // A follow-up to the same source joins the existing session or starts a
        // fresh one; either way the run path succeeds.
        let second = engine
            .agent_run(caller, input_for_source("follow up", "cli:join"))
            .await;
        assert!(second.is_ok());

        // A /new command starts a standalone conversation.
        let new_conv = engine
            .agent_run(caller, input_for_source("/new fresh start", "cli:join"))
            .await;
        assert!(new_conv.is_ok());
    }

    #[tokio::test]
    async fn anda_bot_run_accepts_resources_and_skill_command() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();

        // A message carrying a text resource exercises the media/resource path.
        let mut with_resource = input_for_source("look at this", "cli:res");
        with_resource.resources = vec![Resource {
            name: "note.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            blob: Some(ic_auth_types::ByteBufB64(b"hello".to_vec())),
            tags: vec!["text".to_string()],
            ..Default::default()
        }];
        assert!(engine.agent_run(caller, with_resource).await.is_ok());

        // A /skill command augments instructions and tools.
        let skill = engine
            .agent_run(
                caller,
                input_for_source("/skill coder build it", "cli:skill"),
            )
            .await;
        assert!(skill.is_ok());

        // Let the detached runners settle.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    fn mock_agent_ctx() -> AgentCtx {
        anda_engine::engine::EngineBuilder::new()
            .with_model(Model::mock_implemented())
            .mock_ctx()
    }

    async fn build_test_bot_with_channels(home: PathBuf, channels: Vec<String>) -> Arc<AndaBot> {
        let db = build_test_db().await;
        let brain_url = spawn_brain_mock().await;
        let brain_client = brain::Client::new(brain_url, Some("token".to_string()))
            .with_http_client(new_reqwest_client());
        let conversations = Conversations::connect(db.clone(), "bot".to_string())
            .await
            .unwrap();
        let conversations_tool = Arc::new(ConversationsTool::new(
            conversations,
            home.to_string_lossy().to_string(),
        ));
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let bridge = Arc::new(BrowserBridge::new());
        let skills = SkillLibrary::for_test(home.clone());
        Arc::new(AndaBot::new(
            brain_client,
            home,
            conversations_tool,
            resource_store,
            vec![],
            vec![],
            skills,
            Arc::new(ChromeBrowserTool::tabs(bridge)),
            None,
            None,
            channels,
        ))
    }

    #[tokio::test]
    async fn startup_self_check_resumes_active_im_conversation() {
        let dir = tempfile::tempdir().unwrap();
        // The bot treats "telegram" as an active IM channel, so a resumable
        // telegram conversation reaches the deep continue path (instructions +
        // session runner spawn) instead of bailing at the channel check.
        let bot =
            build_test_bot_with_channels(dir.path().to_path_buf(), vec!["telegram".to_string()])
                .await;
        let now_ms = unix_ms();
        let caller = test_caller();

        let conv = Conversation {
            user: caller,
            messages: vec![json!(Message {
                role: "user".to_string(),
                content: vec![anda_core::ContentPart::Text {
                    text: "resume me".to_string()
                }],
                timestamp: Some(now_ms),
                ..Default::default()
            })],
            status: ConversationStatus::Working,
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some(json!({"workspace": dir.path().to_string_lossy(), "source": "telegram"})),
            ..Default::default()
        };
        let conv_id = bot
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conv))
            .await
            .unwrap();
        bot.inner
            .conversations
            .update_source_state(
                "telegram:reply_target:chat-9".to_string(),
                SourceState {
                    conv_id,
                    status: ConversationStatus::Working,
                    timestamp: now_ms,
                },
            )
            .await
            .unwrap();

        bot.startup_self_check(mock_agent_ctx()).await.unwrap();
        // Let the spawned session runner settle.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn startup_self_check_is_noop_with_empty_db() {
        let dir = tempfile::tempdir().unwrap();
        let (_engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        // No source-bound conversations to resume: returns Ok after scanning.
        bot.startup_self_check(mock_agent_ctx()).await.unwrap();
    }

    #[tokio::test]
    async fn startup_self_check_scans_resumable_source_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let (_engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();
        let now_ms = unix_ms();

        // Seed a recent, resumable conversation with saved history and a source
        // mapping so the startup scan finds and processes it. The bot has no
        // active IM channels, so the resume bails out before re-running, which
        // still exercises startup_source_candidates + continue_startup_conversation.
        let conv = Conversation {
            user: caller,
            messages: vec![json!(Message {
                role: "user".to_string(),
                content: vec![anda_core::ContentPart::Text {
                    text: "earlier request".to_string()
                }],
                timestamp: Some(now_ms),
                ..Default::default()
            })],
            status: ConversationStatus::Working,
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            ..Default::default()
        };
        let conv_id = bot
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conv))
            .await
            .unwrap();
        bot.inner
            .conversations
            .update_source_state(
                "telegram:reply_target:chat-1".to_string(),
                SourceState {
                    conv_id,
                    status: ConversationStatus::Working,
                    timestamp: now_ms,
                },
            )
            .await
            .unwrap();

        bot.startup_self_check(mock_agent_ctx()).await.unwrap();
    }

    #[tokio::test]
    async fn anda_bot_run_stops_and_cancels_active_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, _bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();

        // Establish an active session, then stop it; the stop is routed into the
        // running session and processed by the session runner.
        engine
            .agent_run(caller, input_for_source("start work", "cli:stop"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let stopped = engine
            .agent_run(caller, input_for_source("/stop done", "cli:stop"))
            .await;
        assert!(stopped.is_ok());

        engine
            .agent_run(caller, input_for_source("start again", "cli:cancel"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let cancelled = engine
            .agent_run(caller, input_for_source("/cancel abort", "cli:cancel"))
            .await;
        assert!(cancelled.is_ok());

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}
