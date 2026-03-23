use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CompletionRequest, Message, Resource,
    StateFeatures, Tool,
};
use anda_db::schema::DocumentId;
use anda_engine::{
    context::AgentCtx,
    memory::{Conversation, ConversationRef, ConversationStatus, MemoryManagement},
    rfc3339_datetime, unix_ms,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use super::AgentHooks;

const SELF_INSTRUCTIONS: &str = include_str!("../../assets/HippocampusFormation.md");
const REVIEW_INSTRUCTIONS: &str = include_str!("../../assets/HippocampusFormationReview.md");
const MAX_FORMATION_BYTES: usize = 100_000;

/// Resets the AtomicU64 to 0 on drop (panic guard for processing_conversation).
struct ProcessingGuard(Arc<AtomicU64>);
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        self.0.store(0, Ordering::SeqCst);
    }
}

#[derive(Clone)]
pub struct FormationAgent {
    memory: Arc<MemoryManagement>,
    processing_conversation: Arc<AtomicU64>,
    hooks: Arc<dyn AgentHooks>,

    #[allow(dead_code)]
    max_input_tokens: usize,
}

impl FormationAgent {
    pub const NAME: &'static str = "formation_memory";
    pub fn new(
        memory: Arc<MemoryManagement>,
        hooks: Arc<dyn AgentHooks>,
        max_input_tokens: usize,
    ) -> Self {
        Self {
            max_input_tokens,
            memory,
            processing_conversation: Arc::new(AtomicU64::new(0)),
            hooks,
        }
    }

    pub fn is_processing(&self) -> bool {
        self.processing_conversation.load(Ordering::SeqCst) != 0
    }

    pub fn get_processed(&self) -> Option<DocumentId> {
        self.memory
            .conversations
            .get_extension_as::<DocumentId>("hippocampus_processed")
    }

    pub async fn start_process(
        &self,
        ctx: AgentCtx,
        conversation: DocumentId,
    ) -> Result<(), BoxError> {
        let current = self.processing_conversation.load(Ordering::SeqCst);
        if current != 0 {
            return Err(format!(
                "FormationAgent is already processing conversation {}",
                current
            )
            .into());
        }

        // Find the next valid pending conversation starting from the given ID (inclusive)
        let conv = self
            .find_next_submitted(conversation.saturating_sub(1))
            .await
            .ok_or_else(|| {
                format!(
                    "No pending formation conversation found starting from {}",
                    conversation
                )
            })?;

        self.try_process(ctx, conv);
        Ok(())
    }

    pub fn try_process(&self, ctx: AgentCtx, conversation: Conversation) {
        if self
            .processing_conversation
            .compare_exchange(0, conversation._id, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            log::info!(
                "FormationAgent is already processing conversation {}, cannot process conversation {}",
                self.processing_conversation.load(Ordering::SeqCst),
                conversation._id
            );
            return;
        }

        let agent = self.clone();
        let pc = self.processing_conversation.clone();
        tokio::spawn(async move {
            // Guard resets processing_conversation to 0 if the task panics.
            let guard = ProcessingGuard(pc);
            agent.process_loop(ctx, conversation).await;
            // Normal exit: process_loop already manages the atomic properly,
            // so defuse the guard to avoid clobbering a valid value.
            std::mem::forget(guard);
        });
    }

    async fn process_loop(&self, ctx: AgentCtx, mut conversation: Conversation) {
        loop {
            let conv_id = conversation._id;

            self.process_one(&ctx, &mut conversation).await;
            self.hooks
                .on_conversation_end(Self::NAME, &conversation)
                .await;
            if conversation.status == ConversationStatus::Failed {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await; // 避免快速失败循环
                // 重试一次
                self.process_one(&ctx, &mut conversation).await;
                self.hooks
                    .on_conversation_end(Self::NAME, &conversation)
                    .await;
            }

            if conversation.status != ConversationStatus::Completed {
                log::error!(
                    "Conversation {} ended with status {:?}, not marking as processed",
                    conv_id,
                    conversation.status
                );
                // 上游异常，重置 processing 状态以允许外部干预或后续请求自动触发
                self.processing_conversation.store(0, Ordering::SeqCst);
                break;
            }

            self.memory
                .conversations
                .save_extension("hippocampus_processed".to_string(), conv_id.into())
                .await
                .ok();

            if let Some(id) = self.hooks.try_start_maintenance(conv_id).await {
                log::info!(
                    "Triggered maintenance for conversation {}, new maintenance conversation {}",
                    conv_id,
                    id
                );

                // 重置 processing 状态，以便 maintenance 完成后 try_start_formation 能重新启动
                self.processing_conversation.store(0, Ordering::SeqCst);
                break; // 交由 maintenance agent 处理后续流程，退出循环
            }

            // 查找下一个待处理的 conversation
            match self.find_next_submitted(conv_id).await {
                Some(next_conv) => {
                    if self
                        .processing_conversation
                        .compare_exchange(
                            conv_id,
                            next_conv._id,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        conversation = next_conv;
                        continue;
                    }
                    // CAS 失败说明其他线程已接管，退出
                    break;
                }
                None => {
                    self.processing_conversation.store(0, Ordering::SeqCst);
                    // 双重检查：store(0) 前可能有新 conversation 到达但 try_process CAS 失败
                    if let Some(next_conv) = self.find_next_submitted(conv_id).await
                        && self
                            .processing_conversation
                            .compare_exchange(0, next_conv._id, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                    {
                        conversation = next_conv;
                        continue;
                    }
                    break;
                }
            }
        }
    }

    async fn find_next_submitted(&self, after_id: u64) -> Option<Conversation> {
        let mut id = after_id;
        while id < self.memory.max_conversation_id() {
            id += 1;
            match self.memory.get_conversation(id).await {
                Ok(conv) => {
                    if conv.status == ConversationStatus::Completed
                        || conv.status == ConversationStatus::Cancelled
                    {
                        continue;
                    }
                    if let Some(label) = &conv.label
                        && label != "formation"
                    {
                        continue; // 只处理 label 为 "formation" 的 conversation，跳过其他类型
                    }
                    return Some(conv);
                }
                _ => continue,
            }
        }
        None
    }

    async fn mark_conversation_failed(&self, conversation: &mut Conversation, reason: String) {
        log::error!("Conversation {} failed: {}", conversation._id, reason);
        conversation.failed_reason = Some(reason);
        conversation.status = ConversationStatus::Failed;
        conversation.updated_at = unix_ms();

        if let Ok(changes) = conversation.to_changes() {
            let _ = self
                .memory
                .update_conversation(conversation._id, changes)
                .await;
        }
    }

    async fn process_one(&self, ctx: &AgentCtx, conversation: &mut Conversation) {
        // Deprecated
        let mut prompt = conversation
            .steering_messages
            .take()
            .unwrap_or_default()
            .pop();

        if prompt.is_none()
            && let Some(msg) = conversation.messages.first()
            && let Ok(msg) = serde_json::from_value::<Message>(msg.clone())
        {
            prompt = msg.text();
        }

        let prompt = match prompt {
            Some(p) => p,
            None => {
                self.mark_conversation_failed(conversation, "No prompt found".to_string())
                    .await;
                return;
            }
        };

        let primer = self.memory.describe_primer().await.unwrap_or_default();
        let tools = ctx.tool_definitions(Some(&["execute_kip"]));
        let now_ms = unix_ms();

        let chat_history = vec![
            Message {
                role: "user".into(),
                content: vec![format!("`DESCRIBE PRIMER` result:\n{}", primer).into()],
                ..Default::default()
            },
            Message {
                role: "user".into(),
                content: vec![
                    format!(
                        "Current datetime: {}",
                        rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
                    )
                    .into(),
                ],
                ..Default::default()
            },
        ];

        let mut runner = ctx.completion_iter(
            CompletionRequest {
                instructions: SELF_INSTRUCTIONS.to_string(),
                prompt: prompt.clone(),
                chat_history,
                tools,
                tool_choice_required: true,
                max_output_tokens: Some(10000),
                ..Default::default()
            },
            vec![],
        );

        // Review after formation to ensure quality and correctness
        runner.follow_up(REVIEW_INSTRUCTIONS.to_string());

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
                    }

                    // 检查是否被外部取消，get_conversation 失败不中断处理
                    match self.memory.get_conversation(conversation._id).await {
                        Ok(old) => {
                            if old.status == ConversationStatus::Cancelled
                                && (conversation.status == ConversationStatus::Submitted
                                    || conversation.status == ConversationStatus::Working)
                            {
                                conversation.status = ConversationStatus::Cancelled;
                            }
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to check cancel status for conversation {}: {:?}",
                                conversation._id,
                                err
                            );
                        }
                    }

                    // to_changes 失败不中断处理循环
                    match conversation.to_changes() {
                        Ok(changes) => {
                            let _ = self
                                .memory
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

impl Agent<AgentCtx> for FormationAgent {
    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Receives conversation messages and encodes them into structured memory within the Cognitive Nexus via KIP.".to_string()
    }

    fn tool_dependencies(&self) -> Vec<String> {
        vec![self.memory.name()]
    }

    // 接收来自外部的 FormationInput，创建一个新的 Conversation，并启动处理流程。
    async fn run(
        &self,
        ctx: AgentCtx,
        prompt: String, // FormationInput serialized as JSON string
        _resources: Vec<Resource>,
    ) -> Result<AgentOutput, BoxError> {
        let caller = ctx.caller();
        let now_ms = unix_ms();

        if prompt.len() > MAX_FORMATION_BYTES {
            return Err(format!(
                "Input too large: {} bytes, max allowed is {} bytes",
                prompt.len(),
                MAX_FORMATION_BYTES
            )
            .into());
        }

        let mut conversation = Conversation {
            user: *caller,
            messages: vec![json!(Message {
                role: "user".into(),
                content: vec![prompt.clone().into()],
                ..Default::default()
            })],
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            label: Some("formation".to_string()),
            ..Default::default()
        };

        let id = self
            .memory
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = id;
        let res = AgentOutput {
            conversation: Some(id),
            ..Default::default()
        };

        let is_idle = self.processing_conversation.load(Ordering::SeqCst) == 0;
        if is_idle {
            if let Some(prev_id) = self.get_processed()
                && prev_id + 1 < id
            {
                // Resume from the last processed conversation to catch any missed ones
                if let Some(conv) = self.find_next_submitted(prev_id).await {
                    self.try_process(ctx, conv);
                }
            } else {
                self.try_process(ctx, conversation);
            }
        }

        Ok(res)
    }
}
