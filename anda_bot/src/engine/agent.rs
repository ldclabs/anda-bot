use anda_brain::types::{FormationInputRef, InputContext};
use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, Document, Documents,
    FunctionDefinition, Message, Principal, Resource, StateFeatures, Tool, ToolOutput, Usage,
};
use anda_db_utils::UniqueVec;
use anda_engine::{
    ANONYMOUS,
    context::{AgentCtx, BaseCtx, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{EditFileTool, ReadFileTool, SearchFileTool, WriteFileTool},
        note::NoteTool,
        shell::{ShellTool, ShellToolHook},
        skill::{SkillFrontmatter, SkillManager},
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
    CompletionHook,
    browser::ChromeBrowserTool,
    conversation::{AgentInfo, ConversationsTool, RequestState, SourceState},
    goal::{self, GoalTool, GoalToolState},
    idle::{IDLE_CHECK_INTERVAL, IDLE_HOOK_THRESHOLD_MS, IdleHook, IdleTracker},
    multimodal,
    prompt::{PromptCommand, skill_subagent},
    resources::ResourceStore,
    side,
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
    conversations: Arc<ConversationsTool>,
    resource_store: Arc<ResourceStore>,
    tool_dependencies: Vec<String>,
    tools: Vec<String>,
    sessions: ActiveSessions,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    idle_hooks: Vec<Arc<dyn IdleHook>>,
    home_dir: PathBuf,
    skills_manager: Arc<SkillManager>,
    browser_manager: Arc<ChromeBrowserTool>,
    transcription_manager: Option<Arc<TranscriptionManager>>,
    active_im_channels: HashSet<String>,
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
        ShellTool::NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
        EditFileTool::NAME.to_string(),
        WriteFileTool::NAME.to_string(),
        TodoTool::NAME.to_string(),
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
        GoalTool::NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        TodoTool::NAME.to_string(),
        ShellTool::NAME.to_string(),
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
        skills_manager: Arc<SkillManager>,
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

        Self {
            inner: Arc::new(AndaBotInner {
                brain,
                home_dir,
                conversations,
                resource_store,
                tool_dependencies,
                tools: base_tools(),
                sessions: RwLock::new(HashMap::new()),
                completion_hooks: Arc::new(completion_hooks),
                idle_hooks,
                skills_manager,
                browser_manager,
                transcription_manager,
                active_im_channels: active_im_channels.into_iter().collect(),
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
        let mut active = sessions.values().cloned().collect::<Vec<_>>();
        drop(sessions);

        active.sort_by(|a, b| {
            b.active_at
                .load(Ordering::SeqCst)
                .cmp(&a.active_at.load(Ordering::SeqCst))
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        active
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
                let skills: Vec<SkillFrontmatter> = self
                    .inner
                    .skills_manager
                    .list()
                    .into_values()
                    .map(|skill| skill.frontmatter)
                    .collect();
                json!(
                    skills
                        .into_iter()
                        .map(|skill| json!({
                            "name": skill.name,
                            "description": skill.description,
                        }))
                        .collect::<Vec<_>>()
                )
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
            PromptCommand::Plain { prompt } | PromptCommand::Steer { prompt } => prompt,
            PromptCommand::Goal { prompt } => {
                initial_goal = Some(prompt.clone());
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
                if let Some(subagent) = skill_subagent(&self.inner.skills_manager, &skill) {
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
        let session_request_meta =
            SessionRequestMeta::new(request_meta_for_conversation(ctx.meta(), conversation._id));
        let external_user =
            request_meta_extra_as::<bool>(ctx.meta(), "external_user").unwrap_or(false);
        let formation_counterparty = if external_user {
            Some(scoped_external_user_name_from_meta(ctx.meta()))
        } else {
            Some(caller.to_string())
        };

        let session = Arc::new(Session {
            id: sess_id,
            caller: caller.to_string(),
            workspace,
            source_key: source_key.clone(),
            conversation_id: AtomicU64::new(conversation._id),
            sender,
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
            system_extra_user_context(&extra),
        );
        Ok(res)
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
    fn base_agent_tools_include_goal_tool() {
        assert!(base_tool_dependencies().contains(&GoalTool::NAME.to_string()));
        assert!(base_tools().contains(&GoalTool::NAME.to_string()));
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
}
