use anda_core::{
    BoxError, FunctionDefinition, RequestMeta, Resource, StateFeatures, Tool, ToolOutput,
};
use anda_engine::{context::BaseCtx, unix_ms};
use anda_kip::Response;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
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

const DEFAULT_BROWSER_ACTION_TIMEOUT_MS: u64 = 60_000;
const MIN_BROWSER_ACTION_TIMEOUT_MS: u64 = 1_000;
const MAX_BROWSER_ACTION_TIMEOUT_MS: u64 = 120_000;

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
    Navigate,
    GoBack,
    GoForward,
    Reload,
    Screenshot,
    ReadSelection,
    ListTabs,
    SwitchTab,
    OpenTab,
    CloseTab,
    LaunchBrowser,
    ExecuteJavascript,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChromeBrowserToolArgs {
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
    pub highlight: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass_cache: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeBrowserToolKind {
    Legacy,
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
        validate_browser_action(&args)?;
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
    pub const NAME: &'static str = "chrome_browser";
    pub const TABS_NAME: &'static str = "chrome_tabs";
    pub const PAGE_NAME: &'static str = "chrome_page";
    pub const INPUT_NAME: &'static str = "chrome_input";
    pub const SCRIPT_NAME: &'static str = "chrome_script";

    pub fn new(bridge: Arc<BrowserBridge>) -> Self {
        Self {
            bridge,
            kind: ChromeBrowserToolKind::Legacy,
        }
    }

    pub fn tabs(bridge: Arc<BrowserBridge>) -> Self {
        Self {
            bridge,
            kind: ChromeBrowserToolKind::Tabs,
        }
    }

    pub fn page(bridge: Arc<BrowserBridge>) -> Self {
        Self {
            bridge,
            kind: ChromeBrowserToolKind::Page,
        }
    }

    pub fn input(bridge: Arc<BrowserBridge>) -> Self {
        Self {
            bridge,
            kind: ChromeBrowserToolKind::Input,
        }
    }

    pub fn script(bridge: Arc<BrowserBridge>) -> Self {
        Self {
            bridge,
            kind: ChromeBrowserToolKind::Script,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.bridge.sessions.read().is_empty()
    }

    pub fn dependency_tool_names() -> [&'static str; 5] {
        [
            Self::TABS_NAME,
            Self::PAGE_NAME,
            Self::INPUT_NAME,
            Self::SCRIPT_NAME,
            Self::NAME,
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
}

impl ChromeBrowserToolKind {
    fn name(self) -> &'static str {
        match self {
            Self::Legacy => ChromeBrowserTool::NAME,
            Self::Tabs => ChromeBrowserTool::TABS_NAME,
            Self::Page => ChromeBrowserTool::PAGE_NAME,
            Self::Input => ChromeBrowserTool::INPUT_NAME,
            Self::Script => ChromeBrowserTool::SCRIPT_NAME,
        }
    }

    fn description(self, sessions: Vec<BrowserSession>) -> String {
        let active_hint = format!("\n\nActive sessions: {sessions:?}");
        let body = match self {
            Self::Legacy => concat!(
                "Legacy browser control tool. Prefer chrome_tabs, chrome_page, chrome_input, ",
                "and chrome_script because their schemas are smaller and page actions target the ",
                "active tab by default. Use this only for compatibility with older prompts."
            ),
            Self::Tabs => concat!(
                "Manage Chrome tabs and navigation through the Anda browser extension. ",
                "Use list_tabs or get_current_tab to inspect tabs, switch_tab before using page/input/script tools on another tab, ",
                "and open_tab, close_tab, navigate, go_back, go_forward, reload, or launch_browser as needed."
            ),
            Self::Page => concat!(
                "Inspect the active Chrome tab through the Anda browser extension. ",
                "This tool intentionally targets the active tab; use chrome_tabs.switch_tab first if another tab is needed. ",
                "Use snapshot, extract_text, screenshot, read_selection, get_full_page_html, get_structured_data, ",
                "get_element_info, get_viewport_size, find_in_page, or wait_for_element."
            ),
            Self::Input => concat!(
                "Interact with the active Chrome tab through the Anda browser extension. ",
                "This tool intentionally targets the active tab; use chrome_tabs.switch_tab first if another tab is needed. ",
                "Use click, type_text, press_key, scroll, scroll_to, hover, drag_and_drop, select_dropdown, or copy_to_clipboard."
            ),
            Self::Script => concat!(
                "Run JavaScript in the active Chrome tab through the Anda browser extension. ",
                "Use this only when the smaller page/input tools cannot express the operation, and keep returned data structured and compact. ",
                "Use chrome_tabs.switch_tab first if another tab is needed."
            ),
        };
        format!("{body}{active_hint}")
    }
}

impl Tool<BaseCtx> for ChromeBrowserTool {
    type Args = ChromeBrowserToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        self.kind.name().to_string()
    }

    fn description(&self) -> String {
        self.kind.description(self.bridge.sessions())
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
        args: Self::Args,
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

        let session = match self.bridge.connected_session(preferred_session.as_deref()) {
            Some(session) => session,
            None => {
                let _launch = launch_browser(None)?;
                self.bridge
                    .wait_for_connected_session(preferred_session, timeout_ms)
                    .await
                    .ok_or("No connected Anda browser extension session. Install and configure the extension, then open the browser.")?
            }
        };
        let result = self.bridge.run_action(session, args).await?;
        Ok(ToolOutput::new(Response::Ok {
            result: json!(result),
            next_cursor: None,
        }))
    }
}

fn browser_tool_parameters(kind: ChromeBrowserToolKind) -> Value {
    match kind {
        ChromeBrowserToolKind::Legacy => json!({
            "type": "object",
            "properties": legacy_browser_properties(),
            "required": ["action"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Tabs => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get_current_tab", "list_tabs", "switch_tab", "open_tab", "close_tab", "navigate", "go_back", "go_forward", "reload", "launch_browser"],
                    "description": "Tab/navigation action. Use switch_tab before chrome_page, chrome_input, or chrome_script when another tab is needed."
                },
                "url": {
                    "type": ["string", "null"],
                    "description": "URL for navigate, open_tab, or launch_browser. navigate targets the active tab unless the legacy chrome_browser tool is used."
                },
                "tab_id": {
                    "type": ["integer", "null"],
                    "description": "Chrome tab id. Required for switch_tab and close_tab."
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
                "timeout_ms": timeout_schema(),
                "reason": reason_schema()
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Page => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["snapshot", "extract_text", "screenshot", "read_selection", "get_full_page_html", "get_structured_data", "get_element_info", "get_viewport_size", "find_in_page", "wait_for_element"],
                    "description": "Inspection action for the active tab. Use chrome_tabs.switch_tab first to inspect another tab."
                },
                "selector": {
                    "type": ["string", "null"],
                    "description": "CSS selector for extract_text, get_element_info, or wait_for_element. Open shadow roots are searched when possible."
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
                "include_data_url": {
                    "type": ["boolean", "null"],
                    "description": "Whether screenshot should include the captured PNG data URL. Leave false unless image bytes are needed."
                },
                "highlight": {
                    "type": ["boolean", "null"],
                    "description": "Whether find_in_page should visibly highlight matched elements."
                },
                "max_chars": {
                    "type": ["integer", "null"],
                    "description": "Maximum characters returned for HTML/text-heavy actions."
                },
                "timeout_ms": timeout_schema(),
                "reason": reason_schema()
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Input => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["click", "type_text", "press_key", "scroll", "scroll_to", "hover", "drag_and_drop", "select_dropdown", "copy_to_clipboard"],
                    "description": "Input action for the active tab. Use chrome_tabs.switch_tab first to act on another tab."
                },
                "selector": {
                    "type": ["string", "null"],
                    "description": "CSS selector for click, type_text, scroll_to, hover, or select_dropdown. Open shadow roots are searched when possible."
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
                "x": coordinate_schema("Viewport x coordinate for click or hover when selector is omitted."),
                "y": coordinate_schema("Viewport y coordinate for click or hover when selector is omitted."),
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
                    "description": "Scroll behavior for scroll_to."
                },
                "timeout_ms": timeout_schema(),
                "reason": reason_schema()
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        ChromeBrowserToolKind::Script => json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["execute_javascript"],
                    "description": "Execute JavaScript in the active tab and return structured data."
                },
                "code": {
                    "type": "string",
                    "description": "JavaScript expression or function body to execute. Bare expressions like document.title return automatically; multi-statement code should use return. Keep returned data compact and serializable."
                },
                "world": {
                    "type": ["string", "null"],
                    "enum": ["debugger", "isolated", "main", null],
                    "description": "Execution mode. debugger uses Chrome DevTools Runtime.evaluate and is the default CSP-resistant page-context bridge. isolated or main use chrome.scripting.executeScript when use_bridge is false."
                },
                "use_bridge": {
                    "type": ["boolean", "null"],
                    "description": "Whether to use the CSP-resistant debugger bridge. Defaults to true. Set false to force chrome.scripting.executeScript with world isolated or main."
                },
                "frame_id": {
                    "type": ["integer", "null"],
                    "description": "Optional frame id for non-bridge chrome.scripting.executeScript mode. Omit when using the default debugger bridge."
                },
                "timeout_ms": timeout_schema(),
                "reason": reason_schema()
            },
            "required": ["action", "code"],
            "additionalProperties": false
        }),
    }
}

fn legacy_browser_properties() -> Value {
    json!({
        "action": {
            "type": "string",
            "enum": ["get_current_tab", "snapshot", "extract_text", "get_full_page_html", "get_structured_data", "get_element_info", "get_viewport_size", "wait_for_element", "click", "type_text", "press_key", "scroll", "scroll_to", "hover", "drag_and_drop", "select_dropdown", "find_in_page", "copy_to_clipboard", "navigate", "go_back", "go_forward", "reload", "screenshot", "read_selection", "list_tabs", "switch_tab", "open_tab", "close_tab", "launch_browser", "execute_javascript"],
            "description": "Legacy browser action. Prefer the split tools: chrome_tabs, chrome_page, chrome_input, chrome_script."
        },
        "selector": { "type": ["string", "null"], "description": "CSS selector for page/input actions." },
        "text": { "type": ["string", "null"], "description": "Text for type_text or copy_to_clipboard." },
        "value": { "type": ["string", "null"], "description": "Value for select_dropdown." },
        "code": { "type": ["string", "null"], "description": "JavaScript expression or function body for execute_javascript." },
        "world": { "type": ["string", "null"], "enum": ["debugger", "isolated", "main", null], "description": "Execution mode for execute_javascript. debugger is the default CSP-resistant page-context bridge; isolated/main use chrome.scripting.executeScript when use_bridge is false." },
        "use_bridge": { "type": ["boolean", "null"], "description": "Whether execute_javascript should use the CSP-resistant debugger bridge. Defaults to true." },
        "query": { "type": ["string", "null"], "description": "Search query for find_in_page." },
        "url": { "type": ["string", "null"], "description": "URL for navigate, open_tab, or launch_browser." },
        "key": { "type": ["string", "null"], "description": "Keyboard key for press_key." },
        "amount": { "type": ["integer", "null"], "description": "Vertical scroll amount in pixels." },
        "x": coordinate_schema("Viewport x coordinate."),
        "y": coordinate_schema("Viewport y coordinate."),
        "to_x": coordinate_schema("Target viewport x coordinate."),
        "to_y": coordinate_schema("Target viewport y coordinate."),
        "from_selector": { "type": ["string", "null"], "description": "Source selector for drag_and_drop." },
        "to_selector": { "type": ["string", "null"], "description": "Target selector for drag_and_drop." },
        "tab_id": { "type": ["integer", "null"], "description": "Chrome tab id. For page/input actions the extension activates this tab before acting." },
        "window_id": { "type": ["integer", "null"], "description": "Chrome window id." },
        "frame_id": { "type": ["integer", "null"], "description": "Optional frame id for execute_javascript." },
        "active": { "type": ["boolean", "null"], "description": "Whether open_tab or navigate should activate the target tab." },
        "include_links": { "type": ["boolean", "null"], "description": "Whether snapshot should include visible links." },
        "include_forms": { "type": ["boolean", "null"], "description": "Whether snapshot should include visible form controls and buttons." },
        "include_data_url": { "type": ["boolean", "null"], "description": "Whether screenshot should include the captured PNG data URL." },
        "highlight": { "type": ["boolean", "null"], "description": "Whether find_in_page should highlight matched elements." },
        "bypass_cache": { "type": ["boolean", "null"], "description": "Whether reload should bypass cache." },
        "behavior": { "type": ["string", "null"], "enum": ["auto", "smooth", "instant", null], "description": "Scroll behavior for scroll_to." },
        "max_chars": { "type": ["integer", "null"], "description": "Maximum characters returned for HTML/text-heavy actions." },
        "timeout_ms": timeout_schema(),
        "reason": reason_schema()
    })
}

fn timeout_schema() -> Value {
    json!({
        "type": ["integer", "null"],
        "description": "Optional action timeout in milliseconds, clamped between 1000 and 120000."
    })
}

fn reason_schema() -> Value {
    json!({
        "type": ["string", "null"],
        "description": "Brief reason for this browser action, useful for audit logs in the extension."
    })
}

fn coordinate_schema(description: &str) -> Value {
    json!({
        "type": ["number", "null"],
        "description": description
    })
}

pub fn browser_session_from_meta(meta: &RequestMeta) -> Option<String> {
    meta.get_extra_as::<String>("source")
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

fn validate_browser_action(args: &ChromeBrowserToolArgs) -> Result<(), BoxError> {
    validate_browser_action_for_tool(ChromeBrowserToolKind::Legacy, args)
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

    match args.action {
        BrowserAction::Click | BrowserAction::Hover => require_selector_or_coordinates(args),
        BrowserAction::TypeText => {
            require_field(&args.selector, "selector", "type_text")?;
            require_field(&args.text, "text", "type_text")
        }
        BrowserAction::PressKey => require_field(&args.key, "key", "press_key"),
        BrowserAction::Navigate => require_field(&args.url, "url", "navigate"),
        BrowserAction::SwitchTab => require_i64(&args.tab_id, "tab_id", "switch_tab"),
        BrowserAction::CloseTab => require_i64(&args.tab_id, "tab_id", "close_tab"),
        BrowserAction::GetElementInfo => {
            require_field(&args.selector, "selector", "get_element_info")
        }
        BrowserAction::WaitForElement => {
            require_field(&args.selector, "selector", "wait_for_element")
        }
        BrowserAction::ScrollTo => require_field(&args.selector, "selector", "scroll_to"),
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
        BrowserAction::FindInPage => require_field(&args.query, "query", "find_in_page"),
        BrowserAction::CopyToClipboard => require_field(&args.text, "text", "copy_to_clipboard"),
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
        | BrowserAction::Scroll
        | BrowserAction::Screenshot
        | BrowserAction::ReadSelection
        | BrowserAction::ListTabs
        | BrowserAction::OpenTab
        | BrowserAction::LaunchBrowser
        | BrowserAction::GoBack
        | BrowserAction::GoForward
        | BrowserAction::Reload => Ok(()),
    }
}

fn tool_supports_action(kind: ChromeBrowserToolKind, action: &BrowserAction) -> bool {
    match kind {
        ChromeBrowserToolKind::Legacy => true,
        ChromeBrowserToolKind::Tabs => matches!(
            action,
            BrowserAction::GetCurrentTab
                | BrowserAction::ListTabs
                | BrowserAction::SwitchTab
                | BrowserAction::OpenTab
                | BrowserAction::CloseTab
                | BrowserAction::Navigate
                | BrowserAction::GoBack
                | BrowserAction::GoForward
                | BrowserAction::Reload
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
                | BrowserAction::GetElementInfo
                | BrowserAction::GetViewportSize
                | BrowserAction::FindInPage
                | BrowserAction::WaitForElement
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

fn require_i64(value: &Option<i64>, field: &str, action: &str) -> Result<(), BoxError> {
    if value.is_some() {
        Ok(())
    } else {
        Err(format!("chrome_browser action {action:?} requires {field}").into())
    }
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

fn normalized_action_timeout(timeout_ms: Option<u64>) -> u64 {
    timeout_ms
        .unwrap_or(DEFAULT_BROWSER_ACTION_TIMEOUT_MS)
        .clamp(MIN_BROWSER_ACTION_TIMEOUT_MS, MAX_BROWSER_ACTION_TIMEOUT_MS)
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

    fn snapshot_args() -> ChromeBrowserToolArgs {
        ChromeBrowserToolArgs {
            action: BrowserAction::Snapshot,
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
            highlight: None,
            bypass_cache: None,
            behavior: None,
            max_chars: None,
            timeout_ms: Some(1_000),
            reason: None,
        }
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
    fn browser_action_validation_requires_action_fields() {
        let mut args = snapshot_args();
        args.action = BrowserAction::Click;

        assert!(validate_browser_action(&args).is_err());

        args.selector = Some("button[type=submit]".to_string());
        assert!(validate_browser_action(&args).is_ok());
    }

    #[test]
    fn browser_action_validation_rejects_unknown_script_world() {
        let mut args = snapshot_args();
        args.action = BrowserAction::ExecuteJavascript;
        args.code = Some("document.title".to_string());

        args.world = Some("debugger".to_string());
        assert!(validate_browser_action(&args).is_ok());

        args.world = Some("main".to_string());
        assert!(validate_browser_action(&args).is_ok());

        args.world = Some("page".to_string());
        assert!(validate_browser_action(&args).is_err());
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
    fn split_tool_validation_rejects_cross_category_actions() {
        let mut args = snapshot_args();
        args.action = BrowserAction::Click;
        args.selector = Some("button".to_string());

        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Input, &args).is_ok());
        assert!(validate_browser_action_for_tool(ChromeBrowserToolKind::Page, &args).is_err());
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
