use anda_core::{AgentInput, AgentOutput, BoxError, Principal, RequestMeta, StateFeatures};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    index::jieba_tokenizer,
    query::{Filter, Query, RangeQuery},
    schema::Fv,
    unix_ms,
};
use anda_engine::{context::AgentCtx, engine::EngineRef};
use anda_kip::Map;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::Serialize;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::types::*;
use crate::engine::{
    CompletionHook, PromptCommand, SessionRequestMeta, external_user_prompt_with_space,
};
use crate::util::request_meta::request_meta_extra_as;

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

/// Computes an exponentially backed-off delay for the given 1-based `attempt`,
/// doubling `base_delay` up to `max_shift` times and clamping to `max_delay`.
fn exponential_backoff_delay(
    base_delay: Duration,
    max_delay: Duration,
    attempt: u32,
    max_shift: u32,
) -> Duration {
    let shift = attempt.saturating_sub(1).min(max_shift);
    let factor = 1_u32 << shift;
    base_delay
        .checked_mul(factor)
        .unwrap_or(max_delay)
        .min(max_delay)
}

impl ChannelReconnectPolicy {
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        exponential_backoff_delay(self.base_delay, self.max_delay, attempt, 5)
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
        exponential_backoff_delay(self.base_delay, self.max_delay, attempt, 4)
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
    default_user: Principal,
    channel_users: HashMap<String, Principal>,
    tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    channels: HashMap<String, Arc<dyn Channel>>,
    channels_conversation: RwLock<ChannelConversationMap>, // (channel, reply_target, thread) -> conversation_id
    conversation_routes: RwLock<HashMap<u64, ChannelRoute>>, // conversation_id -> route
    messages: Arc<Collection>,
    work_dir: PathBuf,
}

fn channel_workspace_path(work_dir: &Path, channel_id: &str) -> PathBuf {
    work_dir.join(channel_workspace_dir_name(channel_id))
}

fn legacy_channel_workspace_path(work_dir: &Path, channel_id: &str) -> PathBuf {
    work_dir.join(channel_id)
}

fn legacy_percent_encoded_channel_workspace_path(work_dir: &Path, channel_id: &str) -> PathBuf {
    work_dir.join(legacy_percent_encoded_channel_workspace_dir_name(
        channel_id,
    ))
}

async fn prepare_channel_workspace(work_dir: &Path, channel_id: &str) -> PathBuf {
    let path = channel_workspace_path(work_dir, channel_id);
    if let Err(err) = migrate_legacy_channel_workspace(work_dir, channel_id, &path).await {
        log::warn!(
            "failed to migrate legacy workspace for channel {}: {err}",
            channel_id
        );
    }
    if let Err(err) = tokio::fs::create_dir_all(&path).await {
        log::warn!("failed to create workspace for {}: {err}", channel_id);
    }
    path
}

async fn migrate_legacy_channel_workspace(
    work_dir: &Path,
    channel_id: &str,
    safe_path: &Path,
) -> io::Result<()> {
    let mut legacy_paths = Vec::new();
    if !cfg!(windows) {
        push_legacy_workspace_path(
            &mut legacy_paths,
            legacy_channel_workspace_path(work_dir, channel_id),
            safe_path,
        );
    }
    push_legacy_workspace_path(
        &mut legacy_paths,
        legacy_percent_encoded_channel_workspace_path(work_dir, channel_id),
        safe_path,
    );

    for legacy_path in legacy_paths {
        migrate_legacy_channel_workspace_path(channel_id, &legacy_path, safe_path).await?;
    }

    Ok(())
}

fn push_legacy_workspace_path(paths: &mut Vec<PathBuf>, path: PathBuf, safe_path: &Path) {
    if path != safe_path && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

async fn migrate_legacy_channel_workspace_path(
    channel_id: &str,
    legacy_path: &Path,
    safe_path: &Path,
) -> io::Result<()> {
    let legacy_meta = match tokio::fs::symlink_metadata(legacy_path).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if !legacy_meta.is_dir() {
        log::warn!(
            "legacy workspace for channel {} is not a directory: {}",
            channel_id,
            legacy_path.display()
        );
        return Ok(());
    }

    match tokio::fs::symlink_metadata(safe_path).await {
        Ok(meta) if meta.is_dir() => {
            merge_workspace_dirs(legacy_path, safe_path).await?;
        }
        Ok(_) => {
            log::warn!(
                "safe workspace for channel {} exists but is not a directory: {}",
                channel_id,
                safe_path.display()
            );
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            tokio::fs::rename(legacy_path, safe_path).await?;
        }
        Err(err) => return Err(err),
    }

    Ok(())
}

async fn merge_workspace_dirs(source: &Path, destination: &Path) -> io::Result<()> {
    enum Pending {
        Merge(PathBuf, PathBuf),
        Cleanup(PathBuf),
    }

    let mut pending = vec![Pending::Merge(
        source.to_path_buf(),
        destination.to_path_buf(),
    )];
    while let Some(item) = pending.pop() {
        match item {
            Pending::Merge(source_dir, destination_dir) => {
                tokio::fs::create_dir_all(&destination_dir).await?;
                pending.push(Pending::Cleanup(source_dir.clone()));
                let mut entries = tokio::fs::read_dir(&source_dir).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let source_path = entry.path();
                    let destination_path = destination_dir.join(entry.file_name());
                    let source_type = entry.file_type().await?;
                    match tokio::fs::symlink_metadata(&destination_path).await {
                        Ok(destination_meta)
                            if source_type.is_dir() && destination_meta.is_dir() =>
                        {
                            pending.push(Pending::Merge(source_path, destination_path));
                        }
                        Ok(_) => {
                            log::warn!(
                                "leaving legacy channel workspace entry in place because destination exists: {}",
                                destination_path.display()
                            );
                        }
                        Err(err) if err.kind() == io::ErrorKind::NotFound => {
                            tokio::fs::rename(&source_path, &destination_path).await?;
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            Pending::Cleanup(source_dir) => match tokio::fs::remove_dir(&source_dir).await {
                Ok(()) => {}
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(err) => return Err(err),
            },
        }
    }

    Ok(())
}

impl ChannelRuntime {
    pub async fn connect(
        db: Arc<AndaDB>,
        engine: Arc<EngineRef>,
        default_user: Principal,
        channel_users: HashMap<String, Principal>,
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
            let path = prepare_channel_workspace(&work_dir, channel_name).await;
            channel.set_workspace(path);
        }

        let inner = Arc::new(ChannelRuntimeInner {
            engine,
            default_user,
            channel_users,
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

    pub fn sender(&self) -> ChannelSender {
        ChannelSender {
            inner: self.inner.clone(),
        }
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
                        let new_command = channel_new_prompt_command(&message);
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
                            channel_workspace_path(&self.inner.work_dir, &message.channel)
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
                        let channel_user = self.inner.user_for_channel(&message.channel);
                        extra.insert("channel_user".to_string(), channel_user.to_text().into());
                        match engine
                            .agent_run(
                                channel_user,
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
                                match (new_command, output.conversation) {
                                    (Some(None), _) => {
                                        if let Some(channels_conversation) =
                                            self.inner.clear_route_conversation(&route)
                                        {
                                            messages.set_extension_from::<ChannelConversationMap>(
                                                "channels_conversation".to_string(),
                                                channels_conversation,
                                            );
                                        }
                                    }
                                    (_, Some(conv_id)) => {
                                        if let Some(channels_conversation) =
                                            self.inner.bind_conversation(route, conv_id)
                                        {
                                            messages.set_extension_from::<ChannelConversationMap>(
                                                "channels_conversation".to_string(),
                                                channels_conversation,
                                            );
                                        }
                                    }
                                    _ => {}
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
    fn user_for_channel(&self, channel: &str) -> Principal {
        self.channel_users
            .get(channel)
            .copied()
            .unwrap_or(self.default_user)
    }

    fn bind_conversation(
        &self,
        route: ChannelRoute,
        conv_id: u64,
    ) -> Option<ChannelConversationMap> {
        let key = route.key();
        let (_previous, snapshot) = {
            let mut channels_conversation = self.channels_conversation.write();
            let previous = channels_conversation.insert(key, conv_id);
            if previous == Some(conv_id) {
                return None; // no change
            }
            (previous, channels_conversation.clone())
        };

        let mut conversation_routes = self.conversation_routes.write();
        conversation_routes.insert(conv_id, route);

        Some(snapshot)
    }

    fn clear_route_conversation(&self, route: &ChannelRoute) -> Option<ChannelConversationMap> {
        let key = route.key();
        let (previous, snapshot) = {
            let mut channels_conversation = self.channels_conversation.write();
            let previous = channels_conversation.remove(&key)?;
            (previous, channels_conversation.clone())
        };

        self.conversation_routes
            .write()
            .entry(previous)
            .or_insert_with(|| route.clone());

        Some(snapshot)
    }

    fn current_conversation_for_route(&self, route: &ChannelRoute) -> Option<u64> {
        self.channels_conversation.read().get(&route.key()).copied()
    }

    fn route_for_conversation(&self, conv_id: u64) -> Option<ChannelRoute> {
        self.conversation_routes.read().get(&conv_id).cloned()
    }

    fn route_from_meta(&self, meta: &RequestMeta) -> Option<ChannelRoute> {
        let channel = request_meta_extra_as::<String>(meta, "source")
            .and_then(|value| normalize_non_empty(value.as_str()))?;
        if !self.channels.contains_key(&channel) {
            return None;
        }

        let reply_target = request_meta_extra_as::<String>(meta, "reply_target")
            .and_then(|value| normalize_non_empty(value.as_str()))?;
        let thread = request_meta_extra_as::<String>(meta, "thread")
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
        conversation: Option<u64>,
    ) -> Result<(), BoxError> {
        if let Some(chan) = self.channels.get(&channel) {
            send_message_with_retry(&channel, chan, &message, ChannelSendRetryPolicy::default())
                .await?;

            let timestamp = unix_ms();
            self.messages
                .add_from(&ChannelMessage {
                    sender: chan.username().to_string(),
                    reply_target: message.recipient,
                    content: message.content,
                    channel,
                    timestamp,
                    thread: message.thread,
                    attachments: message.attachments,
                    conversation,
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

/// A cloneable handle that lets the agent push messages into configured IM
/// channels outside of the IM -> agent -> IM reply loop (any -> agent -> IM).
#[derive(Clone)]
pub struct ChannelSender {
    inner: Arc<ChannelRuntimeInner>,
}

/// A distinct counterparty seen in recent channel traffic, used by the agent
/// to resolve valid `recipient` values for a channel.
#[derive(Debug, Clone, Serialize)]
pub struct RecentRecipient {
    pub channel: String,
    pub recipient: String,
    pub thread: Option<String>,
    pub last_sender: String,
    pub last_timestamp: u64,
}

/// How many recent messages to scan when aggregating distinct recipients.
const RECENT_RECIPIENTS_SCAN_LIMIT: usize = 300;

impl ChannelSender {
    pub fn channels(&self) -> Vec<String> {
        let mut channels: Vec<String> = self.inner.channels.keys().cloned().collect();
        channels.sort();
        channels
    }

    pub fn is_empty(&self) -> bool {
        self.inner.channels.is_empty()
    }

    /// Sends a message through the named channel with the runtime retry policy
    /// and records it in the channel messages collection.
    pub async fn send(
        &self,
        channel: &str,
        message: SendMessage,
        conversation: Option<u64>,
    ) -> Result<(), BoxError> {
        self.inner
            .try_send(channel.to_string(), message, conversation)
            .await
    }

    /// Aggregates the most recently active recipients from stored channel
    /// messages, newest first, optionally restricted to one channel.
    pub async fn recent_recipients(
        &self,
        channel: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RecentRecipient>, BoxError> {
        let limit = limit.clamp(1, 100);
        let cursor = self.inner.messages.max_document_id().saturating_add(1);
        let id_filter = Filter::Field(("_id".to_string(), RangeQuery::Lt(Fv::U64(cursor))));
        let filter = match channel {
            Some(channel) => Filter::And(vec![
                Box::new(Filter::Field((
                    "channel".to_string(),
                    RangeQuery::Eq(Fv::Text(channel.to_string())),
                ))),
                Box::new(id_filter),
            ]),
            None => id_filter,
        };
        let messages: Vec<ChannelMessage> = self
            .inner
            .messages
            .search_as(Query {
                search: None,
                filter: Some(filter),
                limit: Some(RECENT_RECIPIENTS_SCAN_LIMIT),
            })
            .await?;

        let mut recipients: Vec<RecentRecipient> = Vec::new();
        // Results are ascending by _id; walk in reverse for newest first.
        for message in messages.iter().rev() {
            if message.reply_target.is_empty() {
                continue;
            }
            if recipients
                .iter()
                .any(|r| r.channel == message.channel && r.recipient == message.reply_target)
            {
                continue;
            }
            recipients.push(RecentRecipient {
                channel: message.channel.clone(),
                recipient: message.reply_target.clone(),
                thread: message.thread.clone(),
                last_sender: message.sender.clone(),
                last_timestamp: message.timestamp,
            });
            if recipients.len() >= limit {
                break;
            }
        }
        Ok(recipients)
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
        let meta = completion_meta(ctx);

        let (route, stale) = match self.route_for_conversation(conv_id) {
            Some(route) => {
                let stale = self.current_conversation_for_route(&route) != Some(conv_id);
                (route, stale)
            }
            None => {
                let Some(route) = self.route_from_meta(&meta) else {
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
                (route, false)
            }
        };

        let channel = route.channel.clone();
        let msg = completion_message(&meta, output, route, stale);

        if let Err(err) = self.try_send(channel.clone(), msg, Some(conv_id)).await {
            log::error!(name = "channel"; "failed to send message to channel {}: {err}", channel);
        }
    }
}

fn completion_meta(ctx: &AgentCtx) -> RequestMeta {
    ctx.base
        .get_state::<SessionRequestMeta>()
        .map(|state| state.get())
        .unwrap_or_else(|| ctx.meta().clone())
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

fn completion_message(
    meta: &RequestMeta,
    output: &AgentOutput,
    route: ChannelRoute,
    stale: bool,
) -> SendMessage {
    let mut msg = String::new();
    if stale {
        if let Some(conv_id) = output.conversation {
            msg.push_str(&format!("[Previous conversation #{conv_id}]\n\n"));
        } else {
            msg.push_str("[Previous conversation]\n\n");
        }
    }
    if let Some(cron_job_id) = request_meta_extra_as::<u64>(meta, "cron_job_id") {
        let mut name = request_meta_extra_as::<String>(meta, "cron_job_name").unwrap_or_default();
        if name.is_empty() {
            name = cron_job_id.to_string();
        }
        let kind = request_meta_extra_as::<String>(meta, "cron_job_kind").unwrap_or_default();
        msg.push_str(&format!("Cron Job ({kind}): {name}\n\n"));
    }
    msg.push_str(&output.content);
    SendMessage::new(msg, route.reply_target)
        .in_thread(route.thread)
        .with_attachments(output.artifacts.clone())
}

fn agent_prompt_from_message(message: &ChannelMessage) -> String {
    if message.external_user.unwrap_or_default() {
        external_user_prompt_with_space(
            &message.channel,
            &message.sender,
            message.thread.as_deref(),
            &message.content,
        )
    } else {
        message.content.clone()
    }
}

fn channel_new_prompt_command(message: &ChannelMessage) -> Option<Option<String>> {
    if message.external_user.unwrap_or_default() {
        return None;
    }

    match PromptCommand::from(message.content.clone()) {
        PromptCommand::New { prompt } => Some(prompt),
        _ => None,
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
    use anda_engine::engine::Engine;
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

    async fn test_runtime_with_users(
        channel: Arc<TestChannel>,
        default_user: Principal,
        channel_users: HashMap<String, Principal>,
    ) -> ChannelRuntime {
        test_runtime_with_users_in_work_dir(
            channel,
            default_user,
            channel_users,
            std::env::temp_dir(),
        )
        .await
    }

    async fn test_runtime_with_users_in_work_dir(
        channel: Arc<TestChannel>,
        default_user: Principal,
        channel_users: HashMap<String, Principal>,
        work_dir: PathBuf,
    ) -> ChannelRuntime {
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
            default_user,
            channel_users,
            channels,
            work_dir,
        )
        .await
        .unwrap()
    }

    async fn test_runtime(channel: Arc<TestChannel>) -> ChannelRuntime {
        test_runtime_with_users(channel, Principal::management_canister(), HashMap::new()).await
    }

    #[tokio::test]
    async fn connect_migrates_legacy_workspace_dir_to_underscore_name() {
        if cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let channel = Arc::new(TestChannel::new("test:legacy", false));
        let legacy_path = dir.path().join(channel.id());
        let safe_path = dir.path().join(channel_workspace_dir_name(&channel.id()));
        assert_eq!(
            safe_path.file_name().and_then(|name| name.to_str()),
            Some("test_legacy")
        );
        tokio::fs::create_dir_all(&legacy_path).await.unwrap();
        tokio::fs::write(legacy_path.join("state.json"), b"legacy")
            .await
            .unwrap();

        let _runtime = test_runtime_with_users_in_work_dir(
            channel,
            Principal::management_canister(),
            HashMap::new(),
            dir.path().to_path_buf(),
        )
        .await;

        assert!(!legacy_path.exists());
        assert_eq!(
            tokio::fs::read(safe_path.join("state.json")).await.unwrap(),
            b"legacy"
        );
    }

    #[tokio::test]
    async fn migration_merges_legacy_workspace_when_safe_dir_exists() {
        if cfg!(windows) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let channel_id = "test:merge";
        let legacy_path = dir.path().join(channel_id);
        let safe_path = dir.path().join(channel_workspace_dir_name(channel_id));
        tokio::fs::create_dir_all(&legacy_path).await.unwrap();
        tokio::fs::create_dir_all(&safe_path).await.unwrap();
        tokio::fs::write(legacy_path.join("legacy.json"), b"legacy")
            .await
            .unwrap();
        tokio::fs::write(safe_path.join("safe.json"), b"safe")
            .await
            .unwrap();

        migrate_legacy_channel_workspace(dir.path(), channel_id, &safe_path)
            .await
            .unwrap();

        assert!(!legacy_path.exists());
        assert_eq!(
            tokio::fs::read(safe_path.join("legacy.json"))
                .await
                .unwrap(),
            b"legacy"
        );
        assert_eq!(
            tokio::fs::read(safe_path.join("safe.json")).await.unwrap(),
            b"safe"
        );
    }

    #[tokio::test]
    async fn migration_moves_percent_encoded_workspace_to_underscore_name() {
        let dir = tempfile::tempdir().unwrap();
        let channel_id = "test:encoded";
        let encoded_path = dir
            .path()
            .join(legacy_percent_encoded_channel_workspace_dir_name(
                channel_id,
            ));
        let safe_path = dir.path().join(channel_workspace_dir_name(channel_id));
        assert_eq!(
            encoded_path.file_name().and_then(|name| name.to_str()),
            Some("test%3Aencoded")
        );
        assert_eq!(
            safe_path.file_name().and_then(|name| name.to_str()),
            Some("test_encoded")
        );
        tokio::fs::create_dir_all(&encoded_path).await.unwrap();
        tokio::fs::write(encoded_path.join("state.json"), b"encoded")
            .await
            .unwrap();

        migrate_legacy_channel_workspace(dir.path(), channel_id, &safe_path)
            .await
            .unwrap();

        assert!(!encoded_path.exists());
        assert_eq!(
            tokio::fs::read(safe_path.join("state.json")).await.unwrap(),
            b"encoded"
        );
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
    async fn user_for_channel_uses_channel_binding_or_default() {
        let channel = Arc::new(TestChannel::new("test:owned", false));
        let default_user = Principal::from_slice(&[1, 2, 3]);
        let channel_user = Principal::from_slice(&[4, 5, 6]);
        let mut channel_users = HashMap::new();
        channel_users.insert(channel.id(), channel_user);
        let runtime = test_runtime_with_users(channel.clone(), default_user, channel_users).await;

        assert_eq!(runtime.inner.user_for_channel(&channel.id()), channel_user);
        assert_eq!(runtime.inner.user_for_channel("test:unbound"), default_user);
    }

    #[tokio::test]
    async fn bind_conversation_keeps_previous_route_for_stale_outputs() {
        let channel = Arc::new(TestChannel::new("test:stale-route", false));
        let runtime = test_runtime(channel.clone()).await;
        let route = ChannelRoute {
            channel: channel.id(),
            reply_target: "#anda".to_string(),
            thread: Some("thread-1".to_string()),
        };

        runtime.inner.bind_conversation(route.clone(), 42).unwrap();
        runtime.inner.bind_conversation(route.clone(), 99).unwrap();

        assert_eq!(
            runtime.inner.current_conversation_for_route(&route),
            Some(99)
        );
        assert_eq!(runtime.inner.route_for_conversation(42), Some(route));
    }

    #[tokio::test]
    async fn clear_route_conversation_removes_current_binding_only() {
        let channel = Arc::new(TestChannel::new("test:clear-route", false));
        let runtime = test_runtime(channel.clone()).await;
        let route = ChannelRoute {
            channel: channel.id(),
            reply_target: "#anda".to_string(),
            thread: None,
        };

        runtime.inner.bind_conversation(route.clone(), 42).unwrap();
        let snapshot = runtime.inner.clear_route_conversation(&route).unwrap();

        assert!(!snapshot.contains_key(&route.key()));
        assert_eq!(runtime.inner.current_conversation_for_route(&route), None);
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

        let message = completion_message(&RequestMeta::default(), &output, route.clone(), false);

        assert_eq!(message.content, output.content);
        assert_eq!(message.recipient, route.reply_target);
        assert_eq!(message.thread, route.thread);
        assert!(message.attachments.is_empty());
    }

    #[test]
    fn completion_message_marks_stale_conversation() {
        let route = ChannelRoute {
            channel: "test:threaded".to_string(),
            reply_target: "#anda".to_string(),
            thread: None,
        };
        let output = AgentOutput {
            content: "late answer".to_string(),
            conversation: Some(42),
            ..Default::default()
        };

        let message = completion_message(&RequestMeta::default(), &output, route, true);

        assert_eq!(
            message.content,
            "[Previous conversation #42]\n\nlate answer"
        );
    }

    #[tokio::test]
    async fn on_completion_routes_from_latest_session_meta() {
        let channel = Arc::new(TestChannel::new("test:cron-route", false));
        let runtime = test_runtime(channel.clone()).await;
        let ctx = Engine::builder().mock_ctx();
        let route = ChannelRoute {
            channel: channel.id(),
            reply_target: "#anda".to_string(),
            thread: Some("thread-1".to_string()),
        };
        let mut extra = serde_json::Map::new();
        extra.insert("source".to_string(), route.channel.clone().into());
        extra.insert(
            "reply_target".to_string(),
            route.reply_target.clone().into(),
        );
        extra.insert("thread".to_string(), "thread-1".into());
        extra.insert("cron_job_id".to_string(), 11.into());
        extra.insert("cron_job_name".to_string(), "hourly-research".into());
        extra.insert("cron_job_kind".to_string(), "agent".into());
        ctx.base.set_state(SessionRequestMeta::new(RequestMeta {
            extra,
            ..Default::default()
        }));
        let output = AgentOutput {
            content: "done".to_string(),
            conversation: Some(154),
            ..Default::default()
        };

        runtime.inner.clone().on_completion(&ctx, &output).await;

        let sent_messages = channel.sent_messages().await;
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].recipient, route.reply_target);
        assert_eq!(sent_messages[0].thread, route.thread);
        assert_eq!(
            sent_messages[0].content,
            "Cron Job (agent): hourly-research\n\ndone"
        );
        assert_eq!(
            runtime.inner.current_conversation_for_route(&route),
            Some(154)
        );
        assert_eq!(runtime.inner.route_for_conversation(154), Some(route));
    }

    #[test]
    fn channel_new_prompt_command_ignores_external_users() {
        let trusted = ChannelMessage {
            content: "/new fresh".to_string(),
            ..Default::default()
        };
        let external = ChannelMessage {
            content: "/new fresh".to_string(),
            external_user: Some(true),
            ..Default::default()
        };

        assert_eq!(
            channel_new_prompt_command(&trusted),
            Some(Some("/new fresh".to_string()))
        );
        assert_eq!(channel_new_prompt_command(&external), None);
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
        assert!(prompt.contains("external untrusted IM participant"));
        assert!(prompt.ends_with("hello\""));
    }

    #[test]
    fn agent_prompt_includes_external_discussion_space() {
        let message = ChannelMessage {
            sender: "agent-a".to_string(),
            channel: "wechat:family".to_string(),
            content: "hello".to_string(),
            thread: Some("room-7".to_string()),
            external_user: Some(true),
            ..Default::default()
        };

        let prompt = agent_prompt_from_message(&message);

        assert!(prompt.starts_with(
            "[$external_user: channel=\"wechat:family\", sender=\"agent-a\", space=\"room-7\"]"
        ));
    }

    #[tokio::test]
    async fn sender_sends_and_aggregates_recent_recipients() {
        let channel = Arc::new(TestChannel::new("test:sender", false));
        let runtime = test_runtime(channel.clone()).await;
        let sender = runtime.sender();

        assert!(!sender.is_empty());
        assert_eq!(sender.channels(), vec![channel.id()]);

        sender
            .send(&channel.id(), SendMessage::new("first", "alice"), None)
            .await
            .unwrap();
        sender
            .send(&channel.id(), SendMessage::new("second", "bob"), Some(7))
            .await
            .unwrap();
        sender
            .send(&channel.id(), SendMessage::new("third", "alice"), None)
            .await
            .unwrap();

        assert_eq!(channel.sent_messages().await.len(), 3);

        let recipients = sender.recent_recipients(None, 10).await.unwrap();
        assert_eq!(recipients.len(), 2);
        assert_eq!(recipients[0].recipient, "alice"); // newest first
        assert_eq!(recipients[0].channel, channel.id());
        assert_eq!(recipients[0].last_sender, "anda-bot");
        assert_eq!(recipients[1].recipient, "bob");

        let filtered = sender
            .recent_recipients(Some(&channel.id()), 1)
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].recipient, "alice");

        let none = sender
            .recent_recipients(Some("test:other"), 10)
            .await
            .unwrap();
        assert!(none.is_empty());

        let err = sender
            .send("test:missing", SendMessage::new("x", "alice"), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
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

    #[tokio::test]
    async fn prepare_channel_workspace_migrates_legacy_layout() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path();
        // A channel id with special characters has a distinct safe path.
        let channel_id = "telegram:chat-1";
        let legacy = legacy_channel_workspace_path(work, channel_id);
        tokio::fs::create_dir_all(&legacy).await.unwrap();
        tokio::fs::write(legacy.join("note.txt"), b"hi")
            .await
            .unwrap();

        let safe = prepare_channel_workspace(work, channel_id).await;
        assert!(safe.join("note.txt").is_file());
        // The legacy directory was migrated (renamed) into the safe path.
        assert!(!legacy.exists());
    }

    #[tokio::test]
    async fn merge_workspace_dirs_moves_non_conflicting_entries() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        tokio::fs::create_dir_all(src.join("sub")).await.unwrap();
        tokio::fs::write(src.join("a.txt"), b"a").await.unwrap();
        tokio::fs::write(src.join("sub/b.txt"), b"b").await.unwrap();
        tokio::fs::create_dir_all(&dst).await.unwrap();
        // Existing destination entry is preserved; conflicting source is left behind.
        tokio::fs::write(dst.join("a.txt"), b"keep").await.unwrap();

        merge_workspace_dirs(&src, &dst).await.unwrap();

        assert_eq!(tokio::fs::read(dst.join("a.txt")).await.unwrap(), b"keep");
        assert!(dst.join("sub/b.txt").is_file());
    }

    #[test]
    fn channel_route_from_message_and_key_round_trip() {
        let message = ChannelMessage {
            channel: "telegram".to_string(),
            reply_target: "chat-1".to_string(),
            thread: Some("topic-2".to_string()),
            ..Default::default()
        };
        let route = ChannelRoute::from_message(&message);
        assert_eq!(route.channel, "telegram");
        assert_eq!(
            route.key(),
            (
                "telegram".to_string(),
                "chat-1".to_string(),
                Some("topic-2".to_string())
            )
        );
    }

    #[test]
    fn channel_workspace_paths_distinguish_layouts() {
        let work_dir = Path::new("/tmp/channels");
        let safe = channel_workspace_path(work_dir, "telegram:chat/1");
        let legacy = legacy_channel_workspace_path(work_dir, "telegram:chat/1");
        let percent = legacy_percent_encoded_channel_workspace_path(work_dir, "telegram:chat/1");
        assert!(safe.starts_with(work_dir));
        assert_eq!(legacy, work_dir.join("telegram:chat/1"));
        assert_ne!(safe, percent);
    }

    #[test]
    fn build_conversation_routes_inverts_channel_map() {
        let mut map = ChannelConversationMap::new();
        map.insert(("tg".to_string(), "c1".to_string(), None), 7);
        let routes = build_conversation_routes(&map);
        assert_eq!(routes.get(&7).map(|r| r.channel.as_str()), Some("tg"));
    }

    #[test]
    fn normalize_non_empty_trims() {
        assert_eq!(normalize_non_empty("  x  ").as_deref(), Some("x"));
        assert_eq!(normalize_non_empty("   "), None);
    }

    #[test]
    fn agent_prompt_from_message_scopes_external_users() {
        let internal = ChannelMessage {
            content: "hello".to_string(),
            ..Default::default()
        };
        assert_eq!(agent_prompt_from_message(&internal), "hello");

        let external = ChannelMessage {
            content: "hi".to_string(),
            channel: "wechat".to_string(),
            sender: "wxid".to_string(),
            external_user: Some(true),
            ..Default::default()
        };
        // External prompts are wrapped with scoping context, not the raw text.
        assert_ne!(agent_prompt_from_message(&external), "hi");
    }

    #[test]
    fn channel_new_prompt_command_detects_new_for_internal_only() {
        let new_cmd = ChannelMessage {
            content: "/new plan the week".to_string(),
            ..Default::default()
        };
        // A `/new` message is recognized as a new-conversation command.
        assert!(channel_new_prompt_command(&new_cmd).is_some());

        let plain = ChannelMessage {
            content: "just chatting".to_string(),
            ..Default::default()
        };
        assert_eq!(channel_new_prompt_command(&plain), None);

        // External users never trigger control commands.
        let external = ChannelMessage {
            content: "/new x".to_string(),
            external_user: Some(true),
            ..Default::default()
        };
        assert_eq!(channel_new_prompt_command(&external), None);
    }

    #[test]
    fn completion_message_includes_cron_and_stale_markers() {
        let mut meta = RequestMeta::default();
        meta.extra.insert("cron_job_id".to_string(), 42u64.into());
        meta.extra
            .insert("cron_job_name".to_string(), "nightly".into());
        meta.extra
            .insert("cron_job_kind".to_string(), "shell".into());
        let output = AgentOutput {
            content: "done".to_string(),
            conversation: Some(9),
            ..Default::default()
        };
        let route = ChannelRoute {
            channel: "tg".to_string(),
            reply_target: "c1".to_string(),
            thread: None,
        };
        let message = completion_message(&meta, &output, route, true);
        assert!(message.content.contains("Previous conversation #9"));
        assert!(message.content.contains("Cron Job (shell): nightly"));
        assert!(message.content.contains("done"));
    }
}
