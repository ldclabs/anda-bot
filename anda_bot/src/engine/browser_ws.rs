use anda_core::{AgentInput, BoxError, Json, Principal, ToolInput};
use anda_engine::{model::Models, unix_ms};
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
use std::{path::PathBuf, sync::Arc};
use tokio::{process::Command, sync::mpsc};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::derive_accept_key, protocol::Role},
};

use super::browser::{BrowserActionResult, BrowserBridge, BrowserCommand};
#[cfg(target_os = "windows")]
use crate::util::windows_process::suppress_tokio_console_window;
use crate::{auto_update::AutoUpdater, transcription::TranscriptionManager, tts::TtsManager};

const SEC_WEBSOCKET_ACCEPT: &str = "sec-websocket-accept";
const SEC_WEBSOCKET_KEY: &str = "sec-websocket-key";
const SEC_WEBSOCKET_VERSION: &str = "sec-websocket-version";

#[derive(Clone)]
pub struct BrowserWebSocketState {
    pub app: AppState,
    pub bridge: Arc<BrowserBridge>,
    pub voice_capabilities: BrowserVoiceCapabilities,
    pub auto_updater: Arc<AutoUpdater>,
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
        "pick_workspace" => handle_pick_workspace().await,
        "capabilities" => handle_capabilities(state, engine_id),
        "model_names" => handle_model_names(state, engine_id),
        "set_model" => handle_set_model(incoming.params, state, engine_id),
        "auto_update_status" => handle_auto_update_status(state),
        "auto_update_check" => handle_auto_update_check(state).await,
        "auto_update_install_and_restart" => handle_auto_update_install_and_restart(state).await,
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

async fn handle_pick_workspace() -> Result<Value, String> {
    let path = select_workspace_path().await?;
    Ok(json!({
        "path": path.map(|path| path.to_string_lossy().to_string())
    }))
}

async fn select_workspace_path() -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    {
        pick_workspace_path_macos().await
    }

    #[cfg(target_os = "windows")]
    {
        return pick_workspace_path_windows().await;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        pick_workspace_path_linux().await
    }
}

#[cfg(target_os = "macos")]
async fn pick_workspace_path_macos() -> Result<Option<PathBuf>, String> {
    let output = Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Open a workspace folder for Anda\")",
        ])
        .output()
        .await
        .map_err(|err| format!("failed to launch macOS folder picker: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") {
            return Ok(None);
        }
        return Err(format!(
            "macOS folder picker failed: {}",
            stderr.trim().trim_matches('"')
        ));
    }

    parse_selected_workspace_path(&output.stdout)
}

#[cfg(target_os = "windows")]
async fn pick_workspace_path_windows() -> Result<Option<PathBuf>, String> {
    let script = concat!(
        "$utf8 = [System.Text.UTF8Encoding]::new($false); ",
        "try { [Console]::OutputEncoding = $utf8 } catch { }; ",
        "$OutputEncoding = $utf8; ",
        "Add-Type -AssemblyName System.Windows.Forms > $null; ",
        "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog; ",
        "$dialog.Description = 'Open a workspace folder for Anda'; ",
        "$dialog.UseDescriptionForTitle = $true; ",
        "if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { Write-Output $dialog.SelectedPath }"
    );
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-STA", "-Command", script]);
    suppress_tokio_console_window(&mut command);
    let output = command
        .output()
        .await
        .map_err(|err| format!("failed to launch Windows folder picker: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Windows folder picker failed: {}",
            stderr.trim().trim_matches('"')
        ));
    }

    parse_selected_workspace_path(&output.stdout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn pick_workspace_path_linux() -> Result<Option<PathBuf>, String> {
    let mut errors = Vec::new();

    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection",
                "--directory",
                "--title=Open a workspace folder for Anda",
            ],
        ),
        (
            "kdialog",
            vec![
                "--getexistingdirectory",
                ".",
                "Open a workspace folder for Anda",
            ],
        ),
    ] {
        let output = match Command::new(program).args(&args).output().await {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                errors.push(format!("{program}: {err}"));
                continue;
            }
        };

        if output.status.success() {
            return parse_selected_workspace_path(&output.stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.code() == Some(1) && stderr.trim().is_empty() {
            return Ok(None);
        }

        errors.push(format!("{program}: {}", stderr.trim()));
    }

    if errors.is_empty() {
        Err("no supported folder picker found; install zenity or kdialog".to_string())
    } else {
        Err(errors.join("; "))
    }
}

fn parse_selected_workspace_path(stdout: &[u8]) -> Result<Option<PathBuf>, String> {
    let selected = decode_selected_workspace_stdout(stdout)
        .ok_or_else(|| "folder picker returned a non-text workspace path".to_string())?;
    Ok(normalize_selected_workspace_path(&selected))
}

fn decode_selected_workspace_stdout(stdout: &[u8]) -> Option<String> {
    if let Ok(text) = std::str::from_utf8(stdout) {
        return Some(text.to_string());
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(text) =
            decode_bytes_with_windows_code_page(stdout, windows_console_output_code_page())
        {
            return Some(text);
        }
        return anda_core::text_from_bytes(stdout).map(|text| text.into_owned());
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
fn decode_bytes_with_windows_code_page(bytes: &[u8], code_page: u32) -> Option<String> {
    anda_core::text_from_bytes_with_encoding(
        bytes,
        anda_core::windows_code_page_encoding(code_page),
    )
    .map(|text| text.into_owned())
}

#[cfg(target_os = "windows")]
fn windows_console_output_code_page() -> u32 {
    unsafe { windows_sys::Win32::Globalization::GetOEMCP() }
}

fn normalize_selected_workspace_path(selected: &str) -> Option<PathBuf> {
    let trimmed = selected.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path: PathBuf = std::path::Path::new(trimmed).components().collect();
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return None;
    }
    Some(path)
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

fn handle_model_names(
    state: &BrowserWebSocketState,
    engine_id: Principal,
) -> Result<Value, String> {
    let engine = state
        .app
        .engines
        .get(&engine_id)
        .ok_or_else(|| format!("engine {} not found", engine_id.to_text()))?;
    Ok(model_info_json(engine.models().as_ref()))
}

fn handle_set_model(
    params: Value,
    state: &BrowserWebSocketState,
    engine_id: Principal,
) -> Result<Value, String> {
    let (model_name,): (String,) = params_from_value(params)?;
    let model_name = model_name.trim();
    if model_name.is_empty() {
        return Err("model name is required".to_string());
    }

    let engine = state
        .app
        .engines
        .get(&engine_id)
        .ok_or_else(|| format!("engine {} not found", engine_id.to_text()))?;
    let models = engine.models();
    let model = models
        .get(model_name)
        .ok_or_else(|| format!("model {model_name:?} not found"))?;
    models.set_model(model);
    Ok(model_info_json(models.as_ref()))
}

fn handle_auto_update_status(state: &BrowserWebSocketState) -> Result<Value, String> {
    serde_json::to_value(state.auto_updater.state()).map_err(|err| err.to_string())
}

async fn handle_auto_update_check(state: &BrowserWebSocketState) -> Result<Value, String> {
    serde_json::to_value(state.auto_updater.check_if_due().await).map_err(|err| err.to_string())
}

async fn handle_auto_update_install_and_restart(
    state: &BrowserWebSocketState,
) -> Result<Value, String> {
    let update_state = state
        .auto_updater
        .install_and_restart()
        .await
        .map_err(|err| err.to_string())?;
    serde_json::to_value(update_state).map_err(|err| err.to_string())
}

fn model_info_json(models: &Models) -> Value {
    let active_model = models.get_model().map(|model| model.model_name());
    let model_names = models.model_names().into_iter().collect::<Vec<_>>();
    json!({
        "active_model": active_model,
        "model_names": model_names,
    })
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
            error_code: None,
        }
    } else {
        match serde_json::from_value::<BrowserActionResult>(incoming.result.unwrap_or(Value::Null))
        {
            Ok(result) => result,
            Err(err) => BrowserActionResult {
                ok: false,
                value: Value::Null,
                error: Some(format!("invalid browser action response: {err}")),
                error_code: None,
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

#[cfg(test)]
mod tests {
    use super::{decode_bytes_with_windows_code_page, normalize_selected_workspace_path};
    use std::{env, path::MAIN_SEPARATOR};

    #[test]
    fn normalize_selected_workspace_path_trims_and_drops_trailing_separator() {
        let expected = env::temp_dir().join("anda").join("workspace");
        let selected = format!("  {}{}  ", expected.display(), MAIN_SEPARATOR);
        let path = normalize_selected_workspace_path(&selected).unwrap();

        assert_eq!(path, expected);
    }

    #[test]
    fn normalize_selected_workspace_path_rejects_empty_or_relative_values() {
        assert_eq!(normalize_selected_workspace_path("   "), None);
        assert_eq!(normalize_selected_workspace_path("workspace/project"), None);
    }

    #[test]
    fn selected_workspace_stdout_decodes_legacy_chinese_windows_bytes() {
        let gbk_path = [b'E', b':', b'\\', 0xD6, 0xD0, 0xCE, 0xC4, b'\r', b'\n'];

        assert_eq!(
            decode_bytes_with_windows_code_page(&gbk_path, 936).as_deref(),
            Some("E:\\中文\r\n")
        );
    }
}
