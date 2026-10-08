//! Startup self-check: resume interrupted source-bound conversations after a
//! daemon restart, and repair the statuses and owners recorded for their
//! sources.

use anda_core::{AgentContext, BoxError, CompletionRequest};
use anda_engine::{
    context::AgentCtx,
    memory::{Conversation, ConversationStatus},
    unix_ms,
};
use ic_auth_types::Xid;
use std::collections::{HashMap, HashSet};

use super::{
    AndaBot, SessionSpec,
    instructions::available_tool_names,
    meta::{
        conversation_chat_history, request_meta_for_conversation, request_meta_from_conversation,
    },
    session::SessionRequestMeta,
};
use crate::engine::{
    conversation::{RequestState, SourceStateRepair},
    system::system_runtime_prompt,
};

struct StartupConversation {
    source_key: String,
    conversation: Conversation,
}

impl AndaBot {
    pub(super) async fn startup_self_check(&self, ctx: AgentCtx) {
        for candidate in self.startup_source_candidates(unix_ms()).await {
            if !should_auto_resume_conversation(&candidate.conversation.status) {
                continue;
            }
            let conversation = candidate.conversation._id;
            let prompt = startup_recovery_prompt(&candidate.conversation);
            // One unrecoverable conversation must not strand the others.
            if let Err(err) = self
                .continue_startup_conversation(
                    ctx.with_caller(candidate.conversation.user),
                    candidate,
                    prompt,
                )
                .await
            {
                log::error!(conversation; "startup self-check could not resume conversation: {err}");
            }
        }
    }

    async fn startup_source_candidates(&self, now_ms: u64) -> Vec<StartupConversation> {
        let source_conversations = self.inner.conversations.source_conversations();
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        let mut repairs = HashMap::new();

        for (source_key, state) in source_conversations {
            if state.conv_id == 0 {
                continue;
            }

            match self.latest_conversation_in_chain(state.conv_id, None).await {
                Ok(None) => {}
                Ok(Some(conversation)) => {
                    if state.status != conversation.status || state.user.is_none() {
                        repairs.insert(
                            source_key.clone(),
                            SourceStateRepair {
                                observed: state,
                                status: conversation.status.clone(),
                                user: conversation.user,
                            },
                        );
                    }
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

        if let Err(err) = self.inner.conversations.repair_source_states(repairs).await {
            log::warn!("startup self-check could not repair source states: {err}");
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

    async fn continue_startup_conversation(
        &self,
        ctx: AgentCtx,
        candidate: StartupConversation,
        prompt: String,
    ) -> Result<(), BoxError> {
        let mut conversation = candidate.conversation;
        let parents = conversation
            .extra
            .as_ref()
            .and_then(|v| v.get("memory_source_parents"))
            .map(|value| serde_json::from_value::<Vec<String>>(value.clone()))
            .transpose()?
            .unwrap_or_default();
        if parents.len() > 15 {
            return Err("Memory source ancestry exceeds the supported nesting limit".into());
        }
        ctx.base
            .set_state(super::memory_policy::InheritedMemorySources(parents));
        ctx.base
            .set_state(super::memory_policy::MemoryPolicy::from_conversation(
                &conversation,
            )?);
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
        self.ensure_plan_owner(&conversation.user, &meta)?;
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
                true,
                now_ms,
            )
            .await?;
        let tools = self.initial_tools(conversation.user, &meta, &available_tools, Vec::new());
        let initial_req = CompletionRequest {
            instructions,
            prompt,
            chat_history: chat_history.clone(),
            tools: ctx.definitions(Some(&tools)).await,
            tool_choice_required: false,
            ..Default::default()
        };

        let session_request_meta = SessionRequestMeta::new(meta.clone());
        // A fresh id when the conversation has no thread: the zero default id
        // would collide across resumed conversations in the session map.
        let sess_id = match conversation.thread {
            Some(thread) => thread,
            None => Xid::new(),
        };

        // Same discipline as AndaBot::run(): the instruction build above spans
        // slow brain/DB calls, so a channel message may have created a session
        // for this conversation meanwhile. Re-check under the session creation
        // lock and hold it through insert_session, or two runners for the same
        // session id would race persist_conversation_state and the orphan's
        // detach_session would later evict the healthy runner.
        let caller = conversation.user.to_string();
        let _session_creation_guard = self.inner.session_creation_lock.lock().await;
        if self
            .find_joinable_session(&sess_id, &source_key, &caller)
            .is_some()
        {
            return Ok(());
        }

        conversation.thread = Some(sess_id);
        conversation.status = ConversationStatus::Working;
        conversation.updated_at = now_ms;
        self.persist_conversation_state(&conversation).await?;

        let (session, rx, action_rx) = self.create_session(
            &ctx,
            SessionSpec {
                sess_id,
                caller,
                workspace,
                source_key,
                conversation_id: conversation._id,
                request_meta: session_request_meta,
                meta: &meta,
                initial_goal: None,
                formation_topic: Some("startup_self_check"),
                active_at_ms: now_ms,
            },
        );

        self.spawn_session_runner(
            ctx,
            initial_req,
            vec![],
            chat_history,
            session,
            conversation,
            rx,
            action_rx,
            None,
            None,
        );
        Ok(())
    }
}

fn should_auto_resume_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted | ConversationStatus::Working
    )
}

fn startup_recovery_prompt(conversation: &Conversation) -> String {
    system_runtime_prompt(
        "startup recovery",
        format!(
            "Startup self-check found this conversation in {:?} state after the process restarted. Continue from the latest saved history. If the previous user request is still incomplete, resume it and send the next useful progress update. If it already appears complete, briefly explain that the session was recovered and ask for the next step. Avoid repeating old content unnecessarily.",
            conversation.status
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
