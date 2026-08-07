//! Startup self-check: resume interrupted source-bound conversations after a
//! daemon restart, plus the optional self-exploration bootstrap.

use anda_core::{AgentContext, BoxError, CompletionRequest, RequestMeta, StateFeatures};
use anda_db_utils::UniqueVec;
use anda_engine::{
    context::AgentCtx,
    memory::{Conversation, ConversationRef, ConversationStatus},
    unix_ms,
};
use ic_auth_types::Xid;
use serde_json::{Map, json};
use std::collections::HashSet;

use super::{
    AndaBot, SessionSpec,
    instructions::available_tool_names,
    meta::{
        conversation_chat_history, request_meta_for_conversation, request_meta_from_conversation,
    },
    select_most_used_tools,
    session::SessionRequestMeta,
};
use crate::engine::{
    browser::ChromeBrowserTool,
    conversation::{RequestState, SourceState},
    system::system_runtime_prompt,
};

const STARTUP_SELF_SOURCE: &str = "startup:self";

#[derive(Debug, Clone)]
struct StartupConversation {
    source_key: String,
    conversation: Conversation,
}

impl AndaBot {
    pub(super) async fn startup_self_check(&self, ctx: AgentCtx) -> Result<(), BoxError> {
        let candidates = self.startup_source_candidates(unix_ms()).await;
        let resume: Vec<&StartupConversation> = candidates
            .iter()
            .filter(|candidate| should_auto_resume_conversation(&candidate.conversation.status))
            .collect();

        if !resume.is_empty() {
            for candidate in resume {
                self.continue_startup_conversation(
                    ctx.with_caller(candidate.conversation.user),
                    candidate.clone(),
                    startup_recovery_prompt(&candidate.conversation),
                )
                .await?;
            }
            return Ok(());
        }

        Ok(())

        // log::info!(
        //     "startup self-check found no source-bound conversation; starting self exploration"
        // );
        // self.start_startup_exploration(ctx).await
    }

    async fn startup_source_candidates(&self, now_ms: u64) -> Vec<StartupConversation> {
        let source_conversations = self.inner.conversations.source_conversations();
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        for (source_key, state) in source_conversations {
            if state.conv_id == 0 {
                continue;
            }

            match self.latest_conversation_in_chain(state.conv_id, None).await {
                Ok(conversation) => {
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
                now_ms,
            )
            .await?;
        let mut tools = UniqueVec::from(self.inner.tools.clone());
        if self.inner.browser_manager.is_active() {
            tools.extend(
                ChromeBrowserTool::active_tool_names()
                    .into_iter()
                    .map(str::to_string),
            );
        }

        tools.extend(
            self.inner.conversations.tool_usage_with(|usage| {
                select_most_used_tools(&available_tools, &tools, usage, 3)
            }),
        );
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
        let sess_id = match conversation.thread.clone() {
            Some(thread) => thread,
            None => Xid::new(),
        };

        // Same discipline as AndaBot::run(): the instruction build above spans
        // slow brain/DB calls, so a channel message may have created a session
        // for this conversation meanwhile. Re-check under the session creation
        // lock and hold it through insert_session, or two runners for the same
        // session id would race persist_conversation_state and the orphan's
        // detach_session would later evict the healthy runner.
        let _session_creation_guard = self.inner.session_creation_lock.lock().await;
        if self.get_session(&sess_id).is_some() || self.get_session_by_source(&source_key).is_some()
        {
            return Ok(());
        }

        conversation.thread = Some(sess_id.clone());
        conversation.status = ConversationStatus::Working;
        conversation.updated_at = now_ms;
        self.persist_conversation_state(&conversation).await;

        let (session, rx, action_rx) = self.create_session(
            &ctx,
            SessionSpec {
                sess_id,
                caller: conversation.user.to_string(),
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
        );
        Ok(())
    }

    #[allow(unused)]
    async fn start_startup_exploration(&self, ctx: AgentCtx) -> Result<(), BoxError> {
        let now_ms = unix_ms();
        let mut extra = Map::new();
        let workspace = self.inner.home_dir.to_string_lossy().to_string();
        extra.insert("workspace".to_string(), workspace.into());
        extra.insert("source".to_string(), STARTUP_SELF_SOURCE.into());
        let meta = RequestMeta {
            extra,
            ..Default::default()
        };
        let mut conversation = Conversation {
            user: *ctx.caller(),
            thread: Some(Xid::new()),
            messages: Vec::new(),
            resources: vec![],
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            extra: Some(json!(meta.extra)),
            ..Default::default()
        };
        let conv_id = self
            .inner
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = conv_id;
        if let Err(err) = self
            .inner
            .conversations
            .update_source_state(
                STARTUP_SELF_SOURCE.to_string(),
                SourceState {
                    conv_id,
                    status: conversation.status.clone(),
                    timestamp: now_ms,
                },
            )
            .await
        {
            log::warn!(conversation = conv_id; "failed to persist startup self source state: {err}");
        }

        self.continue_startup_conversation(
            ctx,
            StartupConversation {
                source_key: STARTUP_SELF_SOURCE.to_string(),
                conversation,
            },
            startup_exploration_prompt(),
        )
        .await
    }
}

fn should_auto_resume_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Submitted | ConversationStatus::Working
    )
}

#[allow(unused)]
fn should_startup_greet_conversation(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Idle | ConversationStatus::Failed
    )
}

#[allow(unused)]
fn startup_recovery_prompt(conversation: &Conversation) -> String {
    system_runtime_prompt(
        "startup recovery",
        format!(
            "Startup self-check found this conversation in {:?} state after the process restarted. Continue from the latest saved history. If the previous user request is still incomplete, resume it and send the next useful progress update. If it already appears complete, briefly explain that the session was recovered and ask for the next step. Avoid repeating old content unnecessarily.",
            conversation.status
        ),
    )
}

#[allow(unused)]
fn startup_greeting_prompt(conversation: &Conversation) -> String {
    system_runtime_prompt(
        "startup greeting",
        format!(
            "Startup self-check found no interrupted conversation. This is the most recent active conversation source, currently in {:?} state. Send a concise, natural greeting that says you are online again and offer one concrete way to continue based on the saved context. Do not claim the user just spoke.",
            conversation.status
        ),
    )
}

#[allow(unused)]
fn startup_exploration_prompt() -> String {
    system_runtime_prompt(
        "startup exploration",
        "Startup self-check found no source-bound conversation. Do a brief, read-only self exploration: inspect your runtime context, identify one useful capability or maintenance idea worth remembering for future work, and summarize it concisely. Do not contact external users.",
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

        assert!(should_startup_greet_conversation(&ConversationStatus::Idle));
        assert!(should_startup_greet_conversation(
            &ConversationStatus::Failed
        ));
        assert!(!should_startup_greet_conversation(
            &ConversationStatus::Submitted
        ));
        assert!(!should_startup_greet_conversation(
            &ConversationStatus::Working
        ));
        assert!(!should_startup_greet_conversation(
            &ConversationStatus::Cancelled
        ));
    }
}
