//! The session runner: drives a session's completion loop, handles idle
//! waiting, goal supervision, history compaction, and memory formation
//! submission.

use anda_core::{
    BoxError, CompletionRequest, ContentPart, Message, Resource, StateFeatures, Usage,
};
use anda_engine::{
    context::{AgentCtx, CompletionRunner},
    memory::{Conversation, ConversationRef, ConversationStatus},
    rfc3339_datetime, unix_ms,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
};

use super::{
    AndaBot,
    session::{ConversationInput, Session},
};
use crate::engine::{
    CompletionHook,
    conversation::SourceState,
    goal::{self},
    model_retry, multimodal,
    prompt::{PromptCommand, skill_subagent},
    system::{mark_special_user_messages, system_extra_user_context, system_user_message},
};

const MAX_TURNS_TO_COMPACT: usize = 81; // The number of turns after which the conversation history will be compacted. This is to prevent the conversation history from growing indefinitely and causing performance issues. The optimal value may depend on the typical length of conversations and the token limits of the language model.
const CONVERSATION_IDLE_MS: u64 = 10 * 60 * 1000; // 10 minutes
const CONVERSATION_WAIT_BACKGROUND_TASK_MS: u64 = 12 * 60 * 60 * 1000; // 12 hours
// Wait this long after a failed memory formation submission before retrying.
// The idle loop reaches the submission point about once per second; without a
// backoff a failing brain endpoint would be hammered continuously.
const FORMATION_RETRY_BACKOFF_MS: u64 = 60 * 1000;
// Wait this long after a failed goal supervisor evaluation before retrying.
// The idle loop reaches the goal check about once per second; without a
// backoff a failing supervisor model would be hammered continuously.
const GOAL_CHECK_RETRY_BACKOFF_MS: u64 = 60 * 1000;
static COMPACTION_PROMPT: &str = include_str!("../../../assets/CompactionPrompt.md");

impl AndaBot {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_session_runner(
        &self,
        ctx: AgentCtx,
        mut req: CompletionRequest,
        resources: Vec<Resource>,
        reserve_chat_history: Vec<Message>,
        session: Arc<Session>,
        conversation: Conversation,
        mut rx: tokio::sync::mpsc::Receiver<ConversationInput>,
        extra_user_context: Option<Message>,
    ) {
        let assistant = self.clone();
        tokio::spawn(async move {
            let (resources, media_usage) =
                multimodal::understand_media_resources(&ctx, resources).await;
            let resources_without_blob = assistant
                .persist_resources_for_message(ctx.caller(), resources)
                .await
                .unwrap_or_default();
            let content = resources_without_blob
                .into_iter()
                .map(|res| ContentPart::any_from("Resource", res))
                .collect::<Vec<_>>();
            let mut runner = ctx
                .clone()
                .completion_iter(
                    CompletionRequest {
                        content,
                        ..req.clone()
                    },
                    vec![],
                )
                .unbound();
            runner.accumulate(&media_usage);
            if !reserve_chat_history.is_empty() {
                runner = runner.reserve_chat_history(reserve_chat_history);
            }

            // Clear the prompt and raw_history to be used for the session.
            req.prompt.clear();
            req.raw_history.clear();
            let mut tools_usage_snapshot: HashMap<String, Usage> = HashMap::new();
            let mut sess_runner = SessionRunner {
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
            let mut pending_inputs = Vec::new();

            loop {
                let mut inputs = std::mem::take(&mut pending_inputs);

                while let Ok(input) = rx.try_recv() {
                    inputs.push(input);
                }

                match sess_runner.run(inputs, &mut tools_usage_snapshot).await {
                    Ok(continue_active) => {
                        if !continue_active {
                            while let Ok(input) = rx.try_recv() {
                                pending_inputs.push(input);
                            }

                            let has_background_tasks = !session.background_tasks.read().is_empty();
                            if should_continue_session_runner_after_stop(
                                &sess_runner.conversation.status,
                                !pending_inputs.is_empty(),
                                has_background_tasks,
                            ) {
                                continue;
                            }

                            // Shutting down: stop accepting new inputs first, so a
                            // concurrent join that already holds the sender fails
                            // and falls back to starting a fresh session, then
                            // rescue anything that slipped in between the drain
                            // above and the close.
                            rx.close();
                            while let Ok(input) = rx.try_recv() {
                                pending_inputs.push(input);
                            }
                            if should_continue_session_runner_after_stop(
                                &sess_runner.conversation.status,
                                !pending_inputs.is_empty(),
                                false,
                            ) {
                                continue;
                            }

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

    fn rebuild_runner_after_model_error(&mut self) {
        let mut chat_history = self.runner.chat_history().clone();
        while let Some(last) = chat_history.last() {
            if last.tool_calls().is_empty() {
                break;
            }
            chat_history.pop();
        }
        mark_special_user_messages(&mut chat_history);

        self.runner = self
            .ctx
            .clone()
            .completion_iter(
                CompletionRequest {
                    chat_history,
                    ..self.req.clone()
                },
                Vec::new(),
            )
            .unbound();
    }

    async fn submit_pending_formation(&self, chat_history: &[Message], now_ms: u64) {
        if now_ms < self.session.formation_backoff_until.load(Ordering::SeqCst) {
            return;
        }

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
                self.session
                    .formation_backoff_until
                    .store(0, Ordering::SeqCst);
            }
            Err(err) => {
                // Keep the offset so the window is retried, but not before the
                // backoff expires — the idle loop reaches this point every
                // second and must not hammer a failing brain endpoint.
                self.session.formation_backoff_until.store(
                    unix_ms().saturating_add(FORMATION_RETRY_BACKOFF_MS),
                    Ordering::SeqCst,
                );
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
            let ConversationInput {
                command,
                resources,
                extra,
                usage,
            } = input;

            // 累计来自于后台任务的工具使用情况
            self.runner.accumulate(&usage);

            let (resources, media_usage) =
                multimodal::understand_media_resources(&self.ctx, resources).await;
            self.runner.accumulate(&media_usage);
            let resources_without_blob = self
                .assistant
                .persist_resources_for_message(self.ctx.caller(), resources)
                .await
                .unwrap_or_default();
            let mut content = resources_without_blob
                .into_iter()
                .map(|res| ContentPart::any_from("Resource", res))
                .collect::<Vec<_>>();

            if let Some(msg) = system_extra_user_context(&extra)
                && self.last_extra_user_context.as_ref() != Some(&msg)
            {
                self.extra_user_context = Some(msg.clone());
                self.last_extra_user_context = Some(msg);
            }

            match command {
                PromptCommand::Ping | PromptCommand::Invalid { .. } => {
                    // PING from the user to keep the conversation alive.
                    log::info!(
                        "Received PING from user in session {}, conversation {}",
                        self.session.id,
                        self.conversation._id
                    );
                }
                PromptCommand::Stop { prompt } => {
                    cancellation_requested = Some(prompt);
                    break;
                }
                PromptCommand::New { .. } => {
                    log::warn!(
                        "Received unexpected /new command in session {}, conversation {}. The /new command should be handled in the agent run() method and should not reach the session runner. Ignoring.",
                        self.session.id,
                        self.conversation._id
                    );
                }
                PromptCommand::Plain { prompt }
                | PromptCommand::Side { prompt }
                | PromptCommand::Steer { prompt } => {
                    prepend_prompt_content(&mut content, prompt);
                    self.runner.follow_up_content(content);
                }
                PromptCommand::Goal { prompt } => {
                    prepend_prompt_content(&mut content, prompt.clone());
                    self.runner.follow_up_content(content);

                    let mut next_goal = self.session.goal.write();
                    if let Some(existing_goal) = next_goal.as_mut() {
                        existing_goal.update_objective(prompt);
                    } else {
                        *next_goal = Some(goal::GoalState::new(prompt));
                    };
                }
                PromptCommand::Skill { mut skill, prompt } => {
                    if let Some(subagent) =
                        skill_subagent(&self.assistant.inner.skills_manager, &skill)
                    {
                        skill = subagent.name;
                    }
                    prepend_prompt_content(
                        &mut content,
                        format!("Use the {skill} skill to handle this request:\n\n{prompt}"),
                    );
                    self.runner.follow_up_content(content);
                }
            }
        }

        let now_ms = unix_ms();
        if let Some(failed_reason) = cancellation_requested {
            self.persist_tools_usage_snapshot(tools_usage_snapshot)
                .await;
            self.submit_pending_formation(self.runner.chat_history(), now_ms)
                .await;

            self.conversation.failed_reason = Some(failed_reason.clone());
            self.conversation.messages.push(json!(Message {
                role: "user".into(),
                content: vec![failed_reason.into()],
                timestamp: Some(now_ms),
                ..Default::default()
            }));
            self.conversation.status = ConversationStatus::Cancelled;

            self.conversation.updated_at = now_ms;
            self.persist_conversation_state().await;
            return Ok(false);
        }

        if self.conversation.status != ConversationStatus::Working && !self.runner.is_idle() {
            self.conversation.status = ConversationStatus::Working;
            self.conversation.failed_reason = None;
            self.conversation.updated_at = now_ms;
            self.persist_conversation_state().await;
        }

        if let Some(mut extra_user_context) = self.extra_user_context.take() {
            if let Some(datetime) = rfc3339_datetime(now_ms) {
                extra_user_context.content.push(ContentPart::Text {
                    text: format!("Current datetime: {}", datetime),
                });
            }

            self.runner.implicit_context(extra_user_context);
        }

        // Mirror the runner's idle state onto the session for the bot-level
        // idle monitor. The flag refreshes on every loop iteration: about
        // once per second while idle, and per completed turn while working
        // (is_idle is false here whenever a turn is about to run).
        self.session
            .runner_idle
            .store(self.runner.is_idle(), Ordering::SeqCst);

        match model_retry::runner_next_with_retry(&mut self.runner, "session runner").await {
            Ok(None) => {
                let now_ms = unix_ms();

                self.persist_tools_usage_snapshot(tools_usage_snapshot)
                    .await;
                self.submit_pending_formation(self.runner.chat_history(), now_ms)
                    .await;

                let maybe_goal =
                    if now_ms >= self.session.goal_check_backoff_until.load(Ordering::SeqCst) {
                        self.session.goal.write().take()
                    } else {
                        None
                    };
                let mut goal_continue_prompt: Option<String> = None;
                let mut active = false;
                if let Some(mut goal) = maybe_goal {
                    match goal.check_progress(&self.runner, &self.ctx).await {
                        Ok(check) => {
                            self.session
                                .goal_check_backoff_until
                                .store(0, Ordering::SeqCst);
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
                            // Keep the goal: a transient supervisor failure
                            // must not silently drop a long-running objective.
                            // Retry after a backoff so a failing supervisor is
                            // not hammered by the once-per-second idle loop.
                            self.session.goal_check_backoff_until.store(
                                unix_ms().saturating_add(GOAL_CHECK_RETRY_BACKOFF_MS),
                                Ordering::SeqCst,
                            );
                            let mut slot = self.session.goal.write();
                            if slot.is_none() {
                                *slot = Some(goal);
                            }
                        }
                    }
                }

                if needs_compaction(&self.runner) {
                    // 上下文过长，先进行一次压缩总结，更新conversation状态和历史消息，再继续后续的处理
                    let mut output = model_retry::runner_finalize_with_retry(
                        &mut self.runner,
                        Some(COMPACTION_PROMPT.to_string()),
                        "session compaction",
                    )
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
                    let artifacts = self
                        .assistant
                        .persist_resources_for_message(&self.conversation.user, output.artifacts)
                        .await?;

                    self.conversation.messages.clear();
                    self.conversation.append_messages(output.chat_history);
                    self.conversation.status = ConversationStatus::Completed;
                    self.conversation.usage = output.usage;
                    self.conversation.artifacts = artifacts;
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
                self.rebuild_runner_after_model_error();
                self.submit_pending_formation(
                    self.runner.chat_history(),
                    self.conversation.updated_at,
                )
                .await;

                let has_background_tasks = !self.session.background_tasks.read().is_empty();
                if has_background_tasks {
                    log::warn!(
                        "Session {} hit a model error while background tasks are running; keeping the session alive to receive background results",
                        self.session.id
                    );
                    self.conversation.failed_reason = None;
                    self.conversation.status = ConversationStatus::Idle;
                    self.conversation.usage = self.runner.total_usage().clone();
                    self.conversation.updated_at = unix_ms();
                    self.persist_conversation_state().await;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    return Ok(true);
                }

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

fn prepend_prompt_content(content: &mut Vec<ContentPart>, prompt: String) {
    if prompt.is_empty() {
        return;
    }
    content.insert(0, prompt.into());
}

fn should_continue_session_runner_after_stop(
    status: &ConversationStatus,
    has_pending_inputs: bool,
    has_background_tasks: bool,
) -> bool {
    !matches!(status, ConversationStatus::Cancelled) && (has_pending_inputs || has_background_tasks)
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
    // context_window is 0 when the model config does not declare it; only then
    // fall back to a conservative default. A floor above a small declared
    // window would disable token-based compaction until the context overflows.
    let context_window = runner.model().context_window as u64;
    let threshold = if context_window == 0 {
        100_000
    } else {
        context_window.saturating_mul(8).saturating_div(10)
    };

    current_usage.input_tokens >= threshold || runner.turns() >= MAX_TURNS_TO_COMPACT
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

    #[test]
    fn session_runner_stop_policy_keeps_background_work() {
        assert!(should_continue_session_runner_after_stop(
            &ConversationStatus::Failed,
            true,
            false,
        ));
        assert!(should_continue_session_runner_after_stop(
            &ConversationStatus::Completed,
            false,
            true,
        ));
        assert!(!should_continue_session_runner_after_stop(
            &ConversationStatus::Failed,
            false,
            false,
        ));
        assert!(!should_continue_session_runner_after_stop(
            &ConversationStatus::Cancelled,
            true,
            true,
        ));
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
}
