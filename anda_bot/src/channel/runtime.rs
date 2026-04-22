use anda_core::{AgentInput, AgentOutput, BoxError, Principal, RequestMeta};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    index::jieba_tokenizer,
    unix_ms,
};
use anda_engine::{context::AgentCtx, engine::EngineRef};
use anda_kip::Map;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::types::*;
use crate::engine::CompletionHook;

pub struct ChannelRuntime {
    rx: tokio::sync::mpsc::Receiver<ChannelMessage>,
    inner: Arc<ChannelRuntimeInner>,
}

struct ChannelRuntimeInner {
    engine: Arc<EngineRef>,
    user: Principal,
    tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    channels: HashMap<String, Arc<dyn Channel>>,
    channels_conversation: RwLock<HashMap<(String, String), u64>>, // (channel, reply_target) -> conversation_id
    messages: Arc<Collection>,
}

impl ChannelRuntime {
    pub async fn connect(
        db: Arc<AndaDB>,
        engine: Arc<EngineRef>,
        user: Principal,
        channels: HashMap<String, Arc<dyn Channel>>,
    ) -> Result<Self, BoxError> {
        let (tx, rx) = tokio::sync::mpsc::channel(21);
        let schema = ChannelMessage::schema()?;
        let messages = db
            .open_or_create_collection(
                schema,
                CollectionConfig {
                    name: "channel_messages".to_string(),
                    description: "channel messages collection".to_string(),
                },
                async |collection| {
                    // set tokenizer
                    collection.set_tokenizer(jieba_tokenizer());
                    // create BTree indexes if not exists
                    collection.create_btree_index_nx(&["sender"]).await?;
                    collection.create_btree_index_nx(&["reply_target"]).await?;
                    collection.create_btree_index_nx(&["channel"]).await?;
                    collection.create_btree_index_nx(&["conversation"]).await?;
                    collection
                        .create_bm25_index_nx(&["content", "attachments", "extra"])
                        .await?;

                    Ok::<(), DBError>(())
                },
            )
            .await?;
        let channels_conversation: HashMap<(String, String), u64> = messages
            .get_extension_as("channels_conversation")
            .unwrap_or_default();

        let inner = Arc::new(ChannelRuntimeInner {
            engine,
            user,
            tx,
            channels,
            channels_conversation: RwLock::new(channels_conversation),
            messages,
        });

        Ok(Self { rx, inner })
    }

    pub fn hook(&self) -> Arc<dyn CompletionHook> {
        Arc::new(self.inner.clone())
    }

    pub async fn serve(
        self,
        cancel_token: CancellationToken,
    ) -> Result<JoinHandle<Result<(), BoxError>>, BoxError> {
        Ok(tokio::spawn(async move {
            log::warn!(name = "channel"; "channel runtime started");
            let messages = self.inner.messages.clone();
            let rx_token = cancel_token.child_token();
            let inner = self.inner.clone();
            let rx_handle = tokio::spawn(async move {
                let mut rx = self.rx;
                while let Some(mut message) = tokio::select! {
                    _ = rx_token.cancelled() => {
                        log::warn!(name = "channel"; "channel runtime receiver stopped");
                        None
                    }
                    message = rx.recv() => {
                        if message.is_none() {
                            log::warn!(name = "channel"; "channel runtime receiver closed");
                        }
                        message
                    },
                } {
                    let _ = messages.flush(unix_ms()).await;
                    if let Some(engine) = self.inner.engine.get() {
                        let mut extra = Map::new();
                        let key = (message.channel.clone(), message.reply_target.clone());
                        let conv_id = {
                            self.inner
                                .channels_conversation
                                .read()
                                .get(&key)
                                .copied()
                                .unwrap_or(0)
                        };
                        extra.insert("conversation".to_string(), conv_id.into());
                        extra.insert("source".to_string(), message.channel.clone().into());
                        extra.insert(
                            "reply_target".to_string(),
                            message.reply_target.clone().into(),
                        );
                        match engine
                            .agent_run(
                                self.inner.user,
                                AgentInput {
                                    name: String::new(),
                                    prompt: message.content.clone(),
                                    resources: message.attachments.clone(),
                                    meta: Some(RequestMeta {
                                        user: Some(message.sender.clone()),
                                        extra,
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                },
                            )
                            .await
                        {
                            Ok(output) => {
                                message.conversation = output.conversation;
                                match messages.add_from(&message).await {
                                    Ok(id) => {
                                        message._id = id;
                                    }
                                    Err(err) => {
                                        log::error!(name = "channel"; "failed to add message to collection: {err}");
                                    }
                                }
                                if let Some(conv_id) = output.conversation {
                                    let mut channels_conversation =
                                        self.inner.channels_conversation.write();
                                    channels_conversation.insert(key, conv_id);
                                    messages.set_extension_from::<HashMap<(String, String), u64>>(
                                        "channels_conversation".to_string(),
                                        channels_conversation.clone(),
                                    );
                                }
                            }
                            Err(err) => {
                                log::error!(name = "channel"; "failed to process message from channel {}: {err}", message.channel);
                            }
                        };
                    } else {
                        log::warn!(name = "channel"; "engine is not available, skipping incoming message");
                    }
                }
            });

            let mut handles: Vec<JoinHandle<()>> = vec![rx_handle];
            handles.extend(inner.channels.values().map(|channel| {
                let tx = inner.tx.clone();
                let cancel_token = cancel_token.child_token();
                let channel = channel.clone();
                tokio::spawn(async move {
                    if let Err(err) = channel.listen(cancel_token, tx).await {
                        log::error!(name = "channel"; "channel {} failed with error: {err}", channel.name());
                    }
                })
            }));

            let _ = futures::future::join_all(handles).await;

            Ok(())
        }))
    }
}

impl ChannelRuntimeInner {
    async fn try_send(
        &self,
        channel: String,
        message: SendMessage,
        conv_id: Option<u64>,
    ) -> Result<(), BoxError> {
        if let Some(chan) = self.channels.get(&channel) {
            if let Err(err) = self
                .messages
                .add_from(&ChannelMessage {
                    sender: chan.username().to_string(),
                    reply_target: message.recipient.clone(),
                    content: message.content.clone(),
                    channel: channel.clone(),
                    timestamp: unix_ms(),
                    thread: message.thread.clone(),
                    attachments: message.attachments.clone(),
                    conversation: conv_id,
                    ..Default::default()
                })
                .await
            {
                log::error!(name = "channel"; "failed to add message to collection: {err}");
            }

            if let Err(err) = chan.send(&message).await {
                log::error!(name = "channel"; "failed to send message to channel {}: {err}", channel);
            }

            Ok(())
        } else {
            Err(format!("channel {} not found", channel).into())
        }
    }
}

#[async_trait]
impl CompletionHook for Arc<ChannelRuntimeInner> {
    async fn on_completion(&self, _ctx: &AgentCtx, output: &AgentOutput) {
        if let Some(conv_id) = output.conversation
            && !output.content.is_empty()
            && let Some((channel, reply_target)) = {
                let channels_conversation = self.channels_conversation.read();
                channels_conversation
                    .iter()
                    .find_map(|((chan, target), &id)| {
                        if id == conv_id {
                            Some((chan.clone(), target.clone()))
                        } else {
                            None
                        }
                    })
            } {
                let msg = SendMessage {
                    content: output.content.clone(),
                    recipient: reply_target,
                    subject: None,
                    thread: None,
                    attachments: output.artifacts.clone(),
                };

                tokio::spawn({
                    let this = self.clone();
                    async move {
                        if let Err(err) = this.try_send(channel.clone(), msg, Some(conv_id)).await {
                            log::error!(name = "channel"; "failed to send message to channel {}: {err}", channel);
                        }
                    }
                });
            }
    }
}
