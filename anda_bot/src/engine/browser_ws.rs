use anda_core::{AgentInput, BoxError, Json, Principal, ToolInput};
use anda_engine::unix_ms;
use anda_engine_server::handler::AppState;
use anda_kip::Request as KipRequest;
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
use rust_i18n::t;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    env,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::{process::Command, sync::mpsc};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, handshake::derive_accept_key, protocol::Role},
};

use super::{
    RuntimeModels,
    browser::{BrowserActionResult, BrowserBridge, BrowserCommand},
};
use crate::brain;
#[cfg(target_os = "windows")]
use crate::util::windows_process::suppress_tokio_console_window;
use crate::{auto_update::AutoUpdater, transcription::TranscriptionManager, tts::TtsManager};

const SEC_WEBSOCKET_ACCEPT: &str = "sec-websocket-accept";
const SEC_WEBSOCKET_KEY: &str = "sec-websocket-key";
const SEC_WEBSOCKET_VERSION: &str = "sec-websocket-version";

#[derive(Clone)]
pub struct BrowserWebSocketState {
    pub app: AppState,
    pub brain: brain::Client,
    pub bridge: Arc<BrowserBridge>,
    pub voice_capabilities: BrowserVoiceCapabilities,
    pub auto_updater: Arc<AutoUpdater>,
    pub home_dir: PathBuf,
    pub(crate) runtime_models: RuntimeModels,
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

#[derive(Clone)]
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
        // Handle requests on their own task: agent runs, folder pickers, and
        // auto-update checks can take seconds to minutes, and the read loop
        // must keep draining pings and browser-action responses meanwhile.
        let state = state.clone();
        let connection = connection.clone();
        let write_sender = write_sender.clone();
        tokio::spawn(async move {
            handle_browser_ws_request(
                incoming,
                &state,
                caller,
                engine_id,
                &connection,
                &write_sender,
            )
            .await;
        });
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
        "brain_status" => handle_brain_status(state).await,
        "brain_kip_readonly" => handle_brain_kip_readonly(incoming.params, state).await,
        "information" => handle_information(state, engine_id),
        "ui_language" => handle_ui_language(state),
        "pick_workspace" => handle_pick_workspace().await,
        "capabilities" => handle_capabilities(state, engine_id),
        "model_names" => handle_model_names(state).await,
        "reload_models" => handle_reload_models(state).await,
        "set_model" => handle_set_model(incoming.params, state, engine_id).await,
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

async fn handle_brain_status(state: &BrowserWebSocketState) -> Result<Value, String> {
    let status = state
        .brain
        .brain_status()
        .await
        .map_err(|err| format!("failed to query Brain status: {err:?}"))?;
    serde_json::to_value(status).map_err(|err| err.to_string())
}

async fn handle_brain_kip_readonly(
    params: Value,
    state: &BrowserWebSocketState,
) -> Result<Value, String> {
    let (request,): (KipRequest,) = params_from_value(params)?;
    let response = state
        .brain
        .execute_kip_readonly(request)
        .await
        .map_err(|err| format!("failed to execute read-only Brain KIP: {err:?}"))?;
    serde_json::to_value(response).map_err(|err| err.to_string())
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

fn handle_ui_language(state: &BrowserWebSocketState) -> Result<Value, String> {
    Ok(json!({ "language": launcher_ui_language(&state.home_dir) }))
}

/// Reads the UI language the launcher persisted (launcher/ui.json) so the
/// browser extension can follow language switches made in the launcher menu.
/// Read per call: the launcher may rewrite the file while the daemon runs.
fn launcher_ui_language(home_dir: &std::path::Path) -> Option<String> {
    #[derive(Deserialize)]
    struct LauncherUiSettings {
        #[serde(default)]
        language: String,
    }

    let content = std::fs::read_to_string(home_dir.join("launcher").join("ui.json")).ok()?;
    let settings = serde_json::from_str::<LauncherUiSettings>(&content).ok()?;
    let language = settings.language.trim().to_string();
    (!language.is_empty()).then_some(language)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspacePickerLanguage {
    En,
    ZhHans,
}

impl WorkspacePickerLanguage {
    fn locale(self) -> &'static str {
        match self {
            WorkspacePickerLanguage::En => "en",
            WorkspacePickerLanguage::ZhHans => "zh-Hans",
        }
    }
}

static WORKSPACE_PICKER_LANGUAGE: OnceLock<WorkspacePickerLanguage> = OnceLock::new();

fn workspace_picker_title() -> String {
    workspace_picker_title_for_language(workspace_picker_language())
}

fn workspace_picker_title_for_language(language: WorkspacePickerLanguage) -> String {
    t!("browser.workspace_picker_title", locale = language.locale()).into_owned()
}

fn workspace_picker_language() -> WorkspacePickerLanguage {
    *WORKSPACE_PICKER_LANGUAGE.get_or_init(detect_workspace_picker_language)
}

fn detect_workspace_picker_language() -> WorkspacePickerLanguage {
    language_from_tags(system_locale_tags())
}

fn language_from_tags<T>(tags: impl IntoIterator<Item = T>) -> WorkspacePickerLanguage
where
    T: AsRef<str>,
{
    for tag in tags {
        if let Some(language) = language_from_tag(tag.as_ref()) {
            return language;
        }
    }
    WorkspacePickerLanguage::En
}

fn language_from_tag(tag: &str) -> Option<WorkspacePickerLanguage> {
    let normalized = tag
        .trim()
        .trim_matches('"')
        .split('.')
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase();

    if normalized.starts_with("zh") || normalized.contains("chinese") {
        Some(WorkspacePickerLanguage::ZhHans)
    } else if normalized.starts_with("en") {
        Some(WorkspacePickerLanguage::En)
    } else {
        None
    }
}

fn system_locale_tags() -> Vec<String> {
    let mut tags = platform_locale_tags();
    tags.extend(environment_locale_tags());
    tags
}

#[cfg(target_os = "macos")]
fn platform_locale_tags() -> Vec<String> {
    let mut tags = macos_defaults_languages();
    if let Some(locale) = macos_defaults_value("AppleLocale") {
        tags.push(locale);
    }
    tags
}

#[cfg(target_os = "macos")]
fn macos_defaults_languages() -> Vec<String> {
    let Some(output) = macos_defaults_value("AppleLanguages") else {
        return Vec::new();
    };

    output
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .trim_end_matches(',')
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_defaults_value(key: &str) -> Option<String> {
    let output = std::process::Command::new("defaults")
        .arg("read")
        .arg("-g")
        .arg(key)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
fn platform_locale_tags() -> Vec<String> {
    let mut buffer = [0u16; 85];
    let len = unsafe {
        windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    if len <= 1 {
        return Vec::new();
    }
    vec![String::from_utf16_lossy(&buffer[..(len as usize - 1)])]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_locale_tags() -> Vec<String> {
    Vec::new()
}

fn environment_locale_tags() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| env::var(name).ok())
        .filter(|value| !value.trim().is_empty())
        .collect()
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
    let prompt = workspace_picker_title();
    let script = workspace_picker_macos_script(&prompt);
    let output = Command::new("osascript")
        .args(["-e", script.as_str()])
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

#[cfg(any(target_os = "macos", test))]
fn workspace_picker_macos_script(prompt: &str) -> String {
    format!(
        "POSIX path of (choose folder with prompt {})",
        applescript_string(prompt)
    )
}

#[cfg(any(target_os = "macos", test))]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "windows")]
async fn pick_workspace_path_windows() -> Result<Option<PathBuf>, String> {
    let title = workspace_picker_title();
    let script = workspace_picker_windows_script(&title);
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-STA")
        .arg("-Command")
        .arg(&script);
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

#[cfg(any(target_os = "windows", test))]
fn workspace_picker_windows_script(title: &str) -> String {
    format!(
        concat!(
            "$utf8 = [System.Text.UTF8Encoding]::new($false); ",
            "try {{ [Console]::OutputEncoding = $utf8 }} catch {{ }}; ",
            "$OutputEncoding = $utf8; ",
            "$title = {title}; ",
            "Add-Type -AssemblyName System.Windows.Forms > $null; ",
            "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog; ",
            "$dialog.Description = $title; ",
            "$dialog.UseDescriptionForTitle = $true; ",
            "$owner = New-Object System.Windows.Forms.Form; ",
            "$owner.Text = 'Anda Bot'; ",
            "$owner.StartPosition = 'CenterScreen'; ",
            "$owner.ShowInTaskbar = $false; ",
            "$owner.TopMost = $true; ",
            "$owner.Width = 1; ",
            "$owner.Height = 1; ",
            "$owner.Opacity = 0; ",
            "try {{ ",
            "$owner.Show(); ",
            "$owner.Activate(); ",
            "[void]$owner.Focus(); ",
            "$result = $dialog.ShowDialog($owner); ",
            "if ($result -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $dialog.SelectedPath }} ",
            "}} finally {{ $owner.Close(); $owner.Dispose(); $dialog.Dispose(); }}"
        ),
        title = powershell_single_quoted_string(title)
    )
}

#[cfg(any(target_os = "windows", test))]
fn powershell_single_quoted_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn pick_workspace_path_linux() -> Result<Option<PathBuf>, String> {
    let mut errors = Vec::new();
    let title = workspace_picker_title();

    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--directory".to_string(),
                format!("--title={title}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--getexistingdirectory".to_string(),
                ".".to_string(),
                title.clone(),
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

async fn handle_model_names(state: &BrowserWebSocketState) -> Result<Value, String> {
    serde_json::to_value(state.runtime_models.current().await).map_err(|err| err.to_string())
}

async fn handle_reload_models(state: &BrowserWebSocketState) -> Result<Value, String> {
    let models = state
        .runtime_models
        .reload_from_config()
        .await
        .map_err(|err| err.to_string())?;
    serde_json::to_value(models).map_err(|err| err.to_string())
}

async fn handle_set_model(
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
    let response = state
        .runtime_models
        .set_active_model(model_name.to_string())
        .await;
    serde_json::to_value(response).map_err(|err| err.to_string())
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
    use super::{
        WorkspacePickerLanguage, decode_bytes_with_windows_code_page, language_from_tag,
        language_from_tags, launcher_ui_language, normalize_selected_workspace_path,
        powershell_single_quoted_string, workspace_picker_macos_script,
        workspace_picker_title_for_language, workspace_picker_windows_script,
    };
    use std::{env, fs, path::MAIN_SEPARATOR};

    #[test]
    fn launcher_ui_language_reads_persisted_launcher_setting() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(launcher_ui_language(home.path()), None);

        let launcher_dir = home.path().join("launcher");
        fs::create_dir_all(&launcher_dir).unwrap();
        let ui_path = launcher_dir.join("ui.json");

        fs::write(&ui_path, r#"{"language": "zh-Hans"}"#).unwrap();
        assert_eq!(
            launcher_ui_language(home.path()),
            Some("zh-Hans".to_string())
        );

        fs::write(&ui_path, r#"{"language": "  "}"#).unwrap();
        assert_eq!(launcher_ui_language(home.path()), None);

        fs::write(&ui_path, "not json").unwrap();
        assert_eq!(launcher_ui_language(home.path()), None);
    }

    #[test]
    fn workspace_picker_language_prefers_chinese_system_tags() {
        assert_eq!(
            language_from_tag("zh_CN.UTF-8"),
            Some(WorkspacePickerLanguage::ZhHans)
        );
        assert_eq!(
            language_from_tag("Chinese (Simplified)"),
            Some(WorkspacePickerLanguage::ZhHans)
        );
        assert_eq!(
            language_from_tag("en-US"),
            Some(WorkspacePickerLanguage::En)
        );
        assert_eq!(
            language_from_tags(["fr-FR", "zh-Hans"]),
            WorkspacePickerLanguage::ZhHans
        );
        assert_eq!(
            language_from_tags(["fr-FR", "de-DE"]),
            WorkspacePickerLanguage::En
        );
    }

    #[test]
    fn workspace_picker_title_uses_locale_resources() {
        let en = workspace_picker_title_for_language(WorkspacePickerLanguage::En);
        let zh = workspace_picker_title_for_language(WorkspacePickerLanguage::ZhHans);

        assert_eq!(en, "Open a workspace folder for Anda");
        assert_ne!(zh, en);
        assert!(zh.contains("Anda"));
    }

    #[test]
    fn macos_workspace_picker_script_escapes_prompt_text() {
        let script = workspace_picker_macos_script("Choose \"Anda\" \\ folder");

        assert_eq!(
            script,
            "POSIX path of (choose folder with prompt \"Choose \\\"Anda\\\" \\\\ folder\")"
        );
    }

    #[test]
    fn windows_workspace_picker_script_uses_localized_title_and_owner() {
        let script = workspace_picker_windows_script("Choose Anda's workspace");

        assert!(script.contains("$title = 'Choose Anda''s workspace';"));
        assert!(script.contains("$dialog.Description = $title;"));
        assert!(script.contains("$owner.TopMost = $true;"));
        assert!(script.contains("$dialog.ShowDialog($owner);"));
    }

    #[test]
    fn powershell_single_quoted_string_escapes_quotes() {
        assert_eq!(
            powershell_single_quoted_string("Anda's workspace"),
            "'Anda''s workspace'"
        );
    }

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

    use super::*;
    use anda_core::{Agent, AgentOutput, FunctionDefinition, Resource, Tool, ToolOutput};
    use anda_db::{
        database::{AndaDB, DBConfig},
        storage::StorageConfig,
    };
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
        crate::util::key::Ed25519Key,
    ) {
        let auth_key = crate::util::key::Ed25519Key::new([9u8; 32]);
        let object_store: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        let db = Arc::new(
            AndaDB::connect(
                object_store,
                DBConfig {
                    name: "ws_test".to_string(),
                    description: "ws".to_string(),
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
            .unwrap(),
        );

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
        let auto_updater = Arc::new(AutoUpdater::new(db, home.clone(), http));

        let state = BrowserWebSocketState {
            app,
            brain,
            bridge: Arc::new(BrowserBridge::new()),
            voice_capabilities: BrowserVoiceCapabilities::default(),
            auto_updater,
            home_dir: home,
            runtime_models,
        };
        (state, engine_id, auth_key)
    }

    #[tokio::test]
    async fn browser_ws_request_dispatches_all_methods() {
        let dir = tempfile::tempdir().unwrap();
        let (state, engine_id, _key) = build_ws_state(dir.path().to_path_buf()).await;
        let caller = Principal::management_canister();

        let (cmd_tx, _cmd_rx) = mpsc::channel::<BrowserCommand>(8);
        let connection = BrowserWsConnection {
            id: 1,
            sender: cmd_tx,
        };
        let (write_tx, mut write_rx) = mpsc::channel::<String>(64);

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
            ("auto_update_status", json!({})),
            ("auto_update_check", json!({})),
            ("auto_update_install_and_restart", json!({})),
            ("brain_status", json!({})),
            (
                "brain_kip_readonly",
                json!([{"command": "DESCRIBE PRIMER"}]),
            ),
            ("agent_run", json!([{"name": "echo_agent", "prompt": "hi"}])),
            ("tool_call", json!([{"name": "echo_tool", "args": {}}])),
            ("reload_models", json!({})),
            ("set_model", json!(["missing-model"])),
            ("unknown_method", json!({})),
        ] {
            handle_browser_ws_request(
                call(method, params),
                &state,
                caller,
                engine_id,
                &connection,
                &write_tx,
            )
            .await;
        }

        // Every request carried an id, so each produced a response frame.
        let mut responses = 0;
        while write_rx.try_recv().is_ok() {
            responses += 1;
        }
        assert!(responses >= 10, "expected response frames, got {responses}");

        // handle_browser_ws_text parses raw frames and routes method calls,
        // responses, and rejects malformed input.
        handle_browser_ws_text(
            "{\"id\":2,\"method\":\"ping\"}",
            &state,
            caller,
            engine_id,
            &connection,
            &write_tx,
        )
        .await;
        handle_browser_ws_text(
            "{\"id\":3,\"result\":{\"ok\":true},\"session\":\"chrome:tab:1\"}",
            &state,
            caller,
            engine_id,
            &connection,
            &write_tx,
        )
        .await;
        handle_browser_ws_text(
            "not-json",
            &state,
            caller,
            engine_id,
            &connection,
            &write_tx,
        )
        .await;
    }

    #[tokio::test]
    async fn browser_websocket_upgrades_and_round_trips_a_message() {
        use crate::util::key::{Claims, iana};
        use tokio_tungstenite::tungstenite::Message as TMessage;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let dir = tempfile::tempdir().unwrap();
        let (state, engine_id, key) = build_ws_state(dir.path().to_path_buf()).await;

        let app = axum::Router::new()
            .route("/{id}/browser_ws", axum::routing::any(browser_websocket))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut claims = Claims::default();
        claims.extra.insert(iana::CWTClaimScope, "*");
        let token = key.sign_cwt(claims).unwrap();

        let url = format!("ws://{addr}/{}/browser_ws", engine_id.to_text());
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
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{addr}/{}/browser_ws", engine_id.to_text());
        let request = url.into_client_request().unwrap();
        // No Authorization header -> the upgrade is rejected (401), so the
        // handshake fails.
        assert!(tokio_tungstenite::connect_async(request).await.is_err());
    }
}
