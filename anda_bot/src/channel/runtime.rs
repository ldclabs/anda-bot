use anda_core::{AgentInput, AgentOutput, BoxError, Principal, RequestMeta, StateFeatures};
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
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::types::*;
use crate::engine::{CompletionHook, external_user_prompt};

type ChannelConversationMap = HashMap<(String, String, Option<String>), u64>;

const CHANNEL_RECONNECT_BASE_DELAY: Duration = Duration::from_secs(2);
const CHANNEL_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(60);
const CHANNEL_RECONNECT_RESET_AFTER: Duration = Duration::from_secs(300);
const CHANNEL_SEND_RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const CHANNEL_SEND_RETRY_MAX_DELAY: Duration = Duration::from_secs(5);
const CHANNEL_SEND_RETRY_MAX_ATTEMPTS: u32 = 6;

#[derive(Debug, Clone, Copy)]
struct ChannelReconnectPolicy {
    base_delay: Duration,
    max_delay: Duration,
    reset_after: Duration,
}

impl Default for ChannelReconnectPolicy {
    fn default() -> Self {
        Self {
            base_delay: CHANNEL_RECONNECT_BASE_DELAY,
            max_delay: CHANNEL_RECONNECT_MAX_DELAY,
            reset_after: CHANNEL_RECONNECT_RESET_AFTER,
        }
    }
}

impl ChannelReconnectPolicy {
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(5);
        let factor = 1_u32 << shift;
        self.base_delay
            .checked_mul(factor)
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }
}

#[derive(Debug, Clone, Copy)]
struct ChannelSendRetryPolicy {
    base_delay: Duration,
    max_delay: Duration,
    max_attempts: u32,
}

impl Default for ChannelSendRetryPolicy {
    fn default() -> Self {
        Self {
            base_delay: CHANNEL_SEND_RETRY_BASE_DELAY,
            max_delay: CHANNEL_SEND_RETRY_MAX_DELAY,
            max_attempts: CHANNEL_SEND_RETRY_MAX_ATTEMPTS,
        }
    }
}

impl ChannelSendRetryPolicy {
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(4);
        let factor = 1_u32 << shift;
        self.base_delay
            .checked_mul(factor)
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelRoute {
    channel: String,
    reply_target: String,
    thread: Option<String>,
}

impl ChannelRoute {
    fn from_message(message: &ChannelMessage) -> Self {
        Self {
            channel: message.channel.clone(),
            reply_target: message.reply_target.clone(),
            thread: message.thread.clone(),
        }
    }

    fn key(&self) -> (String, String, Option<String>) {
        (
            self.channel.clone(),
            self.reply_target.clone(),
            self.thread.clone(),
        )
    }
}

pub struct ChannelRuntime {
    rx: tokio::sync::mpsc::Receiver<ChannelMessage>,
    inner: Arc<ChannelRuntimeInner>,
}

struct ChannelRuntimeInner {
    engine: Arc<EngineRef>,
    user: Principal,
    tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    channels: HashMap<String, Arc<dyn Channel>>,
    channels_conversation: RwLock<ChannelConversationMap>, // (channel, reply_target, thread) -> conversation_id
    conversation_routes: RwLock<HashMap<u64, ChannelRoute>>, // conversation_id -> route
    messages: Arc<Collection>,
    work_dir: PathBuf,
}

impl ChannelRuntime {
    pub async fn connect(
        db: Arc<AndaDB>,
        engine: Arc<EngineRef>,
        user: Principal,
        channels: HashMap<String, Arc<dyn Channel>>,
        work_dir: PathBuf,
    ) -> Result<Self, BoxError> {
        let (tx, rx) = tokio::sync::mpsc::channel(21);
        let mut schema = ChannelMessage::schema()?;
        schema.with_version(2);
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
        let channels_conversation = messages
            .get_extension_as::<ChannelConversationMap>("channels_conversation")
            .unwrap_or_default();
        let conversation_routes = build_conversation_routes(&channels_conversation);
        for (channel_name, channel) in &channels {
            let path = work_dir.join(channel_name);
            if let Err(err) = tokio::fs::create_dir_all(&path).await {
                log::warn!("failed to create workspace for {}: {err}", channel_name);
            }
            channel.set_workspace(path);
        }

        let inner = Arc::new(ChannelRuntimeInner {
            engine,
            user,
            tx,
            channels,
            channels_conversation: RwLock::new(channels_conversation),
            conversation_routes: RwLock::new(conversation_routes),
            messages,
            work_dir,
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
                    log::debug!(
                        channel = message.channel,
                        message:serde = message;
                        "received message from channel {}",
                        message.channel
                    );
                    let _ = messages.flush(unix_ms()).await;
                    if let Some(engine) = self.inner.engine.get() {
                        let mut extra = Map::new();
                        let route = ChannelRoute::from_message(&message);
                        let key = route.key();
                        let conv_id = {
                            self.inner
                                .channels_conversation
                                .read()
                                .get(&key)
                                .copied()
                                .unwrap_or(0)
                        };
                        extra.insert("conversation".to_string(), conv_id.into());
                        extra.insert(
                            "workspace".to_string(),
                            self.inner
                                .work_dir
                                .join(&message.channel)
                                .to_string_lossy()
                                .into(),
                        );
                        extra.insert("source".to_string(), message.channel.clone().into());
                        extra.insert(
                            "reply_target".to_string(),
                            message.reply_target.clone().into(),
                        );
                        if let Some(thread) = &message.thread
                            && !thread.is_empty()
                        {
                            extra.insert("thread".to_string(), thread.clone().into());
                        }

                        extra.insert("external_user".to_string(), message.external_user.into());
                        let prompt = agent_prompt_from_message(&message);
                        match engine
                            .agent_run(
                                self.inner.user,
                                AgentInput {
                                    name: String::new(),
                                    prompt,
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
                                if let Some(conv_id) = output.conversation
                                    && let Some(channels_conversation) =
                                        self.inner.bind_conversation(route, conv_id)
                                {
                                    messages.set_extension_from::<ChannelConversationMap>(
                                        "channels_conversation".to_string(),
                                        channels_conversation,
                                    );
                                }

                                let _ = messages.flush(unix_ms()).await;
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

            if inner.channels.is_empty() {
                log::warn!(name = "channel"; "no channels configured; channel runtime will stay idle");
            }

            let mut handles: Vec<JoinHandle<()>> = vec![rx_handle];
            handles.extend(inner.channels.values().map(|channel| {
                let tx = inner.tx.clone();
                let cancel_token = cancel_token.child_token();
                let channel = channel.clone();
                tokio::spawn(async move {
                    serve_channel_with_reconnect(
                        channel,
                        tx,
                        cancel_token,
                        ChannelReconnectPolicy::default(),
                    )
                    .await;
                })
            }));

            let _ = futures::future::join_all(handles).await;

            Ok(())
        }))
    }
}

impl ChannelRuntimeInner {
    fn bind_conversation(
        &self,
        route: ChannelRoute,
        conv_id: u64,
    ) -> Option<ChannelConversationMap> {
        let key = route.key();
        let (previous, snapshot) = {
            let mut channels_conversation = self.channels_conversation.write();
            let previous = channels_conversation.insert(key, conv_id);
            if previous == Some(conv_id) {
                return None; // no change
            }
            (previous, channels_conversation.clone())
        };

        let mut conversation_routes = self.conversation_routes.write();
        if let Some(previous) = previous
            && previous != conv_id
            && conversation_routes.get(&previous) == Some(&route)
        {
            conversation_routes.remove(&previous);
        }
        conversation_routes.insert(conv_id, route);

        Some(snapshot)
    }

    fn route_for_conversation(&self, conv_id: u64) -> Option<ChannelRoute> {
        self.conversation_routes.read().get(&conv_id).cloned()
    }

    fn route_from_meta(&self, meta: &RequestMeta) -> Option<ChannelRoute> {
        let channel = meta
            .get_extra_as::<String>("source")
            .and_then(|value| normalize_non_empty(value.as_str()))?;
        if !self.channels.contains_key(&channel) {
            return None;
        }

        let reply_target = meta
            .get_extra_as::<String>("reply_target")
            .and_then(|value| normalize_non_empty(value.as_str()))?;
        let thread = meta
            .get_extra_as::<String>("thread")
            .and_then(|value| normalize_non_empty(value.as_str()));

        Some(ChannelRoute {
            channel,
            reply_target,
            thread,
        })
    }

    async fn try_send(
        &self,
        channel: String,
        message: SendMessage,
        conv_id: Option<u64>,
    ) -> Result<(), BoxError> {
        if let Some(chan) = self.channels.get(&channel) {
            send_message_with_retry(&channel, chan, &message, ChannelSendRetryPolicy::default())
                .await?;

            let timestamp = unix_ms();
            self.messages
                .add_from(&ChannelMessage {
                    sender: chan.username().to_string(),
                    reply_target: message.recipient.clone(),
                    content: message.content.clone(),
                    channel: channel.clone(),
                    timestamp,
                    thread: message.thread.clone(),
                    attachments: message.attachments.clone(),
                    conversation: conv_id,
                    ..Default::default()
                })
                .await?;
            self.messages.flush(timestamp).await?;

            Ok(())
        } else {
            Err(format!("channel {} not found", channel).into())
        }
    }
}

#[async_trait]
impl CompletionHook for Arc<ChannelRuntimeInner> {
    async fn on_completion(&self, ctx: &AgentCtx, output: &AgentOutput) {
        let Some(conv_id) = output.conversation else {
            return;
        };
        if output.content.is_empty() {
            return;
        }

        let route = match self.route_for_conversation(conv_id) {
            Some(route) => route,
            None => {
                let Some(route) = self.route_from_meta(ctx.meta()) else {
                    return;
                };
                if let Some(channels_conversation) = self.bind_conversation(route.clone(), conv_id)
                {
                    self.messages.set_extension_from::<ChannelConversationMap>(
                        "channels_conversation".to_string(),
                        channels_conversation,
                    );
                    if let Err(err) = self.messages.flush(unix_ms()).await {
                        log::error!(name = "channel"; "failed to flush channel route binding: {err}");
                    }
                }
                route
            }
        };

        let channel = route.channel.clone();
        let msg = completion_message(output, route);

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

fn build_conversation_routes(
    channels_conversation: &ChannelConversationMap,
) -> HashMap<u64, ChannelRoute> {
    let mut conversation_routes = HashMap::new();
    for ((channel, reply_target, thread), &conversation) in channels_conversation {
        conversation_routes.insert(
            conversation,
            ChannelRoute {
                channel: channel.clone(),
                reply_target: reply_target.clone(),
                thread: thread.clone(),
            },
        );
    }
    conversation_routes
}

fn normalize_non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn completion_message(output: &AgentOutput, route: ChannelRoute) -> SendMessage {
    SendMessage::new(output.content.clone(), route.reply_target)
        .in_thread(route.thread)
        .with_attachments(output.artifacts.clone())
}

fn agent_prompt_from_message(message: &ChannelMessage) -> String {
    if message.external_user.unwrap_or_default() {
        external_user_prompt(&message.channel, &message.sender, &message.content)
    } else {
        message.content.clone()
    }
}

async fn serve_channel_with_reconnect(
    channel: Arc<dyn Channel>,
    tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    cancel_token: CancellationToken,
    policy: ChannelReconnectPolicy,
) {
    let mut attempt = 0_u32;

    loop {
        if cancel_token.is_cancelled() {
            log::warn!(name = "channel"; "channel {} listener stopped", channel.name());
            return;
        }

        if tx.is_closed() {
            log::warn!(name = "channel"; "channel {} listener stopped because receiver is closed", channel.name());
            return;
        }

        let started_at = Instant::now();
        let result = channel.listen(cancel_token.clone(), tx.clone()).await;

        if cancel_token.is_cancelled() {
            log::warn!(name = "channel"; "channel {} listener stopped", channel.name());
            return;
        }

        if tx.is_closed() {
            log::warn!(name = "channel"; "channel {} listener stopped because receiver is closed", channel.name());
            return;
        }

        if started_at.elapsed() >= policy.reset_after {
            attempt = 0;
        }
        attempt = attempt.saturating_add(1);
        let delay = policy.delay_for_attempt(attempt);

        match result {
            Ok(()) => {
                log::warn!(name = "channel"; "channel {} listener exited unexpectedly, reconnecting in {:?}", channel.name(), delay);
            }
            Err(err) => {
                log::error!(name = "channel"; "channel {} failed with error: {err}; reconnecting in {:?}", channel.name(), delay);
            }
        }

        tokio::select! {
            _ = cancel_token.cancelled() => {
                log::warn!(name = "channel"; "channel {} reconnect loop cancelled", channel.name());
                return;
            }
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

async fn send_message_with_retry(
    channel_key: &str,
    channel: &Arc<dyn Channel>,
    message: &SendMessage,
    policy: ChannelSendRetryPolicy,
) -> Result<(), BoxError> {
    let mut attempt = 0_u32;

    loop {
        attempt = attempt.saturating_add(1);

        match channel.send(message).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                let error_text = err.to_string();
                let retryable =
                    channel.should_retry_send(&error_text) || !channel.health_check().await;

                if !retryable || attempt >= policy.max_attempts {
                    return Err(err);
                }

                let delay = policy.delay_for_attempt(attempt);
                log::warn!(
                    name = "channel";
                    "retrying send to channel {} after transient error: {} (attempt {}/{}, in {:?})",
                    channel_key,
                    error_text,
                    attempt,
                    policy.max_attempts,
                    delay,
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
    };
    use object_store::{ObjectStore, memory::InMemory};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;
    use tokio::sync::{Mutex as AsyncMutex, mpsc};

    struct TestChannel {
        id: String,
        username: String,
        fail_send: bool,
        transient_send_failures: AsyncMutex<usize>,
        retryable_send_errors: bool,
        fail_listen_times: usize,
        listen_attempts: AtomicUsize,
        send_attempts: AtomicUsize,
        listen_ready: Notify,
        sent_messages: AsyncMutex<Vec<SendMessage>>,
    }

    impl TestChannel {
        fn new(id: impl Into<String>, fail_send: bool) -> Self {
            Self {
                id: id.into(),
                username: "anda-bot".to_string(),
                fail_send,
                transient_send_failures: AsyncMutex::new(0),
                retryable_send_errors: false,
                fail_listen_times: 0,
                listen_attempts: AtomicUsize::new(0),
                send_attempts: AtomicUsize::new(0),
                listen_ready: Notify::new(),
                sent_messages: AsyncMutex::new(Vec::new()),
            }
        }

        fn with_transient_send_failures(
            mut self,
            transient_send_failures: usize,
            retryable_send_errors: bool,
        ) -> Self {
            self.transient_send_failures = AsyncMutex::new(transient_send_failures);
            self.retryable_send_errors = retryable_send_errors;
            self
        }

        fn with_listen_failures(mut self, fail_listen_times: usize) -> Self {
            self.fail_listen_times = fail_listen_times;
            self
        }

        async fn sent_messages(&self) -> Vec<SendMessage> {
            self.sent_messages.lock().await.clone()
        }

        fn listen_attempts(&self) -> usize {
            self.listen_attempts.load(Ordering::SeqCst)
        }

        fn send_attempts(&self) -> usize {
            self.send_attempts.load(Ordering::SeqCst)
        }

        async fn wait_until_listening(&self) {
            self.listen_ready.notified().await;
        }
    }

    #[async_trait]
    impl Channel for TestChannel {
        fn name(&self) -> &str {
            "test"
        }

        fn username(&self) -> &str {
            &self.username
        }

        fn id(&self) -> String {
            self.id.clone()
        }

        async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
            self.send_attempts.fetch_add(1, Ordering::SeqCst);

            if self.fail_send {
                return Err("send failed".into());
            }

            let mut transient_send_failures = self.transient_send_failures.lock().await;
            if *transient_send_failures > 0 {
                *transient_send_failures -= 1;
                return Err("transient send failed".into());
            }

            self.sent_messages.lock().await.push(message.clone());
            Ok(())
        }

        fn should_retry_send(&self, error: &str) -> bool {
            self.retryable_send_errors && error.contains("transient send failed")
        }

        async fn listen(
            &self,
            cancel_token: CancellationToken,
            _tx: mpsc::Sender<ChannelMessage>,
        ) -> Result<(), BoxError> {
            let attempt = self.listen_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt <= self.fail_listen_times {
                return Err(format!("listen failed on attempt {attempt}").into());
            }

            self.listen_ready.notify_one();
            cancel_token.cancelled().await;
            Ok(())
        }
    }

    async fn test_runtime(channel: Arc<TestChannel>) -> ChannelRuntime {
        let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let db = AndaDB::connect(
            object_store,
            DBConfig {
                name: format!("channel_runtime_test_{}", unix_ms()),
                description: "channel runtime test db".to_string(),
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

        let mut channels = HashMap::new();
        let channel_id = channel.id();
        let channel_impl: Arc<dyn Channel> = channel.clone();
        channels.insert(channel_id, channel_impl);

        ChannelRuntime::connect(
            Arc::new(db),
            Arc::new(EngineRef::new()),
            Principal::management_canister(),
            channels,
            std::env::temp_dir(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn bind_conversation_tracks_threaded_routes() {
        let channel = Arc::new(TestChannel::new("test:threaded", false));
        let runtime = test_runtime(channel.clone()).await;
        let route = ChannelRoute {
            channel: channel.id(),
            reply_target: "#anda".to_string(),
            thread: Some("thread-1".to_string()),
        };

        let snapshot = runtime.inner.bind_conversation(route.clone(), 42).unwrap();

        assert_eq!(
            snapshot.get(&(
                route.channel.clone(),
                route.reply_target.clone(),
                route.thread.clone()
            )),
            Some(&42)
        );
        assert_eq!(runtime.inner.route_for_conversation(42), Some(route));
    }

    #[tokio::test]
    async fn route_from_meta_recovers_channel_reply_context() {
        let channel = Arc::new(TestChannel::new("test:meta-route", false));
        let runtime = test_runtime(channel.clone()).await;
        let mut extra = serde_json::Map::new();
        extra.insert("source".to_string(), channel.id().into());
        extra.insert("reply_target".to_string(), "#anda".into());
        extra.insert("thread".to_string(), "thread-1".into());

        let route = runtime
            .inner
            .route_from_meta(&RequestMeta {
                extra,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(route.channel, channel.id());
        assert_eq!(route.reply_target, "#anda");
        assert_eq!(route.thread, Some("thread-1".to_string()));
    }

    #[test]
    fn completion_message_preserves_thread_context() {
        let route = ChannelRoute {
            channel: "test:threaded".to_string(),
            reply_target: "#anda".to_string(),
            thread: Some("thread-1".to_string()),
        };
        let output = AgentOutput {
            content: "hello from anda".to_string(),
            conversation: Some(42),
            ..Default::default()
        };

        let message = completion_message(&output, route.clone());

        assert_eq!(message.content, output.content);
        assert_eq!(message.recipient, route.reply_target);
        assert_eq!(message.thread, route.thread);
        assert!(message.attachments.is_empty());
    }

    #[test]
    fn agent_prompt_preserves_trusted_channel_message() {
        let message = ChannelMessage {
            sender: "alice".to_string(),
            channel: "telegram:personal".to_string(),
            content: "hello".to_string(),
            ..Default::default()
        };

        assert_eq!(agent_prompt_from_message(&message), "hello");
    }

    #[test]
    fn agent_prompt_marks_external_untrusted_channel_message() {
        let message = ChannelMessage {
            sender: "bob".to_string(),
            channel: "telegram:public".to_string(),
            content: "hello".to_string(),
            external_user: Some(true),
            ..Default::default()
        };

        let prompt = agent_prompt_from_message(&message);

        assert!(
            prompt.starts_with("[$external_user: channel=\"telegram:public\", sender=\"bob\"]")
        );
        assert!(prompt.contains("external untrusted IM user"));
        assert!(prompt.ends_with("hello"));
    }

    #[tokio::test]
    async fn try_send_propagates_channel_errors() {
        let channel = Arc::new(TestChannel::new("test:failing", true));
        let runtime = test_runtime(channel.clone()).await;

        let err = runtime
            .inner
            .try_send(channel.id(), SendMessage::new("hello", "#anda"), Some(42))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("send failed"));
        assert!(channel.sent_messages().await.is_empty());
    }

    #[tokio::test]
    async fn try_send_retries_transient_send_failures() {
        let channel = Arc::new(
            TestChannel::new("test:retry-send", false).with_transient_send_failures(2, true),
        );
        let runtime = test_runtime(channel.clone()).await;

        runtime
            .inner
            .try_send(channel.id(), SendMessage::new("hello", "#anda"), Some(42))
            .await
            .unwrap();

        assert_eq!(channel.send_attempts(), 3);
        assert_eq!(channel.sent_messages().await.len(), 1);
    }

    #[tokio::test]
    async fn serve_channel_reconnects_after_listen_failure() {
        let channel = Arc::new(TestChannel::new("test:reconnect", false).with_listen_failures(2));
        let channel_impl: Arc<dyn Channel> = channel.clone();
        let (tx, _rx) = mpsc::channel(4);
        let cancel_token = CancellationToken::new();

        let handle = tokio::spawn(serve_channel_with_reconnect(
            channel_impl,
            tx,
            cancel_token.clone(),
            ChannelReconnectPolicy {
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(20),
                reset_after: Duration::from_secs(60),
            },
        ));

        tokio::time::timeout(Duration::from_secs(1), channel.wait_until_listening())
            .await
            .expect("channel never reconnected");

        assert_eq!(channel.listen_attempts(), 3);

        cancel_token.cancel();
        handle.await.unwrap();
    }
}
