use crate::util::tool_response::ToolResponse as Response;
use anda_brain::types::{FormationInputRef, InputContext};
use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, ContentPart, Document,
    Documents, FunctionDefinition, Message, Principal, RequestMeta, Resource, StateFeatures, Tool,
    ToolOutput, Usage,
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
        shell::{ShellCommandToolHook, ShellSessionScope, ShellTool, ShellToolHook},
        skill::{SkillManager, SkillToolHook, SkillsListTool, SkillsReadHook, SkillsReadTool},
    },
    hook::DynAgentHook,
    memory::{Conversation, ConversationRef, ConversationStatus},
    model::{Model, Models},
    subagent::SubAgentManager,
    unix_ms,
};
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
pub(crate) mod memory_policy;
mod meta;
mod runner;
#[cfg(feature = "mib")]
mod runner_managed;
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
    prompt::{PromptCommand, skill_command_directive},
    resources::ResourceStore,
    side,
    skill_library::{SkillLibrary, SkillUsageHook},
    system::{SYSTEM_PERSON_NAME, system_extra_user_context, system_runtime_prompt},
};
use crate::{
    brain, channel, cron,
    transcription::TranscriptionManager,
    tts::TtsManager,
    util::request_meta::{keys, request_meta_extra_as},
};

#[derive(Clone)]
pub struct AndaBot {
    inner: Arc<AndaBotInner>,
}

struct AndaBotInner {
    admission: Arc<crate::runtime_admission::Admission>,
    memory_access: Option<Arc<brain::MemoryAccess>>,
    plan_owner: Option<Principal>,
    brain: brain::Client,
    models: Arc<Models>,
    actions: Arc<ActionRuntime>,
    conversations: Arc<ConversationsTool>,
    resource_store: Arc<ResourceStore>,
    tool_dependencies: Vec<String>,
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

/// Name of `anda_engine`'s `ApplyPatchTool`, which has no `NAME` constant.
pub(crate) const APPLY_PATCH_NAME: &str = "apply_patch";
/// Name of `anda_engine`'s `ShellSessionTool`, which has no `NAME` constant.
pub(crate) const SHELL_SESSION_NAME: &str = "shell_session";

fn base_tool_dependencies() -> Vec<String> {
    let mut tools = vec![
        brain::Client::NAME.to_string(),
        GoalTool::NAME.to_string(),
        TOOLS_SEARCH_NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        TOOLS_GROUPS_NAME.to_string(),
        ShellTool::NAME.to_string(),
        SHELL_SESSION_NAME.to_string(),
        AskUserChoiceTool::NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
        EditFileTool::NAME.to_string(),
        WriteFileTool::NAME.to_string(),
        APPLY_PATCH_NAME.to_string(),
        McpServerTool::NAME.to_string(),
        SubAgentManager::NAME.to_string(),
        SkillManager::NAME.to_string(),
        SkillsListTool::NAME.to_string(),
        SkillsReadTool::NAME.to_string(),
        cron::CreateCronTool::NAME.to_string(),
        cron::UpdateCronJobTool::NAME.to_string(),
        cron::ManageCronJobTool::NAME.to_string(),
        cron::ListCronJobsTool::NAME.to_string(),
        cron::ListCronRunsTool::NAME.to_string(),
    ];
    tools.extend(ChromeBrowserTool::NAMES.map(String::from));
    tools.extend(multimodal::media_agent_names());
    tools
}

fn base_tools() -> Vec<String> {
    vec![
        brain::Client::NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        TOOLS_GROUPS_NAME.to_string(),
        ShellTool::NAME.to_string(),
        AskUserChoiceTool::NAME.to_string(),
        SubAgentManager::NAME.to_string(),
        SkillManager::NAME.to_string(),
    ]
}

/// Everything that varies between the entry points that open a live session
/// (a user request in [`AndaBot::run`], startup recovery). The shared wiring
/// — channels, [`ActionSession`], hook installation, registration — lives in
/// [`AndaBot::create_session`].
struct SessionSpec<'a> {
    sess_id: Xid,
    caller: String,
    workspace: String,
    source_key: String,
    conversation_id: u64,
    request_meta: SessionRequestMeta,
    /// Live request metadata; decides whether the formation counterparty is
    /// the caller or a scoped external-user name.
    meta: &'a RequestMeta,
    initial_goal: Option<String>,
    formation_topic: Option<&'static str>,
    active_at_ms: u64,
}

impl AndaBot {
    pub const NAME: &'static str = "anda_bot";

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        brain: brain::Client,
        models: Arc<Models>,
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
                admission: Arc::new(crate::runtime_admission::Admission::default()),
                memory_access: None,
                plan_owner: None,
                brain,
                models,
                actions,
                home_dir,
                conversations,
                resource_store,
                tool_dependencies,
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

    fn ensure_plan_owner(&self, caller: &Principal, meta: &RequestMeta) -> Result<(), BoxError> {
        if self
            .inner
            .models
            .model_names()
            .iter()
            .any(|name| name.starts_with("chatgpt:"))
            && (request_meta_extra_as::<bool>(meta, keys::EXTERNAL_USER).unwrap_or(false)
                || self.inner.plan_owner.is_some_and(|owner| &owner != caller))
        {
            return Err("ChatGPT plan providers are owner-only. Use API-key providers for external or shared users.".into());
        }
        Ok(())
    }

    pub(crate) fn with_plan_owner(mut self, owner: Principal) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("configure plan owner before sharing")
            .plan_owner = Some(owner);
        self
    }

    pub(crate) fn with_memory_access(mut self, access: Arc<brain::MemoryAccess>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("configure memory access before sharing the Bot")
            .memory_access = Some(access);
        self
    }

    pub(crate) fn with_admission(
        mut self,
        admission: Arc<crate::runtime_admission::Admission>,
    ) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("configure admission before sharing")
            .admission = admission;
        self
    }
    pub(crate) fn admission(&self) -> Arc<crate::runtime_admission::Admission> {
        self.inner.admission.clone()
    }
    pub(crate) async fn update_ready(&self) -> bool {
        if self.inner.admission.status().active != 0 || self.has_busy_sessions() {
            return false;
        }
        let Ok(brain) = self.inner.brain.brain_status().await else {
            return false;
        };
        !brain.formation_processing
            && !brain.maintenance_processing
            && self.inner.admission.status().active == 0
            && !self.has_busy_sessions()
    }

    pub async fn status(&self) -> Result<AndaBotStatus, BoxError> {
        let conversations = self.inner.conversations.conversations_len() as u64;
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

    pub(super) fn events(&self) -> Arc<super::app_protocol::AppEvents> {
        self.inner.conversations.events.clone()
    }

    /// Builds a [`Session`], installs its context hooks (goal state, live
    /// request meta, actions, agent/shell hooks), and registers it. Returns
    /// the receivers the session runner consumes; spawn the runner only after
    /// this call so every hook is in place when the first turn executes.
    fn create_session(
        &self,
        ctx: &AgentCtx,
        spec: SessionSpec<'_>,
    ) -> (
        Arc<Session>,
        tokio::sync::mpsc::Receiver<ConversationInput>,
        tokio::sync::mpsc::Receiver<ActionEvent>,
    ) {
        let (sender, rx) = tokio::sync::mpsc::channel::<ConversationInput>(42);
        let (action_sender, action_rx) = tokio::sync::mpsc::channel::<ActionEvent>(42);
        let external_user =
            request_meta_extra_as::<bool>(spec.meta, keys::EXTERNAL_USER).unwrap_or(false);
        let formation_counterparty = if external_user {
            scoped_external_user_name_from_meta(spec.meta)
        } else {
            spec.caller.clone()
        };

        let conversation_id = Arc::new(AtomicU64::new(spec.conversation_id));
        let session_id = spec.sess_id.to_string();
        let mut memory_source =
            brain::product::source_identity(&spec.caller, spec.conversation_id, Some(&session_id));
        memory_source.parents.extend(
            ctx.base
                .get_state::<memory_policy::InheritedMemorySources>()
                .unwrap_or_default()
                .0,
        );
        memory_source.parents.sort();
        memory_source.parents.dedup();
        ctx.base.set_state(memory_source);
        spec.request_meta
            .set_cron_workspace(ctx.base.get_state::<cron::CronWorkspaceGrant>());
        let session = Arc::new(Session {
            control: Default::default(),
            background_controls: Default::default(),
            memory_policy: memory_policy::MemoryPolicy::current(&ctx.base),
            id: spec.sess_id,
            caller: spec.caller.clone(),
            workspace: spec.workspace,
            source_key: spec.source_key.clone(),
            conversation_id: conversation_id.clone(),
            sender,
            actions: ActionSession::new(
                self.inner.actions.clone(),
                action_sender,
                spec.caller,
                session_id,
                conversation_id,
                self.inner.models.clone(),
                self.inner.home_dir.clone(),
            ),
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            background_progress_outputs: Arc::new(RwLock::new(HashMap::new())),
            goal: Arc::new(RwLock::new(spec.initial_goal.map(goal::GoalState::new))),
            request_meta: spec.request_meta.clone(),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            formation_backoff_until: AtomicU64::new(0),
            goal_check_backoff_until: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(spec.active_at_ms)),
            finish_when_idle: AtomicBool::new(
                request_meta_extra_as::<bool>(spec.meta, keys::FINISH_WHEN_IDLE).unwrap_or(false),
            ),
            runner_idle: AtomicBool::new(false),
            formation_context: Some(InputContext {
                counterparty: Some(formation_counterparty),
                agent: Some(AndaBot::NAME.to_string()),
                source: Some(spec.source_key),
                topic: spec.formation_topic.map(str::to_string),
            }),
        });

        ctx.base.set_state(brain::RecallTurn::default());
        ctx.base.set_state(super::resources::SessionArtifacts::new(
            self.inner.resource_store.clone(),
        ));
        ctx.base.set_state(GoalToolState::new(
            session.goal.clone(),
            session.active_at.clone(),
        ));
        ctx.base.set_state(spec.request_meta);
        ctx.base.set_state(session.actions.clone());
        ctx.base.set_state(DynAgentHook::new(session.clone()));
        // One process-session capability per conversation; nested agents inherit it.
        ctx.base.set_state(ShellSessionScope::new());
        ctx.base
            .set_state(ShellCommandToolHook::new(session.clone()));
        ctx.base.set_state(ShellToolHook::new(session.clone()));
        // Inline skills are only read, never called; book those reads so the
        // Dashboard can tell a used skill from an unused one.
        ctx.base
            .set_state(SkillToolHook::new(Arc::new(SkillUsageHook)));
        ctx.base
            .set_state(SkillsReadHook::new(Arc::new(SkillUsageHook)));
        self.insert_session(session.clone());

        (session, rx, action_rx)
    }

    fn insert_session(&self, task: Arc<Session>) {
        self.inner.sessions.write().insert(task.id, task);
    }

    /// The session table without sessions whose runner has exited.
    fn live_sessions(&self) -> parking_lot::RwLockWriteGuard<'_, HashMap<Xid, Arc<Session>>> {
        let mut sessions = self.inner.sessions.write();
        sessions.retain(|_, session| !session.sender.is_closed());
        sessions
    }

    fn get_session(&self, key: &Xid) -> Option<Arc<Session>> {
        self.live_sessions().get(key).cloned()
    }

    /// The live session `caller` can join: the one with id `key`, otherwise
    /// the caller's session for `source_key`. A session is only joinable by
    /// the caller that owns it: several managers without an explicit
    /// source/workspace share the same fallback source_key, and joining
    /// another caller's session would leak its chat history and reroute its
    /// replies.
    fn find_joinable_session(
        &self,
        key: &Xid,
        source_key: &str,
        caller: &str,
    ) -> Option<Arc<Session>> {
        let sessions = self.live_sessions();
        sessions
            .get(key)
            .filter(|session| session.caller == caller)
            .or_else(|| {
                sessions
                    .values()
                    .find(|session| session.caller == caller && session.source_key == source_key)
            })
            .cloned()
    }

    fn detach_session(&self, key: &Xid) -> Option<Arc<Session>> {
        self.inner.sessions.write().remove(key)
    }

    // A live session can itself be idle: its completion runner has no
    // pending work and no background tasks are running. The bot is busy only
    // while some session has work in flight.
    pub(crate) fn has_busy_sessions(&self) -> bool {
        self.live_sessions()
            .values()
            .any(|session| !session.is_idle())
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
                        let Ok(_permit) = this.inner.admission.enter() else {
                            break;
                        };
                        hook.on_idle(idle_ms).await;
                    }
                }
            }
        });
    }

    fn active_sessions(&self) -> Vec<Arc<Session>> {
        // Snapshot `active_at` once per session up front. It is a shared atomic that running
        // session tasks update concurrently; reading it inside the comparator would let the
        // observed order change mid-sort, which makes the comparison a non-total order and
        // panics with "comparison function does not correctly implement a total order".
        let mut active = self
            .live_sessions()
            .values()
            .map(|session| (session.active_at.load(Ordering::SeqCst), session.clone()))
            .collect::<Vec<_>>();

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
        let session = self
            .live_sessions()
            .values()
            .find(|session| session.id.to_string() == session_id)
            .cloned();
        session.map(|session| session.state(now_ms))
    }

    async fn persist_conversation_state(
        &self,
        conversation: &Conversation,
    ) -> Result<(), BoxError> {
        self.inner
            .conversations
            .conversations
            .update_conversation(conversation._id, conversation.to_changes()?)
            .await?;
        if let Err(err) = self
            .inner
            .conversations
            .sync_source_status(conversation)
            .await
        {
            log::error!(conversation = conversation._id; "Failed to sync source status: {err:?}");
        }
        self.inner
            .conversations
            .events
            .changed(&conversation.user.to_string());
        Ok(())
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

    /// The model a session's next turn runs on, resolved as the completion
    /// runner does: the pinned request model, otherwise the context's label.
    fn next_turn_model(&self, ctx: &AgentCtx, pinned: Option<&str>) -> Option<Model> {
        self.inner.models.resolve(pinned.unwrap_or(&ctx.label))
    }

    async fn complete_conversation_if_unfinished(
        &self,
        conversation: &mut Conversation,
        now_ms: u64,
    ) -> Result<(), BoxError> {
        if is_terminal_conversation_status(&conversation.status) {
            return Ok(());
        }

        conversation.status = ConversationStatus::Completed;
        conversation.updated_at = now_ms;
        self.persist_conversation_state(conversation).await
    }

    async fn submit_formation(
        &self,
        submission: brain::FormationSubmission,
        messages: &[Message],
        context: &Option<InputContext>,
        timestamp: &Option<String>,
    ) -> Result<brain::FormationSubmission, BoxError> {
        self.inner
            .brain
            .submit_formation_window(
                submission,
                FormationInputRef {
                    messages,
                    context,
                    timestamp,
                },
            )
            .await
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
        // A side request is one-shot and never joins the conversation history,
        // so its attachments go to the model as they are.
        let mut output = subagent
            .run(
                ctx.child(&subagent.name, super::ACTIVE_MODEL_LABEL)?,
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

    /// The newest conversation in the child chain that starts at `conv_id`.
    /// With `user`, the chain ends before the first conversation the user
    /// does not own, so `None` means `conv_id` itself is someone else's.
    async fn latest_conversation_in_chain(
        &self,
        conv_id: u64,
        user: Option<Principal>,
    ) -> Result<Option<Conversation>, BoxError> {
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

        Ok(latest)
    }

    /// The admission permit a request holds while it runs. Cron runs and
    /// calls from inside a session were admitted by their caller, and
    /// `/stop` and `/cancel` get through while new work is refused.
    fn admit(
        &self,
        ctx: &AgentCtx,
        command: &PromptCommand,
    ) -> Result<Option<crate::runtime_admission::Permit>, BoxError> {
        if ctx
            .base
            .get_state::<crate::runtime_admission::AdmittedCron>()
            .is_some()
            || ctx.base.get_state::<SessionRequestMeta>().is_some()
        {
            return Ok(None);
        }
        let permit = if matches!(
            command,
            PromptCommand::Stop { .. } | PromptCommand::Cancel { .. }
        ) {
            self.inner.admission.enter_existing()
        } else {
            self.inner.admission.enter()?
        };
        Ok(Some(permit))
    }

    /// Hands `input` to a live session of the caller, or returns it when the
    /// session's runner has already shut down.
    async fn join_session(
        &self,
        ctx: &AgentCtx,
        session: &Session,
        mut input: ConversationInput,
    ) -> Result<AgentOutput, Box<ConversationInput>> {
        if request_meta_extra_as::<bool>(ctx.meta(), keys::FINISH_WHEN_IDLE).unwrap_or(false) {
            session.finish_when_idle.store(true, Ordering::SeqCst);
        }
        let conversation_id = session.conversation_id.load(Ordering::SeqCst);
        session
            .request_meta
            .set(request_meta_for_conversation(ctx.meta(), conversation_id));
        session
            .request_meta
            .set_cron_workspace(ctx.base.get_state::<cron::CronWorkspaceGrant>());
        let control = matches!(
            input.command,
            PromptCommand::Stop { .. } | PromptCommand::Cancel { .. }
        );
        // A user writing in chat answers any choice card the agent is
        // waiting on; scheduled prompts are not the user's answer.
        let answers_choices = matches!(
            input.command,
            PromptCommand::Plain { .. } | PromptCommand::Steer { .. }
        ) && input.cron_receipt.is_none()
            && !input.extra.contains_key(keys::CRON_JOB_ID);
        if let Some(receipt) = &mut input.cron_receipt {
            session.bind_cron_receipt(receipt);
        }
        session.runner_idle.store(false, Ordering::SeqCst);
        if let Err(err) = session.sender.send(input).await {
            log::warn!("Failed to enqueue prompt for processing conversation {conversation_id}");
            self.detach_session(&session.id);
            return Err(Box::new(err.0));
        }
        if control {
            session.control.request();
        }
        if answers_choices {
            session.actions.answer_choices_in_chat().await;
        }
        Ok(AgentOutput {
            conversation: (conversation_id > 0).then_some(conversation_id),
            session: Some(session.id.to_string()),
            ..Default::default()
        })
    }

    /// The caller's latest conversations as context for a conversation that
    /// starts fresh, each cut to its recent text messages.
    async fn history_conversations_message(
        &self,
        caller: &Principal,
        current: Option<&Conversation>,
        now_ms: u64,
    ) -> Result<Option<Message>, BoxError> {
        let (mut conversations, _) = self
            .inner
            .conversations
            .conversations
            .list_conversations_by_user(caller, None, Some(2))
            .await?;
        if let Some(conv) = current
            && !conversations.iter().any(|c| c._id == conv._id)
        {
            conversations.push(conv.clone());
        }
        let conversations = self
            .inner
            .conversations
            .filter_memory_sources(conversations)
            .await?;
        if conversations.is_empty() {
            return Ok(None);
        }

        let documents = Documents::new(
            "user_history_conversations".to_string(),
            conversations
                .into_iter()
                .map(|conv| Document::from(recent_text_messages(conv)))
                .collect(),
        );
        Ok(Some(Message {
            role: "user".into(),
            content: vec![documents.to_string().into()],
            name: Some(SYSTEM_PERSON_NAME.into()),
            timestamp: Some(now_ms),
            ..Default::default()
        }))
    }

    /// The tools a session's first request loads: the base set, `extra`, the
    /// browser tools while the caller has a browser this request may drive,
    /// and the caller's three most used other tools.
    fn initial_tools(
        &self,
        caller: Principal,
        meta: &RequestMeta,
        available_tools: &[String],
        extra: Vec<String>,
    ) -> UniqueVec<String> {
        let mut tools = UniqueVec::from(base_tools());
        tools.extend(extra);
        if self.inner.browser_manager.is_available(caller, meta) {
            tools.extend(ChromeBrowserTool::NAMES.map(String::from));
        }
        let most_used = self
            .inner
            .conversations
            .tool_usage_with(|usage| select_most_used_tools(available_tools, &tools, usage, 3));
        tools.extend(most_used);
        tools
    }

    /// Points `source_key` at a conversation; a failed save is logged and the
    /// request goes on.
    async fn record_source_conversation(&self, source_key: &str, state: SourceState) {
        if let Err(err) = self
            .inner
            .conversations
            .update_source_state(source_key.to_string(), state)
            .await
        {
            log::error!("Failed to update_source_state: {err:?}");
        }
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
            this.startup_self_check(ctx).await;
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

        self.ensure_plan_owner(caller, ctx.meta())?;

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
        let _permit = self.admit(&ctx, &command)?;

        let now_ms = unix_ms();
        let requested = requested_memory_mode(&ctx)?;
        let RequestState {
            workspace,
            source_key,
            source_state,
            conversation: requested_conversation,
            ..
        } = self.inner.conversations.state_from_meta(ctx.meta());
        let is_new = matches!(command, PromptCommand::New { .. });
        // One chain walk serves the memory policy, a side command and the
        // session lookup. Only `/new` may start over when the saved chain
        // cannot be read. A source shared with another caller may point at
        // their conversation, which this caller starts over from.
        let mut current_conversation = if requested_conversation > 0 {
            match self
                .latest_conversation_in_chain(requested_conversation, Some(*caller))
                .await
            {
                Ok(conversation) => conversation,
                Err(err) if !is_new => return Err(err),
                Err(_) => None,
            }
        } else {
            None
        };
        let (policy, inherited_sources) = install_memory_policy(
            &ctx,
            &command,
            requested,
            current_conversation.as_ref().filter(|_| !is_new),
        )?;
        let home_dir = self.inner.home_dir.to_string_lossy().to_string();

        ctx.base.set_state(AgentInfo);

        if let PromptCommand::Side { prompt } = &command {
            let available_tools = available_tool_names(&ctx).await;
            let instructions = self
                .build_system_instructions(&ctx, &home_dir, &workspace, &available_tools, now_ms)
                .await?;
            let side_conversation_id = current_conversation.as_ref().map(|conv| conv._id);
            return crate::util::boxed(self.run_side_command(
                &ctx,
                instructions,
                prompt.clone(),
                resources,
                side_conversation_id,
            ))
            .await;
        }

        let mut ancestors = current_conversation.as_ref().map(recent_ancestors);

        let mut input = ConversationInput {
            cron_receipt: ctx
                .base
                .get_state::<cron::AgentSubmission>()
                .and_then(|submission| submission.take()),
            command,
            resources,
            extra: ctx.meta().extra.clone(),
            usage: Usage::default(),
        };
        let current_conversation_id = current_conversation.as_ref().map(|conv| conv._id);
        if let Some(id) = current_conversation_id {
            input
                .extra
                .insert(keys::CONVERSATION.to_string(), id.into());
        }

        let caller_id = caller.to_string();
        let mut sess_id = current_conversation
            .as_ref()
            .and_then(|conv| conv.thread)
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
        let mut available_tools = Vec::new();
        let session_creation_guard = loop {
            let guard = self.inner.session_creation_lock.lock().await;
            if let Some(session) = self.find_joinable_session(&sess_id, &source_key, &caller_id) {
                if !is_new {
                    if requested.is_some_and(|mode| mode != session.memory_policy.mode) {
                        return Err("Change memory mode in a new conversation with /new".into());
                    }
                    // Release the lock first: enqueueing can wait on a full
                    // channel and must not stall unrelated requests.
                    drop(guard);
                    match self.join_session(&ctx, &session, input).await {
                        Ok(output) => return Ok(output),
                        Err(rejected) => input = *rejected,
                    }
                    continue;
                }

                detached_conversation_id = session.conversation_id.load(Ordering::SeqCst);
                if let Some(session) = self.detach_session(&session.id) {
                    session.finish_when_idle.store(true, Ordering::SeqCst);
                    detached_existing_session = true;
                }
                if Some(detached_conversation_id) != current_conversation_id {
                    // Fetch the latest ancestors in the detached session for
                    // the /new command. The chain walk is a sequence of DB
                    // reads, so release the creation lock first: holding it
                    // here would stall every unrelated session creation.
                    drop(guard);
                    if let Ok(Some(conv)) = self
                        .latest_conversation_in_chain(detached_conversation_id, Some(*caller))
                        .await
                    {
                        ancestors = Some(recent_ancestors(&conv));
                    }
                    continue;
                }
            }

            if instructions.is_none() {
                drop(guard);
                available_tools = available_tool_names(&ctx).await;
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
            mut cron_receipt,
            command,
            resources,
            mut extra,
            ..
        } = input;
        // Session lifecycle control is not user context for the model.
        extra.remove(keys::FINISH_WHEN_IDLE);
        let instructions =
            instructions.expect("system instructions are built before session creation");

        let mut initial_goal = None;
        let mut session_tools = Vec::new();
        let mut content: Vec<ContentPart> = Vec::new();
        let mut force_standalone_conversation = false;
        let prompt = match command {
            PromptCommand::Plain { prompt }
            | PromptCommand::Steer { prompt }
            | PromptCommand::Loop { prompt } => prompt,
            PromptCommand::Goal { prompt } => {
                initial_goal = Some(prompt.clone());
                session_tools.push(GoalTool::NAME.to_string());
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
                let (callable, directive) =
                    skill_command_directive(self.inner.skill_library.subagent_set(), &skill);
                session_tools.extend(callable);
                content.push(system_runtime_prompt("prompt command", directive).into());
                prompt
            }
            PromptCommand::Invalid { reason } => return Err(reason.into()),
            PromptCommand::Side { .. } => unreachable!(),
            PromptCommand::New { prompt } => {
                if !detached_existing_session
                    && let Some(conversation) = current_conversation.as_mut()
                {
                    self.complete_conversation_if_unfinished(conversation, now_ms)
                        .await?;
                }

                let Some(prompt) = prompt else {
                    if detached_conversation_id > 0
                        && source_state.conv_id != detached_conversation_id
                    {
                        self.record_source_conversation(
                            &source_key,
                            SourceState {
                                conv_id: detached_conversation_id,
                                status: ConversationStatus::Cancelled,
                                timestamp: now_ms,
                                user: Some(*caller),
                            },
                        )
                        .await;
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

        let should_continue = !force_standalone_conversation
            && current_conversation
                .as_ref()
                .is_some_and(|conv| should_continue_conversation(&conv.status));

        let mut chat_history: Vec<Message> = Vec::new();
        let mut reserve_chat_history: Vec<Message> = Vec::new();
        let conversation = if should_continue && let Some(conv) = current_conversation {
            // 如果 conversation 已经存在，允许 prompt 为空（会进入等待模式）
            reserve_chat_history = conversation_chat_history(&conv);
            chat_history = reserve_chat_history.clone();
            conv
        } else {
            if prompt.trim().is_empty() && !has_resources {
                return Err("prompt cannot be empty".into());
            }

            if !force_standalone_conversation
                && policy.may_read()
                && let Some(message) = self
                    .history_conversations_message(caller, current_conversation.as_ref(), now_ms)
                    .await?
            {
                chat_history.push(message);
            }

            let mut conv = Conversation {
                user: *caller,
                thread: Some(sess_id),
                messages: vec![],
                ancestors,
                resources: vec![],
                period: now_ms / 3600 / 1000,
                created_at: now_ms,
                updated_at: now_ms,
                extra: Some({
                    let mut extra = conversation_extra_without_id(ctx.meta());
                    policy.persist(&mut extra);
                    extra.insert("memory_source_parents".into(), json!(inherited_sources));
                    json!(extra)
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
                // The chain goes on in the child. Clients show a failure only
                // for a Failed status, and failed_reason is set only with it.
                if conversation.status == ConversationStatus::Failed {
                    conversation.status = ConversationStatus::Completed;
                    conversation.failed_reason = None;
                }
                self.persist_conversation_state(&conversation).await?;
            }

            conv._id = conv_id;
            conv
        };

        if source_state.conv_id != conversation._id {
            self.record_source_conversation(
                &source_key,
                SourceState {
                    conv_id: conversation._id,
                    status: conversation.status.clone(),
                    timestamp: now_ms,
                    user: Some(conversation.user),
                },
            )
            .await;
        }

        let conversation_id = conversation._id;
        let (session, rx, action_rx) = self.create_session(
            &ctx,
            SessionSpec {
                sess_id,
                caller: caller_id,
                workspace,
                source_key,
                conversation_id,
                request_meta: SessionRequestMeta::new(request_meta_for_conversation(
                    ctx.meta(),
                    conversation_id,
                )),
                meta: ctx.meta(),
                initial_goal,
                formation_topic: None,
                active_at_ms: unix_ms(),
            },
        );
        // The session is registered: a concurrent request for it now joins
        // and queues its input until the runner starts.
        drop(session_creation_guard);

        if let Some(receipt) = &mut cron_receipt {
            session.bind_cron_receipt(receipt);
        }

        // Attachments reach the model as references, so load the tools that
        // can inspect them instead of making the model discover them first.
        session_tools.extend(multimodal::media_agent_names_for(&resources));
        let tools = self.initial_tools(*caller, ctx.meta(), &available_tools, session_tools);
        let req = CompletionRequest {
            instructions,
            prompt,
            content,
            chat_history,
            tools: ctx.definitions(Some(&tools)).await,
            tool_choice_required: false,
            ..Default::default()
        };

        self.spawn_session_runner(
            ctx,
            req,
            resources,
            reserve_chat_history,
            session,
            conversation,
            rx,
            action_rx,
            system_extra_user_context(&extra),
            cron_receipt,
        );
        Ok(AgentOutput {
            conversation: Some(conversation_id),
            ..Default::default()
        })
    }
}

impl AndaBotInner {
    fn apply_merge_discovered_tools(&self, runner: &mut CompletionRunner) {
        if let Some(merge_discovered_tools) = merge_discovered_tools_for_model(
            &self.merge_discovered_tools_cache,
            &runner.model().model_name(),
        ) {
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
}

/// Whether a model takes discovered tools merged into its request: the known
/// policy of its family, otherwise what an earlier run of it found out.
fn merge_discovered_tools_for_model(
    cache: &RwLock<HashMap<String, bool>>,
    model_name: &str,
) -> Option<bool> {
    let key = merge_discovered_tools_model_key(model_name)?;
    if key.contains("deepseek") {
        Some(false)
    } else if key.starts_with("gpt") || key.contains("/gpt") || key.contains("chatgpt") {
        Some(true)
    } else {
        cache.read().get(&key).copied()
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

/// The memory mode a request asks for. The persisted policy is host-owned and
/// never accepted from request metadata.
fn requested_memory_mode(ctx: &AgentCtx) -> Result<Option<memory_policy::MemoryMode>, BoxError> {
    use memory_policy::{MODE_KEY, MemoryPolicy, POLICY_KEY};
    let extra = &ctx.meta().extra;
    if (extra.contains_key(POLICY_KEY) || extra.contains_key("memory_source_parents"))
        && ctx.base.get_state::<MemoryPolicy>().is_none()
    {
        return Err(
            "memory_policy is host-owned; select memory_mode for a new conversation".into(),
        );
    }
    let requested = extra
        .get(MODE_KEY)
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    if requested.is_some()
        && request_meta_extra_as::<bool>(ctx.meta(), keys::EXTERNAL_USER).unwrap_or(false)
    {
        return Err("External channel senders cannot select owner memory policies".into());
    }
    Ok(requested)
}

/// Resolves the memory policy and source ancestry of the conversation a
/// request continues (`None` for `/new` or a fresh source), and installs both
/// on the context for tools and nested agents.
fn install_memory_policy(
    ctx: &AgentCtx,
    command: &PromptCommand,
    requested: Option<memory_policy::MemoryMode>,
    continued: Option<&Conversation>,
) -> Result<(memory_policy::MemoryPolicy, Vec<String>), BoxError> {
    use memory_policy::{InheritedMemorySources, MemoryMode, MemoryPolicy};
    let inherited = ctx.base.get_state::<MemoryPolicy>();
    let mut sources = ctx
        .base
        .get_state::<anda_brain::product::SourceIdentity>()
        .map(|source| {
            let mut keys = source.parents;
            keys.push(source.key);
            keys
        })
        .unwrap_or_default();
    let policy = match continued {
        Some(conversation) => {
            let policy = MemoryPolicy::from_conversation(conversation)?;
            if let Some(parents) = conversation
                .extra
                .as_ref()
                .and_then(|v| v.get("memory_source_parents"))
            {
                sources.extend(serde_json::from_value::<Vec<String>>(parents.clone())?);
            }
            if requested.is_some_and(|mode| mode != policy.mode) {
                return Err("Change memory mode in a new conversation with /new".into());
            }
            policy
        }
        None => MemoryPolicy::new(
            requested
                .unwrap_or_else(|| inherited.as_ref().map_or(MemoryMode::Standard, |p| p.mode)),
        ),
    };
    if inherited.as_ref().is_some_and(|p| policy.mode < p.mode) {
        return Err("A child conversation cannot relax its parent's memory policy".into());
    }
    if inherited
        .as_ref()
        .is_some_and(|parent| parent.mode != policy.mode)
    {
        return Err("Nested calls cannot change their inherited memory mode".into());
    }
    if !policy.may_write() && matches!(command, PromptCommand::Side { .. }) {
        return Err("Side tasks are unavailable in restricted memory mode".into());
    }
    ctx.base.set_state(policy.clone());
    sources.sort();
    sources.dedup();
    if sources.len() > 15 {
        return Err("Memory source ancestry exceeds the supported nesting limit".into());
    }
    ctx.base.set_state(InheritedMemorySources(sources.clone()));
    Ok((policy, sources))
}

/// The latest ten conversation ids of a chain, ending with `conversation`.
fn recent_ancestors(conversation: &Conversation) -> Vec<u64> {
    let mut ids = conversation.ancestors.clone().unwrap_or_default();
    ids.push(conversation._id);
    if ids.len() > 10 {
        ids.drain(0..ids.len() - 10);
    }
    ids
}

/// How many text messages of each past conversation a fresh conversation
/// sees as history.
const HISTORY_TEXT_MESSAGES: usize = 20;

/// `conversation` cut to its last [`HISTORY_TEXT_MESSAGES`] messages that
/// carry text, with only their text: its saved history can be as long as the
/// model's context window, and tool traffic and reasoning add no context.
fn recent_text_messages(mut conversation: Conversation) -> Conversation {
    let mut recent = Vec::new();
    for value in conversation.messages.iter().rev() {
        if recent.len() == HISTORY_TEXT_MESSAGES {
            break;
        }
        let Ok(mut message) = serde_json::from_value::<Message>(value.clone()) else {
            continue;
        };
        message
            .content
            .retain(|part| matches!(part, ContentPart::Text { .. }));
        if !message.content.is_empty()
            && let Ok(value) = serde_json::to_value(message)
        {
            recent.push(value);
        }
    }
    recent.reverse();
    conversation.messages = recent;
    conversation
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
        assert!(!base_tools().contains(&McpServerTool::NAME.to_string()));
    }

    #[test]
    fn known_merge_discovered_tools_policy_covers_deepseek_and_gpt_models() {
        // A cached probe never overrides a known model family.
        let cache = RwLock::new(HashMap::from([("deepseek-v4-pro".to_string(), true)]));
        assert_eq!(
            merge_discovered_tools_for_model(&cache, "DeepSeek-V4-Pro"),
            Some(false)
        );
        assert_eq!(
            merge_discovered_tools_for_model(&cache, "openai/gpt-5.4"),
            Some(true)
        );
        assert_eq!(
            merge_discovered_tools_for_model(&cache, "chatgpt-codex"),
            Some(true)
        );
        assert_eq!(
            merge_discovered_tools_for_model(&cache, "gemini-3-pro"),
            None
        );
        assert_eq!(merge_discovered_tools_for_model(&cache, "  "), None);
    }

    #[test]
    fn merge_discovered_tools_cache_reuses_unknown_model_probe_result() {
        let cache = RwLock::new(HashMap::from([("custom-model".to_string(), true)]));

        assert_eq!(
            merge_discovered_tools_for_model(&cache, "CUSTOM-MODEL"),
            Some(true)
        );
        assert_eq!(
            merge_discovered_tools_for_model(&cache, "unknown-model"),
            None
        );
    }

    #[test]
    fn select_most_used_tools_prefers_high_request_tools() {
        let available_tools = vec![
            "shell".to_string(),
            "read_file".to_string(),
            "write_file".to_string(),
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
                "write_file".to_string(),
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

        assert_eq!(
            selected,
            vec!["read_file".to_string(), "write_file".to_string()]
        );
    }

    use crate::engine::ACTIVE_MODEL_LABEL;
    use crate::engine::browser::BrowserBridge;
    use crate::engine::multimodal::MediaUnderstandingAgent;
    use crate::engine::resources::ResourceStore;
    use crate::util::http_client::new_reqwest_client;
    use anda_core::{AgentInput, RequestMeta};
    use anda_engine::{
        engine::{AgentInfo, Engine, EngineRef},
        management::{BaseManagement, Visibility},
        model::Model,
    };
    use anda_kip::Response as KipResp;
    use axum::{Router, routing};
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
        crate::test_support::memory_db("anda_bot_run").await
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
                routing::post(|| async { axum::Json(json!({"result": {"name": "tester"}})) }),
            )
            .route(
                "/v1/anda_bot/formation",
                routing::post(|| async { axum::Json(json!({"result": AgentOutput::default()})) }),
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
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/v1/anda_bot")
    }

    async fn build_bot_engine(home: PathBuf) -> (Arc<Engine>, Arc<AndaBot>) {
        build_bot_engine_with_brain(home, spawn_brain_mock().await).await
    }

    async fn build_bot_engine_with_brain(
        home: PathBuf,
        brain_url: String,
    ) -> (Arc<Engine>, Arc<AndaBot>) {
        build_bot_engine_with_model(home, brain_url, Model::mock_implemented()).await
    }

    async fn build_bot_engine_with_model(
        home: PathBuf,
        brain_url: String,
        model: Model,
    ) -> (Arc<Engine>, Arc<AndaBot>) {
        let db = build_test_db().await;
        let brain_client = brain::Client::new(brain_url, Some("token".to_string()))
            .with_http_client(new_reqwest_client());

        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let conversations_tool = Arc::new(
            ConversationsTool::connect(
                db.clone(),
                "bot".to_string(),
                home.to_string_lossy().to_string(),
            )
            .await
            .unwrap(),
        );
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
            Default::default(),
        ));
        let cron_runtime = Arc::new(
            crate::cron::CronRuntime::connect(Arc::new(EngineRef::new()), db.clone())
                .await
                .unwrap(),
        );

        let bot = Arc::new(AndaBot::new(
            brain_client.clone(),
            Arc::new(Models::default()),
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
            .with_model(model)
            .register_tool(Arc::new(brain_client.clone()))
            .unwrap()
            .register_tool(Arc::new(FakeShellTool))
            .unwrap()
            .register_tool(Arc::new(
                anda_engine::extension::shell::ShellSessionTool::new(Arc::new(
                    anda_engine::extension::shell::NativeRuntime::new(home.clone()),
                )),
            ))
            .unwrap()
            .register_tool(Arc::new(crate::engine::ActionsTool::new(
                bot.action_runtime(),
            )))
            .unwrap()
            .register_tool(Arc::new(AskUserChoiceTool))
            .unwrap()
            .register_tool(Arc::new(GoalTool::new()))
            .unwrap()
            .register_tool(Arc::new(ReadFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(SearchFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(EditFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(WriteFileTool::with_workspaces(vec![])))
            .unwrap()
            .register_tool(Arc::new(
                anda_engine::extension::fs::ApplyPatchTool::with_workspaces(vec![]),
            ))
            .unwrap()
            .register_tool(Arc::new(cron::CreateCronTool::new(
                cron_runtime.store.clone(),
            )))
            .unwrap()
            .register_tool(Arc::new(cron::ListCronJobsTool::new(
                cron_runtime.store.clone(),
            )))
            .unwrap()
            .register_tool(Arc::new(cron::UpdateCronJobTool::new(
                cron_runtime.store.clone(),
            )))
            .unwrap()
            .register_tool(Arc::new(cron::ManageCronJobTool::new(
                cron_runtime.store.clone(),
            )))
            .unwrap()
            .register_tool(Arc::new(cron::ListCronRunsTool::new(
                cron_runtime.store.clone(),
            )))
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
            .register_tool(Arc::new(SkillsListTool::new(skills.skill_manager())))
            .unwrap()
            .register_tool(Arc::new(SkillsReadTool::new(skills.skill_manager())))
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

    async fn assert_conversation_reaches_status(
        bot: &AndaBot,
        conversation_id: u64,
        expected: ConversationStatus,
    ) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let conversation = bot
                .inner
                .conversations
                .conversations
                .get_conversation(conversation_id)
                .await
                .unwrap();
            if conversation.status == expected {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "conversation did not reach {expected:?}; current status is {:?}",
                conversation.status
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn memory_policy_full_runs_never_submit_restricted_sources_and_restore_without_profile_initialization()
     {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for mode in [
            memory_policy::MemoryMode::Off,
            memory_policy::MemoryMode::NoStore,
        ] {
            let reads = Arc::new(AtomicUsize::new(0));
            let writes = Arc::new(AtomicUsize::new(0));
            let r = reads.clone();
            let w = writes.clone();
            let f = writes.clone();
            let app = Router::new()
                .route(
                    "/execute_kip_readonly",
                    routing::post(move || {
                        let r = r.clone();
                        async move {
                            r.fetch_add(1, Ordering::SeqCst);
                            axum::Json(serde_json::to_value(KipResp::ok(json!([]))).unwrap())
                        }
                    }),
                )
                .route(
                    "/get_or_init_user",
                    routing::post(move || {
                        let w = w.clone();
                        async move {
                            w.fetch_add(1, Ordering::SeqCst);
                            axum::Json(json!({"result":{}}))
                        }
                    }),
                )
                .route(
                    "/formation",
                    routing::post(move || {
                        let f = f.clone();
                        async move {
                            f.fetch_add(1, Ordering::SeqCst);
                            axum::Json(json!({"result":{"content":""}}))
                        }
                    }),
                );
            let url = crate::test_support::spawn_http_mock(app).await;
            let dir = tempfile::tempdir().unwrap();
            let (engine, bot) = build_bot_engine_with_brain(dir.path().into(), url).await;
            let mut input = AgentInput::new(AndaBot::NAME.into(), "/new private source".into());
            input.meta = Some(RequestMeta {
                extra: serde_json::Map::from_iter([
                    ("memory_mode".into(), json!(mode)),
                    (keys::FINISH_WHEN_IDLE.into(), true.into()),
                ]),
                ..Default::default()
            });
            let output = engine.agent_run(test_caller(), input).await.unwrap();
            let id = output.conversation.unwrap();
            assert_conversation_reaches_status(&bot, id, ConversationStatus::Completed).await;
            let conversation = bot
                .inner
                .conversations
                .conversations
                .get_conversation(id)
                .await
                .unwrap();
            assert_eq!(
                memory_policy::MemoryPolicy::from_conversation(&conversation)
                    .unwrap()
                    .mode,
                mode
            );
            let ctx = engine
                .ctx_with(test_caller(), AndaBot::NAME, "", RequestMeta::default())
                .unwrap();
            ctx.base
                .set_state(memory_policy::MemoryPolicy::from_conversation(&conversation).unwrap());
            bot.build_system_instructions(
                &ctx,
                "fixture-home",
                "fixture-workspace",
                &[],
                unix_ms(),
            )
            .await
            .unwrap();
            assert_eq!(
                writes.load(Ordering::SeqCst),
                0,
                "restricted mode must never initialize or submit memory"
            );
            if mode == memory_policy::MemoryMode::Off {
                assert_eq!(reads.load(Ordering::SeqCst), 0)
            } else {
                assert!(reads.load(Ordering::SeqCst) >= 2)
            }
            let mut forged = AgentInput::new(AndaBot::NAME.into(), "hello".into());
            forged.meta = Some(RequestMeta {
                extra: serde_json::Map::from_iter([(
                    "memory_source_parents".into(),
                    json!(["forged"]),
                )]),
                ..Default::default()
            });
            assert!(engine.agent_run(test_caller(), forged).await.is_err());
        }
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
    async fn chatgpt_plan_rejects_external_channel_input_before_inference() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let model = anda_engine::model::openai::Client::new("unused-test-key", None)
            .completion_model_v2("chatgpt:test:model");
        bot.inner.models.set_model(Model::new(Arc::new(model)));
        let mut input = AgentInput::new(AndaBot::NAME.to_string(), "external message".into());
        let mut meta = RequestMeta::default();
        meta.extra
            .insert(keys::EXTERNAL_USER.to_string(), true.into());
        input.meta = Some(meta);
        let error = engine.agent_run(test_caller(), input).await.unwrap_err();
        assert!(error.to_string().contains("owner-only"));
        assert_eq!(bot.inner.conversations.conversations_len(), 0);
    }

    #[tokio::test]
    async fn one_shot_request_completes_after_model_becomes_idle() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let mut input = AgentInput::new(AndaBot::NAME.to_string(), "hello there".to_string());
        let mut meta = RequestMeta::default();
        meta.extra
            .insert(keys::FINISH_WHEN_IDLE.to_string(), true.into());
        input.meta = Some(meta);

        let output = engine.agent_run(test_caller(), input).await.unwrap();
        let conversation_id = output.conversation.expect("conversation id");
        assert_conversation_reaches_status(
            bot.as_ref(),
            conversation_id,
            ConversationStatus::Completed,
        )
        .await;
    }

    #[tokio::test]
    async fn one_shot_request_finishes_a_joined_idle_session() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let source = "cli:one-shot-join";
        let mut first = AgentInput::new(AndaBot::NAME.to_string(), "first turn".to_string());
        let mut first_meta = RequestMeta::default();
        first_meta
            .extra
            .insert(keys::SOURCE.to_string(), source.into());
        first.meta = Some(first_meta);

        let first_output = engine.agent_run(test_caller(), first).await.unwrap();
        let conversation_id = first_output.conversation.expect("conversation id");
        assert_conversation_reaches_status(bot.as_ref(), conversation_id, ConversationStatus::Idle)
            .await;

        let mut second = AgentInput::new(AndaBot::NAME.to_string(), "second turn".to_string());
        let mut second_meta = RequestMeta::default();
        second_meta
            .extra
            .insert(keys::SOURCE.to_string(), source.into());
        second_meta
            .extra
            .insert(keys::FINISH_WHEN_IDLE.to_string(), true.into());
        second.meta = Some(second_meta);

        let second_output = engine.agent_run(test_caller(), second).await.unwrap();
        assert_eq!(second_output.conversation, Some(conversation_id));
        assert_conversation_reaches_status(
            bot.as_ref(),
            conversation_id,
            ConversationStatus::Completed,
        )
        .await;
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
    async fn callers_sharing_a_source_keep_their_own_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let owner = test_caller();
        let other = Principal::from_slice(&[9, 8, 7, 6, 5, 4, 3, 2, 1]);

        let first = engine
            .agent_run(owner, input_for_source("hello", "cli:shared"))
            .await
            .unwrap();
        // The source now points at the owner's conversation; the other caller
        // starts its own instead of failing on a conversation it cannot read.
        let second = engine
            .agent_run(other, input_for_source("hello", "cli:shared"))
            .await
            .unwrap();
        assert_ne!(second.conversation, first.conversation);
        let conversation = bot
            .inner
            .conversations
            .conversations
            .get_conversation(second.conversation.unwrap())
            .await
            .unwrap();
        assert_eq!(conversation.user, other);

        // The owner goes on in its live session although the source moved on.
        let again = engine
            .agent_run(owner, input_for_source("again", "cli:shared"))
            .await
            .unwrap();
        assert_eq!(again.conversation, first.conversation);
        assert!(again.session.is_some());
    }

    #[tokio::test]
    async fn a_failed_conversation_continued_in_a_child_drops_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();
        let now_ms = unix_ms();
        let failed = Conversation {
            user: caller,
            status: ConversationStatus::Failed,
            failed_reason: Some("model error".to_string()),
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            ..Default::default()
        };
        let failed_id = bot
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&failed))
            .await
            .unwrap();

        let mut input = input_for_source("try again", "cli:failed");
        input
            .meta
            .as_mut()
            .unwrap()
            .extra
            .insert(keys::CONVERSATION.to_string(), json!(failed_id));
        let output = engine.agent_run(caller, input).await.unwrap();
        let child_id = output.conversation.unwrap();
        assert_ne!(child_id, failed_id);

        let conversations = &bot.inner.conversations.conversations;
        let parent = conversations.get_conversation(failed_id).await.unwrap();
        assert_eq!(parent.child, Some(child_id));
        assert_eq!(parent.status, ConversationStatus::Completed);
        assert_eq!(parent.failed_reason, None);
        // The child's saved request metadata does not name its parent.
        let child = conversations.get_conversation(child_id).await.unwrap();
        let extra = child.extra.unwrap();
        assert_eq!(extra.get("source"), Some(&json!("cli:failed")));
        assert!(extra.get(keys::CONVERSATION).is_none());
        assert_eq!(child.ancestors, Some(vec![failed_id]));
    }

    #[test]
    fn history_keeps_the_recent_text_of_a_past_conversation() {
        let text = |role: &str, text: String| {
            json!(Message {
                role: role.to_string(),
                content: vec![ContentPart::Text { text }],
                ..Default::default()
            })
        };
        let mut messages = (0..30)
            .map(|i| text("user", format!("question {i}")))
            .collect::<Vec<_>>();
        messages.push(json!(Message {
            role: "assistant".to_string(),
            content: vec![
                ContentPart::Reasoning {
                    text: "thinking".to_string()
                },
                ContentPart::Text {
                    text: "answer".to_string()
                },
                ContentPart::ToolCall {
                    name: "shell".to_string(),
                    args: json!({}),
                    call_id: Some("call-1".to_string()),
                },
            ],
            ..Default::default()
        }));
        messages.push(json!(Message {
            role: "tool".to_string(),
            content: vec![ContentPart::ToolOutput {
                name: "shell".to_string(),
                output: json!({"ok": true}),
                is_error: None,
                call_id: Some("call-1".to_string()),
                remote_id: None,
            }],
            ..Default::default()
        }));

        let conversation = recent_text_messages(Conversation {
            messages,
            ..Default::default()
        });

        let kept = conversation
            .messages
            .into_iter()
            .map(|value| serde_json::from_value::<Message>(value).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(kept.len(), HISTORY_TEXT_MESSAGES);
        assert_eq!(kept[0].text().as_deref(), Some("question 11"));
        let last = kept.last().unwrap();
        assert_eq!(last.role, "assistant");
        assert_eq!(
            last.content,
            vec![ContentPart::Text {
                text: "answer".to_string()
            }]
        );
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
        let conversations_tool = Arc::new(
            ConversationsTool::connect(
                db.clone(),
                "bot".to_string(),
                home.to_string_lossy().to_string(),
            )
            .await
            .unwrap(),
        );
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let bridge = Arc::new(BrowserBridge::new());
        let skills = SkillLibrary::for_test(home.clone());
        Arc::new(AndaBot::new(
            brain_client,
            Arc::new(Models::default()),
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
                    user: None,
                },
            )
            .await
            .unwrap();

        bot.startup_self_check(mock_agent_ctx()).await;
        // Let the spawned session runner settle.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn startup_self_check_is_noop_with_empty_db() {
        let dir = tempfile::tempdir().unwrap();
        let (_engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        // No source-bound conversations to resume: returns Ok after scanning.
        bot.startup_self_check(mock_agent_ctx()).await;
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
                    user: None,
                },
            )
            .await
            .unwrap();

        bot.startup_self_check(mock_agent_ctx()).await;
    }

    #[tokio::test]
    async fn startup_self_check_repairs_stale_source_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let (_engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let conv = Conversation {
            user: test_caller(),
            status: ConversationStatus::Idle,
            ..Default::default()
        };
        let conv_id = bot
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conv))
            .await
            .unwrap();
        // Earlier releases recorded a source's status only when binding it.
        let source = "cli:/tmp/finished".to_string();
        bot.inner
            .conversations
            .update_source_state(
                source.clone(),
                SourceState {
                    conv_id,
                    status: ConversationStatus::Submitted,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        bot.startup_self_check(mock_agent_ctx()).await;
        let state = bot.inner.conversations.get_source_state(&source).unwrap();
        assert_eq!(state.status, ConversationStatus::Idle);

        let mut conv = bot
            .inner
            .conversations
            .conversations
            .get_conversation(conv_id)
            .await
            .unwrap();
        conv.status = ConversationStatus::Working;
        bot.persist_conversation_state(&conv).await.unwrap();
        let state = bot.inner.conversations.get_source_state(&source).unwrap();
        assert_eq!(state.status, ConversationStatus::Working);
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

    #[tokio::test]
    async fn desktop_maintenance_rejects_new_work_but_allows_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine(dir.path().to_path_buf()).await;
        let caller = test_caller();
        engine
            .agent_run(caller, input_for_source("start", "cli:maintenance-control"))
            .await
            .unwrap();
        let _lease = bot.admission().begin().unwrap();
        let rejected = engine
            .agent_run(
                caller,
                input_for_source("new work", "cli:maintenance-control"),
            )
            .await
            .unwrap_err();
        assert!(rejected.to_string().contains("preparing an update"));
        engine
            .agent_run(
                caller,
                input_for_source("/cancel", "cli:maintenance-control"),
            )
            .await
            .unwrap();
    }

    struct CronTestCompleter;
    impl anda_engine::model::CompletionFeaturesDyn for CronTestCompleter {
        fn model_name(&self) -> String {
            "cron-output".into()
        }
        fn completion(
            &self,
            _: CompletionRequest,
        ) -> anda_core::BoxPinFut<Result<AgentOutput, BoxError>> {
            Box::pin(async {
                Ok(AgentOutput {
                    content: "scheduled work finished".into(),
                    ..Default::default()
                })
            })
        }
    }

    #[tokio::test]
    async fn cron_receipt_reports_actual_output_for_new_and_joined_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, bot) = build_bot_engine_with_model(
            dir.path().into(),
            spawn_brain_mock().await,
            Model::new(Arc::new(CronTestCompleter)),
        )
        .await;
        let mut conversation = None;
        for prompt in ["scheduled first turn", "scheduled second turn"] {
            let mut meta = RequestMeta::default();
            meta.extra
                .insert(keys::SOURCE.into(), "cron-receipt-test".into());
            meta.extra.insert(keys::CRON_JOB_ID.into(), 1.into());
            if let Some(id) = conversation {
                meta.extra.insert(keys::CONVERSATION.into(), json!(id));
            }
            let ctx = engine
                .ctx_with(test_caller(), AndaBot::NAME, "", meta)
                .unwrap();
            let (submission, receiver) = crate::cron::AgentSubmission::new();
            ctx.base.set_state(submission.clone());
            let (ack, _) = ctx
                .agent_run(AgentInput::new(AndaBot::NAME.into(), prompt.into()))
                .await
                .unwrap();
            assert!(submission.was_claimed());
            assert!(ack.content.is_empty());
            let result = tokio::time::timeout(std::time::Duration::from_secs(3), receiver)
                .await
                .unwrap()
                .unwrap();
            assert!(result.error.is_none(), "{:?}", result.error);
            assert!(
                result
                    .result
                    .as_deref()
                    .is_some_and(|text| !text.is_empty())
            );
            assert_eq!(result.conversation_id, ack.conversation);
            conversation = result.conversation_id;
            assert_conversation_reaches_status(
                &bot,
                conversation.unwrap(),
                ConversationStatus::Idle,
            )
            .await;
        }
    }

    #[derive(Clone)]
    struct RecordingCompleter(Arc<parking_lot::Mutex<Vec<CompletionRequest>>>);

    impl anda_engine::model::CompletionFeaturesDyn for RecordingCompleter {
        fn model_name(&self) -> String {
            "recording".into()
        }
        fn completion(
            &self,
            req: CompletionRequest,
        ) -> anda_core::BoxPinFut<Result<AgentOutput, BoxError>> {
            self.0.lock().push(req.clone());
            // Echo the sent content into the history, as real adapters do.
            let mut content = req.content;
            if !req.prompt.is_empty() {
                content.insert(0, req.prompt.into());
            }
            let chat_history = vec![
                Message {
                    role: req.role.unwrap_or_else(|| "user".into()),
                    content,
                    ..Default::default()
                },
                Message {
                    role: "assistant".into(),
                    content: vec!["done".to_string().into()],
                    ..Default::default()
                },
            ];
            Box::pin(async {
                Ok(AgentOutput {
                    content: "done".into(),
                    chat_history,
                    ..Default::default()
                })
            })
        }
    }

    async fn first_request_after(
        requests: &parking_lot::Mutex<Vec<CompletionRequest>>,
        seen: usize,
    ) -> CompletionRequest {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some(req) = requests.lock().get(seen).cloned() {
                    break req;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the session never sent the expected request")
    }

    fn png_attachment(bytes: &[u8]) -> Resource {
        let mut blob = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
        blob.extend_from_slice(bytes);
        Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ic_auth_types::ByteBufB64(blob)),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn image_reaches_a_vision_model_inline_but_not_the_saved_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let model = Model::new(Arc::new(RecordingCompleter(requests.clone())))
            .with_labels(vec!["image".to_string()]);
        let (engine, bot) =
            build_bot_engine_with_model(dir.path().into(), spawn_brain_mock().await, model.clone())
                .await;
        // Production shares one registry between the engine and the bot.
        bot.inner.models.set_model(model);

        let attachment = png_attachment(b"vision pixels");
        let encoded = attachment.blob.as_ref().unwrap().to_base64();
        let mut input = input_for_source("what is in this picture?", "cli:vision");
        input.resources = vec![attachment];
        let ack = engine.agent_run(test_caller(), input).await.unwrap();

        let req = first_request_after(&requests, 0).await;
        let reference = req.content[0]
            .clone()
            .any_into::<Resource>("Resource")
            .expect("the reference comes first");
        assert_eq!(reference.name, "photo.png");
        assert!(reference.blob.is_none());
        assert!(matches!(
            &req.content[1],
            ContentPart::InlineData { mime_type, .. } if mime_type == "image/png"
        ));

        let conversation_id = ack.conversation.unwrap();
        assert_conversation_reaches_status(&bot, conversation_id, ConversationStatus::Idle).await;
        let conversation = bot
            .inner
            .conversations
            .conversations
            .get_conversation(conversation_id)
            .await
            .unwrap();
        let saved = serde_json::to_string(&conversation.messages).unwrap();
        assert!(saved.contains("photo.png"), "{saved}");
        assert!(!saved.contains(&encoded), "{saved}");
        assert!(!saved.contains("InlineData"), "{saved}");
    }

    #[tokio::test]
    async fn attachment_mid_session_loads_its_inspection_tool() {
        let dir = tempfile::tempdir().unwrap();
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (engine, bot) = build_bot_engine_with_model(
            dir.path().into(),
            spawn_brain_mock().await,
            Model::new(Arc::new(RecordingCompleter(requests.clone()))),
        )
        .await;

        let ack = engine
            .agent_run(test_caller(), input_for_source("hello", "cli:mid-session"))
            .await
            .unwrap();
        let first = first_request_after(&requests, 0).await;
        let offers_image_tool = |req: &CompletionRequest| {
            req.tools
                .iter()
                .any(|tool| tool.name == multimodal::IMAGE_UNDERSTANDING_AGENT_NAME)
        };
        assert!(!offers_image_tool(&first));
        assert_conversation_reaches_status(
            &bot,
            ack.conversation.unwrap(),
            ConversationStatus::Idle,
        )
        .await;

        let mut input = input_for_source("and this one?", "cli:mid-session");
        input.resources = vec![png_attachment(b"later pixels")];
        engine.agent_run(test_caller(), input).await.unwrap();

        let second = first_request_after(&requests, 1).await;
        assert!(offers_image_tool(&second));
        // A model without the `image` label gets the reference only.
        assert!(
            !second
                .content
                .iter()
                .any(|part| matches!(part, ContentPart::InlineData { .. }))
        );
    }

    #[tokio::test]
    async fn new_session_sends_attachment_references_and_loads_their_tools() {
        let dir = tempfile::tempdir().unwrap();
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (engine, _bot) = build_bot_engine_with_model(
            dir.path().into(),
            spawn_brain_mock().await,
            Model::new(Arc::new(RecordingCompleter(requests.clone()))),
        )
        .await;

        let png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
        let mut input = input_for_source("what is in this picture?", "cli:attachment");
        input.resources = vec![Resource {
            name: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            blob: Some(ic_auth_types::ByteBufB64(png)),
            ..Default::default()
        }];
        engine.agent_run(test_caller(), input).await.unwrap();

        let req = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some(req) = requests.lock().first().cloned() {
                    break req;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the session never sent its first request");

        assert!(
            req.tools
                .iter()
                .any(|tool| tool.name == multimodal::IMAGE_UNDERSTANDING_AGENT_NAME)
        );
        let references = req
            .content
            .iter()
            .cloned()
            .filter_map(|part| part.any_into::<Resource>("Resource").ok())
            .collect::<Vec<_>>();
        assert_eq!(references.len(), 1);
        assert!(references[0]._id > 0);
        assert_eq!(references[0].name, "photo.png");
        assert!(references[0].blob.is_none());
        assert!(
            !req.content
                .iter()
                .any(|part| matches!(part, ContentPart::InlineData { .. }))
        );
    }
}
