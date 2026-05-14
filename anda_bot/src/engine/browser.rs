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
const DEFAULT_BROWSER_POLL_TIMEOUT_MS: u64 = 20_000;
const MAX_BROWSER_POLL_TIMEOUT_MS: u64 = 30_000;

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
    command: BrowserCommand,
    picked: bool,
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
    Snapshot,
    ExtractText,
    Click,
    TypeText,
    PressKey,
    Scroll,
    Navigate,
    Screenshot,
    ReadSelection,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChromeBrowserToolArgs {
    pub action: BrowserAction,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_links: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_forms: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_data_url: Option<bool>,

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
}

#[derive(Clone)]
pub struct ChromeBrowserApiTool {
    bridge: Arc<BrowserBridge>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum ChromeBrowserApiArgs {
    Register {
        session: String,
        #[serde(default)]
        tab_id: Option<i64>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        title: Option<String>,
    },
    Poll {
        session: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Complete {
        session: String,
        request_id: u64,
        result: BrowserActionResult,
    },
    GetStatus {
        #[serde(default)]
        session: Option<String>,
    },
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
        Ok(entry.clone())
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
                "Chrome extension session {session:?} is not connected. Open the Anda Chrome side panel on the target tab and try again."
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
                    command: command.clone(),
                    picked: action_sender.is_some(),
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

    pub async fn poll(&self, session: String, timeout_ms: Option<u64>) -> Option<BrowserCommand> {
        let Ok(session) = normalize_session(session) else {
            return None;
        };
        self.touch_session(&session);

        let timeout_ms = timeout_ms
            .unwrap_or(DEFAULT_BROWSER_POLL_TIMEOUT_MS)
            .min(MAX_BROWSER_POLL_TIMEOUT_MS);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            if let Some(command) = self.next_pending_command(&session).await {
                return Some(command);
            }

            let now = Instant::now();
            if now >= deadline {
                return None;
            }

            if tokio::time::timeout(deadline - now, self.notify.notified())
                .await
                .is_err()
            {
                return None;
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

    async fn next_pending_command(&self, session: &str) -> Option<BrowserCommand> {
        let mut pending = self.pending.lock().await;
        for request in pending.values_mut() {
            if request.session == session && !request.picked {
                request.picked = true;
                return Some(request.command.clone());
            }
        }
        None
    }

    fn touch_session(&self, session: &str) {
        if let Some(active_session) = self.sessions.write().get_mut(session) {
            active_session.last_seen_at = unix_ms();
        }
    }
}

impl ChromeBrowserTool {
    pub const NAME: &'static str = "chrome_browser";

    pub fn new(bridge: Arc<BrowserBridge>) -> Self {
        Self { bridge }
    }
}

impl Tool<BaseCtx> for ChromeBrowserTool {
    type Args = ChromeBrowserToolArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        concat!(
            "Controls the user's current Chrome tab through the Anda Chrome extension. ",
            "Use this when the user asks about or wants action on the current webpage. ",
            "Start with snapshot or extract_text to inspect the page, then use click, type_text, press_key, scroll, navigate, screenshot, or read_selection as needed."
        )
        .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["snapshot", "extract_text", "click", "type_text", "press_key", "scroll", "navigate", "screenshot", "read_selection"],
                        "description": "Browser action to perform on the connected Chrome tab."
                    },
                    "selector": {
                        "type": ["string", "null"],
                        "description": "CSS selector for click, type_text, or extract_text. Prefer stable selectors when available."
                    },
                    "text": {
                        "type": ["string", "null"],
                        "description": "Text to enter for type_text."
                    },
                    "url": {
                        "type": ["string", "null"],
                        "description": "URL to open for navigate."
                    },
                    "key": {
                        "type": ["string", "null"],
                        "description": "Keyboard key for press_key, such as Enter, Escape, ArrowDown, or Tab."
                    },
                    "amount": {
                        "type": ["integer", "null"],
                        "description": "Vertical scroll amount in pixels for scroll. Positive scrolls down, negative scrolls up."
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
                    "timeout_ms": {
                        "type": ["integer", "null"],
                        "description": "Optional action timeout in milliseconds, clamped between 1000 and 120000."
                    },
                    "reason": {
                        "type": ["string", "null"],
                        "description": "Brief reason for this browser action, useful for audit logs in the extension."
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let session = browser_session_from_meta(ctx.meta()).ok_or(
            "chrome_browser requires a Chrome extension request context with browser_session metadata",
        )?;
        let result = self.bridge.run_action(session, args).await?;
        Ok(ToolOutput::new(Response::Ok {
            result: json!(result),
            next_cursor: None,
        }))
    }
}

impl ChromeBrowserApiTool {
    pub const NAME: &'static str = "chrome_browser_api";

    pub fn new(bridge: Arc<BrowserBridge>) -> Self {
        Self { bridge }
    }
}

impl Tool<BaseCtx> for ChromeBrowserApiTool {
    type Args = ChromeBrowserApiArgs;
    type Output = Response;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Internal bridge API used by the Anda Chrome extension to register tabs, poll browser actions, and return action results."
            .to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["Register", "Poll", "Complete", "GetStatus"],
                        "description": "Bridge operation type."
                    },
                    "session": {
                        "type": ["string", "null"],
                        "description": "Chrome browser session identifier. Required for Register, Poll, and Complete."
                    },
                    "tab_id": {
                        "type": ["integer", "null"],
                        "description": "Chrome tab id associated with the session."
                    },
                    "url": {
                        "type": ["string", "null"],
                        "description": "Current tab URL."
                    },
                    "title": {
                        "type": ["string", "null"],
                        "description": "Current tab title."
                    },
                    "timeout_ms": {
                        "type": ["integer", "null"],
                        "description": "Long-poll timeout in milliseconds, capped at 30000."
                    },
                    "request_id": {
                        "type": ["integer", "null"],
                        "description": "Browser request id to complete."
                    },
                    "result": {
                        "type": ["object", "null"],
                        "description": "Browser action result for Complete."
                    }
                },
                "required": ["type"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let result = match args {
            ChromeBrowserApiArgs::Register {
                session,
                tab_id,
                url,
                title,
            } => {
                let session = self.bridge.register(session, tab_id, url, title)?;
                json!({ "registered": true, "session": session })
            }
            ChromeBrowserApiArgs::Poll {
                session,
                timeout_ms,
            } => {
                let command = self.bridge.poll(session, timeout_ms).await;
                json!({ "command": command })
            }
            ChromeBrowserApiArgs::Complete {
                session,
                request_id,
                result,
            } => {
                self.bridge.complete(session, request_id, result).await?;
                json!({ "completed": true, "request_id": request_id })
            }
            ChromeBrowserApiArgs::GetStatus { session } => match session {
                Some(session) => {
                    let session = normalize_session(session)?;
                    json!({ "session": self.bridge.session(&session) })
                }
                None => json!({ "sessions": self.bridge.sessions() }),
            },
        };

        Ok(ToolOutput::new(Response::Ok {
            result,
            next_cursor: None,
        }))
    }
}

pub fn browser_session_from_meta(meta: &RequestMeta) -> Option<String> {
    meta.get_extra_as::<String>("browser_session")
        .and_then(|session| normalize_session(session).ok())
        .or_else(|| {
            meta.get_extra_as::<String>("source")
                .filter(|source| source.starts_with("chrome:"))
                .and_then(|source| normalize_session(source).ok())
        })
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
    match args.action {
        BrowserAction::Click => require_field(&args.selector, "selector", "click"),
        BrowserAction::TypeText => {
            require_field(&args.selector, "selector", "type_text")?;
            require_field(&args.text, "text", "type_text")
        }
        BrowserAction::PressKey => require_field(&args.key, "key", "press_key"),
        BrowserAction::Navigate => require_field(&args.url, "url", "navigate"),
        BrowserAction::Snapshot
        | BrowserAction::ExtractText
        | BrowserAction::Scroll
        | BrowserAction::Screenshot
        | BrowserAction::ReadSelection => Ok(()),
    }
}

fn require_field(value: &Option<String>, field: &str, action: &str) -> Result<(), BoxError> {
    if value.as_ref().is_some_and(|value| !value.trim().is_empty()) {
        Ok(())
    } else {
        Err(format!("chrome_browser action {action:?} requires {field}").into())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_args() -> ChromeBrowserToolArgs {
        ChromeBrowserToolArgs {
            action: BrowserAction::Snapshot,
            selector: None,
            text: None,
            url: None,
            key: None,
            amount: None,
            include_links: None,
            include_forms: None,
            include_data_url: None,
            timeout_ms: Some(1_000),
            reason: None,
        }
    }

    #[test]
    fn browser_session_prefers_explicit_meta() {
        let mut meta = RequestMeta::default();
        meta.extra
            .insert("source".to_string(), "chrome:window:1".into());
        meta.extra
            .insert("browser_session".to_string(), "chrome:tab:2".into());

        assert_eq!(
            browser_session_from_meta(&meta).as_deref(),
            Some("chrome:tab:2")
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

    #[tokio::test]
    async fn bridge_polls_and_completes_browser_request() {
        let bridge = Arc::new(BrowserBridge::new());
        bridge
            .register(
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

        let command = bridge
            .poll("chrome:tab:1".to_string(), Some(1_000))
            .await
            .expect("browser command should be available");
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
