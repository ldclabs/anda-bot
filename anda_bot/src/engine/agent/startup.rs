//! Startup self-check: resume interrupted source-bound conversations after a
//! daemon restart, plus the optional self-exploration bootstrap.

use anda_brain::types::InputContext;
use anda_core::{AgentContext, BoxError, CompletionRequest, RequestMeta, StateFeatures};
use anda_db_utils::UniqueVec;
use anda_engine::{
    context::AgentCtx,
    extension::shell::ShellToolHook,
    hook::DynAgentHook,
    memory::{Conversation, ConversationRef, ConversationStatus},
    unix_ms,
};
use ic_auth_types::Xid;
use parking_lot::RwLock;
use serde_json::{Map, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

use super::{
    AndaBot,
    instructions::available_tool_names,
    meta::{
        conversation_chat_history, request_meta_for_conversation, request_meta_from_conversation,
        scoped_external_user_name_from_meta,
    },
    select_most_used_tools,
    session::{ConversationInput, Session, SessionRequestMeta},
};
use crate::engine::{
    ActionEvent, ActionSession,
    browser::ChromeBrowserTool,
    conversation::{RequestState, SourceState},
    goal::GoalToolState,
    system::system_runtime_prompt,
};
use crate::util::request_meta::request_meta_extra_as;

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

        conversation.thread = Some(sess_id.clone());
        conversation.status = ConversationStatus::Working;
        conversation.updated_at = now_ms;
        self.persist_conversation_state(&conversation).await;

        let (sender, rx) = tokio::sync::mpsc::channel::<ConversationInput>(42);
        let (action_sender, action_rx) = tokio::sync::mpsc::channel::<ActionEvent>(42);
        let external_user = request_meta_extra_as::<bool>(&meta, "external_user").unwrap_or(false);
        let formation_counterparty = if external_user {
            Some(scoped_external_user_name_from_meta(&meta))
        } else {
            Some(conversation.user.to_string())
        };
        let conversation_id = Arc::new(AtomicU64::new(conversation._id));
        let session_id = sess_id.to_string();
        let session = Arc::new(Session {
            id: sess_id,
            caller: conversation.user.to_string(),
            workspace,
            source_key: source_key.clone(),
            conversation_id: conversation_id.clone(),
            sender,
            actions: ActionSession::new(
                self.inner.actions.clone(),
                action_sender,
                conversation.user.to_string(),
                session_id,
                conversation_id,
                self.inner.models.clone(),
                self.inner.home_dir.clone(),
            ),
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            background_progress_outputs: Arc::new(RwLock::new(HashMap::new())),
            goal: Arc::new(RwLock::new(None)),
            request_meta: session_request_meta.clone(),
            completion_hooks: self.inner.completion_hooks.clone(),
            submit_formation_at: AtomicU64::new(0),
            formation_backoff_until: AtomicU64::new(0),
            goal_check_backoff_until: AtomicU64::new(0),
            active_at: Arc::new(AtomicU64::new(now_ms)),
            finish_when_idle: AtomicBool::new(false),
            runner_idle: AtomicBool::new(false),
            formation_context: Some(InputContext {
                counterparty: formation_counterparty,
                agent: Some(AndaBot::NAME.to_string()),
                source: Some(source_key),
                topic: Some("startup_self_check".to_string()),
            }),
        });

        ctx.base.set_state(GoalToolState::new(
            session.goal.clone(),
            session.active_at.clone(),
        ));
        ctx.base.set_state(session_request_meta);
        ctx.base.set_state(session.actions.clone());

        let agent_hook = DynAgentHook::new(session.clone());
        ctx.base.set_state(agent_hook);

        let shell_hook = ShellToolHook::new(session.clone());
        ctx.base.set_state(shell_hook);
        self.insert_session(session.clone());

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
