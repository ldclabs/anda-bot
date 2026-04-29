use anda_core::{
    AgentInput, AgentOutput, BoxError, ByteBufB64, Json, ToolInput, ToolOutput,
    http::{RPCRequestRef, RPCResponse},
};
use anda_kip::{Request as KipRequest, Response as KipResponse};
use std::time::{Duration, Instant};

use crate::daemon::{Daemon, LaunchState, process_exists};

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

        let child = daemon.spawn_background()?;
        if let Err(err) = self.wait_for_daemon_ready(Duration::from_secs(20)).await {
            return Err(format!("{err}; logs: {}", child.log_path.display()).into());
        }

        Ok(LaunchState::Started(child))
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
