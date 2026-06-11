use anda_core::{AgentOutput, BoxError, FunctionDefinition, Json, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use anda_kip::{Request as KipRequest, Response as KipResponse};
use serde_json::json;
use std::time::Duration;

use crate::util::http_client::new_reqwest_client;

pub use anda_brain::{
    payload::RpcResponse,
    types::{FormationInputRef, FormationStatus, GetOrInitUserInput, RecallInput, RecallInputRef},
};

// Recall runs LLM work inline in the brain handler and can exceed the shared
// HTTP client's 120s default timeout; a client-side timeout drops the loopback
// connection and cancels the in-flight work. Lightweight reads (primer, user
// info, status) keep the client default for fast failure.
const RECALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    // Base URL of the Brain space, e.g., "http://localhost:8042/v1/{space_id}"
    base_url: String,
    auth_token: Option<String>,
}

impl Client {
    pub const NAME: &'static str = "recall_memory";
    pub fn new(base_url: String, auth_token: Option<String>) -> Self {
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

    pub async fn formation<'a>(
        &self,
        input: FormationInputRef<'a>,
    ) -> Result<AgentOutput, BoxError> {
        let rt: RpcResponse<AgentOutput> = self.post("/formation", &input).await?;
        if let Some(result) = rt.result {
            Ok(result)
        } else {
            Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| "[BrainClient] formation failed with unknown error".to_string())
                .into())
        }
    }

    pub async fn recall<'a>(&self, input: RecallInputRef<'a>) -> Result<AgentOutput, BoxError> {
        let rt: RpcResponse<AgentOutput> = self
            .post_with_timeout("/recall", &input, RECALL_TIMEOUT)
            .await?;
        if let Some(result) = rt.result {
            Ok(result)
        } else {
            Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| "[BrainClient] recall failed with unknown error".to_string())
                .into())
        }
    }

    pub async fn describe_primer(&self) -> Result<Json, BoxError> {
        let rt = self
            .execute_kip_readonly(KipRequest {
                command: "DESCRIBE PRIMER".to_string(),
                ..Default::default()
            })
            .await?;
        match rt {
            KipResponse::Ok { result, .. } => Ok(result),
            KipResponse::Err { .. } => Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| {
                    "[BrainClient] describe_primer failed with unknown error".to_string()
                })
                .into()),
        }
    }

    pub async fn execute_kip_readonly(&self, request: KipRequest) -> Result<KipResponse, BoxError> {
        self.post("/execute_kip_readonly", &request).await
    }

    pub async fn user_info(&self, user: String, name: Option<String>) -> Result<Json, BoxError> {
        let rt: Json = self
            .post("/get_or_init_user", &GetOrInitUserInput { user, name })
            .await?;

        Ok(rt)
    }

    pub async fn brain_status(&self) -> Result<FormationStatus, BoxError> {
        let rt: RpcResponse<FormationStatus> = self.get("/formation_status").await?;
        if let Some(result) = rt.result {
            Ok(result)
        } else {
            Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| {
                    "[BrainClient] brain_state failed with unknown error".to_string()
                })
                .into())
        }
    }

    async fn post<I, O>(&self, path: &str, input: &I) -> Result<O, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let req = self.request(reqwest::Method::POST, path);
        let response = req.json(&input).send().await?;
        self.decode_response(reqwest::Method::POST, path, response)
            .await
    }

    async fn post_with_timeout<I, O>(
        &self,
        path: &str,
        input: &I,
        timeout: Duration,
    ) -> Result<O, BoxError>
    where
        I: serde::Serialize,
        O: serde::de::DeserializeOwned,
    {
        let req = self.request(reqwest::Method::POST, path).timeout(timeout);
        let response = req.json(&input).send().await?;
        self.decode_response(reqwest::Method::POST, path, response)
            .await
    }

    async fn get<O>(&self, path: &str) -> Result<O, BoxError>
    where
        O: serde::de::DeserializeOwned,
    {
        let req = self.request(reqwest::Method::GET, path);
        let response = req.send().await?;
        self.decode_response(reqwest::Method::GET, path, response)
            .await
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        if let Some(token) = &self.auth_token {
            self.http.request(method, url).bearer_auth(token)
        } else {
            self.http.request(method, url)
        }
    }

    async fn decode_response<O>(
        &self,
        method: reqwest::Method,
        path: &str,
        response: reqwest::Response,
    ) -> Result<O, BoxError>
    where
        O: serde::de::DeserializeOwned,
    {
        if response.status().is_success() {
            let text = response.text().await?;

            match serde_json::from_str::<O>(&text) {
                Ok(res) => Ok(res),
                Err(err) => Err(format!(
                    "[BrainClient] Invalid response for {} {}, error: {}, body: {}",
                    method, path, err, text
                )
                .into()),
            }
        } else {
            let status = response.status();
            let msg = response.text().await?;
            log::error!(
                "[BrainClient] request failed for {} {}: {status}, body: {msg}",
                method,
                path
            );
            Err(format!(
                "[BrainClient] request failed for {} {}: {status}, body: {msg}",
                method, path
            )
            .into())
        }
    }
}

impl Tool<BaseCtx> for Client {
    type Args = RecallInput;
    type Output = String;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Recall information from the assistant's long-term memory (the Cognitive Nexus owned by $self). Use only for information that is not already present in the active conversation. Do not call for facts just mentioned, just submitted to formation, or otherwise available in current context; formation is asynchronous and fresh memories may take a minute or more to become searchable.".to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
              "type": "object",
              "properties": {
                "query": {
                  "type": "string",
                  "description": "A natural language question about older or out-of-context memory. Be specific and include the subject, timeframe, and topic when known. Examples: 'What do we know about the current user's communication preferences?', 'What happened in our last discussion about Project Aurora?', 'Who are the members of the engineering team?'"
                },
                "context": {
                  "type": [
                    "object",
                    "null"
                  ],
                  "description": "Optional current conversational context used only to disambiguate the query within $self's memory. Pass an object, not a JSON string. It does not change the memory owner.",
                  "properties": {
                    "counterparty": {
                      "type": [
                        "string",
                        "null"
                      ],
                      "description": "Preferred. Durable identifier of the current external person or organization interacting with the business agent. Useful for resolving implicit references such as 'the current user', 'they', or omitted subjects."
                    },
                    "agent": {
                      "type": [
                        "string",
                        "null"
                      ],
                      "description": "The identifier of the calling business agent, if applicable. Useful for provenance or caller-specific queries, but it does not change whose memory is searched."
                    },
                    "source": {
                      "type": [
                        "string",
                        "null"
                      ],
                      "description": "Identifier of the current source, thread, channel, or app context. Useful when the query refers to a previous discussion in the same place."
                    },
                    "topic": {
                      "type": [
                        "string",
                        "null"
                      ],
                      "description": "The topic of the current conversation, to help disambiguate the query."
                    }
                  },
                  "required": [
                    "counterparty",
                    "agent",
                    "source",
                    "topic"
                  ],
                  "additionalProperties": false
                }
              },
              "required": [
                "query",
                "context"
              ],
              "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        request: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let rt = self.recall((&request).into()).await?;
        Ok(ToolOutput::new(rt.content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;

    #[test]
    fn recall_memory_schema_is_openai_strict() {
        let client = Client::new("http://localhost:8042/v1/test".to_string(), None);
        let definition = client.definition();

        assert_eq!(definition.strict, Some(true));
        assert_openai_strict_parameters(&definition.parameters);
    }

    #[test]
    fn recall_memory_args_accept_null_context() {
        let request = serde_json::from_value::<RecallInput>(serde_json::json!({
            "query": "What did we discuss about the release?",
            "context": null,
        }));

        assert!(request.is_ok());
    }

    use anda_engine::engine::EngineBuilder;
    use axum::{Router, routing};
    use serde_json::Value;

    async fn spawn_brain_mock(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/v1/anda_bot")
    }

    fn agent_output_response(content: &str) -> Value {
        json!({
            "result": serde_json::to_value(AgentOutput {
                content: content.to_string(),
                ..Default::default()
            })
            .unwrap()
        })
    }

    fn formation_status_response() -> Value {
        json!({
            "result": {
                "id": "anda_bot",
                "concepts": 3,
                "propositions": 5,
                "conversations": 2,
                "formation_processing": false,
                "maintenance_processing": false,
                "formation_processed_id": 9,
                "maintenance_processed_id": 4,
                "maintenance_at": {"daydream": 0, "full": 0, "quick": 0},
            }
        })
    }

    #[tokio::test]
    async fn brain_status_decodes_rpc_result_and_sends_auth() {
        let app = Router::new().route(
            "/v1/anda_bot/formation_status",
            routing::get(|headers: http::HeaderMap| async move {
                if headers
                    .get(http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    != Some("Bearer brain-token")
                {
                    return (
                        http::StatusCode::UNAUTHORIZED,
                        axum::Json(json!({"error": "unauthorized"})),
                    );
                }
                (
                    http::StatusCode::OK,
                    axum::Json(formation_status_response()),
                )
            }),
        );
        let base_url = spawn_brain_mock(app).await;

        let client = Client::new(base_url.clone(), Some("brain-token".to_string()))
            .with_http_client(new_reqwest_client());
        let status = client.brain_status().await.unwrap();
        assert_eq!(status.concepts, 3);
        assert_eq!(status.propositions, 5);

        // Without a token the request carries no Authorization header.
        let anonymous = Client::new(base_url, None);
        let err = anonymous.brain_status().await.map(|_| ()).unwrap_err();
        assert!(
            err.to_string()
                .contains("request failed for GET /formation_status")
        );
    }

    #[tokio::test]
    async fn brain_status_surfaces_rpc_error_payload() {
        let app = Router::new().route(
            "/v1/anda_bot/formation_status",
            routing::get(|| async {
                axum::Json(json!({"error": {"code": 500, "message": "brain offline"}}))
            }),
        );
        let base_url = spawn_brain_mock(app).await;

        let client = Client::new(base_url, None);
        let err = client.brain_status().await.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("brain offline"));
    }

    #[tokio::test]
    async fn formation_and_recall_unwrap_agent_output() {
        let app = Router::new()
            .route(
                "/v1/anda_bot/formation",
                routing::post(|| async { axum::Json(agent_output_response("formed")) }),
            )
            .route(
                "/v1/anda_bot/recall",
                routing::post(|| async { axum::Json(agent_output_response("recalled")) }),
            );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);

        let output = client
            .formation(FormationInputRef {
                messages: &[],
                context: &None,
                timestamp: &None,
            })
            .await
            .unwrap();
        assert_eq!(output.content, "formed");

        let output = client
            .recall(RecallInputRef {
                query: "what happened",
                context: &None,
            })
            .await
            .unwrap();
        assert_eq!(output.content, "recalled");
    }

    #[tokio::test]
    async fn formation_and_recall_report_missing_result() {
        let app = Router::new()
            .route(
                "/v1/anda_bot/formation",
                routing::post(|| async {
                    axum::Json(json!({"error": {"code": 503, "message": "queue full"}}))
                }),
            )
            .route(
                "/v1/anda_bot/recall",
                routing::post(|| async {
                    axum::Json(json!({"error": {"code": 503, "message": "recall failed"}}))
                }),
            );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);

        let err = client
            .formation(FormationInputRef {
                messages: &[],
                context: &None,
                timestamp: &None,
            })
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("queue full"));

        let err = client
            .recall(RecallInputRef {
                query: "what happened",
                context: &None,
            })
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("recall failed"));
    }

    #[tokio::test]
    async fn describe_primer_handles_ok_and_err_kip_responses() {
        let app = Router::new().route(
            "/v1/anda_bot/execute_kip_readonly",
            routing::post(|axum::Json(body): axum::Json<Value>| async move {
                assert_eq!(body["command"], "DESCRIBE PRIMER");
                axum::Json(
                    serde_json::to_value(KipResponse::ok(json!({"identity": "panda"}))).unwrap(),
                )
            }),
        );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);
        let primer = client.describe_primer().await.unwrap();
        assert_eq!(primer["identity"], "panda");

        let app = Router::new().route(
            "/v1/anda_bot/execute_kip_readonly",
            routing::post(|| async {
                axum::Json(
                    serde_json::to_value(KipResponse::Err {
                        error: anda_kip::ErrorObject {
                            code: "KIP_2001".to_string(),
                            message: "nexus unavailable".to_string(),
                            hint: None,
                            data: None,
                        },
                        result: None,
                    })
                    .unwrap(),
                )
            }),
        );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);
        let err = client.describe_primer().await.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("nexus unavailable"));
    }

    #[tokio::test]
    async fn user_info_posts_user_and_decodes_json() {
        let app = Router::new().route(
            "/v1/anda_bot/get_or_init_user",
            routing::post(|axum::Json(body): axum::Json<Value>| async move {
                axum::Json(json!({"user": body["user"], "trust": "high"}))
            }),
        );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);

        let info = client.user_info("alice".to_string(), None).await.unwrap();
        assert_eq!(info["user"], "alice");
        assert_eq!(info["trust"], "high");
    }

    #[tokio::test]
    async fn decode_response_reports_invalid_body_with_route() {
        let app = Router::new().route(
            "/v1/anda_bot/formation_status",
            routing::get(|| async { "plain text" }),
        );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);

        let err = client.brain_status().await.map(|_| ()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Invalid response for GET /formation_status"),
            "got: {msg}"
        );
        assert!(msg.contains("plain text"), "got: {msg}");
    }

    #[tokio::test]
    async fn recall_tool_call_returns_agent_content() {
        let app = Router::new().route(
            "/v1/anda_bot/recall",
            routing::post(|| async { axum::Json(agent_output_response("memory found")) }),
        );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);
        let ctx = EngineBuilder::new().mock_ctx().base;

        let result = client
            .call(
                ctx,
                RecallInput {
                    query: "What is the project status?".to_string(),
                    context: None,
                },
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.output, "memory found");
    }
}
