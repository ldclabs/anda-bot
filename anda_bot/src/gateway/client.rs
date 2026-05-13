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

use crate::daemon::{Daemon, LaunchState, process_exists};

const DAEMON_STARTUP_LOG_TAIL_BYTES: u64 = 64 * 1024;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    auth_token: String,
}

impl Client {
    pub fn new(base_url: String, auth_token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
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

    pub async fn status(&self) -> Result<Json, BoxError> {
        self.get_json("").await
    }

    #[allow(unused)]
    pub async fn execute_kip_readonly(&self, req: &KipRequest) -> Result<KipResponse, BoxError> {
        self.post_json("/v1/anda_bot/execute_kip_readonly", &req)
            .await
    }

    pub async fn agent_run(&self, input: &AgentInput) -> Result<AgentOutput, BoxError> {
        let params = serde_json::to_vec(&(input,))?;
        let rt: RPCResponse = self
            .post_json(
                "/engine/default",
                &RPCRequestRef {
                    method: "agent_run",
                    params: &ByteBufB64(params),
                },
            )
            .await?;
        let rt: AgentOutput = serde_json::from_slice(&(rt?))?;
        Ok(rt)
    }

    pub async fn tool_call<I, O>(&self, input: &ToolInput<I>) -> Result<ToolOutput<O>, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let params = serde_json::to_vec(&(input,))?;
        let rt: RPCResponse = self
            .post_json(
                "/engine/default",
                &RPCRequestRef {
                    method: "tool_call",
                    params: &ByteBufB64(params),
                },
            )
            .await?;
        let rt: ToolOutput<O> = serde_json::from_slice(&(rt?))?;
        Ok(rt)
    }

    pub async fn ensure_daemon_running(&self, daemon: &Daemon) -> Result<LaunchState, BoxError> {
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

        let mut child = daemon.spawn_background()?;
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

    async fn get_json<O>(&self, path: &str) -> Result<O, BoxError>
    where
        O: serde::de::DeserializeOwned,
    {
        let req = self.request(reqwest::Method::GET, path);
        let response = req.send().await?;
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
}
