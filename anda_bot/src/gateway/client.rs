use crate::util::tool_response::ToolResponse;
use anda_core::{
    AgentInput, AgentOutput, BoxError, ByteBufB64, Json, ToolInput, ToolOutput,
    http::{RPCRequestRef, RPCResponse},
};
use anda_engine::memory::{Conversation, ConversationDelta, ConversationStatus};
use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{
    auto_update::AutoUpdateState,
    daemon::{BackgroundDaemon, Daemon, LaunchState},
    engine::{
        AndaBotStatus, ConversationsTool, ConversationsToolArgs, DaemonModelsResponse,
        PromptCommand,
    },
    identity::LocalIdentitySecrets,
    util::{http_client::build_http_client, request_meta::keys},
};

const DAEMON_STARTUP_LOG_TAIL_BYTES: u64 = 64 * 1024;
// First access to a populated KIP 1.x store migrates it before the gateway
// can answer status requests. A child that exits still fails immediately.
const DAEMON_STARTUP_TIMEOUT: Duration = Duration::from_secs(10 * 60);

// Agent runs routinely take minutes (tool loops, model retries), so bound
// them explicitly. Callers that need a quick failure signal, such as the chat
// keepalive ping, pass their own timeout via `agent_run_with_timeout`.
pub const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

// The status endpoint is a loopback health check polled from interactive
// loops (TUI refresh, daemon readiness waits); fail fast instead of letting a
// wedged daemon hold callers for the client's full default timeout.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

// Memory views are read interactively (CLI, TUI): fail while the user waits.
const MEMORY_TIMEOUT: Duration = Duration::from_secs(12);

// Error messages quote this much of a response body. A mismatched daemon can
// answer with a whole conversation snapshot.
const ERROR_BODY_EXCERPT: usize = 1024;

/// Hard cap every conversation child-chain walk shares: a malformed chain
/// (a cycle, or an absurd length) must never turn a polling loop into an
/// unbounded sequence of HTTP requests.
pub const MAX_CONVERSATION_CHAIN: usize = 64;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    auth_token: String,
}

impl Client {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn new(base_url: String, auth_token: String) -> Self {
        Self {
            // The gateway is local: never route it through a proxy.
            http: build_http_client(None, |client| client.no_proxy())
                .expect("failed to build gateway HTTP client"),
            base_url,
            auth_token,
        }
    }

    pub fn rebased(&self, base_url: String) -> Self {
        let mut client = self.clone();
        client.base_url = base_url;
        client
    }

    pub fn brain(&self) -> crate::brain::Client {
        crate::brain::Client::new(
            format!("{}/v1/anda_bot", self.base_url),
            Some(self.auth_token.clone()),
        )
    }

    pub async fn status(&self) -> Result<AndaBotStatus, BoxError> {
        let req = self
            .request(reqwest::Method::GET, "/daemon/status")
            .timeout(STATUS_TIMEOUT);
        self.decode_response(req.send().await?).await
    }

    pub async fn memory_overview(&self) -> Result<crate::brain::product::MemoryOverview, BoxError> {
        let req = self
            .request(reqwest::Method::GET, "/daemon/memory/v1/overview")
            .timeout(MEMORY_TIMEOUT);
        tool_result(self.decode_response(req.send().await?).await?)
    }

    pub async fn memory_setup(
        &self,
        apply: Option<&str>,
    ) -> Result<crate::brain::setup::SetupPreview, BoxError> {
        let envelope: ToolResponse = match apply {
            Some(preview_digest) => {
                self.post_json(
                    "/daemon/memory/v1/inbox/setup/commit",
                    &serde_json::json!({ "preview_digest": preview_digest }),
                )
                .await?
            }
            None => {
                self.post_json(
                    "/daemon/memory/v1/inbox/setup/prepare",
                    &serde_json::json!({}),
                )
                .await?
            }
        };
        tool_result(envelope)
    }

    pub async fn memory_activity(
        &self,
        query: &crate::brain::activity::ActivityQuery,
    ) -> Result<crate::brain::activity::ActivityPage, BoxError> {
        let req = self
            .request(reqwest::Method::GET, "/daemon/memory/v1/activity")
            .query(query)
            .timeout(MEMORY_TIMEOUT);
        match self.decode_response(req.send().await?).await? {
            ToolResponse::Ok {
                result,
                next_cursor,
            } => {
                let mut page: crate::brain::activity::ActivityPage =
                    serde_json::from_value(result)?;
                page.next_cursor = next_cursor;
                Ok(page)
            }
            error => tool_result(error),
        }
    }

    pub async fn auto_update_check(&self) -> Result<AutoUpdateState, BoxError> {
        self.post_json("/auto_update/check", &()).await
    }

    pub async fn reload_models(&self) -> Result<DaemonModelsResponse, BoxError> {
        self.post_json("/daemon/models/reload", &()).await
    }

    /// Register this interactive CLI's launch directory with the daemon.
    /// Only the local owner's bearer token is accepted by this endpoint.
    pub async fn register_cli_workspace(&self, workspace: &Path) -> Result<(), BoxError> {
        let request = serde_json::json!({
            "workspace": workspace.to_string_lossy(),
        });
        let _: Json = self.post_json("/daemon/cli-workspace", &request).await?;
        Ok(())
    }

    /// Runs a CLI prompt after registering its workspace again. The daemon
    /// keeps registrations in memory, so a restart or their 24-hour lifetime
    /// ends them while the CLI stays open. Stop and cancel start no work and
    /// skip it, so a directory that no longer resolves never blocks them.
    pub async fn agent_run_in_cli_workspace(
        &self,
        input: &AgentInput,
    ) -> Result<AgentOutput, BoxError> {
        let control = matches!(
            PromptCommand::from(input.prompt.clone()),
            PromptCommand::Stop { .. } | PromptCommand::Cancel { .. }
        );
        let workspace = input
            .meta
            .as_ref()
            .and_then(|meta| meta.get_extra_as::<PathBuf>(keys::WORKSPACE));
        if let Some(workspace) = workspace
            && !control
        {
            self.register_cli_workspace(&workspace).await?;
        }
        self.agent_run(input).await
    }

    pub async fn chatgpt(&self, request: &crate::chatgpt::api::Request) -> Result<Json, BoxError> {
        self.post_json("/daemon/chatgpt", request).await
    }

    pub async fn shutdown(&self) -> Result<Json, BoxError> {
        self.post_json("/daemon/shutdown", &()).await
    }

    pub async fn agent_run(&self, input: &AgentInput) -> Result<AgentOutput, BoxError> {
        self.agent_run_with_timeout(input, AGENT_RUN_TIMEOUT).await
    }

    pub async fn agent_run_with_timeout(
        &self,
        input: &AgentInput,
        timeout: Duration,
    ) -> Result<AgentOutput, BoxError> {
        self.rpc("agent_run", input, Some(timeout)).await
    }

    pub async fn tool_call<I, O>(&self, input: &ToolInput<I>) -> Result<ToolOutput<O>, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        self.rpc("tool_call", input, None).await
    }

    pub async fn tool_call_with_timeout<I, O>(
        &self,
        input: &ToolInput<I>,
        timeout: Duration,
    ) -> Result<ToolOutput<O>, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        self.rpc("tool_call", input, Some(timeout)).await
    }

    /// Fetch a conversation by id.
    pub async fn get_conversation(&self, conversation_id: u64) -> Result<Conversation, BoxError> {
        self.conversations(
            ConversationsToolArgs::GetConversation {
                _id: conversation_id,
            },
            None,
        )
        .await
    }

    /// Fetch only the messages and artifacts appended after the given offsets.
    pub async fn get_conversation_delta(
        &self,
        conversation_id: u64,
        messages_offset: usize,
        artifacts_offset: usize,
    ) -> Result<ConversationDelta, BoxError> {
        self.conversations(
            ConversationsToolArgs::GetConversationDelta {
                _id: conversation_id,
                messages_offset,
                artifacts_offset,
            },
            None,
        )
        .await
    }

    /// Call the conversations tool, unwrapping the daemon's tool envelope.
    pub(super) async fn conversations<T>(
        &self,
        args: ConversationsToolArgs,
        timeout: Option<Duration>,
    ) -> Result<T, BoxError>
    where
        T: serde::de::DeserializeOwned,
    {
        let input = ToolInput::new(ConversationsTool::NAME.to_string(), args);
        let output: ToolOutput<ToolResponse> = self.rpc("tool_call", &input, timeout).await?;
        tool_result(output.output)
    }

    pub async fn ensure_daemon_running(&self, daemon: &Daemon) -> Result<LaunchState, BoxError> {
        self.ensure_daemon_running_with_identity_secrets(daemon, None)
            .await
    }

    pub async fn ensure_daemon_running_with_identity_secrets(
        &self,
        daemon: &Daemon,
        identity_secrets: Option<&LocalIdentitySecrets>,
    ) -> Result<LaunchState, BoxError> {
        if self.status().await.is_ok() {
            return Ok(LaunchState::AlreadyRunning);
        }

        // A running daemon whose gateway is not up yet is still starting.
        if daemon.running_pid().await?.is_some() {
            self.wait_for_daemon_ready(Duration::from_secs(10)).await?;
            return Ok(LaunchState::AlreadyRunning);
        }

        let mut child = daemon.spawn_background_with_identity_secrets(identity_secrets)?;
        if let Err(err) = self
            .wait_until_ready(Some(&mut child), DAEMON_STARTUP_TIMEOUT)
            .await
        {
            return Err(format!("{err}; logs: {}", child.log_path.display()).into());
        }

        Ok(LaunchState::Started(child))
    }

    pub async fn wait_for_daemon_ready(&self, timeout: Duration) -> Result<(), BoxError> {
        self.wait_until_ready(None, timeout).await
    }

    /// Poll the status endpoint until it answers. A daemon this client just
    /// spawned fails the wait as soon as it exits, quoting its log.
    async fn wait_until_ready(
        &self,
        mut child: Option<&mut BackgroundDaemon>,
        timeout: Duration,
    ) -> Result<(), BoxError> {
        let deadline = Instant::now() + timeout;
        let detail = loop {
            let err = match self.status().await {
                Ok(_) => return Ok(()),
                Err(err) => err,
            };
            if let Some(child) = child.as_deref_mut()
                && let Some(status) = child.try_wait()?
            {
                let mut message = format!("Daemon exited during startup with {status}");
                if let Some(error) = daemon_startup_error(&child.log_path).await {
                    message.push_str(": ");
                    message.push_str(&error);
                }
                return Err(message.into());
            }
            if Instant::now() >= deadline {
                break err.to_string();
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };

        let mut message = format!("Daemon not ready within {timeout:?}: {detail}");
        if let Some(child) = child
            && let Some(error) = daemon_startup_error(&child.log_path).await
        {
            message.push_str("; last daemon error: ");
            message.push_str(&error);
        }
        Err(message.into())
    }

    /// Call an engine RPC method. Its single argument travels as JSON
    /// inside the base64 `params` envelope, and so does the result.
    async fn rpc<A, O>(
        &self,
        method: &str,
        args: &A,
        timeout: Option<Duration>,
    ) -> Result<O, BoxError>
    where
        A: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let params = serde_json::to_vec(&(args,))?;
        let mut req = self
            .request(reqwest::Method::POST, "/engine/default")
            .json(&RPCRequestRef {
                method,
                params: &ByteBufB64(params),
            });
        if let Some(timeout) = timeout {
            req = req.timeout(timeout);
        }
        let response: RPCResponse = self.decode_response(req.send().await?).await?;
        Ok(serde_json::from_slice(&response?)?)
    }

    async fn post_json<I, O>(&self, path: &str, input: &I) -> Result<O, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let req = self.request(reqwest::Method::POST, path);
        let response = req.json(&input).send().await?;
        self.decode_response(response).await
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let req = self.http.request(method, url);
        if self.auth_token.is_empty() {
            req
        } else {
            req.bearer_auth(&self.auth_token)
        }
    }

    async fn decode_response<O>(&self, response: reqwest::Response) -> Result<O, BoxError>
    where
        O: serde::de::DeserializeOwned,
    {
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(format!(
                "[GatewayClient] request failed, status: {status}, body: {}",
                body_excerpt(&body)
            )
            .into());
        }
        serde_json::from_slice(&body).map_err(|err| {
            format!(
                "[GatewayClient] Invalid response, error: {err}, body: {}",
                body_excerpt(&body)
            )
            .into()
        })
    }
}

fn body_excerpt(body: &[u8]) -> String {
    let excerpt = String::from_utf8_lossy(&body[..body.len().min(ERROR_BODY_EXCERPT)]);
    if body.len() > ERROR_BODY_EXCERPT {
        format!("{excerpt}… ({} bytes)", body.len())
    } else {
        excerpt.into_owned()
    }
}

/// Whether a conversation has finished and will receive no further updates.
pub fn is_terminal_conversation_status(status: &ConversationStatus) -> bool {
    matches!(
        status,
        ConversationStatus::Completed | ConversationStatus::Cancelled | ConversationStatus::Failed
    )
}

/// Unwraps an application tool response into its typed result.
pub(crate) fn tool_result<T>(response: ToolResponse) -> Result<T, BoxError>
where
    T: serde::de::DeserializeOwned,
{
    match response {
        ToolResponse::Ok { result, .. } => Ok(serde_json::from_value::<T>(result)?),
        ToolResponse::Err { error, .. } => {
            Err(format!("tool returned an error: {}: {}", error.code, error.message).into())
        }
    }
}

async fn daemon_startup_error(log_path: &Path) -> Option<String> {
    match read_daemon_log_tail(log_path).await {
        Ok(log_tail) => extract_daemon_startup_error(&log_tail),
        Err(err) => {
            log::warn!("Failed to read daemon log at {}: {err}", log_path.display());
            None
        }
    }
}

async fn read_daemon_log_tail(log_path: &Path) -> Result<String, BoxError> {
    let mut file = tokio::fs::File::open(log_path).await?;
    let len = file.metadata().await?.len();
    let start = len.saturating_sub(DAEMON_STARTUP_LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).await?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn extract_daemon_startup_error(log_tail: &str) -> Option<String> {
    log_tail.lines().rev().find_map(error_from_log_line)
}

fn error_from_log_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    if line.starts_with("{") {
        return serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| error_from_json_log(&value));
    }

    if line.starts_with("Error:") || line.to_ascii_lowercase().contains("error") {
        return Some(line.to_string());
    }

    None
}

fn error_from_json_log(value: &serde_json::Value) -> Option<String> {
    let level = value
        .get("level")
        .or_else(|| value.get("severity"))
        .and_then(|value| value.as_str())?;
    if !level.eq_ignore_ascii_case("error") {
        return None;
    }

    ["msg", "message", "error"]
        .iter()
        .find_map(|key| value.get(key).and_then(|value| value.as_str()))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_daemon_error_from_log_tail() {
        let log_tail = r#"{"level":"INFO","msg":"Starting daemon"}
Error: "Default TTS provider 'stepfun' is not configured. Available: []"
"#;

        assert_eq!(
            extract_daemon_startup_error(log_tail).as_deref(),
            Some("Error: \"Default TTS provider 'stepfun' is not configured. Available: []\"")
        );
    }

    #[test]
    fn extracts_structured_daemon_error_from_log_tail() {
        let log_tail = r#"{"level":"INFO","msg":"Starting daemon"}
{"level":"ERROR","msg":"Default TTS provider 'stepfun' is not configured. Available: []"}
{"level":"INFO","msg":"daemon process exited"}
"#;

        assert_eq!(
            extract_daemon_startup_error(log_tail).as_deref(),
            Some("Default TTS provider 'stepfun' is not configured. Available: []")
        );
    }

    use axum::{Router, routing};
    use serde_json::json;

    fn authorized(headers: &http::HeaderMap) -> bool {
        headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some("Bearer token-1")
    }

    fn status_app() -> Router {
        Router::new().route(
            "/daemon/status",
            routing::get(|headers: http::HeaderMap| async move {
                if !authorized(&headers) {
                    return (
                        http::StatusCode::UNAUTHORIZED,
                        axum::Json(json!({"error": "unauthorized"})),
                    );
                }
                (
                    http::StatusCode::OK,
                    axum::Json(json!({
                        "conversations": 7,
                        "memory_nodes": 11,
                        "memory_links": 13,
                    })),
                )
            }),
        )
    }

    #[test]
    fn request_omits_authorization_header_when_token_is_empty() {
        let anonymous = Client::new("http://127.0.0.1:1".to_string(), String::new())
            .request(reqwest::Method::GET, "/daemon/status")
            .build()
            .unwrap();
        assert!(
            !anonymous
                .headers()
                .contains_key(http::header::AUTHORIZATION)
        );

        let authed = Client::new("http://127.0.0.1:1".to_string(), "token-1".to_string())
            .request(reqwest::Method::GET, "/daemon/status")
            .build()
            .unwrap();
        assert_eq!(
            authed
                .headers()
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer token-1")
        );
    }

    #[tokio::test]
    async fn status_sends_bearer_token_and_decodes_response() {
        let base_url = crate::test_support::spawn_http_mock(status_app()).await;

        let client = Client::new(base_url.clone(), "token-1".to_string());
        let status = client.status().await.unwrap();
        assert_eq!(status.conversations, 7);
        assert_eq!(status.memory_nodes, 11);
        assert_eq!(status.memory_links, 13);

        // A wrong token is rejected by the server and surfaced as a status error.
        let unauthorized = Client::new(base_url, "wrong".to_string());
        let err = unauthorized.status().await.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("request failed, status: 401"));
    }

    #[tokio::test]
    async fn cli_workspace_registration_posts_the_path_with_authentication() {
        let app = Router::new().route(
            "/daemon/cli-workspace",
            routing::post(
                |headers: http::HeaderMap, axum::Json(body): axum::Json<serde_json::Value>| async move {
                    if !authorized(&headers) {
                        return (
                            http::StatusCode::UNAUTHORIZED,
                            axum::Json(json!({ "error": "unauthorized" })),
                        );
                    }
                    if body["workspace"] != "/tmp/anda-project" {
                        return (
                            http::StatusCode::BAD_REQUEST,
                            axum::Json(json!({ "error": "wrong workspace" })),
                        );
                    }
                    (http::StatusCode::OK, axum::Json(body))
                },
            ),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(base_url.clone(), "token-1".to_string());
        client
            .register_cli_workspace(Path::new("/tmp/anda-project"))
            .await
            .unwrap();

        let unauthorized = Client::new(base_url, "wrong".to_string());
        let err = unauthorized
            .register_cli_workspace(Path::new("/tmp/anda-project"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("request failed, status: 401"));
    }

    #[tokio::test]
    async fn decode_response_reports_invalid_json_bodies() {
        let app = Router::new().route(
            "/daemon/status",
            routing::get(|| async { "definitely not json" }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;

        let client = Client::new(base_url, "token-1".to_string());
        let err = client.status().await.map(|_| ()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid response"), "got: {msg}");
        assert!(msg.contains("definitely not json"), "got: {msg}");

        // A large body is quoted only in part.
        let app = Router::new().route(
            "/daemon/status",
            routing::get(|| async { "x".repeat(100_000) }),
        );
        let client = Client::new(
            crate::test_support::spawn_http_mock(app).await,
            "token-1".to_string(),
        );
        let msg = client.status().await.map(|_| ()).unwrap_err().to_string();
        assert!(
            msg.len() < 2 * ERROR_BODY_EXCERPT,
            "got {} bytes",
            msg.len()
        );
        assert!(msg.ends_with("(100000 bytes)"), "got: {msg}");
    }

    #[tokio::test]
    async fn memory_activity_sends_only_the_set_query_fields() {
        let app = Router::new().route(
            "/daemon/memory/v1/activity",
            routing::get(|uri: http::Uri| async move {
                assert_eq!(uri.query(), Some("conversation=42&limit=5"));
                axum::Json(json!({
                    "result": {
                        "schema_version": 1,
                        "items": [],
                        "next_cursor": null,
                        "complete": true,
                        "partial_reason": null,
                    },
                    "next_cursor": "page-two",
                }))
            }),
        );
        let client = Client::new(
            crate::test_support::spawn_http_mock(app).await,
            "token-1".to_string(),
        );
        let page = client
            .memory_activity(&crate::brain::activity::ActivityQuery {
                conversation: Some("42".to_string()),
                cursor: None,
                limit: Some(5),
            })
            .await
            .unwrap();
        assert_eq!(page.next_cursor.as_deref(), Some("page-two"));
    }

    #[tokio::test]
    async fn post_json_endpoints_round_trip() {
        let app = Router::new()
            .route(
                "/auto_update/check",
                routing::post(|| async {
                    axum::Json(serde_json::to_value(AutoUpdateState::default()).unwrap())
                }),
            )
            .route(
                "/daemon/shutdown",
                routing::post(|| async { axum::Json(json!({"shutdown": true})) }),
            )
            .route(
                "/daemon/models/reload",
                routing::post(|| async {
                    axum::Json(json!({
                        "active_model": "gpt-next",
                        "model_names": ["gpt-next"]
                    }))
                }),
            );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(base_url, "token-1".to_string());

        let state = client.auto_update_check().await.unwrap();
        assert!(state.latest_tag.is_none());

        let result = client.shutdown().await.unwrap();
        assert_eq!(result["shutdown"], true);

        let models = client.reload_models().await.unwrap();
        assert_eq!(
            serde_json::to_value(models).unwrap(),
            json!({
                "active_model": "gpt-next",
                "model_names": ["gpt-next"]
            })
        );
    }

    #[tokio::test]
    async fn agent_run_unwraps_rpc_response_payload() {
        let output = AgentOutput {
            content: "agent says hi".to_string(),
            ..Default::default()
        };
        let payload = ByteBufB64(serde_json::to_vec(&output).unwrap());
        let rpc: RPCResponse = Ok(payload);
        let body = serde_json::to_value(&rpc).unwrap();
        let app = Router::new().route(
            "/engine/default",
            routing::post(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(base_url, "token-1".to_string());

        let result = client
            .agent_run(&AgentInput::new(String::new(), "hello".to_string()))
            .await
            .unwrap();
        assert_eq!(result.content, "agent says hi");
    }

    #[tokio::test]
    async fn agent_run_surfaces_rpc_error_payload() {
        let rpc: RPCResponse = Err("engine exploded".to_string());
        let body = serde_json::to_value(&rpc).unwrap();
        let app = Router::new().route(
            "/engine/default",
            routing::post(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(base_url, "token-1".to_string());

        let err = client
            .agent_run_with_timeout(
                &AgentInput::new(String::new(), "hello".to_string()),
                Duration::from_secs(5),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("engine exploded"));
    }

    #[tokio::test]
    async fn cli_agent_run_registers_its_workspace_first_except_to_stop_or_cancel() {
        use std::sync::{Arc, Mutex};

        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let output = AgentOutput::default();
        let rpc: RPCResponse = Ok(ByteBufB64(serde_json::to_vec(&output).unwrap()));
        let body = serde_json::to_value(&rpc).unwrap();
        let register_calls = calls.clone();
        let run_calls = calls.clone();
        let app = Router::new()
            .route(
                "/daemon/cli-workspace",
                routing::post(move |axum::Json(request): axum::Json<Json>| {
                    let calls = register_calls.clone();
                    async move {
                        let workspace = request["workspace"].as_str().unwrap_or_default();
                        calls.lock().unwrap().push(format!("register {workspace}"));
                        if workspace == "/tmp/moved" {
                            return (
                                http::StatusCode::BAD_REQUEST,
                                axum::Json(json!({ "error": "cannot resolve workspace" })),
                            );
                        }
                        (http::StatusCode::OK, axum::Json(request))
                    }
                }),
            )
            .route(
                "/engine/default",
                routing::post(move || {
                    let calls = run_calls.clone();
                    let body = body.clone();
                    async move {
                        calls.lock().unwrap().push("agent_run".to_string());
                        axum::Json(body)
                    }
                }),
            );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(base_url, "token-1".to_string());
        let input = |prompt: &str, workspace: &str| {
            let mut input = AgentInput::new(String::new(), prompt.to_string());
            input.meta = Some(serde_json::from_value(json!({ "workspace": workspace })).unwrap());
            input
        };

        client
            .agent_run_in_cli_workspace(&input("hello", "/tmp/project"))
            .await
            .unwrap();
        // A restarted daemon has forgotten the directory; stopping never waits
        // on registering a directory that may no longer resolve.
        for prompt in ["/stop", "/cancel"] {
            client
                .agent_run_in_cli_workspace(&input(prompt, "/tmp/moved"))
                .await
                .unwrap();
        }
        let err = client
            .agent_run_in_cli_workspace(&input("hello", "/tmp/moved"))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("cannot resolve workspace"));
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "register /tmp/project",
                "agent_run",
                "agent_run",
                "agent_run",
                "register /tmp/moved",
            ]
        );
    }

    #[tokio::test]
    async fn tool_call_unwraps_rpc_response_payload() {
        let output: ToolOutput<Json> = ToolOutput::new(json!({"echo": "ok"}));
        let payload = ByteBufB64(serde_json::to_vec(&output).unwrap());
        let rpc: RPCResponse = Ok(payload);
        let body = serde_json::to_value(&rpc).unwrap();
        let app = Router::new().route(
            "/engine/default",
            routing::post(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let client = Client::new(base_url, "token-1".to_string());

        let result: ToolOutput<Json> = client
            .tool_call(&ToolInput::new("echo".to_string(), json!({})))
            .await
            .unwrap();
        assert_eq!(result.output["echo"], "ok");

        let result: ToolOutput<Json> = client
            .tool_call_with_timeout(
                &ToolInput::new("echo".to_string(), json!({})),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(result.output["echo"], "ok");
    }

    #[tokio::test]
    async fn rebased_client_targets_new_base_url() {
        let base_url = crate::test_support::spawn_http_mock(status_app()).await;

        let dead = Client::new("http://127.0.0.1:1".to_string(), "token-1".to_string());
        assert!(dead.status().await.is_err());

        let rebased = dead.rebased(base_url);
        assert!(rebased.status().await.is_ok());
    }

    #[tokio::test]
    async fn wait_for_daemon_ready_succeeds_and_times_out() {
        let base_url = crate::test_support::spawn_http_mock(status_app()).await;
        let client = Client::new(base_url, "token-1".to_string());
        client
            .wait_for_daemon_ready(Duration::from_secs(5))
            .await
            .unwrap();

        let dead = Client::new("http://127.0.0.1:1".to_string(), "token-1".to_string());
        let err = dead
            .wait_for_daemon_ready(Duration::ZERO)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Daemon not ready within"));
    }

    #[test]
    fn error_from_log_line_handles_plain_json_and_noise() {
        assert_eq!(error_from_log_line(""), None);
        assert_eq!(error_from_log_line("   "), None);
        assert_eq!(
            error_from_log_line("Error: bind failed").as_deref(),
            Some("Error: bind failed")
        );
        assert_eq!(
            error_from_log_line("fatal error while starting").as_deref(),
            Some("fatal error while starting")
        );
        assert_eq!(error_from_log_line("all good"), None);
        // Malformed JSON lines are ignored rather than treated as errors.
        assert_eq!(error_from_log_line("{not json"), None);
        assert_eq!(
            error_from_log_line(r#"{"level":"INFO","msg":"fine"}"#),
            None
        );
        assert_eq!(
            error_from_log_line(r#"{"severity":"ERROR","message":"db locked"}"#).as_deref(),
            Some("db locked")
        );
        assert_eq!(
            error_from_log_line(r#"{"level":"ERROR","error":"oom"}"#).as_deref(),
            Some("oom")
        );
        // Error-level entries without a recognizable message field yield None.
        assert_eq!(error_from_log_line(r#"{"level":"ERROR"}"#), None);
        // JSON without a level/severity field yields None.
        assert_eq!(error_from_log_line(r#"{"msg":"error happened"}"#), None);
    }

    #[tokio::test]
    async fn daemon_startup_error_reads_log_tail_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("daemon.log");
        tokio::fs::write(
            &log_path,
            "{\"level\":\"INFO\",\"msg\":\"starting\"}\nError: port in use\n",
        )
        .await
        .unwrap();

        assert_eq!(
            daemon_startup_error(&log_path).await.as_deref(),
            Some("Error: port in use")
        );

        // Missing log file is tolerated.
        assert_eq!(
            daemon_startup_error(&dir.path().join("missing.log")).await,
            None
        );
    }

    #[tokio::test]
    async fn ensure_daemon_running_returns_already_running_when_status_ok() {
        let base_url = crate::test_support::spawn_http_mock(status_app()).await;
        let client = Client::new(base_url, "token-1".to_string());
        let dir = tempfile::tempdir().unwrap();
        let daemon =
            crate::daemon::Daemon::new(dir.path().to_path_buf(), crate::config::Config::default());

        let state = client.ensure_daemon_running(&daemon).await.unwrap();
        assert!(matches!(state, LaunchState::AlreadyRunning));
    }

    #[tokio::test]
    async fn wait_for_daemon_ready_times_out_without_daemon() {
        // No server listening: the readiness wait times out quickly.
        let client = Client::new("http://127.0.0.1:1".to_string(), "token-1".to_string());
        let err = client
            .wait_for_daemon_ready(Duration::from_millis(300))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("not ready"));
    }
    #[test]
    fn terminal_conversation_status_covers_variants() {
        assert!(is_terminal_conversation_status(
            &ConversationStatus::Completed
        ));
        assert!(is_terminal_conversation_status(
            &ConversationStatus::Cancelled
        ));
        assert!(is_terminal_conversation_status(&ConversationStatus::Failed));
        assert!(!is_terminal_conversation_status(
            &ConversationStatus::Working
        ));
        assert!(!is_terminal_conversation_status(&ConversationStatus::Idle));
    }

    #[test]
    fn tool_result_unwraps_ok_and_reports_errors() {
        let ok: u64 = tool_result(ToolResponse::Ok {
            result: serde_json::json!(7),
            next_cursor: None,
        })
        .unwrap();
        assert_eq!(ok, 7);

        let err = tool_result::<u64>(ToolResponse::Err {
            error: crate::util::tool_response::ToolError::new("KIP_404", "nope".to_string()),
            result: None,
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "tool returned an error: KIP_404: nope");
    }
}
