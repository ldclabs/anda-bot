use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, Document, Documents, Message,
    Principal, Resource, StateFeatures, ToolOutput, Usage,
};
use anda_engine::{
    ANONYMOUS,
    context::{AgentCtx, BaseCtx, SubAgentManager, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
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
use anda_hippocampus::{
    agents::SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
    types::{FormationInputRef, InputContext},
};
use async_trait::async_trait;
use futures::future::join_all;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    brain, cron,
    engine::CompletionHook,
    transcription::{TranscriptionManager, audio_resource_file_name, is_audio_resource},
    tts::TtsManager,
};

const CONVERSATION_IDLE_MS: u64 = 30 * 60 * 1000; // 30 minutes
const CONVERSATION_WAIT_BACKGROUND_TASK_MS: u64 = 60 * 60 * 1000; // 1 hour
static SELF_INSTRUCTIONS: &str = include_str!("../../assets/SelfInstructions.md");

#[derive(Clone)]
pub struct AndaBot {
    inner: Arc<AndaBotInner>,
}

struct AndaBotInner {
    brain: brain::Client,
    conversations: Conversations,
    tool_dependencies: Vec<String>,
    tools: Vec<String>,
    processing_conversations: ProcessingConversations,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    workspace_conversation: RwLock<HashMap<String, WorkDirState>>,
    home_dir: PathBuf,
    tts_manager: Option<Arc<TtsManager>>,
    transcription_manager: Option<Arc<TranscriptionManager>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct WorkDirState {
    #[serde(rename = "c")]
    pub conversation_id: u64,
}

type ProcessingConversations = RwLock<HashMap<(Principal, u64), Arc<ConversationTask>>>;

impl AndaBot {
    pub const NAME: &'static str = "anda_bot";

    pub fn new(
        brain: brain::Client,
        conversations: Conversations,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
        home_dir: PathBuf,
        tts_manager: Option<Arc<TtsManager>>,
        transcription_manager: Option<Arc<TranscriptionManager>>,
    ) -> Self {
        let mut tool_dependencies = vec![
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
        ];
        let mut tools = vec![
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
        ];

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
                processing_conversations: RwLock::new(HashMap::new()),
                completion_hooks: Arc::new(completion_hooks),
                workspace_conversation: RwLock::new(HashMap::new()),
                home_dir,
                tts_manager,
                transcription_manager,
            }),
        }
    }

    fn get_processing_task(&self, key: &(Principal, u64)) -> Option<Arc<ConversationTask>> {
        let mut processing = self.inner.processing_conversations.write();
        processing.retain(|_, task| !task.sender.is_closed());
        processing.get(key).cloned()
    }

    fn insert_processing_task(&self, key: (Principal, u64), task: Arc<ConversationTask>) {
        self.inner
            .processing_conversations
            .write()
            .insert(key, task);
    }

    fn remove_processing_task(&self, key: &(Principal, u64)) {
        self.inner.processing_conversations.write().remove(key);
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
        resources: &[Resource],
    ) -> Result<String, BoxError> {
        if !resources.iter().any(is_audio_resource) {
            return Ok(prompt);
        }

        let Some(manager) = &self.inner.transcription_manager else {
            if prompt.trim().is_empty() {
                return Err("voice transcription is not enabled".into());
            }
            return Ok(prompt);
        };

        let mut transcripts = Vec::new();
        for (index, resource) in resources
            .iter()
            .filter(|res| is_audio_resource(res))
            .enumerate()
        {
            let audio = resource
                .blob
                .as_ref()
                .map(|blob| blob.0.clone())
                .ok_or("audio resource missing inline blob data")?;
            let file_name = audio_resource_file_name(resource, &format!("voice_{}", index + 1));
            let text = manager.transcribe(&audio, &file_name).await?;
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

    async fn attach_tts_artifact(&self, output: &mut AgentOutput) -> Result<(), BoxError> {
        let Some(manager) = &self.inner.tts_manager else {
            return Ok(());
        };
        let text = agent_output_text_for_tts(output);
        if text.trim().is_empty() {
            return Ok(());
        }

        let bytes = manager.synthesize(text.trim()).await?;
        output
            .artifacts
            .push(manager.audio_artifact(bytes, Some(format!("anda_bot_response_{}", unix_ms()))));
        Ok(())
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
        let workspace_conversation: HashMap<String, WorkDirState> = self
            .inner
            .conversations
            .conversations
            .get_extension_as("workspace_conversation")
            .unwrap_or_default();

        let mut map = self.inner.workspace_conversation.write();
        *map = workspace_conversation;
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

        let now_ms = unix_ms();
        let workspace = ctx
            .meta()
            .get_extra_as::<String>("workspace")
            .unwrap_or_else(|| self.inner.home_dir.to_string_lossy().to_string());
        let workspace_state = {
            self.inner
                .workspace_conversation
                .read()
                .get(&workspace)
                .cloned()
                .unwrap_or_default()
        };
        let current_conversation_id = ctx
            .meta()
            .get_extra_as::<u64>("conversation")
            .unwrap_or(workspace_state.conversation_id);
        let voice_response = ctx
            .meta()
            .get_extra_as::<bool>("voice_response")
            .unwrap_or(false);
        let wait_for_completion = voice_response
            || ctx
                .meta()
                .get_extra_as::<bool>("wait_completion")
                .unwrap_or(false);
        let prompt = self.prompt_with_audio_resources(prompt, &resources).await?;

        let mut input = ConversationInput {
            prompt,
            resources,
            usage: Usage::default(),
        };
        if current_conversation_id > 0
            && let Some(task) = self.get_processing_task(&(*caller, current_conversation_id))
        {
            match task.sender.send(input).await {
                Ok(_) => {
                    return Ok(AgentOutput {
                        conversation: Some(current_conversation_id),
                        ..Default::default()
                    });
                }
                Err(err) => {
                    log::warn!(
                        "Failed to enqueue prompt for processing conversation {}",
                        current_conversation_id,
                    );
                    self.remove_processing_task(&(*caller, current_conversation_id));
                    input = err.0;
                }
            }
        }

        let ConversationInput {
            prompt, resources, ..
        } = input;

        if prompt.trim().is_empty() {
            return Err("prompt cannot be empty".into());
        }

        let (mut prev_conversations, _) = self
            .inner
            .conversations
            .list_conversations_by_user(caller, None, Some(2))
            .await?;

        let mut ancestors = if current_conversation_id > 0 {
            if let Some(pos) = prev_conversations
                .iter()
                .position(|conv| conv._id == current_conversation_id)
            {
                prev_conversations.get(pos).map(|conv| {
                    let mut ancestors = conv.ancestors.clone().unwrap_or_default();
                    ancestors.push(conv._id);
                    ancestors
                })
            } else if let Some(conv) = self
                .inner
                .conversations
                .get_conversation(current_conversation_id)
                .await
                .ok()
                .filter(|conversation| &conversation.user == caller)
            {
                let mut ancestors = conv.ancestors.clone().unwrap_or_default();
                ancestors.push(conv._id);
                prev_conversations.insert(0, conv);
                Some(ancestors)
            } else {
                None
            }
        } else {
            prev_conversations.last().map(|conv| {
                let mut ancestors = conv.ancestors.clone().unwrap_or_default();
                ancestors.push(conv._id);
                ancestors
            })
        };

        if let Some(ancestors) = ancestors.as_mut()
            && ancestors.len() > 10
        {
            ancestors.drain(0..ancestors.len() - 10);
        }

        let primer = self.inner.brain.describe_primer().await?;
        let user_info = self.inner.brain.user_info(*caller, None).await?;
        let notes = load_notes(&ctx).await.unwrap_or_default();
        let tools: Vec<String> = ctx
            .definitions(None)
            .await
            .into_iter()
            .map(|def| def.name)
            .collect();
        let instructions = format!(
            "{}\n\n{}\n\n---\n\n# Your identity & knowledge domains:\n{}\n\n---\n\n# Your notes:\n{}\n\n# User profile:\n{}\n\n# Available tools:\n{}\n\n# Current datetime:\n{}\n\n# Current workspace:\n{workspace}",
            SELF_INSTRUCTIONS,
            SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
            primer,
            serde_json::to_string(&notes.notes).unwrap_or_default(),
            serde_json::to_string(&user_info).unwrap_or_default(),
            serde_json::to_string(&tools).unwrap_or_default(),
            rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
        );

        let mut msg = Message {
            role: "user".into(),
            content: vec![],
            name: Some("$system".into()),
            timestamp: Some(now_ms),
            ..Default::default()
        };

        if !prev_conversations.is_empty() {
            msg.content.push(
                Documents::new(
                    "user_history_conversations".to_string(),
                    prev_conversations.into_iter().map(Document::from).collect(),
                )
                .to_string()
                .into(),
            );
        }

        if !resources.is_empty() {
            let user_resources = text_resource_documents(&resources);
            if !user_resources.is_empty() {
                msg.content.push(
                    Documents::new("user_resources".to_string(), user_resources)
                        .to_string()
                        .into(),
                );
            }
        }

        let mut conversation = Conversation {
            user: *caller,
            messages: vec![serde_json::json!(Message {
                role: "user".into(),
                content: vec![prompt.clone().into()],
                timestamp: Some(now_ms),
                ..Default::default()
            })],
            ancestors,
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some(json!(ctx.meta().extra)),
            ..Default::default()
        };

        let id = self
            .inner
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = id;

        if workspace_state.conversation_id != id {
            // Update the mapping of workspace to conversation_id if it's different from the current one
            let map = {
                let mut map = self.inner.workspace_conversation.write();
                map.insert(
                    workspace.clone(),
                    WorkDirState {
                        conversation_id: id,
                    },
                );
                map.clone()
            };

            let _ = self
                .inner
                .conversations
                .conversations
                .save_extension_from("workspace_conversation".to_string(), &map)
                .await;
        }

        let res = AgentOutput {
            conversation: Some(id),
            ..Default::default()
        };

        let task_key = (*caller, id);
        let (sender, mut rx) = tokio::sync::mpsc::channel::<ConversationInput>(7);
        let conversation_task = Arc::new(ConversationTask {
            sender,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            active_at: AtomicU64::new(unix_ms()),
        });

        let agent_hook = DynAgentHook::new(conversation_task.clone());
        ctx.base.set_state(agent_hook);

        let shell_hook = ShellToolHook::new(conversation_task.clone());
        ctx.base.set_state(shell_hook);

        self.insert_processing_task(task_key, conversation_task.clone());

        let assistant = self.clone();
        let chat_history = if msg.content.is_empty() {
            vec![]
        } else {
            vec![msg]
        };
        let mut runner = ctx.clone().completion_iter(
            CompletionRequest {
                instructions,
                prompt,
                chat_history,
                tools: ctx.definitions(Some(&assistant.inner.tools)).await,
                tool_choice_required: false,
                max_output_tokens: Some(50000),
                ..Default::default()
            },
            resources.clone(),
        );

        let source = ctx
            .meta()
            .get_extra_as::<String>("source")
            .unwrap_or_else(|| format!("conversation:{id}"));
        let context = Some(InputContext {
            counterparty: Some(caller.to_string()),
            agent: Some(AndaBot::NAME.to_string()),
            source: Some(source),
            topic: None,
        });

        runner.set_unbound(!wait_for_completion);
        let (completion_tx, completion_rx) = if wait_for_completion {
            let (tx, rx) = tokio::sync::oneshot::channel::<AgentOutput>();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        tokio::spawn(async move {
            let mut completion_tx = completion_tx;
            let task_result = async {
                let mut first_round = true;
                loop {
                    let mut inputs = Vec::new();

                    while let Ok(input) = rx.try_recv() {
                        inputs.push(input);
                    }

                    if !inputs.is_empty() {
                        conversation_task
                            .active_at
                            .store(unix_ms(), Ordering::SeqCst);
                    }

                    for input in inputs {
                        runner.accumulate(&input.usage);

                        if input.prompt.trim().is_empty() {
                            // PING from the user to keep the conversation alive, update the active_at timestamp and continue without sending a new prompt to the runner.
                            continue;
                        }

                        // Cancel the conversation if the user sends "/stop" or "/cancel" without any additional prompt.
                        if input.prompt.trim().eq_ignore_ascii_case("/stop")
                            || input.prompt.trim().eq_ignore_ascii_case("/cancel")
                        {
                            ctx.cancellation_token().cancel();
                            break;
                        }

                        let prompt = prompt_with_resources(
                            input.prompt.trim().to_string(),
                            &input.resources,
                        );
                        if prompt.starts_with("/steer")
                            || prompt.starts_with("/stop")
                            || prompt.starts_with("/cancel")
                        {
                            runner.steer(prompt.clone());
                        } else {
                            runner.follow_up(prompt.clone());
                        }
                    }

                    match runner.next().await {
                        Ok(None) => {
                            let now_ms = unix_ms();
                            let idle = now_ms
                                .saturating_sub(conversation_task.active_at.load(Ordering::SeqCst));
                            let has_background_tasks =
                                !conversation_task.background_tasks.read().is_empty();

                            if idle > CONVERSATION_IDLE_MS && !has_background_tasks
                                || (idle > CONVERSATION_WAIT_BACKGROUND_TASK_MS
                                    && has_background_tasks)
                            {
                                conversation.status = ConversationStatus::Completed;
                                conversation.updated_at = now_ms;
                                persist_conversation_state(
                                    &assistant.inner.conversations,
                                    &conversation,
                                )
                                .await;
                                break;
                            } else {
                                if conversation.status != ConversationStatus::Idle {
                                    conversation.status = ConversationStatus::Idle;
                                    conversation.updated_at = now_ms;
                                    persist_conversation_state(
                                        &assistant.inner.conversations,
                                        &conversation,
                                    )
                                    .await;
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                continue;
                            }
                        }

                        Ok(Some(mut res)) => {
                            let now_ms = unix_ms();
                            conversation_task.active_at.store(now_ms, Ordering::SeqCst);
                            let is_done = runner.is_done();
                            res.conversation = Some(id);
                            if voice_response
                                && is_done
                                && let Err(err) = assistant.attach_tts_artifact(&mut res).await
                            {
                                log::warn!("Failed to synthesize voice response: {err}");
                            }
                            let output_for_caller = is_done.then(|| res.clone());

                            conversation_task.on_completion(&ctx, &res).await;
                            if !is_done {
                                runner.prune_raw_history_if(13, 6);
                            }

                            if first_round {
                                first_round = false;
                                conversation.messages.clear();
                                conversation.append_messages(res.chat_history);
                            } else {
                                let existing_len = conversation.messages.len();
                                if res.chat_history.len() >= existing_len {
                                    res.chat_history.drain(0..existing_len);
                                    conversation.append_messages(res.chat_history);
                                } else {
                                    conversation.messages.clear();
                                    conversation.append_messages(res.chat_history);
                                }
                            }

                            conversation.status = if res.failed_reason.is_some() {
                                ConversationStatus::Failed
                            } else if is_done {
                                ConversationStatus::Completed
                            } else {
                                ConversationStatus::Working
                            };
                            conversation.usage = res.usage;
                            conversation.updated_at = now_ms;
                            conversation.failed_reason = res.failed_reason.take();

                            persist_conversation_state(
                                &assistant.inner.conversations,
                                &conversation,
                            )
                            .await;

                            if conversation.failed_reason.is_none() {
                                let timestamp = rfc3339_datetime(now_ms);
                                let submit_formation_at = conversation.messages.len();
                                let messages = conversation_chat_history(
                                    &conversation,
                                    conversation_task.submit_formation_at.load(Ordering::SeqCst)
                                        as usize,
                                );
                                if let Err(err) = assistant
                                    .submit_formation(&messages, &context, &timestamp)
                                    .await
                                {
                                    log::error!(
                                        "Failed to send formation for conversation {id}: {:?}",
                                        err
                                    );
                                } else {
                                    conversation_task
                                        .submit_formation_at
                                        .store(submit_formation_at as u64, Ordering::SeqCst);
                                }
                            }

                            if conversation.status == ConversationStatus::Cancelled
                                || conversation.status == ConversationStatus::Failed
                                || is_done
                            {
                                if let Some(output) = output_for_caller
                                    && let Some(tx) = completion_tx.take()
                                {
                                    let _ = tx.send(output);
                                }
                                break;
                            }
                        }

                        Err(err) => {
                            let failed_reason = err.to_string();
                            log::error!("Conversation {id} in CompletionRunner error: {:?}", err);
                            conversation.failed_reason = Some(failed_reason.clone());
                            conversation.status = ConversationStatus::Failed;
                            conversation.updated_at = unix_ms();
                            persist_conversation_state(
                                &assistant.inner.conversations,
                                &conversation,
                            )
                            .await;
                            if let Some(tx) = completion_tx.take() {
                                let _ = tx.send(AgentOutput {
                                    conversation: Some(id),
                                    failed_reason: Some(failed_reason),
                                    ..Default::default()
                                });
                            }
                            break;
                        }
                    }
                }

                Ok::<(), BoxError>(())
            }
            .await;

            assistant.remove_processing_task(&task_key);
            if let Err(err) = task_result {
                log::error!("Error occurred in conversation {id}: {:?}", err);
            }
        });

        if let Some(rx) = completion_rx {
            return Ok(rx.await.unwrap_or(res));
        }

        Ok(res)
    }
}

struct ConversationTask {
    sender: tokio::sync::mpsc::Sender<ConversationInput>,
    background_tasks: Arc<RwLock<HashMap<String, BackgroundTaskInfo>>>,
    completion_hooks: Arc<Vec<Arc<dyn CompletionHook>>>,
    submit_formation_at: AtomicU64,
    active_at: AtomicU64,
}

#[async_trait]
impl CompletionHook for ConversationTask {
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
impl AgentHook for ConversationTask {
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
                prompt,
                resources: vec![],
                usage: output.usage,
            })
            .await
            .ok();
    }
}

#[async_trait]
impl ToolHook<ExecArgs, ExecOutput> for ConversationTask {
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
                prompt: format!(
                    "Background task {task_id} completed:\n\n{}",
                    serde_json::to_string(&output.output).unwrap_or_default()
                ),
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
    prompt: String,
    resources: Vec<Resource>,
    usage: Usage,
}

fn text_resource_documents(resources: &[Resource]) -> Vec<Document> {
    let mut user_resources: Vec<Document> = Vec::with_capacity(resources.len());
    for resource in resources {
        if resource.tags.iter().any(|tag| tag == "text" || tag == "md")
            && let Some(content) = resource
                .blob
                .as_ref()
                .and_then(|blob| String::from_utf8(blob.0.clone()).ok())
        {
            user_resources.push(Document::from_text(
                resource._id.to_string().as_str(),
                &content,
            ));
        }
    }

    user_resources
}

fn prompt_with_resources(prompt: String, resources: &[Resource]) -> String {
    let user_resources = text_resource_documents(resources);
    if user_resources.is_empty() {
        prompt
    } else {
        format!(
            "{prompt}\n\n{}",
            Documents::new("user_resources".to_string(), user_resources)
        )
    }
}

fn agent_output_text_for_tts(output: &AgentOutput) -> String {
    if !output.content.trim().is_empty() {
        return output.content.clone();
    }

    output
        .chat_history
        .iter()
        .rev()
        .find(|msg| msg.role == "assistant")
        .and_then(Message::text)
        .unwrap_or_default()
}

fn conversation_chat_history(conversation: &Conversation, skip: usize) -> Vec<Message> {
    conversation
        .messages
        .iter()
        .skip(skip)
        .filter_map(|raw| match serde_json::from_value::<Message>(raw.clone()) {
            Ok(mut msg) => {
                let pruned = msg.prune_content();
                if msg.content.is_empty() || pruned > 0 && msg.content.len() <= 1 {
                    None
                } else {
                    Some(msg)
                }
            }
            Err(_) => None,
        })
        .collect()
}

async fn persist_conversation_state(conversations: &Conversations, conversation: &Conversation) {
    match conversation.to_changes() {
        Ok(changes) => {
            let _ = conversations
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
