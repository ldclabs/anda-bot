use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use weixin_agent::{
    LoginStatus, MediaInfo, MediaType, MessageContext, MessageHandler, Result as WeixinResult,
    StandaloneQrLogin, WeixinClient, WeixinConfig,
};

use super::{
    Channel, ChannelInitOptions, ChannelInitResult, ChannelMessage, ChannelWorkspace, SendMessage,
    file_name_for_resource, is_http_url, is_transient_send_error, resource_from_bytes,
};
use crate::{
    config::{self, normalize_identity, normalize_string},
    util::{file_uri::path_from_file_uri_or_path, text::read_text_file},
};

const WECHAT_MAX_MESSAGE_LENGTH: usize = 4000;
const WECHAT_CONTINUATION_OVERHEAD: usize = 30;
const WECHAT_MAX_FILE_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;
const WECHAT_QR_POLL_DELAY: Duration = Duration::from_secs(2);
const WECHAT_MAX_QR_REFRESH_COUNT: u32 = 3;
const WECHAT_CONTEXT_TOKEN_MAX_AGE_MS: u64 = 2 * 60 * 60 * 1000;
const WECHAT_CONTEXT_TOKENS_FILE: &str = "context_tokens.json";
const WECHAT_CONTEXT_TOKEN_META_FILE: &str = "context_tokens_meta.json";

pub fn build_wechat_channels(
    cfg: &[config::WechatChannelSettings],
) -> Result<HashMap<String, Arc<dyn Channel>>, BoxError> {
    let mut channels = HashMap::new();

    for (index, wechat_cfg) in cfg.iter().enumerate() {
        if wechat_cfg.is_empty() {
            continue;
        }

        if wechat_cfg.bot_token.trim().is_empty() {
            log::info!(
                "WeChat channel '{}' has no bot_token; saved token or QR login will be used",
                wechat_cfg.label(index)
            );
        }

        let channel: Arc<dyn Channel> = Arc::new(WechatChannel::new(wechat_cfg));
        let channel_id = channel.id();
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate WeChat channel id '{channel_id}'").into());
        }
    }

    Ok(channels)
}

pub struct WechatChannel {
    id: String,
    bot_token: Option<String>,
    username: String,
    allowed_users: Vec<String>,
    allow_external_users: bool,
    base_url: String,
    cdn_base_url: String,
    route_tag: Option<u32>,
    workspace: Arc<ChannelWorkspace>,
}

impl WechatChannel {
    pub fn new(cfg: &config::WechatChannelSettings) -> Self {
        Self {
            id: cfg.channel_id(),
            bot_token: normalize_string(&cfg.bot_token),
            username: cfg.username.clone().unwrap_or_else(|| "wechat".to_string()),
            allowed_users: cfg
                .allowed_users
                .iter()
                .map(|s| normalize_identity(s))
                .collect(),
            allow_external_users: cfg.allow_external_users,
            base_url: config::DEFAULT_WECHAT_API_BASE.to_string(),
            cdn_base_url: config::DEFAULT_WECHAT_CDN_BASE.to_string(),
            route_tag: cfg.route_tag,
            workspace: Arc::new(ChannelWorkspace::default()),
        }
    }

    fn build_weixin_config(&self, token: &str) -> Result<WeixinConfig, BoxError> {
        let mut builder = WeixinConfig::builder().token(token);
        builder = builder.base_url(&self.base_url);
        builder = builder.cdn_base_url(&self.cdn_base_url);
        if let Some(route_tag) = self.route_tag {
            builder = builder.route_tag(route_tag);
        }
        Ok(builder.build()?)
    }

    fn build_client(
        &self,
        token: &str,
        handler: impl MessageHandler + 'static,
        cancel_token: CancellationToken,
    ) -> Result<WeixinClient, BoxError> {
        let config = self.build_weixin_config(token)?;
        Ok(WeixinClient::builder(config)
            .with_cancel_token(cancel_token)
            .on_message(handler)
            .build()?)
    }

    async fn saved_token(&self) -> Option<String> {
        let path = self.workspace.path()?.join("token.txt");
        let token = read_text_file(path).await.ok()?;
        let token = token.trim().to_string();
        (!token.is_empty()).then_some(token)
    }

    async fn resolve_token(&self) -> Result<String, BoxError> {
        if let Some(token) = &self.bot_token {
            return Ok(token.clone());
        }

        if let Some(token) = self.saved_token().await {
            return Ok(token);
        }

        Err("WeChat channel has no bot_token configured and no saved token found; Run `anda channel init wechat` to initialize the bot_token.".into())
    }

    async fn qr_login_and_save(&self, workspace: &Path) -> Result<String, BoxError> {
        let config = self.build_weixin_config("")?;
        let qr = StandaloneQrLogin::new(&config);
        let mut session = qr.start(None, &[]).await?;
        print_qr_hint(&session.qrcode_img_content);

        let mut refresh_count = 0_u32;
        loop {
            match qr.poll_status(&session, None).await? {
                LoginStatus::Confirmed {
                    bot_token,
                    ilink_bot_id,
                    base_url,
                    ilink_user_id,
                } => {
                    let info = format!(
                        "WeChat QR login confirmed: bot_id={ilink_bot_id}, user_id={ilink_user_id}, base_url={base_url}"
                    );
                    log::info!("{}", info);
                    println!("{}\n\nYou can now close this terminal if you like.", info);
                    tokio::fs::write(workspace.join("bot.txt"), &info).await?;
                    tokio::fs::write(workspace.join("token.txt"), &bot_token).await?;
                    return Ok(bot_token);
                }
                LoginStatus::Scanned => {
                    log::info!("WeChat QR code scanned; waiting for confirmation");
                }
                LoginStatus::NeedVerifyCode => {
                    log::info!(
                        "Server requires a verification code (pair-code displayed on phone)."
                    );
                }
                LoginStatus::VerifyCodeBlocked => {
                    log::warn!("Too many wrong verification codes; QR code must be refreshed.");
                }
                LoginStatus::BindedRedirect => {
                    log::info!("Bot is already bound to this instance; no new credentials issued.");
                }
                LoginStatus::Expired => {
                    refresh_count += 1;
                    if refresh_count >= WECHAT_MAX_QR_REFRESH_COUNT {
                        return Err("WeChat QR code expired too many times".into());
                    }
                    log::warn!(
                        "WeChat QR code expired; refreshing ({refresh_count}/{WECHAT_MAX_QR_REFRESH_COUNT})"
                    );
                    session = qr.start(None, &[]).await?;
                    print_qr_hint(&session.qrcode_img_content);
                }
                _ => {}
            }

            tokio::time::sleep(WECHAT_QR_POLL_DELAY).await;
        }
    }

    async fn load_sync_buf(&self) -> Option<String> {
        load_sync_buf_from_workspace(&self.workspace).await
    }

    async fn load_context_tokens(&self) -> HashMap<String, String> {
        load_context_tokens_from_workspace(&self.workspace).await
    }

    async fn save_context_tokens(&self, tokens: &HashMap<String, String>) {
        save_context_tokens_to_workspace(&self.workspace, tokens).await;
    }

    async fn send_text_chunks(
        &self,
        client: &WeixinClient,
        recipient: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<(), BoxError> {
        let chunks = split_message_for_wechat(text);
        for (index, chunk) in chunks.iter().enumerate() {
            let text = if chunks.len() > 1 {
                if index == 0 {
                    format!("{chunk}\n\n(continues...)")
                } else if index == chunks.len() - 1 {
                    format!("(continued)\n\n{chunk}")
                } else {
                    format!("(continued)\n\n{chunk}\n\n(continues...)")
                }
            } else {
                chunk.to_string()
            };
            client.send_text(recipient, &text, context_token).await?;
        }
        Ok(())
    }

    async fn send_resource(
        &self,
        client: &WeixinClient,
        recipient: &str,
        resource: &Resource,
        context_token: Option<&str>,
    ) -> Result<(), BoxError> {
        if let Some(uri) = resource.uri.as_deref() {
            if is_http_url(uri) {
                let label = if resource.name.trim().is_empty() {
                    uri.to_string()
                } else {
                    format!("{}: {uri}", resource.name)
                };
                client.send_text(recipient, &label, context_token).await?;
                return Ok(());
            }

            let path = path_from_file_uri_or_path(uri)?;
            if path.exists() {
                client.send_media(recipient, &path, context_token).await?;
                return Ok(());
            }
        }

        if let Some(blob) = &resource.blob {
            let path = self.write_outgoing_blob(resource, &blob.0).await?;
            client.send_media(recipient, &path, context_token).await?;
            return Ok(());
        }

        Err(format!("WeChat resource '{}' has no uri or blob", resource.name).into())
    }

    async fn send_message_parts(
        &self,
        client: &WeixinClient,
        recipient: &str,
        message: &SendMessage,
        context_token: Option<&str>,
    ) -> Result<(), BoxError> {
        if !message.content.trim().is_empty() {
            self.send_text_chunks(client, recipient, &message.content, context_token)
                .await?;
        }

        for resource in &message.attachments {
            self.send_resource(client, recipient, resource, context_token)
                .await?;
        }

        if message.content.trim().is_empty() && message.attachments.is_empty() {
            self.send_text_chunks(client, recipient, " ", context_token)
                .await?;
        }

        Ok(())
    }

    async fn write_outgoing_blob(
        &self,
        resource: &Resource,
        bytes: &[u8],
    ) -> Result<PathBuf, BoxError> {
        let base = self.workspace.path().unwrap_or_else(std::env::temp_dir);
        let dir = base.join("outgoing");
        tokio::fs::create_dir_all(&dir).await?;

        let file_name = file_name_for_resource(resource).to_string();
        let path = dir.join(format!(
            "{}-{}",
            unix_ms(),
            sanitize_path_component(&file_name, "attachment.bin")
        ));
        tokio::fs::write(&path, bytes).await?;
        Ok(path)
    }
}

#[async_trait]
impl Channel for WechatChannel {
    fn name(&self) -> &str {
        "wechat"
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn id(&self) -> String {
        format!("wechat:{}", self.id)
    }

    fn set_workspace(&self, workspace: PathBuf) {
        self.workspace.set_path(workspace);
    }

    async fn init(&self, options: ChannelInitOptions) -> Result<ChannelInitResult, BoxError> {
        if self.bot_token.is_some() && !options.force {
            return Ok(ChannelInitResult::unchanged(
                "WeChat bot_token is configured; QR login is not required",
            ));
        }

        let workspace = self
            .workspace
            .path()
            .ok_or("WeChat channel workspace is not initialized")?;
        let token_path = workspace.join("token.txt");
        if !options.force && self.saved_token().await.is_some() {
            return Ok(ChannelInitResult::unchanged(format!(
                "WeChat token already exists at {}",
                token_path.display()
            )));
        }

        self.qr_login_and_save(&workspace).await?;
        let mut message = format!(
            "WeChat QR login completed; token saved to {}",
            token_path.display()
        );
        if self.bot_token.is_some() {
            message.push_str("; configured bot_token still takes precedence");
        }
        Ok(ChannelInitResult::changed(message))
    }

    async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
        let recipient = message.recipient.trim();
        if recipient.is_empty() {
            return Err("WeChat recipient is empty".into());
        }

        let token = self.resolve_token().await?;
        let client = self.build_client(&token, NoopMessageHandler, CancellationToken::new())?;
        let context_token = load_context_token_for_send(&self.workspace, recipient).await;
        let result = self
            .send_message_parts(&client, recipient, message, context_token.as_deref())
            .await;

        match result {
            Ok(()) => Ok(()),
            Err(err)
                if context_token.is_some() && is_wechat_context_token_error(&err.to_string()) =>
            {
                log::warn!(
                    "WeChat send to {recipient} failed with a cached context token; clearing it and retrying without context token: {err}"
                );
                remove_context_token_from_workspace(
                    &self.workspace,
                    recipient,
                    context_token.as_deref(),
                )
                .await;
                self.send_message_parts(&client, recipient, message, None)
                    .await
            }
            Err(err) => Err(err),
        }
    }

    fn should_retry_send(&self, error: &str) -> bool {
        is_transient_send_error(error) || is_wechat_context_token_error(error)
    }

    async fn listen(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        let token = self.resolve_token().await?;
        let handler = WechatMessageHandler {
            channel_id: self.id(),
            allowed_users: self.allowed_users.clone(),
            allow_external_users: self.allow_external_users,
            tx,
            workspace: self.workspace.clone(),
            cancel_token: cancel_token.clone(),
        };
        let client = self.build_client(&token, handler, cancel_token)?;
        client
            .context_tokens()
            .import(self.load_context_tokens().await);

        log::info!("WeChat channel {} listening for messages", self.id());
        let result = client.start(self.load_sync_buf().await).await;
        self.save_context_tokens(&client.context_tokens().export_all())
            .await;
        Ok(result?)
    }

    async fn health_check(&self) -> bool {
        self.bot_token.is_some() || self.saved_token().await.is_some()
    }
}

struct NoopMessageHandler;

#[async_trait]
impl MessageHandler for NoopMessageHandler {
    async fn on_message(&self, _ctx: &MessageContext) -> WeixinResult<()> {
        Ok(())
    }
}

struct WechatMessageHandler {
    channel_id: String,
    allowed_users: Vec<String>,
    allow_external_users: bool,
    tx: mpsc::Sender<ChannelMessage>,
    workspace: Arc<ChannelWorkspace>,
    cancel_token: CancellationToken,
}

#[async_trait]
impl MessageHandler for WechatMessageHandler {
    async fn on_message(&self, ctx: &MessageContext) -> WeixinResult<()> {
        let trusted_user = is_identity_allowed(&self.allowed_users, &ctx.from);
        if !trusted_user && !self.allow_external_users {
            log::warn!(
                "WeChat ignoring message from unauthorized user: {}",
                ctx.from
            );
            return Ok(());
        }
        let Some(mut message) =
            channel_message_from_context(ctx, &self.channel_id, &self.workspace).await
        else {
            return Ok(());
        };
        message.external_user = (!trusted_user).then_some(true);
        message
            .extra
            .insert("trusted_user".to_string(), trusted_user.into());

        if let Some(context_token) = ctx.context_token.as_deref() {
            save_context_token_to_workspace(&self.workspace, &message.reply_target, context_token)
                .await;
        }

        if self.tx.send(message).await.is_err() {
            self.cancel_token.cancel();
        }
        Ok(())
    }

    async fn on_sync_buf_updated(&self, sync_buf: &str) -> WeixinResult<()> {
        save_sync_buf_to_workspace(&self.workspace, sync_buf).await;
        Ok(())
    }
}

async fn channel_message_from_context(
    ctx: &MessageContext,
    channel_id: &str,
    workspace: &Arc<ChannelWorkspace>,
) -> Option<ChannelMessage> {
    let mut content = ctx.body.clone().unwrap_or_default();
    if content.trim().is_empty()
        && let Some(ref_message) = &ctx.ref_message
    {
        content = format_ref_message(ref_message);
    }

    let mut attachments = Vec::new();
    if let Some(media) = &ctx.media
        && let Some(resource) = download_media_resource(ctx, media, workspace).await
    {
        attachments.push(resource);
    }

    if content.trim().is_empty() && !attachments.is_empty() {
        content = attachment_fallback_text(&attachments[0]);
    }

    if content.trim().is_empty() && attachments.is_empty() {
        return None;
    }

    let timestamp = u64::try_from(ctx.timestamp)
        .ok()
        .filter(|timestamp| *timestamp > 0)
        .unwrap_or_else(unix_ms);

    let reply_target = wechat_reply_target(ctx);
    let thread = wechat_thread(ctx);
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("message_id".to_string(), ctx.message_id.clone().into());
    extra.insert("from".to_string(), ctx.from.clone().into());
    extra.insert("to".to_string(), ctx.to.clone().into());
    if let Some(server_message_id) = ctx.server_message_id {
        extra.insert("server_message_id".to_string(), server_message_id.into());
    }
    if let Some(session_id) = &thread {
        extra.insert("session_id".to_string(), session_id.clone().into());
        extra.insert("space".to_string(), session_id.clone().into());
    }

    Some(ChannelMessage {
        sender: ctx.from.clone(),
        reply_target,
        content,
        channel: channel_id.to_string(),
        timestamp,
        thread,
        attachments,
        extra,
        ..Default::default()
    })
}

fn wechat_reply_target(ctx: &MessageContext) -> String {
    wechat_reply_target_from(&ctx.from)
}

fn wechat_thread(ctx: &MessageContext) -> Option<String> {
    wechat_thread_from_session_id(ctx.session_id.as_deref())
}

fn wechat_reply_target_from(from: &str) -> String {
    from.trim().to_string()
}

fn wechat_thread_from_session_id(session_id: Option<&str>) -> Option<String> {
    session_id.and_then(normalize_string)
}

async fn download_media_resource(
    ctx: &MessageContext,
    media: &MediaInfo,
    workspace: &Arc<ChannelWorkspace>,
) -> Option<Resource> {
    if let Some(file_size) = media.file_size
        && file_size > WECHAT_MAX_FILE_DOWNLOAD_BYTES
    {
        log::warn!(
            "WeChat skipping attachment larger than {} bytes: {file_size}",
            WECHAT_MAX_FILE_DOWNLOAD_BYTES
        );
        return None;
    }

    let file_name = wechat_media_file_name(media, &ctx.message_id);
    let temp_path = temp_media_path(workspace, &ctx.message_id, &file_name).await;
    let bytes = match ctx.download_media(media, &temp_path).await {
        Ok(path) => {
            let bytes = tokio::fs::read(&path).await.ok()?;
            let _ = tokio::fs::remove_file(path).await;
            bytes
        }
        Err(err) => {
            log::warn!("WeChat failed to download media: {err}");
            return None;
        }
    };

    if bytes.len() as u64 > WECHAT_MAX_FILE_DOWNLOAD_BYTES {
        log::warn!(
            "WeChat skipping downloaded attachment larger than {} bytes: {}",
            WECHAT_MAX_FILE_DOWNLOAD_BYTES,
            bytes.len()
        );
        return None;
    }

    let mut resource = resource_from_bytes(file_name, bytes, "WeChat attachment");
    workspace
        .store_resource_lossy(&mut resource, Some(&ctx.message_id), "WeChat attachment")
        .await;
    Some(resource)
}

async fn temp_media_path(
    workspace: &Arc<ChannelWorkspace>,
    message_id: &str,
    file_name: &str,
) -> PathBuf {
    let base = workspace.path().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("incoming");
    if let Err(err) = tokio::fs::create_dir_all(&dir).await {
        log::warn!("failed to create WeChat incoming media dir: {err}");
        return std::env::temp_dir().join(sanitize_path_component(file_name, "media.bin"));
    }

    dir.join(format!(
        "{}-{}",
        sanitize_path_component(message_id, "message"),
        sanitize_path_component(file_name, "media.bin")
    ))
}

fn format_ref_message(ref_message: &weixin_agent::RefMessageInfo) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &ref_message.title {
        parts.push(title.clone());
    }
    if let Some(body) = &ref_message.body {
        parts.push(body.clone());
    }
    if parts.is_empty() {
        "[Quoted message]".to_string()
    } else {
        format!("[Quoted: {}]", parts.join(" | "))
    }
}

fn attachment_fallback_text(resource: &Resource) -> String {
    // `infer_resource` records capitalized matcher tags ("Image"/"Video"/...),
    // while callers may set lowercase tags by hand; match case-insensitively.
    let has_tag = |kind: &str| {
        resource
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(kind))
    };
    let label = if has_tag("image") {
        "Image"
    } else if has_tag("video") {
        "Video"
    } else if has_tag("audio") {
        "Audio"
    } else {
        "Document"
    };
    format!("[{label}: {}]", resource.name)
}

fn wechat_media_file_name(media: &MediaInfo, message_id: &str) -> String {
    if let Some(file_name) = media
        .file_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        return file_name.to_string();
    }

    if let Some(url) = media.url.as_deref()
        && let Some(name) = url
            .split('?')
            .next()
            .and_then(|path| path.rsplit('/').next())
            .filter(|name| !name.trim().is_empty())
    {
        return name.to_string();
    }

    match media.media_type {
        MediaType::Image => format!("{message_id}.jpg"),
        MediaType::Video => format!("{message_id}.mp4"),
        MediaType::Voice => format!("{message_id}.silk"),
        MediaType::File => format!("{message_id}.bin"),
    }
}

async fn load_sync_buf_from_workspace(workspace: &Arc<ChannelWorkspace>) -> Option<String> {
    let path = workspace.path()?.join("sync_buf.json");
    let data = read_text_file(path).await.ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
    parsed
        .get("get_updates_buf")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

async fn save_sync_buf_to_workspace(workspace: &Arc<ChannelWorkspace>, sync_buf: &str) {
    let Some(path) = workspace.path() else {
        return;
    };
    if let Err(err) = tokio::fs::create_dir_all(&path).await {
        log::warn!("failed to create WeChat workspace for sync buf: {err}");
        return;
    }
    let data = serde_json::json!({ "get_updates_buf": sync_buf });
    if let Err(err) = tokio::fs::write(path.join("sync_buf.json"), data.to_string()).await {
        log::warn!("failed to save WeChat sync buf: {err}");
    }
}

async fn load_context_tokens_from_workspace(
    workspace: &Arc<ChannelWorkspace>,
) -> HashMap<String, String> {
    let Some(path) = workspace.path() else {
        return HashMap::new();
    };
    let Ok(data) = read_text_file(path.join(WECHAT_CONTEXT_TOKENS_FILE)).await else {
        return HashMap::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

async fn load_context_token_meta_from_workspace(
    workspace: &Arc<ChannelWorkspace>,
) -> HashMap<String, u64> {
    let Some(path) = workspace.path() else {
        return HashMap::new();
    };
    let Ok(data) = read_text_file(path.join(WECHAT_CONTEXT_TOKEN_META_FILE)).await else {
        return HashMap::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

async fn save_context_tokens_to_workspace(
    workspace: &Arc<ChannelWorkspace>,
    tokens: &HashMap<String, String>,
) {
    let Some(path) = workspace.path() else {
        return;
    };
    if let Err(err) = tokio::fs::create_dir_all(&path).await {
        log::warn!("failed to create WeChat workspace for context tokens: {err}");
        return;
    }
    let data = match serde_json::to_string(tokens) {
        Ok(data) => data,
        Err(err) => {
            log::warn!("failed to serialize WeChat context tokens: {err}");
            return;
        }
    };
    if let Err(err) = tokio::fs::write(path.join(WECHAT_CONTEXT_TOKENS_FILE), data).await {
        log::warn!("failed to save WeChat context tokens: {err}");
        return;
    }

    ensure_context_token_meta_to_workspace(workspace, tokens).await;
}

async fn save_context_token_meta_to_workspace(
    workspace: &Arc<ChannelWorkspace>,
    meta: &HashMap<String, u64>,
) {
    let Some(path) = workspace.path() else {
        return;
    };
    if let Err(err) = tokio::fs::create_dir_all(&path).await {
        log::warn!("failed to create WeChat workspace for context token metadata: {err}");
        return;
    }
    let data = match serde_json::to_string(meta) {
        Ok(data) => data,
        Err(err) => {
            log::warn!("failed to serialize WeChat context token metadata: {err}");
            return;
        }
    };
    if let Err(err) = tokio::fs::write(path.join(WECHAT_CONTEXT_TOKEN_META_FILE), data).await {
        log::warn!("failed to save WeChat context token metadata: {err}");
    }
}

async fn ensure_context_token_meta_to_workspace(
    workspace: &Arc<ChannelWorkspace>,
    tokens: &HashMap<String, String>,
) {
    let mut meta = load_context_token_meta_from_workspace(workspace).await;
    let mut changed = false;

    meta.retain(|user_id, _| {
        let keep = tokens.contains_key(user_id);
        changed |= !keep;
        keep
    });

    let now = unix_ms();
    for user_id in tokens.keys() {
        if !meta.contains_key(user_id) {
            meta.insert(user_id.clone(), now);
            changed = true;
        }
    }

    if changed {
        save_context_token_meta_to_workspace(workspace, &meta).await;
    }
}

async fn touch_context_token_meta_to_workspace(workspace: &Arc<ChannelWorkspace>, user_id: &str) {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return;
    }

    let mut meta = load_context_token_meta_from_workspace(workspace).await;
    meta.insert(user_id.to_string(), unix_ms());
    save_context_token_meta_to_workspace(workspace, &meta).await;
}

async fn context_tokens_file_modified_ms(workspace: &Arc<ChannelWorkspace>) -> Option<u64> {
    let path = workspace.path()?.join(WECHAT_CONTEXT_TOKENS_FILE);
    let modified = tokio::fs::metadata(path).await.ok()?.modified().ok()?;
    let millis = modified.duration_since(UNIX_EPOCH).ok()?.as_millis();
    u64::try_from(millis).ok()
}

fn context_token_is_stale(updated_at: Option<u64>) -> bool {
    let Some(updated_at) = updated_at else {
        return false;
    };
    unix_ms().saturating_sub(updated_at) > WECHAT_CONTEXT_TOKEN_MAX_AGE_MS
}

async fn load_context_token_for_send(
    workspace: &Arc<ChannelWorkspace>,
    user_id: &str,
) -> Option<String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return None;
    }

    let tokens = load_context_tokens_from_workspace(workspace).await;
    let token = tokens.get(user_id)?.trim().to_string();
    if token.is_empty() {
        return None;
    }

    let meta = load_context_token_meta_from_workspace(workspace).await;
    let updated_at = match meta.get(user_id).copied() {
        Some(updated_at) => Some(updated_at),
        None => context_tokens_file_modified_ms(workspace).await,
    };
    if context_token_is_stale(updated_at) {
        log::warn!(
            "WeChat cached context token for {user_id} is stale; retrying future sends without it"
        );
        remove_context_token_from_workspace(workspace, user_id, Some(&token)).await;
        return None;
    }

    Some(token)
}

async fn remove_context_token_from_workspace(
    workspace: &Arc<ChannelWorkspace>,
    user_id: &str,
    expected_token: Option<&str>,
) {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return;
    }

    let mut tokens = load_context_tokens_from_workspace(workspace).await;
    let should_remove = expected_token.is_none_or(|expected| {
        tokens
            .get(user_id)
            .is_some_and(|current| current == expected)
    });
    if !should_remove {
        return;
    }

    if tokens.remove(user_id).is_some() {
        save_context_tokens_to_workspace(workspace, &tokens).await;
        return;
    }

    let mut meta = load_context_token_meta_from_workspace(workspace).await;
    if meta.remove(user_id).is_some() {
        save_context_token_meta_to_workspace(workspace, &meta).await;
    }
}

async fn save_context_token_to_workspace(
    workspace: &Arc<ChannelWorkspace>,
    user_id: &str,
    token: &str,
) {
    let user_id = user_id.trim();
    let token = token.trim();
    if user_id.is_empty() || token.is_empty() {
        return;
    }

    let mut tokens = load_context_tokens_from_workspace(workspace).await;
    if tokens.get(user_id).is_none_or(|current| current != token) {
        tokens.insert(user_id.to_string(), token.to_string());
        save_context_tokens_to_workspace(workspace, &tokens).await;
    }

    touch_context_token_meta_to_workspace(workspace, user_id).await;
}

fn is_wechat_context_token_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("session expired")
        || error.contains("errcode=-14")
        || error.contains("errcode=-2")
        || error.contains("\"ret\":-2")
        || error.contains("ret=-2")
        || error.contains("context_token")
        || error.contains("context token")
        || error.contains("invalid token")
        || ((error.contains("token") || error.contains("session")) && error.contains("expired"))
}

fn split_message_for_wechat(message: &str) -> Vec<String> {
    if message.chars().count() <= WECHAT_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;
    let chunk_limit = WECHAT_MAX_MESSAGE_LENGTH - WECHAT_CONTINUATION_OVERHEAD;

    while !remaining.is_empty() {
        if remaining.chars().count() <= WECHAT_MAX_MESSAGE_LENGTH {
            chunks.push(remaining.to_string());
            break;
        }

        let hard_split = remaining
            .char_indices()
            .nth(chunk_limit)
            .map_or(remaining.len(), |(idx, _)| idx);
        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            search_area
                .rfind('\n')
                .filter(|pos| *pos > 0)
                .or_else(|| search_area.rfind(' ').filter(|pos| *pos > 0))
                .unwrap_or(hard_split)
        };

        chunks.push(remaining[..chunk_end].trim_end().to_string());
        remaining = remaining[chunk_end..].trim_start();
    }

    chunks
}

fn is_identity_allowed(allowed_users: &[String], identity: &str) -> bool {
    let identity = normalize_identity(identity);
    !identity.is_empty()
        && allowed_users
            .iter()
            .any(|allowed| allowed == "*" || allowed == &identity)
}

fn sanitize_path_component(value: &str, fallback: &str) -> String {
    let mut sanitized = String::with_capacity(value.len().min(96));
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            sanitized.push(ch);
        } else if !sanitized.ends_with('_') {
            sanitized.push('_');
        }
        if sanitized.len() >= 96 {
            break;
        }
    }

    let sanitized = sanitized.trim_matches(['.', '-', '_']).to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn print_qr_hint(content: &str) {
    println!("\nWeChat QR login required for anda_bot.");
    println!("Open or scan this QR content, then confirm login:");
    println!("{content}\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> config::WechatChannelSettings {
        config::WechatChannelSettings {
            id: Some("test".to_string()),
            user: None,
            bot_token: "token".to_string(),
            username: Some("anda-wechat".to_string()),
            allowed_users: vec!["alice".to_string(), "wxid_123".to_string()],
            allow_external_users: false,
            route_tag: None,
        }
    }

    #[test]
    fn wechat_channel_identity() {
        let channel = WechatChannel::new(&test_config());
        assert_eq!(channel.name(), "wechat");
        assert_eq!(channel.username(), "anda-wechat");
        assert_eq!(channel.id(), "wechat:test");
    }

    #[test]
    fn wechat_empty_bot_token_is_unconfigured() {
        let mut cfg = test_config();
        cfg.bot_token = "   ".to_string();

        let channel = WechatChannel::new(&cfg);

        assert!(channel.bot_token.is_none());
    }

    #[test]
    fn split_message_for_wechat_chunks_long_text() {
        let text = "a".repeat(WECHAT_MAX_MESSAGE_LENGTH + 10);
        let chunks = split_message_for_wechat(&text);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].chars().count() <= WECHAT_MAX_MESSAGE_LENGTH);
        assert!(chunks[1].chars().count() <= WECHAT_MAX_MESSAGE_LENGTH);
    }

    #[test]
    fn sanitize_path_component_uses_fallback() {
        assert_eq!(sanitize_path_component("../../", "media.bin"), "media.bin");
        assert_eq!(
            sanitize_path_component("hello world.txt", "media.bin"),
            "hello_world.txt"
        );
    }

    #[test]
    fn wechat_thread_ignores_empty_session_id() {
        assert_eq!(wechat_thread_from_session_id(None), None);
        assert_eq!(wechat_thread_from_session_id(Some("  ")), None);
    }

    #[test]
    fn wechat_thread_preserves_discussion_space() {
        assert_eq!(
            wechat_thread_from_session_id(Some(" room-7 ")).as_deref(),
            Some("room-7")
        );
    }

    #[test]
    fn wechat_reply_target_trims_sender_target() {
        assert_eq!(wechat_reply_target_from(" wxid_123 "), "wxid_123");
    }

    #[test]
    fn wechat_context_token_error_matches_ret_minus_two() {
        assert!(is_wechat_context_token_error(
            r#"API error: errcode=-2, errmsg={"ret":-2}"#
        ));
    }

    #[tokio::test]
    async fn save_context_token_merges_existing_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_path_buf());
        let mut existing = HashMap::new();
        existing.insert("alice".to_string(), "old-token".to_string());
        existing.insert("bob".to_string(), "bob-token".to_string());
        save_context_tokens_to_workspace(&workspace, &existing).await;

        save_context_token_to_workspace(&workspace, "alice", "new-token").await;

        let tokens = load_context_tokens_from_workspace(&workspace).await;
        assert_eq!(tokens.get("alice").map(String::as_str), Some("new-token"));
        assert_eq!(tokens.get("bob").map(String::as_str), Some("bob-token"));
    }

    #[tokio::test]
    async fn load_context_token_for_send_returns_fresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_path_buf());
        save_context_token_to_workspace(&workspace, "alice", "fresh-token").await;

        let token = load_context_token_for_send(&workspace, "alice").await;

        assert_eq!(token.as_deref(), Some("fresh-token"));
    }

    #[tokio::test]
    async fn load_context_token_for_send_removes_stale_token() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_path_buf());
        save_context_token_to_workspace(&workspace, "alice", "stale-token").await;
        let mut meta = HashMap::new();
        meta.insert(
            "alice".to_string(),
            unix_ms() - WECHAT_CONTEXT_TOKEN_MAX_AGE_MS - 1,
        );
        save_context_token_meta_to_workspace(&workspace, &meta).await;

        let token = load_context_token_for_send(&workspace, "alice").await;

        assert_eq!(token, None);
        let tokens = load_context_tokens_from_workspace(&workspace).await;
        assert!(!tokens.contains_key("alice"));
        let meta = load_context_token_meta_from_workspace(&workspace).await;
        assert!(!meta.contains_key("alice"));
    }

    #[tokio::test]
    async fn save_context_token_refreshes_meta_when_token_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_path_buf());
        save_context_token_to_workspace(&workspace, "alice", "same-token").await;
        let old_updated_at = unix_ms() - WECHAT_CONTEXT_TOKEN_MAX_AGE_MS - 1;
        let mut meta = HashMap::new();
        meta.insert("alice".to_string(), old_updated_at);
        save_context_token_meta_to_workspace(&workspace, &meta).await;

        save_context_token_to_workspace(&workspace, "alice", "same-token").await;

        let meta = load_context_token_meta_from_workspace(&workspace).await;
        assert!(
            meta.get("alice")
                .is_some_and(|updated| *updated > old_updated_at)
        );
    }

    fn workspace_at(dir: &tempfile::TempDir) -> Arc<ChannelWorkspace> {
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_path_buf());
        workspace
    }

    #[tokio::test]
    async fn sync_buf_round_trips_through_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = workspace_at(&dir);

        assert!(load_sync_buf_from_workspace(&workspace).await.is_none());
        save_sync_buf_to_workspace(&workspace, "buf-state-1").await;
        assert_eq!(
            load_sync_buf_from_workspace(&workspace).await.as_deref(),
            Some("buf-state-1")
        );

        // Without a workspace path both operations are no-ops.
        let detached = Arc::new(ChannelWorkspace::default());
        save_sync_buf_to_workspace(&detached, "ignored").await;
        assert!(load_sync_buf_from_workspace(&detached).await.is_none());
    }

    #[tokio::test]
    async fn context_tokens_load_for_send_until_stale() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = workspace_at(&dir);

        assert!(
            load_context_token_for_send(&workspace, "alice")
                .await
                .is_none()
        );
        assert!(load_context_token_for_send(&workspace, " ").await.is_none());

        save_context_token_to_workspace(&workspace, "alice", "tok-1").await;
        assert_eq!(
            load_context_token_for_send(&workspace, "alice")
                .await
                .as_deref(),
            Some("tok-1")
        );

        // A stale token is dropped on read.
        let mut meta = HashMap::new();
        meta.insert(
            "alice".to_string(),
            unix_ms() - WECHAT_CONTEXT_TOKEN_MAX_AGE_MS - 1,
        );
        save_context_token_meta_to_workspace(&workspace, &meta).await;
        assert!(
            load_context_token_for_send(&workspace, "alice")
                .await
                .is_none()
        );
        assert!(
            load_context_tokens_from_workspace(&workspace)
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn remove_context_token_respects_expected_value() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = workspace_at(&dir);

        save_context_token_to_workspace(&workspace, "alice", "tok-1").await;

        // Mismatched expectation keeps the newer token.
        remove_context_token_from_workspace(&workspace, "alice", Some("other")).await;
        assert_eq!(
            load_context_token_for_send(&workspace, "alice")
                .await
                .as_deref(),
            Some("tok-1")
        );

        remove_context_token_from_workspace(&workspace, "alice", Some("tok-1")).await;
        assert!(
            load_context_token_for_send(&workspace, "alice")
                .await
                .is_none()
        );
    }

    #[test]
    fn context_token_staleness_uses_max_age() {
        assert!(!context_token_is_stale(None));
        assert!(!context_token_is_stale(Some(unix_ms())));
        assert!(context_token_is_stale(Some(
            unix_ms() - WECHAT_CONTEXT_TOKEN_MAX_AGE_MS - 1
        )));
    }

    #[tokio::test]
    async fn resolve_token_prefers_config_then_saved_file() {
        let dir = tempfile::tempdir().unwrap();

        let channel = WechatChannel::new(&test_config());
        assert!(channel.resolve_token().await.is_ok());

        let mut cfg = test_config();
        cfg.bot_token = String::new();
        let channel = WechatChannel::new(&cfg);
        channel.set_workspace(dir.path().to_path_buf());
        let err = channel.resolve_token().await.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("no bot_token configured"));

        tokio::fs::write(dir.path().join("token.txt"), " saved-token \n")
            .await
            .unwrap();
        assert_eq!(channel.resolve_token().await.unwrap(), "saved-token");
    }

    #[tokio::test]
    async fn temp_media_paths_are_sanitized_under_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = workspace_at(&dir);

        let path = temp_media_path(&workspace, "msg/1", "weird name!.bin").await;
        assert!(path.starts_with(dir.path().join("incoming")));
        let file_name = path.file_name().unwrap().to_str().unwrap();
        assert!(!file_name.contains('/'));
        assert!(!file_name.contains('!'));
        assert!(!file_name.contains(' '));

        // Without a workspace the temp dir is used.
        let detached = Arc::new(ChannelWorkspace::default());
        let path = temp_media_path(&detached, "msg", "name.bin").await;
        assert!(path.starts_with(std::env::temp_dir()));
    }

    #[test]
    fn media_file_names_fall_back_by_type() {
        let media = |file_name: Option<&str>, url: Option<&str>, media_type: MediaType| MediaInfo {
            media_type,
            cdn_media: None,
            url: url.map(str::to_string),
            file_name: file_name.map(str::to_string),
            file_size: None,
            aes_key_base64: None,
        };

        assert_eq!(
            wechat_media_file_name(&media(Some("notes.pdf"), None, MediaType::File), "m1"),
            "notes.pdf"
        );
        assert_eq!(
            wechat_media_file_name(
                &media(
                    None,
                    Some("https://cdn.example.com/path/pic.jpg?x=1"),
                    MediaType::Image
                ),
                "m1"
            ),
            "pic.jpg"
        );
        assert_eq!(
            wechat_media_file_name(&media(None, None, MediaType::Image), "m1"),
            "m1.jpg"
        );
        assert_eq!(
            wechat_media_file_name(&media(None, None, MediaType::Video), "m1"),
            "m1.mp4"
        );
        assert_eq!(
            wechat_media_file_name(&media(None, None, MediaType::Voice), "m1"),
            "m1.silk"
        );
        assert_eq!(
            wechat_media_file_name(&media(None, None, MediaType::File), "m1"),
            "m1.bin"
        );
    }

    #[test]
    fn quoted_messages_and_attachments_render_labels() {
        assert_eq!(
            format_ref_message(&weixin_agent::RefMessageInfo {
                title: Some("Alice".to_string()),
                body: Some("original text".to_string()),
            }),
            "[Quoted: Alice | original text]"
        );
        assert_eq!(
            format_ref_message(&weixin_agent::RefMessageInfo {
                title: None,
                body: None,
            }),
            "[Quoted message]"
        );

        let image = Resource {
            name: "pic.png".to_string(),
            tags: vec!["image".to_string()],
            ..Default::default()
        };
        assert_eq!(attachment_fallback_text(&image), "[Image: pic.png]");
        // Inferred resources carry capitalized matcher tags; they must still be
        // recognized rather than falling through to the "Document" label.
        let inferred_image = Resource {
            name: "photo.jpg".to_string(),
            tags: vec!["Image".to_string()],
            ..Default::default()
        };
        assert_eq!(
            attachment_fallback_text(&inferred_image),
            "[Image: photo.jpg]"
        );
        let video = Resource {
            name: "clip.mp4".to_string(),
            tags: vec!["video".to_string()],
            ..Default::default()
        };
        assert_eq!(attachment_fallback_text(&video), "[Video: clip.mp4]");
        let audio = Resource {
            name: "voice.silk".to_string(),
            tags: vec!["audio".to_string()],
            ..Default::default()
        };
        assert_eq!(attachment_fallback_text(&audio), "[Audio: voice.silk]");
        let other = Resource {
            name: "doc.pdf".to_string(),
            ..Default::default()
        };
        assert_eq!(attachment_fallback_text(&other), "[Document: doc.pdf]");
    }

    #[test]
    fn identity_allowlist_normalizes_and_supports_wildcard() {
        let allowed = vec!["alice".to_string(), "*".to_string()];
        assert!(is_identity_allowed(&allowed, "@Alice"));
        assert!(is_identity_allowed(&allowed, "anyone"));
        assert!(!is_identity_allowed(&allowed, "  "));

        let strict = vec!["alice".to_string()];
        assert!(!is_identity_allowed(&strict, "bob"));
    }

    #[test]
    fn context_token_errors_match_known_signatures() {
        assert!(is_wechat_context_token_error("Session Expired"));
        assert!(is_wechat_context_token_error("errcode=-14"));
        assert!(is_wechat_context_token_error("\"ret\":-2"));
        assert!(is_wechat_context_token_error("invalid token"));
        assert!(is_wechat_context_token_error("the token has expired"));
        assert!(!is_wechat_context_token_error("network unreachable"));
    }

    #[test]
    fn should_retry_send_matches_transient_errors() {
        let channel = WechatChannel::new(&test_config());
        assert!(channel.should_retry_send("connection reset"));
        assert!(channel.should_retry_send("HTTP 503"));
        assert!(!channel.should_retry_send("400 bad request"));
    }
}
