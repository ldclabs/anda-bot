use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, Document, Documents, Message,
    Resource, StateFeatures, ToolOutput, Usage, select_resources,
};
use anda_db::schema::Fv;
use anda_engine::{
    ANONYMOUS,
    context::{
        AgentCtx, BaseCtx, CompletionRunner, SubAgentManager, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME,
    },
    extension::{
        fs::{EditFileTool, ReadFileTool, SearchFileTool, WriteFileTool},
        note::{NoteTool, load_notes},
        shell::{ExecArgs, ExecOutput, ShellTool, ShellToolHook},
        skill::SkillManager,
        todo::TodoTool,
    },
    hook::{AgentHook, DynAgentHook, ToolHook},
    memory::{Conversation, ConversationRef, ConversationStatus, Conversations},
    rfc3339_datetime, unix_ms,
};
use anda_hippocampus::types::{FormationInputRef, InputContext};
use async_trait::async_trait;
use futures::future::join_all;
use ic_auth_types::Xid;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    brain, cron,
    transcription::{TranscriptionManager, audio_resource_file_name, is_audio_resource},
    tts::TtsManager,
};

use super::{
    CompletionHook, goal,
    prompt::{PromptCommand, prompt_with_resources, skill_subagent, text_resource_documents},
    side,
};

const MAX_TURNS_TO_COMPACT: usize = 81; // The number of turns after which the conversation history will be compacted. This is to prevent the conversation history from growing indefinitely and causing performance issues. The optimal value may depend on the typical length of conversations and the token limits of the language model.
const CONVERSATION_IDLE_MS: u64 = 10 * 60 * 1000; // 10 minutes
const CONVERSATION_WAIT_BACKGROUND_TASK_MS: u64 = 12 * 60 * 60 * 1000; // 12 hours
static SELF_INSTRUCTIONS: &str = include_str!("../../assets/SelfInstructions.md");
static COMPACTION_PROMPT: &str = include_str!("../../assets/CompactionPrompt.md");

#[derive(Clone)]
pub struct AndaBot {
    inner: Arc<AndaBotInner>,
}

struct AndaBotInner {
    brain: brain::Client,
    conversations: Conversations,
    tool_dependencies: Vec<String>,
    tools: Vec<String>,
    sessions: ActiveSessions,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    source_conversation: RwLock<HashMap<String, SourceState>>,
    tools_usage: RwLock<HashMap<String, Usage>>,
    home_dir: PathBuf,
    skills_manager: Arc<SkillManager>,
    transcription_manager: Option<Arc<TranscriptionManager>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SourceState {
    #[serde(rename = "c")]
    pub conv_id: u64,
}

type ActiveSessions = RwLock<HashMap<Xid, Arc<Session>>>;

fn base_tool_dependencies() -> Vec<String> {
    vec![
        brain::Client::NAME.to_string(),
        NoteTool::NAME.to_string(),
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
    ]
}

fn base_tools() -> Vec<String> {
    vec![
        brain::Client::NAME.to_string(),
        NoteTool::NAME.to_string(),
        TOOLS_SELECT_NAME.to_string(),
        ShellTool::NAME.to_string(),
        ReadFileTool::NAME.to_string(),
        SearchFileTool::NAME.to_string(),
        TodoTool::NAME.to_string(),
    ]
}

fn source_conversation_key(
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

impl AndaBot {
    pub const NAME: &'static str = "anda_bot";

    pub fn new(
        brain: brain::Client,
        conversations: Conversations,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
        home_dir: PathBuf,
        skills_manager: Arc<SkillManager>,
        tts_manager: Option<Arc<TtsManager>>,
        transcription_manager: Option<Arc<TranscriptionManager>>,
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
                conversations,
                tool_dependencies,
                tools,
                sessions: RwLock::new(HashMap::new()),
                completion_hooks: Arc::new(completion_hooks),
                source_conversation: RwLock::new(HashMap::new()),
                tools_usage: RwLock::new(HashMap::new()),
                home_dir,
                skills_manager,
                transcription_manager,
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

    fn remove_session(&self, key: &Xid) {
        self.inner.sessions.write().remove(key);
    }

    async fn build_system_instructions(
        &self,
        ctx: &AgentCtx,
        home_dir: &str,
        workspace: &str,
        available_tools: &[String],
        now_ms: u64,
    ) -> Result<String, BoxError> {
        let primer = self.inner.brain.describe_primer().await?;
        let user_profile = self.inner.brain.user_info(*ctx.caller(), None).await?;
        let notes = load_notes(ctx).await.unwrap_or_default();

        Ok(format!(
            "{}\n\n---\n\n# Your Context\n\n## Identity & Knowledge Domains:\n\n{}\n\n## Notes:\n\n{}\n\n## Tools:\n\n{}\n\n## Home:\n\n{home_dir}\n\n---\n\n# User Context\n\n## User Profile:\n\n{}\n\n## Workspace:\n{workspace}\n\n---\n\n# Current Datetime: {}",
            SELF_INSTRUCTIONS,
            primer,
            serde_json::to_string(&notes.notes)?,
            serde_json::to_string(&available_tools)?,
            serde_json::to_string(&user_profile)?,
            rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
        ))
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

    async fn prompt_with_audio_resources(
        &self,
        prompt: String,
        resources: Vec<Resource>,
    ) -> Result<String, BoxError> {
        if resources.is_empty() {
            return Ok(prompt);
        }

        let Some(manager) = &self.inner.transcription_manager else {
            if prompt.trim().is_empty() {
                return Err("voice transcription is not enabled".into());
            }
            return Ok(prompt);
        };

        let mut transcripts = Vec::new();
        for (index, resource) in resources.into_iter().filter(is_audio_resource).enumerate() {
            let file_name = audio_resource_file_name(&resource, &format!("voice_{}", index + 1));
            let audio = resource
                .blob
                .as_ref()
                .ok_or("audio resource missing inline blob data")?;

            let text = manager.transcribe(audio, &file_name).await?;
            if !text.trim().is_empty() {
                transcripts.push((file_name, text.trim().to_string()));
            }
        }

        if transcripts.is_empty() {
            return Ok(prompt);
        }

        let transcript = if transcripts.len() == 1 {
            transcripts.remove(0).1
        } else {
            transcripts
                .into_iter()
                .map(|(file_name, text)| format!("{file_name}: {text}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if prompt.trim().is_empty() {
            Ok(transcript)
        } else {
            Ok(format!(
                "{}\n\nTranscribed voice input:\n{}",
                prompt.trim(),
                transcript
            ))
        }
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

    async fn init(&self, _ctx: AgentCtx) -> Result<(), BoxError> {
        {
            let source_conversation: HashMap<String, SourceState> = self
                .inner
                .conversations
                .conversations
                .get_extension_as("source_conversation")
                .unwrap_or_default();

            *self.inner.source_conversation.write() = source_conversation;
        }
        {
            let tools_usage: HashMap<String, Usage> = self
                .inner
                .conversations
                .conversations
                .get_extension_as("tools_usage")
                .unwrap_or_default();

            *self.inner.tools_usage.write() = tools_usage;
        }

        Ok(())
    }

    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String,
        mut resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let caller = ctx.caller();
        if caller == &ANONYMOUS {
            return Err("anonymous caller not allowed".into());
        }

        let now_ms = unix_ms();
        let home_dir = self.inner.home_dir.to_string_lossy().to_string();
        let workspace = ctx
            .meta()
            .get_extra_as::<String>("workspace")
            .unwrap_or_else(|| {
                self.inner
                    .home_dir
                    .join("workspace")
                    .to_string_lossy()
                    .to_string()
            });
        let available_tools: Vec<String> = ctx
            .definitions(None)
            .await
            .into_iter()
            .filter_map(|def| {
                if def.name == Self::NAME {
                    None
                } else {
                    Some(def.name)
                }
            })
            .collect();

        let mut instructions = self
            .build_system_instructions(&ctx, &home_dir, &workspace, &available_tools, now_ms)
            .await?;

        let source = ctx
            .meta()
            .get_extra_as::<String>("source")
            .unwrap_or_else(|| format!("cli:{workspace}"));
        let reply_target = ctx.meta().get_extra_as::<String>("reply_target");
        let thread = ctx.meta().get_extra_as::<String>("thread");
        let source_key =
            source_conversation_key(&source, reply_target.as_deref(), thread.as_deref());
        let source_state = {
            self.inner
                .source_conversation
                .read()
                .get(&source_key)
                .cloned()
                .unwrap_or_default()
        };
        let current_conv_id = ctx
            .meta()
            .get_extra_as::<u64>("conversation")
            .filter(|conv_id| *conv_id > 0)
            .unwrap_or(source_state.conv_id);

        let (history_conversations, _) = self
            .inner
            .conversations
            .list_conversations_by_user(caller, None, Some(2))
            .await?;

        let current_conversation = if current_conv_id > 0 {
            if let Some(pos) = history_conversations
                .iter()
                .position(|conv| conv._id == current_conv_id)
            {
                history_conversations.get(pos).cloned()
            } else {
                self.inner
                    .conversations
                    .get_conversation(current_conv_id)
                    .await
                    .ok()
                    .filter(|conversation| &conversation.user == caller)
            }
        } else {
            None
        };
        let current_conversation_id = current_conversation.as_ref().map(|conv| conv._id);

        let ancestors = match &current_conversation {
            Some(conv) => {
                let mut ids = conv.ancestors.clone().unwrap_or_default();
                ids.push(conv._id);
                if ids.len() > 10 {
                    ids.drain(0..ids.len() - 10);
                }
                Some(ids)
            }
            None => None,
        };

        let audio_resources: Vec<Resource> =
            select_resources(&mut resources, &["audio".to_string()]);
        let prompt = self
            .prompt_with_audio_resources(prompt, audio_resources)
            .await?;
        let command = PromptCommand::from(prompt);
        if let PromptCommand::Invalid { reason } = &command {
            return Err(reason.clone().into());
        }

        if let PromptCommand::Side { prompt } = &command {
            return self
                .run_side_command(
                    &ctx,
                    instructions,
                    prompt.clone(),
                    resources,
                    current_conversation.as_ref().map(|conv| conv._id),
                )
                .await;
        }

        let mut input = ConversationInput {
            command,
            resources,
            usage: Usage::default(),
        };

        let sess_id = current_conversation
            .as_ref()
            .and_then(|conv| conv.thread.clone())
            .unwrap_or_else(Xid::new);
        if let Some(session) = self.get_session(&sess_id) {
            // Join existing conversation session if it's active
            match session.sender.send(input).await {
                Ok(_) => {
                    return Ok(AgentOutput {
                        conversation: Some(current_conversation_id.unwrap_or(current_conv_id)),
                        ..Default::default()
                    });
                }
                Err(err) => {
                    log::warn!(
                        "Failed to enqueue prompt for processing conversation {}",
                        current_conv_id,
                    );
                    self.remove_session(&sess_id);
                    input = err.0;
                }
            }
        }

        // If the conversation session is not active, start a new session and process the prompt
        let ConversationInput {
            command,
            mut resources,
            ..
        } = input;

        let mut initial_goal = None;
        let mut additional_tools: Vec<String> = Vec::new();
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
        };

        if prompt.trim().is_empty() {
            return Err("prompt cannot be empty".into());
        }

        let mut msg = Message {
            role: "user".into(),
            content: vec![],
            name: Some("$system".into()),
            timestamp: Some(now_ms),
            ..Default::default()
        };

        if !history_conversations.is_empty() {
            msg.content.push(
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

        let user_resources = text_resource_documents(&mut resources);
        if !user_resources.is_empty() {
            msg.content.push(
                Documents::new("user_attachments".to_string(), user_resources)
                    .to_string()
                    .into(),
            );
        }

        let mut conversation = Conversation {
            user: *caller,
            thread: Some(sess_id.clone()),
            messages: vec![serde_json::json!(Message {
                role: "user".into(),
                content: vec![prompt.clone().into()],
                timestamp: Some(now_ms),
                ..Default::default()
            })],
            ancestors,
            resources: vec![], // Don't save the resources in the conversation, as they are already included in the message content as documents
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some(json!(ctx.meta().extra)),
            ..Default::default()
        };

        let conv_id = self
            .inner
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = conv_id;

        if source_state.conv_id != conv_id {
            // Update the mapping of source to conv_id if it's different from the current one.
            let fv = {
                let mut map = self.inner.source_conversation.write();
                map.insert(source_key, SourceState { conv_id });
                Fv::serialized(&*map, None)
            };

            match fv {
                Ok(v) => {
                    if let Err(err) = self
                        .inner
                        .conversations
                        .conversations
                        .save_extension("source_conversation".to_string(), v)
                        .await
                    {
                        log::error!("Failed to save source_conversation extension: {:?}", err);
                    }
                }
                Err(err) => {
                    log::error!(
                        "Failed to serialize source_conversation extension: {:?}",
                        err
                    );
                }
            }
        }

        let res = AgentOutput {
            conversation: Some(conv_id),
            ..Default::default()
        };

        let (sender, mut rx) = tokio::sync::mpsc::channel::<ConversationInput>(42);
        let session = Arc::new(Session {
            id: sess_id,
            sender,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            goal: RwLock::new(initial_goal.map(goal::GoalState::new)),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            active_at: AtomicU64::new(unix_ms()),
            formation_context: Some(InputContext {
                counterparty: Some(caller.to_string()),
                agent: Some(AndaBot::NAME.to_string()),
                source: Some(source),
                topic: None,
            }),
        });

        let agent_hook = DynAgentHook::new(session.clone());
        ctx.base.set_state(agent_hook);

        let shell_hook = ShellToolHook::new(session.clone());
        ctx.base.set_state(shell_hook);

        self.insert_session(session.clone());

        let assistant = self.clone();
        let chat_history = if msg.content.is_empty() {
            vec![]
        } else {
            vec![msg]
        };

        let mut tools = assistant.inner.tools.clone();
        tools.extend(additional_tools);
        let additional_tools = {
            let tools_usage = assistant.inner.tools_usage.read();
            select_most_used_tools(&available_tools, &tools, &tools_usage, 5)
        };
        tools.extend(additional_tools);
        let req = CompletionRequest {
            instructions,
            tools: ctx.definitions(Some(&tools)).await,
            tool_choice_required: false,
            max_output_tokens: Some(ctx.model.max_output.max(32000)),
            ..Default::default()
        };

        let runner = ctx
            .clone()
            .completion_iter(
                CompletionRequest {
                    prompt,
                    chat_history,
                    ..req.clone()
                },
                resources.clone(),
            )
            .unbound();

        tokio::spawn(async move {
            let mut tools_usage_snapshot: HashMap<String, Usage> = HashMap::new();
            let mut job = SessionJob {
                ctx,
                req,
                assistant: assistant.clone(),
                session: session.clone(),
                conversation,
                runner,
                first_round: true,
            };

            loop {
                let mut inputs = Vec::new();

                while let Ok(input) = rx.try_recv() {
                    inputs.push(input);
                }

                match job.run(inputs, &mut tools_usage_snapshot).await {
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

            assistant.remove_session(&session.id);
        });
        Ok(res)
    }
}

struct SessionJob {
    ctx: AgentCtx,
    req: CompletionRequest,
    assistant: AndaBot,
    session: Arc<Session>,
    conversation: Conversation,
    runner: CompletionRunner,
    first_round: bool,
}

impl SessionJob {
    async fn persist_conversation_state(&self) {
        match self.conversation.to_changes() {
            Ok(changes) => {
                let _ = self
                    .assistant
                    .inner
                    .conversations
                    .update_conversation(self.conversation._id, changes)
                    .await;
            }
            Err(err) => {
                log::error!(
                    "Failed to serialize conversation {} changes: {:?}",
                    self.conversation._id,
                    err
                );
            }
        }
    }

    async fn persist_tools_usage_snapshot(
        &self,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) {
        let current_tools_usage = self.runner.tools_usage().clone();
        let tools_usage_delta =
            compute_tools_usage_delta(&current_tools_usage, tools_usage_snapshot);
        *tools_usage_snapshot = current_tools_usage;
        if tools_usage_delta.is_empty() {
            return;
        }

        let tools_usage = {
            let mut tools_usage = self.assistant.inner.tools_usage.write();
            for (tool, usage) in tools_usage_delta.into_iter() {
                let entry = tools_usage.entry(tool.clone()).or_default();
                entry.accumulate(&usage);
            }
            Fv::serialized(&*tools_usage, None)
        };

        match tools_usage {
            Ok(v) => {
                if let Err(err) = self
                    .assistant
                    .inner
                    .conversations
                    .conversations
                    .save_extension("tools_usage".to_string(), v)
                    .await
                {
                    log::error!("Failed to save tools_usage extension: {:?}", err);
                }
            }
            Err(err) => {
                log::error!("Failed to serialize tools_usage extension: {:?}", err);
            }
        }
    }

    async fn submit_pending_formation(&self, chat_history: &[Message], now_ms: u64) {
        let messages = chat_history
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

        for mut input in inputs {
            // 累计来自于后台任务的工具使用情况
            self.runner.accumulate(&input.usage);

            match input.command {
                PromptCommand::Ping | PromptCommand::Invalid { .. } => {
                    // PING from the user to keep the conversation alive.
                    continue;
                }
                PromptCommand::Plain { prompt } => {
                    self.runner
                        .follow_up(prompt_with_resources(prompt, &mut input.resources));
                }
                PromptCommand::Goal { prompt } => {
                    let prompt = prompt_with_resources(prompt, &mut input.resources);
                    self.runner.follow_up(prompt.clone());

                    let mut next_goal = self.session.goal.write();
                    if let Some(existing_goal) = next_goal.as_mut() {
                        existing_goal.update_objective(prompt);
                    } else {
                        *next_goal = Some(goal::GoalState::new(prompt));
                    };
                }
                PromptCommand::Side { prompt } => {
                    self.runner
                        .follow_up(prompt_with_resources(prompt, &mut input.resources));
                }
                PromptCommand::Steer { prompt } => {
                    self.runner
                        .steer(prompt_with_resources(prompt, &mut input.resources));
                }
                PromptCommand::Skill { mut skill, prompt } => {
                    if let Some(subagent) =
                        skill_subagent(&self.assistant.inner.skills_manager, &skill)
                    {
                        skill = subagent.name;
                    }
                    self.runner.follow_up(prompt_with_resources(
                        format!("Use the {skill} skill to handle this request:\n\n{prompt}"),
                        &mut input.resources,
                    ));
                }
                PromptCommand::Stop { prompt } => {
                    cancellation_requested =
                        Some(prompt.unwrap_or_else(|| "Cancelled by user".to_string()));
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
                                goal::GoalAction::Complete => {
                                    // 目标已经完成，但继续保持对话活跃，等待用户的下一步指令
                                    active = true;
                                    self.session.active_at.store(now_ms, Ordering::SeqCst);
                                }
                                goal::GoalAction::Continue(prompt) => {
                                    let now_ms = unix_ms();
                                    goal_continue_prompt = Some(prompt);
                                    // runner.follow_up(prompt);
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
                    let output = self
                        .runner
                        .finalize(Some(COMPACTION_PROMPT.to_string()))
                        .await?;

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
                                serde_json::json!(Message {
                                    role: "user".into(),
                                    content: vec![prompt.clone().into()],
                                    timestamp: Some(now_ms),
                                    ..Default::default()
                                }),
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
                    || is_done
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
    sender: tokio::sync::mpsc::Sender<ConversationInput>,
    // task_id -> BackgroundTaskInfo
    background_tasks: Arc<RwLock<HashMap<String, BackgroundTaskInfo>>>,
    goal: RwLock<Option<goal::GoalState>>,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    submit_formation_at: AtomicU64,
    active_at: AtomicU64,
    formation_context: Option<InputContext>,
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
    async fn on_background_start(&self, ctx: &AgentCtx, task_id: &str, _req: &CompletionRequest) {
        self.background_tasks.write().insert(
            task_id.to_string(),
            BackgroundTaskInfo {
                agent_name: ctx.base.agent.clone(),
                tool_name: None,
                progress_message: None,
            },
        );
    }

    async fn on_background_end(&self, _ctx: AgentCtx, task_id: String, output: AgentOutput) {
        {
            self.background_tasks.write().remove(&task_id);
        }

        let prompt = if !output.content.is_empty() {
            format!("Background task {task_id} completed:\n\n{}", output.content)
        } else if let Some(failed_reason) = output.failed_reason {
            format!(
                "Background task {task_id} failed with reason: {:?}",
                failed_reason
            )
        } else {
            format!("Background task {task_id} completed")
        };
        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain { prompt },
                resources: vec![],
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

    async fn on_background_end(
        &self,
        _ctx: BaseCtx,
        task_id: String,
        output: ToolOutput<ExecOutput>,
    ) {
        {
            self.background_tasks.write().remove(&task_id);
        }

        self.sender
            .send(ConversationInput {
                command: PromptCommand::Plain {
                    prompt: format!(
                        "Background task {task_id} completed:\n\n{}",
                        serde_json::to_string(&output.output).unwrap_or_default()
                    ),
                },
                usage: output.usage,
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
    usage: Usage,
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

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::Usage;
    use std::collections::HashMap;

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

    #[test]
    fn compaction_token_threshold_uses_half_window_with_cap() {
        assert_eq!(compaction_token_threshold(0), 100_000);
        assert_eq!(compaction_token_threshold(140_000), 70_000);
        assert_eq!(compaction_token_threshold(3_000_000), 500_000);
    }

    #[test]
    fn compaction_prompt_preserves_goal_continuation_evidence() {
        assert!(COMPACTION_PROMPT.contains("not a final answer"));
        assert!(COMPACTION_PROMPT.contains("user-provided task data"));
        assert!(COMPACTION_PROMPT.contains("prompt-to-artifact checklist"));
        assert!(COMPACTION_PROMPT.contains("next concrete action"));
        assert!(COMPACTION_PROMPT.contains("Do not invent progress"));
    }
}
