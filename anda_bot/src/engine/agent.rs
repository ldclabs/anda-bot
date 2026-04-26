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
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{brain, cron, engine::CompletionHook};

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
}

type ProcessingConversations = RwLock<HashMap<(Principal, u64), Arc<ConversationTask>>>;

impl AndaBot {
    pub const NAME: &'static str = "anda_bot";

    pub fn new(
        brain: brain::Client,
        conversations: Conversations,
        completion_hooks: Vec<Arc<dyn CompletionHook>>,
    ) -> Self {
        Self {
            inner: Arc::new(AndaBotInner {
                brain,
                conversations,
                tool_dependencies: vec![
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
                ],
                tools: vec![
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
                ],
                processing_conversations: RwLock::new(HashMap::new()),
                completion_hooks: Arc::new(completion_hooks),
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
        let input = FormationInputRef {
            messages,
            context,
            timestamp,
        };

        let _ = self.inner.brain.formation(input).await?;
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
        vec!["text".to_string()]
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

        // let user = caller.to_string();
        let now_ms = unix_ms();
        let current_conversation_id = ctx.meta().get_extra_as::<u64>("conversation").unwrap_or(0);
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
            && ancestors.len() > 10 {
                ancestors.drain(0..ancestors.len() - 10);
            }

        let primer = self.inner.brain.describe_primer().await?;
        let user_info = self.inner.brain.user_info(caller.to_string()).await;
        let notes = load_notes(&ctx).await.unwrap_or_default();
        let instructions = format!(
            "{}\n\n{}\n\n---\n\n# Your identity & knowledge domains:\n{}\n\n---\n\n# Your notes:\n{}\n\n# User profile:\n{}\n\n# Current datetime:\n{}",
            SELF_INSTRUCTIONS,
            SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
            primer,
            serde_json::to_string(&notes.notes).unwrap_or_default(),
            serde_json::to_string(&user_info).unwrap_or_default(),
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
            vec![],
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

        runner.set_unbound(true);
        tokio::spawn(async move {
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
                            conversation_task.on_completion(&ctx, &res).await;

                            let is_done = runner.is_done();
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
                                break;
                            }
                        }

                        Err(err) => {
                            log::error!("Conversation {id} in CompletionRunner error: {:?}", err);
                            conversation.failed_reason = Some(err.to_string());
                            conversation.status = ConversationStatus::Failed;
                            conversation.updated_at = unix_ms();
                            persist_conversation_state(
                                &assistant.inner.conversations,
                                &conversation,
                            )
                            .await;
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

fn conversation_chat_history(conversation: &Conversation, skip: usize) -> Vec<Message> {
    conversation
        .messages
        .iter()
        .skip(skip)
        .filter_map(|raw| serde_json::from_value::<Message>(raw.clone()).ok())
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
