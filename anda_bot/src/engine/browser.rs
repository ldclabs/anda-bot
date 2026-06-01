use anda_core::{
    BoxError, FunctionDefinition, RequestMeta, Resource, StateFeatures, Tool, ToolOutput,
};
use anda_engine::{context::BaseCtx, unix_ms};
use anda_kip::Response;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ic_auth_types::Xid;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Notify, mpsc, oneshot},
    time::Instant,
};

use crate::util::request_meta::request_meta_extra_as;

const DEFAULT_BROWSER_ACTION_TIMEOUT_MS: u64 = 60_000;
const MIN_BROWSER_ACTION_TIMEOUT_MS: u64 = 1_000;
const MAX_BROWSER_ACTION_TIMEOUT_MS: u64 = 120_000;
const BROWSER_SCREENSHOT_TMP_DIR: &str = "browser-screenshots";

#[derive(Debug, Default)]
pub struct BrowserBridge {
    next_request_id: AtomicU64,
    next_connection_id: AtomicU64,
    sessions: RwLock<HashMap<String, BrowserSession>>,
    connections: RwLock<HashMap<String, BrowserConnection>>,
    pending: Mutex<HashMap<u64, PendingBrowserRequest>>,
    notify: Notify,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserSession {
    pub session: String,
    pub connected_at: u64,
    pub last_seen_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug)]
struct PendingBrowserRequest {
    session: String,
    response: Option<oneshot::Sender<BrowserActionResult>>,
}

#[derive(Debug, Clone)]
struct BrowserConnection {
    connection_id: u64,
    sender: mpsc::Sender<BrowserCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAction {
    GetCurrentTab,
    Snapshot,
    ExtractText,
    GetFullPageHtml,
    GetStructuredData,
    GetElementInfo,
    GetViewportSize,
    GetAccessibilityTree,
    WaitForElement,
    Click,
    TypeText,
    PressKey,
    Scroll,
    ScrollTo,
    Hover,
    DragAndDrop,
    SelectDropdown,
    FindInPage,
    CopyToClipboard,
    UploadFile,
    Navigate,
    GoBack,
    GoForward,
    Reload,
    Screenshot,
    PrintToPdf,
    AnnotateViewport,
    ClearAnnotations,
    ReadSelection,
    Download,
    ListDownloads,
    CancelDownload,
    OpenDownload,
    GetCookies,
    SetCookie,
    DeleteCookie,
    ClearBrowserCache,
    ListTabs,
    SwitchTab,
    OpenTab,
    OpenFile,
    CloseTab,
    GetFrames,
    LaunchBrowser,
    ExecuteJavascript,
    HandleDialog,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChromeBrowserToolArgs {
    #[serde(default = "default_browser_action")]
    pub action: BrowserAction,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_bridge: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_x: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_y: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_selector: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_selector: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_links: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_forms: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_data_url: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_page: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport_width: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport_height: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_scale_factor: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass_cache: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_as: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_id: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origins: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_only: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_text: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn default_browser_action() -> BrowserAction {
    BrowserAction::ExecuteJavascript
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserCommand {
    pub request_id: u64,
    pub session: String,
    pub created_at: u64,
    pub args: ChromeBrowserToolArgs,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BrowserActionResult {
    #[serde(default)]
    pub ok: bool,

    #[serde(default = "json_null")]
    pub value: Value,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct ChromeBrowserTool {
    bridge: Arc<BrowserBridge>,
    kind: ChromeBrowserToolKind,
    screenshot_workspace: Option<Arc<PathBuf>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeBrowserToolKind {
    Tabs,
    Page,
    Input,
    Script,
}

impl BrowserBridge {
    pub fn new() -> Self {
        Self {
            next_request_id: AtomicU64::new(1),
            next_connection_id: AtomicU64::new(1),
            sessions: RwLock::new(HashMap::new()),
            connections: RwLock::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    pub(crate) fn open_ws_connection(
        &self,
    ) -> (
        u64,
        mpsc::Sender<BrowserCommand>,
        mpsc::Receiver<BrowserCommand>,
    ) {
        let connection_id = self.next_connection_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel(32);
        (connection_id, sender, receiver)
    }

    pub(crate) fn register_ws_session(
        &self,
        connection_id: u64,
        sender: mpsc::Sender<BrowserCommand>,
        session: String,
        tab_id: Option<i64>,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<BrowserSession, BoxError> {
        let session = self.register(session, tab_id, url, title)?;
        let mut connections = self.connections.write();
        connections.retain(|_, connection| connection.connection_id != connection_id);
        connections.insert(
            session.session.clone(),
            BrowserConnection {
                connection_id,
                sender,
            },
        );
        self.notify.notify_waiters();
        Ok(session)
    }

    pub(crate) fn disconnect_ws_connection(&self, connection_id: u64) {
        self.connections
            .write()
            .retain(|_, connection| connection.connection_id != connection_id);
    }

    pub fn register(
        &self,
        session: String,
        tab_id: Option<i64>,
        url: Option<String>,
        title: Option<String>,
    ) -> Result<BrowserSession, BoxError> {
        let session = normalize_session(session)?;
        let now_ms = unix_ms();
        let mut sessions = self.sessions.write();
        let entry = sessions
            .entry(session.clone())
            .or_insert_with(|| BrowserSession {
                session: session.clone(),
                connected_at: now_ms,
                last_seen_at: now_ms,
                tab_id: None,
                url: None,
                title: None,
            });
        entry.last_seen_at = now_ms;
        entry.tab_id = tab_id;
        entry.url = normalize_optional_string(url);
        entry.title = normalize_optional_string(title);
        let session = entry.clone();
        drop(sessions); // release lock before notifying to avoid waking up waiters only to block on the lock
        self.notify.notify_waiters();
        Ok(session)
    }

    pub fn connected_session(&self, preferred: Option<&str>) -> Option<String> {
        let preferred = preferred.and_then(|session| normalize_session(session.to_string()).ok());
        let connections = self.connections.read();

        if let Some(preferred) = preferred {
            return connections.contains_key(&preferred).then_some(preferred);
        }

        let connected = connections.keys().cloned().collect::<Vec<_>>();
        drop(connections);

        let sessions = self.sessions();
        sessions
            .into_iter()
            .find_map(|session| {
                connected
                    .contains(&session.session)
                    .then_some(session.session)
            })
            .or_else(|| connected.into_iter().next())
    }

    pub async fn wait_for_connected_session(
        &self,
        preferred: Option<String>,
        timeout_ms: u64,
    ) -> Option<String> {
        let preferred = preferred.and_then(|session| normalize_session(session).ok());
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let notified = self.notify.notified();

            if let Some(session) = self.connected_session(preferred.as_deref()) {
                return Some(session);
            }

            let now = Instant::now();
            if now >= deadline {
                return None;
            }

            if tokio::time::timeout(deadline - now, notified)
                .await
                .is_err()
            {
                return None;
            }
        }
    }

    pub async fn run_action(
        &self,
        session: String,
        args: ChromeBrowserToolArgs,
    ) -> Result<BrowserActionResult, BoxError> {
        let session = normalize_session(session)?;
        if self.session(&session).is_none() {
            return Err(format!(
                "Chrome extension session {session:?} is not connected. Open the Anda browser extension or launch the browser and try again."
            )
            .into());
        }
        let action_sender = self
            .connections
            .read()
            .get(&session)
            .map(|connection| connection.sender.clone());

        let request_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let created_at = unix_ms();
        let timeout_ms = normalized_action_timeout(args.timeout_ms);
        let command = BrowserCommand {
            request_id,
            session: session.clone(),
            created_at,
            args,
        };
        let (sender, receiver) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(
                request_id,
                PendingBrowserRequest {
                    session,
                    response: Some(sender),
                },
            );
        }
        if let Some(action_sender) = action_sender {
            if let Err(_err) = action_sender.send(command).await {
                self.pending.lock().await.remove(&request_id);
                return Err("Chrome browser WebSocket connection is closed".into());
            }
        } else {
            self.notify.notify_waiters();
        }

        match tokio::time::timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err("Chrome browser action response channel closed".into()),
            Err(_) => {
                self.pending.lock().await.remove(&request_id);
                Err(
                    format!("Chrome browser action {request_id} timed out after {timeout_ms}ms")
                        .into(),
                )
            }
        }
    }

    pub async fn complete(
        &self,
        session: String,
        request_id: u64,
        result: BrowserActionResult,
    ) -> Result<(), BoxError> {
        let session = normalize_session(session)?;
        self.touch_session(&session);
        let mut pending = self.pending.lock().await;
        let Some(mut request) = pending.remove(&request_id) else {
            return Err(format!("browser request {request_id} was not found").into());
        };

        if request.session != session {
            pending.insert(request_id, request);
            return Err(
                format!("browser request {request_id} belongs to a different session").into(),
            );
        }

        if let Some(sender) = request.response.take() {
            let _ = sender.send(result);
        }
        Ok(())
    }

    pub fn session(&self, session: &str) -> Option<BrowserSession> {
        self.sessions.read().get(session).cloned()
    }

    pub fn sessions(&self) -> Vec<BrowserSession> {
        let mut sessions = self.sessions.read().values().cloned().collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            right
                .last_seen_at
                .cmp(&left.last_seen_at)
                .then_with(|| left.session.cmp(&right.session))
        });
        sessions
    }

    fn touch_session(&self, session: &str) {
        if let Some(active_session) = self.sessions.write().get_mut(session) {
            active_session.last_seen_at = unix_ms();
        }
    }
}

impl ChromeBrowserTool {
    pub const TABS_NAME: &'static str = "chrome_tabs";
    pub const PAGE_NAME: &'static str = "chrome_page";
    pub const INPUT_NAME: &'static str = "chrome_input";
    pub const SCRIPT_NAME: &'static str = "chrome_script";

    pub fn tabs(bridge: Arc<BrowserBridge>) -> Self {
        Self::for_kind(bridge, ChromeBrowserToolKind::Tabs)
    }

    pub fn page(bridge: Arc<BrowserBridge>) -> Self {
        Self::for_kind(bridge, ChromeBrowserToolKind::Page)
    }

    pub fn input(bridge: Arc<BrowserBridge>) -> Self {
        Self::for_kind(bridge, ChromeBrowserToolKind::Input)
    }

    pub fn script(bridge: Arc<BrowserBridge>) -> Self {
        Self::for_kind(bridge, ChromeBrowserToolKind::Script)
    }

    fn for_kind(bridge: Arc<BrowserBridge>, kind: ChromeBrowserToolKind) -> Self {
        Self {
            bridge,
            kind,
            screenshot_workspace: None,
        }
    }

    pub fn with_screenshot_workspace(mut self, workspace: PathBuf) -> Self {
        self.screenshot_workspace = Some(Arc::new(workspace));
        self
    }

    pub fn is_active(&self) -> bool {
        !self.bridge.sessions.read().is_empty()
    }

    pub fn dependency_tool_names() -> [&'static str; 4] {
        [
            Self::TABS_NAME,
            Self::PAGE_NAME,
            Self::INPUT_NAME,
            Self::SCRIPT_NAME,
        ]
    }

    pub fn active_tool_names() -> [&'static str; 4] {
        [
            Self::TABS_NAME,
            Self::PAGE_NAME,
            Self::INPUT_NAME,
            Self::SCRIPT_NAME,
        ]
    }

    fn screenshot_tmp_dir(&self) -> PathBuf {
        self.screenshot_workspace
            .as_ref()
            .map(|workspace| workspace.join(BROWSER_SCREENSHOT_TMP_DIR))
            .unwrap_or_else(|| {
                std::env::temp_dir()
                    .join("anda_bot")
                    .join("browser-screenshots")
            })
    }

    fn workspace_root(&self) -> Option<&Path> {
        self.screenshot_workspace.as_deref().map(PathBuf::as_path)
    }
}

impl ChromeBrowserToolKind {
    fn name(self) -> &'static str {
        match self {
            Self::Tabs => ChromeBrowserTool::TABS_NAME,
            Self::Page => ChromeBrowserTool::PAGE_NAME,
            Self::Input => ChromeBrowserTool::INPUT_NAME,
            Self::Script => ChromeBrowserTool::SCRIPT_NAME,
        }
    }

    fn description(self) -> String {
        let body = match self {
            Self::Tabs => concat!(
                "Manage Chrome tabs, local files, navigation, and downloads through the Anda browser extension. ",
                "Use list_tabs or get_current_tab to inspect tabs, switch_tab before using page/input/script tools on another tab, ",
                "and open_tab, open_file, close_tab, navigate, go_back, go_forward, reload, download, list_downloads, cancel_download, or open_download as needed. ",
                "Navigation and page-changing actions wait until the resulting page is usable before returning. Inspect page_ready in the action result instead of issuing a separate navigation wait."
            ),
            Self::Page => concat!(
                "Inspect the active Chrome tab through the Anda browser extension. ",
                "This tool intentionally targets the active tab; use chrome_tabs.switch_tab first if another tab is needed. ",
                "Use snapshot, extract_text, screenshot, print_to_pdf, read_selection, get_full_page_html, get_structured_data, get_element_info, get_accessibility_tree, get_viewport_size, find_in_page, wait_for_element, annotate_viewport, clear_annotations, or handle_dialog."
            ),
            Self::Input => concat!(
                "Interact with the active Chrome tab through the Anda browser extension. ",
                "This tool intentionally targets the active tab; use chrome_tabs.switch_tab first to act on another tab. ",
                "Use click, type_text, press_key, scroll, scroll_to, hover, drag_and_drop, select_dropdown, upload_file, or copy_to_clipboard. Native input is preferred by default when available."
            ),
            Self::Script => concat!(
                "Run JavaScript in the active Chrome tab through the Anda browser extension. ",
                "Pass code directly; execute_javascript is the implicit action. Use this only when the smaller page/input tools cannot express the operation, and keep returned data structured and compact. ",
                "Use chrome_tabs.switch_tab first if another tab is needed."
            ),
        };
        body.to_string()
    }
}

impl Tool<BaseCtx> for ChromeBrowserTool {
    type Args = ChromeBrowserToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        self.kind.name().to_string()
    }

    fn description(&self) -> String {
        self.kind.description()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: browser_tool_parameters(self.kind),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        mut args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        validate_browser_action_for_tool(self.kind, &args)?;
        let preferred_session = browser_session_from_meta(ctx.meta());
        let timeout_ms = normalized_action_timeout(args.timeout_ms);

        if args.action == BrowserAction::LaunchBrowser {
            let launch = launch_browser(args.url.as_deref())?;
            let session = self
                .bridge
                .wait_for_connected_session(preferred_session, timeout_ms)
                .await;
            let result = BrowserActionResult {
                ok: true,
                value: json!({
                    "launched": true,
                    "launch": launch,
                    "connected": session.is_some(),
                    "session": session,
                }),
                error: None,
            };
            return Ok(ToolOutput::new(Response::Ok {
                result: json!(result),
                next_cursor: None,
            }));
        }

        if matches!(
            args.action,
            BrowserAction::Screenshot | BrowserAction::PrintToPdf
        ) {
            args.include_data_url = Some(true);
        }

        let session = self
            .connected_session_or_launch(preferred_session, timeout_ms)
            .await?;

        let result = match args.action {
            BrowserAction::OpenFile => self.run_open_file_action(&session, args).await?,
            _ => self.run_browser_action(&session, args).await?,
        };
        Ok(ToolOutput::new(Response::Ok {
            result: json!(result),
            next_cursor: None,
        }))
    }
}

impl ChromeBrowserTool {
    async fn connected_session_or_launch(
        &self,
        preferred_session: Option<String>,
        timeout_ms: u64,
    ) -> Result<String, BoxError> {
        match self.bridge.connected_session(preferred_session.as_deref()) {
            Some(session) => Ok(session),
            None => {
                let _launch = launch_browser(None)?;
                self.bridge
                    .wait_for_connected_session(preferred_session, timeout_ms)
                    .await
                    .ok_or_else(|| "No connected Anda browser extension session. Install and configure the extension, then open the browser.".into())
            }
        }
    }

    async fn run_browser_action(
        &self,
        session: &str,
        args: ChromeBrowserToolArgs,
    ) -> Result<BrowserActionResult, BoxError> {
        let mut result = self.bridge.run_action(session.to_string(), args).await?;
        materialize_screenshot_data_url(&mut result, &self.screenshot_tmp_dir())?;
        Ok(result)
    }

    async fn run_open_file_action(
        &self,
        session: &str,
        args: ChromeBrowserToolArgs,
    ) -> Result<BrowserActionResult, BoxError> {
        let file = self.local_file_from_args(&args)?;
        let file_url = file_url_for_path(&file.path)?;
        let mut open_args = browser_args(BrowserAction::OpenTab);
        open_args.url = Some(file_url.clone());
        open_args.active = args.active;
        open_args.window_id = args.window_id;
        open_args.timeout_ms = args.timeout_ms;
        open_args.reason = args.reason;

        let mut result = self.run_browser_action(session, open_args).await?;
        if let Some(value) = result.value.as_object_mut().filter(|_| result.ok) {
            value.insert("opened_file".to_string(), json!(true));
            value.insert("file_path".to_string(), json!(file.path_string));
            value.insert("file_url".to_string(), json!(file_url));
            value.insert("mime_type".to_string(), json!(file.mime_type));
        }
        Ok(result)
    }

    fn local_file_from_args(
        &self,
        args: &ChromeBrowserToolArgs,
    ) -> Result<LocalBrowserFile, BoxError> {
        let reference = args
            .path
            .as_deref()
            .or(args.url.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("chrome_browser local file actions require path or file:// url")?;
        let path = local_path_from_reference(reference, self.workspace_root())?;
        if !path.exists() {
            return Err(format!("local browser file does not exist: {}", path.display()).into());
        }
        let path = path.canonicalize()?;
        let mime_type = browser_file_mime_type(&path);
        let path_string = path.to_string_lossy().to_string();
        Ok(LocalBrowserFile {
            path,
            path_string,
            mime_type,
        })
    }
}

fn browser_tool_parameters(kind: ChromeBrowserToolKind) -> Value {
    match kind {
        ChromeBrowserToolKind::Tabs => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get_current_tab", "list_tabs", "switch_tab", "open_tab", "open_file", "close_tab", "navigate", "get_frames", "go_back", "go_forward", "reload", "launch_browser", "download", "list_downloads", "cancel_download", "open_download"],
                    "description": "Tab, local-file, navigation, frame, and download action. navigate, open_tab, open_file, reload, go_back, go_forward, and page-changing input/script actions wait for page readiness and include page_ready in the result."
                },
                "url": {
                    "type": ["string", "null"],
                    "description": "URL for navigate, open_tab, launch_browser, download, or open_file."
                },
                "path": {
                    "type": ["string", "null"],
                    "description": "Local filesystem path for open_file. Relative paths are resolved against the workspace."
                },
                "tab_id": {
                    "type": ["integer", "null"],
                    "description": "Chrome tab id. Required for switch_tab, close_tab, and get_frames on another tab."
                },
                "window_id": {
                    "type": ["integer", "null"],
                    "description": "Chrome window id for list_tabs filtering or open_tab placement."
                },
                "active": {
                    "type": ["boolean", "null"],
                    "description": "Whether open_tab or navigate should activate the tab. Defaults to true."
                },
                "bypass_cache": {
                    "type": ["boolean", "null"],
                    "description": "Whether reload should bypass cache."
                },
                "filename": {
                    "type": ["string", "null"],
                    "description": "Suggested relative filename for download."
                },
                "save_as": {
                    "type": ["boolean", "null"],
                    "description": "Whether download should show Chrome's Save As dialog."
                },
                "download_id": {
                    "type": ["integer", "null"],
                    "description": "Chrome download id for cancel_download or open_download."
                },
                "amount": {
                    "type": ["integer", "null"],
                    "description": "Maximum downloads to list. Defaults to 50."
                },
                "value": {
                    "type": ["string", "null"],
                    "description": "Optional download state filter for list_downloads."
                },
                "timeout_ms": timeout_schema()
            },
            "required": ["action", "url", "path", "tab_id", "window_id", "active", "bypass_cache", "filename", "save_as", "download_id", "amount", "value", "timeout_ms"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Page => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["snapshot", "extract_text", "screenshot", "read_selection", "get_full_page_html", "get_structured_data", "get_accessibility_tree", "print_to_pdf", "annotate_viewport", "clear_annotations", "get_element_info", "get_viewport_size", "find_in_page", "wait_for_element", "handle_dialog"],
                    "description": "Inspection, capture, annotation, and dialog action for the active tab. Use chrome_tabs.switch_tab first to inspect another tab."
                },
                "selector": {
                    "type": ["string", "null"],
                    "description": "CSS selector for extract_text, get_element_info, wait_for_element, or element screenshot. Omit for whole-page actions. Open shadow roots are searched when possible."
                },
                "query": {
                    "type": ["string", "null"],
                    "description": "Search query for find_in_page."
                },
                "include_links": {
                    "type": ["boolean", "null"],
                    "description": "Whether snapshot should include visible links."
                },
                "include_forms": {
                    "type": ["boolean", "null"],
                    "description": "Whether snapshot should include visible form controls and buttons."
                },
                "highlight": {
                    "type": ["boolean", "null"],
                    "description": "Whether find_in_page should visibly highlight matched elements."
                },
                "full_page": {
                    "type": ["boolean", "null"],
                    "description": "Whether screenshot should capture the full scrollable page instead of just the viewport."
                },
                "viewport_width": viewport_dimension_schema("Viewport width in CSS pixels for screenshot capture. Use with viewport_height."),
                "viewport_height": viewport_dimension_schema("Viewport height in CSS pixels for screenshot capture. Use with viewport_width."),
                "device_scale_factor": device_scale_factor_schema(),
                "amount": {
                    "type": ["integer", "null"],
                    "description": "Maximum accessibility tree nodes to return for get_accessibility_tree. Defaults to 500."
                },
                "accept": {
                    "type": ["boolean", "null"],
                    "description": "Whether handle_dialog should accept the current JavaScript dialog. Defaults to true."
                },
                "prompt_text": {
                    "type": ["string", "null"],
                    "description": "Prompt text to submit when handle_dialog accepts a prompt dialog."
                },
                "max_chars": {
                    "type": ["integer", "null"],
                    "description": "Maximum characters returned for HTML/text-heavy actions."
                },
                "timeout_ms": timeout_schema()
            },
            "required": ["action", "selector", "query", "include_links", "include_forms", "highlight", "full_page", "viewport_width", "viewport_height", "device_scale_factor", "amount", "accept", "prompt_text", "max_chars", "timeout_ms"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Input => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["click", "type_text", "press_key", "scroll", "scroll_to", "hover", "drag_and_drop", "select_dropdown", "upload_file", "copy_to_clipboard"],
                    "description": "Input action for the active tab. Use chrome_tabs.switch_tab first to act on another tab."
                },
                "selector": {
                    "type": ["string", "null"],
                    "description": "CSS selector for click, type_text, scroll_to, hover, select_dropdown, or upload_file. type_text may omit selector when the active element is editable. Open shadow roots and same-origin frames are searched when possible."
                },
                "text": {
                    "type": ["string", "null"],
                    "description": "Text for type_text or copy_to_clipboard."
                },
                "value": {
                    "type": ["string", "null"],
                    "description": "Option value or label for select_dropdown."
                },
                "key": {
                    "type": ["string", "null"],
                    "description": "Keyboard key for press_key, such as Enter, Escape, ArrowDown, or Tab."
                },
                "amount": {
                    "type": ["integer", "null"],
                    "description": "Vertical scroll amount in pixels for scroll. Positive scrolls down, negative scrolls up."
                },
                "x": coordinate_schema("Viewport x coordinate for click or hover when selector is omitted, or document x scroll coordinate for scroll_to."),
                "y": coordinate_schema("Viewport y coordinate for click or hover when selector is omitted, or document y scroll coordinate for scroll_to."),
                "from_selector": {
                    "type": ["string", "null"],
                    "description": "Source CSS selector for drag_and_drop."
                },
                "to_selector": {
                    "type": ["string", "null"],
                    "description": "Target CSS selector for drag_and_drop."
                },
                "to_x": coordinate_schema("Target viewport x coordinate for drag_and_drop when to_selector is omitted."),
                "to_y": coordinate_schema("Target viewport y coordinate for drag_and_drop when to_selector is omitted."),
                "behavior": {
                    "type": ["string", "null"],
                    "enum": ["auto", "smooth", "instant", null],
                    "description": "Scroll behavior for scroll_to when using a selector or x/y coordinates."
                },
                "files": {
                    "type": ["array", "null"],
                    "items": { "type": "string" },
                    "description": "Absolute local file paths for upload_file."
                },
                "timeout_ms": timeout_schema()
            },
            "required": ["action", "selector", "text", "value", "key", "amount", "x", "y", "from_selector", "to_selector", "to_x", "to_y", "behavior", "files", "timeout_ms"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Script => json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "JavaScript expression or function body to execute. Bare expressions like document.title return automatically; for multi-statement code, an explicit return or final expression returns data. Keep returned data compact and serializable."
                },
                "timeout_ms": timeout_schema()
            },
            "required": ["code", "timeout_ms"],
            "additionalProperties": false
        }),
    }
}

fn timeout_schema() -> Value {
    json!({
        "type": ["integer", "null"],
        "description": "Optional action timeout in milliseconds, clamped between 1000 and 120000."
    })
}

fn coordinate_schema(description: &str) -> Value {
    json!({
        "type": ["number", "null"],
        "description": description
    })
}

fn viewport_dimension_schema(description: &str) -> Value {
    json!({
        "type": ["integer", "null"],
        "minimum": 1,
        "maximum": 10000,
        "description": description
    })
}

fn device_scale_factor_schema() -> Value {
    json!({
        "type": ["number", "null"],
        "minimum": 0.1,
        "maximum": 5.0,
        "description": "Device scale factor for screenshot capture. Defaults to the current page scale."
    })
}

pub fn browser_session_from_meta(meta: &RequestMeta) -> Option<String> {
    request_meta_extra_as::<String>(meta, "source")
        .filter(|source| source.starts_with("browser:"))
        .and_then(|source| normalize_session(source).ok())
}

fn normalize_session(session: String) -> Result<String, BoxError> {
    let session = session.trim();
    if session.is_empty() {
        return Err("browser session cannot be empty".into());
    }
    if session.len() > 256 {
        return Err("browser session is too long".into());
    }
    Ok(session.to_string())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_browser_action_for_tool(
    kind: ChromeBrowserToolKind,
    args: &ChromeBrowserToolArgs,
) -> Result<(), BoxError> {
    if !tool_supports_action(kind, &args.action) {
        return Err(format!(
            "browser action {:?} is not supported by {}",
            args.action,
            kind.name()
        )
        .into());
    }

    validate_viewport_options(args)?;

    match args.action {
        BrowserAction::Click | BrowserAction::Hover => require_selector_or_coordinates(args),
        BrowserAction::TypeText => require_field(&args.text, "text", "type_text"),
        BrowserAction::PressKey => require_field(&args.key, "key", "press_key"),
        BrowserAction::Navigate => require_field(&args.url, "url", "navigate"),
        BrowserAction::OpenFile => require_path_or_url(args),
        BrowserAction::Download => require_field(&args.url, "url", "download"),
        BrowserAction::SwitchTab => require_i64(&args.tab_id, "tab_id", "switch_tab"),
        BrowserAction::CloseTab => require_i64(&args.tab_id, "tab_id", "close_tab"),
        BrowserAction::CancelDownload => {
            require_i64(&args.download_id, "download_id", "cancel_download")
        }
        BrowserAction::OpenDownload => {
            require_i64(&args.download_id, "download_id", "open_download")
        }
        BrowserAction::GetElementInfo => {
            require_field(&args.selector, "selector", "get_element_info")
        }
        BrowserAction::WaitForElement => {
            require_field(&args.selector, "selector", "wait_for_element")
        }
        BrowserAction::ScrollTo => require_selector_or_coordinates(args),
        BrowserAction::DragAndDrop => {
            require_field(&args.from_selector, "from_selector", "drag_and_drop")?;
            if args
                .to_selector
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
                || (args.to_x.is_some() && args.to_y.is_some())
            {
                Ok(())
            } else {
                Err(
                    "chrome_browser action \"drag_and_drop\" requires to_selector or to_x/to_y"
                        .into(),
                )
            }
        }
        BrowserAction::SelectDropdown => {
            require_field(&args.selector, "selector", "select_dropdown")?;
            require_field(&args.value, "value", "select_dropdown")
        }
        BrowserAction::UploadFile => {
            require_field(&args.selector, "selector", "upload_file")?;
            require_files(&args.files, "upload_file")
        }
        BrowserAction::FindInPage => require_field(&args.query, "query", "find_in_page"),
        BrowserAction::CopyToClipboard => require_field(&args.text, "text", "copy_to_clipboard"),
        BrowserAction::SetCookie => {
            require_field(&args.name, "name", "set_cookie")?;
            require_present(&args.value, "value", "set_cookie")?;
            validate_same_site(&args.same_site)
        }
        BrowserAction::DeleteCookie => require_field(&args.name, "name", "delete_cookie"),
        BrowserAction::ExecuteJavascript => {
            require_field(&args.code, "code", "execute_javascript")?;
            validate_script_world(&args.world)
        }
        BrowserAction::GetCurrentTab
        | BrowserAction::Snapshot
        | BrowserAction::ExtractText
        | BrowserAction::GetFullPageHtml
        | BrowserAction::GetStructuredData
        | BrowserAction::GetViewportSize
        | BrowserAction::GetAccessibilityTree
        | BrowserAction::Scroll
        | BrowserAction::Screenshot
        | BrowserAction::PrintToPdf
        | BrowserAction::AnnotateViewport
        | BrowserAction::ClearAnnotations
        | BrowserAction::ReadSelection
        | BrowserAction::ListDownloads
        | BrowserAction::GetCookies
        | BrowserAction::ClearBrowserCache
        | BrowserAction::ListTabs
        | BrowserAction::OpenTab
        | BrowserAction::GetFrames
        | BrowserAction::LaunchBrowser
        | BrowserAction::GoBack
        | BrowserAction::GoForward
        | BrowserAction::Reload
        | BrowserAction::HandleDialog => Ok(()),
    }
}

fn tool_supports_action(kind: ChromeBrowserToolKind, action: &BrowserAction) -> bool {
    match kind {
        ChromeBrowserToolKind::Tabs => matches!(
            action,
            BrowserAction::GetCurrentTab
                | BrowserAction::ListTabs
                | BrowserAction::SwitchTab
                | BrowserAction::OpenTab
                | BrowserAction::OpenFile
                | BrowserAction::CloseTab
                | BrowserAction::Navigate
                | BrowserAction::GetFrames
                | BrowserAction::GoBack
                | BrowserAction::GoForward
                | BrowserAction::Reload
                | BrowserAction::Download
                | BrowserAction::ListDownloads
                | BrowserAction::CancelDownload
                | BrowserAction::OpenDownload
                | BrowserAction::GetCookies
                | BrowserAction::SetCookie
                | BrowserAction::DeleteCookie
                | BrowserAction::ClearBrowserCache
                | BrowserAction::LaunchBrowser
        ),
        ChromeBrowserToolKind::Page => matches!(
            action,
            BrowserAction::Snapshot
                | BrowserAction::ExtractText
                | BrowserAction::Screenshot
                | BrowserAction::ReadSelection
                | BrowserAction::GetFullPageHtml
                | BrowserAction::GetStructuredData
                | BrowserAction::GetAccessibilityTree
                | BrowserAction::PrintToPdf
                | BrowserAction::AnnotateViewport
                | BrowserAction::ClearAnnotations
                | BrowserAction::GetElementInfo
                | BrowserAction::GetViewportSize
                | BrowserAction::FindInPage
                | BrowserAction::WaitForElement
                | BrowserAction::HandleDialog
        ),
        ChromeBrowserToolKind::Input => matches!(
            action,
            BrowserAction::Click
                | BrowserAction::TypeText
                | BrowserAction::PressKey
                | BrowserAction::Scroll
                | BrowserAction::ScrollTo
                | BrowserAction::Hover
                | BrowserAction::DragAndDrop
                | BrowserAction::SelectDropdown
                | BrowserAction::UploadFile
                | BrowserAction::CopyToClipboard
        ),
        ChromeBrowserToolKind::Script => matches!(action, BrowserAction::ExecuteJavascript),
    }
}

fn require_field(value: &Option<String>, field: &str, action: &str) -> Result<(), BoxError> {
    if value.as_ref().is_some_and(|value| !value.trim().is_empty()) {
        Ok(())
    } else {
        Err(format!("chrome_browser action {action:?} requires {field}").into())
    }
}

fn require_present(value: &Option<String>, field: &str, action: &str) -> Result<(), BoxError> {
    if value.is_some() {
        Ok(())
    } else {
        Err(format!("chrome_browser action {action:?} requires {field}").into())
    }
}

fn require_files(value: &Option<Vec<String>>, action: &str) -> Result<(), BoxError> {
    if value
        .as_ref()
        .is_some_and(|files| !files.is_empty() && files.iter().all(|file| !file.trim().is_empty()))
    {
        Ok(())
    } else {
        Err(format!("chrome_browser action {action:?} requires files").into())
    }
}

fn require_i64(value: &Option<i64>, field: &str, action: &str) -> Result<(), BoxError> {
    if value.is_some() {
        Ok(())
    } else {
        Err(format!("chrome_browser action {action:?} requires {field}").into())
    }
}

fn require_path_or_url(args: &ChromeBrowserToolArgs) -> Result<(), BoxError> {
    if args
        .path
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
        || args
            .url
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        Ok(())
    } else {
        Err(format!(
            "chrome_browser action {:?} requires path or file:// url",
            args.action
        )
        .into())
    }
}

fn validate_viewport_options(args: &ChromeBrowserToolArgs) -> Result<(), BoxError> {
    if args.viewport_width.is_some() || args.viewport_height.is_some() {
        let width = args.viewport_width.ok_or(
            "chrome_browser viewport capture requires viewport_width when viewport_height is used",
        )?;
        let height = args.viewport_height.ok_or(
            "chrome_browser viewport capture requires viewport_height when viewport_width is used",
        )?;
        if !(1..=10_000).contains(&width) || !(1..=10_000).contains(&height) {
            return Err("chrome_browser viewport dimensions must be between 1 and 10000".into());
        }
    }
    if let Some(scale) = args.device_scale_factor
        && (!scale.is_finite() || !(0.1..=5.0).contains(&scale))
    {
        return Err("chrome_browser device_scale_factor must be between 0.1 and 5".into());
    }
    Ok(())
}

fn require_selector_or_coordinates(args: &ChromeBrowserToolArgs) -> Result<(), BoxError> {
    if args
        .selector
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
        || (args.x.is_some() && args.y.is_some())
    {
        Ok(())
    } else {
        Err(format!(
            "chrome_browser action {:?} requires selector or x/y coordinates",
            args.action
        )
        .into())
    }
}

fn validate_script_world(value: &Option<String>) -> Result<(), BoxError> {
    let Some(value) = value else {
        return Ok(());
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "debugger" | "isolated" | "main" => Ok(()),
        world => Err(format!(
            "chrome_browser action \"execute_javascript\" has unsupported world {world:?}"
        )
        .into()),
    }
}

fn validate_same_site(value: &Option<String>) -> Result<(), BoxError> {
    let Some(value) = value else {
        return Ok(());
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "no_restriction" | "lax" | "strict" => Ok(()),
        same_site => Err(format!(
            "chrome_browser action \"set_cookie\" has unsupported same_site {same_site:?}"
        )
        .into()),
    }
}

fn normalized_action_timeout(timeout_ms: Option<u64>) -> u64 {
    timeout_ms
        .unwrap_or(DEFAULT_BROWSER_ACTION_TIMEOUT_MS)
        .clamp(MIN_BROWSER_ACTION_TIMEOUT_MS, MAX_BROWSER_ACTION_TIMEOUT_MS)
}

#[derive(Debug)]
struct LocalBrowserFile {
    path: PathBuf,
    path_string: String,
    mime_type: String,
}

fn browser_args(action: BrowserAction) -> ChromeBrowserToolArgs {
    ChromeBrowserToolArgs {
        action,
        selector: None,
        text: None,
        value: None,
        code: None,
        world: None,
        use_bridge: None,
        query: None,
        url: None,
        key: None,
        amount: None,
        x: None,
        y: None,
        to_x: None,
        to_y: None,
        from_selector: None,
        to_selector: None,
        tab_id: None,
        window_id: None,
        frame_id: None,
        active: None,
        include_links: None,
        include_forms: None,
        include_data_url: None,
        full_page: None,
        viewport_width: None,
        viewport_height: None,
        device_scale_factor: None,
        highlight: None,
        bypass_cache: None,
        behavior: None,
        filename: None,
        save_as: None,
        download_id: None,
        files: None,
        origins: None,
        domain: None,
        name: None,
        path: None,
        secure: None,
        http_only: None,
        same_site: None,
        expiration_date: None,
        since_ms: None,
        store_id: None,
        accept: None,
        prompt_text: None,
        max_chars: None,
        timeout_ms: None,
        reason: None,
    }
}

fn local_path_from_reference(
    reference: &str,
    workspace: Option<&Path>,
) -> Result<PathBuf, BoxError> {
    let path = if reference
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("file://")
    {
        path_from_file_url(reference)?
    } else {
        PathBuf::from(reference.trim())
    };
    if path.is_absolute() {
        Ok(path)
    } else if let Some(workspace) = workspace {
        Ok(workspace.join(path))
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn file_url_for_path(path: &Path) -> Result<String, BoxError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut path = absolute.to_string_lossy().replace('\\', "/");
    if cfg!(windows) && !path.starts_with('/') {
        path = format!("/{path}");
    }
    Ok(format!("file://{}", percent_encode_url_path(&path)))
}

fn path_from_file_url(url: &str) -> Result<PathBuf, BoxError> {
    let trimmed = url.trim();
    let Some(payload) = trimmed.strip_prefix("file://") else {
        return Err("local file URL must start with file://".into());
    };
    let payload = payload.strip_prefix("localhost/").unwrap_or(payload);
    let decoded = percent_decode_url_path(payload)?;
    #[cfg(windows)]
    {
        let decoded = decoded
            .strip_prefix('/')
            .unwrap_or(&decoded)
            .replace('/', "\\");
        Ok(PathBuf::from(decoded))
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from(decoded))
    }
}

fn percent_encode_url_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(*byte as char)
            }
            byte => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn percent_decode_url_path(path: &str) -> Result<String, BoxError> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("file URL has an incomplete percent escape".into());
            }
            let hi = hex_value(bytes[index + 1])?;
            let lo = hex_value(bytes[index + 2])?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| "file URL path is not valid UTF-8".into())
}

fn hex_value(byte: u8) -> Result<u8, BoxError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("file URL has an invalid percent escape".into()),
    }
}

fn browser_file_mime_type(path: &Path) -> String {
    if path.is_dir() {
        return "inode/directory".to_string();
    }
    file_extension_lower(path)
        .as_deref()
        .and_then(mime_type_for_extension)
        .or_else(|| {
            infer2::get_from_path(path)
                .ok()
                .flatten()
                .map(|kind| kind.mime_type())
        })
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(infer2::get_from_filename)
                .map(|kind| kind.mime_type())
        })
        .unwrap_or("application/octet-stream")
        .to_string()
}

fn mime_type_for_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "md" | "markdown" => Some("text/markdown"),
        "html" | "htm" => Some("text/html"),
        "svg" => Some("image/svg+xml"),
        "css" => Some("text/css"),
        "csv" => Some("text/csv"),
        "json" | "jsonl" => Some("application/json"),
        "js" | "mjs" | "cjs" => Some("text/javascript"),
        "txt" | "text" | "log" | "toml" | "yaml" | "yml" | "xml" | "rs" | "ts" | "tsx" | "jsx"
        | "svelte" | "vue" | "py" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "sh" | "zsh"
        | "fish" | "sql" => Some("text/plain"),
        _ => None,
    }
}

fn file_extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}

fn materialize_screenshot_data_url(
    result: &mut BrowserActionResult,
    screenshot_dir: &Path,
) -> Result<(), BoxError> {
    if !result.ok {
        return Ok(());
    }

    let Some(value) = result.value.as_object_mut() else {
        return Ok(());
    };

    let Some(data_url) = value
        .get("data_url")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
    else {
        return Ok(());
    };

    let (mime_type, encoded) = parse_screenshot_data_url(&data_url)?;
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|_| "invalid screenshot data_url base64 payload")?;
    fs::create_dir_all(screenshot_dir)?;

    let path = screenshot_dir.join(format!(
        "chrome-screenshot-{}.{}",
        Xid::new(),
        screenshot_extension_for_mime(&mime_type)
    ));
    fs::write(&path, &bytes)?;

    let path = path.to_string_lossy().to_string();
    value.remove("data_url");
    value.insert("path".to_string(), json!(path));
    value.insert("file_path".to_string(), json!(path));
    value.insert("file_uri".to_string(), json!(format!("file://{path}")));
    value.insert("mime_type".to_string(), json!(mime_type));
    value.insert("size".to_string(), json!(bytes.len()));
    value.insert("data_url_saved".to_string(), json!(true));
    Ok(())
}

fn parse_screenshot_data_url(data_url: &str) -> Result<(String, &str), BoxError> {
    let Some(payload) = data_url.trim().strip_prefix("data:") else {
        return Err("screenshot data_url must start with data:".into());
    };
    let Some((metadata, encoded)) = payload.split_once(',') else {
        return Err("screenshot data_url is missing a comma separator".into());
    };

    let mut parts = metadata.split(';');
    let mime_type = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("image/png")
        .trim()
        .to_ascii_lowercase();
    if !mime_type.starts_with("image/") && mime_type != "application/pdf" {
        return Err(format!("browser data_url has unsupported MIME type {mime_type:?}").into());
    }
    if !metadata
        .split(';')
        .any(|part| part.trim().eq_ignore_ascii_case("base64"))
    {
        return Err("screenshot data_url must be base64 encoded".into());
    }

    Ok((mime_type, encoded))
}

fn screenshot_extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        _ => "png",
    }
}

fn json_null() -> Value {
    Value::Null
}

fn launch_browser(url: Option<&str>) -> Result<Value, BoxError> {
    let url = url.map(str::trim).filter(|url| !url.is_empty());

    #[cfg(target_os = "macos")]
    {
        let browsers = ["Google Chrome", "Microsoft Edge", "Chromium"];
        let mut last_error = None;
        for browser in browsers {
            let mut command = Command::new("open");
            command.arg("-a").arg(browser);
            if let Some(url) = url {
                command.arg(url);
            }

            match command.status() {
                Ok(status) if status.success() => {
                    return Ok(json!({ "browser": browser, "url": url }));
                }
                Ok(status) => {
                    last_error = Some(format!("{browser} exited with status {status}"));
                }
                Err(err) => {
                    last_error = Some(format!("{browser}: {err}"));
                }
            }
        }
        Err(format!(
            "failed to launch a supported browser: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        )
        .into())
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", "chrome"]);
        if let Some(url) = url {
            command.arg(url);
        }
        command
            .status()
            .map_err(|err| format!("failed to launch Chrome: {err}"))?;
        return Ok(json!({ "browser": "chrome", "url": url }));
    }

    #[cfg(target_os = "linux")]
    {
        let browsers = [
            "google-chrome",
            "google-chrome-stable",
            "chromium-browser",
            "chromium",
            "microsoft-edge",
            "microsoft-edge-stable",
        ];
        let mut last_error = None;
        for browser in browsers {
            let mut command = Command::new(browser);
            if let Some(url) = url {
                command.arg(url);
            }
            match command.spawn() {
                Ok(_child) => return Ok(json!({ "browser": browser, "url": url })),
                Err(err) => last_error = Some(format!("{browser}: {err}")),
            }
        }
        return Err(format!(
            "failed to launch a supported browser: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        )
        .into());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("launch_browser is not supported on this operating system".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;

    fn snapshot_args() -> ChromeBrowserToolArgs {
        let mut args = browser_args(BrowserAction::Snapshot);
        args.timeout_ms = Some(1_000);
        args
    }

    fn schema_has_action(actions: &[Value], action: &str) -> bool {
        actions.iter().any(|value| value.as_str() == Some(action))
    }

    #[test]
    fn browser_session_prefers_explicit_meta() {
        let mut meta = RequestMeta::default();
        meta.extra
            .insert("source".to_string(), "browser:chrome:1".into());
        meta.extra
            .insert("browser_client".to_string(), "chrome_extension".into());

        assert_eq!(
            browser_session_from_meta(&meta).as_deref(),
            Some("browser:chrome:1")
        );
    }

    #[test]
    fn split_page_tool_schema_targets_active_tab() {
        let tool = ChromeBrowserTool::page(Arc::new(BrowserBridge::new()));
        let definition = tool.definition();
        let properties = definition.parameters["properties"].as_object().unwrap();

        assert_eq!(definition.name, ChromeBrowserTool::PAGE_NAME);
        assert!(properties.get("tab_id").is_none());
        assert!(properties.get("selector").is_some());
    }

    #[test]
    fn script_tool_schema_uses_implicit_action() {
        let tool = ChromeBrowserTool::script(Arc::new(BrowserBridge::new()));
        let definition = tool.definition();
        let properties = definition.parameters["properties"].as_object().unwrap();
        let required = definition.parameters["required"].as_array().unwrap();

        assert_eq!(definition.name, ChromeBrowserTool::SCRIPT_NAME);
        assert!(properties.get("action").is_none());
        assert!(
            !required
                .iter()
                .any(|value| value.as_str() == Some("action"))
        );
        assert!(properties.get("code").is_some());
    }

    #[test]
    fn script_args_default_to_execute_javascript() {
        let args: ChromeBrowserToolArgs = serde_json::from_value(json!({
            "code": "document.title"
        }))
        .unwrap();

        assert_eq!(args.action, BrowserAction::ExecuteJavascript);
        assert_eq!(args.code.as_deref(), Some("document.title"));
    }

    #[test]
    fn browser_tool_schemas_expose_useful_actions_without_state_mutation_defaults() {
        let tabs = ChromeBrowserTool::tabs(Arc::new(BrowserBridge::new())).definition();
        let tabs_properties = tabs.parameters["properties"].as_object().unwrap();
        let tab_actions = tabs_properties["action"]["enum"].as_array().unwrap();
        assert!(schema_has_action(tab_actions, "navigate"));
        assert!(schema_has_action(tab_actions, "open_file"));
        assert!(schema_has_action(tab_actions, "get_frames"));
        assert!(schema_has_action(tab_actions, "list_downloads"));
        assert!(schema_has_action(tab_actions, "open_download"));
        assert!(!schema_has_action(tab_actions, "get_cookies"));
        assert!(!schema_has_action(tab_actions, "clear_browser_cache"));
        assert!(tabs_properties.get("path").is_some());
        assert!(tabs_properties.get("window_id").is_some());
        assert!(tabs_properties.get("bypass_cache").is_some());
        assert!(tabs_properties.get("download_id").is_some());
        assert!(tabs_properties.get("name").is_none());
        assert!(tabs_properties.get("origins").is_none());

        let page = ChromeBrowserTool::page(Arc::new(BrowserBridge::new())).definition();
        let page_properties = page.parameters["properties"].as_object().unwrap();
        let page_actions = page_properties["action"]["enum"].as_array().unwrap();
        assert!(schema_has_action(page_actions, "snapshot"));
        assert!(schema_has_action(page_actions, "screenshot"));
        assert!(schema_has_action(page_actions, "print_to_pdf"));
        assert!(schema_has_action(page_actions, "get_full_page_html"));
        assert!(schema_has_action(page_actions, "get_structured_data"));
        assert!(schema_has_action(page_actions, "get_element_info"));
        assert!(schema_has_action(page_actions, "get_accessibility_tree"));
        assert!(schema_has_action(page_actions, "handle_dialog"));
        assert!(page_properties.get("full_page").is_some());
        assert!(page_properties.get("viewport_width").is_some());
        assert!(page_properties.get("include_forms").is_some());
        assert!(page_properties.get("accept").is_some());

        let input = ChromeBrowserTool::input(Arc::new(BrowserBridge::new())).definition();
        let input_properties = input.parameters["properties"].as_object().unwrap();
        let input_actions = input_properties["action"]["enum"].as_array().unwrap();
        assert!(schema_has_action(input_actions, "click"));
        assert!(schema_has_action(input_actions, "drag_and_drop"));
        assert!(schema_has_action(input_actions, "select_dropdown"));
        assert!(schema_has_action(input_actions, "upload_file"));
        assert!(schema_has_action(input_actions, "copy_to_clipboard"));
        assert!(input_properties.get("to_selector").is_some());
        assert!(input_properties.get("files").is_some());
        assert!(input_properties.get("value").is_some());

        let script = ChromeBrowserTool::script(Arc::new(BrowserBridge::new())).definition();
        let script_properties = script.parameters["properties"].as_object().unwrap();
        assert!(script_properties.get("code").is_some());
        assert!(script_properties.get("world").is_none());
        assert!(script_properties.get("use_bridge").is_none());
        assert!(script_properties.get("frame_id").is_none());
    }

    #[test]
    fn browser_tool_schemas_are_openai_strict() {
        let bridge = Arc::new(BrowserBridge::new());
        let tools = [
            ChromeBrowserTool::tabs(bridge.clone()),
            ChromeBrowserTool::page(bridge.clone()),
            ChromeBrowserTool::input(bridge.clone()),
            ChromeBrowserTool::script(bridge),
        ];

        for tool in tools {
            let definition = tool.definition();
            assert_eq!(definition.strict, Some(true));
            assert_openai_strict_parameters(&definition.parameters);
        }
    }

    #[test]
    fn split_tool_validation_rejects_cross_category_actions() {
        let mut args = snapshot_args();
        args.action = BrowserAction::Click;
        args.selector = Some("button".to_string());

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Input, &args).is_ok());
        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Page, &args).is_err());
    }

    #[test]
    fn input_validation_allows_type_text_active_element() {
        let mut args = snapshot_args();
        args.action = BrowserAction::TypeText;
        args.text = Some("hello".to_string());

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Input, &args).is_ok());
    }

    #[test]
    fn input_validation_allows_scroll_to_coordinates() {
        let mut args = snapshot_args();
        args.action = BrowserAction::ScrollTo;
        args.x = Some(0.0);
        args.y = Some(500.0);

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Input, &args).is_ok());
    }

    #[test]
    fn viewport_validation_allows_device_scale_factor_without_dimensions() {
        let mut args = snapshot_args();
        args.device_scale_factor = Some(1.0);

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Page, &args).is_ok());
    }

    #[test]
    fn viewport_validation_requires_dimensions_as_a_pair() {
        let mut args = snapshot_args();
        args.viewport_width = Some(1280);

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Page, &args).is_err());

        args.viewport_height = Some(720);

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Page, &args).is_ok());
    }

    #[test]
    fn tabs_validation_allows_local_file_actions() {
        let mut args = browser_args(BrowserAction::OpenFile);
        args.path = Some("report.html".to_string());

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Tabs, &args).is_ok());
    }

    #[test]
    fn local_file_url_round_trips_spaces() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("hello world.html");
        fs::write(&path, "<p>hi</p>").unwrap();

        let url = file_url_for_path(&path).unwrap();

        assert!(url.starts_with("file://"));
        assert!(url.contains("hello%20world.html"));
        assert_eq!(path_from_file_url(&url).unwrap(), path);
    }

    #[tokio::test]
    async fn open_file_action_opens_local_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let report_path = temp_dir.path().join("report.html");
        fs::write(&report_path, "<p>hi</p>").unwrap();
        let canonical_report_path = report_path.canonicalize().unwrap();
        let bridge = Arc::new(BrowserBridge::new());
        let (connection_id, sender, mut receiver) = bridge.open_ws_connection();
        bridge
            .register_ws_session(
                connection_id,
                sender,
                "chrome:tab:1".to_string(),
                Some(1),
                None,
                None,
            )
            .unwrap();
        let tool = ChromeBrowserTool::tabs(bridge.clone())
            .with_screenshot_workspace(temp_dir.path().to_path_buf());
        let mut args = browser_args(BrowserAction::OpenFile);
        args.path = Some("report.html".to_string());
        args.timeout_ms = Some(1_000);

        let action = tokio::spawn(async move {
            tool.run_open_file_action("chrome:tab:1", args)
                .await
                .unwrap()
        });

        let open_command = receiver.recv().await.unwrap();
        assert_eq!(open_command.args.action, BrowserAction::OpenTab);
        let file_url = open_command.args.url.as_deref().unwrap();
        assert_eq!(path_from_file_url(file_url).unwrap(), canonical_report_path);
        bridge
            .complete(
                "chrome:tab:1".to_string(),
                open_command.request_id,
                BrowserActionResult {
                    ok: true,
                    value: json!({
                        "opened": true,
                        "tab": { "id": 77, "url": file_url },
                        "page_ready": { "loaded": true }
                    }),
                    error: None,
                },
            )
            .await
            .unwrap();

        let result = action.await.unwrap();
        assert!(result.ok);
        assert_eq!(result.value["opened_file"], true);
        assert_eq!(
            result.value["file_path"].as_str().unwrap(),
            canonical_report_path.to_string_lossy()
        );
        assert_eq!(result.value["file_url"].as_str().unwrap(), file_url);
        assert_eq!(result.value["mime_type"], "text/html");
    }

    #[test]
    fn screenshot_data_url_is_saved_to_tmp_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut result = BrowserActionResult {
            ok: true,
            value: json!({
                "captured": true,
                "mime_type": "image/png",
                "size": 42,
                "data_url": "data:image/png;base64,aW1hZ2UtYnl0ZXM="
            }),
            error: None,
        };

        materialize_screenshot_data_url(&mut result, temp_dir.path()).unwrap();

        let value = result.value.as_object().unwrap();
        assert!(value.get("data_url").is_none());
        assert_eq!(value["data_url_saved"], true);
        assert_eq!(value["mime_type"], "image/png");
        assert_eq!(value["size"], 11);

        let path = value["path"].as_str().unwrap();
        assert_eq!(value["file_path"], path);
        assert_eq!(value["file_uri"], format!("file://{path}"));
        assert!(Path::new(path).starts_with(temp_dir.path()));
        assert_eq!(fs::read(path).unwrap(), b"image-bytes");
    }

    #[tokio::test]
    async fn bridge_sends_browser_request_to_websocket_connection() {
        let bridge = Arc::new(BrowserBridge::new());
        let (connection_id, sender, mut receiver) = bridge.open_ws_connection();
        bridge
            .register_ws_session(
                connection_id,
                sender,
                "chrome:tab:1".to_string(),
                Some(1),
                Some("https://example.com".to_string()),
                Some("Example".to_string()),
            )
            .unwrap();

        let worker_bridge = bridge.clone();
        let action = tokio::spawn(async move {
            worker_bridge
                .run_action("chrome:tab:1".to_string(), snapshot_args())
                .await
                .unwrap()
        });

        let command = receiver
            .recv()
            .await
            .expect("browser command should be sent over WebSocket");
        assert_eq!(command.args.action, BrowserAction::Snapshot);

        bridge
            .complete(
                "chrome:tab:1".to_string(),
                command.request_id,
                BrowserActionResult {
                    ok: true,
                    value: json!({ "title": "Example" }),
                    error: None,
                },
            )
            .await
            .unwrap();

        let result = action.await.unwrap();
        assert!(result.ok);
        assert_eq!(result.value["title"], "Example");
    }
}
