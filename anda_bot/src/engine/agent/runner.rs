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
#[cfg(test)]
use crate::engine::SkillLibrary;
use crate::engine::{
    CompletionHook,
    conversation::SourceState,
    goal::{self},
    multimodal,
    prompt::{PromptCommand, skill_subagent},
    system::{
        mark_special_user_messages, system_extra_user_context, system_runtime_prompt,
        system_user_message,
    },
};

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
const COMPACTION_PROMPT: &str = include_str!("../../../assets/CompactionPrompt.md");
const COMPACTION_CONTINUE_PROMPT: &str = "Continue the active work from the compaction handoff. The handoff includes the conversation state and any pending user or tool messages captured immediately before compaction.";

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
            assistant.inner.apply_merge_discovered_tools(&mut runner);
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

                            let has_background_tasks = session.has_running_background_tasks();
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

    async fn stop_current_task(
        &mut self,
        reason: String,
        now_ms: u64,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) {
        self.persist_tools_usage_snapshot(tools_usage_snapshot)
            .await;
        self.submit_pending_formation(self.runner.chat_history(), now_ms)
            .await;

        let content = task_stopped_message(&reason);
        self.session.stop_background_tasks();
        self.runner.append_chat_history(vec![system_user_message(
            system_runtime_prompt("task stopped", &content),
            now_ms,
        )]);

        let mut output = self.runner.stop_current_task(anda_core::AgentOutput {
            content,
            conversation: Some(self.conversation._id),
            ..Default::default()
        });
        mark_special_user_messages(&mut output.chat_history);

        self.session.runner_idle.store(true, Ordering::SeqCst);
        self.conversation.messages.clear();
        self.conversation.append_messages(output.chat_history);
        self.conversation.failed_reason = None;
        self.conversation.status = ConversationStatus::Idle;
        self.conversation.usage = output.usage;
        self.conversation.updated_at = now_ms;
        self.persist_conversation_state().await;
    }

    async fn compact(
        &mut self,
        continuation_prompt: Option<String>,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) -> Result<bool, BoxError> {
        self.persist_tools_usage_snapshot(tools_usage_snapshot)
            .await;

        let (mut runner, output) = match self
            .runner
            .handoff(Some(COMPACTION_PROMPT.to_string()))
            .await
        {
            Ok((runner, output)) => (runner, output),
            Err(err) => {
                self.session.stop_background_tasks();
                self.conversation.failed_reason = Some(format!("Compaction failed: {err}"));
                self.conversation.status = ConversationStatus::Failed;
                self.conversation.updated_at = unix_ms();
                self.persist_conversation_state().await;
                return Ok(false);
            }
        };

        // 如果目标还没有完成，也需要关闭本轮 conversation （conversation 数据大小有限，不应该超过 10MB），为 session 创建新的 conversation 和 runner 继续后续的交互
        // 同一个 session 可以逐步产生不限数量的 conversation 对话，可支持超长程推理。
        // 前一轮压缩总结的内容作为新 conversation 的第一条消息，继续后续的交互
        let now_ms = unix_ms();
        let mut ancestors = self.conversation.ancestors.clone().unwrap_or_default();
        ancestors.push(self.conversation._id);
        let mut child_conversation = Conversation {
            user: self.conversation.user,
            thread: Some(self.session.id.clone()),
            ancestors: Some(ancestors),
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some(json!(self.ctx.meta().extra)),
            ..Default::default()
        };

        let child_id = self
            .assistant
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&child_conversation))
            .await?;
        child_conversation._id = child_id;

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
        self.conversation.child = Some(child_id);
        self.persist_conversation_state().await;

        self.first_round = true;
        self.session.submit_formation_at.store(0, Ordering::SeqCst);
        self.conversation = child_conversation;
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
        self.assistant
            .inner
            .apply_merge_discovered_tools(&mut runner);
        self.runner = runner;
        if let Some(prompt) = continuation_prompt {
            self.runner.follow_up(prompt);
        }

        // Compaction is real work: refresh the activity clock so the idle-timeout check on the
        // turn that follows does not mistake the session for stale.
        self.session.active_at.store(unix_ms(), Ordering::SeqCst);
        Ok(true)
    }

    async fn needed_compact(
        &self,
        follow_up_batch: &[ContentPart],
        steer_batch: &[ContentPart],
    ) -> bool {
        self.runner.needs_compaction_with(|| {
            estimated_content_tokens(follow_up_batch)
                .saturating_add(estimated_content_tokens(steer_batch))
                .saturating_add(
                    self.runner
                        .steering_message_iter()
                        .map(|c| c.estimated_tokens() as u64)
                        .sum(),
                )
                .saturating_add(
                    self.runner
                        .follow_up_message_iter()
                        .map(|c| c.estimated_tokens() as u64)
                        .sum(),
                )
        })
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
        let mut stop_requested: Option<String> = None;
        let mut cancellation_requested: Option<String> = None;
        if !inputs.is_empty() {
            self.session.active_at.store(unix_ms(), Ordering::SeqCst);
        }

        // Accumulate all follow-up content for this batch instead of queueing
        // it input-by-input. Background subagent/shell results arrive as
        // separate inputs and are drained into a single run() call (the channel
        // buffers many of them), so the batch can be far larger than any single
        // input. Queueing each one immediately defeated compaction: only the
        // first input was size-checked, because attaching it made the runner
        // report not-idle and the estimate never saw the already-queued tail.
        // Sizing the whole batch up front lets idle compaction run before the
        // content is attached — and it must run first, because compaction
        // drains queued follow-ups into its own request and would overflow too.
        let mut follow_up_batch: Vec<ContentPart> = Vec::new();
        // Steering is delivered through the runner's separate steering channel: it interrupts the
        // current run and skips pending tool calls, unlike follow-up content which waits for the
        // next safe turn. Keep it out of the follow-up batch so /steer keeps its redirect semantics.
        let mut steer_batch: Vec<ContentPart> = Vec::new();

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
                    stop_requested = Some(control_command_reason(&prompt, "stop"));
                    break;
                }
                PromptCommand::Cancel { prompt } => {
                    cancellation_requested = Some(cancel_reason(&prompt));
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
                | PromptCommand::Loop { prompt } => {
                    prepend_prompt_content(&mut content, prompt);
                    follow_up_batch.append(&mut content);
                }
                PromptCommand::Steer { prompt } => {
                    prepend_prompt_content(&mut content, prompt);
                    steer_batch.append(&mut content);
                }
                PromptCommand::Goal { prompt } => {
                    prepend_prompt_content(&mut content, prompt.clone());
                    follow_up_batch.append(&mut content);

                    let mut next_goal = self.session.goal.write();
                    if let Some(existing_goal) = next_goal.as_mut() {
                        existing_goal.update_objective(prompt);
                    } else {
                        *next_goal = Some(goal::GoalState::new(prompt));
                    };
                }
                PromptCommand::Skill { mut skill, prompt } => {
                    if let Some(subagent) =
                        skill_subagent(self.assistant.inner.skill_library.as_ref(), &skill)
                    {
                        skill = subagent.name;
                    }
                    prepend_prompt_content(
                        &mut content,
                        format!("Use the {skill} skill to handle this request:\n\n{prompt}"),
                    );
                    follow_up_batch.append(&mut content);
                }
            }
        }

        let now_ms = unix_ms();
        if let Some(reason) = stop_requested {
            self.stop_current_task(reason, now_ms, tools_usage_snapshot)
                .await;
            return Ok(true);
        }

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

        if self.conversation.status != ConversationStatus::Working
            && (!follow_up_batch.is_empty() || !steer_batch.is_empty() || !self.runner.is_idle())
        {
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

        // Compact the idle context before attaching anything if doing so would exceed the window,
        // sizing the decision against the follow-up and steering content combined (both land in the
        // next request). Skip when stopping or cancelling: that input discards queued content anyway.
        if self.needed_compact(&follow_up_batch, &steer_batch).await {
            self.session.runner_idle.store(false, Ordering::SeqCst);
            if !self
                .compact(Some(compaction_continue_prompt()), tools_usage_snapshot)
                .await?
            {
                return Ok(false);
            }
        }

        if !follow_up_batch.is_empty() {
            self.runner.follow_up_content(follow_up_batch);
        }
        if !steer_batch.is_empty() {
            self.runner.steer_content(steer_batch);
        }

        // Mirror the runner's idle state onto the session for the bot-level
        // idle monitor. The flag refreshes on every loop iteration: about
        // once per second while idle, and per completed turn while working
        // (is_idle is false here whenever a turn is about to run).
        self.session
            .runner_idle
            .store(self.runner.is_idle(), Ordering::SeqCst);

        let next_result = self.runner.next().await;
        self.assistant
            .inner
            .cache_merge_discovered_tools(&self.runner);

        match next_result {
            Ok(None) => {
                let now_ms = unix_ms();

                // The turn completed without error: clear any stale failure so a
                // later Working/Idle/Completed transition below can never persist
                // a non-Failed status alongside a leftover failed_reason.
                self.conversation.failed_reason = None;

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

                if let Some(prompt) = goal_continue_prompt {
                    self.runner.follow_up(prompt);
                }

                let now_ms = unix_ms();
                let has_background_tasks = self.session.has_running_background_tasks();
                let is_idle = self.runner.is_idle();
                if is_idle {
                    let idle = now_ms.saturating_sub(self.session.active_at.load(Ordering::SeqCst));
                    if !has_background_tasks && self.session.finish_when_idle.load(Ordering::SeqCst)
                    {
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
                }

                let next_status = if !is_idle || has_background_tasks {
                    ConversationStatus::Working
                } else {
                    ConversationStatus::Idle
                };
                if self.conversation.status != next_status {
                    if next_status == ConversationStatus::Working {
                        self.conversation.usage = self.runner.total_usage().clone();
                    }
                    self.conversation.status = next_status;
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
                if is_context_length_error(&err) {
                    log::warn!(
                        "Session {} hit context length error; attempting session compaction before continuing",
                        self.session.id
                    );
                    self.runner.discard_in_flight_request();
                    match self
                        .compact(Some(compaction_continue_prompt()), tools_usage_snapshot)
                        .await
                    {
                        Ok(continue_active) => return Ok(continue_active),
                        Err(compaction_err) => {
                            log::error!(
                                "Session {} failed to compact after context length error: {:?}",
                                self.session.id,
                                compaction_err
                            );
                        }
                    }
                }
                self.persist_tools_usage_snapshot(tools_usage_snapshot)
                    .await;
                self.submit_pending_formation(
                    self.runner.chat_history(),
                    self.conversation.updated_at,
                )
                .await;

                self.session.stop_background_tasks();
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

fn compaction_continue_prompt() -> String {
    system_runtime_prompt(
        "context compaction continuation",
        COMPACTION_CONTINUE_PROMPT,
    )
}

fn is_context_length_error(err: &BoxError) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err.as_ref());
    while let Some(error) = current {
        let message = error.to_string().to_ascii_lowercase();
        if message.contains("context_length_exceeded")
            || message.contains("input exceeds the context window")
            || message.contains("context window")
        {
            return true;
        }
        current = error.source();
    }

    false
}

fn control_command_reason(prompt: &str, command: &str) -> String {
    let trimmed = prompt.trim();
    let Some(body) = trimmed.strip_prefix('/') else {
        return trimmed.to_string();
    };
    let command_end = body.find(char::is_whitespace).unwrap_or(body.len());
    let parsed_command = &body[..command_end];
    if !parsed_command.eq_ignore_ascii_case(command) {
        return trimmed.to_string();
    }

    body[command_end..].trim().to_string()
}

fn cancel_reason(prompt: &str) -> String {
    let reason = control_command_reason(prompt, "cancel");
    if reason.trim().is_empty() {
        "conversation cancelled".to_string()
    } else {
        reason
    }
}

fn task_stopped_message(reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        "Current task stopped. The conversation is idle and ready for the next message.".to_string()
    } else {
        format!("Current task stopped: {reason}")
    }
}

fn should_continue_session_runner_after_stop(
    status: &ConversationStatus,
    has_pending_inputs: bool,
    has_background_tasks: bool,
) -> bool {
    !matches!(
        status,
        ConversationStatus::Cancelled | ConversationStatus::Failed
    ) && (has_pending_inputs || has_background_tasks)
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

fn estimated_content_tokens(content: &[ContentPart]) -> u64 {
    content.iter().map(|c| c.estimated_tokens() as u64).sum()
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

    use crate::brain;
    use crate::engine::agent::session::{BackgroundTaskInfo, SessionRequestMeta};
    use crate::engine::browser::{BrowserBridge, ChromeBrowserTool};
    use crate::engine::conversation::ConversationsTool;
    use crate::engine::prompt::PromptCommand;
    use crate::engine::resources::ResourceStore;
    use anda_brain::types::InputContext;
    use anda_core::{AgentOutput, BoxPinFut, RequestMeta};
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
    };
    use anda_engine::{
        engine::EngineBuilder,
        memory::Conversations,
        model::{CompletionFeaturesDyn, Model},
    };
    use ic_auth_types::Xid;
    use parking_lot::{Mutex, RwLock};
    use std::sync::atomic::{AtomicBool, AtomicU64};

    fn pending_request_messages(req: &CompletionRequest, now_ms: u64) -> Vec<Message> {
        let mut messages = req.chat_history.clone();

        if let Some(datetime) = rfc3339_datetime(now_ms)
            && let Some(message) = req.documents.to_message(&datetime)
        {
            messages.push(message);
        }

        let mut content = Vec::new();
        if !req.prompt.is_empty() {
            content.push(req.prompt.clone().into());
        }
        content.extend(req.content.clone());
        if !content.is_empty() {
            messages.push(Message {
                role: req.role.clone().unwrap_or_else(|| "user".to_string()),
                content,
                timestamp: Some(now_ms),
                ..Default::default()
            });
        }

        messages
    }

    async fn spawn_runner_brain_mock() -> String {
        use axum::{Router, routing};
        let app = Router::new()
            .route(
                "/v1/anda_bot/formation",
                routing::post(|| async {
                    axum::Json(serde_json::json!({"result": {"content": ""}}))
                }),
            )
            .route(
                "/v1/anda_bot/execute_kip_readonly",
                routing::post(|| async {
                    axum::Json(serde_json::json!({"result": {"identity": "panda"}}))
                }),
            )
            .route(
                "/v1/anda_bot/get_or_init_user",
                routing::post(|| async { axum::Json(serde_json::json!({"name": "u"})) }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/v1/anda_bot")
    }

    async fn build_runner_bot_with_brain(brain_url: String) -> AndaBot {
        let object_store: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        let db = Arc::new(
            AndaDB::connect(
                object_store,
                DBConfig {
                    name: "runner_brain_test".to_string(),
                    description: "runner brain test".to_string(),
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
        );
        let brain_client = brain::Client::new(brain_url, Some("t".to_string()))
            .with_http_client(crate::util::http_client::new_reqwest_client());
        let conversations = Conversations::connect(db.clone(), "bot".to_string())
            .await
            .unwrap();
        let conversations_tool =
            Arc::new(ConversationsTool::new(conversations, "/tmp".to_string()));
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let skills = SkillLibrary::for_test(std::env::temp_dir().join("runner_skills2_home"));
        let bridge = Arc::new(BrowserBridge::new());
        AndaBot::new(
            brain_client,
            std::env::temp_dir(),
            conversations_tool,
            resource_store,
            vec![],
            vec![],
            skills,
            Arc::new(ChromeBrowserTool::tabs(bridge)),
            None,
            None,
            vec![],
        )
    }

    #[tokio::test]
    async fn session_runner_plain_input_submits_formation_and_checks_goal() {
        let brain_url = spawn_runner_brain_mock().await;
        let bot = build_runner_bot_with_brain(brain_url).await;
        let (mut sess_runner, _rx) = build_session_runner(&bot).await;
        // Install a goal so the post-completion goal supervision path runs.
        *sess_runner.session.goal.write() =
            Some(crate::engine::goal::GoalState::new("ship it".to_string()));
        let mut snapshot = HashMap::new();

        // A plain prompt produces chat history, so formation is submitted to the
        // brain mock and the goal supervisor evaluation runs.
        let result = sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "make progress".to_string(),
                })],
                &mut snapshot,
            )
            .await;
        assert!(result.is_ok());
    }

    async fn build_runner_bot() -> AndaBot {
        let object_store: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        let db = Arc::new(
            AndaDB::connect(
                object_store,
                DBConfig {
                    name: "runner_test".to_string(),
                    description: "runner test".to_string(),
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
        );
        // Dead-proxy brain client: formation submission fails fast (the stop
        // path tolerates the error) without needing a live brain.
        let http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap();
        let brain_client = brain::Client::new(
            "http://127.0.0.1:1/v1/anda_bot".to_string(),
            Some("t".to_string()),
        )
        .with_http_client(http);
        let conversations = Conversations::connect(db.clone(), "bot".to_string())
            .await
            .unwrap();
        let conversations_tool =
            Arc::new(ConversationsTool::new(conversations, "/tmp".to_string()));
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let skills = SkillLibrary::for_test(std::env::temp_dir().join("runner_skills_home"));
        let bridge = Arc::new(BrowserBridge::new());

        AndaBot::new(
            brain_client,
            std::env::temp_dir(),
            conversations_tool,
            resource_store,
            vec![],
            vec![],
            skills,
            Arc::new(ChromeBrowserTool::tabs(bridge)),
            None,
            None,
            vec![],
        )
    }

    fn build_session() -> (Arc<Session>, tokio::sync::mpsc::Receiver<ConversationInput>) {
        let (sender, rx) = tokio::sync::mpsc::channel(8);
        let session = Arc::new(Session {
            id: Xid::new(),
            caller: "caller".to_string(),
            workspace: "/tmp".to_string(),
            source_key: "test".to_string(),
            conversation_id: AtomicU64::new(1),
            sender,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            background_progress_outputs: Arc::new(RwLock::new(HashMap::new())),
            goal: Arc::new(RwLock::new(None)),
            request_meta: SessionRequestMeta::new(RequestMeta::default()),
            completion_hooks: Arc::new(Vec::new()),
            submit_formation_at: AtomicU64::new(0),
            formation_backoff_until: AtomicU64::new(0),
            goal_check_backoff_until: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(0)),
            finish_when_idle: AtomicBool::new(false),
            runner_idle: AtomicBool::new(false),
            formation_context: Some(InputContext {
                counterparty: Some("caller".to_string()),
                agent: Some(AndaBot::NAME.to_string()),
                source: Some("test".to_string()),
                topic: None,
            }),
        });
        (session, rx)
    }

    fn mock_runner_ctx() -> AgentCtx {
        EngineBuilder::new()
            .with_model(Model::mock_implemented())
            .mock_ctx()
    }

    fn input(command: PromptCommand) -> ConversationInput {
        ConversationInput {
            command,
            resources: vec![],
            extra: serde_json::Map::new(),
            usage: Usage::default(),
        }
    }

    async fn build_session_runner(
        bot: &AndaBot,
    ) -> (
        SessionRunner,
        tokio::sync::mpsc::Receiver<ConversationInput>,
    ) {
        let ctx = mock_runner_ctx();
        build_session_runner_with_ctx(bot, ctx).await
    }

    async fn build_session_runner_with_ctx(
        bot: &AndaBot,
        ctx: AgentCtx,
    ) -> (
        SessionRunner,
        tokio::sync::mpsc::Receiver<ConversationInput>,
    ) {
        let (session, rx) = build_session();
        let req = CompletionRequest::default();
        let runner = ctx.clone().completion_iter(req.clone(), vec![]).unbound();
        let sess_runner = SessionRunner {
            ctx,
            assistant: bot.clone(),
            session,
            conversation: Conversation {
                _id: 1,
                ..Default::default()
            },
            runner,
            first_round: true,
            extra_user_context: None,
            last_extra_user_context: None,
        };
        (sess_runner, rx)
    }

    async fn persist_runner_conversation(sess_runner: &mut SessionRunner) -> u64 {
        let mut conversation = sess_runner.conversation.clone();
        conversation._id = 0;
        let parent_id = sess_runner
            .assistant
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await
            .unwrap();
        sess_runner.conversation._id = parent_id;
        sess_runner
            .session
            .conversation_id
            .store(parent_id, Ordering::SeqCst);
        parent_id
    }

    #[derive(Clone, Debug)]
    struct RecordingUsageCompleter {
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
        usage_input_tokens: u64,
    }

    impl CompletionFeaturesDyn for RecordingUsageCompleter {
        fn model_name(&self) -> String {
            "recording-usage".to_string()
        }

        fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            self.requests.lock().push(req.clone());
            let is_compaction = request_text(&req).trim() == COMPACTION_PROMPT.trim();
            let content = if is_compaction {
                "compacted handoff"
            } else {
                "normal output"
            };
            let mut chat_history = pending_request_messages(&req, 42);
            chat_history.push(Message {
                role: "assistant".to_string(),
                content: vec![content.to_string().into()],
                ..Default::default()
            });

            Box::pin(futures::future::ready(Ok(AgentOutput {
                content: content.to_string(),
                chat_history,
                usage: Usage {
                    input_tokens: self.usage_input_tokens,
                    output_tokens: 10,
                    cached_tokens: 0,
                    requests: 1,
                },
                ..Default::default()
            })))
        }
    }

    fn recording_usage_ctx(requests: Arc<Mutex<Vec<CompletionRequest>>>) -> AgentCtx {
        recording_usage_ctx_with_input_tokens(requests, 100_000)
    }

    fn recording_usage_ctx_with_input_tokens(
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
        usage_input_tokens: u64,
    ) -> AgentCtx {
        let mut model = Model::new(Arc::new(RecordingUsageCompleter {
            requests,
            usage_input_tokens,
        }));
        model.context_window = 1_000;
        EngineBuilder::new().with_model(model).mock_ctx()
    }

    #[derive(Clone, Debug)]
    struct ContextLengthErrorCompleter;

    impl CompletionFeaturesDyn for ContextLengthErrorCompleter {
        fn model_name(&self) -> String {
            "context-length-error".to_string()
        }

        fn completion(&self, _req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            Box::pin(futures::future::ready(Err(
                "{\"code\":\"context_length_exceeded\",\"message\":\"Your input exceeds the context window of this model. Please adjust your input and try again.\"}".into(),
            )))
        }
    }

    fn context_length_error_ctx() -> AgentCtx {
        EngineBuilder::new()
            .with_model(Model::new(Arc::new(ContextLengthErrorCompleter)))
            .mock_ctx()
    }

    // Normal turns overflow the window; the smaller compaction request (sent
    // after the runner is rebuilt) succeeds. This models a real context-length
    // spike where re-sending the offending request would overflow again but
    // compacting the committed history fits.
    #[derive(Clone, Debug)]
    struct ContextLengthThenCompactCompleter {
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    impl CompletionFeaturesDyn for ContextLengthThenCompactCompleter {
        fn model_name(&self) -> String {
            "context-length-then-compact".to_string()
        }

        fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            self.requests.lock().push(req.clone());
            if request_text(&req).trim() == COMPACTION_PROMPT.trim() {
                let mut chat_history = pending_request_messages(&req, 42);
                chat_history.push(Message {
                    role: "assistant".to_string(),
                    content: vec!["compacted handoff".to_string().into()],
                    ..Default::default()
                });
                return Box::pin(futures::future::ready(Ok(AgentOutput {
                    content: "compacted handoff".to_string(),
                    chat_history,
                    usage: Usage {
                        input_tokens: 10,
                        output_tokens: 10,
                        cached_tokens: 0,
                        requests: 1,
                    },
                    ..Default::default()
                })));
            }

            Box::pin(futures::future::ready(Err(
                "{\"code\":\"context_length_exceeded\",\"message\":\"Your input exceeds the context window of this model.\"}".into(),
            )))
        }
    }

    fn context_length_then_compact_ctx(requests: Arc<Mutex<Vec<CompletionRequest>>>) -> AgentCtx {
        EngineBuilder::new()
            .with_model(Model::new(Arc::new(ContextLengthThenCompactCompleter {
                requests,
            })))
            .mock_ctx()
    }

    #[derive(Clone, Debug)]
    struct GenericModelErrorCompleter;

    impl CompletionFeaturesDyn for GenericModelErrorCompleter {
        fn model_name(&self) -> String {
            "generic-model-error".to_string()
        }

        fn completion(&self, _req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            Box::pin(futures::future::ready(Err("model failed".into())))
        }
    }

    fn generic_model_error_ctx() -> AgentCtx {
        EngineBuilder::new()
            .with_model(Model::new(Arc::new(GenericModelErrorCompleter)))
            .mock_ctx()
    }

    fn request_text(req: &CompletionRequest) -> String {
        let mut text = Vec::new();
        if !req.prompt.is_empty() {
            text.push(req.prompt.clone());
        }
        text.extend(req.content.iter().filter_map(|part| match part {
            ContentPart::Text { text } | ContentPart::Reasoning { text } => Some(text.clone()),
            _ => None,
        }));
        text.join("\n\n")
    }

    #[tokio::test]
    async fn session_runner_stop_input_idles_conversation() {
        let bot = build_runner_bot().await;
        let (mut sess_runner, _rx) = build_session_runner(&bot).await;
        let mut snapshot = HashMap::new();

        let cont = sess_runner
            .run(
                vec![input(PromptCommand::Stop {
                    prompt: "/stop please".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();
        assert!(cont);
        assert_eq!(sess_runner.conversation.status, ConversationStatus::Idle);
    }

    #[tokio::test]
    async fn session_runner_cancel_input_marks_cancelled() {
        let bot = build_runner_bot().await;
        let (mut sess_runner, _rx) = build_session_runner(&bot).await;
        let mut snapshot = HashMap::new();

        let cont = sess_runner
            .run(
                vec![input(PromptCommand::Cancel {
                    prompt: "/cancel".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();
        assert!(!cont);
        assert_eq!(
            sess_runner.conversation.status,
            ConversationStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn session_runner_ping_runs_completion_round() {
        let bot = build_runner_bot().await;
        let (mut sess_runner, _rx) = build_session_runner(&bot).await;
        let mut snapshot = HashMap::new();

        // A ping falls through to a completion round driven by the mock model.
        let result = sess_runner
            .run(vec![input(PromptCommand::Ping)], &mut snapshot)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn session_runner_plain_and_goal_inputs_drive_completion() {
        let bot = build_runner_bot().await;
        let (mut sess_runner, _rx) = build_session_runner(&bot).await;
        let mut snapshot = HashMap::new();

        // Plain + Goal inputs append follow-up content, set the session goal,
        // and run a completion round with the mock model.
        let result = sess_runner
            .run(
                vec![
                    input(PromptCommand::Plain {
                        prompt: "do a thing".to_string(),
                    }),
                    input(PromptCommand::Goal {
                        prompt: "finish the task".to_string(),
                    }),
                ],
                &mut snapshot,
            )
            .await;
        assert!(result.is_ok());
        // The goal command installs an objective on the session.
        assert!(sess_runner.session.goal.read().is_some());
    }

    #[tokio::test]
    async fn session_runner_compacts_pending_request_before_next_model_call() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx(requests.clone());
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let mut snapshot = HashMap::new();

        sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "seed enough usage".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();
        assert_eq!(requests.lock().len(), 1);

        sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "pending user message after threshold".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 3);
        assert_eq!(request_text(&recorded[1]).trim(), COMPACTION_PROMPT.trim());
        let continuation_history = recorded[2]
            .chat_history
            .iter()
            .filter_map(Message::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(continuation_history.contains("compacted handoff"));
        assert!(request_text(&recorded[2]).contains("pending user message after threshold"));

        let conversation_text = sess_runner
            .conversation
            .messages
            .iter()
            .filter_map(|message| serde_json::from_value::<Message>(message.clone()).ok())
            .filter_map(|message| message.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(conversation_text.contains("compacted handoff"));
        assert!(conversation_text.contains("pending user message after threshold"));
    }

    #[tokio::test]
    async fn session_runner_ignores_background_usage_for_compaction() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx_with_input_tokens(requests.clone(), 1);
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let mut snapshot = HashMap::new();

        let mut background_input = input(PromptCommand::Plain {
            prompt: "follow up after background usage".to_string(),
        });
        background_input.usage = Usage {
            input_tokens: 100_000,
            output_tokens: 0,
            cached_tokens: 0,
            requests: 1,
        };

        sess_runner
            .run(vec![background_input], &mut snapshot)
            .await
            .unwrap();

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 1);
        assert_eq!(
            request_text(&recorded[0]),
            "follow up after background usage"
        );
    }

    #[tokio::test]
    async fn session_runner_compacts_oversized_follow_up_before_queueing() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx_with_input_tokens(requests.clone(), 1);
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let mut snapshot = HashMap::new();
        let oversized_prompt = "x".repeat(4_000);

        sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: oversized_prompt.clone(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 2);
        assert_eq!(request_text(&recorded[0]).trim(), COMPACTION_PROMPT.trim());
        let continuation_history = recorded[1]
            .chat_history
            .iter()
            .filter_map(Message::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(continuation_history.contains("compacted handoff"));
        assert!(request_text(&recorded[1]).contains(&oversized_prompt));
    }

    #[tokio::test]
    async fn session_runner_compacts_oversized_input_batch_before_queueing() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        // Per-completion usage stays tiny, so only the batch-size estimate can
        // trigger compaction. Each input is well under the 800-token threshold;
        // batched together (as background results are) they exceed it. This is
        // the case the per-input check missed: queueing the first follow-up made
        // the runner report not-idle, so the rest bypassed the size check.
        let ctx = recording_usage_ctx_with_input_tokens(requests.clone(), 1);
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let mut snapshot = HashMap::new();
        let chunk = "x".repeat(1_600); // ~400 tokens each, ~1200 for the batch

        sess_runner
            .run(
                vec![
                    input(PromptCommand::Plain {
                        prompt: chunk.clone(),
                    }),
                    input(PromptCommand::Plain {
                        prompt: chunk.clone(),
                    }),
                    input(PromptCommand::Plain {
                        prompt: chunk.clone(),
                    }),
                ],
                &mut snapshot,
            )
            .await
            .unwrap();

        let recorded = requests.lock();
        // Compaction runs once up front, then the whole batch is queued on top
        // of the compacted handoff in a single follow-up request.
        assert_eq!(recorded.len(), 2);
        assert_eq!(request_text(&recorded[0]).trim(), COMPACTION_PROMPT.trim());
        let continuation_history = recorded[1]
            .chat_history
            .iter()
            .filter_map(Message::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(continuation_history.contains("compacted handoff"));
        assert_eq!(
            request_text(&recorded[1]).matches(chunk.as_str()).count(),
            3
        );
    }

    #[tokio::test]
    async fn session_runner_idle_compaction_without_pending_work_continues_in_child() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx(requests.clone());
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let parent_id = persist_runner_conversation(&mut sess_runner).await;
        let mut snapshot = HashMap::new();

        sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "seed high context usage".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        let cont = sess_runner.run(vec![], &mut snapshot).await.unwrap();

        assert!(cont);
        assert_ne!(sess_runner.conversation._id, parent_id);
        assert_eq!(sess_runner.conversation.ancestors, Some(vec![parent_id]));
        assert_eq!(
            sess_runner.session.conversation_id.load(Ordering::SeqCst),
            sess_runner.conversation._id
        );

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 3);
        assert_eq!(request_text(&recorded[1]).trim(), COMPACTION_PROMPT.trim());
        let continuation_history = recorded[2]
            .chat_history
            .iter()
            .filter_map(Message::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(continuation_history.contains("compacted handoff"));
        assert!(
            request_text(&recorded[2])
                .contains("Continue the active work from the compaction handoff")
        );
    }

    #[tokio::test]
    async fn session_runner_idle_compaction_with_background_task_continues_in_child() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx(requests.clone());
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let parent_id = persist_runner_conversation(&mut sess_runner).await;
        sess_runner.session.background_tasks.write().insert(
            "subagent-session".to_string(),
            BackgroundTaskInfo {
                agent_name: "runner-test".to_string(),
                tool_name: None,
                progress_message: None,
                stopped: false,
            },
        );
        let mut snapshot = HashMap::new();

        sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "seed high context usage".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        let cont = sess_runner.run(vec![], &mut snapshot).await.unwrap();

        assert!(cont);
        assert_ne!(sess_runner.conversation._id, parent_id);
        assert_eq!(sess_runner.conversation.ancestors, Some(vec![parent_id]));
        assert_eq!(
            sess_runner.session.conversation_id.load(Ordering::SeqCst),
            sess_runner.conversation._id
        );

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 3);
        assert_eq!(request_text(&recorded[1]).trim(), COMPACTION_PROMPT.trim());
        let continuation_history = recorded[2]
            .chat_history
            .iter()
            .filter_map(Message::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(continuation_history.contains("compacted handoff"));
    }

    #[tokio::test]
    async fn session_runner_persists_tool_usage_before_compaction_handoff() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx(requests);
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let mut snapshot = HashMap::new();

        sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "seed high context usage".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        let tool_usage = Usage {
            input_tokens: 7,
            output_tokens: 11,
            cached_tokens: 3,
            requests: 2,
        };
        sess_runner.runner.accumulate_tools_usage(&HashMap::from([(
            "browser_open".to_string(),
            tool_usage.clone(),
        )]));

        sess_runner.run(vec![], &mut snapshot).await.unwrap();

        let persisted = bot.inner.conversations.tools_usage();
        let persisted_usage = persisted
            .get("browser_open")
            .expect("tool usage should survive compaction handoff");
        assert_eq!(persisted_usage.input_tokens, tool_usage.input_tokens);
        assert_eq!(persisted_usage.output_tokens, tool_usage.output_tokens);
        assert_eq!(persisted_usage.cached_tokens, tool_usage.cached_tokens);
        assert_eq!(persisted_usage.requests, tool_usage.requests);
    }

    #[tokio::test]
    async fn session_runner_delivers_steer_through_steering_channel() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx_with_input_tokens(requests.clone(), 1);
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let mut snapshot = HashMap::new();

        // A /steer input is routed to the runner's steering channel (not the follow-up batch), so
        // its prompt must still reach the model rather than being dropped by the batch split.
        sess_runner
            .run(
                vec![input(PromptCommand::Steer {
                    prompt: "redirect the approach".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 1);
        assert_eq!(request_text(&recorded[0]), "redirect the approach");
    }

    #[tokio::test]
    async fn session_runner_recovers_from_context_length_error_via_compaction() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = context_length_then_compact_ctx(requests.clone());
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        sess_runner.session.background_tasks.write().insert(
            "task-1".to_string(),
            BackgroundTaskInfo {
                agent_name: "runner-test".to_string(),
                tool_name: None,
                progress_message: None,
                stopped: false,
            },
        );
        let mut snapshot = HashMap::new();

        let cont = sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "trigger context length".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        // The session keeps running on a compacted context instead of failing,
        // and its background tasks are left intact.
        assert!(cont);
        assert_ne!(sess_runner.conversation.status, ConversationStatus::Failed);
        assert!(sess_runner.conversation.failed_reason.is_none());
        assert!(sess_runner.session.has_running_background_tasks());

        let recorded = requests.lock();
        assert_eq!(recorded.len(), 2);
        assert_eq!(request_text(&recorded[1]).trim(), COMPACTION_PROMPT.trim());
    }

    #[tokio::test]
    async fn session_runner_marks_context_length_error_failed_with_background_tasks() {
        let bot = build_runner_bot().await;
        let ctx = context_length_error_ctx();
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        sess_runner.session.background_tasks.write().insert(
            "task-1".to_string(),
            BackgroundTaskInfo {
                agent_name: "runner-test".to_string(),
                tool_name: None,
                progress_message: None,
                stopped: false,
            },
        );
        let mut snapshot = HashMap::new();

        let cont = sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "trigger context length".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        assert!(!cont);
        assert_eq!(sess_runner.conversation.status, ConversationStatus::Failed);
        assert!(
            sess_runner
                .conversation
                .failed_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("context_length_exceeded"))
        );
        assert!(!sess_runner.session.has_running_background_tasks());
    }

    #[tokio::test]
    async fn session_runner_marks_model_error_failed_with_background_tasks() {
        let bot = build_runner_bot().await;
        let ctx = generic_model_error_ctx();
        let (mut sess_runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        sess_runner.session.background_tasks.write().insert(
            "task-1".to_string(),
            BackgroundTaskInfo {
                agent_name: "runner-test".to_string(),
                tool_name: None,
                progress_message: None,
                stopped: false,
            },
        );
        let mut snapshot = HashMap::new();

        let cont = sess_runner
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "trigger model error".to_string(),
                })],
                &mut snapshot,
            )
            .await
            .unwrap();

        assert!(!cont);
        assert_eq!(sess_runner.conversation.status, ConversationStatus::Failed);
        assert_eq!(
            sess_runner.conversation.failed_reason.as_deref(),
            Some("model failed")
        );
        assert!(!sess_runner.session.has_running_background_tasks());
    }

    #[tokio::test]
    async fn session_runner_skill_and_steer_inputs_are_handled() {
        let bot = build_runner_bot().await;
        let (mut sess_runner, _rx) = build_session_runner(&bot).await;
        let mut snapshot = HashMap::new();

        let result = sess_runner
            .run(
                vec![
                    input(PromptCommand::Skill {
                        skill: "coder".to_string(),
                        prompt: "build it".to_string(),
                    }),
                    input(PromptCommand::Steer {
                        prompt: "actually do this instead".to_string(),
                    }),
                ],
                &mut snapshot,
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn session_runner_persist_helpers_run() {
        let bot = build_runner_bot().await;
        let (sess_runner, _rx) = build_session_runner(&bot).await;
        // The persistence helpers operate on the in-memory conversation store.
        sess_runner.persist_conversation_state().await;
        let mut snapshot = HashMap::new();
        sess_runner
            .persist_tools_usage_snapshot(&mut snapshot)
            .await;
    }

    #[test]
    fn session_runner_stop_policy_keeps_background_work() {
        assert!(!should_continue_session_runner_after_stop(
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
    fn control_command_reason_strips_known_command_prefix() {
        assert_eq!(
            control_command_reason("/stop because it is wrong", "stop"),
            "because it is wrong"
        );
        assert_eq!(control_command_reason("/STOP", "stop"), "");
        assert_eq!(
            control_command_reason("/cancel because it is wrong", "stop"),
            "/cancel because it is wrong"
        );
    }

    #[test]
    fn cancel_reason_defaults_when_reason_is_empty() {
        assert_eq!(
            cancel_reason("/cancel because it is wrong"),
            "because it is wrong"
        );
        assert_eq!(cancel_reason("/cancel"), "conversation cancelled");
    }

    #[test]
    fn task_stopped_message_reports_idle_state() {
        assert_eq!(
            task_stopped_message(""),
            "Current task stopped. The conversation is idle and ready for the next message."
        );
        assert_eq!(
            task_stopped_message("wrong branch"),
            "Current task stopped: wrong branch"
        );
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
