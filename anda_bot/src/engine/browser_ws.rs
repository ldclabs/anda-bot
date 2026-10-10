use anda_core::{AgentInput, BoxError, Json, Principal, ToolInput};
use anda_engine::{engine::Engine, memory::KipArgs, unix_ms};
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
use futures::{SinkExt, StreamExt};
use hyper::upgrade;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::derive_accept_key, protocol::Role},
};
use tokio_util::sync::CancellationToken;

use super::{
    RuntimeModels,
    app_protocol::{AppCapabilities, AppInitialize, AppSubmit, StateChanged, SubmissionRead},
    browser::{BrowserActionResult, BrowserBridge, BrowserCommand, BrowserRegisterArgs},
    shell_runtime::CliWorkspaceGrants,
    workspace_picker,
};
use crate::brain;
use crate::util::locale;
use crate::{transcription::TranscriptionManager, tts::TtsManager};

const SEC_WEBSOCKET_ACCEPT: &str = "sec-websocket-accept";
const SEC_WEBSOCKET_KEY: &str = "sec-websocket-key";
const SEC_WEBSOCKET_VERSION: &str = "sec-websocket-version";
/// How often an idle socket rechecks its credential against the wall clock.
const EXPIRY_CHECK_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct BrowserWebSocketState {
    pub(super) admission: Arc<crate::runtime_admission::Admission>,
    pub(super) events: Arc<super::app_protocol::AppEvents>,
    pub(super) submissions: Arc<super::app_protocol::Submissions>,
    pub(super) app_protocol: bool,
    pub(super) memory: super::memory_api::MemoryApiState,
    pub(super) mcp: super::mcp::McpApiState,
    pub(super) auth_headers: HeaderMap,
    /// When the connection's bearer expires. The signature is verified once at
    /// the upgrade; a live socket only rechecks the clock.
    pub(super) credential_expires_at_ms: u64,
    pub app: AppState,
    pub brain: brain::Client,
    pub bridge: Arc<BrowserBridge>,
    pub voice_capabilities: BrowserVoiceCapabilities,
    pub home_dir: PathBuf,
    pub(crate) runtime_models: RuntimeModels,
    pub(super) cli_workspaces: CliWorkspaceGrants,
}

#[derive(Clone, Debug, Default)]
pub struct BrowserVoiceCapabilities {
    pub transcription: Vec<String>,
    pub tts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserWsIncoming {
    #[serde(default)]
    jsonrpc: Option<String>,
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

#[derive(Serialize)]
struct BrowserWsRequest<'a> {
    id: u64,
    method: &'a str,
    params: &'a BrowserCommand,
}

/// A reply frame. It borrows the result, which is serialized once instead of
/// being copied into another `Value` first.
#[derive(Serialize)]
struct BrowserWsResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    jsonrpc: Option<&'static str>,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<BrowserWsError<'a>>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum BrowserWsError<'a> {
    /// The extension transport carries the message alone.
    Message(&'a str),
    /// The desktop transport's JSON-RPC 2.0 error object.
    Rpc { code: i32, message: &'a str },
}

/// One authenticated socket, shared by its request tasks.
struct WsConnection {
    state: BrowserWebSocketState,
    caller: Principal,
    engine: Arc<Engine>,
    /// The bridge connection that browser registrations and replies belong to.
    id: u64,
    actions: mpsc::Sender<BrowserCommand>,
    writer: mpsc::Sender<String>,
}

impl WsConnection {
    fn is_owner(&self) -> bool {
        self.caller == self.state.cli_workspaces.owner()
    }

    async fn reply(&self, id: u64, result: Result<Value, String>) {
        let app_protocol = self.state.app_protocol;
        let (result, error) = match &result {
            Ok(value) => (Some(value), None),
            Err(message) if app_protocol => (
                None,
                Some(BrowserWsError::Rpc {
                    code: -32000,
                    message,
                }),
            ),
            Err(message) => (None, Some(BrowserWsError::Message(message))),
        };
        let response = BrowserWsResponse {
            jsonrpc: app_protocol.then_some("2.0"),
            id,
            result,
            error,
        };
        match serde_json::to_string(&response) {
            Ok(payload) => {
                let _ = self.writer.send(payload).await;
            }
            Err(err) => log::warn!("failed to encode WebSocket response {id}: {err}"),
        }
    }
}

pub async fn app_websocket(
    State(mut state): State<BrowserWebSocketState>,
    request: Request<Body>,
) -> Response {
    // Desktop Main is the only client of this local privileged endpoint.
    // Browser/extension clients retain their existing transport.
    if request.headers().contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            "Desktop transport does not accept browser origins",
        )
            .into_response();
    }
    state.app_protocol = true;
    browser_websocket(State(state), Path("default".into()), request).await
}

pub async fn browser_websocket(
    State(mut state): State<BrowserWebSocketState>,
    Path(id): Path<String>,
    mut request: Request<Body>,
) -> Response {
    let engine = match resolve_engine(&state.app, &id) {
        Ok(engine) => engine,
        Err((status, message)) => return (status, message).into_response(),
    };

    // Only the browser extension, which cannot set WebSocket headers, may pass
    // its bearer in the query string.
    let auth_headers = if state.app_protocol {
        request.headers().clone()
    } else {
        websocket_auth_headers(request.headers(), request.uri())
    };
    let caller = match super::verify_trusted_user(&state.app, &auth_headers, unix_ms()) {
        Ok(caller) => caller,
        Err(error) => return error.into_response(),
    };
    if state.app_protocol && caller != state.cli_workspaces.owner() {
        return (
            StatusCode::FORBIDDEN,
            "Application transport requires the local owner",
        )
            .into_response();
    }

    let Some(sec_key) = websocket_key(request.headers()) else {
        return (StatusCode::BAD_REQUEST, "missing WebSocket upgrade headers").into_response();
    };

    // Brain must revalidate the original caller's bearer, including expiry and
    // native mapping. Never substitute the daemon-wide Brain credential.
    let Some(bearer) = auth_headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return (StatusCode::UNAUTHORIZED, "missing bearer token").into_response();
    };
    let Some(credential_expires_at_ms) = bearer_expires_at_ms(bearer) else {
        return (StatusCode::UNAUTHORIZED, "invalid or expired credential").into_response();
    };
    state.brain = state.brain.with_auth_token(bearer.to_string());
    state.auth_headers = auth_headers;
    state.credential_expires_at_ms = credential_expires_at_ms;

    let upgraded = upgrade::on(&mut request);
    tokio::spawn(async move {
        match upgraded.await {
            Ok(upgraded) => {
                let io = TokioIo::new(upgraded);
                let websocket = WebSocketStream::from_raw_socket(io, Role::Server, None).await;
                handle_browser_websocket(websocket, state, caller, engine).await;
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
    engine: Arc<Engine>,
) {
    let (mut socket_writer, mut socket_reader) = websocket.split();
    let (connection_id, actions, mut action_receiver) = state.bridge.open_ws_connection();
    let (writer, mut write_receiver) = mpsc::channel::<String>(64);
    // Cancels in-flight request tasks (agent runs, tool calls, ...) when the
    // connection goes away, so orphans do not keep running side effects that a
    // reconnecting client will retry.
    let request_tasks = CancellationToken::new();
    let expires_at_ms = state.credential_expires_at_ms;
    let app_protocol = state.app_protocol;
    let events = state.events.clone();
    let connection = Arc::new(WsConnection {
        state,
        caller,
        engine,
        id: connection_id,
        actions,
        writer: writer.clone(),
    });

    let write_task = {
        let cancel = request_tasks.clone();
        tokio::spawn(async move {
            while let Some(payload) = write_receiver.recv().await {
                if socket_writer
                    .send(Message::Text(payload.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            cancel.cancel();
        })
    };

    // Register before processing requests. Snapshot reads after initialize are
    // covered by this subscription; changes during a read cause another read.
    // watch retains only the latest invalidation, bounding slow-client memory.
    // The extension uses these to refresh its channel list.
    let event_task = {
        let mut changes = events.subscribe(&caller.to_string());
        let writer = writer.clone();
        let cancel = request_tasks.clone();
        tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                let revision = changes.borrow_and_update().to_string();
                let message = json!({"jsonrpc":"2.0", "method":"state/changed", "params": StateChanged { instance_id: events.instance.clone(), revision }}).to_string();
                // Desktop reconnects for a fresh snapshot rather than
                // silently lose the final invalidation. A browser only
                // refreshes its channel list, so it waits for queue room
                // instead of dropping its session and in-flight requests.
                let sent = if app_protocol {
                    writer.try_send(message).is_ok()
                } else {
                    writer.send(message).await.is_ok()
                };
                if !sent {
                    cancel.cancel();
                    break;
                }
            }
        })
    };

    let action_task = {
        let cancel = request_tasks.clone();
        tokio::spawn(async move {
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
                if writer.send(payload).await.is_err() {
                    break;
                }
            }
            cancel.cancel();
        })
    };

    // A live socket does not extend the credential's lifetime. The wall clock
    // is checked on every frame and on a timer, so a machine waking from sleep
    // closes an expired socket promptly. Closing also revokes its browser
    // registration and outstanding actions.
    let mut expiry_check = tokio::time::interval(EXPIRY_CHECK_INTERVAL);
    loop {
        let frame = tokio::select! {
            _ = request_tasks.cancelled() => break,
            _ = expiry_check.tick() => None,
            frame = socket_reader.next() => Some(frame),
        };
        if unix_ms() >= expires_at_ms {
            break;
        }
        let message = match frame {
            None => continue,
            Some(Some(Ok(message))) => message,
            Some(None) => break,
            Some(Some(Err(err))) => {
                // Read errors are common in practice (extension service worker
                // killed, network drop); the cleanup below still runs.
                log::warn!("Chrome browser WebSocket read error: {err}");
                break;
            }
        };
        let text = match &message {
            Message::Text(text) => text.as_str(),
            Message::Binary(data) => match std::str::from_utf8(data) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        };
        handle_browser_ws_text(&connection, text, &request_tasks);
    }

    connection
        .state
        .bridge
        .disconnect_ws_connection(connection_id);
    request_tasks.cancel();
    action_task.abort();
    event_task.abort();
    write_task.abort();
}

fn handle_browser_ws_text(
    connection: &Arc<WsConnection>,
    text: &str,
    request_tasks: &CancellationToken,
) {
    let incoming = match serde_json::from_str::<BrowserWsIncoming>(text) {
        Ok(incoming) => incoming,
        Err(err) => {
            log::warn!("invalid Chrome browser WebSocket message: {err}");
            return;
        }
    };

    match incoming.method.as_deref() {
        None => handle_browser_ws_response(connection, incoming),
        // The extension's keep-alive ping expects no reply.
        Some("ping") if incoming.id.is_none() => {}
        Some(_) => {
            // Handle requests on their own task: agent runs, folder pickers,
            // and auto-update checks can take seconds to minutes, and the read
            // loop must keep draining pings and browser-action responses
            // meanwhile. The task dies with the connection: its results would
            // go to a dead sender anyway, and a reconnecting client retries.
            let connection = connection.clone();
            let cancel = request_tasks.child_token();
            tokio::spawn(async move {
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = crate::util::boxed(handle_browser_ws_request(incoming, &connection)) => {}
                }
            });
        }
    }
}

async fn handle_browser_ws_request(incoming: BrowserWsIncoming, connection: &Arc<WsConnection>) {
    let BrowserWsIncoming {
        jsonrpc,
        id,
        method,
        params,
        ..
    } = incoming;
    let result = dispatch_browser_ws_request(
        connection,
        jsonrpc.as_deref(),
        method.as_deref().unwrap_or_default(),
        params,
    )
    .await;
    if let Some(id) = id {
        connection.reply(id, result).await;
    }
}

async fn dispatch_browser_ws_request(
    connection: &Arc<WsConnection>,
    jsonrpc: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let state = &connection.state;
    // Requests run in separate tasks and may start after the frame was read.
    if unix_ms() >= state.credential_expires_at_ms {
        return Err("invalid or expired credential".into());
    }
    if state.runtime_models.uses_chatgpt() && !connection.is_owner() {
        return Err(
            "ChatGPT plan providers are owner-only; use API-key providers for shared users".into(),
        );
    }
    if state.app_protocol && jsonrpc != Some("2.0") {
        return Err("jsonrpc must be 2.0".into());
    }
    // Daemon lifecycle and machine-wide settings belong to the local owner.
    if (matches!(
        method,
        "pick_workspace" | "register_workspace" | "reload_models" | "set_model"
    ) || method.starts_with("mcp_"))
        && !connection.is_owner()
    {
        return Err("Only the local owner may control the daemon".into());
    }
    let _permit = if method.starts_with("memory_")
        || method.starts_with("brain_")
        || super::mcp::is_write_method(method)
        || matches!(method, "set_model" | "reload_models" | "register_workspace")
    {
        Some(state.admission.enter().map_err(str::to_string)?)
    } else {
        None
    };

    // Each method is its own subsystem; see `crate::util::boxed`.
    use crate::util::boxed;
    match method {
        "initialize" | "chat/subscribe" if state.app_protocol => Ok(json!(AppInitialize {
            protocol_version: 1,
            instance_id: state.events.instance.clone(),
            capabilities: AppCapabilities {
                state_invalidation: true,
                submission_receipts: true
            }
        })),
        "chat/submit" if state.app_protocol => boxed(handle_app_submit(params, connection)).await,
        "submission/read" if state.app_protocol => {
            let args: SubmissionRead = serde_json::from_value(params).map_err(|e| e.to_string())?;
            let receipt = state
                .submissions
                .read(
                    &connection.caller.to_string(),
                    &args.source,
                    &args.request_id,
                )
                .await?;
            to_json(receipt)
        }
        "ping" => Ok(json!({ "ok": true })),
        "browser_register" => handle_browser_register(params, connection),
        "agent_run" => {
            let (input,): (AgentInput,) = params_from_value(params)?;
            boxed(run_agent(connection, input)).await
        }
        "tool_call" => boxed(handle_tool_call(params, connection)).await,
        "brain_status" => boxed(handle_brain_status(state)).await,
        method if method.starts_with("memory_") => Ok(boxed(state.memory.websocket_dispatch(
            &state.auth_headers,
            method,
            params,
        ))
        .await),
        method if method.starts_with("mcp_") => {
            Ok(boxed(state.mcp.websocket_dispatch(method, params)).await)
        }
        "brain_kip_readonly" => boxed(handle_brain_kip_readonly(params, state)).await,
        "brain_attention" | "brain_respond" | "brain_runtime_status" => {
            boxed(handle_brain_runtime(method, params, connection)).await
        }
        "information" => to_json(connection.engine.information()),
        "ui_language" => Ok(json!({ "language": locale::persisted_ui_language(&state.home_dir) })),
        "pick_workspace" => {
            let path = workspace_picker::pick_workspace_path(&state.home_dir).await?;
            Ok(json!({ "path": path.map(|path| path.to_string_lossy().to_string()) }))
        }
        "register_workspace" => {
            let (workspace,): (PathBuf,) = params_from_value(params)?;
            let workspace = state
                .cli_workspaces
                .register(&workspace)
                .await
                .map_err(|err| err.to_string())?;
            Ok(json!({ "workspace": workspace }))
        }
        "capabilities" => Ok(capabilities(connection)),
        "model_names" => to_json(state.runtime_models.current()),
        "reload_models" => to_json(
            state
                .runtime_models
                .reload_from_config()
                .await
                .map_err(|err| err.to_string())?,
        ),
        "set_model" => handle_set_model(params, connection),
        method => Err(format!("{method} on WebSocket engine RPC not implemented")),
    }
}

async fn handle_app_submit(params: Value, connection: &Arc<WsConnection>) -> Result<Value, String> {
    let args: AppSubmit = serde_json::from_value(params).map_err(|e| e.to_string())?;
    let source = args
        .input
        .meta
        .as_ref()
        .and_then(|m| m.get_extra_as::<String>("source"))
        .ok_or("Chat source is required")?;
    if !connection.is_owner() || source.contains(":reply_target:") || !args.input.name.is_empty() {
        return Err("Desktop submissions require the local owner and a local chat".into());
    }
    let input = serde_json::to_value(&args.input).map_err(|e| e.to_string())?;
    let caller = connection.caller.to_string();
    let run = connection.clone();
    let receipt = connection
        .state
        .submissions
        .submit(&caller, source, args.request_id, &input, async move {
            let result = crate::util::boxed(run_agent(&run, args.input)).await;
            run.state.events.changed(&run.caller.to_string());
            result
        })
        .await?;
    to_json(receipt)
}

fn handle_browser_register(params: Value, connection: &WsConnection) -> Result<Value, String> {
    let (args,): (BrowserRegisterArgs,) = params_from_value(params)?;
    let session = connection
        .state
        .bridge
        .register_ws_session(
            connection.id,
            connection.caller,
            connection.actions.clone(),
            args,
            connection.state.app_protocol,
        )
        .map_err(|err| err.to_string())?;
    Ok(json!({ "registered": true, "session": session }))
}

async fn run_agent(connection: &WsConnection, input: AgentInput) -> Result<Value, String> {
    let output = connection
        .engine
        .agent_run(connection.caller, input)
        .await
        .map_err(|err| format!("failed to run agent: {err:?}"))?;
    to_json(output)
}

async fn handle_tool_call(params: Value, connection: &WsConnection) -> Result<Value, String> {
    let (input,): (ToolInput<Json>,) = params_from_value(params)?;
    let _permit = if crate::runtime_admission::MAINTENANCE_TOOLS.contains(&input.name.as_str()) {
        None
    } else {
        Some(connection.state.admission.enter().map_err(str::to_string)?)
    };
    let output = connection
        .engine
        .tool_call(connection.caller, input)
        .await
        .map_err(|err| format!("failed to call tool: {err:?}"))?;
    to_json(output)
}

async fn handle_brain_status(state: &BrowserWebSocketState) -> Result<Value, String> {
    let status = state
        .brain
        .brain_status()
        .await
        .map_err(|err| format!("failed to query Brain status: {err:?}"))?;
    to_json(status)
}

async fn handle_brain_runtime(
    method: &str,
    params: Value,
    connection: &WsConnection,
) -> Result<Value, String> {
    let brain = &connection.state.brain;
    let result: Result<Value, BoxError> = async {
        match method {
            "brain_attention" => {
                let (query,): (brain::AttentionQuery,) = params_from_value(params)?;
                Ok(serde_json::to_value(brain.attention(&query).await?)?)
            }
            "brain_respond" => {
                let (id, response): (String, brain::AttentionResponse) = params_from_value(params)?;
                Ok(serde_json::to_value(brain.respond(&id, &response).await?)?)
            }
            _ => {
                let mut status = serde_json::to_value(brain.runtime_status().await?)?;
                if let Some(status) = status.as_object_mut() {
                    // This is the already verified WebSocket caller. The UI
                    // uses it only to retain pending idempotency keys across
                    // bearer-token rotation without mixing different users.
                    status.insert("caller".into(), connection.caller.to_text().into());
                }
                Ok(status)
            }
        }
    }
    .await;
    result.map_err(|err| err.to_string())
}

async fn handle_brain_kip_readonly(
    params: Value,
    state: &BrowserWebSocketState,
) -> Result<Value, String> {
    let (request,): (KipArgs,) = params_from_value(params)?;
    let response = state
        .brain
        .execute_kip_readonly(
            request
                .into_readonly_request()
                .map_err(|err| err.to_string())?,
        )
        .await
        .map_err(|err| format!("failed to execute read-only Brain KIP: {err:?}"))?;
    to_json(response)
}

fn capabilities(connection: &WsConnection) -> Value {
    let names = vec![
        TranscriptionManager::NAME.to_string(),
        TtsManager::NAME.to_string(),
    ];
    let tools = connection.engine.tools(Some(&names));
    let has_tool = |name: &str| {
        tools
            .iter()
            .any(|tool| tool.definition.name.as_str() == name)
    };
    let voice = &connection.state.voice_capabilities;
    let transcription: &[String] = if has_tool(TranscriptionManager::NAME) {
        &voice.transcription
    } else {
        &[]
    };
    let tts: &[String] = if has_tool(TtsManager::NAME) {
        &voice.tts
    } else {
        &[]
    };

    json!({
        "desktop": {
            "protocol": 1,
            "app_transport": true,
            "maintenance": true,
            "workspace_sources": true,
            "config_revision": true,
            "runtime_version": env!("CARGO_PKG_VERSION"),
        },
        "transcription": transcription,
        "tts": tts,
    })
}

fn handle_set_model(params: Value, connection: &WsConnection) -> Result<Value, String> {
    let (model_name,): (String,) = params_from_value(params)?;
    let model_name = model_name.trim();
    if model_name.is_empty() {
        return Err("model name is required".to_string());
    }

    let models = connection.engine.models();
    let model = models
        .get(model_name)
        .ok_or_else(|| format!("model {model_name:?} not found"))?;
    let runtime_models = &connection.state.runtime_models;
    runtime_models
        .check_plan_switch(model.model_name().starts_with("chatgpt:"))
        .map_err(|e| e.to_string())?;
    models.set_model(model);
    to_json(runtime_models.current())
}

fn handle_browser_ws_response(connection: &WsConnection, incoming: BrowserWsIncoming) {
    let Some(id) = incoming.id else {
        return;
    };
    let Some(session) = incoming.session else {
        log::warn!("Chrome browser response {id} is missing session");
        return;
    };

    let result = match incoming.error {
        Some(error) => BrowserActionResult::error(error),
        None => serde_json::from_value(incoming.result.unwrap_or_default()).unwrap_or_else(|err| {
            BrowserActionResult::error(format!("invalid browser action response: {err}"))
        }),
    };

    if let Err(err) = connection
        .state
        .bridge
        .complete(connection.id, &session, id, result)
    {
        log::warn!("failed to complete Chrome browser action {id}: {err}");
    }
}

fn to_json(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|err| err.to_string())
}

fn params_from_value<T>(value: Value) -> Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|err| format!("failed to decode params: {err}"))
}

fn resolve_engine(app: &AppState, id: &str) -> Result<Arc<Engine>, (StatusCode, String)> {
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

    app.engines.get(&id).cloned().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("engine {} not found", id.to_text()),
        )
    })
}

/// Expiry of a bearer CWT that `verify_trusted_user` already accepted.
fn bearer_expires_at_ms(token: &str) -> Option<u64> {
    use cose2::cwt::{Claims, NumericDate};
    use std::str::FromStr;
    let bytes = ic_auth_types::ByteBufB64::from_str(token).ok()?;
    let payload = cose2::Sign1Message::from_slice(&bytes).ok()?.payload?;
    let claims = Claims::from_slice(&payload)
        .or_else(|_| Claims::from_slice_legacy_tagged(&payload))
        .ok()?;
    match claims.expiration? {
        NumericDate::Integer(secs) => u64::try_from(secs).ok()?.checked_mul(1000),
        NumericDate::Float(secs) => {
            (secs.is_finite() && secs >= 0.0).then_some((secs * 1000.0) as u64)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::{Agent, AgentOutput, FunctionDefinition, Resource, Tool, ToolOutput};
    use anda_engine::{
        context::{AgentCtx, BaseCtx},
        engine::{AgentInfo, Engine},
        management::{BaseManagement, Visibility},
        model::{Model, ModelConfig, Models},
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct EchoAgent;

    impl Agent<AgentCtx> for EchoAgent {
        fn name(&self) -> String {
            "echo_agent".to_string()
        }
        fn description(&self) -> String {
            "echo".to_string()
        }
        async fn run(
            &self,
            _ctx: AgentCtx,
            prompt: String,
            _resources: Vec<Resource>,
        ) -> Result<AgentOutput, BoxError> {
            Ok(AgentOutput {
                content: prompt,
                ..Default::default()
            })
        }
    }

    struct EchoTool;

    impl Tool<BaseCtx> for EchoTool {
        type Args = Json;
        type Output = Json;
        fn name(&self) -> String {
            "echo_tool".to_string()
        }
        fn description(&self) -> String {
            "echo".to_string()
        }
        fn definition(&self) -> FunctionDefinition {
            FunctionDefinition {
                name: self.name(),
                description: self.description(),
                parameters: json!({"type": "object"}),
                strict: Some(false),
            }
        }
        async fn call(
            &self,
            _ctx: BaseCtx,
            args: Json,
            _resources: Vec<Resource>,
        ) -> Result<ToolOutput<Json>, BoxError> {
            Ok(ToolOutput::new(args))
        }
    }

    fn dead_http() -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap()
    }

    async fn build_ws_state(
        home: PathBuf,
    ) -> (
        BrowserWebSocketState,
        Principal,
        crate::identity::Ed25519Key,
    ) {
        let auth_key = crate::identity::Ed25519Key::new([9u8; 32]);

        let engine = Arc::new(
            Engine::builder()
                .with_info(AgentInfo {
                    handle: "e".to_string(),
                    name: "E".to_string(),
                    description: "test".to_string(),
                    endpoint: "https://example.com/engine".to_string(),
                    ..Default::default()
                })
                .with_management(Arc::new(BaseManagement {
                    controller: Principal::management_canister(),
                    managers: BTreeSet::new(),
                    visibility: Visibility::Public,
                }))
                .with_model(Model::mock_implemented())
                .register_tool(Arc::new(EchoTool))
                .unwrap()
                .register_agent(Arc::new(EchoAgent), None)
                .unwrap()
                .export_tools(vec!["echo_tool".to_string()])
                .build("echo_agent".to_string())
                .await
                .unwrap(),
        );
        let engine_id = engine.id();

        let app = AppState {
            engines: Arc::new(BTreeMap::from([(engine_id, engine)])),
            default_engine: engine_id,
            start_time_ms: 0,
            extra_info: Arc::new(BTreeMap::new()),
            ed25519_pubkeys: Arc::new(vec![auth_key.pubkey().into()]),
        };

        let http = dead_http();
        let brain = brain::Client::new(
            "http://127.0.0.1:1/v1/anda_bot".to_string(),
            Some("t".to_string()),
        )
        .with_http_client(http.clone());

        let config_path = home.join("config.yaml");
        let models = Arc::new(Models::from_configs(
            &[ModelConfig {
                family: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_base: "http://127.0.0.1:1/v1".to_string(),
                api_key: "k".to_string(),
                labels: vec!["memory".to_string()],
                ..Default::default()
            }],
            http.clone(),
        ));
        let runtime_models = RuntimeModels::new(models.clone(), models, config_path, http.clone());

        let token = auth_key
            .sign_cwt(crate::identity::expiring_claims(std::time::Duration::from_secs(60)).unwrap())
            .unwrap();
        let state = BrowserWebSocketState {
            admission: Arc::new(crate::runtime_admission::Admission::default()),
            app_protocol: false,
            events: Arc::new(super::super::app_protocol::AppEvents::default()),
            submissions: super::super::app_protocol::Submissions::new(&home),
            memory: super::super::memory_api::MemoryApiState {
                app: app.clone(),
                owner: auth_key.id(),
                service: brain::MemoryService::new(brain.clone()),
            },
            mcp: {
                let manager = super::super::mcp::McpManager::for_test(&home).await;
                super::super::mcp::McpApiState {
                    app: app.clone(),
                    owner: auth_key.id(),
                    admission: Arc::new(crate::runtime_admission::Admission::default()),
                    events: super::super::mcp::McpEventRuntime::for_test(manager.clone()).await,
                    manager,
                    http: reqwest::Client::builder().no_proxy().build().unwrap(),
                    registry_url: super::super::mcp::MCP_REGISTRY_URL.to_string(),
                }
            },
            auth_headers: {
                let mut headers = HeaderMap::new();
                headers.insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
                headers
            },
            credential_expires_at_ms: bearer_expires_at_ms(&token).unwrap(),
            app,
            brain,
            bridge: Arc::new(BrowserBridge::new()),
            voice_capabilities: BrowserVoiceCapabilities::default(),
            home_dir: home,
            runtime_models,
            cli_workspaces: CliWorkspaceGrants::new(auth_key.id()),
        };
        (state, engine_id, auth_key)
    }

    /// A connection context for `caller`, with the frames it writes and the
    /// browser actions it forwards.
    fn connect(
        state: &BrowserWebSocketState,
        caller: Principal,
    ) -> (
        Arc<WsConnection>,
        mpsc::Receiver<String>,
        mpsc::Receiver<BrowserCommand>,
    ) {
        let (id, actions, commands) = state.bridge.open_ws_connection();
        let (writer, frames) = mpsc::channel(64);
        let engine = state.app.engines[&state.app.default_engine].clone();
        let connection = WsConnection {
            state: state.clone(),
            caller,
            engine,
            id,
            actions,
            writer,
        };
        (Arc::new(connection), frames, commands)
    }

    #[tokio::test]
    async fn browser_ws_request_dispatches_all_methods() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _, key) = build_ws_state(dir.path().to_path_buf()).await;
        let (connection, mut write_rx, _commands) = connect(&state, key.id());

        let call = |method: &str, params: Value| {
            serde_json::from_value::<BrowserWsIncoming>(json!({
                "id": 1,
                "method": method,
                "params": params,
            }))
            .unwrap()
        };

        // Methods that do not need a live network/engine response.
        for (method, params) in [
            ("ping", json!({})),
            (
                "browser_register",
                json!([{"session": "chrome:tab:1", "tab_id": 1}]),
            ),
            ("ui_language", json!({})),
            ("information", json!({})),
            ("capabilities", json!({})),
            ("model_names", json!({})),
            ("brain_status", json!({})),
            (
                "brain_kip_readonly",
                json!([anda_kip::Request::single("DESCRIBE PRIMER")]),
            ),
            ("agent_run", json!([{"name": "echo_agent", "prompt": "hi"}])),
            ("tool_call", json!([{"name": "echo_tool", "args": {}}])),
            ("reload_models", json!({})),
            ("set_model", json!(["missing-model"])),
            ("unknown_method", json!({})),
        ] {
            handle_browser_ws_request(call(method, params), &connection).await;
        }

        // Every request carried an id, so each produced a response frame.
        let mut responses = 0;
        while write_rx.try_recv().is_ok() {
            responses += 1;
        }
        assert!(responses >= 10, "expected response frames, got {responses}");

        // handle_browser_ws_text parses raw frames and routes method calls,
        // responses, and rejects malformed input.
        let request_tasks = CancellationToken::new();
        handle_browser_ws_text(
            &connection,
            "{\"id\":2,\"method\":\"ping\"}",
            &request_tasks,
        );
        handle_browser_ws_text(
            &connection,
            "{\"id\":3,\"result\":{\"ok\":true},\"session\":\"chrome:tab:1\"}",
            &request_tasks,
        );
        handle_browser_ws_text(&connection, "not-json", &request_tasks);
    }

    #[tokio::test]
    async fn browser_workspace_registration_requires_owner_and_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("project");
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let (state, _, key) = build_ws_state(dir.path().to_path_buf()).await;
        let (stranger, mut stranger_rx, _) = connect(&state, Principal::anonymous());
        let (owner, mut write_rx, _) = connect(&state, key.id());
        let request = |path: &std::path::Path| {
            serde_json::from_value::<BrowserWsIncoming>(json!({
                "id": 1,
                "method": "register_workspace",
                "params": [path],
            }))
            .unwrap()
        };

        handle_browser_ws_request(request(&workspace), &stranger).await;
        let denied: Value = serde_json::from_str(&stranger_rx.recv().await.unwrap()).unwrap();
        assert_eq!(
            denied["error"],
            "Only the local owner may control the daemon"
        );

        // Other daemon controls are owner-only as well, MCP management
        // included, reads too: they show the owner's servers.
        for method in [
            "pick_workspace",
            "reload_models",
            "set_model",
            "mcp_list",
            "mcp_reload",
        ] {
            let request =
                serde_json::from_value(json!({"id":1,"method":method,"params":["m"]})).unwrap();
            handle_browser_ws_request(request, &stranger).await;
            let denied: Value = serde_json::from_str(&stranger_rx.recv().await.unwrap()).unwrap();
            assert_eq!(
                denied["error"],
                "Only the local owner may control the daemon"
            );
        }

        handle_browser_ws_request(request(&workspace), &owner).await;
        let granted: Value = serde_json::from_str(&write_rx.recv().await.unwrap()).unwrap();
        assert_eq!(
            granted["result"]["workspace"],
            json!(workspace.canonicalize().unwrap())
        );

        handle_browser_ws_request(request(&dir.path().join("missing")), &owner).await;
        let invalid: Value = serde_json::from_str(&write_rx.recv().await.unwrap()).unwrap();
        assert!(
            invalid["error"]
                .as_str()
                .unwrap()
                .contains("cannot resolve workspace")
        );
    }

    #[tokio::test]
    async fn mcp_methods_answer_the_owner_in_the_tool_response_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _, key) = build_ws_state(dir.path().to_path_buf()).await;
        let (owner, mut write_rx, _) = connect(&state, key.id());
        for (method, params) in [
            ("mcp_list", json!({})),
            ("mcp_get", json!({ "id": "missing" })),
            ("mcp_apply", json!({ "change": { "op": "rename" } })),
        ] {
            let request =
                serde_json::from_value(json!({"id": 1, "method": method, "params": params}))
                    .unwrap();
            handle_browser_ws_request(request, &owner).await;
        }

        let list: Value = serde_json::from_str(&write_rx.recv().await.unwrap()).unwrap();
        assert_eq!(list["result"]["result"]["running"], true);
        assert_eq!(list["result"]["result"]["servers"], json!([]));
        let missing: Value = serde_json::from_str(&write_rx.recv().await.unwrap()).unwrap();
        assert_eq!(missing["result"]["error"]["code"], "not_found");
        let invalid: Value = serde_json::from_str(&write_rx.recv().await.unwrap()).unwrap();
        assert_eq!(invalid["result"]["error"]["code"], "invalid_request");
    }

    /// The WebSocket base for a mock server started by `spawn_http_mock`,
    /// which hands back an `http://` base URL.
    fn ws_base(base_url: &str) -> String {
        base_url.replacen("http://", "ws://", 1)
    }

    #[tokio::test]
    async fn browser_websocket_upgrades_and_round_trips_a_message() {
        use crate::identity::iana;
        use tokio_tungstenite::tungstenite::Message as TMessage;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let dir = tempfile::tempdir().unwrap();
        let (state, engine_id, key) = build_ws_state(dir.path().to_path_buf()).await;
        let events = state.events.clone();

        let app = axum::Router::new()
            .route("/{id}/browser_ws", axum::routing::any(browser_websocket))
            .with_state(state);
        let base_url = crate::test_support::spawn_http_mock(app).await;

        let mut claims =
            crate::identity::expiring_claims(std::time::Duration::from_secs(60)).unwrap();
        claims.extra.insert(iana::CWTClaimScope, "*");
        let token = key.sign_cwt(claims).unwrap();

        let url = format!("{}/{}/browser_ws", ws_base(&base_url), engine_id.to_text());
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());

        let (mut ws, _resp) = tokio_tungstenite::connect_async(request)
            .await
            .expect("websocket handshake should succeed with a valid token");

        ws.send(TMessage::Text("{\"id\":1,\"method\":\"ping\"}".into()))
            .await
            .unwrap();
        let reply = ws.next().await.expect("a reply frame").unwrap();
        assert!(reply.is_text());

        // The extension refreshes its channel list on the caller's changes.
        events.changed("another-caller");
        events.changed(&key.id().to_string());
        let notification = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let notification: Value = serde_json::from_str(notification.to_text().unwrap()).unwrap();
        assert_eq!(notification["method"], "state/changed");
        assert_eq!(notification["params"]["instanceId"], events.instance);

        ws.close(None).await.ok();
    }

    #[tokio::test]
    async fn desktop_application_transport_pushes_owned_state_and_replays_receipts() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let dir = tempfile::tempdir().unwrap();
        let (state, _, owner) = build_ws_state(dir.path().to_path_buf()).await;
        let events = state.events.clone();
        let authorization = state.auth_headers.get(AUTHORIZATION).unwrap().clone();
        let app = axum::Router::new()
            .route("/ws/app/v1", axum::routing::get(app_websocket))
            .with_state(state);
        let base = crate::test_support::spawn_http_mock(app).await;
        let mut request = format!("{}/ws/app/v1", ws_base(&base))
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(AUTHORIZATION, authorization);
        let mut forbidden = request.clone();
        forbidden
            .headers_mut()
            .insert("origin", "https://example.com".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(forbidden).await.is_err());
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let input = json!({"name":"", "prompt":"receipt test", "meta":{"source":"desktop:test"}});
        let requests = [
            json!({"id":1,"jsonrpc":"2.0","method":"initialize","params":{}}),
            json!({"id":2,"jsonrpc":"2.0","method":"chat/submit","params":{"requestId":"one","input":input}}),
            json!({"id":3,"jsonrpc":"2.0","method":"chat/submit","params":{"requestId":"one","input":input}}),
            json!({"id":4,"jsonrpc":"2.0","method":"submission/read","params":{"requestId":"one","source":"desktop:test"}}),
        ];
        let mut receipts = Vec::new();
        for request in requests {
            ws.send(Message::Text(request.to_string().into()))
                .await
                .unwrap();
            loop {
                let message = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let response: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if response.get("id") == request.get("id") {
                    assert_eq!(response["jsonrpc"], "2.0");
                    assert!(response.get("error").is_none(), "{response}");
                    if request["id"] != 1 {
                        receipts.push(response["result"].clone());
                    }
                    break;
                }
            }
        }
        assert_eq!(receipts[0]["state"], "completed");
        assert_eq!(receipts[0], receipts[1]);
        assert_eq!(receipts[1], receipts[2]);
        events.changed("another-caller");
        events.changed(&owner.id().to_string());
        let notification = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let notification: Value = serde_json::from_str(notification.to_text().unwrap()).unwrap();
        assert_eq!(notification["method"], "state/changed");
        assert_eq!(notification["params"]["instanceId"], events.instance);
        ws.close(None).await.unwrap();
    }

    #[tokio::test]
    async fn desktop_browser_registration_preserves_other_chats_and_pending_actions() {
        for app_protocol in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (mut state, _, key) = build_ws_state(dir.path().to_path_buf()).await;
            state.app_protocol = app_protocol;
            let owner = key.id();
            let (connection, _frames, mut commands) = connect(&state, owner);
            let first = "browser:desktop:first";
            let second = "browser:desktop:second";
            handle_browser_register(json!([{ "session": first }]), &connection).unwrap();
            let bridge = state.bridge.clone();
            let task = tokio::spawn(async move {
                bridge
                    .run_action(
                        owner,
                        first.into(),
                        serde_json::from_value(json!({
                            "action": "snapshot", "timeout_ms": 1000
                        }))
                        .unwrap(),
                    )
                    .await
            });
            let command = commands.recv().await.unwrap();
            handle_browser_register(json!([{ "session": second }]), &connection).unwrap();
            assert_eq!(
                state.bridge.connected_session(owner, Some(first)).is_some(),
                app_protocol
            );
            assert!(
                state
                    .bridge
                    .connected_session(owner, Some(second))
                    .is_some()
            );
            if app_protocol {
                state
                    .bridge
                    .complete(
                        connection.id,
                        first,
                        command.request_id,
                        BrowserActionResult::ok(json!({ "title": "First chat" })),
                    )
                    .unwrap();
                assert_eq!(task.await.unwrap().unwrap().value["title"], "First chat");
            } else {
                assert!(task.await.unwrap().is_err());
            }
            state.bridge.disconnect_ws_connection(connection.id);
            assert!(state.bridge.connected_session(owner, Some(first)).is_none());
            assert!(
                state
                    .bridge
                    .connected_session(owner, Some(second))
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn websocket_brain_proxy_forwards_verified_bearer_and_application_kip_args() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let dir = tempfile::tempdir().unwrap();
        let (mut state, engine_id, key) = build_ws_state(dir.path().to_path_buf()).await;
        let mut claims =
            crate::identity::expiring_claims(std::time::Duration::from_secs(60)).unwrap();
        claims
            .extra
            .insert(crate::identity::iana::CWTClaimScope, "*");
        let token = key.sign_cwt(claims).unwrap();
        let expected = format!("Bearer {token}");
        let brain = axum::Router::new()
            .route(
                "/v1/anda_bot/execute_kip_readonly",
                axum::routing::post(
                    move |headers: HeaderMap, axum::Json(body): axum::Json<Value>| {
                        let expected = expected.clone();
                        async move {
                            assert_eq!(headers[AUTHORIZATION], expected);
                            assert!(body.get("kip").is_none());
                            assert_eq!(body["operations"][0]["op_id"], "read-one");
                            assert_eq!(body["parameters"]["name"], "safe bound name");
                            axum::Json(anda_kip::Response::ok(
                                json!({"identity":"original caller"}),
                            ))
                        }
                    },
                ),
            )
            .route(
                "/v1/anda_bot/runtime/status",
                axum::routing::get(|| async {
                    axum::Json(json!({"result": {
                        "supported": true,
                        "configured": false,
                        "scope": null,
                        "attention_enabled": false,
                        "actions_enabled": false,
                        "observation_enabled": false,
                        "observer_authenticated": false,
                        "blocked_reasons": ["runtime_bindings_not_installed"],
                        "visible_items": 0,
                        "inventory_complete": false
                    }}))
                }),
            );
        let brain_url = crate::test_support::spawn_http_mock(brain).await;
        state.brain = brain::Client::new(
            format!("{brain_url}/v1/anda_bot"),
            Some("must-not-use-global-token".into()),
        );
        let app = axum::Router::new()
            .route("/{id}/browser_ws", axum::routing::any(browser_websocket))
            .with_state(state);
        let url = crate::test_support::spawn_http_mock(app).await;
        let mut request = format!("{}/{}/browser_ws", ws_base(&url), engine_id.to_text())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        ws.send(Message::Text(json!({"id":9,"method":"brain_kip_readonly","params":[{"operations":[{"op_id":"read-one","command":"DESCRIBE PRIMER"}],"parameters":{"name":"safe bound name"}}]}).to_string().into())).await.unwrap();
        let reply = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["result"]["status"], "succeeded", "{reply}");

        ws.send(Message::Text(
            json!({"id":10,"method":"brain_runtime_status","params":[]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
        let reply = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["result"]["caller"], key.id().to_text(), "{reply}");
        ws.close(None).await.ok();
    }

    #[tokio::test]
    async fn browser_websocket_rejects_missing_token() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let dir = tempfile::tempdir().unwrap();
        let (state, engine_id, _key) = build_ws_state(dir.path().to_path_buf()).await;
        let app = axum::Router::new()
            .route("/{id}/browser_ws", axum::routing::any(browser_websocket))
            .with_state(state);
        let base_url = crate::test_support::spawn_http_mock(app).await;

        let url = format!("{}/{}/browser_ws", ws_base(&base_url), engine_id.to_text());
        let request = url.into_client_request().unwrap();
        // No Authorization header -> the upgrade is rejected (401), so the
        // handshake fails.
        assert!(tokio_tungstenite::connect_async(request).await.is_err());
    }

    #[tokio::test]
    async fn browser_requests_reject_expired_credentials_before_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, _, key) = build_ws_state(dir.path().into()).await;
        let expired = key
            .sign_cwt(crate::identity::Claims {
                expiration: Some(1.into()),
                ..Default::default()
            })
            .unwrap();
        state.credential_expires_at_ms = bearer_expires_at_ms(&expired).unwrap();
        assert_eq!(state.credential_expires_at_ms, 1000);
        let (connection, mut rx, _commands) = connect(&state, key.id());
        for method in [
            "browser_register",
            "agent_run",
            "tool_call",
            "reload_models",
        ] {
            let request = serde_json::from_value(
                json!({"id":1,"method":method,"params":[{"session":"expired"}]}),
            )
            .unwrap();
            handle_browser_ws_request(request, &connection).await;
            let response: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
            assert_eq!(response["error"], "invalid or expired credential");
        }
        assert!(state.bridge.connected_session(key.id(), None).is_none());
    }

    #[tokio::test]
    async fn replies_follow_each_transport_and_only_the_sending_socket_completes_actions() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, _, key) = build_ws_state(dir.path().into()).await;
        let reply = async |connection: &WsConnection, frames: &mut mpsc::Receiver<String>| {
            connection.reply(1, Ok(Value::Null)).await;
            connection.reply(2, Err("boom".into())).await;
            let ok: Value = serde_json::from_str(&frames.recv().await.unwrap()).unwrap();
            let failed: Value = serde_json::from_str(&frames.recv().await.unwrap()).unwrap();
            (ok, failed)
        };
        let (extension, mut frames, mut commands) = connect(&state, key.id());
        assert_eq!(
            reply(&extension, &mut frames).await,
            (
                json!({ "id": 1, "result": null }),
                json!({ "id": 2, "error": "boom" })
            )
        );
        state.app_protocol = true;
        let (desktop, mut desktop_frames, _) = connect(&state, key.id());
        assert_eq!(
            reply(&desktop, &mut desktop_frames).await,
            (
                json!({ "jsonrpc": "2.0", "id": 1, "result": null }),
                json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -32000, "message": "boom" } })
            )
        );

        handle_browser_register(json!([{ "session": "browser:chrome:1" }]), &extension).unwrap();
        let bridge = state.bridge.clone();
        let owner = key.id();
        let task = tokio::spawn(async move {
            bridge
                .run_action(
                    owner,
                    "browser:chrome:1".into(),
                    serde_json::from_value(json!({ "action": "snapshot", "timeout_ms": 1000 }))
                        .unwrap(),
                )
                .await
        });
        let command = commands.recv().await.unwrap();
        let response = json!({
            "id": command.request_id,
            "session": "browser:chrome:1",
            "result": { "ok": true, "value": { "title": "Spoofed" } }
        })
        .to_string();
        let request_tasks = CancellationToken::new();
        handle_browser_ws_text(&desktop, &response, &request_tasks);
        handle_browser_ws_text(
            &extension,
            &response.replace("Spoofed", "Real"),
            &request_tasks,
        );
        assert_eq!(task.await.unwrap().unwrap().value["title"], "Real");
    }
}
