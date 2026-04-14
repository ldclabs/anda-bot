use anda_core::{AgentOutput, BoxError, FunctionDefinition, Json, Resource, Tool, ToolOutput};
use anda_engine::{context::BaseCtx, model::reqwest};
use anda_kip::{Map, Request as KipRequest, Response as KipResponse};
use serde_json::json;

pub use anda_hippocampus::{
    payload::RpcResponse,
    types::{FormationInput, RecallInput},
};

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    // Base URL of the Hippocampus space, e.g., "http://localhost:8042/v1/{space_id}"
    base_url: String,
    auth_token: Option<String>,
}

impl Client {
    pub const NAME: &'static str = "recall_memory";
    pub fn new(base_url: String, auth_token: Option<String>) -> Self {
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

    pub async fn formation(&self, input: FormationInput) -> Result<AgentOutput, BoxError> {
        let rt: RpcResponse<AgentOutput> = self.post("/formation", &input).await?;
        if let Some(result) = rt.result {
            Ok(result)
        } else {
            Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| {
                    "[HippocampusClient] formation failed with unknown error".to_string()
                })
                .into())
        }
    }

    pub async fn recall(&self, input: RecallInput) -> Result<AgentOutput, BoxError> {
        let rt: RpcResponse<AgentOutput> = self.post("/recall", &input).await?;
        if let Some(result) = rt.result {
            Ok(result)
        } else {
            Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| {
                    "[HippocampusClient] recall failed with unknown error".to_string()
                })
                .into())
        }
    }

    pub async fn describe_primer(&self) -> Result<Json, BoxError> {
        let rt: KipResponse = self
            .post(
                "/execute_kip_readonly",
                &KipRequest {
                    command: "DESCRIBE PRIMER".to_string(),
                    ..Default::default()
                },
            )
            .await?;
        match rt {
            KipResponse::Ok { result, .. } => Ok(result),
            KipResponse::Err { .. } => Err(serde_json::to_string(&rt)
                .unwrap_or_else(|_| {
                    "[HippocampusClient] describe_primer failed with unknown error".to_string()
                })
                .into()),
        }
    }

    pub async fn user_info(&self, id: String) -> Json {
        let rt: Result<KipResponse, BoxError> = self
            .post(
                "/execute_kip_readonly",
                &KipRequest {
                    command: "FIND(?user) WHERE { ?user {type:\"Person\", name: :name} }"
                        .to_string(),
                    parameters: Map::from_iter([("name".to_string(), Json::String(id.clone()))]),
                    ..Default::default()
                },
            )
            .await;
        match rt {
            Ok(KipResponse::Ok { result, .. }) => result,
            _ => json!({
                "type": "Person",
                "name": id,
            }),
        }
    }

    async fn post<I, O>(&self, path: &str, input: &I) -> Result<O, BoxError>
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
        if let Some(token) = &self.auth_token {
            self.http.request(method, url).bearer_auth(token)
        } else {
            self.http.request(method, url)
        }
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
                    "[HippocampusClient] Invalid response, error: {}, body: {}",
                    err, text
                )
                .into()),
            }
        } else {
            let status = response.status();
            let msg = response.text().await?;
            log::error!("[HippocampusClient] request failed: {status}, body: {msg}");
            Err(format!(
                "[HippocampusClient] request failed, status: {}, body: {}",
                status, msg
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
        "Recall information from the assistant's long-term memory (the Cognitive Nexus owned by $self). Send a natural language query describing what you want to remember or look up — the memory system will search and return relevant knowledge, including facts, preferences, relationships, past events, self-reflective insights, and other stored information. Use this whenever you need context from previous interactions or stored knowledge to answer the current conversation.".to_string()
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
                  "description": "A natural language question or description of what information to retrieve from memory. Be specific and include relevant context. Examples: 'What do we know about the current user's communication preferences?', 'What happened in our last discussion about Project Aurora?', 'Who are the members of the engineering team?', 'What has the assistant learned about how to respond when the user asks for brevity?'"
                },
                "context": {
                  "type": "object",
                  "description": "Optional current conversational context used only to disambiguate the query within $self's memory. It does not change the memory owner. Provide any relevant identifiers or scope hints that could improve retrieval accuracy.",
                  "properties": {
                    "counterparty": {
                      "type": "string",
                      "description": "Preferred. Durable identifier of the current external person or organization interacting with the business agent. Useful for resolving implicit references such as 'the current user', 'they', or omitted subjects."
                    },
                    "agent": {
                      "type": "string",
                      "description": "The identifier of the calling business agent, if applicable. Useful for provenance or caller-specific queries, but it does not change whose memory is searched."
                    },
                    "source": {
                      "type": "string",
                      "description": "Identifier of the current source, thread, channel, or app context. Useful when the query refers to a previous discussion in the same place."
                    },
                    "topic": {
                      "type": "string",
                      "description": "The topic of the current conversation, to help disambiguate the query."
                    }
                  }
                }
              },
              "required": [
                "query"
              ]
            }),
            strict: None,
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        request: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let rt = self.recall(request).await?;
        Ok(ToolOutput::new(rt.content))
    }
}
