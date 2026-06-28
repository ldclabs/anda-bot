use anda_core::{
    AgentInput, AgentOutput, BoxError, ByteBufB64, Json, ToolInput, ToolOutput,
    http::{RPCRequestRef, RPCResponse},
};
use anda_kip::{Request as KipRequest, Response as KipResponse};
use std::{
    io::SeekFrom,
    path::Path,
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{
    auto_update::AutoUpdateState,
    daemon::{Daemon, LaunchState, process_exists},
    engine::{AndaBotStatus, DaemonModelsResponse},
    util::{http_client::new_reqwest_client, key::LocalIdentitySecrets},
};

const DAEMON_STARTUP_LOG_TAIL_BYTES: u64 = 64 * 1024;

// Agent runs routinely outlast the shared HTTP client's 120s default timeout
// (tool loops, model retries). Callers that need a quick failure signal, such
// as the chat keepalive ping, should pass their own timeout via
// `agent_run_with_timeout`.
pub const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

// The status endpoint is a loopback health check polled from interactive
// loops (TUI refresh, daemon readiness waits); fail fast instead of letting a
// wedged daemon hold callers for the client's full default timeout.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    auth_token: String,
}

impl Client {
    pub fn new(base_url: String, auth_token: String) -> Self {
        Self {
            http: new_reqwest_client(),
            base_url,
            auth_token,
        }
    }

    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    pub fn rebased(&self, base_url: String) -> Self {
        let mut client = self.clone();
        client.base_url = base_url;
        client
    }

    pub async fn status(&self) -> Result<AndaBotStatus, BoxError> {
        let req = self
            .request(reqwest::Method::GET, "/daemon/status")
            .timeout(STATUS_TIMEOUT);
        self.decode_response(req.send().await?).await
    }

    pub async fn auto_update_check(&self) -> Result<AutoUpdateState, BoxError> {
        self.post_json("/auto_update/check", &()).await
    }

    pub async fn reload_models(&self) -> Result<DaemonModelsResponse, BoxError> {
        self.post_json("/daemon/models/reload", &()).await
    }

    #[allow(unused)]
    pub async fn auto_update_install_and_restart(&self) -> Result<AutoUpdateState, BoxError> {
        self.post_json("/auto_update/install_and_restart", &())
            .await
    }

    pub async fn shutdown(&self) -> Result<Json, BoxError> {
        self.post_json("/daemon/shutdown", &()).await
    }

    #[allow(unused)]
    pub async fn execute_kip_readonly(&self, req: &KipRequest) -> Result<KipResponse, BoxError> {
        self.post_json("/v1/anda_bot/execute_kip_readonly", &req)
            .await
    }

    pub async fn agent_run(&self, input: &AgentInput) -> Result<AgentOutput, BoxError> {
        self.agent_run_with_timeout(input, AGENT_RUN_TIMEOUT).await
    }

    pub async fn agent_run_with_timeout(
        &self,
        input: &AgentInput,
        timeout: Duration,
    ) -> Result<AgentOutput, BoxError> {
        let params = serde_json::to_vec(&(input,))?;
        let req = self
            .request(reqwest::Method::POST, "/engine/default")
            .timeout(timeout)
            .json(&RPCRequestRef {
                method: "agent_run",
                params: &ByteBufB64(params),
            });
        let rt: RPCResponse = self.decode_response(req.send().await?).await?;
        let rt: AgentOutput = serde_json::from_slice(&(rt?))?;
        Ok(rt)
    }

    pub async fn tool_call<I, O>(&self, input: &ToolInput<I>) -> Result<ToolOutput<O>, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        self.tool_call_inner(input, None).await
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
        self.tool_call_inner(input, Some(timeout)).await
    }

    async fn tool_call_inner<I, O>(
        &self,
        input: &ToolInput<I>,
        timeout: Option<Duration>,
    ) -> Result<ToolOutput<O>, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let params = serde_json::to_vec(&(input,))?;
        let mut req = self.request(reqwest::Method::POST, "/engine/default");
        if let Some(timeout) = timeout {
            req = req.timeout(timeout);
        }
        let req = req.json(&RPCRequestRef {
            method: "tool_call",
            params: &ByteBufB64(params),
        });
        let rt: RPCResponse = self.decode_response(req.send().await?).await?;
        let rt: ToolOutput<O> = serde_json::from_slice(&(rt?))?;
        Ok(rt)
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

        let pid_path = daemon.pid_file_path();
        if let Some(pid) = daemon.read_pid_file().await? {
            if process_exists(pid) {
                self.wait_for_daemon_ready(Duration::from_secs(10)).await?;
                return Ok(LaunchState::AlreadyRunning);
            }
            let _ = tokio::fs::remove_file(&pid_path).await;
        }

        let mut child = if let Some(identity_secrets) = identity_secrets {
            daemon.spawn_background_with_identity_secrets(Some(identity_secrets))?
        } else {
            daemon.spawn_background()?
        };
        if let Err(err) = self
            .wait_for_spawned_daemon_ready(&mut child, Duration::from_secs(20))
            .await
        {
            return Err(format!("{err}; logs: {}", child.log_path.display()).into());
        }

        Ok(LaunchState::Started(child))
    }

    async fn wait_for_spawned_daemon_ready(
        &self,
        child: &mut crate::daemon::BackgroundDaemon,
        timeout: Duration,
    ) -> Result<(), BoxError> {
        let deadline = Instant::now() + timeout;
        let detail = loop {
            match self.status().await {
                Ok(_) => return Ok(()),
                Err(err) => {
                    if let Some(status) = child.try_wait()? {
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
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };

        let mut message = format!("Daemon not ready within {timeout:?}: {detail}");
        if let Some(error) = daemon_startup_error(&child.log_path).await {
            message.push_str("; last daemon error: ");
            message.push_str(&error);
        }
        Err(message.into())
    }

    pub async fn wait_for_daemon_ready(&self, timeout: Duration) -> Result<(), BoxError> {
        let deadline = Instant::now() + timeout;
        let detail = loop {
            match self.status().await {
                Ok(_) => return Ok(()),
                Err(err) if Instant::now() >= deadline => break err.to_string(),
                Err(_) => {}
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        Err(format!("Daemon not ready within {timeout:?}: {detail}").into())
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
        self.http.request(method, url).bearer_auth(&self.auth_token)
    }

    async fn decode_response<O>(&self, response: reqwest::Response) -> Result<O, BoxError>
    where
        O: serde::de::DeserializeOwned,
    {
        if response.status().is_success() {
            let text = response.text().await?;

            match serde_json::from_str::<O>(&text) {
                Ok(res) => Ok(res),
                Err(err) => Err(format!(
                    "[GatewayClient] Invalid response, error: {}, body: {}",
                    err, text
                )
                .into()),
            }
        } else {
            let status = response.status();
            let msg = response.text().await?;
            log::error!("[GatewayClient] request failed: {status}, body: {msg}");
            Err(format!(
                "[GatewayClient] request failed, status: {}, body: {}",
                status, msg
            )
            .into())
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

    async fn spawn_gateway_mock(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

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

    #[tokio::test]
    async fn status_sends_bearer_token_and_decodes_response() {
        let base_url = spawn_gateway_mock(status_app()).await;

        let client = Client::new(base_url.clone(), "token-1".to_string())
            .with_http_client(new_reqwest_client());
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
    async fn decode_response_reports_invalid_json_bodies() {
        let app = Router::new().route(
            "/daemon/status",
            routing::get(|| async { "definitely not json" }),
        );
        let base_url = spawn_gateway_mock(app).await;

        let client = Client::new(base_url, "token-1".to_string());
        let err = client.status().await.map(|_| ()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid response"), "got: {msg}");
        assert!(msg.contains("definitely not json"), "got: {msg}");
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
        let base_url = spawn_gateway_mock(app).await;
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
        let base_url = spawn_gateway_mock(app).await;
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
        let base_url = spawn_gateway_mock(app).await;
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
        let base_url = spawn_gateway_mock(app).await;
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
        let base_url = spawn_gateway_mock(status_app()).await;

        let dead = Client::new("http://127.0.0.1:1".to_string(), "token-1".to_string());
        assert!(dead.status().await.is_err());

        let rebased = dead.rebased(base_url);
        assert!(rebased.status().await.is_ok());
    }

    #[tokio::test]
    async fn wait_for_daemon_ready_succeeds_and_times_out() {
        let base_url = spawn_gateway_mock(status_app()).await;
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
    async fn auto_update_install_and_execute_kip_round_trip() {
        let app = Router::new()
            .route(
                "/auto_update/install_and_restart",
                routing::post(|| async {
                    axum::Json(serde_json::to_value(AutoUpdateState::default()).unwrap())
                }),
            )
            .route(
                "/v1/anda_bot/execute_kip_readonly",
                routing::post(|| async {
                    axum::Json(json!({"result": {"ok": true}, "next_cursor": null}))
                }),
            );
        let base_url = spawn_gateway_mock(app).await;
        let client = Client::new(base_url, "token-1".to_string());

        let state = client.auto_update_install_and_restart().await.unwrap();
        assert!(state.latest_tag.is_none());

        let kip = client
            .execute_kip_readonly(&anda_kip::Request::default())
            .await
            .unwrap();
        assert!(matches!(kip, anda_kip::Response::Ok { .. }));
    }

    #[tokio::test]
    async fn ensure_daemon_running_returns_already_running_when_status_ok() {
        let base_url = spawn_gateway_mock(status_app()).await;
        let client =
            Client::new(base_url, "token-1".to_string()).with_http_client(new_reqwest_client());
        let dir = tempfile::tempdir().unwrap();
        let daemon =
            crate::daemon::Daemon::new(dir.path().to_path_buf(), crate::config::Config::default());

        let state = client.ensure_daemon_running(&daemon).await.unwrap();
        assert!(matches!(state, LaunchState::AlreadyRunning));
    }

    #[tokio::test]
    async fn wait_for_daemon_ready_times_out_without_daemon() {
        // No server listening: the readiness wait times out quickly.
        let client = Client::new("http://127.0.0.1:1".to_string(), "token-1".to_string())
            .with_http_client(new_reqwest_client());
        let err = client
            .wait_for_daemon_ready(Duration::from_millis(300))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("not ready"));
    }
}
