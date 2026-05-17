use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, ContentPart, Document,
    Documents, FunctionDefinition, Message, Principal, RequestMeta, Resource, StateFeatures, Tool,
    ToolOutput, Usage,
};
use anda_engine::{
    ANONYMOUS,
    context::{AgentCtx, BaseCtx, CompletionRunner, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{EditFileTool, ReadFileTool, SearchFileTool, WriteFileTool},
        note::{NoteTool, load_notes},
        shell::{ExecArgs, ExecOutput, ShellTool, ShellToolHook},
        skill::{SkillFrontmatter, SkillManager},
        todo::TodoTool,
    },
    hook::{AgentHook, DynAgentHook, ToolHook},
    memory::{Conversation, ConversationRef, ConversationStatus},
    rfc3339_datetime,
    subagent::SubAgentManager,
    unix_ms,
};
use anda_hippocampus::types::{FormationInputRef, InputContext};
use anda_kip::Response;
use async_trait::async_trait;
use chrono::{DateTime, Local, Utc};
use futures::future::join_all;
use ic_auth_types::Xid;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use super::{
    CompletionHook,
    browser::ChromeBrowserTool,
    conversation::{AgentInfo, ConversationsTool, RequestState, SourceState},
    goal::{self, GoalStateSnapshot, GoalTool, GoalToolState},
    prompt::{PromptCommand, skill_subagent},
    side,
    system::{
        SYSTEM_PERSON_NAME, external_user_name, mark_special_user_messages,
        system_extra_user_context, system_runtime_prompt, system_user_message,
    },
};
use crate::{brain, cron, transcription::TranscriptionManager, tts::TtsManager};

const MAX_TURNS_TO_COMPACT: usize = 81; // The number of turns after which the conversation history will be compacted. This is to prevent the conversation history from growing indefinitely and causing performance issues. The optimal value may depend on the typical length of conversations and the token limits of the language model.
const CONVERSATION_IDLE_MS: u64 = 10 * 60 * 1000; // 10 minutes
const CONVERSATION_WAIT_BACKGROUND_TASK_MS: u64 = 12 * 60 * 60 * 1000; // 12 hours
const STARTUP_SELF_SOURCE: &str = "startup:self";
static SELF_INSTRUCTIONS: &str = include_str!("../../assets/SelfInstructions.md");
static COMPACTION_PROMPT: &str = include_str!("../../assets/CompactionPrompt.md");

#[derive(Debug, Clone)]
struct StartupConversation {
    source_key: String,
    conversation: Conversation,
}

struct SystemInstructionSections<'a> {
    self_knowledge: &'a str,
    notes: &'a str,
    available_tools: &'a [String],
    home_dir: &'a str,
    workspace: &'a str,
    user_profile: &'a str,
    local_date: &'a str,
}

fn render_system_instructions(sections: SystemInstructionSections<'_>) -> String {
    format!(
        "{}\n\n---\n\n# Runtime Context\n\n## Self Knowledge\n{}\n\n## Notes\n{}\n\n## Available Callable Names\nNames only; schemas are intentionally omitted here. Use `tools_select` before calling any name whose full schema is not already loaded.\n{}\n\n## Environment\n- home: {}\n- current workspace (authoritative): {}\n\nUse the current workspace for filesystem and shell operations. Workspace paths in history are historical unless the user explicitly selects them.\n\n## User Profile\n{}\n\n## Current Datetime: {}",
        SELF_INSTRUCTIONS.trim(),
        sections.self_knowledge,
        sections.notes,
        format_available_tools(sections.available_tools),
        sections.home_dir,
        sections.workspace,
        sections.user_profile,
        sections.local_date,
    )
}

fn format_available_tools(available_tools: &[String]) -> String {
    if available_tools.is_empty() {
        "none".to_string()
    } else {
        available_tools.join(", ")
    }
}

async fn available_tool_names(ctx: &AgentCtx) -> Vec<String> {
    ctx.definitions(None)
        .await
        .into_iter()
        .filter_map(|def| {
            if def.name == AndaBot::NAME {
                None
            } else {
                Some(def.name)
            }
        })
        .collect()
}

#[derive(Clone)]
pub struct AndaBot {
    inner: Arc<AndaBotInner>,
}

struct AndaBotInner {
    brain: brain::Client,
    conversations: Arc<ConversationsTool>,
    tool_dependencies: Vec<String>,
    tools: Vec<String>,
    sessions: ActiveSessions,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    home_dir: PathBuf,
    skills_manager: Arc<SkillManager>,
    browser_manager: Arc<ChromeBrowserTool>,
    transcription_manager: Option<Arc<TranscriptionManager>>,
    active_im_channels: HashSet<String>,
    session_creation_lock: tokio::sync::Mutex<()>,
}

type ActiveSessions = RwLock<HashMap<Xid, Arc<Session>>>;

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub caller: String,
    pub workspace: String,
    pub source: String,
    pub conversation_id: u64,
    pub active_at: u64,
    pub idle_ms: u64,
    pub has_goal: bool,
    pub background_task_count: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionFormationContext {
    pub counterparty: Option<String>,
    pub agent: Option<String>,
    pub source: Option<String>,
    pub topic: Option<String>,
}

impl From<&InputContext> for SessionFormationContext {
    fn from(context: &InputContext) -> Self {
        Self {
            counterparty: context.counterparty.clone(),
            agent: context.agent.clone(),
            source: context.source.clone(),
            topic: context.topic.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionState {
    pub summary: SessionSummary,
    pub formation_context: Option<SessionFormationContext>,
    pub goal: Option<GoalStateSnapshot>,
    pub background_tasks: HashMap<String, BackgroundTaskInfo>,
    pub submit_formation_at: u64,
}

fn base_tool_dependencies() -> Vec<String> {
    vec![
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
        cron::ManageCronJobTool::NAME.to_string(),
        cron::ListCronJobsTool::NAME.to_string(),
        cron::ListCronRunsTool::NAME.to_string(),
        ChromeBrowserTool::NAME.to_string(),
    ]
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
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
        skills_manager: Arc<SkillManager>,
        browser_manager: Arc<ChromeBrowserTool>,
        tts_manager: Option<Arc<TtsManager>>,
        transcription_manager: Option<Arc<TranscriptionManager>>,
        active_im_channels: Vec<String>,
    ) -> Self {
        let mut tool_dependencies = base_tool_dependencies();
        let mut tools = base_tools();

        if tts_manager.is_some() {
            tool_dependencies.push(TtsManager::NAME.to_string());
            tools.push(TtsManager::NAME.to_string());
        }
        if transcription_manager.is_some() {
            tool_dependencies.push(TranscriptionManager::NAME.to_string());
            tools.push(TranscriptionManager::NAME.to_string());
        }

        Self {
            inner: Arc::new(AndaBotInner {
                brain,
                home_dir,
                conversations,
                tool_dependencies,
                tools,
                sessions: RwLock::new(HashMap::new()),
                completion_hooks: Arc::new(completion_hooks),
                skills_manager,
                browser_manager,
                transcription_manager,
                active_im_channels: active_im_channels.into_iter().collect(),
                session_creation_lock: tokio::sync::Mutex::new(()),
            }),
        }
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

    async fn build_system_instructions(
        &self,
        ctx: &AgentCtx,
        home_dir: &str,
        workspace: &str,
        available_tools: &[String],
        now_ms: u64,
    ) -> Result<String, BoxError> {
        self.build_system_instructions_for_user(
            ctx,
            ctx.caller(),
            home_dir,
            workspace,
            available_tools,
            now_ms,
        )
        .await
    }

    async fn build_system_instructions_for_user(
        &self,
        ctx: &AgentCtx,
        user: &Principal,
        home_dir: &str,
        workspace: &str,
        available_tools: &[String],
        now_ms: u64,
    ) -> Result<String, BoxError> {
        let primer = self.inner.brain.describe_primer().await?;
        let user_profile = self.inner.brain.user_info(user.to_string(), None).await?;
        let notes = load_notes(ctx).await.unwrap_or_default();
        let local_date = format_local_date(now_ms);
        let self_knowledge = serde_json::to_string(primer.get("identity").unwrap_or(&primer))?;
        let notes = serde_json::to_string(&notes.notes)?;
        let user_profile = serde_json::to_string(&user_profile)?;

        Ok(render_system_instructions(SystemInstructionSections {
            self_knowledge: &self_knowledge,
            notes: &notes,
            available_tools,
            home_dir,
            workspace,
            user_profile: &user_profile,
            local_date: &local_date,
        }))
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
        let mut output = subagent
            .run(
                ctx.child(&subagent.name, &subagent.name)?,
                prompt,
                resources,
            )
            .await?;

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

    async fn startup_self_check(&self, ctx: AgentCtx) -> Result<(), BoxError> {
        let candidates = self.startup_source_candidates(unix_ms()).await;
        let resume: Vec<&StartupConversation> = candidates
            .iter()
            .filter(|candidate| should_auto_resume_conversation(&candidate.conversation.status))
            .collect();

        if !resume.is_empty() {
            for candidate in resume {
                self.continue_startup_conversation(
                    ctx.with_caller(candidate.conversation.user),
                    candidate.clone(),
                    startup_recovery_prompt(&candidate.conversation),
                )
                .await?;
            }
            return Ok(());
        }

        Ok(())

        // log::info!(
        //     "startup self-check found no source-bound conversation; starting self exploration"
        // );
        // self.start_startup_exploration(ctx).await
    }

    async fn startup_source_candidates(&self, now_ms: u64) -> Vec<StartupConversation> {
        let source_conversations = self.inner.conversations.source_conversations();
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        for (source_key, state) in source_conversations {
            if state.conv_id == 0 {
                continue;
            }

            match self.latest_conversation_in_chain(state.conv_id, None).await {
                Ok(conversation) => {
                    if seen.insert(conversation._id)
                        && conversation.updated_at + 3 * 24 * 3600 * 1000 > now_ms
                    {
                        candidates.push(StartupConversation {
                            source_key,
                            conversation,
                        });
                    }
                }
                Err(err) => {
                    log::warn!(
                        source = source_key,
                        conversation = state.conv_id;
                        "startup self-check failed to load source conversation: {err}"
                    );
                }
            }
        }

        candidates.sort_by(|left, right| {
            right
                .conversation
                .updated_at
                .cmp(&left.conversation.updated_at)
                .then_with(|| right.conversation._id.cmp(&left.conversation._id))
        });
        candidates
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

    async fn continue_startup_conversation(
        &self,
        ctx: AgentCtx,
        candidate: StartupConversation,
        prompt: String,
    ) -> Result<(), BoxError> {
        let mut conversation = candidate.conversation;
        if let Some(thread) = &conversation.thread
            && self.get_session(thread).is_some()
        {
            return Ok(());
        }
        let chat_history = conversation_chat_history(&conversation);
        if chat_history.is_empty() {
            return Ok(());
        }

        let now_ms = unix_ms();
        let mut meta = request_meta_from_conversation(&conversation, &candidate.source_key);
        meta = request_meta_for_conversation(&meta, conversation._id);
        let RequestState {
            workspace,
            source,
            source_key,
            ..
        } = self.inner.conversations.state_from_meta(&meta);
        if !self.inner.active_im_channels.contains(&source) {
            return Ok(());
        }

        log::warn!(
            conversation = conversation._id,
            status = conversation.status.to_string(),
            source = source_key;
            "startup self-check continuing conversation from source"
        );

        let agent_label = ctx.label.clone();
        let ctx = ctx.child(Self::NAME, &agent_label)?;
        let home_dir = self.inner.home_dir.to_string_lossy().to_string();
        let available_tools = available_tool_names(&ctx).await;
        let instructions = self
            .build_system_instructions_for_user(
                &ctx,
                &conversation.user,
                &home_dir,
                &workspace,
                &available_tools,
                now_ms,
            )
            .await?;
        let mut tools = self.inner.tools.clone();
        if self.inner.browser_manager.is_active() {
            tools.push(ChromeBrowserTool::NAME.to_string());
        }
        let additional_tools = self
            .inner
            .conversations
            .tool_usage_with(|usage| select_most_used_tools(&available_tools, &tools, usage, 5));
        tools.extend(additional_tools);
        let base_req = CompletionRequest {
            instructions,
            tools: ctx.definitions(Some(&tools)).await,
            tool_choice_required: false,
            max_output_tokens: Some(ctx.model.max_output.max(32000)),
            ..Default::default()
        };

        let initial_req = CompletionRequest {
            prompt,
            chat_history: chat_history.clone(),
            ..base_req.clone()
        };
        let session_request_meta = SessionRequestMeta::new(meta.clone());
        let sess_id = conversation.thread.clone().unwrap_or_default();
        let runner = ctx
            .clone()
            .completion_iter(initial_req, Vec::new())
            .reserve_chat_history(chat_history)
            .unbound();

        conversation.thread = Some(sess_id.clone());
        conversation.status = ConversationStatus::Working;
        conversation.updated_at = now_ms;
        self.persist_conversation_state(&conversation).await;

        let (sender, rx) = tokio::sync::mpsc::channel::<ConversationInput>(42);
        let external_user = meta.get_extra_as::<bool>("external_user").unwrap_or(false);
        let formation_counterparty = if external_user {
            meta.user
                .as_ref()
                .map(|sender| format!("$external_user:{sender}"))
                .or_else(|| Some("$external_user".to_string()))
        } else {
            Some(conversation.user.to_string())
        };
        let session = Arc::new(Session {
            id: sess_id,
            caller: conversation.user.to_string(),
            workspace,
            source_key: source_key.clone(),
            conversation_id: AtomicU64::new(conversation._id),
            sender,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            goal: Arc::new(RwLock::new(None)),
            request_meta: session_request_meta.clone(),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(now_ms)),
            finish_when_idle: AtomicBool::new(false),
            formation_context: Some(InputContext {
                counterparty: formation_counterparty,
                agent: Some(AndaBot::NAME.to_string()),
                source: Some(source_key),
                topic: Some("startup_self_check".to_string()),
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
        self.spawn_session_runner(ctx, base_req, session, conversation, runner, rx, None);
        Ok(())
    }

    #[allow(unused)]
    async fn start_startup_exploration(&self, ctx: AgentCtx) -> Result<(), BoxError> {
        let now_ms = unix_ms();
        let mut extra = Map::new();
        let workspace = self.inner.home_dir.to_string_lossy().to_string();
        extra.insert("workspace".to_string(), workspace.into());
        extra.insert("source".to_string(), STARTUP_SELF_SOURCE.into());
        let meta = RequestMeta {
            extra,
            ..Default::default()
        };
        let mut conversation = Conversation {
            user: *ctx.caller(),
            thread: Some(Xid::new()),
            messages: Vec::new(),
            resources: vec![],
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some(json!(meta.extra)),
            ..Default::default()
        };
        let conv_id = self
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = conv_id;
        if let Err(err) = self
            .inner
            .conversations
            .update_source_state(
                STARTUP_SELF_SOURCE.to_string(),
                SourceState {
                    conv_id,
                    status: conversation.status.clone(),
                    timestamp: now_ms,
                },
            )
            .await
        {
            log::warn!(conversation = conv_id; "failed to persist startup self source state: {err}");
        }

        self.continue_startup_conversation(
            ctx,
            StartupConversation {
                source_key: STARTUP_SELF_SOURCE.to_string(),
                conversation,
            },
            startup_exploration_prompt(),
        )
        .await
    }

    fn spawn_session_runner(
        &self,
        ctx: AgentCtx,
        req: CompletionRequest,
        session: Arc<Session>,
        conversation: Conversation,
        runner: CompletionRunner,
        mut rx: tokio::sync::mpsc::Receiver<ConversationInput>,
        extra_user_context: Option<Message>,
    ) {
        let assistant = self.clone();
        tokio::spawn(async move {
            let mut tools_usage_snapshot: HashMap<String, Usage> = HashMap::new();
            let mut runner = SessionRunner {
                ctx,
                req,
                assistant: assistant.clone(),
                session: session.clone(),
                conversation,
                runner,
                first_round: true,
                extra_user_context: extra_user_context.clone(),
                last_extra_user_context: extra_user_context,
            };

            loop {
                let mut inputs = Vec::new();

                while let Ok(input) = rx.try_recv() {
                    inputs.push(input);
                }

                match runner.run(inputs, &mut tools_usage_snapshot).await {
                    Ok(continue_active) => {
                        if !continue_active {
                            break;
                        }
                    }
                    Err(err) => {
                        log::error!("Error processing session {}: {:?}", session.id, err);
                        break;
                    }
                }
            }

            assistant.detach_session(&session.id);
        });
    }
}

impl Tool<BaseCtx> for AndaBot {
    type Args = AndaBotToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        format!("{}_api", Self::NAME)
    }

    fn description(&self) -> String {
        "Client API for inspecting currently active AndaBot sessions, including goals and background tasks."
            .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: Tool::name(self),
            description: Tool::description(self),
            parameters: json!({
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
                "required": ["type"],
                "additionalProperties": false
            }),
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
                    .into_iter()
                    .map(|(_, skill)| skill.frontmatter)
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
        if self.inner.transcription_manager.is_some() {
            tags.extend(crate::transcription::supported_audio_resource_tags());
        }
        tags
    }

    async fn init(&self, ctx: AgentCtx) -> Result<(), BoxError> {
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

        let command = PromptCommand::from(prompt);
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

        let _session_creation_guard = self.inner.session_creation_lock.lock().await;
        let RequestState {
            workspace,
            source_key,
            source_state,
            conversation: maybe_conv_id,
            ..
        } = self.inner.conversations.state_from_meta(ctx.meta());
        let mut instructions = self
            .build_system_instructions(&ctx, &home_dir, &workspace, &available_tools, now_ms)
            .await?;
        let mut current_conversation = if maybe_conv_id > 0 {
            self.latest_conversation_in_chain(maybe_conv_id, Some(*caller))
                .await
                .ok()
        } else {
            None
        };

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
        let active_session = self
            .get_session(&sess_id)
            .or_else(|| self.get_session_by_source(&source_key));
        let mut detached_existing_session = false;
        let mut detached_conversation_id = current_conversation_id;
        if let Some(session) = active_session {
            // Join existing conversation session if it's active
            if matches!(input.command, PromptCommand::New { .. }) {
                detached_conversation_id =
                    Some(session.conversation_id.load(Ordering::SeqCst)).filter(|id| *id > 0);
                if let Some(session) = self.detach_session(&session.id) {
                    session.finish_when_idle.store(true, Ordering::SeqCst);
                    detached_existing_session = true;
                }
            } else {
                let response_conversation_id = session.conversation_id.load(Ordering::SeqCst);
                let meta = request_meta_for_conversation(ctx.meta(), response_conversation_id);
                session.request_meta.set(meta);
                match session.sender.send(input).await {
                    Ok(_) => {
                        return Ok(AgentOutput {
                            conversation: (response_conversation_id > 0)
                                .then_some(response_conversation_id),
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
            }
        }

        // If the conversation session is not active, start a new session and process the prompt
        let ConversationInput {
            command,
            resources,
            extra,
            ..
        } = input;

        let mut initial_goal = None;
        let mut additional_tools: Vec<String> = Vec::new();
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
            PromptCommand::Skill { mut skill, prompt } => {
                if let Some(subagent) = skill_subagent(&self.inner.skills_manager, &skill) {
                    skill = subagent.name.clone();
                    additional_tools.push(subagent.name);
                }

                instructions =
                    format!("{instructions}\n\nUse the {skill} skill to handle user's request");
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
                    if source_state.conv_id != 0
                        && let Err(err) = self
                            .inner
                            .conversations
                            .update_source_state(
                                source_key.clone(),
                                SourceState {
                                    conv_id: 0,
                                    status: ConversationStatus::Cancelled,
                                    timestamp: now_ms,
                                },
                            )
                            .await
                    {
                        log::error!("Failed to update_source_state: {:?}", err);
                    }

                    return Ok(AgentOutput {
                        conversation: detached_conversation_id,
                        ..Default::default()
                    });
                };

                force_standalone_conversation = true;
                current_conversation = None;
                sess_id = Xid::new();
                prompt
            }
        };

        let mut initial_messages = vec![Message {
            role: "user".into(),
            content: vec![prompt.clone().into()],
            timestamp: Some(now_ms),
            ..Default::default()
        }];
        mark_special_user_messages(&mut initial_messages);

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
            if prompt.trim().is_empty() {
                return Err("prompt cannot be empty".into());
            }

            let mut ancestors: Option<Vec<u64>> = None;
            if !force_standalone_conversation {
                let (mut history_conversations, _) = self
                    .inner
                    .conversations
                    .conversations
                    .list_conversations_by_user(caller, None, Some(2))
                    .await?;

                if let Some(conv) = &current_conversation {
                    let mut ids = conv.ancestors.clone().unwrap_or_default();
                    ids.push(conv._id);
                    if ids.len() > 10 {
                        ids.drain(0..ids.len() - 10);
                    }
                    ancestors = Some(ids);

                    if !history_conversations.iter().any(|c| c._id == conv._id) {
                        history_conversations.push(conv.clone());
                    }
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
                messages: initial_messages.into_iter().map(|msg| json!(msg)).collect(),
                ancestors,
                resources: vec![], // Don't save the resources in the conversation, as they are already included in the message content as documents
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
        let external_user = ctx
            .meta()
            .get_extra_as::<bool>("external_user")
            .unwrap_or(false);
        let formation_counterparty = if external_user {
            ctx.meta()
                .user
                .as_ref()
                .map(|sender| external_user_name(sender))
                .or_else(|| Some(external_user_name("")))
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
            goal: Arc::new(RwLock::new(initial_goal.map(goal::GoalState::new))),
            request_meta: session_request_meta.clone(),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(unix_ms())),
            finish_when_idle: AtomicBool::new(false),
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

        let mut tools = assistant.inner.tools.clone();
        tools.extend(additional_tools);
        let additional_tools = assistant
            .inner
            .conversations
            .tool_usage_with(|usage| select_most_used_tools(&available_tools, &tools, usage, 5));
        tools.extend(additional_tools);
        let req = CompletionRequest {
            instructions,
            tools: ctx.definitions(Some(&tools)).await,
            tool_choice_required: false,
            max_output_tokens: Some(ctx.model.max_output.max(32000)),
            ..Default::default()
        };

        let content: Vec<ContentPart> = resources
            .into_iter()
            .filter_map(|res| res.try_into().ok())
            .collect();

        let mut runner = ctx
            .clone()
            .completion_iter(
                CompletionRequest {
                    prompt,
                    chat_history,
                    content,
                    ..req.clone()
                },
                vec![],
            )
            .unbound();
        if !reserve_chat_history.is_empty() {
            runner = runner.reserve_chat_history(reserve_chat_history);
        }

        assistant.spawn_session_runner(
            ctx,
            req,
            session,
            conversation,
            runner,
            rx,
            system_extra_user_context(&extra),
        );
        Ok(res)
    }
}

struct SessionRunner {
    ctx: AgentCtx,
    req: CompletionRequest,
    assistant: AndaBot,
    session: Arc<Session>,
    conversation: Conversation,
    runner: CompletionRunner,
    first_round: bool,
    extra_user_context: Option<Message>,
    last_extra_user_context: Option<Message>,
}

impl SessionRunner {
    async fn persist_conversation_state(&self) {
        self.assistant
            .persist_conversation_state(&self.conversation)
            .await;
    }

    async fn persist_tools_usage_snapshot(
        &self,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) {
        let current_tools_usage = self.runner.tools_usage().clone();
        let tools_usage_delta =
            compute_tools_usage_delta(&current_tools_usage, tools_usage_snapshot);
        *tools_usage_snapshot = current_tools_usage;
        if let Err(err) = self
            .assistant
            .inner
            .conversations
            .accumulate_tool_usage(tools_usage_delta)
            .await
        {
            log::error!("Failed to accumulate_tool_usage: {:?}", err);
        }
    }

    async fn submit_pending_formation(&self, chat_history: &[Message], now_ms: u64) {
        let mut messages = chat_history
            .iter()
            .skip(self.session.submit_formation_at.load(Ordering::SeqCst) as usize)
            .filter_map(|msg| {
                let mut msg = msg.clone();
                let pruned = msg.prune_content();
                if msg.content.is_empty() || pruned > 0 && msg.content.len() <= 1 {
                    None
                } else {
                    Some(msg)
                }
            })
            .collect::<Vec<_>>();
        mark_special_user_messages(&mut messages);

        let next_submit_formation_at = chat_history.len();
        if messages.is_empty() {
            self.session
                .submit_formation_at
                .store(next_submit_formation_at as u64, Ordering::SeqCst);
            return;
        }

        let timestamp = rfc3339_datetime(now_ms);
        match self
            .assistant
            .submit_formation(&messages, &self.session.formation_context, &timestamp)
            .await
        {
            Ok(_) => {
                self.session
                    .submit_formation_at
                    .store(next_submit_formation_at as u64, Ordering::SeqCst);
            }
            Err(err) => {
                log::error!(
                    "Failed to send formation for session {}, conversation {}, error: {:?}",
                    self.session.id,
                    self.conversation._id,
                    err
                );
            }
        }
    }

    // returns true if the conversation should continue to be active after processing the inputs, or false if it should be terminated
    async fn run(
        &mut self,
        inputs: Vec<ConversationInput>,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) -> Result<bool, BoxError> {
        let mut cancellation_requested: Option<String> = None;
        if !inputs.is_empty() {
            self.session.active_at.store(unix_ms(), Ordering::SeqCst);
        }

        for input in inputs {
            // 累计来自于后台任务的工具使用情况
            self.runner.accumulate(&input.usage);

            let mut content: Vec<ContentPart> = input
                .resources
                .into_iter()
                .filter_map(|res| res.try_into().ok())
                .collect();

            if let Some(msg) = system_extra_user_context(&input.extra)
                && self.last_extra_user_context.as_ref() != Some(&msg)
            {
                self.extra_user_context = Some(msg.clone());
                self.last_extra_user_context = Some(msg);
            }

            match input.command {
                PromptCommand::Ping | PromptCommand::Invalid { .. } => {
                    // PING from the user to keep the conversation alive.
                    log::info!(
                        "Received PING from user in session {}, conversation {}",
                        self.session.id,
                        self.conversation._id
                    );
                    continue;
                }
                PromptCommand::Plain { prompt } => {
                    content.push(prompt.into());
                    self.runner.follow_up_content(content);
                }
                PromptCommand::Goal { prompt } => {
                    content.push(prompt.clone().into());
                    self.runner.follow_up_content(content);

                    let mut next_goal = self.session.goal.write();
                    if let Some(existing_goal) = next_goal.as_mut() {
                        existing_goal.update_objective(prompt);
                    } else {
                        *next_goal = Some(goal::GoalState::new(prompt));
                    };
                }
                PromptCommand::Side { prompt } => {
                    content.push(prompt.into());
                    self.runner.follow_up_content(content);
                }
                PromptCommand::Steer { prompt } => {
                    content.push(prompt.into());
                    self.runner.follow_up_content(content);
                }
                PromptCommand::Skill { mut skill, prompt } => {
                    if let Some(subagent) =
                        skill_subagent(&self.assistant.inner.skills_manager, &skill)
                    {
                        skill = subagent.name;
                    }
                    content.push(
                        format!("Use the {skill} skill to handle this request:\n\n{prompt}").into(),
                    );
                    self.runner.follow_up_content(content);
                }
                PromptCommand::Stop { prompt } => {
                    cancellation_requested =
                        Some(prompt.unwrap_or_else(|| "Cancelled by user".to_string()));
                    break;
                }
                PromptCommand::New { prompt } => {
                    if let Some(prompt) = prompt {
                        content.push(prompt.into());
                    }
                    if !content.is_empty() {
                        self.runner.follow_up_content(content);
                    }
                    break;
                }
            }
        }

        let now_ms = unix_ms();
        if let Some(failed_reason) = cancellation_requested {
            self.persist_tools_usage_snapshot(tools_usage_snapshot)
                .await;
            self.submit_pending_formation(self.runner.chat_history(), now_ms)
                .await;

            self.conversation.status = ConversationStatus::Cancelled;
            self.conversation.failed_reason = Some(failed_reason);
            self.conversation.updated_at = now_ms;
            self.persist_conversation_state().await;
            return Ok(false);
        }

        if let Some(extra_user_context) = self.extra_user_context.take() {
            self.runner.implicit_context(extra_user_context);
        }

        match self.runner.next().await {
            Ok(None) => {
                let now_ms = unix_ms();

                self.persist_tools_usage_snapshot(tools_usage_snapshot)
                    .await;
                self.submit_pending_formation(self.runner.chat_history(), now_ms)
                    .await;

                let maybe_goal = { self.session.goal.write().take() };
                let mut goal_continue_prompt: Option<String> = None;
                let mut active = false;
                if let Some(mut goal) = maybe_goal {
                    match goal.check_progress(&self.runner, &self.ctx).await {
                        Ok(check) => {
                            self.runner.accumulate(&check.usage);
                            match check.action {
                                goal::GoalAction::Complete(reason) => {
                                    let message = goal_completed_message(&reason, now_ms);
                                    self.runner.append_chat_history(vec![message]);
                                    log::info!(
                                        turns = self.runner.turns(),
                                        last_usage:serde = self.runner.current_usage(),
                                        total_usage:serde = self.runner.total_usage(),
                                        tools_usage:serde = self.runner.tools_usage();
                                        "Goal completed: {:?}", reason);
                                }
                                goal::GoalAction::Continue(prompt) => {
                                    let now_ms = unix_ms();
                                    goal_continue_prompt = Some(prompt);
                                    active = true;
                                    self.session.active_at.store(now_ms, Ordering::SeqCst);
                                    *self.session.goal.write() = Some(goal);
                                }
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "Failed to evaluate goal progress for session {}: {:?}",
                                self.session.id,
                                err
                            );
                        }
                    }
                }

                if needs_compaction(&self.runner) {
                    // 上下文过长，先进行一次压缩总结，更新conversation状态和历史消息，再继续后续的处理
                    let mut output = self
                        .runner
                        .finalize(Some(COMPACTION_PROMPT.to_string()))
                        .await?;
                    mark_special_user_messages(&mut output.chat_history);

                    let now_ms = unix_ms();
                    if let Some(failed_reason) = output.failed_reason {
                        self.persist_tools_usage_snapshot(tools_usage_snapshot)
                            .await;

                        self.conversation.failed_reason =
                            Some(format!("Compaction failed: {failed_reason}"));
                        self.conversation.status = ConversationStatus::Failed;
                        self.conversation.usage = output.usage;
                        self.conversation.updated_at = now_ms;
                        self.persist_conversation_state().await;
                        return Ok(false);
                    }

                    // 如果目标还没有完成，也需要关闭本轮 conversation （conversation 数据大小有限，不应该超过 10MB），为 session 创建新的 conversation 和 runner 继续后续的交互
                    // 同一个 session 可以逐步产生不限数量的 conversation 对话，可支持超长程推理。

                    // 前一轮压缩总结的内容作为新 conversation 的第一条消息，继续后续的交互
                    let compaction_msg = Message {
                        role: "assistant".into(),
                        content: vec![output.content.into()],
                        timestamp: Some(now_ms),
                        ..Default::default()
                    };
                    let child = if let Some(prompt) = goal_continue_prompt {
                        let mut ancestors = self.conversation.ancestors.clone().unwrap_or_default();
                        ancestors.push(self.conversation._id);

                        let mut conversation = Conversation {
                            user: self.conversation.user,
                            thread: Some(self.session.id.clone()),
                            messages: vec![
                                serde_json::json!(compaction_msg),
                                serde_json::json!(system_user_message(prompt.clone(), now_ms)),
                            ],
                            ancestors: Some(ancestors),
                            period: now_ms / 3600 / 1000,
                            created_at: now_ms,
                            updated_at: now_ms,
                            extra: Some(json!(self.ctx.meta().extra)),
                            ..Default::default()
                        };

                        let conv_id = self
                            .assistant
                            .inner
                            .conversations
                            .conversations
                            .add_conversation(ConversationRef::from(&conversation))
                            .await?;
                        conversation._id = conv_id;

                        let req = CompletionRequest {
                            prompt,
                            chat_history: vec![compaction_msg.clone()],
                            ..self.req.clone()
                        };

                        Some((conversation, req))
                    } else {
                        None
                    };

                    self.persist_tools_usage_snapshot(tools_usage_snapshot)
                        .await;
                    self.submit_pending_formation(&output.chat_history, now_ms)
                        .await;

                    self.conversation.messages.clear();
                    self.conversation.append_messages(output.chat_history);
                    self.conversation.status = ConversationStatus::Completed;
                    self.conversation.usage = output.usage;
                    self.conversation.artifacts = output.artifacts;
                    self.conversation.updated_at = now_ms;
                    // 把新的 conversation 设为原 conversation 的 child，延续同一个 session，客户端可以读取连续的 conversation 记录来展示给用户
                    self.conversation.child = child.as_ref().map(|(conv, _)| conv._id);
                    self.persist_conversation_state().await;
                    match child {
                        Some((conv, req)) => {
                            self.first_round = true;
                            self.session.submit_formation_at.store(0, Ordering::SeqCst);
                            self.conversation = conv;
                            self.session
                                .conversation_id
                                .store(self.conversation._id, Ordering::SeqCst);
                            if !self.session.finish_when_idle.load(Ordering::SeqCst)
                                && let Err(err) = self
                                    .assistant
                                    .inner
                                    .conversations
                                    .update_source_state(
                                        self.session.source_key.clone(),
                                        SourceState {
                                            conv_id: self.conversation._id,
                                            status: self.conversation.status.clone(),
                                            timestamp: now_ms,
                                        },
                                    )
                                    .await
                            {
                                log::error!("Failed to update_source_state: {:?}", err);
                            }
                            // runner 的 chat_history 作为唯一对话历史记录真相源，conversation 和 formation 都从这里获取 messages。
                            self.runner = self
                                .ctx
                                .clone()
                                .completion_iter(req, Vec::new())
                                .reserve_chat_history(vec![compaction_msg])
                                .unbound();
                            return Ok(true);
                        }
                        None => return Ok(false),
                    }
                }

                if let Some(prompt) = goal_continue_prompt {
                    self.runner.follow_up(prompt);
                }

                let now_ms = unix_ms();
                let idle = now_ms.saturating_sub(self.session.active_at.load(Ordering::SeqCst));
                let has_background_tasks = !self.session.background_tasks.read().is_empty();

                if self.session.finish_when_idle.load(Ordering::SeqCst) && !has_background_tasks {
                    self.conversation.status = ConversationStatus::Completed;
                    self.conversation.updated_at = now_ms;
                    self.persist_conversation_state().await;
                    return Ok(false);
                }

                if idle > CONVERSATION_IDLE_MS && !has_background_tasks
                    || (idle > CONVERSATION_WAIT_BACKGROUND_TASK_MS && has_background_tasks)
                {
                    self.conversation.status = ConversationStatus::Completed;
                    self.conversation.updated_at = now_ms;
                    self.persist_conversation_state().await;
                    return Ok(false);
                }

                if active {
                    self.conversation.status = ConversationStatus::Working;
                    self.conversation.usage = self.runner.total_usage().clone();
                    self.conversation.updated_at = now_ms;
                    self.persist_conversation_state().await;
                } else if self.conversation.status != ConversationStatus::Idle {
                    self.conversation.status = ConversationStatus::Idle;
                    self.conversation.updated_at = now_ms;
                    self.persist_conversation_state().await;
                }

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                return Ok(true);
            }

            Ok(Some(mut res)) => {
                let now_ms = unix_ms();
                self.session.active_at.store(now_ms, Ordering::SeqCst);
                let is_done = self.runner.is_done();
                res.conversation = Some(self.conversation._id);
                mark_special_user_messages(&mut res.chat_history);

                self.session.on_completion(&self.ctx, &res).await;

                if self.first_round {
                    self.first_round = false;
                    self.conversation.messages.clear();
                    self.conversation.append_messages(res.chat_history);
                } else {
                    let existing_len = self.conversation.messages.len();
                    if res.chat_history.len() >= existing_len {
                        res.chat_history.drain(0..existing_len);
                        self.conversation.append_messages(res.chat_history);
                    } else {
                        self.conversation.messages.clear();
                        self.conversation.append_messages(res.chat_history);
                    }
                }

                self.conversation.status = if res.failed_reason.is_some() {
                    ConversationStatus::Failed
                } else if is_done {
                    ConversationStatus::Completed
                } else {
                    ConversationStatus::Working
                };
                self.conversation.usage = res.usage;
                self.conversation.updated_at = now_ms;
                self.conversation.failed_reason = res.failed_reason.take();
                self.persist_conversation_state().await;

                if self.conversation.status == ConversationStatus::Completed
                    || self.conversation.status == ConversationStatus::Failed
                {
                    self.persist_tools_usage_snapshot(tools_usage_snapshot)
                        .await;
                    self.submit_pending_formation(self.runner.chat_history(), now_ms)
                        .await;
                }

                if self.conversation.status == ConversationStatus::Cancelled
                    || self.conversation.status == ConversationStatus::Failed
                    || (is_done && self.session.goal.read().is_none())
                {
                    return Ok(false);
                }
            }

            Err(err) => {
                let failed_reason = err.to_string();
                log::error!(
                    "Session {} in CompletionRunner error: {:?}",
                    self.session.id,
                    err
                );
                self.persist_tools_usage_snapshot(tools_usage_snapshot)
                    .await;
                self.submit_pending_formation(
                    self.runner.chat_history(),
                    self.conversation.updated_at,
                )
                .await;

                self.conversation.failed_reason = Some(failed_reason.clone());
                self.conversation.status = ConversationStatus::Failed;
                self.conversation.updated_at = unix_ms();
                self.persist_conversation_state().await;

                return Ok(false);
            }
        }

        Ok(true)
    }
}

struct Session {
    id: Xid,
    caller: String,
    workspace: String,
    source_key: String,
    conversation_id: AtomicU64,
    sender: tokio::sync::mpsc::Sender<ConversationInput>,
    // task_id -> BackgroundTaskInfo
    background_tasks: Arc<RwLock<HashMap<String, BackgroundTaskInfo>>>,
    goal: Arc<RwLock<Option<goal::GoalState>>>,
    request_meta: SessionRequestMeta,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    submit_formation_at: AtomicU64,
    active_at: Arc<AtomicU64>,
    finish_when_idle: AtomicBool,
    formation_context: Option<InputContext>,
}

#[derive(Clone)]
pub struct SessionRequestMeta {
    meta: Arc<RwLock<RequestMeta>>,
}

impl SessionRequestMeta {
    pub fn new(meta: RequestMeta) -> Self {
        Self {
            meta: Arc::new(RwLock::new(meta)),
        }
    }

    pub fn get(&self) -> RequestMeta {
        self.meta.read().clone()
    }

    fn set(&self, meta: RequestMeta) {
        *self.meta.write() = meta;
    }
}

fn request_meta_for_conversation(meta: &RequestMeta, conversation_id: u64) -> RequestMeta {
    let mut meta = meta.clone();
    if conversation_id > 0 {
        meta.extra
            .insert("conversation".to_string(), conversation_id.into());
    }
    meta
}

fn conversation_extra_without_id(meta: &RequestMeta) -> Map<String, Value> {
    let mut extra = meta.extra.clone();
    extra.remove("conversation");
    extra
}

fn request_meta_from_conversation(conversation: &Conversation, source_key: &str) -> RequestMeta {
    let mut extra = conversation
        .extra
        .as_ref()
        .and_then(|extra| extra.as_object().cloned())
        .unwrap_or_default();
    apply_source_key_to_meta_extra(&mut extra, source_key);
    extra.insert("conversation".to_string(), conversation._id.into());

    RequestMeta {
        extra,
        ..Default::default()
    }
}

fn apply_source_key_to_meta_extra(extra: &mut Map<String, Value>, source_key: &str) {
    if extra.get("source").is_some() {
        return;
    }

    if let Some((source, route)) = source_key.split_once(":reply_target:") {
        extra.insert("source".to_string(), source.to_string().into());
        if let Some((reply_target, thread)) = route.split_once(":thread:") {
            extra.insert("reply_target".to_string(), reply_target.to_string().into());
            if !thread.is_empty() {
                extra.insert("thread".to_string(), thread.to_string().into());
            }
        }
    } else if !source_key.is_empty() {
        extra.insert("source".to_string(), source_key.to_string().into());
    }
}

fn conversation_chat_history(conversation: &Conversation) -> Vec<Message> {
    let mut messages = conversation
        .messages
        .iter()
        .filter_map(|message| match serde_json::from_value::<Message>(message.clone()) {
            Ok(message) => Some(message),
            Err(err) => {
                log::warn!(conversation = conversation._id; "failed to parse startup conversation message: {err}");
                None
            }
        })
        .collect::<Vec<_>>();
    while let Some(last) = messages.last() {
        if last.tool_calls().is_empty() {
            break;
        }
        // 移除最后的 tool_calls
        // Each `tool_use` block must have a corresponding `tool_result` block in the next message.
        messages.pop();
    }
    mark_special_user_messages(&mut messages);
    messages
}

fn should_auto_resume_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted | ConversationStatus::Working
    )
}

fn should_continue_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted | ConversationStatus::Working | ConversationStatus::Idle
    )
}

fn is_terminal_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Completed | ConversationStatus::Cancelled | ConversationStatus::Failed
    )
}

#[allow(unused)]
fn should_startup_greet_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Idle | ConversationStatus::Failed
    )
}

#[allow(unused)]
fn startup_recovery_prompt(conversation: &Conversation) -> String {
    system_runtime_prompt(
        "startup recovery",
        format!(
            "Startup self-check found this conversation in {:?} state after the process restarted. Continue from the latest saved history. If the previous user request is still incomplete, resume it and send the next useful progress update. If it already appears complete, briefly explain that the session was recovered and ask for the next step. Avoid repeating old content unnecessarily.",
            conversation.status
        ),
    )
}

#[allow(unused)]
fn startup_greeting_prompt(conversation: &Conversation) -> String {
    system_runtime_prompt(
        "startup greeting",
        format!(
            "Startup self-check found no interrupted conversation. This is the most recent active conversation source, currently in {:?} state. Send a concise, natural greeting that says you are online again and offer one concrete way to continue based on the saved context. Do not claim the user just spoke.",
            conversation.status
        ),
    )
}

#[allow(unused)]
fn startup_exploration_prompt() -> String {
    system_runtime_prompt(
        "startup exploration",
        "Startup self-check found no source-bound conversation. Do a brief, read-only self exploration: inspect your runtime context, identify one useful capability or maintenance idea worth remembering for future work, and summarize it concisely. Do not contact external users.",
    )
}

impl Session {
    fn summary(&self, now_ms: u64) -> SessionSummary {
        let active_at = self.active_at.load(Ordering::SeqCst);
        SessionSummary {
            id: self.id.to_string(),
            caller: self.caller.clone(),
            workspace: self.workspace.clone(),
            source: self.source_key.clone(),
            conversation_id: self.conversation_id.load(Ordering::SeqCst),
            active_at,
            idle_ms: now_ms.saturating_sub(active_at),
            has_goal: self.goal.read().is_some(),
            background_task_count: self.background_tasks.read().len(),
        }
    }

    fn state(&self, now_ms: u64) -> SessionState {
        SessionState {
            summary: self.summary(now_ms),
            formation_context: self
                .formation_context
                .as_ref()
                .map(SessionFormationContext::from),
            goal: self.goal.read().as_ref().map(|goal| goal.snapshot()),
            background_tasks: self.background_tasks.read().clone(),
            submit_formation_at: self.submit_formation_at.load(Ordering::SeqCst),
        }
    }
}

#[async_trait]
impl CompletionHook for Session {
    async fn on_completion(&self, _ctx: &AgentCtx, _output: &AgentOutput) {
        join_all(
            self.completion_hooks
                .iter()
                .map(|hook| hook.on_completion(_ctx, _output)),
        )
        .await;
    }
}

#[async_trait]
impl AgentHook for Session {
    async fn on_background_start(
        &self,
        ctx: &AgentCtx,
        session_id: &str,
        _req: &CompletionRequest,
    ) {
        self.background_tasks.write().insert(
            session_id.to_string(),
            BackgroundTaskInfo {
                agent_name: ctx.base.agent.clone(),
                tool_name: None,
                progress_message: None,
            },
        );
    }

    async fn on_background_progress(
        &self,
        ctx: &AgentCtx,
        session_id: String,
        output: AgentOutput,
    ) {
        let prompt = if !output.content.is_empty() {
            system_runtime_prompt(
                "subagent progress",
                format!(
                    "Subagent session {session_id} intermediate output:\n\n{}",
                    output.content
                ),
            )
        } else if let Some(failed_reason) = output.failed_reason {
            system_runtime_prompt(
                "subagent progress",
                format!(
                    "Subagent session {session_id} failed with reason: {:?}",
                    failed_reason
                ),
            )
        } else {
            return;
        };
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain { prompt },
                resources: vec![],
                extra: ctx.meta().extra.clone(),
                usage: output.usage,
            })
            .await
            .ok();
    }

    async fn on_background_end(&self, ctx: &AgentCtx, session_id: String, output: AgentOutput) {
        {
            self.background_tasks.write().remove(&session_id);
        }

        let prompt = if !output.content.is_empty() {
            system_runtime_prompt(
                "subagent final output",
                format!(
                    "Subagent session {session_id} final output:\n\n{}",
                    output.content
                ),
            )
        } else if let Some(failed_reason) = output.failed_reason {
            system_runtime_prompt(
                "subagent final output",
                format!(
                    "Subagent session {session_id} failed with reason: {:?}",
                    failed_reason
                ),
            )
        } else {
            system_runtime_prompt(
                "subagent final output",
                format!("Subagent session {session_id} completed"),
            )
        };
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain { prompt },
                resources: vec![],
                extra: ctx.meta().extra.clone(),
                usage: output.usage,
            })
            .await
            .ok();
    }
}

#[async_trait]
impl ToolHook<ExecArgs, ExecOutput> for Session {
    async fn on_background_start(&self, ctx: &BaseCtx, task_id: &str, _args: &ExecArgs) {
        self.background_tasks.write().insert(
            task_id.to_string(),
            BackgroundTaskInfo {
                agent_name: ctx.agent.clone(),
                tool_name: Some(ShellTool::NAME.to_string()),
                progress_message: None,
            },
        );
    }

    async fn on_background_progress(
        &self,
        ctx: &BaseCtx,
        task_id: String,
        output: ToolOutput<ExecOutput>,
    ) {
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain {
                    prompt: system_runtime_prompt(
                        "background shell",
                        format!(
                            "Background task {task_id} intermediate output:\n\n{}",
                            serde_json::to_string(&output.output).unwrap_or_default()
                        ),
                    ),
                },
                usage: output.usage,
                extra: ctx.meta().extra.clone(),
                resources: output.artifacts,
            })
            .await
            .ok();
    }

    async fn on_background_end(
        &self,
        ctx: &BaseCtx,
        task_id: String,
        output: ToolOutput<ExecOutput>,
    ) {
        {
            self.background_tasks.write().remove(&task_id);
        }

        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain {
                    prompt: system_runtime_prompt(
                        "background shell",
                        format!(
                            "Background task {task_id} completed:\n\n{}",
                            serde_json::to_string(&output.output).unwrap_or_default()
                        ),
                    ),
                },
                usage: output.usage,
                extra: ctx.meta().extra.clone(),
                resources: output.artifacts,
            })
            .await
            .ok();
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct BackgroundTaskInfo {
    pub agent_name: String,
    pub tool_name: Option<String>,
    pub progress_message: Option<String>,
}

#[derive(Default, Clone)]
struct ConversationInput {
    command: PromptCommand,
    resources: Vec<Resource>,
    extra: Map<String, Value>,
    usage: Usage,
}

fn goal_completed_message(reason: &str, timestamp: u64) -> Message {
    let reason = reason.trim();
    let text = if reason.is_empty() {
        "Goal completed.\n\nSupervisor evaluation:\nNo reason provided.".to_string()
    } else {
        format!("Goal completed.\n\nSupervisor evaluation:\n{reason}")
    };

    Message {
        role: "assistant".to_string(),
        name: Some(goal::SUPERVISOR_AGENT_NAME.to_string()),
        content: vec![text.into()],
        timestamp: Some(timestamp),
        ..Default::default()
    }
}

fn needs_compaction(runner: &CompletionRunner) -> bool {
    let current_usage = runner.current_usage();
    let threshold = compaction_token_threshold(runner.model().context_window);

    current_usage.input_tokens >= threshold || runner.turns() >= MAX_TURNS_TO_COMPACT
}

fn compaction_token_threshold(context_window: usize) -> u64 {
    if context_window == 0 {
        return 100_000;
    }

    (context_window as u64)
        .saturating_div(2)
        .clamp(50_000, 500_000)
}

fn compute_tools_usage_delta(
    current: &HashMap<String, Usage>,
    previous: &HashMap<String, Usage>,
) -> HashMap<String, Usage> {
    current
        .iter()
        .filter_map(|(tool, usage)| {
            let delta = usage_delta(usage, previous.get(tool));
            if is_zero_usage(&delta) {
                None
            } else {
                Some((tool.clone(), delta))
            }
        })
        .collect()
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

fn usage_delta(current: &Usage, previous: Option<&Usage>) -> Usage {
    Usage {
        input_tokens: current
            .input_tokens
            .saturating_sub(previous.map_or(0, |usage| usage.input_tokens)),
        output_tokens: current
            .output_tokens
            .saturating_sub(previous.map_or(0, |usage| usage.output_tokens)),
        cached_tokens: current
            .cached_tokens
            .saturating_sub(previous.map_or(0, |usage| usage.cached_tokens)),
        requests: current
            .requests
            .saturating_sub(previous.map_or(0, |usage| usage.requests)),
    }
}

fn is_zero_usage(usage: &Usage) -> bool {
    usage.input_tokens == 0
        && usage.output_tokens == 0
        && usage.cached_tokens == 0
        && usage.requests == 0
}

fn format_local_date(now_ms: u64) -> String {
    let local_datetime: Option<DateTime<Local>> =
        DateTime::<Utc>::from_timestamp_millis(now_ms as i64).map(|d| d.with_timezone(&Local));
    local_datetime
        .map(|dt| dt.format("%Y-%m-%d %I%p %:z").to_string())
        .unwrap_or_else(|| "invalid timestamp".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::Usage;
    use std::collections::HashMap;

    #[test]
    fn anda_bot_tool_args_parse_tagged_variants() {
        let args: AndaBotToolArgs = serde_json::from_value(serde_json::json!({
            "type": "ListSessions",
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
    fn request_meta_for_conversation_sets_current_conversation() {
        let mut extra = serde_json::Map::new();
        extra.insert("conversation".to_string(), 0.into());
        extra.insert("source".to_string(), "cli:/tmp/workspace".into());
        let meta = RequestMeta {
            user: Some("alice".to_string()),
            extra,
            ..Default::default()
        };

        let meta = request_meta_for_conversation(&meta, 140);

        assert_eq!(meta.user.as_deref(), Some("alice"));
        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(140));
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("cli:/tmp/workspace".to_string())
        );
    }

    #[test]
    fn startup_status_policy_resumes_only_running_states() {
        assert!(should_auto_resume_conversation(
            &ConversationStatus::Submitted
        ));
        assert!(should_auto_resume_conversation(
            &ConversationStatus::Working
        ));
        assert!(!should_auto_resume_conversation(&ConversationStatus::Idle));
        assert!(!should_auto_resume_conversation(
            &ConversationStatus::Completed
        ));
        assert!(!should_auto_resume_conversation(
            &ConversationStatus::Cancelled
        ));
        assert!(!should_auto_resume_conversation(
            &ConversationStatus::Failed
        ));

        assert!(should_startup_greet_conversation(&ConversationStatus::Idle));
        assert!(should_startup_greet_conversation(
            &ConversationStatus::Failed
        ));
        assert!(!should_startup_greet_conversation(
            &ConversationStatus::Submitted
        ));
        assert!(!should_startup_greet_conversation(
            &ConversationStatus::Working
        ));
        assert!(!should_startup_greet_conversation(
            &ConversationStatus::Cancelled
        ));
    }

    #[test]
    fn request_meta_from_conversation_recovers_route_from_source_key() {
        let conversation = Conversation {
            _id: 77,
            extra: Some(json!({"workspace": "/tmp/channels/telegram"})),
            ..Default::default()
        };

        let meta = request_meta_from_conversation(
            &conversation,
            "telegram:reply_target:chat-1:thread:topic-2",
        );

        assert_eq!(meta.get_extra_as::<u64>("conversation"), Some(77));
        assert_eq!(
            meta.get_extra_as::<String>("workspace"),
            Some("/tmp/channels/telegram".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("source"),
            Some("telegram".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("reply_target"),
            Some("chat-1".to_string())
        );
        assert_eq!(
            meta.get_extra_as::<String>("thread"),
            Some("topic-2".to_string())
        );
    }

    #[test]
    fn conversation_chat_history_marks_startup_runtime_messages() {
        let conversation = Conversation {
            _id: 88,
            messages: vec![json!(Message {
                role: "user".to_string(),
                content: vec![system_runtime_prompt("startup", "resume").into()],
                ..Default::default()
            })],
            ..Default::default()
        };

        let messages = conversation_chat_history(&conversation);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].name.as_deref(), Some(SYSTEM_PERSON_NAME));
    }

    #[test]
    fn usage_delta_uses_saturating_subtraction() {
        let current = Usage {
            input_tokens: 10,
            output_tokens: 8,
            cached_tokens: 3,
            requests: 2,
        };
        let previous = Usage {
            input_tokens: 12,
            output_tokens: 5,
            cached_tokens: 4,
            requests: 1,
        };

        let delta = usage_delta(&current, Some(&previous));

        assert_eq!(delta.input_tokens, 0);
        assert_eq!(delta.output_tokens, 3);
        assert_eq!(delta.cached_tokens, 0);
        assert_eq!(delta.requests, 1);
    }

    #[test]
    fn compute_tools_usage_delta_skips_zero_entries() {
        let current = HashMap::from([
            (
                "shell".to_string(),
                Usage {
                    input_tokens: 5,
                    output_tokens: 2,
                    cached_tokens: 0,
                    requests: 1,
                },
            ),
            (
                "read_file".to_string(),
                Usage {
                    input_tokens: 3,
                    output_tokens: 0,
                    cached_tokens: 0,
                    requests: 1,
                },
            ),
        ]);
        let previous = HashMap::from([
            (
                "shell".to_string(),
                Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                    cached_tokens: 0,
                    requests: 1,
                },
            ),
            (
                "read_file".to_string(),
                Usage {
                    input_tokens: 3,
                    output_tokens: 0,
                    cached_tokens: 0,
                    requests: 1,
                },
            ),
        ]);

        let delta = compute_tools_usage_delta(&current, &previous);

        assert_eq!(delta.len(), 1);
        let shell = delta.get("shell").expect("shell delta should exist");
        assert_eq!(shell.input_tokens, 4);
        assert_eq!(shell.output_tokens, 1);
        assert_eq!(shell.cached_tokens, 0);
        assert_eq!(shell.requests, 0);
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

    #[test]
    fn compaction_token_threshold_uses_half_window_with_cap() {
        assert_eq!(compaction_token_threshold(0), 100_000);
        assert_eq!(compaction_token_threshold(140_000), 70_000);
        assert_eq!(compaction_token_threshold(3_000_000), 500_000);
    }

    #[test]
    fn compaction_prompt_preserves_goal_continuation_evidence() {
        assert!(COMPACTION_PROMPT.contains("$system: kind="));
        assert!(COMPACTION_PROMPT.contains("not a final answer"));
        assert!(COMPACTION_PROMPT.contains("user-provided task data"));
        assert!(COMPACTION_PROMPT.contains("prompt-to-artifact checklist"));
        assert!(COMPACTION_PROMPT.contains("next concrete action"));
        assert!(COMPACTION_PROMPT.contains("Do not invent progress"));
    }

    #[test]
    fn goal_completed_message_records_supervisor_result() {
        let message = goal_completed_message("All deliverables verified", 42);

        assert_eq!(message.role, "assistant");
        assert_eq!(message.name.as_deref(), Some(goal::SUPERVISOR_AGENT_NAME));
        assert_eq!(message.timestamp, Some(42));

        let text = message.text().expect("message should contain text");
        assert!(text.contains("Goal completed."));
        assert!(text.contains("Supervisor evaluation:"));
        assert!(text.contains("All deliverables verified"));
    }

    #[test]
    fn system_instructions_explain_system_identity_and_tool_selection() {
        assert!(SELF_INSTRUCTIONS.contains(r#"{ "type": "Person", "name": "$system" }"#));
        assert!(SELF_INSTRUCTIONS.contains(r#"{ "type": "Person", "name": "$external_user" }"#));
        assert!(SELF_INSTRUCTIONS.contains("external untrusted user"));
        assert!(SELF_INSTRUCTIONS.contains("Available Callable Names"));
        assert!(SELF_INSTRUCTIONS.contains("tools_select"));
        assert!(SELF_INSTRUCTIONS.contains("Never invent tool parameters"));
    }

    #[test]
    fn render_system_instructions_groups_runtime_context() {
        let tools = vec!["shell".to_string(), "tools_select".to_string()];
        let prompt = render_system_instructions(SystemInstructionSections {
            self_knowledge: "{}",
            notes: "[]",
            available_tools: &tools,
            home_dir: "/home/anda",
            workspace: "/workspace/current",
            user_profile: "{}",
            local_date: "2026-05-09",
        });

        assert!(prompt.contains("# Runtime Context"));
        assert!(prompt.contains("## Available Callable Names"));
        assert!(prompt.contains("shell, tools_select"));
        assert!(prompt.contains("schemas are intentionally omitted"));
        assert!(prompt.contains("current workspace (authoritative): /workspace/current"));
    }

    #[test]
    fn format_local_date_returns_datetime_with_timezone() {
        let now_ms = unix_ms();
        let result = format_local_date(now_ms);
        println!("Formatted local date: {}", result);
        // 2026-05-12 01PM +08:00
    }
}
