use anda_core::{AgentInput, BoxError, Json, Principal, ToolInput};
use anda_engine::unix_ms;
use anda_engine_server::handler::AppState;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        HeaderMap, HeaderValue, Request, StatusCode, Uri,
        header::{AUTHORIZATION, CONNECTION, UPGRADE},
    },
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use hyper::upgrade;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::derive_accept_key, protocol::Role},
};

use super::browser::{BrowserActionResult, BrowserBridge, BrowserCommand};
use crate::{transcription::TranscriptionManager, tts::TtsManager};

const SEC_WEBSOCKET_ACCEPT: &str = "sec-websocket-accept";
const SEC_WEBSOCKET_KEY: &str = "sec-websocket-key";
const SEC_WEBSOCKET_VERSION: &str = "sec-websocket-version";

#[derive(Clone)]
pub struct BrowserWebSocketState {
    pub app: AppState,
    pub bridge: Arc<BrowserBridge>,
    pub voice_capabilities: BrowserVoiceCapabilities,
}

#[derive(Clone, Debug, Default)]
pub struct BrowserVoiceCapabilities {
    pub transcription: Vec<String>,
    pub tts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserWsIncoming {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    session: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserRegisterArgs {
    session: String,
    #[serde(default)]
    tab_id: Option<i64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Serialize)]
struct BrowserWsRequest<'a> {
    id: u64,
    method: &'a str,
    params: &'a BrowserCommand,
}

struct BrowserWsConnection {
    id: u64,
    sender: mpsc::Sender<BrowserCommand>,
}

pub async fn browser_websocket(
    State(state): State<BrowserWebSocketState>,
    Path(id): Path<String>,
    mut request: Request<Body>,
) -> Response {
    let engine_id = match resolve_engine_id(&state.app, &id) {
        Ok(id) => id,
        Err((status, message)) => return (status, message).into_response(),
    };

    let auth_headers = websocket_auth_headers(request.headers(), request.uri());
    let caller = state
        .app
        .verify_user(&auth_headers, unix_ms(), Some(engine_id), None);
    if caller == Principal::anonymous() {
        return (StatusCode::UNAUTHORIZED, "invalid or missing bearer token").into_response();
    }

    let Some(sec_key) = websocket_key(request.headers()) else {
        return (StatusCode::BAD_REQUEST, "missing WebSocket upgrade headers").into_response();
    };

    let upgraded = upgrade::on(&mut request);
    tokio::spawn(async move {
        match upgraded.await {
            Ok(upgraded) => {
                let io = TokioIo::new(upgraded);
                let websocket = WebSocketStream::from_raw_socket(io, Role::Server, None).await;
                if let Err(err) =
                    handle_browser_websocket(websocket, state, caller, engine_id).await
                {
                    log::warn!("Chrome browser WebSocket closed with error: {err}");
                }
            }
            Err(err) => {
                log::warn!("Chrome browser WebSocket upgrade failed: {err}");
            }
        }
    });

    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "Upgrade")
        .header(SEC_WEBSOCKET_ACCEPT, derive_accept_key(sec_key.as_bytes()))
        .body(Body::empty())
        .unwrap_or_else(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to build WebSocket response: {err}"),
            )
                .into_response()
        })
}

async fn handle_browser_websocket(
    websocket: WebSocketStream<TokioIo<upgrade::Upgraded>>,
    state: BrowserWebSocketState,
    caller: Principal,
    engine_id: Principal,
) -> Result<(), BoxError> {
    let (mut socket_writer, mut socket_reader) = websocket.split();
    let (connection_id, action_sender, mut action_receiver) = state.bridge.open_ws_connection();
    let connection = BrowserWsConnection {
        id: connection_id,
        sender: action_sender,
    };
    let (write_sender, mut write_receiver) = mpsc::channel::<String>(64);

    let writer = tokio::spawn(async move {
        while let Some(payload) = write_receiver.recv().await {
            if socket_writer
                .send(Message::Text(payload.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let action_write_sender = write_sender.clone();
    let action_forwarder = tokio::spawn(async move {
        while let Some(command) = action_receiver.recv().await {
            let payload = match serde_json::to_string(&BrowserWsRequest {
                id: command.request_id,
                method: "browser_action",
                params: &command,
            }) {
                Ok(payload) => payload,
                Err(err) => {
                    log::warn!("failed to encode browser action request: {err}");
                    continue;
                }
            };

            if action_write_sender.send(payload).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = socket_reader.next().await {
        match message? {
            Message::Text(text) => {
                handle_browser_ws_text(
                    text.as_ref(),
                    &state,
                    caller,
                    engine_id,
                    &connection,
                    &write_sender,
                )
                .await;
            }
            Message::Binary(data) => {
                if let Ok(text) = std::str::from_utf8(data.as_ref()) {
                    handle_browser_ws_text(
                        text,
                        &state,
                        caller,
                        engine_id,
                        &connection,
                        &write_sender,
                    )
                    .await;
                }
            }
            Message::Close(_frame) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }

    state.bridge.disconnect_ws_connection(connection_id);
    action_forwarder.abort();
    writer.abort();
    Ok(())
}

async fn handle_browser_ws_text(
    text: &str,
    state: &BrowserWebSocketState,
    caller: Principal,
    engine_id: Principal,
    connection: &BrowserWsConnection,
    write_sender: &mpsc::Sender<String>,
) {
    let incoming = match serde_json::from_str::<BrowserWsIncoming>(text) {
        Ok(incoming) => incoming,
        Err(err) => {
            log::warn!("invalid Chrome browser WebSocket message: {err}");
            return;
        }
    };

    if incoming.method.is_some() {
        handle_browser_ws_request(incoming, state, caller, engine_id, connection, write_sender)
            .await;
    } else {
        handle_browser_ws_response(incoming, state).await;
    }
}

async fn handle_browser_ws_request(
    incoming: BrowserWsIncoming,
    state: &BrowserWebSocketState,
    caller: Principal,
    engine_id: Principal,
    connection: &BrowserWsConnection,
    write_sender: &mpsc::Sender<String>,
) {
    let id = incoming.id;
    let result = match incoming.method.as_deref().unwrap_or_default() {
        "ping" => Ok(json!({ "ok": true })),
        "browser_register" => handle_browser_register(incoming.params, state, connection),
        "agent_run" => handle_agent_run(incoming.params, state, caller, engine_id).await,
        "tool_call" => handle_tool_call(incoming.params, state, caller, engine_id).await,
        "information" => handle_information(state, engine_id),
        "capabilities" => handle_capabilities(state, engine_id),
        method => Err(format!("{method} on WebSocket engine RPC not implemented")),
    };

    if let Some(id) = id {
        send_ws_result(write_sender, id, result).await;
    }
}

fn handle_browser_register(
    params: Value,
    state: &BrowserWebSocketState,
    connection: &BrowserWsConnection,
) -> Result<Value, String> {
    let (args,): (BrowserRegisterArgs,) = params_from_value(params)?;
    let session = state
        .bridge
        .register_ws_session(
            connection.id,
            connection.sender.clone(),
            args.session,
            args.tab_id,
            args.url,
            args.title,
        )
        .map_err(|err| err.to_string())?;
    Ok(json!({ "registered": true, "session": session }))
}

async fn handle_agent_run(
    params: Value,
    state: &BrowserWebSocketState,
    caller: Principal,
    engine_id: Principal,
) -> Result<Value, String> {
    let (input,): (AgentInput,) = params_from_value(params)?;
    let engine = state
        .app
        .engines
        .get(&engine_id)
        .ok_or_else(|| format!("engine {} not found", engine_id.to_text()))?;
    let output = engine
        .agent_run(caller, input)
        .await
        .map_err(|err| format!("failed to run agent: {err:?}"))?;
    serde_json::to_value(output).map_err(|err| err.to_string())
}

async fn handle_tool_call(
    params: Value,
    state: &BrowserWebSocketState,
    caller: Principal,
    engine_id: Principal,
) -> Result<Value, String> {
    let (input,): (ToolInput<Json>,) = params_from_value(params)?;
    let engine = state
        .app
        .engines
        .get(&engine_id)
        .ok_or_else(|| format!("engine {} not found", engine_id.to_text()))?;
    let output = engine
        .tool_call(caller, input)
        .await
        .map_err(|err| format!("failed to call tool: {err:?}"))?;
    serde_json::to_value(output).map_err(|err| err.to_string())
}

fn handle_information(
    state: &BrowserWebSocketState,
    engine_id: Principal,
) -> Result<Value, String> {
    let engine = state
        .app
        .engines
        .get(&engine_id)
        .ok_or_else(|| format!("engine {} not found", engine_id.to_text()))?;
    serde_json::to_value(engine.information()).map_err(|err| err.to_string())
}

fn handle_capabilities(
    state: &BrowserWebSocketState,
    engine_id: Principal,
) -> Result<Value, String> {
    let engine = state
        .app
        .engines
        .get(&engine_id)
        .ok_or_else(|| format!("engine {} not found", engine_id.to_text()))?;
    let names = vec![
        TranscriptionManager::NAME.to_string(),
        TtsManager::NAME.to_string(),
    ];
    let tools = engine.tools(Some(&names));
    let has_tool = |name: &str| {
        tools
            .iter()
            .any(|tool| tool.definition.name.as_str() == name)
    };

    Ok(json!({
        "transcription": if has_tool(TranscriptionManager::NAME) {
            state.voice_capabilities.transcription.clone()
        } else {
            Vec::<String>::new()
        },
        "tts": if has_tool(TtsManager::NAME) {
            state.voice_capabilities.tts.clone()
        } else {
            Vec::<String>::new()
        },
    }))
}

async fn handle_browser_ws_response(incoming: BrowserWsIncoming, state: &BrowserWebSocketState) {
    let Some(id) = incoming.id else {
        return;
    };
    let Some(session) = incoming.session else {
        log::warn!("Chrome browser response {id} is missing session");
        return;
    };

    let result = if let Some(error) = incoming.error {
        BrowserActionResult {
            ok: false,
            value: Value::Null,
            error: Some(error),
        }
    } else {
        match serde_json::from_value::<BrowserActionResult>(incoming.result.unwrap_or(Value::Null))
        {
            Ok(result) => result,
            Err(err) => BrowserActionResult {
                ok: false,
                value: Value::Null,
                error: Some(format!("invalid browser action response: {err}")),
            },
        }
    };

    if let Err(err) = state.bridge.complete(session, id, result).await {
        log::warn!("failed to complete Chrome browser action {id}: {err}");
    }
}

async fn send_ws_result(
    write_sender: &mpsc::Sender<String>,
    id: u64,
    result: Result<Value, String>,
) {
    let payload = match result {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(error) => json!({ "id": id, "error": error }),
    };
    let _ = write_sender.send(payload.to_string()).await;
}

fn params_from_value<T>(value: Value) -> Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|err| format!("failed to decode params: {err}"))
}

fn resolve_engine_id(app: &AppState, id: &str) -> Result<Principal, (StatusCode, String)> {
    let id = if id == "default" {
        app.default_engine
    } else {
        Principal::from_text(id).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("invalid engine id: {id:?}"),
            )
        })?
    };

    if app.engines.contains_key(&id) {
        Ok(id)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("engine {} not found", id.to_text()),
        ))
    }
}

fn websocket_auth_headers(headers: &HeaderMap, uri: &Uri) -> HeaderMap {
    let mut headers = headers.clone();
    if headers.get(AUTHORIZATION).is_none()
        && let Some(token) = query_param(uri, "token")
        && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(AUTHORIZATION, value);
    }
    headers
}

fn websocket_key(headers: &HeaderMap) -> Option<String> {
    let upgrade = header_contains(headers, UPGRADE.as_str(), "websocket");
    let connection = header_contains(headers, CONNECTION.as_str(), "upgrade");
    let version = headers
        .get(SEC_WEBSOCKET_VERSION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|version| version == "13");
    let key = headers
        .get(SEC_WEBSOCKET_KEY)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    if upgrade && connection && version {
        key
    } else {
        None
    }
}

fn header_contains(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(expected))
        })
}

fn query_param(uri: &Uri, name: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            output.push(byte);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}
