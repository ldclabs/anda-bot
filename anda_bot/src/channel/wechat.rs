use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use weixin_agent::{
    LoginStatus, MediaInfo, MediaType, MessageContext, MessageHandler, Result as WeixinResult,
    StandaloneQrLogin, WeixinClient, WeixinConfig,
};

use super::{
    AttachmentKind, Channel, ChannelMessage, ChannelWorkspace, SendMessage, attachment_kind,
    default_file_name_for_resource, is_http_url, mime_type_for_path, resource_from_bytes,
};
use crate::config;

const WECHAT_MAX_MESSAGE_LENGTH: usize = 4000;
const WECHAT_CONTINUATION_OVERHEAD: usize = 30;
const WECHAT_MAX_FILE_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;
const WECHAT_QR_POLL_DELAY: Duration = Duration::from_secs(2);
const WECHAT_MAX_QR_REFRESH_COUNT: u32 = 3;

pub struct WechatChannelConfig {
    pub id: String,
    pub bot_token: Option<String>,
    pub username: Option<String>,
    pub allowed_users: Vec<String>,
    pub base_url: String,
    pub cdn_base_url: String,
    pub route_tag: Option<u32>,
}

impl WechatChannelConfig {
    fn from_settings(
        cfg: &config::WechatChannelSettings,
        _https_proxy: Option<String>,
    ) -> Result<Self, BoxError> {
        Ok(Self {
            id: cfg.channel_id(),
            bot_token: config::normalize_string(&cfg.bot_token),
            username: config::normalize_optional(&cfg.username),
            allowed_users: config::normalize_list(&cfg.allowed_users),
            base_url: config::normalize_optional(&cfg.base_url)
                .unwrap_or_else(|| config::DEFAULT_WECHAT_API_BASE.to_string()),
            cdn_base_url: config::normalize_optional(&cfg.cdn_base_url)
                .unwrap_or_else(|| config::DEFAULT_WECHAT_CDN_BASE.to_string()),
            route_tag: cfg.route_tag,
        })
    }
}

pub fn build_wechat_channels(
    cfg: &[config::WechatChannelSettings],
    https_proxy: Option<String>,
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

        let channel: Arc<dyn Channel> = Arc::new(WechatChannel::new(
            WechatChannelConfig::from_settings(wechat_cfg, https_proxy.clone())?,
        ));
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
    base_url: String,
    cdn_base_url: String,
    route_tag: Option<u32>,
    workspace: Arc<ChannelWorkspace>,
}

impl WechatChannel {
    pub fn new(cfg: WechatChannelConfig) -> Self {
        Self {
            id: if cfg.id.trim().is_empty() {
                "default".to_string()
            } else {
                cfg.id.trim().to_string()
            },
            bot_token: cfg.bot_token,
            username: cfg.username.unwrap_or_else(|| "wechat".to_string()),
            allowed_users: normalize_allowed_users(cfg.allowed_users),
            base_url: cfg.base_url,
            cdn_base_url: cfg.cdn_base_url,
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

    async fn ensure_workspace_dir(&self) -> Result<PathBuf, BoxError> {
        let path = self
            .workspace
            .path()
            .ok_or("WeChat channel workspace is not initialized")?;
        tokio::fs::create_dir_all(&path).await?;
        Ok(path)
    }

    async fn saved_token(&self) -> Option<String> {
        let path = self.workspace.path()?.join("token.txt");
        let token = tokio::fs::read_to_string(path).await.ok()?;
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

        let workspace = self.ensure_workspace_dir().await?;
        self.qr_login_and_save(&workspace).await
    }

    async fn qr_login_and_save(&self, workspace: &Path) -> Result<String, BoxError> {
        let config = self.build_weixin_config("")?;
        let qr = StandaloneQrLogin::new(&config);
        let mut session = qr.start(None).await?;
        print_qr_hint(&session.qrcode_img_content);

        let mut refresh_count = 0_u32;
        loop {
            match qr.poll_status(&session).await? {
                LoginStatus::Confirmed {
                    bot_token,
                    ilink_bot_id,
                    base_url,
                    ilink_user_id,
                } => {
                    log::info!(
                        "WeChat QR login confirmed: bot_id={ilink_bot_id}, user_id={ilink_user_id}, base_url={base_url}"
                    );
                    tokio::fs::write(workspace.join("token.txt"), &bot_token).await?;
                    return Ok(bot_token);
                }
                LoginStatus::Scanned => {
                    log::info!("WeChat QR code scanned; waiting for confirmation");
                }
                LoginStatus::Expired => {
                    refresh_count += 1;
                    if refresh_count >= WECHAT_MAX_QR_REFRESH_COUNT {
                        return Err("WeChat QR code expired too many times".into());
                    }
                    log::warn!(
                        "WeChat QR code expired; refreshing ({refresh_count}/{WECHAT_MAX_QR_REFRESH_COUNT})"
                    );
                    session = qr.start(None).await?;
                    print_qr_hint(&session.qrcode_img_content);
                }
                LoginStatus::Wait | LoginStatus::ScannedButRedirect { .. } => {}
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

            let path = uri.strip_prefix("file://").unwrap_or(uri);
            if Path::new(path).exists() {
                client
                    .send_media(recipient, Path::new(path), context_token)
                    .await?;
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

    async fn write_outgoing_blob(
        &self,
        resource: &Resource,
        bytes: &[u8],
    ) -> Result<PathBuf, BoxError> {
        let base = self.workspace.path().unwrap_or_else(std::env::temp_dir);
        let dir = base.join("outgoing");
        tokio::fs::create_dir_all(&dir).await?;

        let file_name = if resource.name.trim().is_empty() {
            default_file_name_for_resource(resource).to_string()
        } else {
            resource.name.clone()
        };
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

    async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
        let recipient = message.recipient.trim();
        if recipient.is_empty() {
            return Err("WeChat recipient is empty".into());
        }

        let token = self.resolve_token().await?;
        let client = self.build_client(&token, NoopMessageHandler, CancellationToken::new())?;
        client
            .context_tokens()
            .import(self.load_context_tokens().await);
        let context_token = client.context_tokens().get(recipient);
        let context_token = context_token.as_deref();

        if !message.content.trim().is_empty() {
            self.send_text_chunks(&client, recipient, &message.content, context_token)
                .await?;
        }

        for resource in &message.attachments {
            self.send_resource(&client, recipient, resource, context_token)
                .await?;
        }

        if message.content.trim().is_empty() && message.attachments.is_empty() {
            self.send_text_chunks(&client, recipient, " ", context_token)
                .await?;
        }

        self.save_context_tokens(&client.context_tokens().export_all())
            .await;
        Ok(())
    }

    fn should_retry_send(&self, error: &str) -> bool {
        let error = error.to_ascii_lowercase();
        error.contains("timeout")
            || error.contains("connection")
            || error.contains("temporarily")
            || error.contains("too many requests")
            || error.contains("429")
            || error.contains("502")
            || error.contains("503")
            || error.contains("504")
            || error.contains("session expired")
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
    tx: mpsc::Sender<ChannelMessage>,
    workspace: Arc<ChannelWorkspace>,
    cancel_token: CancellationToken,
}

#[async_trait]
impl MessageHandler for WechatMessageHandler {
    async fn on_message(&self, ctx: &MessageContext) -> WeixinResult<()> {
        if !is_identity_allowed(&self.allowed_users, &ctx.from) {
            log::warn!(
                "WeChat ignoring message from unauthorized user: {}",
                ctx.from
            );
            return Ok(());
        }

        let Some(message) =
            channel_message_from_context(ctx, &self.channel_id, &self.workspace).await
        else {
            return Ok(());
        };

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
        && let Some(resource) = download_media_resource(ctx, media, workspace).await {
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

    let mut extra = std::collections::BTreeMap::new();
    extra.insert("message_id".to_string(), ctx.message_id.clone().into());
    extra.insert("to".to_string(), ctx.to.clone().into());
    if let Some(server_message_id) = ctx.server_message_id {
        extra.insert("server_message_id".to_string(), server_message_id.into());
    }
    if let Some(session_id) = &ctx.session_id {
        extra.insert("session_id".to_string(), session_id.clone().into());
    }

    Some(ChannelMessage {
        sender: ctx.from.clone(),
        reply_target: ctx.from.clone(),
        content,
        channel: channel_id.to_string(),
        timestamp,
        thread: ctx.session_id.clone(),
        attachments,
        extra,
        ..Default::default()
    })
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
            if let Some(url) = media.url.as_deref() {
                match download_url_bytes(url, WECHAT_MAX_FILE_DOWNLOAD_BYTES).await {
                    Ok(bytes) => bytes,
                    Err(url_err) => {
                        log::warn!(
                            "WeChat failed to download media via SDK ({err}) or URL ({url_err})"
                        );
                        return None;
                    }
                }
            } else {
                log::warn!("WeChat failed to download media: {err}");
                return None;
            }
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

    let mime_type = wechat_media_mime(media, &file_name);
    let kind = wechat_attachment_kind(media.media_type, &file_name, mime_type.as_deref());
    let mut resource = resource_from_bytes(kind, file_name, mime_type, bytes, "WeChat attachment");
    workspace
        .store_resource_lossy(&mut resource, Some(&ctx.message_id), "WeChat attachment")
        .await;
    Some(resource)
}

async fn download_url_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, BoxError> {
    let response = reqwest::get(url).await?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP media download failed ({status})").into());
    }
    if let Some(length) = response.content_length()
        && length > max_bytes
    {
        return Err(format!("HTTP media is too large: {length} bytes").into());
    }
    let bytes = response.bytes().await?.to_vec();
    if bytes.len() as u64 > max_bytes {
        return Err(format!("HTTP media is too large: {} bytes", bytes.len()).into());
    }
    Ok(bytes)
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
    let label = if resource.tags.iter().any(|tag| tag == "image") {
        "Image"
    } else if resource.tags.iter().any(|tag| tag == "video") {
        "Video"
    } else if resource.tags.iter().any(|tag| tag == "audio") {
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

fn wechat_media_mime(media: &MediaInfo, file_name: &str) -> Option<String> {
    mime_type_for_path(file_name)
        .map(String::from)
        .or_else(|| match media.media_type {
            MediaType::Image => Some("image/jpeg".to_string()),
            MediaType::Video => Some("video/mp4".to_string()),
            MediaType::Voice => Some("audio/silk".to_string()),
            MediaType::File => None,
        })
}

fn wechat_attachment_kind(
    media_type: MediaType,
    file_name: &str,
    mime_type: Option<&str>,
) -> AttachmentKind {
    match media_type {
        MediaType::Image => AttachmentKind::Image,
        MediaType::Video => AttachmentKind::Video,
        MediaType::Voice => AttachmentKind::Audio,
        MediaType::File => attachment_kind(file_name, mime_type),
    }
}

async fn load_sync_buf_from_workspace(workspace: &Arc<ChannelWorkspace>) -> Option<String> {
    let path = workspace.path()?.join("sync_buf.json");
    let data = tokio::fs::read_to_string(path).await.ok()?;
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
    let Ok(data) = tokio::fs::read_to_string(path.join("context_tokens.json")).await else {
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
    if let Err(err) = tokio::fs::write(path.join("context_tokens.json"), data).await {
        log::warn!("failed to save WeChat context tokens: {err}");
    }
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

fn normalize_identity(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_allowed_users(allowed_users: Vec<String>) -> Vec<String> {
    allowed_users
        .into_iter()
        .filter_map(|entry| {
            let normalized = normalize_identity(&entry);
            (!normalized.is_empty()).then_some(normalized)
        })
        .collect()
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

    fn test_config() -> WechatChannelConfig {
        WechatChannelConfig {
            id: "test".to_string(),
            bot_token: Some("token".to_string()),
            username: Some("anda-wechat".to_string()),
            allowed_users: vec!["alice".to_string(), "wxid_123".to_string()],
            base_url: config::DEFAULT_WECHAT_API_BASE.to_string(),
            cdn_base_url: config::DEFAULT_WECHAT_CDN_BASE.to_string(),
            route_tag: None,
        }
    }

    #[test]
    fn wechat_channel_identity() {
        let channel = WechatChannel::new(test_config());
        assert_eq!(channel.name(), "wechat");
        assert_eq!(channel.username(), "anda-wechat");
        assert_eq!(channel.id(), "wechat:test");
    }

    #[test]
    fn allowed_users_match_exact_identity() {
        let channel = WechatChannel::new(test_config());
        assert!(is_identity_allowed(&channel.allowed_users, "alice"));
        assert!(is_identity_allowed(&channel.allowed_users, "wxid_123"));
        assert!(!is_identity_allowed(&channel.allowed_users, "bob"));
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
}
