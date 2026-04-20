use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, Document, Documents, Message,
    Resource, StateFeatures,
};
use anda_db::schema::DocumentId;
use anda_engine::{
    context::AgentCtx,
    extension::note::{NoteTool, load_notes},
    memory::{Conversation, ConversationRef, ConversationStatus, Conversations, MemoryManagement},
    rfc3339_datetime, unix_ms,
};
use parking_lot::RwLock;
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{HippocampusHook, SELF_USER_ID, SYSTEM_PROMPT_DYNAMIC_BOUNDARY};
use crate::types::{MaintenanceAt, MaintenanceScope};

const SELF_INSTRUCTIONS: &str = include_str!("../../assets/HippocampusMaintenance.md");

/// Resets the AtomicBool to false on drop (panic guard for processing flag).
struct ProcessingGuard(Arc<AtomicBool>);
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone)]
pub struct MaintenanceAgent {
    pub conversations: Conversations,
    memory: Arc<MemoryManagement>,
    processing: Arc<AtomicBool>,
    hook: Arc<dyn HippocampusHook>,
    history: Arc<RwLock<VecDeque<Document>>>,
}

impl MaintenanceAgent {
    pub const NAME: &'static str = "maintenance_memory";
    pub fn new(
        memory: Arc<MemoryManagement>,
        conversations: Conversations,
        hook: Arc<dyn HippocampusHook>,
    ) -> Self {
        Self {
            memory,
            conversations,
            processing: Arc::new(AtomicBool::new(false)),
            hook,
            history: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub async fn init(&self) -> Result<(), BoxError> {
        let (conversations, _) = self
            .conversations
            .list_conversations_by_user(&SELF_USER_ID, None, Some(2))
            .await?;
        *self.history.write() = conversations.into_iter().map(Document::from).collect();
        Ok(())
    }

    pub fn is_processing(&self) -> bool {
        self.processing.load(Ordering::SeqCst)
    }

    pub fn get_processed(&self) -> Option<DocumentId> {
        match self.conversations.conversations.max_document_id() {
            0 => None,
            id => Some(id),
        }
    }

    pub fn get_processed_at(&self) -> MaintenanceAt {
        let mut rt = MaintenanceAt::default();
        self.conversations.conversations.extensions_with(|kv| {
            if let Some(v) = kv.get("full")
                && let Ok(id) = v.try_into()
            {
                rt.full = id;
            }
            if let Some(v) = kv.get("quick")
                && let Ok(id) = v.try_into()
            {
                rt.quick = id;
            }
            if let Some(v) = kv.get("daydream")
                && let Ok(id) = v.try_into()
            {
                rt.daydream = id;
            }
        });
        rt
    }

    pub fn set_processed_at(&self, scope: MaintenanceScope, formation_id: DocumentId) {
        self.conversations
            .conversations
            .set_extension_from(scope.to_string(), formation_id);
    }
}

impl Agent<AgentCtx> for MaintenanceAgent {
    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "The Hippocampus Maintenance agent operates in Sleep Mode — performing memory metabolism including consolidation, organization, pruning, and health optimization of the Cognitive Nexus during scheduled maintenance cycles.".to_string()
    }

    fn tool_dependencies(&self) -> Vec<String> {
        vec!["execute_kip".to_string(), NoteTool::NAME.to_string()]
    }

    /// Receives a trigger envelope (MaintenanceInput JSON), creates a conversation to track the
    /// maintenance cycle, and runs the sleep cycle workflow.
    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String, // MaintenanceInput serialized as JSON string
        _resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        // Prevent concurrent maintenance runs
        if self
            .processing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(AgentOutput {
                content: "Maintenance cycle is already in progress.".to_string(),
                ..Default::default()
            });
        }

        let caller = ctx.caller();
        let now_ms = unix_ms();

        let mut conversation = Conversation {
            user: *caller,
            messages: vec![json!(Message {
                role: "user".into(),
                content: vec![prompt.into()],
                ..Default::default()
            })],
            status: ConversationStatus::Working,
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            label: Some("maintenance".to_string()),
            ..Default::default()
        };

        let id = self
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = id;

        let agent = self.clone();
        let ctx_clone = ctx.clone();
        tokio::spawn(async move {
            // Guard resets processing to false when the task completes or panics.
            let _guard = ProcessingGuard(agent.processing.clone());
            agent.process_one(&ctx_clone, &mut conversation).await;
            agent
                .hook
                .on_conversation_end(MaintenanceAgent::NAME, &conversation)
                .await;
            // Trigger formation after maintenance completes
            agent.hook.try_start_formation().await;
        });

        Ok(AgentOutput {
            conversation: Some(id),
            ..Default::default()
        })
    }
}

impl MaintenanceAgent {
    async fn mark_conversation_failed(&self, conversation: &mut Conversation, reason: String) {
        log::error!(
            "Maintenance conversation {} failed: {}",
            conversation._id,
            reason
        );
        conversation.failed_reason = Some(reason);
        conversation.status = ConversationStatus::Failed;
        conversation.updated_at = unix_ms();
        if let Ok(changes) = conversation.to_changes() {
            let _ = self
                .conversations
                .update_conversation(conversation._id, changes)
                .await;
        }
    }

    async fn process_one(&self, ctx: &AgentCtx, conversation: &mut Conversation) {
        let prompt = match conversation
            .messages
            .first()
            .and_then(|v| serde_json::from_value::<Message>(v.clone()).ok())
            .and_then(|v| v.text())
        {
            Some(p) => p,
            None => {
                self.mark_conversation_failed(conversation, "No prompt found".to_string())
                    .await;
                return;
            }
        };

        let primer = self.memory.describe_primer().await.unwrap_or_default();
        let now_ms = unix_ms();
        let chat_history: Vec<Document> = { self.history.read().iter().cloned().collect() };

        let chat_history = if chat_history.is_empty() {
            vec![]
        } else {
            vec![Message {
                role: "user".into(),
                content: vec![
                    Documents::new("history_maintenance".to_string(), chat_history)
                        .to_string()
                        .into(),
                ],
                name: Some("$system".into()),
                timestamp: Some(now_ms),
                ..Default::default()
            }]
        };
        let notes = load_notes(ctx).await.unwrap_or_default();
        let mut runner = ctx.clone().completion_iter(
            CompletionRequest {
                instructions: format!(
                    "{}\n\n{}\n\n---\n\n# `DESCRIBE PRIMER` Result:\n{}\n\n---\n\n# Your notes:\n{}\n\n# Current datetime: {}",
                    SELF_INSTRUCTIONS,
                    SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
                    primer,
                    serde_json::to_string(&notes.notes).unwrap_or_default(),
                    rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
                ),
                prompt,
                chat_history,
                tools: ctx.tool_definitions(Some(&self.tool_dependencies())),
                tool_choice_required: true,
                max_output_tokens: Some(8192),
                ..Default::default()
            },
            vec![],
        );

        let mut first_round = true;
        loop {
            match runner.next().await {
                Ok(None) => break,
                Ok(Some(mut res)) => {
                    let now_ms = unix_ms();

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
                    } else if runner.is_done() {
                        ConversationStatus::Completed
                    } else {
                        ConversationStatus::Working
                    };
                    conversation.usage = res.usage;
                    conversation.updated_at = now_ms;

                    if let Some(failed_reason) = res.failed_reason {
                        conversation.failed_reason = Some(failed_reason);
                    } else {
                        let doc: Document = conversation.clone().into();
                        let mut history = self.history.write();
                        history.push_back(doc);
                        let len = history.len();
                        if len > 3 {
                            history.drain(0..(len - 2));
                        }
                    }

                    match conversation.to_changes() {
                        Ok(changes) => {
                            let _ = self
                                .conversations
                                .update_conversation(conversation._id, changes)
                                .await;
                        }
                        Err(err) => {
                            log::error!(
                                "Failed to serialize maintenance conversation {} changes: {:?}",
                                conversation._id,
                                err
                            );
                        }
                    }

                    if conversation.status == ConversationStatus::Cancelled
                        || conversation.status == ConversationStatus::Failed
                    {
                        break;
                    }
                }
                Err(err) => {
                    self.mark_conversation_failed(
                        conversation,
                        format!("CompletionRunner error: {err:?}"),
                    )
                    .await;
                    break;
                }
            }
        }
    }
}
