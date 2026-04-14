use anda_core::{
    Agent, AgentContext, AgentOutput, BoxError, CacheExpiry, CacheFeatures, CompletionRequest,
    Document, Documents, Message, Resource, StateFeatures, estimate_tokens,
};
use anda_engine::{
    ANONYMOUS,
    context::{AgentCtx, SubAgentManager, TOOLS_SEARCH_NAME, TOOLS_SELECT_NAME},
    extension::{
        fs::{EditFileTool, ReadFileTool, SearchFileTool, WriteFileTool},
        shell::ShellTool,
        skill::SkillManager,
    },
    memory::{Conversation, ConversationRef, ConversationState, ConversationStatus, Conversations},
    rfc3339_datetime, unix_ms,
};
use anda_hippocampus::agents::SYSTEM_PROMPT_DYNAMIC_BOUNDARY;
use std::{collections::BTreeMap, time::Duration};

use crate::brain;

static SELF_INSTRUCTIONS: &str = "You are $self, an AI agent created for the user. You have a unique identity and memory. Always think step by step. Use tools when necessary. Your goal is to assist the user in any way you can.";

#[derive(Clone)]
pub struct AndaBot {
    brain: brain::Client,
    conversations: Conversations,
    tools: Vec<String>,
    full_tools: Vec<String>,
    max_input_tokens: usize,
}

impl AndaBot {
    pub const NAME: &'static str = "anda_bot";
    pub fn new(
        brain: brain::Client,
        conversations: Conversations,
        max_input_tokens: usize,
    ) -> Self {
        Self {
            brain,
            conversations,
            tools: vec![
                brain::Client::NAME.to_string(),
                TOOLS_SEARCH_NAME.to_string(),
                TOOLS_SELECT_NAME.to_string(),
                ShellTool::NAME.to_string(),
                ReadFileTool::NAME.to_string(),
                SearchFileTool::NAME.to_string(),
                EditFileTool::NAME.to_string(),
                WriteFileTool::NAME.to_string(),
            ],
            full_tools: vec![
                brain::Client::NAME.to_string(),
                TOOLS_SEARCH_NAME.to_string(),
                TOOLS_SELECT_NAME.to_string(),
                ShellTool::NAME.to_string(),
                ReadFileTool::NAME.to_string(),
                SearchFileTool::NAME.to_string(),
                EditFileTool::NAME.to_string(),
                WriteFileTool::NAME.to_string(),
                SubAgentManager::NAME.to_string(),
                SkillManager::NAME.to_string(),
            ],
            max_input_tokens,
        }
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
        self.tools.clone()
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

        let caller_key = format!("Running:{}", caller);
        let now_ms = unix_ms();
        let ok = ctx
            .cache_set_if_not_exists(
                &caller_key,
                (now_ms, Some(CacheExpiry::TTL(Duration::from_secs(300)))),
            )
            .await;
        if !ok {
            return Err("Only one prompt can run at a time for you".into());
        }

        let _conversation = ctx
            .meta()
            .extra
            .get("conversation")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let primer = self.brain.describe_primer().await?;
        let caller_info = self.brain.user_info(caller.to_string()).await;

        let mut msg = Message {
            role: "user".into(),
            content: vec![],
            name: Some("$system".into()),
            timestamp: Some(now_ms),
            ..Default::default()
        };

        let (mut conversations, _cursor) = self
            .conversations
            .list_conversations_by_user(caller, None, Some(7))
            .await?;
        for c in &mut conversations {
            if c.status == ConversationStatus::Failed && c.messages.len() > 1 {
                c.messages.pop(); // remove the failing message
            }
        }

        let instructions = format!(
            "{}\n\n{}\n\n---\n\n# Identity & Domains:\n{}\n\n# Current Datetime: {}",
            SELF_INSTRUCTIONS,
            SYSTEM_PROMPT_DYNAMIC_BOUNDARY,
            primer,
            rfc3339_datetime(now_ms).unwrap_or_else(|| format!("{now_ms} in unix ms"))
        );
        let max_history_bytes = self.max_input_tokens.saturating_sub(
            ((estimate_tokens(&instructions) + estimate_tokens(&prompt)) as f64 * 1.2) as usize,
        ) * 3; // Rough estimate of bytes per token
        let mut writer: Vec<u8> = Vec::with_capacity(256);
        let mut history_bytes = if serde_json::to_writer(&mut writer, &conversations).is_ok() {
            writer.len()
        } else {
            0
        };

        // Keep the most recent conversations; remove the oldest first.
        while history_bytes > max_history_bytes && !conversations.is_empty() {
            let oldest_idx = conversations
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| c.created_at)
                .map(|(idx, _)| idx)
                .unwrap_or(0);

            writer.clear();
            if serde_json::to_writer(&mut writer, &conversations[oldest_idx]).is_ok() {
                history_bytes = history_bytes.saturating_sub(writer.len());
            } else {
                break;
            }

            conversations.remove(oldest_idx);
        }

        let mut user_conversations: Vec<Document> = Vec::with_capacity(conversations.len());
        user_conversations.extend(conversations.into_iter().map(Document::from));

        if !user_conversations.is_empty() {
            msg.content.push(
                Documents::new(
                    "user_history_conversations".to_string(),
                    user_conversations.clone(),
                )
                .to_string()
                .into(),
            );
        }

        if !resources.is_empty() {
            let mut user_resources: Vec<Document> = Vec::with_capacity(resources.len());
            for r in resources.iter() {
                if r.tags.iter().any(|t| t == "text" || t == "md")
                    && let Some(content) = r
                        .blob
                        .as_ref()
                        .and_then(|b| String::from_utf8(b.0.clone()).ok())
                {
                    user_resources.push(Document::from_text(r._id.to_string().as_str(), &content));
                }
            }

            if !user_resources.is_empty() {
                msg.content.push(
                    Documents::new("user_resources".to_string(), user_resources)
                        .to_string()
                        .into(),
                );
            }
        }

        msg.content.push(
            Documents::new(
                "user_profile".to_string(),
                vec![Document {
                    content: caller_info,
                    metadata: BTreeMap::from([
                        ("type".to_string(), "Person".into()),
                        (
                            "description".to_string(),
                            "The latest user's profile (KIP concept node)".into(),
                        ),
                    ]),
                }],
            )
            .to_string()
            .into(),
        );

        let chat_history = vec![msg];

        let mut conversation = Conversation {
            user: *caller,
            messages: vec![serde_json::json!(Message {
                role: "user".into(),
                content: vec![prompt.clone().into()],
                timestamp: Some(now_ms),
                ..Default::default()
            })],
            resources,
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            ..Default::default()
        };

        let id = self
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        conversation._id = id;
        ctx.base.set_state(ConversationState::from(&conversation));
        let res = AgentOutput {
            conversation: Some(id),
            ..Default::default()
        };

        let assistant = self.clone();
        let mut runner = ctx.clone().completion_iter(
            CompletionRequest {
                instructions,
                prompt,
                chat_history,
                tools: ctx.definitions(Some(&self.full_tools)).await,
                tool_choice_required: false,
                max_output_tokens: Some(50000),
                ..Default::default()
            },
            vec![],
        );

        tokio::spawn(async move {
            let mut rt = async || {
                let mut first_round = true;
                loop {
                    match runner.next().await {
                        Ok(None) => break,
                        Ok(Some(mut res)) => {
                            let now_ms = unix_ms();

                            if first_round {
                                first_round = false;
                                conversation.messages.clear(); // clear the first pending message.
                                conversation.append_messages(res.chat_history);
                            } else {
                                let existing_len = conversation.messages.len();
                                if res.chat_history.len() >= existing_len {
                                    res.chat_history.drain(0..existing_len);
                                    conversation.append_messages(res.chat_history);
                                } else {
                                    // Unexpected: runner returned shorter full history.
                                    // Fall back to replacing stored messages.
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

                            let old = assistant
                                .conversations
                                .get_conversation(conversation._id)
                                .await?;
                            if old.status == ConversationStatus::Cancelled
                                && (conversation.status == ConversationStatus::Submitted
                                    || conversation.status == ConversationStatus::Working)
                            {
                                conversation.status = ConversationStatus::Cancelled;
                            }

                            let _ = assistant
                                .conversations
                                .update_conversation(id, conversation.to_changes()?)
                                .await;

                            ctx.base.set_state(ConversationState::from(&conversation));

                            if conversation.status == ConversationStatus::Cancelled
                                || conversation.status == ConversationStatus::Failed
                            {
                                break;
                            }
                        }
                        Err(err) => {
                            log::error!("Conversation {id} in CompletionRunner error: {:?}", err);
                            let now_ms = unix_ms();
                            conversation.failed_reason = Some(err.to_string());
                            conversation.status = ConversationStatus::Failed;
                            conversation.updated_at = now_ms;
                            let _ = assistant
                                .conversations
                                .update_conversation(id, conversation.to_changes()?)
                                .await;

                            ctx.base.set_state(ConversationState::from(&conversation));
                            break;
                        }
                    }
                }

                Ok::<(), BoxError>(())
            };

            ctx.cache_delete(&caller_key).await;
            match rt().await {
                Ok(_) => {}
                Err(err) => {
                    log::error!("Error occurred in conversation {id}: {:?}", err);
                }
            }
        });

        Ok(res)
    }
}
