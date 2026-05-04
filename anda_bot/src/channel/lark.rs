use anda_core::{BoxError, Resource};
use anda_db::unix_ms;
use async_trait::async_trait;
use futures_util::{Sink, SinkExt, StreamExt};
use prost::Message as ProstMessage;
use reqwest::Client;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock as StdRwLock},
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};
use tokio_util::sync::CancellationToken;

use super::{
    Channel, ChannelMessage, ChannelWorkspace, SendMessage, file_name_for_resource, is_http_url,
    resource_from_bytes,
};
use crate::config::{self, normalize_identity};

const LARK_CARD_MARKDOWN_MAX_BYTES: usize = 28_000;
const LARK_MAX_FILE_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;
const LARK_MAX_AUDIO_BYTES: u64 = 25 * 1024 * 1024;
const LARK_TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(120);
const LARK_DEFAULT_TOKEN_TTL: Duration = Duration::from_secs(7200);
const LARK_INVALID_ACCESS_TOKEN_CODE: i64 = 99_991_663;
const LARK_WS_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(300);

const LARK_ACK_REACTIONS_ZH_CN: &[&str] = &[
    "OK", "JIAYI", "APPLAUSE", "THUMBSUP", "MUSCLE", "SMILE", "DONE",
];
const LARK_ACK_REACTIONS_ZH_TW: &[&str] = &[
    "OK",
    "JIAYI",
    "APPLAUSE",
    "THUMBSUP",
    "FINGERHEART",
    "SMILE",
    "DONE",
];
const LARK_ACK_REACTIONS_EN: &[&str] = &[
    "OK",
    "THUMBSUP",
    "THANKS",
    "MUSCLE",
    "FINGERHEART",
    "APPLAUSE",
    "SMILE",
    "DONE",
];
const LARK_ACK_REACTIONS_JA: &[&str] = &[
    "OK",
    "THUMBSUP",
    "THANKS",
    "MUSCLE",
    "FINGERHEART",
    "APPLAUSE",
    "SMILE",
    "DONE",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LarkAckLocale {
    ZhCn,
    ZhTw,
    En,
    Ja,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PbHeader {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(string, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PbFrame {
    #[prost(uint64, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, tag = "2")]
    pub log_id: u64,
    #[prost(int32, tag = "3")]
    pub service: i32,
    #[prost(int32, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<PbHeader>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
}

impl PbFrame {
    fn header_value<'a>(&'a self, key: &str) -> &'a str {
        self.headers
            .iter()
            .find(|header| header.key == key)
            .map(|header| header.value.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
struct WsClientConfig {
    #[serde(rename = "PingInterval")]
    ping_interval: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct WsEndpointResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<WsEndpoint>,
}

#[derive(Debug, serde::Deserialize)]
struct WsEndpoint {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "ClientConfig")]
    client_config: Option<WsClientConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct LarkEventEnvelope {
    header: LarkEventHeader,
    event: Value,
}

#[derive(Debug, serde::Deserialize)]
struct LarkEventHeader {
    event_type: String,
}

#[derive(Debug, serde::Deserialize)]
struct LarkReceiveEvent {
    sender: LarkSender,
    message: LarkMessage,
}

#[derive(Debug, serde::Deserialize)]
struct LarkSender {
    sender_id: LarkSenderId,
    #[serde(default)]
    sender_type: String,
}

#[derive(Debug, serde::Deserialize, Default)]
struct LarkSenderId {
    open_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct LarkMessage {
    message_id: String,
    chat_id: String,
    chat_type: String,
    message_type: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    mentions: Vec<Value>,
    #[serde(default)]
    create_time: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedTenantToken {
    value: String,
    refresh_after: Instant,
}

pub fn build_lark_channels(
    cfg: &[config::LarkChannelSettings],
    client: Client,
) -> Result<HashMap<String, Arc<dyn Channel>>, BoxError> {
    let mut channels = HashMap::new();

    for (index, lark_cfg) in cfg.iter().enumerate() {
        if lark_cfg.is_empty() {
            continue;
        }

        if lark_cfg.app_id.trim().is_empty() || lark_cfg.app_secret.trim().is_empty() {
            return Err(format!(
                "Lark channel '{}' requires app_id and app_secret",
                lark_cfg.label(index)
            )
            .into());
        }

        if lark_cfg.receive_mode == config::LarkReceiveMode::Webhook && lark_cfg.port.is_none() {
            return Err(format!(
                "Lark channel '{}' webhook mode requires port",
                lark_cfg.label(index)
            )
            .into());
        }

        let channel: Arc<dyn Channel> = Arc::new(LarkChannel::new(lark_cfg, client.clone()));
        let channel_id = channel.id();
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate Lark channel id '{channel_id}'").into());
        }
    }

    Ok(channels)
}

#[derive(Clone)]
pub struct LarkChannel {
    id: String,
    app_id: String,
    app_secret: String,
    username: String,
    verification_token: String,
    port: Option<u16>,
    allowed_users: Vec<String>,
    mention_only: bool,
    platform: config::LarkPlatform,
    receive_mode: config::LarkReceiveMode,
    api_base: String,
    ws_base: String,
    ack_reactions: bool,
    client: Client,
    workspace: Arc<ChannelWorkspace>,
    bot_open_id: Arc<StdRwLock<Option<String>>>,
    tenant_token: Arc<RwLock<Option<CachedTenantToken>>>,
    ws_seen_ids: Arc<RwLock<HashMap<String, Instant>>>,
}

impl LarkChannel {
    pub fn new(cfg: &config::LarkChannelSettings, client: Client) -> Self {
        let channel_name = cfg.platform.channel_name();
        Self {
            id: cfg.channel_id(),
            app_id: cfg.app_id.clone(),
            app_secret: cfg.app_secret.clone(),
            username: cfg
                .username
                .clone()
                .unwrap_or_else(|| channel_name.to_string()),
            verification_token: cfg.verification_token.clone().unwrap_or_default(),
            port: cfg.port,
            allowed_users: cfg
                .allowed_users
                .iter()
                .map(|s| normalize_identity(s))
                .collect(),
            mention_only: cfg.mention_only,
            platform: cfg.platform,
            receive_mode: cfg.receive_mode,
            api_base: cfg.platform.api_base().to_string(),
            ws_base: cfg.platform.ws_base().to_string(),
            ack_reactions: cfg.ack_reactions,
            client,
            workspace: Arc::new(ChannelWorkspace::default()),
            bot_open_id: Arc::new(StdRwLock::new(None)),
            tenant_token: Arc::new(RwLock::new(None)),
            ws_seen_ids: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn channel_name(&self) -> &'static str {
        self.platform.channel_name()
    }

    fn tenant_access_token_url(&self) -> String {
        format!("{}/auth/v3/tenant_access_token/internal", self.api_base)
    }

    fn bot_info_url(&self) -> String {
        format!("{}/bot/v3/info", self.api_base)
    }

    fn send_message_url(&self) -> String {
        format!("{}/im/v1/messages?receive_id_type=chat_id", self.api_base)
    }

    fn message_reaction_url(&self, message_id: &str) -> String {
        format!("{}/im/v1/messages/{message_id}/reactions", self.api_base)
    }

    fn image_download_url(&self, image_key: &str) -> String {
        format!("{}/im/v1/images/{image_key}", self.api_base)
    }

    fn file_download_url(&self, message_id: &str, file_key: &str) -> String {
        format!(
            "{}/im/v1/messages/{message_id}/resources/{file_key}?type=file",
            self.api_base
        )
    }

    fn is_user_allowed(&self, open_id: &str) -> bool {
        let open_id = open_id.trim();
        !open_id.is_empty()
            && self
                .allowed_users
                .iter()
                .any(|allowed| allowed == "*" || allowed == open_id)
    }

    fn resolved_bot_open_id(&self) -> Option<String> {
        self.bot_open_id.read().ok().and_then(|guard| guard.clone())
    }

    fn set_resolved_bot_open_id(&self, open_id: Option<String>) {
        if let Ok(mut guard) = self.bot_open_id.write() {
            *guard = open_id;
        }
    }

    async fn get_tenant_access_token(&self) -> Result<String, BoxError> {
        {
            let cached = self.tenant_token.read().await;
            if let Some(token) = cached.as_ref()
                && Instant::now() < token.refresh_after
            {
                return Ok(token.value.clone());
            }
        }

        let response = self
            .client
            .post(self.tenant_access_token_url())
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Lark tenant_access_token failed ({status}): {body}").into());
        }

        let data: Value = serde_json::from_str(&body)?;
        let code = extract_lark_response_code(&data).unwrap_or(-1);
        if code != 0 {
            let msg = data
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("Lark tenant_access_token failed: {msg}").into());
        }

        let token = data
            .get("tenant_access_token")
            .and_then(Value::as_str)
            .ok_or("Lark tenant_access_token response missing tenant_access_token")?
            .to_string();
        let ttl_seconds = extract_lark_token_ttl_seconds(&data);
        let refresh_after = next_token_refresh_deadline(Instant::now(), ttl_seconds);

        let mut cached = self.tenant_token.write().await;
        *cached = Some(CachedTenantToken {
            value: token.clone(),
            refresh_after,
        });

        Ok(token)
    }

    async fn invalidate_token(&self) {
        let mut cached = self.tenant_token.write().await;
        *cached = None;
    }

    async fn fetch_bot_open_id_with_token(
        &self,
        token: &str,
    ) -> Result<(reqwest::StatusCode, Value), BoxError> {
        let response = self
            .client
            .get(self.bot_info_url())
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let data = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
        Ok((status, data))
    }

    async fn refresh_bot_open_id(&self) -> Result<Option<String>, BoxError> {
        let token = self.get_tenant_access_token().await?;
        let (status, body) = self.fetch_bot_open_id_with_token(&token).await?;

        let body = if should_refresh_lark_tenant_token(status, &body) {
            self.invalidate_token().await;
            let token = self.get_tenant_access_token().await?;
            let (retry_status, retry_body) = self.fetch_bot_open_id_with_token(&token).await?;
            if !retry_status.is_success() {
                return Err(format!(
                    "Lark bot info failed after token refresh ({retry_status}): {retry_body}"
                )
                .into());
            }
            retry_body
        } else {
            if !status.is_success() {
                return Err(format!("Lark bot info failed ({status}): {body}").into());
            }
            body
        };

        let code = extract_lark_response_code(&body).unwrap_or(-1);
        if code != 0 {
            return Err(format!("Lark bot info failed: code={code}, body={body}").into());
        }

        let open_id = body
            .pointer("/bot/open_id")
            .or_else(|| body.pointer("/data/bot/open_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.set_resolved_bot_open_id(open_id.clone());
        Ok(open_id)
    }

    async fn ensure_bot_open_id(&self) {
        if !self.mention_only || self.resolved_bot_open_id().is_some() {
            return;
        }

        match self.refresh_bot_open_id().await {
            Ok(Some(open_id)) => log::info!("Lark resolved bot open_id: {open_id}"),
            Ok(None) => {
                log::warn!("Lark bot open_id missing; mention_only group messages will be ignored")
            }
            Err(err) => log::warn!(
                "Lark failed to resolve bot open_id: {err}; mention_only group messages will be ignored"
            ),
        }
    }

    async fn post_json_with_token(
        &self,
        url: &str,
        token: &str,
        body: &Value,
    ) -> Result<(reqwest::StatusCode, Value), BoxError> {
        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        let data = serde_json::from_str::<Value>(&raw)
            .unwrap_or_else(|_| serde_json::json!({ "raw": raw }));
        Ok((status, data))
    }

    async fn send_text_once(
        &self,
        url: &str,
        token: &str,
        body: &Value,
    ) -> Result<(reqwest::StatusCode, Value), BoxError> {
        self.post_json_with_token(url, token, body).await
    }

    async fn post_message_reaction_with_token(
        &self,
        message_id: &str,
        token: &str,
        emoji_type: &str,
    ) -> Result<(reqwest::StatusCode, Value), BoxError> {
        let body = serde_json::json!({
            "reaction_type": {
                "emoji_type": emoji_type,
            }
        });
        self.post_json_with_token(&self.message_reaction_url(message_id), token, &body)
            .await
    }

    async fn try_add_ack_reaction(&self, message_id: &str, emoji_type: &str) {
        if message_id.trim().is_empty() {
            return;
        }

        let mut token = match self.get_tenant_access_token().await {
            Ok(token) => token,
            Err(err) => {
                log::debug!("Lark failed to fetch token for ACK reaction: {err}");
                return;
            }
        };

        let mut retried = false;
        loop {
            let (status, body) = match self
                .post_message_reaction_with_token(message_id, &token, emoji_type)
                .await
            {
                Ok(result) => result,
                Err(err) => {
                    log::debug!("Lark failed to add ACK reaction: {err}");
                    return;
                }
            };

            if should_refresh_lark_tenant_token(status, &body) && !retried {
                self.invalidate_token().await;
                token = match self.get_tenant_access_token().await {
                    Ok(token) => token,
                    Err(err) => {
                        log::debug!("Lark failed to refresh token for ACK reaction: {err}");
                        return;
                    }
                };
                retried = true;
                continue;
            }

            if !status.is_success() || extract_lark_response_code(&body).unwrap_or(0) != 0 {
                log::debug!("Lark add ACK reaction failed ({status}): {body}");
            }
            return;
        }
    }

    async fn get_ws_endpoint(&self) -> Result<(String, WsClientConfig), BoxError> {
        let response = self
            .client
            .post(format!("{}/callback/ws/endpoint", self.ws_base))
            .header("locale", self.platform.locale_header())
            .json(&serde_json::json!({
                "AppID": self.app_id,
                "AppSecret": self.app_secret,
            }))
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Lark WS endpoint failed ({status}): {body}").into());
        }

        let data: WsEndpointResp = serde_json::from_str(&body)?;
        if data.code != 0 {
            return Err(format!(
                "Lark WS endpoint failed: code={} msg={}",
                data.code,
                data.msg.as_deref().unwrap_or("unknown")
            )
            .into());
        }

        let endpoint = data.data.ok_or("Lark WS endpoint response missing data")?;
        Ok((endpoint.url, endpoint.client_config.unwrap_or_default()))
    }

    async fn listen_ws(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        self.ensure_bot_open_id().await;
        let (wss_url, client_config) = self.get_ws_endpoint().await?;
        let service_id = service_id_from_ws_url(&wss_url);

        log::info!("Lark channel {} connecting to websocket", self.id());
        let (ws_stream, _) = tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            result = connect_async(&wss_url) => result?,
        };
        let (mut write, mut read) = ws_stream.split();

        let mut ping_secs = client_config.ping_interval.unwrap_or(120).max(10);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(ping_secs));
        let mut timeout_check = tokio::time::interval(Duration::from_secs(10));
        heartbeat.tick().await;

        let mut seq = 0_u64;
        send_lark_ws_ping(&mut write, &mut seq, service_id).await?;
        let mut last_recv = Instant::now();
        type FragEntry = (Vec<Option<Vec<u8>>>, Instant);
        let mut frag_cache: HashMap<String, FragEntry> = HashMap::new();

        log::info!("Lark channel {} listening for messages", self.id());

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => return Ok(()),
                _ = heartbeat.tick() => {
                    if send_lark_ws_ping(&mut write, &mut seq, service_id).await.is_err() {
                        return Ok(());
                    }
                    let cutoff = Instant::now()
                        .checked_sub(Duration::from_secs(300))
                        .unwrap_or_else(Instant::now);
                    frag_cache.retain(|_, (_, created_at)| *created_at > cutoff);
                }
                _ = timeout_check.tick() => {
                    if last_recv.elapsed() > LARK_WS_HEARTBEAT_TIMEOUT {
                        log::warn!("Lark websocket heartbeat timeout");
                        return Ok(());
                    }
                }
                message = read.next() => {
                    let raw = match message {
                        Some(Ok(ws_msg)) => {
                            if should_refresh_last_recv(&ws_msg) {
                                last_recv = Instant::now();
                            }
                            match ws_msg {
                                WsMsg::Binary(bytes) => bytes,
                                WsMsg::Ping(payload) => {
                                    let _ = write.send(WsMsg::Pong(payload)).await;
                                    continue;
                                }
                                WsMsg::Close(_) => return Ok(()),
                                _ => continue,
                            }
                        }
                        Some(Err(err)) => {
                            log::warn!("Lark websocket read error: {err}");
                            return Ok(());
                        }
                        None => return Ok(()),
                    };

                    let frame = match PbFrame::decode(raw.as_ref()) {
                        Ok(frame) => frame,
                        Err(err) => {
                            log::debug!("Lark websocket frame decode error: {err}");
                            continue;
                        }
                    };

                    if frame.method == 0 {
                        if frame.header_value("type") == "pong"
                            && let Some(payload) = &frame.payload
                            && let Ok(cfg) = serde_json::from_slice::<WsClientConfig>(payload)
                            && let Some(next_ping_secs) = cfg.ping_interval.map(|secs| secs.max(10))
                            && next_ping_secs != ping_secs
                        {
                            ping_secs = next_ping_secs;
                            heartbeat = tokio::time::interval(Duration::from_secs(ping_secs));
                        }
                        continue;
                    }

                    let mut ack = frame.clone();
                    ack.payload = Some(br#"{"code":200,"headers":{},"data":[]}"#.to_vec());
                    ack.headers.push(PbHeader {
                        key: "biz_rt".to_string(),
                        value: "0".to_string(),
                    });
                    let _ = write.send(WsMsg::Binary(ack.encode_to_vec().into())).await;

                    let payload = match reassemble_lark_ws_payload(frame, &mut frag_cache) {
                        Some(payload) => payload,
                        None => continue,
                    };

                    let event: LarkEventEnvelope = match serde_json::from_slice(&payload) {
                        Ok(event) => event,
                        Err(err) => {
                            log::debug!("Lark websocket event JSON parse error: {err}");
                            continue;
                        }
                    };
                    if event.header.event_type != "im.message.receive_v1" {
                        continue;
                    }

                    let Some(message) = self.parse_event_object(&event.event).await else {
                        continue;
                    };

                    if self.ack_reactions
                        && let Some(message_id) = event.event.pointer("/message/message_id").and_then(Value::as_str)
                    {
                        self.spawn_ack_reaction(message_id.to_string(), Some(event.event.clone()), message.content.clone());
                    }

                    if tx.send(message).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn listen_http(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        self.ensure_bot_open_id().await;
        use axum::{Json, Router, extract::State, routing::post};

        #[derive(Clone)]
        struct AppState {
            verification_token: String,
            channel: Arc<LarkChannel>,
            tx: mpsc::Sender<ChannelMessage>,
        }

        async fn handle_event(
            State(state): State<AppState>,
            Json(payload): Json<Value>,
        ) -> axum::response::Response {
            use axum::http::StatusCode;
            use axum::response::IntoResponse;

            if let Some(challenge) = payload.get("challenge").and_then(Value::as_str) {
                let token_ok = state.verification_token.trim().is_empty()
                    || payload
                        .get("token")
                        .and_then(Value::as_str)
                        .is_some_and(|token| token == state.verification_token);
                if !token_ok {
                    return (StatusCode::FORBIDDEN, "invalid token").into_response();
                }
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "challenge": challenge })),
                )
                    .into_response();
            }

            if !state.verification_token.trim().is_empty()
                && let Some(token) = payload.get("token").and_then(Value::as_str)
                && token != state.verification_token
            {
                return (StatusCode::FORBIDDEN, "invalid token").into_response();
            }

            let messages = state.channel.parse_event_payload(&payload).await;
            if !messages.is_empty()
                && state.channel.ack_reactions
                && let Some(message_id) = payload
                    .pointer("/event/message/message_id")
                    .and_then(Value::as_str)
            {
                let ack_text = messages
                    .first()
                    .map_or("", |message| message.content.as_str());
                state.channel.spawn_ack_reaction(
                    message_id.to_string(),
                    payload.get("event").cloned(),
                    ack_text.to_string(),
                );
            }

            for message in messages {
                if state.tx.send(message).await.is_err() {
                    break;
                }
            }

            (StatusCode::OK, "ok").into_response()
        }

        let port = self.port.ok_or("Lark webhook mode requires port")?;
        let state = AppState {
            verification_token: self.verification_token.clone(),
            channel: Arc::new(self.clone()),
            tx,
        };
        let app = Router::new()
            .route("/lark", post(handle_event))
            .with_state(state);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        let listener = tokio::net::TcpListener::bind(addr).await?;
        log::info!("Lark webhook listener started on {addr}");

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel_token.cancelled().await;
            })
            .await?;

        Ok(())
    }

    async fn parse_event_payload(&self, payload: &Value) -> Vec<ChannelMessage> {
        let event_type = payload
            .pointer("/header/event_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if event_type != "im.message.receive_v1" {
            return Vec::new();
        }

        let Some(event) = payload.get("event") else {
            return Vec::new();
        };

        self.parse_event_object(event).await.into_iter().collect()
    }

    async fn parse_event_object(&self, event: &Value) -> Option<ChannelMessage> {
        let recv: LarkReceiveEvent = match serde_json::from_value(event.clone()) {
            Ok(recv) => recv,
            Err(err) => {
                log::debug!("Lark event parse error: {err}");
                return None;
            }
        };

        if matches!(recv.sender.sender_type.as_str(), "app" | "bot") {
            return None;
        }

        let sender_open_id = recv.sender.sender_id.open_id.as_deref().unwrap_or("");
        if !self.is_user_allowed(sender_open_id) {
            log::warn!("Lark ignoring message from unauthorized user: open_id={sender_open_id}");
            return None;
        }

        let lark_message = recv.message;
        if self.is_duplicate_message(&lark_message.message_id).await {
            return None;
        }

        let parsed = match self
            .parse_message_content(
                &lark_message.message_type,
                &lark_message.content,
                &lark_message.message_id,
            )
            .await
        {
            Some(parsed) => parsed,
            None => return None,
        };

        if lark_message.chat_type == "group"
            && !should_respond_in_group(
                self.mention_only,
                self.resolved_bot_open_id().as_deref(),
                &lark_message.mentions,
                &parsed.post_mentioned_open_ids,
            )
        {
            return None;
        }

        let content = strip_at_placeholders(&parsed.content).trim().to_string();
        if content.is_empty() && parsed.attachments.is_empty() {
            return None;
        }

        let timestamp = lark_message
            .create_time
            .as_deref()
            .and_then(|time| time.parse::<u64>().ok())
            .unwrap_or_else(unix_ms);

        let mut extra = std::collections::BTreeMap::new();
        extra.insert("message_id".to_string(), lark_message.message_id.into());
        extra.insert("chat_id".to_string(), lark_message.chat_id.clone().into());
        extra.insert("open_id".to_string(), sender_open_id.to_string().into());
        extra.insert("message_type".to_string(), lark_message.message_type.into());

        Some(ChannelMessage {
            sender: sender_open_id.to_string(),
            reply_target: lark_message.chat_id,
            content,
            channel: self.id(),
            timestamp,
            thread: None,
            attachments: parsed.attachments,
            extra,
            ..Default::default()
        })
    }

    async fn is_duplicate_message(&self, message_id: &str) -> bool {
        if message_id.trim().is_empty() {
            return false;
        }

        let now = Instant::now();
        let mut seen = self.ws_seen_ids.write().await;
        seen.retain(|_, instant| now.duration_since(*instant) < Duration::from_secs(30 * 60));
        if seen.contains_key(message_id) {
            return true;
        }
        seen.insert(message_id.to_string(), now);
        false
    }

    async fn parse_message_content(
        &self,
        message_type: &str,
        content: &str,
        message_id: &str,
    ) -> Option<ParsedMessageContent> {
        match message_type {
            "text" => serde_json::from_str::<Value>(content)
                .ok()
                .and_then(|value| {
                    value
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .map(|text| ParsedMessageContent::text(text.to_string()))
                }),
            "post" => parse_post_content_details(content).map(|details| ParsedMessageContent {
                content: details.text,
                attachments: Vec::new(),
                post_mentioned_open_ids: details.mentioned_open_ids,
            }),
            "list" => parse_list_content(content).map(ParsedMessageContent::text),
            "image" => self.parse_image_message(message_id, content).await,
            "file" => self.parse_file_message(message_id, content).await,
            "audio" => self.parse_audio_message(message_id, content).await,
            _ => {
                log::debug!("Lark skipping unsupported message type: {message_type}");
                None
            }
        }
    }

    async fn parse_image_message(
        &self,
        message_id: &str,
        content: &str,
    ) -> Option<ParsedMessageContent> {
        let image_key = serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|value| {
                value
                    .get("image_key")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })?;

        match self.download_image_resource(message_id, &image_key).await {
            Some(resource) => Some(ParsedMessageContent::with_attachment(
                format!("[Image: {}]", resource.name),
                resource,
            )),
            None => Some(ParsedMessageContent::text(format!(
                "[Image: {image_key} | download failed]"
            ))),
        }
    }

    async fn parse_file_message(
        &self,
        message_id: &str,
        content: &str,
    ) -> Option<ParsedMessageContent> {
        let value = serde_json::from_str::<Value>(content).ok()?;
        let file_key = value.get("file_key").and_then(Value::as_str)?.to_string();
        let file_name = value
            .get("file_name")
            .and_then(Value::as_str)
            .unwrap_or(&file_key);

        match self
            .download_file_resource(message_id, &file_key, file_name)
            .await
        {
            Some(resource) => Some(ParsedMessageContent::with_attachment(
                format!("[File: {}]", resource.name),
                resource,
            )),
            None => Some(ParsedMessageContent::text(format!(
                "[File: {file_name} | download failed]"
            ))),
        }
    }

    async fn parse_audio_message(
        &self,
        message_id: &str,
        content: &str,
    ) -> Option<ParsedMessageContent> {
        let value = serde_json::from_str::<Value>(content).ok()?;
        let file_key = value.get("file_key").and_then(Value::as_str)?.to_string();
        match self.download_audio_resource(message_id, &file_key).await {
            Some(resource) => Some(ParsedMessageContent::with_attachment(
                String::new(),
                resource,
            )),
            None => Some(ParsedMessageContent::text(
                "[Audio message | download failed]".to_string(),
            )),
        }
    }

    async fn download_image_resource(&self, message_id: &str, image_key: &str) -> Option<Resource> {
        let bytes = match self
            .get_authorized_bytes(
                &self.image_download_url(image_key),
                LARK_MAX_FILE_DOWNLOAD_BYTES,
                "Lark image download",
            )
            .await
        {
            Ok(result) => result,
            Err(err) => {
                log::warn!("Lark failed to download image {image_key}: {err}");
                return None;
            }
        };

        let mut resource = resource_from_bytes(image_key.to_string(), bytes, "Lark image");
        self.workspace
            .store_resource_lossy(&mut resource, Some(message_id), "Lark image")
            .await;
        Some(resource)
    }

    async fn download_file_resource(
        &self,
        message_id: &str,
        file_key: &str,
        file_name: &str,
    ) -> Option<Resource> {
        let bytes = match self
            .get_authorized_bytes(
                &self.file_download_url(message_id, file_key),
                LARK_MAX_FILE_DOWNLOAD_BYTES,
                "Lark file download",
            )
            .await
        {
            Ok(result) => result,
            Err(err) => {
                log::warn!("Lark failed to download file {file_key}: {err}");
                return None;
            }
        };

        let mut resource = resource_from_bytes(file_name.to_string(), bytes, "Lark file");
        self.workspace
            .store_resource_lossy(&mut resource, Some(message_id), "Lark file")
            .await;
        Some(resource)
    }

    async fn download_audio_resource(&self, message_id: &str, file_key: &str) -> Option<Resource> {
        let bytes = match self
            .get_authorized_bytes(
                &self.file_download_url(message_id, file_key),
                LARK_MAX_AUDIO_BYTES,
                "Lark audio download",
            )
            .await
        {
            Ok(result) => result,
            Err(err) => {
                log::warn!("Lark failed to download audio {file_key}: {err}");
                return None;
            }
        };

        let mut resource = resource_from_bytes(file_key.to_string(), bytes, "Lark audio");
        self.workspace
            .store_resource_lossy(&mut resource, Some(message_id), "Lark audio")
            .await;
        Some(resource)
    }

    async fn get_authorized_bytes(
        &self,
        url: &str,
        max_bytes: u64,
        context: &str,
    ) -> Result<Vec<u8>, BoxError> {
        let mut token = self.get_tenant_access_token().await?;
        let mut retried = false;

        loop {
            let response = self
                .client
                .get(url)
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await?;
            let status = response.status();

            if status.is_success() {
                if let Some(content_length) = response.content_length()
                    && content_length > max_bytes
                {
                    return Err(
                        format!("{context} exceeds {max_bytes} bytes: {content_length}").into(),
                    );
                }

                let bytes = response.bytes().await?.to_vec();
                if bytes.len() as u64 > max_bytes {
                    return Err(
                        format!("{context} exceeds {max_bytes} bytes: {}", bytes.len()).into(),
                    );
                }
                return Ok(bytes);
            }

            let body = response.text().await.unwrap_or_default();
            let parsed = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
            if should_refresh_lark_tenant_token(status, &parsed) && !retried {
                self.invalidate_token().await;
                token = self.get_tenant_access_token().await?;
                retried = true;
                continue;
            }

            return Err(format!("{context} failed ({status}): {body}").into());
        }
    }

    fn spawn_ack_reaction(
        &self,
        message_id: String,
        payload: Option<Value>,
        fallback_text: String,
    ) {
        let channel = self.clone();
        tokio::spawn(async move {
            let emoji = random_lark_ack_reaction(payload.as_ref(), &fallback_text).to_string();
            channel.try_add_ack_reaction(&message_id, &emoji).await;
        });
    }
}

#[async_trait]
impl Channel for LarkChannel {
    fn name(&self) -> &str {
        self.channel_name()
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn id(&self) -> String {
        format!("{}:{}", self.channel_name(), self.id)
    }

    fn set_workspace(&self, workspace: PathBuf) {
        self.workspace.set_path(workspace);
    }

    async fn send(&self, message: &SendMessage) -> Result<(), BoxError> {
        let content = outgoing_markdown_with_resources(&message.content, &message.attachments);
        let content = if content.trim().is_empty() {
            " ".to_string()
        } else {
            content
        };
        let chunks = split_markdown_chunks(&content, LARK_CARD_MARKDOWN_MAX_BYTES);
        let url = self.send_message_url();
        let mut token = self.get_tenant_access_token().await?;

        for chunk in chunks {
            let body = build_interactive_card_body(&message.recipient, chunk);
            let (status, response) = self.send_text_once(&url, &token, &body).await?;

            if should_refresh_lark_tenant_token(status, &response) {
                self.invalidate_token().await;
                token = self.get_tenant_access_token().await?;
                let (retry_status, retry_response) =
                    self.send_text_once(&url, &token, &body).await?;
                ensure_lark_send_success(retry_status, &retry_response, "after token refresh")?;
            } else {
                ensure_lark_send_success(status, &response, "without token refresh")?;
            }
        }

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
    }

    async fn listen(
        &self,
        cancel_token: CancellationToken,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Result<(), BoxError> {
        match self.receive_mode {
            config::LarkReceiveMode::Websocket => self.listen_ws(cancel_token, tx).await,
            config::LarkReceiveMode::Webhook => self.listen_http(cancel_token, tx).await,
        }
    }

    async fn health_check(&self) -> bool {
        matches!(
            tokio::time::timeout(Duration::from_secs(5), self.get_tenant_access_token()).await,
            Ok(Ok(_))
        )
    }

    async fn start_typing(&self, _recipient: &str) -> Result<(), BoxError> {
        Ok(())
    }

    async fn add_reaction(
        &self,
        _channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<(), BoxError> {
        let token = self.get_tenant_access_token().await?;
        let (status, body) = self
            .post_message_reaction_with_token(message_id, &token, emoji)
            .await?;
        ensure_lark_send_success(status, &body, "add reaction")
    }
}

struct ParsedMessageContent {
    content: String,
    attachments: Vec<Resource>,
    post_mentioned_open_ids: Vec<String>,
}

impl ParsedMessageContent {
    fn text(content: String) -> Self {
        Self {
            content,
            attachments: Vec::new(),
            post_mentioned_open_ids: Vec::new(),
        }
    }

    fn with_attachment(content: String, resource: Resource) -> Self {
        Self {
            content,
            attachments: vec![resource],
            post_mentioned_open_ids: Vec::new(),
        }
    }
}

struct ParsedPostContent {
    text: String,
    mentioned_open_ids: Vec<String>,
}

async fn send_lark_ws_ping<S, E>(
    write: &mut S,
    seq: &mut u64,
    service_id: i32,
) -> Result<(), BoxError>
where
    S: Sink<WsMsg, Error = E> + Unpin,
    E: std::error::Error + Send + Sync + 'static,
{
    *seq = seq.wrapping_add(1);
    let ping = PbFrame {
        seq_id: *seq,
        log_id: 0,
        service: service_id,
        method: 0,
        headers: vec![PbHeader {
            key: "type".to_string(),
            value: "ping".to_string(),
        }],
        payload: None,
    };
    write
        .send(WsMsg::Binary(ping.encode_to_vec().into()))
        .await?;
    Ok(())
}

fn service_id_from_ws_url(url: &str) -> i32 {
    url.split('?')
        .nth(1)
        .and_then(|query| {
            query
                .split('&')
                .find(|part| part.starts_with("service_id="))
                .and_then(|part| part.split('=').nth(1))
                .and_then(|value| value.parse::<i32>().ok())
        })
        .unwrap_or(0)
}

fn should_refresh_last_recv(message: &WsMsg) -> bool {
    matches!(message, WsMsg::Binary(_) | WsMsg::Ping(_) | WsMsg::Pong(_))
}

type FragCache = HashMap<String, (Vec<Option<Vec<u8>>>, Instant)>;

fn reassemble_lark_ws_payload(frame: PbFrame, frag_cache: &mut FragCache) -> Option<Vec<u8>> {
    if frame.header_value("type") != "event" {
        return None;
    }

    let msg_id = frame.header_value("message_id").to_string();
    let sum = frame
        .header_value("sum")
        .parse::<usize>()
        .unwrap_or(1)
        .max(1);
    let seq_num = frame.header_value("seq").parse::<usize>().unwrap_or(0);

    if sum == 1 || msg_id.is_empty() || seq_num >= sum {
        return Some(frame.payload.unwrap_or_default());
    }

    let entry = frag_cache
        .entry(msg_id.clone())
        .or_insert_with(|| (vec![None; sum], Instant::now()));
    if entry.0.len() != sum {
        *entry = (vec![None; sum], Instant::now());
    }
    entry.0[seq_num] = frame.payload;

    if !entry.0.iter().all(Option::is_some) {
        return None;
    }

    let payload = entry
        .0
        .iter()
        .flat_map(|slot| slot.as_deref().unwrap_or(&[]))
        .copied()
        .collect::<Vec<_>>();
    frag_cache.remove(&msg_id);
    Some(payload)
}

fn build_card_content(markdown: &str) -> String {
    serde_json::json!({
        "schema": "2.0",
        "body": {
            "elements": [{
                "tag": "markdown",
                "content": markdown,
            }]
        }
    })
    .to_string()
}

fn build_interactive_card_body(recipient: &str, markdown: &str) -> Value {
    serde_json::json!({
        "receive_id": recipient,
        "msg_type": "interactive",
        "content": build_card_content(markdown),
    })
}

fn split_markdown_chunks(text: &str, max_bytes: usize) -> Vec<&str> {
    if text.len() <= max_bytes {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        if start + max_bytes >= text.len() {
            chunks.push(&text[start..]);
            break;
        }

        let mut end = start + max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let search_region = &text[start..end];
        let split_at = search_region
            .rfind('\n')
            .map(|position| start + position + 1)
            .unwrap_or(end);
        let split_at = if text.is_char_boundary(split_at) {
            split_at
        } else {
            (start..split_at)
                .rev()
                .find(|index| text.is_char_boundary(*index))
                .unwrap_or(start)
        };

        if split_at <= start {
            let forced = (end..=text.len())
                .find(|index| text.is_char_boundary(*index))
                .unwrap_or(text.len());
            chunks.push(&text[start..forced]);
            start = forced;
        } else {
            chunks.push(&text[start..split_at]);
            start = split_at;
        }
    }

    chunks
}

fn extract_lark_response_code(body: &Value) -> Option<i64> {
    body.get("code").and_then(Value::as_i64)
}

fn should_refresh_lark_tenant_token(status: reqwest::StatusCode, body: &Value) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED
        || extract_lark_response_code(body) == Some(LARK_INVALID_ACCESS_TOKEN_CODE)
}

fn extract_lark_token_ttl_seconds(body: &Value) -> u64 {
    body.get("expire")
        .or_else(|| body.get("expires_in"))
        .and_then(Value::as_u64)
        .or_else(|| {
            body.get("expire")
                .or_else(|| body.get("expires_in"))
                .and_then(Value::as_i64)
                .and_then(|value| u64::try_from(value).ok())
        })
        .unwrap_or(LARK_DEFAULT_TOKEN_TTL.as_secs())
        .max(1)
}

fn next_token_refresh_deadline(now: Instant, ttl_seconds: u64) -> Instant {
    let ttl = Duration::from_secs(ttl_seconds.max(1));
    now + ttl
        .checked_sub(LARK_TOKEN_REFRESH_SKEW)
        .unwrap_or(Duration::from_secs(1))
}

fn ensure_lark_send_success(
    status: reqwest::StatusCode,
    body: &Value,
    context: &str,
) -> Result<(), BoxError> {
    if !status.is_success() {
        return Err(format!("Lark send failed {context}: status={status}, body={body}").into());
    }

    let code = extract_lark_response_code(body).unwrap_or(0);
    if code != 0 {
        return Err(format!("Lark send failed {context}: code={code}, body={body}").into());
    }

    Ok(())
}

fn outgoing_markdown_with_resources(content: &str, resources: &[Resource]) -> String {
    if resources.is_empty() {
        return content.to_string();
    }

    let mut lines = Vec::new();
    if !content.trim().is_empty() {
        lines.push(content.trim().to_string());
    }
    lines.push("Attachments:".to_string());
    for resource in resources {
        let name = file_name_for_resource(resource);
        if let Some(uri) = resource.uri.as_deref().filter(|uri| is_http_url(uri)) {
            lines.push(format!("- [{name}]({uri})"));
        } else {
            lines.push(format!("- {name}"));
        }
    }
    lines.join("\n")
}

fn pick_uniform_index(len: usize) -> usize {
    debug_assert!(len > 0);
    let upper = len as u64;
    let reject_threshold = (u64::MAX / upper) * upper;

    loop {
        let value = rand::random::<u64>();
        if value < reject_threshold {
            return (value % upper) as usize;
        }
    }
}

fn random_from_pool(pool: &'static [&'static str]) -> &'static str {
    pool[pick_uniform_index(pool.len())]
}

fn lark_ack_pool(locale: LarkAckLocale) -> &'static [&'static str] {
    match locale {
        LarkAckLocale::ZhCn => LARK_ACK_REACTIONS_ZH_CN,
        LarkAckLocale::ZhTw => LARK_ACK_REACTIONS_ZH_TW,
        LarkAckLocale::En => LARK_ACK_REACTIONS_EN,
        LarkAckLocale::Ja => LARK_ACK_REACTIONS_JA,
    }
}

fn map_locale_tag(tag: &str) -> Option<LarkAckLocale> {
    let normalized = tag.trim().to_ascii_lowercase().replace('-', "_");
    if normalized.starts_with("ja") {
        return Some(LarkAckLocale::Ja);
    }
    if normalized.starts_with("en") {
        return Some(LarkAckLocale::En);
    }
    if normalized.contains("hant")
        || normalized.starts_with("zh_tw")
        || normalized.starts_with("zh_hk")
        || normalized.starts_with("zh_mo")
    {
        return Some(LarkAckLocale::ZhTw);
    }
    if normalized.starts_with("zh") {
        return Some(LarkAckLocale::ZhCn);
    }
    None
}

fn find_locale_hint(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in [
                "locale",
                "language",
                "lang",
                "i18n_locale",
                "user_locale",
                "locale_id",
            ] {
                if let Some(locale) = map.get(key).and_then(Value::as_str) {
                    return Some(locale.to_string());
                }
            }

            map.values().find_map(find_locale_hint)
        }
        Value::Array(items) => items.iter().find_map(find_locale_hint),
        _ => None,
    }
}

fn detect_locale_from_post_content(content: &str) -> Option<LarkAckLocale> {
    let parsed = serde_json::from_str::<Value>(content).ok()?;
    let object = parsed.as_object()?;
    object.keys().find_map(|key| map_locale_tag(key))
}

fn is_japanese_kana(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF)
}

fn is_cjk_han(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF)
}

fn is_traditional_only_han(ch: char) -> bool {
    matches!(
        ch,
        '奮' | '鬥'
            | '強'
            | '體'
            | '國'
            | '臺'
            | '萬'
            | '與'
            | '為'
            | '這'
            | '學'
            | '機'
            | '開'
            | '裡'
    )
}

fn is_simplified_only_han(ch: char) -> bool {
    matches!(
        ch,
        '奋' | '斗'
            | '强'
            | '体'
            | '国'
            | '台'
            | '万'
            | '与'
            | '为'
            | '这'
            | '学'
            | '机'
            | '开'
            | '里'
    )
}

fn detect_locale_from_text(text: &str) -> Option<LarkAckLocale> {
    if text.chars().any(is_japanese_kana) {
        return Some(LarkAckLocale::Ja);
    }
    if text.chars().any(is_traditional_only_han) {
        return Some(LarkAckLocale::ZhTw);
    }
    if text.chars().any(is_simplified_only_han) || text.chars().any(is_cjk_han) {
        return Some(LarkAckLocale::ZhCn);
    }
    None
}

fn detect_lark_ack_locale(payload: Option<&Value>, fallback_text: &str) -> LarkAckLocale {
    if let Some(payload) = payload {
        if let Some(locale) = find_locale_hint(payload).and_then(|hint| map_locale_tag(&hint)) {
            return locale;
        }
        let message_content = payload
            .pointer("/message/content")
            .and_then(Value::as_str)
            .or_else(|| {
                payload
                    .pointer("/event/message/content")
                    .and_then(Value::as_str)
            });
        if let Some(locale) = message_content.and_then(detect_locale_from_post_content) {
            return locale;
        }
    }

    detect_locale_from_text(fallback_text).unwrap_or(LarkAckLocale::En)
}

fn random_lark_ack_reaction(payload: Option<&Value>, fallback_text: &str) -> &'static str {
    random_from_pool(lark_ack_pool(detect_lark_ack_locale(
        payload,
        fallback_text,
    )))
}

fn parse_post_content_details(content: &str) -> Option<ParsedPostContent> {
    let parsed = serde_json::from_str::<Value>(content).ok()?;
    let locale = parsed
        .get("zh_cn")
        .or_else(|| parsed.get("en_us"))
        .or_else(|| {
            parsed
                .as_object()
                .and_then(|object| object.values().find(|value| value.is_object()))
        })?;

    let mut text = String::new();
    let mut mentioned_open_ids = Vec::new();

    if let Some(title) = locale
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
    {
        text.push_str(title);
        text.push_str("\n\n");
    }

    if let Some(paragraphs) = locale.get("content").and_then(Value::as_array) {
        for paragraph in paragraphs {
            if let Some(elements) = paragraph.as_array() {
                for element in elements {
                    match element.get("tag").and_then(Value::as_str).unwrap_or("") {
                        "text" => {
                            if let Some(value) = element.get("text").and_then(Value::as_str) {
                                text.push_str(value);
                            }
                        }
                        "a" => text.push_str(
                            element
                                .get("text")
                                .and_then(Value::as_str)
                                .filter(|value| !value.is_empty())
                                .or_else(|| element.get("href").and_then(Value::as_str))
                                .unwrap_or(""),
                        ),
                        "at" => {
                            let name = element
                                .get("user_name")
                                .and_then(Value::as_str)
                                .or_else(|| element.get("user_id").and_then(Value::as_str))
                                .unwrap_or("user");
                            text.push('@');
                            text.push_str(name);
                            if let Some(open_id) = element
                                .get("user_id")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                            {
                                mentioned_open_ids.push(open_id.to_string());
                            }
                        }
                        _ => {
                            if let Some(value) = element.get("text").and_then(Value::as_str) {
                                text.push_str(value);
                            }
                        }
                    }
                }
                text.push('\n');
            }
        }
    }

    let text = text.trim().to_string();
    (!text.is_empty()).then_some(ParsedPostContent {
        text,
        mentioned_open_ids,
    })
}

fn parse_list_content(content: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(content).ok()?;
    let items = parsed
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| parsed.get("content").and_then(Value::as_array))?;
    let mut lines = Vec::new();
    collect_list_items(items, &mut lines, 0);
    let text = lines.join("\n").trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn collect_list_items(items: &[Value], lines: &mut Vec<String>, depth: usize) {
    let indent = "  ".repeat(depth);
    for item in items {
        let (inline_elements, children) = if let Some(array) = item.as_array() {
            (array.as_slice(), None)
        } else if let Some(object) = item.as_object() {
            let inline_elements = object
                .get("content")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let children = object.get("children").and_then(Value::as_array);
            (inline_elements, children)
        } else {
            continue;
        };

        let mut text = String::new();
        for element in inline_elements {
            if let Some(inner) = element.as_array() {
                for inner_element in inner {
                    extract_inline_text(inner_element, &mut text);
                }
            } else {
                extract_inline_text(element, &mut text);
            }
        }

        let text = text.trim();
        if !text.is_empty() {
            lines.push(format!("{indent}- {text}"));
        }

        if let Some(children) = children {
            collect_list_items(children, lines, depth + 1);
        }
    }
}

fn extract_inline_text(element: &Value, out: &mut String) {
    match element.get("tag").and_then(Value::as_str).unwrap_or("") {
        "text" => {
            if let Some(value) = element.get("text").and_then(Value::as_str) {
                out.push_str(value);
            }
        }
        "a" => out.push_str(
            element
                .get("text")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .or_else(|| element.get("href").and_then(Value::as_str))
                .unwrap_or(""),
        ),
        "at" => {
            let name = element
                .get("user_name")
                .and_then(Value::as_str)
                .or_else(|| element.get("user_id").and_then(Value::as_str))
                .unwrap_or("user");
            out.push('@');
            out.push_str(name);
        }
        _ => {}
    }
}

fn strip_at_placeholders(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch == '@' {
            let rest: String = chars.clone().map(|(_, candidate)| candidate).collect();
            if let Some(after) = rest.strip_prefix("_user_") {
                let skip = "_user_".len()
                    + after
                        .chars()
                        .take_while(|candidate| candidate.is_ascii_digit())
                        .count();
                for _ in 0..skip {
                    chars.next();
                }
                if chars.peek().is_some_and(|(_, candidate)| *candidate == ' ') {
                    chars.next();
                }
                continue;
            }
        }
        result.push(ch);
    }
    result
}

fn mention_matches_bot_open_id(mention: &Value, bot_open_id: &str) -> bool {
    mention
        .pointer("/id/open_id")
        .or_else(|| mention.pointer("/open_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| value == bot_open_id)
}

fn should_respond_in_group(
    mention_only: bool,
    bot_open_id: Option<&str>,
    mentions: &[Value],
    post_mentioned_open_ids: &[String],
) -> bool {
    if !mention_only {
        return true;
    }
    let Some(bot_open_id) = bot_open_id.filter(|value| !value.is_empty()) else {
        return false;
    };
    mentions
        .iter()
        .any(|mention| mention_matches_bot_open_id(mention, bot_open_id))
        || post_mentioned_open_ids
            .iter()
            .any(|open_id| open_id == bot_open_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> config::LarkChannelSettings {
        config::LarkChannelSettings {
            id: Some("test".to_string()),
            app_id: "cli_test_app_id".to_string(),
            app_secret: "test_app_secret".to_string(),
            username: Some("anda-lark".to_string()),
            verification_token: Some("test_verification_token".to_string()),
            port: None,
            allowed_users: vec!["ou_testuser123".to_string()],
            mention_only: true,
            platform: config::LarkPlatform::Lark,
            receive_mode: config::LarkReceiveMode::Websocket,
            ack_reactions: true,
        }
    }

    fn test_channel() -> LarkChannel {
        let channel = LarkChannel::new(&test_config(), Client::new());
        channel.set_resolved_bot_open_id(Some("ou_bot".to_string()));
        channel
    }

    #[test]
    fn lark_channel_identity() {
        let channel = test_channel();
        assert_eq!(channel.name(), "lark");
        assert_eq!(channel.username(), "anda-lark");
        assert_eq!(channel.id(), "lark:test");
    }

    #[test]
    fn lark_user_allowed_exact() {
        let channel = test_channel();
        assert!(channel.is_user_allowed("ou_testuser123"));
        assert!(!channel.is_user_allowed("ou_other"));
    }

    #[test]
    fn lark_group_response_requires_matching_bot_mention() {
        let mentions = vec![serde_json::json!({ "id": { "open_id": "ou_other" } })];
        assert!(!should_respond_in_group(
            true,
            Some("ou_bot"),
            &mentions,
            &[]
        ));

        let mentions = vec![serde_json::json!({ "id": { "open_id": "ou_bot" } })];
        assert!(should_respond_in_group(
            true,
            Some("ou_bot"),
            &mentions,
            &[]
        ));
    }

    #[test]
    fn lark_should_refresh_token_on_http_401_or_body_code() {
        assert!(should_refresh_lark_tenant_token(
            reqwest::StatusCode::UNAUTHORIZED,
            &serde_json::json!({ "code": 0 })
        ));
        assert!(should_refresh_lark_tenant_token(
            reqwest::StatusCode::OK,
            &serde_json::json!({ "code": LARK_INVALID_ACCESS_TOKEN_CODE })
        ));
    }

    #[test]
    fn lark_extract_token_ttl_seconds_supports_expire_and_expires_in() {
        assert_eq!(
            extract_lark_token_ttl_seconds(&serde_json::json!({ "expire": 7200 })),
            7200
        );
        assert_eq!(
            extract_lark_token_ttl_seconds(&serde_json::json!({ "expires_in": 3600 })),
            3600
        );
    }

    #[test]
    fn lark_next_token_refresh_deadline_reserves_refresh_skew() {
        let now = Instant::now();
        assert_eq!(
            next_token_refresh_deadline(now, 7200).duration_since(now),
            Duration::from_secs(7080)
        );
        assert_eq!(
            next_token_refresh_deadline(now, 60).duration_since(now),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn split_markdown_chunks_keeps_chunks_under_limit() {
        let text = "a".repeat(LARK_CARD_MARKDOWN_MAX_BYTES + 10);
        let chunks = split_markdown_chunks(&text, LARK_CARD_MARKDOWN_MAX_BYTES);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= LARK_CARD_MARKDOWN_MAX_BYTES);
        assert!(chunks[1].len() <= LARK_CARD_MARKDOWN_MAX_BYTES);
    }

    #[test]
    fn parse_list_content_flat_items() {
        let content =
            r#"{"items":[[{"tag":"text","text":"first"}],[{"tag":"text","text":"second"}]]}"#;
        assert_eq!(parse_list_content(content).unwrap(), "- first\n- second");
    }

    #[test]
    fn strip_at_placeholders_removes_lark_user_tokens() {
        assert_eq!(strip_at_placeholders("@_user_1 hello"), "hello");
        assert_eq!(strip_at_placeholders("hi @_user_12 there"), "hi there");
    }

    #[tokio::test]
    async fn lark_parse_valid_text_event() {
        let channel = test_channel();
        let payload = serde_json::json!({
            "sender": {
                "sender_id": { "open_id": "ou_testuser123" }
            },
            "message": {
                "message_id": "om_1",
                "message_type": "text",
                "content": "{\"text\":\"Hello Anda!\"}",
                "chat_id": "oc_chat123",
                "chat_type": "p2p",
                "create_time": "1699999999000"
            }
        });

        let message = channel.parse_event_object(&payload).await.unwrap();
        assert_eq!(message.sender, "ou_testuser123");
        assert_eq!(message.reply_target, "oc_chat123");
        assert_eq!(message.channel, "lark:test");
        assert_eq!(message.content, "Hello Anda!");
        assert_eq!(message.timestamp, 1_699_999_999_000);
    }

    #[tokio::test]
    async fn lark_parse_unauthorized_user() {
        let channel = test_channel();
        let payload = serde_json::json!({
            "sender": {
                "sender_id": { "open_id": "ou_unauthorized" }
            },
            "message": {
                "message_id": "om_2",
                "message_type": "text",
                "content": "{\"text\":\"spam\"}",
                "chat_id": "oc_chat123",
                "chat_type": "p2p"
            }
        });

        assert!(channel.parse_event_object(&payload).await.is_none());
    }
}
