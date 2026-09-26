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
    future::Future,
    sync::{Arc, atomic::Ordering},
};

use super::{
    AndaBot,
    session::{ConversationInput, Session, SessionControl},
};
#[cfg(test)]
use crate::engine::SkillLibrary;
use crate::engine::{
    ActionEvent, CompletionHook, action_id_from_message, action_id_from_message_value,
    apply_action_resolution_to_chat_message, apply_action_resolution_to_message,
    conversation::SourceState,
    goal::{self},
    is_action_message, is_action_message_value, multimodal,
    prompt::{PromptCommand, skill_command_directive},
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
        mut conversation: Conversation,
        mut rx: tokio::sync::mpsc::Receiver<ConversationInput>,
        mut action_rx: tokio::sync::mpsc::Receiver<ActionEvent>,
        extra_user_context: Option<Message>,
        cron_receipt: Option<crate::cron::AgentReceipt>,
    ) {
        let assistant = self.clone();
        tokio::spawn(async move {
            let mut cron_receipts = crate::cron::AgentReceipts::default();
            cron_receipts.push(cron_receipt);
            let preparation = async {
                let (resources, usage) =
                    multimodal::understand_media_resources(&ctx, resources).await;
                let resources = assistant
                    .persist_resources_for_message(ctx.caller(), resources)
                    .await?;
                Ok::<_, BoxError>((resources, usage))
            };
            let prepared = drive_session_operation(
                &assistant,
                &mut conversation,
                &mut action_rx,
                &session.control,
                preparation,
            )
            .await;
            let (resources, media_usage, initial_events) = match prepared {
                Ok((Some(Ok((resources, usage))), events)) => (resources, usage, events),
                Ok((None, events)) => (Vec::new(), Usage::default(), events),
                Ok((Some(Err(err)), _)) | Err(err) => {
                    session.stop_background_tasks();
                    for event in session.actions.cancel_pending().await {
                        apply_action_event_to_conversation(&mut conversation, event);
                    }
                    conversation.status = ConversationStatus::Failed;
                    conversation.failed_reason =
                        Some(format!("Attachment preparation failed: {err}"));
                    cron_receipts.fail(conversation.failed_reason.as_deref().unwrap());
                    conversation.updated_at = unix_ms();
                    if let Err(error) = assistant.persist_conversation_state(&conversation).await {
                        log::error!("Failed to save attachment failure: {error}");
                    }
                    assistant.detach_session(&session.id);
                    return;
                }
            };
            req.content.extend(
                resources
                    .into_iter()
                    .map(|res| ContentPart::any_from("Resource", res)),
            );
            let mut runner = ctx.clone().completion_iter(req, vec![]).unbound();
            assistant.inner.apply_merge_discovered_tools(&mut runner);
            runner.accumulate(&media_usage);
            if !reserve_chat_history.is_empty() {
                runner = runner.reserve_chat_history(reserve_chat_history);
            }

            let mut tools_usage_snapshot: HashMap<String, Usage> = HashMap::new();
            let mut sess_runner = SessionRunner {
                cron_receipts,
                ctx,
                assistant: assistant.clone(),
                session: session.clone(),
                conversation,
                action_rx,
                runner,
                extra_user_context: extra_user_context.clone(),
                last_extra_user_context: extra_user_context,
                wait_for_input: false,
            };
            for event in initial_events {
                sess_runner.apply_action_event_to_runner(event);
            }
            let mut pending_inputs = Vec::new();

            loop {
                session.control.reset();
                let mut inputs = std::mem::take(&mut pending_inputs);

                while let Ok(input) = rx.try_recv() {
                    inputs.push(input);
                }

                // A stop discards earlier work, but messages sent after it belong
                // to the next task and must survive the batch boundary.
                if let Some(index) = inputs.iter().position(|input| {
                    matches!(
                        input.command,
                        PromptCommand::Stop { .. } | PromptCommand::Cancel { .. }
                    )
                }) {
                    pending_inputs = inputs.split_off(index + 1);
                }
                match sess_runner.run(inputs, &mut tools_usage_snapshot).await {
                    Ok(continue_active) => {
                        if continue_active
                            && sess_runner.wait_for_input
                            && pending_inputs.is_empty()
                        {
                            // Idle tick: block on the next input so a queued
                            // message is picked up immediately, waking at
                            // least once per second for the idle bookkeeping
                            // in run(). A closed channel falls back to a
                            // plain sleep to keep the loop paced.
                            match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
                                .await
                            {
                                Ok(Some(input)) => {
                                    session.runner_idle.store(false, Ordering::SeqCst);
                                    pending_inputs.push(input);
                                }
                                Ok(None) => {
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                }
                                Err(_) => {}
                            }
                        }
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
                        sess_runner.cron_receipts.fail(&err.to_string());
                        // Best-effort fallback: if the error escaped before the
                        // conversation reached a terminal state, persist it as
                        // Failed so it does not stay `Working` in the DB after
                        // the session is dropped from the in-memory table.
                        if sess_runner.conversation.status != ConversationStatus::Cancelled {
                            sess_runner
                                .mark_conversation_failed(format!("Session runner failed: {err}"))
                                .await;
                        }
                        break;
                    }
                }
            }

            assistant.detach_session(&session.id);
        });
    }
}

struct SessionRunner {
    cron_receipts: crate::cron::AgentReceipts,
    ctx: AgentCtx,
    assistant: AndaBot,
    session: Arc<Session>,
    conversation: Conversation,
    action_rx: tokio::sync::mpsc::Receiver<ActionEvent>,
    runner: CompletionRunner,
    extra_user_context: Option<Message>,
    last_extra_user_context: Option<Message>,
    // Set by run() on an idle tick: the outer loop should wait for new input
    // (bounded by the 1s idle poll interval) instead of spinning.
    wait_for_input: bool,
}

impl SessionRunner {
    async fn persist_conversation_state(&self) -> Result<(), BoxError> {
        self.assistant
            .persist_conversation_state(&self.conversation)
            .await
    }

    async fn drain_action_events(&mut self) -> Result<(), BoxError> {
        let mut changed = false;
        while let Ok(event) = self.action_rx.try_recv() {
            changed |= self.apply_action_event(event);
        }
        if changed {
            self.persist_conversation_state().await?;
        }
        Ok(())
    }

    async fn next_with_action_events(
        &mut self,
    ) -> Result<
        (
            Option<Result<Option<anda_core::AgentOutput>, BoxError>>,
            Vec<ActionEvent>,
        ),
        BoxError,
    > {
        drive_session_operation(
            &self.assistant,
            &mut self.conversation,
            &mut self.action_rx,
            &self.session.control,
            self.runner.next(),
        )
        .await
    }

    fn collect_artifacts(&mut self) -> Vec<Resource> {
        let mut added = Vec::new();
        if let Some(artifacts) = self
            .ctx
            .base
            .get_state::<crate::engine::resources::SessionArtifacts>()
        {
            for artifact in artifacts.take() {
                if !self
                    .conversation
                    .artifacts
                    .iter()
                    .any(|saved| saved._id == artifact._id)
                {
                    self.conversation.artifacts.push(artifact.clone());
                    added.push(artifact);
                }
            }
        }
        added
    }

    fn apply_action_event(&mut self, event: ActionEvent) -> bool {
        self.apply_action_event_to_runner(event.clone());
        apply_action_event_to_conversation(&mut self.conversation, event)
    }

    fn apply_action_event_to_runner(&mut self, event: ActionEvent) -> bool {
        apply_action_event_to_history(&mut self.runner, event)
    }

    fn replace_conversation_messages_from_chat_history(&mut self, chat_history: Vec<Message>) {
        self.conversation.messages.clear();
        self.conversation.append_messages(chat_history);
    }

    async fn persist_tools_usage_snapshot(
        &self,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) {
        self.persist_tools_usage(self.runner.tools_usage(), tools_usage_snapshot)
            .await;
    }

    async fn persist_tools_usage(
        &self,
        current: &HashMap<String, Usage>,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) {
        let current_tools_usage = current.clone();
        let tools_usage_delta =
            compute_tools_usage_delta(&current_tools_usage, tools_usage_snapshot);
        if let Err(err) = self
            .assistant
            .inner
            .conversations
            .accumulate_tool_usage(tools_usage_delta)
            .await
        {
            log::error!("Failed to accumulate_tool_usage: {:?}", err);
        } else {
            *tools_usage_snapshot = current_tools_usage;
        }
    }

    async fn stop_current_task(
        &mut self,
        reason: String,
        now_ms: u64,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) -> Result<(), BoxError> {
        *self.session.goal.write() = None;
        self.session
            .goal_check_backoff_until
            .store(0, Ordering::SeqCst);
        self.session.stop_background_tasks();
        self.drain_action_events().await?;
        for event in self.session.actions.cancel_pending().await {
            self.apply_action_event(event);
        }
        self.collect_artifacts();
        self.persist_tools_usage_snapshot(tools_usage_snapshot)
            .await;

        let content = task_stopped_message(&reason);
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

        self.replace_conversation_messages_from_chat_history(output.chat_history);
        self.conversation.failed_reason = None;
        self.conversation.status = ConversationStatus::Idle;
        self.conversation.usage = output.usage;
        self.conversation.updated_at = now_ms;
        self.persist_conversation_state().await
    }

    /// Marks the current conversation `Failed` with a reason and persists it,
    /// so the session never stays `Working` in the DB after its runner exits.
    async fn mark_conversation_failed(&mut self, reason: String) {
        *self.session.goal.write() = None;
        self.session.stop_background_tasks();
        while let Ok(event) = self.action_rx.try_recv() {
            self.apply_action_event(event);
        }
        for event in self.session.actions.cancel_pending().await {
            self.apply_action_event(event);
        }
        self.collect_artifacts();
        self.conversation.failed_reason = Some(reason);
        self.conversation.status = ConversationStatus::Failed;
        self.conversation.updated_at = unix_ms();
        if let Err(error) = self.persist_conversation_state().await {
            log::error!("Failed to save conversation failure: {error}");
        }
    }

    async fn compact(
        &mut self,
        continuation_prompt: Option<String>,
        tools_usage_snapshot: &mut HashMap<String, Usage>,
    ) -> Result<bool, BoxError> {
        self.persist_tools_usage_snapshot(tools_usage_snapshot)
            .await;

        let tools = self.runner.req().tools.clone();
        let (handoff, events) = drive_session_operation(
            &self.assistant,
            &mut self.conversation,
            &mut self.action_rx,
            &self.session.control,
            self.runner.handoff(Some(COMPACTION_PROMPT.to_string())),
        )
        .await?;
        let Some(handoff) = handoff else {
            // handoff temporarily removes schemas and switches to bounded mode.
            // A user stop owns the interrupt; restore a reusable runner first.
            self.runner.set_tools(tools);
            self.runner.set_unbound(true);
            for event in events {
                self.apply_action_event(event);
            }
            return Ok(true);
        };
        let (mut runner, mut output) = match handoff {
            Ok(result) => result,
            Err(err) => {
                self.mark_conversation_failed(format!("Compaction failed: {err}"))
                    .await;
                return Ok(false);
            }
        };
        for event in events {
            apply_action_event_to_messages(&mut output.chat_history, event);
        }

        // 如果目标还没有完成，也需要关闭本轮 conversation （conversation 数据大小有限，不应该超过 10MB），为 session 创建新的 conversation 和 runner 继续后续的交互
        // 同一个 session 可以逐步产生不限数量的 conversation 对话，可支持超长程推理。
        // 前一轮压缩总结的内容作为新 conversation 的第一条消息，继续后续的交互
        let now_ms = unix_ms();
        let mut ancestors = self.conversation.ancestors.clone().unwrap_or_default();
        ancestors.push(self.conversation._id);
        let mut child_conversation = Conversation {
            user: self.conversation.user,
            thread: Some(self.session.id),
            ancestors: Some(ancestors),
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some({
                let mut extra = self.session.request_meta.get().extra;
                extra.remove(crate::util::request_meta::keys::CONVERSATION);
                self.session.memory_policy.persist(&mut extra);
                extra.insert(
                    "memory_source_parents".into(),
                    json!(
                        self.ctx
                            .base
                            .get_state::<super::memory_policy::InheritedMemorySources>()
                            .unwrap_or_default()
                            .0
                    ),
                );
                json!(extra)
            }),
            ..Default::default()
        };

        // DB errors here must not propagate to the outer runner loop: that
        // would drop the session while the conversation stays `Working` in
        // the DB with no failed_reason, making it unrecoverable for CLI/API
        // sources. Fail the conversation explicitly instead, matching the
        // handoff-failure branch above.
        let child_id = match self
            .assistant
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&child_conversation))
            .await
        {
            Ok(child_id) => child_id,
            Err(err) => {
                self.mark_conversation_failed(format!("Compaction failed: {err}"))
                    .await;
                return Ok(false);
            }
        };
        child_conversation._id = child_id;

        self.submit_pending_formation(&output.chat_history, now_ms)
            .await;
        let artifacts = match self
            .assistant
            .persist_resources_for_message(&self.conversation.user, output.artifacts)
            .await
        {
            Ok(artifacts) => artifacts,
            Err(err) => {
                self.mark_conversation_failed(format!("Compaction failed: {err}"))
                    .await;
                return Ok(false);
            }
        };

        self.replace_conversation_messages_from_chat_history(output.chat_history);
        self.conversation.status = ConversationStatus::Completed;
        self.conversation.usage = output.usage;
        self.collect_artifacts();
        for artifact in artifacts {
            if !self
                .conversation
                .artifacts
                .iter()
                .any(|saved| saved._id == artifact._id)
            {
                self.conversation.artifacts.push(artifact);
            }
        }
        self.conversation.updated_at = now_ms;
        // 把新的 conversation 设为原 conversation 的 child，延续同一个 session，客户端可以读取连续的 conversation 记录来展示给用户
        self.conversation.child = Some(child_id);
        self.persist_conversation_state().await?;

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
        let current_meta = self.session.request_meta.get();
        self.session
            .request_meta
            .set(super::meta::request_meta_for_conversation(
                &current_meta,
                child_id,
            ));
        let session_id = self.session.id.to_string();
        let mut source = crate::brain::product::source_identity(
            &self.session.caller,
            child_id,
            Some(&session_id),
        );
        source.parents.extend(
            self.ctx
                .base
                .get_state::<super::memory_policy::InheritedMemorySources>()
                .unwrap_or_default()
                .0,
        );
        source.parents.sort();
        source.parents.dedup();
        self.ctx.base.set_state(source.clone());
        runner.ctx().base.set_state(source);
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
        if !self.session.memory_policy.may_write() {
            self.session
                .submit_formation_at
                .store(chat_history.len() as u64, Ordering::SeqCst);
            return;
        }
        if now_ms < self.session.formation_backoff_until.load(Ordering::SeqCst) {
            return;
        }

        if self.session.submit_formation_at.load(Ordering::SeqCst) as usize >= chat_history.len() {
            return;
        }

        // Persist the exact original message indices before filtering/pruning
        // the Formation input. Update messages only: inbound queues are owned
        // by the conversation API and must not be overwritten by this snapshot.
        let messages = chat_history
            .iter()
            .map(|message| json!(message))
            .collect::<Vec<_>>();
        let persisted = async {
            let changes = std::collections::BTreeMap::from([(
                "messages".to_string(),
                anda_db::schema::Fv::array_from(
                    cbor2::cbor!(&messages)?,
                    &[anda_db::schema::Ft::Json],
                )?,
            )]);
            self.assistant
                .inner
                .conversations
                .conversations
                .update_conversation(self.conversation._id, changes)
                .await?;
            Ok::<_, BoxError>(())
        }
        .await;
        if let Err(error) = persisted {
            self.session.formation_backoff_until.store(
                now_ms.saturating_add(FORMATION_RETRY_BACKOFF_MS),
                Ordering::SeqCst,
            );
            log::error!(
                "Cannot persist Formation source conversation {}: {error}",
                self.conversation._id
            );
            return;
        }
        let mut source_messages = Vec::new();
        let mut messages = chat_history
            .iter()
            .enumerate()
            .skip(self.session.submit_formation_at.load(Ordering::SeqCst) as usize)
            .filter(|(_, msg)| !is_action_message(msg))
            .filter_map(|(index, msg)| {
                let digest = anda_cognitive_nexus::content_digest(&serde_json::json!(msg)).ok()?;
                let mut msg = msg.clone();
                let pruned = msg.prune_content();
                if msg.content.is_empty() || pruned > 0 && msg.content.len() <= 1 {
                    None
                } else {
                    source_messages.push(crate::brain::SourceMessageRef {
                        conversation: self.conversation._id.to_string(),
                        index: index.to_string(),
                        role: msg.role.clone(),
                        content_digest: digest,
                        submitted_digest: None,
                    });
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
        let meta = self.session.request_meta.get();
        use crate::util::request_meta::{keys, request_meta_extra_as};
        match self
            .assistant
            .submit_formation(
                crate::brain::FormationSubmission {
                    bot_conversation: self.conversation._id,
                    window_start: self.session.submit_formation_at.load(Ordering::SeqCst) as usize,
                    window_end: next_submit_formation_at,
                    submitted_at: now_ms,
                    observed_at: None,
                    brain_conversation: None,
                    state: crate::brain::FormationState::Pending,
                    error: None,
                    provenance: Some(crate::brain::FormationProvenance {
                        policy_revision: Some(self.session.memory_policy.revision.clone()),
                        version: 1,
                        caller: self.session.caller.clone(),
                        session: Some(self.session.id.to_string()),
                        source_identity: self
                            .ctx
                            .base
                            .get_state::<anda_brain::product::SourceIdentity>(),
                        source: self.session.source_key.clone(),
                        reply_target: request_meta_extra_as(&meta, keys::REPLY_TARGET),
                        thread: request_meta_extra_as(&meta, keys::THREAD),
                        external_user: request_meta_extra_as(&meta, keys::EXTERNAL_USER)
                            .unwrap_or(false),
                        counterparty: self
                            .session
                            .formation_context
                            .as_ref()
                            .and_then(|c| c.counterparty.clone()),
                        source_messages,
                        input_digest: None,
                    }),
                    updated_at: None,
                    failure_stage: None,
                    receipt_ref: None,
                    attempt: 0,
                },
                &messages,
                &self.session.formation_context,
                &timestamp,
            )
            .await
        {
            Ok(submission) => {
                self.session
                    .submit_formation_at
                    .store(submission.window_end as u64, Ordering::SeqCst);
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
        self.wait_for_input = false;
        if !inputs.is_empty() {
            self.session.runner_idle.store(false, Ordering::SeqCst);
            self.session.active_at.store(unix_ms(), Ordering::SeqCst);
        }

        if let Some(index) = inputs.iter().position(|input| {
            matches!(
                input.command,
                PromptCommand::Stop { .. } | PromptCommand::Cancel { .. }
            )
        }) {
            for input in &inputs[..=index] {
                self.runner.accumulate(&input.usage);
            }
            let (reason, cancelled) = match &inputs[index].command {
                PromptCommand::Stop { prompt } => (control_command_reason(prompt, "stop"), false),
                PromptCommand::Cancel { prompt } => (cancel_reason(prompt), true),
                _ => unreachable!(),
            };
            let now_ms = unix_ms();
            self.cron_receipts.fail(&reason);
            self.stop_current_task(reason.clone(), now_ms, tools_usage_snapshot)
                .await?;
            if cancelled {
                self.conversation.failed_reason = Some(reason);
                self.conversation.status = ConversationStatus::Cancelled;
                self.persist_conversation_state().await?;
                self.submit_pending_formation(self.runner.chat_history(), now_ms)
                    .await;
            }
            return Ok(!cancelled);
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
                cron_receipt,
                command,
                resources,
                mut extra,
                usage,
            } = input;
            self.cron_receipts.push(cron_receipt);
            // Session lifecycle control is not user context for the model.
            extra.remove(crate::util::request_meta::keys::FINISH_WHEN_IDLE);
            extra.remove(super::memory_policy::MODE_KEY);
            extra.remove(super::memory_policy::POLICY_KEY);
            extra.remove("memory_source_parents");

            // 累计来自于后台任务的工具使用情况
            self.runner.accumulate(&usage);

            let prepare = async {
                let (resources, usage) =
                    multimodal::understand_media_resources(&self.ctx, resources).await;
                let resources = self
                    .assistant
                    .persist_resources_for_message(self.ctx.caller(), resources)
                    .await?;
                Ok::<_, BoxError>((resources, usage))
            };
            let (prepared, events) = drive_session_operation(
                &self.assistant,
                &mut self.conversation,
                &mut self.action_rx,
                &self.session.control,
                prepare,
            )
            .await?;
            for event in events {
                self.apply_action_event_to_runner(event);
            }
            let Some(prepared) = prepared else {
                return Ok(true);
            };
            let (resources_without_blob, media_usage) = prepared?;
            self.runner.accumulate(&media_usage);
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
                PromptCommand::Stop { .. } | PromptCommand::Cancel { .. } => unreachable!(),
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
                PromptCommand::Skill { skill, prompt } => {
                    let (_, directive) = skill_command_directive(
                        self.assistant.inner.skill_library.subagent_set(),
                        &skill,
                    );
                    content.push(system_runtime_prompt("prompt command", directive).into());
                    prepend_prompt_content(&mut content, prompt);
                    follow_up_batch.append(&mut content);
                }
            }
        }

        let now_ms = unix_ms();
        if self.conversation.status != ConversationStatus::Working
            && (!follow_up_batch.is_empty() || !steer_batch.is_empty() || !self.runner.is_idle())
        {
            self.conversation.status = ConversationStatus::Working;
            self.conversation.failed_reason = None;
            self.conversation.updated_at = now_ms;
            self.persist_conversation_state().await?;
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

        if self.session.control.is_pending() {
            return Ok(true);
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

        let (next_result, mut events) = self.next_with_action_events().await?;
        while let Ok(event) = self.action_rx.try_recv() {
            events.push(event);
        }
        let Some(mut next_result) = next_result else {
            for event in events {
                self.apply_action_event(event);
            }
            return Ok(true);
        };
        // Completion hooks, history persistence and Formation submission are
        // still part of the active turn, even if the model runner just ended.
        self.session.runner_idle.store(false, Ordering::SeqCst);
        if let Ok(Some(res)) = &mut next_result {
            if self.runner.is_done() {
                // Finalization moved history into the output. Apply action events to
                // that authoritative history instead of replacing it with an empty runner.
                for event in &events {
                    apply_action_event_to_messages(&mut res.chat_history, event.clone());
                }
            } else {
                for event in &events {
                    self.apply_action_event_to_runner(event.clone());
                }
                res.chat_history = self.runner.chat_history().clone();
            }
            if let Some(trace) = self
                .runner
                .ctx()
                .base
                .get_state::<crate::brain::RecallTurn>()
            {
                trace.prepare(self.conversation._id, &res.tool_calls);
            }
        } else {
            for event in &events {
                self.apply_action_event_to_runner(event.clone());
            }
        }
        for event in events {
            apply_action_event_to_conversation(&mut self.conversation, event);
        }
        let artifacts = self.collect_artifacts();
        if let Ok(Some(output)) = &mut next_result {
            for artifact in artifacts {
                if !output
                    .artifacts
                    .iter()
                    .any(|existing| existing._id == artifact._id)
                {
                    output.artifacts.push(artifact);
                }
            }
        }
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

                // The turn produced no further output, so the runner is idle
                // here. Reclaim context-window budget by pruning completed tool
                // calls and results from the accumulated provider raw history.
                // Do this before the goal supervisor runs: if the goal queues a
                // follow-up the runner turns non-idle and the prune becomes a
                // no-op, so pruning first keeps the next request lean.
                self.runner.prune_req_raw_history();

                let maybe_goal =
                    if now_ms >= self.session.goal_check_backoff_until.load(Ordering::SeqCst) {
                        self.session.goal.write().take()
                    } else {
                        None
                    };
                let mut goal_continue_prompt: Option<String> = None;
                if let Some(mut goal) = maybe_goal {
                    let check = tokio::select! {
                        _ = self.session.control.interrupted() => { return Ok(true); }
                        result = goal.check_progress(&self.runner, &self.ctx) => result,
                    };
                    match check {
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
                    if !has_background_tasks
                        && !self.session.has_pending_inputs()
                        && self.session.goal.read().is_none()
                    {
                        self.cron_receipts.finish();
                    }
                    let idle = now_ms.saturating_sub(self.session.active_at.load(Ordering::SeqCst));
                    if !has_background_tasks && self.session.finish_when_idle.load(Ordering::SeqCst)
                    {
                        self.conversation.status = ConversationStatus::Completed;
                        self.conversation.updated_at = now_ms;
                        self.persist_conversation_state().await?;
                        return Ok(false);
                    }

                    if idle > CONVERSATION_IDLE_MS && !has_background_tasks
                        || (idle > CONVERSATION_WAIT_BACKGROUND_TASK_MS && has_background_tasks)
                    {
                        self.conversation.status = ConversationStatus::Completed;
                        self.conversation.updated_at = now_ms;
                        self.persist_conversation_state().await?;
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
                    self.persist_conversation_state().await?;
                }

                // The outer loop performs the idle wait (input-aware, up to
                // one second) so a queued message does not sit through a
                // fixed sleep before being seen.
                self.wait_for_input = true;
                return Ok(true);
            }

            Ok(Some(mut res)) => {
                let now_ms = unix_ms();
                self.session.active_at.store(now_ms, Ordering::SeqCst);
                let is_done = self.runner.is_done();
                res.conversation = Some(self.conversation._id);
                mark_special_user_messages(&mut res.chat_history);
                if let Some(artifacts) = self
                    .ctx
                    .base
                    .get_state::<crate::engine::resources::SessionArtifacts>()
                {
                    artifacts
                        .record(self.ctx.caller(), &mut res.artifacts)
                        .await?;
                    self.collect_artifacts();
                }

                self.session.on_completion(&self.ctx, &res).await;
                self.cron_receipts.record(&res);
                let cron_finished = res.failed_reason.is_some()
                    || (is_done
                        && self.runner.is_idle()
                        && !self.session.has_running_background_tasks()
                        && !self.session.has_pending_inputs()
                        && self.session.goal.read().is_none());

                let mut terminal_history =
                    (is_done || res.failed_reason.is_some()).then(|| res.chat_history.clone());
                if terminal_history.is_some() {
                    self.persist_tools_usage(&res.tools_usage, tools_usage_snapshot)
                        .await;
                }
                self.replace_conversation_messages_from_chat_history(res.chat_history);

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
                if self.conversation.status == ConversationStatus::Failed {
                    self.session.stop_background_tasks();
                    *self.session.goal.write() = None;
                    for event in self.session.actions.cancel_pending().await {
                        apply_action_event_to_conversation(&mut self.conversation, event.clone());
                        if let Some(history) = terminal_history.as_mut() {
                            apply_action_event_to_messages(history, event);
                        }
                    }
                }
                self.persist_conversation_state().await?;
                if cron_finished {
                    self.cron_receipts.finish();
                }

                if let Some(history) = terminal_history.as_ref() {
                    self.submit_pending_formation(history, now_ms).await;
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
                self.cron_receipts.fail(&failed_reason);
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
                self.persist_conversation_state().await?;

                return Ok(false);
            }
        }

        Ok(true)
    }
}

/// Keep approvals visible while an operation is in flight, and drop the
/// operation at a user control boundary. Database transitions themselves are
/// awaited to completion by the caller rather than cancelled halfway through.
async fn drive_session_operation<F: Future>(
    assistant: &AndaBot,
    conversation: &mut Conversation,
    action_rx: &mut tokio::sync::mpsc::Receiver<ActionEvent>,
    control: &SessionControl,
    operation: F,
) -> Result<(Option<F::Output>, Vec<ActionEvent>), BoxError> {
    tokio::pin!(operation);
    let mut events = Vec::new();
    let mut actions_open = true;
    loop {
        tokio::select! {
            biased;
            _ = control.interrupted() => return Ok((None, events)),
            result = &mut operation => return Ok((Some(result), events)),
            event = action_rx.recv(), if actions_open => {
                match event {
                    Some(event) => {
                        if apply_action_event_to_conversation(conversation, event.clone()) {
                            assistant.persist_conversation_state(conversation).await?;
                        }
                        events.push(event);
                    }
                    None => actions_open = false,
                }
            }
        }
    }
}

fn apply_action_event_to_history(runner: &mut CompletionRunner, event: ActionEvent) -> bool {
    match event {
        ActionEvent::Add(message) => {
            let id = action_id_from_message(&message);
            if id.is_some()
                && runner
                    .chat_history()
                    .iter()
                    .any(|m| action_id_from_message(m) == id)
            {
                return false;
            }
            runner.append_chat_history(vec![message]);
            true
        }
        ActionEvent::Resolve {
            action_id,
            status,
            response,
            responded_at,
        } => runner.chat_history_mut().iter_mut().rev().any(|message| {
            apply_action_resolution_to_chat_message(
                message,
                &action_id,
                status,
                &response,
                responded_at,
            )
        }),
    }
}

fn apply_action_event_to_messages(messages: &mut Vec<Message>, event: ActionEvent) {
    match event {
        ActionEvent::Add(message) => {
            let id = action_id_from_message(&message);
            if id.is_none() || !messages.iter().any(|m| action_id_from_message(m) == id) {
                messages.push(message);
            }
        }
        ActionEvent::Resolve {
            action_id,
            status,
            response,
            responded_at,
        } => {
            for message in messages.iter_mut().rev() {
                if apply_action_resolution_to_chat_message(
                    message,
                    &action_id,
                    status,
                    &response,
                    responded_at,
                ) {
                    break;
                }
            }
        }
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

fn apply_action_event_to_conversation(conversation: &mut Conversation, event: ActionEvent) -> bool {
    let updated = match event {
        ActionEvent::Add(message) => {
            let value = json!(message);
            let new_action_id = action_id_from_message_value(&value);
            if let Some(action_id) = new_action_id.as_deref()
                && conversation
                    .messages
                    .iter()
                    .filter(|message| is_action_message_value(message))
                    .filter_map(action_id_from_message_value)
                    .any(|existing| existing == action_id)
            {
                false
            } else {
                conversation.messages.push(value);
                true
            }
        }
        ActionEvent::Resolve {
            action_id,
            status,
            response,
            responded_at,
        } => conversation.messages.iter_mut().any(|message| {
            apply_action_resolution_to_message(message, &action_id, status, &response, responded_at)
        }),
    };

    if updated {
        conversation.updated_at = unix_ms();
    }
    updated
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
    use crate::engine::{ActionEvent, ActionRuntime, action::ActionStatus};
    use anda_brain::types::InputContext;
    use anda_core::{AgentOutput, BoxPinFut, RequestMeta};
    use anda_engine::{
        engine::EngineBuilder,
        model::{CompletionFeaturesDyn, Model},
    };
    use ic_auth_types::{ByteBufB64, Xid};
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

    #[test]
    fn action_events_are_persisted_and_ignored_for_model_history_count() {
        let mut conversation = Conversation {
            _id: 10,
            messages: vec![json!(Message {
                role: "user".to_string(),
                content: vec!["hello".to_string().into()],
                ..Default::default()
            })],
            ..Default::default()
        };
        let action_message = Message {
            role: "assistant".to_string(),
            name: Some("$action".to_string()),
            content: vec![ContentPart::Action {
                name: "anda.tool_approval".to_string(),
                payload: json!({"id": "act_1", "status": "pending"}),
                recipients: None,
                signature: None,
            }],
            ..Default::default()
        };

        assert!(apply_action_event_to_conversation(
            &mut conversation,
            ActionEvent::Add(action_message.clone())
        ));
        assert_eq!(conversation.messages.len(), 2);
        assert_eq!(
            conversation
                .messages
                .iter()
                .filter(|message| !is_action_message_value(message))
                .count(),
            1
        );

        assert!(!apply_action_event_to_conversation(
            &mut conversation,
            ActionEvent::Add(action_message.clone())
        ));
        assert_eq!(conversation.messages.len(), 2);

        assert!(apply_action_event_to_conversation(
            &mut conversation,
            ActionEvent::Resolve {
                action_id: "act_1".to_string(),
                status: ActionStatus::Approved,
                response: json!({"approve": true}),
                responded_at: 50,
            }
        ));
        assert_eq!(
            conversation.messages[1]["content"][0]["payload"]["status"],
            "approved"
        );
        assert_eq!(
            conversation.messages[1]["content"][0]["payload"]["response"]["approve"],
            true
        );

        let mut runner_history = [
            Message {
                role: "user".to_string(),
                content: vec!["hello".to_string().into()],
                ..Default::default()
            },
            action_message,
        ];
        assert!(runner_history.iter_mut().rev().any(|message| {
            action_id_from_message(message).as_deref() == Some("act_1")
                && apply_action_resolution_to_chat_message(
                    message,
                    "act_1",
                    ActionStatus::Approved,
                    &json!({"approve": true}),
                    50,
                )
        }));
        assert_eq!(runner_history.len(), 2);
        assert_eq!(
            action_id_from_message(&runner_history[1]).as_deref(),
            Some("act_1")
        );
        let resolved_value = json!(&runner_history[1]);
        assert_eq!(
            resolved_value["content"][0]["payload"]["status"],
            "approved"
        );
    }

    async fn spawn_runner_brain_mock() -> String {
        use axum::{Router, routing};
        let app = Router::new()
            .route(
                "/v1/anda_bot/formation",
                routing::post(|| async {
                    axum::Json(serde_json::json!({"result": AgentOutput::default()}))
                }),
            )
            .route(
                "/v1/anda_bot/execute_kip_readonly",
                routing::post(|| async {
                    axum::Json(anda_kip::Response::ok(
                        serde_json::json!({"cognitive_identity": {"name": "panda"}}),
                    ))
                }),
            )
            .route(
                "/v1/anda_bot/get_or_init_user",
                routing::post(|| async {
                    axum::Json(serde_json::json!({"result": {"name": "u"}}))
                }),
            );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/v1/anda_bot")
    }

    async fn build_runner_bot_with_brain(brain_url: String) -> AndaBot {
        let db = crate::test_support::memory_db("runner_brain").await;
        let brain_client = brain::Client::new(brain_url, Some("t".to_string()))
            .with_http_client(crate::util::http_client::new_reqwest_client());
        let conversations_tool = Arc::new(
            ConversationsTool::connect(db.clone(), "bot".to_string(), "/tmp".to_string())
                .await
                .unwrap(),
        );
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let skills = SkillLibrary::for_test(std::env::temp_dir().join("runner_skills2_home"));
        let bridge = Arc::new(BrowserBridge::new());
        AndaBot::new(
            brain_client,
            Arc::new(anda_engine::model::Models::default()),
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
        let db = crate::test_support::memory_db("runner").await;
        // Keep Brain failures deterministic. A refused TCP connection can take
        // seconds on Windows and outlive the cancellation tests' deadlines.
        let brain_url =
            crate::test_support::spawn_http_mock(axum::Router::new().fallback(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "test Brain unavailable",
                )
            }))
            .await;
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let brain_client =
            brain::Client::new(format!("{brain_url}/v1/anda_bot"), Some("t".to_string()))
                .with_http_client(http);
        let conversations_tool = Arc::new(
            ConversationsTool::connect(db.clone(), "bot".to_string(), "/tmp".to_string())
                .await
                .unwrap(),
        );
        let resource_store = Arc::new(ResourceStore::connect(db.clone()).await.unwrap());
        let skills = SkillLibrary::for_test(std::env::temp_dir().join("runner_skills_home"));
        let bridge = Arc::new(BrowserBridge::new());

        AndaBot::new(
            brain_client,
            Arc::new(anda_engine::model::Models::default()),
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

    fn build_session() -> (
        Arc<Session>,
        tokio::sync::mpsc::Receiver<ConversationInput>,
        tokio::sync::mpsc::Receiver<ActionEvent>,
    ) {
        let (sender, rx) = tokio::sync::mpsc::channel(8);
        let (action_sender, action_rx) = tokio::sync::mpsc::channel(8);
        let session_id = Xid::new();
        let conversation_id = Arc::new(AtomicU64::new(1));
        let session = Arc::new(Session {
            control: Default::default(),
            background_controls: Default::default(),
            memory_policy: Default::default(),
            id: session_id,
            caller: "caller".to_string(),
            workspace: "/tmp".to_string(),
            source_key: "test".to_string(),
            conversation_id: conversation_id.clone(),
            sender,
            actions: crate::engine::ActionSession::new(
                Arc::new(ActionRuntime::new()),
                action_sender,
                "caller".to_string(),
                session_id.to_string(),
                conversation_id,
                Arc::new(anda_engine::model::Models::default()),
                std::env::temp_dir(),
            ),
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
        (session, rx, action_rx)
    }

    fn mock_runner_ctx() -> AgentCtx {
        EngineBuilder::new()
            .with_model(Model::mock_implemented())
            .mock_ctx()
    }

    fn input(command: PromptCommand) -> ConversationInput {
        ConversationInput {
            cron_receipt: None,
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
        let (session, rx, action_rx) = build_session();
        ctx.base
            .set_state(crate::engine::resources::SessionArtifacts::new(
                bot.inner.resource_store.clone(),
            ));
        let req = CompletionRequest::default();
        let runner = ctx.clone().completion_iter(req.clone(), vec![]).unbound();
        let mut sess_runner = SessionRunner {
            cron_receipts: Default::default(),
            ctx,
            assistant: bot.clone(),
            session,
            conversation: Conversation {
                _id: 1,
                ..Default::default()
            },
            action_rx,
            runner,
            extra_user_context: None,
            last_extra_user_context: None,
            wait_for_input: false,
        };
        persist_runner_conversation(&mut sess_runner).await;
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
    async fn memory_policy_compaction_preserves_restrictions_and_source_ancestry_without_formation()
    {
        use super::super::memory_policy::{InheritedMemorySources, MemoryMode, MemoryPolicy};
        for mode in [MemoryMode::NoStore, MemoryMode::Off] {
            let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let count = writes.clone();
            let app = axum::Router::new().route(
                "/formation",
                axum::routing::post(move || {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        axum::Json(json!({"result":{"content":""}}))
                    }
                }),
            );
            let url = crate::test_support::spawn_http_mock(app).await;
            let bot = build_runner_bot_with_brain(url).await;
            let requests = Arc::new(Mutex::new(Vec::new()));
            let ctx = recording_usage_ctx(requests);
            let policy = MemoryPolicy::new(mode);
            ctx.base.set_state(policy.clone());
            ctx.base
                .set_state(InheritedMemorySources(vec!["trusted-parent-source".into()]));
            let (mut runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
            Arc::get_mut(&mut runner.session).unwrap().memory_policy = policy.clone();
            let mut extra = serde_json::Map::new();
            policy.persist(&mut extra);
            runner.conversation.extra = Some(json!(extra));
            let parent = persist_runner_conversation(&mut runner).await;
            let mut snapshot = HashMap::new();
            runner
                .run(
                    vec![input(PromptCommand::Plain {
                        prompt: "private source".into(),
                    })],
                    &mut snapshot,
                )
                .await
                .unwrap();
            runner.run(vec![], &mut snapshot).await.unwrap();
            assert_ne!(runner.conversation._id, parent);
            let child = bot
                .inner
                .conversations
                .conversations
                .get_conversation(runner.conversation._id)
                .await
                .unwrap();
            assert_eq!(MemoryPolicy::from_conversation(&child).unwrap().mode, mode);
            assert_eq!(
                child.extra.as_ref().unwrap()["memory_source_parents"],
                json!(["trusted-parent-source"])
            );
            assert_eq!(writes.load(Ordering::SeqCst), 0);
        }
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
                reported_usage: Usage::default(),
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
                reported_usage: Usage::default(),
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
                reported_usage: Usage::default(),
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
                reported_usage: Usage::default(),
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
    async fn spawned_runner_preserves_skill_context_when_attaching_resources() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx_with_input_tokens(requests.clone(), 1);
        let (session, rx, action_rx) = build_session();
        let stop_sender = session.sender.clone();
        let req = CompletionRequest {
            prompt: "build it".to_string(),
            content: vec![
                system_runtime_prompt(
                    "prompt command",
                    "Use the coder skill to handle this request",
                )
                .into(),
            ],
            ..Default::default()
        };
        let resource = Resource {
            name: "note.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            blob: Some(ByteBufB64(b"hello".to_vec())),
            tags: vec!["text".to_string()],
            ..Default::default()
        };

        bot.inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&Conversation::default()))
            .await
            .unwrap();
        bot.spawn_session_runner(
            ctx,
            req,
            vec![resource],
            vec![],
            session,
            Conversation {
                _id: 1,
                status: ConversationStatus::Working,
                ..Default::default()
            },
            rx,
            action_rx,
            None,
            None,
        );

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !requests.lock().is_empty() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("spawned runner did not issue a completion request");

        {
            let recorded = requests.lock();
            let request = &recorded[0];
            let text = request_text(request);
            assert!(text.contains("Use the coder skill to handle this request"));
            assert!(text.contains("build it"));
            let content_json = serde_json::to_string(&request.content).unwrap();
            assert!(content_json.contains("note.txt"));
            drop(recorded);
        }

        let _ = stop_sender
            .send(input(PromptCommand::Stop {
                prompt: "/stop".to_string(),
            }))
            .await;
    }

    #[tokio::test]
    async fn session_runner_persist_helpers_run() {
        let bot = build_runner_bot().await;
        let (sess_runner, _rx) = build_session_runner(&bot).await;
        // The persistence helpers operate on the in-memory conversation store.
        sess_runner.persist_conversation_state().await.unwrap();
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

    #[tokio::test]
    async fn stop_clears_active_goal() {
        let bot = build_runner_bot().await;
        let (mut r, _rx) = build_session_runner(&bot).await;
        *r.session.goal.write() = Some(crate::engine::goal::GoalState::new(
            "unfinished goal".into(),
        ));
        r.run(
            vec![input(PromptCommand::Stop {
                prompt: "/stop".into(),
            })],
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert!(r.session.goal.read().is_none(), "STOP left the goal active");
    }

    #[tokio::test]
    async fn stop_cancels_background_handle() {
        use anda_engine::hook::{AgentHook, BackgroundHandle};
        let (session, _rx, _actions) = build_session();
        let ctx = mock_runner_ctx();
        let token = tokio_util::sync::CancellationToken::new();
        session
            .on_background_start(
                &ctx,
                BackgroundHandle::new("bg-review", token.clone()),
                &CompletionRequest::default(),
            )
            .await;
        session.stop_background_tasks();
        assert!(
            token.is_cancelled(),
            "background task was hidden but not cancelled"
        );
    }

    #[derive(Clone, Debug)]
    struct FailedOutputCompleter;
    impl CompletionFeaturesDyn for FailedOutputCompleter {
        fn model_name(&self) -> String {
            "review-failed-output".into()
        }
        fn completion(&self, _req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            Box::pin(futures::future::ready(Ok(AgentOutput {
                failed_reason: Some("provider failure".into()),
                chat_history: vec![Message {
                    role: "assistant".into(),
                    content: vec!["failure details".to_string().into()],
                    ..Default::default()
                }],
                ..Default::default()
            })))
        }
    }
    #[tokio::test]
    async fn failed_output_keeps_history() {
        let bot = build_runner_bot().await;
        let ctx = EngineBuilder::new()
            .with_model(Model::new(Arc::new(FailedOutputCompleter)))
            .mock_ctx();
        let (mut r, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        persist_runner_conversation(&mut r).await;
        r.runner.append_chat_history(vec![Message {
            role: "user".into(),
            content: vec!["previous request".to_string().into()],
            ..Default::default()
        }]);
        let keep = r
            .run(
                vec![input(PromptCommand::Plain {
                    prompt: "continue".into(),
                })],
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        assert!(!keep);
        assert!(
            !r.conversation.messages.is_empty(),
            "terminal output erased existing conversation history"
        );
    }
    #[tokio::test]
    async fn compaction_updates_source_identity() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx_with_input_tokens(requests, 10);
        let (mut r, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        persist_runner_conversation(&mut r).await;
        let sid = r.session.id.to_string();
        r.ctx.base.set_state(crate::brain::product::source_identity(
            &r.session.caller,
            r.conversation._id,
            Some(&sid),
        ));
        r.run(
            vec![input(PromptCommand::Plain {
                prompt: "work".into(),
            })],
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert!(
            r.compact(Some("continue".into()), &mut HashMap::new())
                .await
                .unwrap()
        );
        let actual = r
            .ctx
            .base
            .get_state::<anda_brain::product::SourceIdentity>()
            .unwrap();
        let expected = crate::brain::product::source_identity(
            &r.session.caller,
            r.conversation._id,
            Some(&sid),
        );
        assert_eq!(
            actual.key, expected.key,
            "compacted child still uses parent memory source"
        );
    }

    #[tokio::test]
    async fn background_usage_counts_deltas() {
        use anda_engine::hook::{AgentHook, BackgroundHandle};
        let (session, mut rx, _actions) = build_session();
        let ctx = mock_runner_ctx();
        session
            .on_background_start(
                &ctx,
                BackgroundHandle::new("usage-review", tokio_util::sync::CancellationToken::new()),
                &CompletionRequest::default(),
            )
            .await;
        for (n, text) in [(100, "first progress"), (250, "second progress")] {
            session
                .on_background_progress(
                    &ctx,
                    "usage-review".into(),
                    AgentOutput {
                        content: text.into(),
                        usage: Usage {
                            input_tokens: n,
                            requests: 1,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .await;
        }
        session
            .on_background_end(
                &ctx,
                "usage-review".into(),
                AgentOutput {
                    content: "final output".into(),
                    usage: Usage {
                        input_tokens: 250,
                        requests: 2,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await;
        let mut sum = 0;
        while let Ok(input) = rx.try_recv() {
            sum += input.usage.input_tokens;
        }
        assert_eq!(sum, 250, "cumulative background usage was added repeatedly");
    }

    struct ArtifactToolFixture;
    impl anda_core::Tool<anda_engine::context::BaseCtx> for ArtifactToolFixture {
        type Args = serde_json::Value;
        type Output = String;
        fn name(&self) -> String {
            "review_artifact".into()
        }
        fn description(&self) -> String {
            "fixture".into()
        }
        fn definition(&self) -> anda_core::FunctionDefinition {
            anda_core::FunctionDefinition {
                name: self.name(),
                description: self.description(),
                parameters: json!({"type":"object"}),
                strict: None,
            }
        }
        async fn call(
            &self,
            _ctx: anda_engine::context::BaseCtx,
            _args: Self::Args,
            _resources: Vec<Resource>,
        ) -> Result<anda_core::ToolOutput<String>, BoxError> {
            Ok(anda_core::ToolOutput {
                output: "created".into(),
                artifacts: vec![Resource {
                    name: "review-output.txt".into(),
                    blob: Some(ByteBufB64(b"output".to_vec())),
                    ..Default::default()
                }],
                ..Default::default()
            })
        }
    }
    #[derive(Clone, Debug)]
    struct ArtifactCompleter;
    impl CompletionFeaturesDyn for ArtifactCompleter {
        fn model_name(&self) -> String {
            "review-artifact-model".into()
        }
        fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            let is_tool_result = req.role.as_deref() == Some("tool");
            let tools = if is_tool_result {
                vec![]
            } else {
                vec![anda_core::ToolCall {
                    name: "review_artifact".into(),
                    args: json!({}),
                    call_id: Some("review-call".into()),
                    result: None,
                    remote_id: None,
                }]
            };
            Box::pin(futures::future::ready(Ok(AgentOutput {
                content: "done".into(),
                tool_calls: tools,
                chat_history: pending_request_messages(&req, 42),
                ..Default::default()
            })))
        }
    }
    #[tokio::test]
    async fn one_shot_persists_tool_artifact() {
        let bot = build_runner_bot().await;
        let ctx = EngineBuilder::new()
            .with_model(Model::new(Arc::new(ArtifactCompleter)))
            .register_tool(crate::engine::resources::record_artifacts(Arc::new(
                ArtifactToolFixture,
            )))
            .unwrap()
            .mock_ctx();
        let (mut r, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let id = persist_runner_conversation(&mut r).await;
        r.session.finish_when_idle.store(true, Ordering::SeqCst);
        let mut snapshot = HashMap::new();
        assert!(
            r.run(
                vec![input(PromptCommand::Plain {
                    prompt: "create a file".into()
                })],
                &mut snapshot
            )
            .await
            .unwrap()
        );
        assert!(r.run(vec![], &mut snapshot).await.unwrap());
        assert!(
            r.runner.tools_usage().contains_key("review_artifact"),
            "fixture must execute artifact tool"
        );
        assert!(!r.run(vec![], &mut snapshot).await.unwrap());
        let stored = bot
            .inner
            .conversations
            .conversations
            .get_conversation(id)
            .await
            .unwrap();
        assert_eq!(
            stored.artifacts.len(),
            1,
            "normal completion discarded tool artifacts"
        );
    }

    #[derive(Clone, Debug)]
    struct InterruptibleCompleter {
        entered: Arc<tokio::sync::Notify>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl CompletionFeaturesDyn for InterruptibleCompleter {
        fn model_name(&self) -> String {
            "interruptible".into()
        }
        fn completion(&self, req: CompletionRequest) -> BoxPinFut<Result<AgentOutput, BoxError>> {
            let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
            let entered = self.entered.clone();
            Box::pin(async move {
                if first {
                    entered.notify_one();
                    futures::future::pending::<()>().await;
                }
                Ok(AgentOutput {
                    content: "resumed".into(),
                    chat_history: pending_request_messages(&req, 42),
                    ..Default::default()
                })
            })
        }
    }

    #[tokio::test]
    async fn control_interrupts_in_flight_completion_and_keeps_runner_reusable() {
        let bot = build_runner_bot().await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let ctx = EngineBuilder::new()
            .with_model(Model::new(Arc::new(InterruptibleCompleter {
                entered: entered.clone(),
                calls: Arc::default(),
            })))
            .mock_ctx();
        let (mut r, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        r.runner.follow_up("start".to_string());
        let control = r.session.control.clone();
        let stopper = tokio::spawn(async move {
            entered.notified().await;
            control.request();
        });
        let (result, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            r.next_with_action_events(),
        )
        .await
        .unwrap()
        .unwrap();
        stopper.await.unwrap();
        assert!(result.is_none());
        r.session.control.reset();
        r.run(
            vec![input(PromptCommand::Stop {
                prompt: "/stop".into(),
            })],
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(r.conversation.status, ConversationStatus::Idle);
        r.run(
            vec![input(PromptCommand::Plain {
                prompt: "next task".into(),
            })],
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert!(
            r.runner
                .chat_history()
                .iter()
                .filter_map(Message::text)
                .any(|text| text.contains("next task"))
        );
    }

    #[tokio::test]
    async fn formation_without_new_messages_does_not_rewrite_saved_history() {
        let bot = build_runner_bot().await;
        let (r, _rx) = build_session_runner(&bot).await;
        let history = vec![Message {
            role: "user".into(),
            content: vec!["already submitted".to_string().into()],
            ..Default::default()
        }];
        r.session.submit_formation_at.store(1, Ordering::SeqCst);
        let mut saved = r.conversation.clone();
        saved.append_messages(vec![Message {
            role: "assistant".into(),
            content: vec!["newer persisted state".to_string().into()],
            ..Default::default()
        }]);
        bot.persist_conversation_state(&saved).await.unwrap();
        r.submit_pending_formation(&history, unix_ms()).await;
        let reloaded = bot
            .inner
            .conversations
            .conversations
            .get_conversation(saved._id)
            .await
            .unwrap();
        assert_eq!(reloaded.messages, saved.messages);
        assert_eq!(r.session.formation_backoff_until.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn conversation_write_errors_reach_the_caller() {
        let bot = build_runner_bot().await;
        let missing = Conversation {
            _id: u64::MAX,
            ..Default::default()
        };
        assert!(bot.persist_conversation_state(&missing).await.is_err());
    }

    #[tokio::test]
    async fn stop_interrupts_approval_wait_and_resolves_the_card() {
        let bot = build_runner_bot().await;
        let (mut r, _rx) = build_session_runner(&bot).await;
        let mut meta = RequestMeta::default();
        meta.extra
            .insert("approval_mode".into(), json!("request_approval"));
        r.session.request_meta.set(meta);
        r.ctx.base.set_state(r.session.request_meta.clone());
        let actions = r.session.actions.clone();
        let ctx = r.ctx.base.clone();
        let control = r.session.control.clone();
        let observer = bot.clone();
        let id = r.conversation._id;
        let stopper = tokio::spawn(async move {
            loop {
                let saved = observer
                    .inner
                    .conversations
                    .conversations
                    .get_conversation(id)
                    .await
                    .unwrap();
                if saved.messages.iter().any(is_action_message_value) {
                    control.request();
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        });
        let approval = actions.request_shell_approval(
            &ctx,
            anda_engine::extension::shell::ExecArgs {
                command: "echo approval".into(),
                ..Default::default()
            },
        );
        let (result, events) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            drive_session_operation(
                &bot,
                &mut r.conversation,
                &mut r.action_rx,
                &r.session.control,
                approval,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        stopper.await.unwrap();
        assert!(result.is_none());
        for event in events {
            r.apply_action_event_to_runner(event);
        }
        // Intentionally keep the interrupt set: a queued control must be handled
        // before the run() path starts preparing resources.
        r.run(
            vec![input(PromptCommand::Stop {
                prompt: "/stop".into(),
            })],
            &mut HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(r.conversation.status, ConversationStatus::Idle);
        assert!(
            r.conversation
                .messages
                .iter()
                .any(|message| message["content"][0]["payload"]["status"] == "denied")
        );
        assert!(r.session.actions.cancel_pending().await.is_empty());
    }

    #[tokio::test]
    async fn queued_message_after_stop_survives_the_batch() {
        let bot = build_runner_bot().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let ctx = recording_usage_ctx_with_input_tokens(requests.clone(), 1);
        let (session, rx, action_rx) = build_session();
        let mut conversation = Conversation::default();
        conversation._id = bot
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await
            .unwrap();
        let conversation_id = conversation._id;
        bot.spawn_session_runner(
            ctx,
            CompletionRequest::default(),
            vec![],
            vec![],
            session.clone(),
            conversation,
            rx,
            action_rx,
            None,
            None,
        );
        session
            .sender
            .try_send(input(PromptCommand::Stop {
                prompt: "/stop".into(),
            }))
            .unwrap();
        session
            .sender
            .try_send(input(PromptCommand::Plain {
                prompt: "new task after stop".into(),
            }))
            .unwrap();
        session.control.request();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if requests
                    .lock()
                    .iter()
                    .any(|req| request_text(req).contains("new task after stop"))
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("the message queued after stop must reach the model");
        session
            .sender
            .send(input(PromptCommand::Cancel {
                prompt: "/cancel".into(),
            }))
            .await
            .unwrap();
        session.control.request();
        tokio::time::timeout(std::time::Duration::from_secs(2), session.sender.closed())
            .await
            .expect("cancel must close the session even when Brain rejects formation");
        let saved = bot
            .inner
            .conversations
            .conversations
            .get_conversation(conversation_id)
            .await
            .unwrap();
        assert_eq!(saved.status, ConversationStatus::Cancelled);
        assert!(session.formation_backoff_until.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn formation_persists_json_messages_and_advances_the_watermark() {
        let bot = build_runner_bot_with_brain(spawn_runner_brain_mock().await).await;
        let (r, _rx) = build_session_runner(&bot).await;
        let history = vec![Message {
            role: "user".into(),
            content: vec!["remember this source".to_string().into()],
            ..Default::default()
        }];
        r.submit_pending_formation(&history, unix_ms()).await;
        assert_eq!(r.session.submit_formation_at.load(Ordering::SeqCst), 1);
        assert_eq!(r.session.formation_backoff_until.load(Ordering::SeqCst), 0);
        let saved = bot
            .inner
            .conversations
            .conversations
            .get_conversation(r.conversation._id)
            .await
            .unwrap();
        assert_eq!(saved.messages, vec![json!(history[0])]);
    }

    #[tokio::test]
    async fn cron_receipt_waits_for_queued_background_result_and_reports_model_failure() {
        let bot = build_runner_bot().await;
        let ctx = recording_usage_ctx_with_input_tokens(Arc::default(), 10);
        let (mut runner, mut rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let (submission, mut completion) = crate::cron::AgentSubmission::new();
        let mut request = input(PromptCommand::Plain {
            prompt: "scheduled".into(),
        });
        request.cron_receipt = submission.take();
        runner
            .session
            .background_tasks
            .write()
            .insert("shell:test".into(), Default::default());
        runner
            .run(vec![request], &mut HashMap::new())
            .await
            .unwrap();
        assert!(matches!(
            completion.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        // The process has ended, but its final output is still queued for the model.
        runner
            .session
            .sender
            .send(input(PromptCommand::Plain {
                prompt: "final background output".into(),
            }))
            .await
            .unwrap();
        runner.session.background_tasks.write().remove("shell:test");
        runner.run(vec![], &mut HashMap::new()).await.unwrap();
        assert!(matches!(
            completion.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        let final_input = rx.recv().await.unwrap();
        runner
            .run(vec![final_input], &mut HashMap::new())
            .await
            .unwrap();
        runner.run(vec![], &mut HashMap::new()).await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), completion)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.result.as_deref(), Some("normal output"));

        let ctx = EngineBuilder::new()
            .with_model(Model::new(Arc::new(FailedOutputCompleter)))
            .mock_ctx();
        let (mut runner, _rx) = build_session_runner_with_ctx(&bot, ctx).await;
        let (submission, completion) = crate::cron::AgentSubmission::new();
        let mut request = input(PromptCommand::Plain {
            prompt: "scheduled failure".into(),
        });
        request.cron_receipt = submission.take();
        runner
            .run(vec![request], &mut HashMap::new())
            .await
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), completion)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.error.as_deref(), Some("provider failure"));
    }

    #[tokio::test]
    async fn cron_cancellation_stops_work_in_an_existing_session() {
        let (session, mut rx, _actions) = build_session();
        let (submission, completion) = crate::cron::AgentSubmission::new();
        let mut receipt = submission.take().unwrap();
        session.bind_cron_receipt(&mut receipt);
        submission.cancellation_token().cancel();
        let input = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(input.command, PromptCommand::Cancel { .. }));
        assert!(session.control.is_pending());
        drop(receipt);
        assert!(completion.await.unwrap().error.is_some());
    }
}
