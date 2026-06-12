use anda_core::{BoxError, FunctionDefinition, Resource, StateFeatures, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use anda_kip::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::runtime::ChannelSender;
use super::types::SendMessage;
use crate::cron::deserialize_optional_usize_from_number_or_string;
use crate::engine::SessionRequestMeta;
use crate::util::request_meta::request_meta_extra_as;

/// Sends a message to a recipient on a configured IM channel, regardless of
/// where the current conversation originated (any -> agent -> IM).
#[derive(Clone)]
pub struct SendImMessageTool {
    sender: ChannelSender,
}

impl SendImMessageTool {
    pub const NAME: &'static str = "send_im_message";

    pub fn new(sender: ChannelSender) -> Self {
        Self { sender }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendImMessageArgs {
    pub channel: String,
    pub recipient: String,
    pub content: String,
    pub thread: Option<String>,
}

impl Tool<BaseCtx> for SendImMessageTool {
    type Args = SendImMessageArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Sends a message to a recipient on a configured IM channel (Telegram, WeChat, Discord, Lark), ",
            "independent of where the current conversation came from. ",
            "Use list_im_channels first to discover channel ids and valid recipient ids from recent traffic. ",
            "Example: {\"channel\":\"wechat:personal\",\"recipient\":\"wxid_abc\",\"content\":\"hello\",\"thread\":null}. ",
            "Relevant tools: list_im_channels."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: send_im_message_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let channel = args.channel.trim().to_string();
        if channel.is_empty() {
            return Err("channel must not be empty".into());
        }
        let recipient = args.recipient.trim().to_string();
        if recipient.is_empty() {
            return Err("recipient must not be empty".into());
        }
        if args.content.trim().is_empty() && resources.is_empty() {
            return Err("content must not be empty".into());
        }
        let thread = args
            .thread
            .map(|thread| thread.trim().to_string())
            .filter(|thread| !thread.is_empty());

        let meta = ctx
            .get_state::<SessionRequestMeta>()
            .map(|state| state.get())
            .unwrap_or_else(|| ctx.meta().clone());
        let conversation =
            request_meta_extra_as::<u64>(&meta, "conversation").filter(|conv_id| *conv_id > 0);

        let message = SendMessage::new(args.content, recipient.clone())
            .in_thread(thread.clone())
            .with_attachments(resources);
        self.sender.send(&channel, message, conversation).await?;

        Ok(ToolOutput::new(Response::Ok {
            result: json!({
                "sent": true,
                "channel": channel,
                "recipient": recipient,
                "thread": thread,
            }),
            next_cursor: None,
        }))
    }
}

/// Lists configured IM channels and recently active recipients so the agent
/// can resolve valid send targets.
#[derive(Clone)]
pub struct ListImChannelsTool {
    sender: ChannelSender,
}

impl ListImChannelsTool {
    pub const NAME: &'static str = "list_im_channels";

    pub fn new(sender: ChannelSender) -> Self {
        Self { sender }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ListImChannelsArgs {
    pub channel: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_usize_from_number_or_string"
    )]
    pub limit: Option<usize>,
}

impl Tool<BaseCtx> for ListImChannelsTool {
    type Args = ListImChannelsArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Lists the configured IM channel ids and recently active recipients on them. ",
            "Recipients are aggregated from recent channel traffic, newest first; ",
            "use their recipient ids with send_im_message. ",
            "Relevant tools: send_im_message."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: list_im_channels_parameters(),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let channels = self.sender.channels();
        let channel = args
            .channel
            .map(|channel| channel.trim().to_string())
            .filter(|channel| !channel.is_empty());
        if let Some(channel) = &channel
            && !channels.contains(channel)
        {
            return Err(format!(
                "channel {} not found, configured channels: {}",
                channel,
                channels.join(", ")
            )
            .into());
        }

        let recipients = self
            .sender
            .recent_recipients(channel.as_deref(), args.limit.unwrap_or(20))
            .await?;

        Ok(ToolOutput::new(Response::Ok {
            result: json!({
                "channels": channels,
                "recent_recipients": recipients,
            }),
            next_cursor: None,
        }))
    }
}

fn send_im_message_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "channel": {
                "type": "string",
                "description": "The configured channel id to send through, e.g. 'wechat:personal' or 'telegram:ops'. Use list_im_channels to discover valid ids."
            },
            "recipient": {
                "type": "string",
                "description": "The platform recipient id (Telegram chat id, WeChat wxid, Discord channel id, Lark chat id). Use a recipient id seen in list_im_channels."
            },
            "content": {
                "type": "string",
                "description": "The message text to send."
            },
            "thread": {
                "type": ["string", "null"],
                "description": "Optional platform thread identifier for threaded replies, or null to send without a thread."
            }
        },
        "required": ["channel", "recipient", "content", "thread"],
        "additionalProperties": false
    })
}

fn list_im_channels_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "channel": {
                "type": ["string", "null"],
                "description": "Optional channel id to restrict recent recipients to, or null for all channels."
            },
            "limit": {
                "type": ["integer", "string", "null"],
                "description": "Maximum number of recent recipients to return. Numeric strings are accepted. Defaults to 20 and is capped at 100."
            }
        },
        "required": ["channel", "limit"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{Channel, ChannelMessage, ChannelRuntime};
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::{Principal, RequestMeta};
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
        unix_ms,
    };
    use anda_engine::engine::{EngineBuilder, EngineRef};
    use async_trait::async_trait;
    use object_store::{ObjectStore, memory::InMemory};
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::{Mutex as AsyncMutex, mpsc};
    use tokio_util::sync::CancellationToken;

    struct RecordingChannel {
        id: String,
        sent_messages: AsyncMutex<Vec<SendMessage>>,
    }

    impl RecordingChannel {
        fn new(id: impl Into<String>) -> Self {
            Self {
                id: id.into(),
                sent_messages: AsyncMutex::new(Vec::new()),
            }
        }

        async fn sent_messages(&self) -> Vec<SendMessage> {
            self.sent_messages.lock().await.clone()
        }
    }

    #[async_trait]
    impl Channel for RecordingChannel {
        fn name(&self) -> &str {
            "test"
        }

        fn username(&self) -> &str {
            "anda-bot"
        }

        fn id(&self) -> String {
            self.id.clone()
        }

        async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
            self.sent_messages.lock().await.push(message.clone());
            Ok(())
        }

        async fn listen(
            &self,
            cancel_token: CancellationToken,
            _tx: mpsc::Sender<ChannelMessage>,
        ) -> Result<(), BoxError> {
            cancel_token.cancelled().await;
            Ok(())
        }
    }

    async fn test_sender(channel: Arc<RecordingChannel>) -> ChannelSender {
        let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let db = AndaDB::connect(
            object_store,
            DBConfig {
                name: format!("channel_tools_test_{}", unix_ms()),
                description: "channel tools test db".to_string(),
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
        .unwrap();

        let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();
        channels.insert(channel.id(), channel);

        ChannelRuntime::connect(
            Arc::new(db),
            Arc::new(EngineRef::new()),
            Principal::management_canister(),
            HashMap::new(),
            channels,
            std::env::temp_dir(),
        )
        .await
        .unwrap()
        .sender()
    }

    #[test]
    fn im_tool_schemas_are_openai_strict() {
        for parameters in [send_im_message_parameters(), list_im_channels_parameters()] {
            assert_openai_strict_parameters(&parameters);
        }
    }

    #[test]
    fn list_im_channels_args_accept_numeric_strings() {
        let args: ListImChannelsArgs = serde_json::from_value(json!({
            "channel": null,
            "limit": "25"
        }))
        .unwrap();
        assert_eq!(args.channel, None);
        assert_eq!(args.limit, Some(25));
    }

    fn result_of(output: ToolOutput<Response>) -> Value {
        match output.output {
            Response::Ok { result, .. } => result,
            other => panic!("expected ok response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_im_message_delivers_through_named_channel() {
        let channel = Arc::new(RecordingChannel::new("wechat:test"));
        let sender = test_sender(channel.clone()).await;
        let tool = SendImMessageTool::new(sender.clone());
        let ctx = EngineBuilder::new().mock_ctx().base;
        let mut extra = serde_json::Map::new();
        extra.insert("conversation".to_string(), 42.into());
        ctx.set_state(SessionRequestMeta::new(RequestMeta {
            extra,
            ..Default::default()
        }));

        let result = result_of(
            tool.call(
                ctx,
                SendImMessageArgs {
                    channel: " wechat:test ".to_string(),
                    recipient: " wxid_alice ".to_string(),
                    content: "hello from anywhere".to_string(),
                    thread: Some("  ".to_string()),
                },
                Vec::new(),
            )
            .await
            .unwrap(),
        );

        assert_eq!(result["sent"], true);
        assert_eq!(result["channel"], "wechat:test");
        assert_eq!(result["recipient"], "wxid_alice");
        assert_eq!(result["thread"], Value::Null);

        let sent_messages = channel.sent_messages().await;
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].recipient, "wxid_alice");
        assert_eq!(sent_messages[0].content, "hello from anywhere");
        assert_eq!(sent_messages[0].thread, None);

        // The sent message is recorded and surfaces as a recent recipient.
        let recipients = sender.recent_recipients(None, 10).await.unwrap();
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].channel, "wechat:test");
        assert_eq!(recipients[0].recipient, "wxid_alice");
        assert_eq!(recipients[0].last_sender, "anda-bot");
    }

    #[tokio::test]
    async fn send_im_message_rejects_unknown_channel_and_empty_args() {
        let channel = Arc::new(RecordingChannel::new("wechat:test"));
        let sender = test_sender(channel.clone()).await;
        let tool = SendImMessageTool::new(sender);
        let ctx = EngineBuilder::new().mock_ctx().base;

        let err = tool
            .call(
                ctx.clone(),
                SendImMessageArgs {
                    channel: "telegram:missing".to_string(),
                    recipient: "alice".to_string(),
                    content: "hi".to_string(),
                    thread: None,
                },
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));

        for args in [
            SendImMessageArgs {
                channel: "".to_string(),
                recipient: "alice".to_string(),
                content: "hi".to_string(),
                thread: None,
            },
            SendImMessageArgs {
                channel: "wechat:test".to_string(),
                recipient: " ".to_string(),
                content: "hi".to_string(),
                thread: None,
            },
            SendImMessageArgs {
                channel: "wechat:test".to_string(),
                recipient: "alice".to_string(),
                content: "  ".to_string(),
                thread: None,
            },
        ] {
            assert!(tool.call(ctx.clone(), args, Vec::new()).await.is_err());
        }

        assert!(channel.sent_messages().await.is_empty());
    }

    #[tokio::test]
    async fn list_im_channels_returns_channels_and_recent_recipients() {
        let channel = Arc::new(RecordingChannel::new("wechat:test"));
        let sender = test_sender(channel).await;
        sender
            .send("wechat:test", SendMessage::new("ping", "wxid_alice"), None)
            .await
            .unwrap();
        let tool = ListImChannelsTool::new(sender);
        let ctx = EngineBuilder::new().mock_ctx().base;

        let result = result_of(
            tool.call(ctx.clone(), ListImChannelsArgs::default(), Vec::new())
                .await
                .unwrap(),
        );
        assert_eq!(result["channels"], json!(["wechat:test"]));
        assert_eq!(result["recent_recipients"][0]["recipient"], "wxid_alice");

        let err = tool
            .call(
                ctx,
                ListImChannelsArgs {
                    channel: Some("discord:missing".to_string()),
                    limit: None,
                },
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn im_tool_metadata_exposes_names_and_strict_schemas() {
        let channel = Arc::new(RecordingChannel::new("wechat:test"));
        let sender = test_sender(channel).await;

        let send_tool = SendImMessageTool::new(sender.clone());
        let list_tool = ListImChannelsTool::new(sender);
        assert_eq!(send_tool.name(), "send_im_message");
        assert_eq!(list_tool.name(), "list_im_channels");

        for definition in [send_tool.definition(), list_tool.definition()] {
            assert_eq!(definition.strict, Some(true));
            assert!(!definition.description.is_empty());
        }
    }
}
