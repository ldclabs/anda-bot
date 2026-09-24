use anda_core::{AgentOutput, BoxError, FunctionDefinition, Json, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use anda_kip::{Request as KipRequest, Response as KipResponse};
use serde_json::json;
use std::time::{Duration, Instant};

use crate::util::http_client::build_http_client;

pub use anda_brain::runtime_api::{
    AttentionItem, AttentionPage, AttentionQuery, AttentionResponse, ResponseReceipt, RuntimeStatus,
};
pub use anda_brain::types::RecallOutput;
pub use anda_brain::{
    payload::RpcResponse,
    types::{
        FormationInputRef, FormationStatus, GetOrInitUserInput, MaintenanceInput, MaintenanceScope,
        RecallInput, RecallInputRef,
    },
};

/// HTTP status remains available to callers (e.g. restart an expired cursor on
/// 409, but never turn a revoked 403 into an anonymous request).
#[derive(Debug)]
pub struct HttpError {
    pub status: reqwest::StatusCode,
    pub message: String,
}
impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for HttpError {}

/// A syntactically valid Brain RPC response that explicitly reported failure.
/// Unlike a timeout or interrupted connection, this proves the request reached
/// the service and received a negative response.
#[derive(Debug)]
pub struct RpcFailure {
    pub message: String,
}
impl std::fmt::Display for RpcFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RpcFailure {}

fn rpc_result<T: serde::Serialize>(mut response: RpcResponse<T>) -> Result<T, BoxError> {
    if response.error.is_none()
        && let Some(result) = response.result.take()
    {
        return Ok(result);
    }
    Err(Box::new(RpcFailure {
        message: format!("[BrainClient] {}", serde_json::to_string(&response)?),
    }))
}

fn recall_tool_output(output: AgentOutput, budgeted: bool) -> ToolOutput<String> {
    let is_error = output.failed_reason.as_ref().map(|_| true);
    let content = match output.failed_reason {
        Some(_) if budgeted => output.content,
        Some(reason) => format!(
            "Recall failed: {}",
            reason.chars().take(512).collect::<String>()
        ),
        None => output.content,
    };
    ToolOutput {
        output: content,
        is_error,
        usage: output.usage,
        tools_usage: output.tools_usage,
        artifacts: Vec::new(),
    }
}

// Recall runs LLM work inline in the brain handler. Keep its client-side
// timeout explicit so slow calls fail predictably while lightweight reads
// (primer, user info, status) keep the client default behavior.
const RECALL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const SLOW_RECALL_WARN_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    // Base URL of the Brain space, e.g., "http://localhost:8042/v1/{space_id}"
    base_url: String,
    auth_token: Option<String>,
    host: Option<super::Host>,
    journal: Option<super::Journal>,
}

impl Client {
    pub const NAME: &'static str = "recall_memory";
    pub fn new(base_url: String, auth_token: Option<String>) -> Self {
        Self {
            // Recall and formation can accept billable work before an error
            // response arrives. Keep one connection pool and never replay it.
            http: build_http_client(None, |builder| builder.retry(reqwest::retry::never()))
                .expect("failed to build Brain HTTP client"),
            base_url,
            auth_token,
            host: None,
            journal: None,
        }
    }

    #[cfg(test)]
    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    pub fn with_host(mut self, host: super::Host, journal: super::Journal) -> Self {
        self.host = Some(host);
        self.journal = Some(journal);
        self
    }

    pub async fn submit_formation_window(
        &self,
        mut submission: super::FormationSubmission,
        input: FormationInputRef<'_>,
    ) -> Result<super::FormationSubmission, BoxError> {
        if let Some(provenance) = submission.provenance.as_mut() {
            if provenance.source_messages.len() != input.messages.len() {
                return Err("Formation source mapping length mismatch".into());
            }
            for (source, message) in provenance.source_messages.iter_mut().zip(input.messages) {
                source.submitted_digest = Some(anda_cognitive_nexus::content_digest(
                    &serde_json::to_value(message)?,
                )?);
            }
            provenance.input_digest = Some(anda_cognitive_nexus::content_digest(
                &serde_json::to_value(&input)?,
            )?);
        }
        match &self.journal {
            Some(journal) => journal.submit_formation(self, submission, input).await,
            None => {
                let output = self.formation(input).await?;
                if let Some(reason) = output.failed_reason {
                    return Err(reason.into());
                }
                Ok(super::FormationSubmission {
                    brain_conversation: output.conversation,
                    state: super::FormationState::Accepted,
                    ..submission
                })
            }
        }
    }

    pub(super) fn embedded_host(&self) -> Option<super::Host> {
        self.host.clone()
    }
    pub(super) fn journal(&self) -> Option<super::Journal> {
        self.journal.clone()
    }

    pub(super) async fn formation_submission(
        &self,
        input: FormationInputRef<'_>,
        submission: &super::FormationSubmission,
    ) -> Result<AgentOutput, BoxError> {
        if let (Some(host), Some(provenance)) = (&self.host, &submission.provenance) {
            let space = host
                .state
                .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
                .await?;
            return space
                .ingest_product(
                    anda_brain::agents::SELF_USER_ID,
                    anda_brain::types::FormationInput {
                        messages: input.messages.to_vec(),
                        context: input.context.clone(),
                        timestamp: input.timestamp.clone(),
                    },
                    provenance.source_identity.clone().unwrap_or_else(|| {
                        super::product::source_identity(
                            &provenance.caller,
                            submission.bot_conversation,
                            provenance.session.as_deref(),
                        )
                    }),
                )
                .await
                .map_err(|error| {
                    if matches!(
                        error.downcast_ref::<anda_brain::product::SourceAdmissionError>(),
                        Some(anda_brain::product::SourceAdmissionError::Busy)
                    ) {
                        Box::new(RpcFailure {
                            message: "memory_change_pending".into(),
                        }) as BoxError
                    } else {
                        error
                    }
                });
        }
        self.formation(input).await
    }

    /// Forward the original verified bearer across a transport boundary.
    pub fn with_auth_token(&self, token: String) -> Self {
        let mut client = self.clone();
        client.auth_token = Some(token);
        client
    }

    pub async fn attention(&self, query: &AttentionQuery) -> Result<AttentionPage, BoxError> {
        let mut url = reqwest::Url::parse(&format!("{}/attention", self.base_url))?;
        {
            let mut params = url.query_pairs_mut();
            if let Some(cursor) = &query.cursor {
                params.append_pair("cursor", cursor);
            }
            if let Some(limit) = query.limit {
                params.append_pair("limit", &limit.to_string());
            }
        }
        let path = format!("/attention?{}", url.query().unwrap_or_default());
        rpc_result(self.get(&path).await?)
    }

    pub async fn respond(
        &self,
        id: &str,
        response: &AttentionResponse,
    ) -> Result<ResponseReceipt, BoxError> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("use the URL-safe attention item id returned by Brain".into());
        }
        rpc_result(
            self.post(&format!("/attention/{id}/responses"), response)
                .await?,
        )
    }

    pub async fn runtime_status(&self) -> Result<RuntimeStatus, BoxError> {
        rpc_result(self.get("/runtime/status").await?)
    }

    /// For independently authenticated instruments only; never registered as a model tool.
    #[allow(dead_code)]
    pub async fn submit_outcome(
        &self,
        input: &anda_brain::consequence::OutcomeInput,
    ) -> Result<anda_brain::consequence::ObservationReceipt, BoxError> {
        rpc_result(self.post("/outcomes", input).await?)
    }

    #[allow(dead_code)]
    pub async fn recall_structured(&self, input: &RecallInput) -> Result<RecallOutput, BoxError> {
        rpc_result(
            self.post_with_timeout("/recall_structured", input, RECALL_TIMEOUT)
                .await?,
        )
    }

    pub async fn formation_conversation(
        &self,
        id: u64,
    ) -> Result<anda_engine::memory::Conversation, BoxError> {
        rpc_result(
            self.get(&format!("/conversations/{id}?collection=formation"))
                .await?,
        )
    }

    pub async fn formation<'a>(
        &self,
        input: FormationInputRef<'a>,
    ) -> Result<AgentOutput, BoxError> {
        rpc_result(self.post("/formation", &input).await?)
    }

    pub async fn recall<'a>(&self, input: RecallInputRef<'a>) -> Result<AgentOutput, BoxError> {
        let started_at = Instant::now();
        let result: Result<RpcResponse<AgentOutput>, BoxError> = self
            .post_with_timeout("/recall", &input, RECALL_TIMEOUT)
            .await;
        let elapsed = started_at.elapsed();
        if elapsed > SLOW_RECALL_WARN_AFTER {
            match &result {
                Ok(_) => log::warn!(
                    "[BrainClient] recall request took {:?}, timeout: {:?}",
                    elapsed,
                    RECALL_TIMEOUT
                ),
                Err(err) => log::warn!(
                    "[BrainClient] recall request failed after {:?}, timeout: {:?}, error: {err}",
                    elapsed,
                    RECALL_TIMEOUT
                ),
            }
        }

        rpc_result(result?)
    }

    pub async fn describe_primer(&self) -> Result<Json, BoxError> {
        let rt = self
            .execute_kip_readonly(KipRequest::single("DESCRIBE PRIMER"))
            .await?;
        single_kip_result(rt)
    }

    pub async fn execute_kip_readonly(&self, request: KipRequest) -> Result<KipResponse, BoxError> {
        self.post("/execute_kip_readonly", &super::http_kip_args(request)?)
            .await
    }

    pub async fn user_info(&self, user: String, name: Option<String>) -> Result<Json, BoxError> {
        let rt: RpcResponse<Json> = self
            .post("/get_or_init_user", &GetOrInitUserInput { user, name })
            .await?;
        rpc_result(rt)
    }

    /// Existing profile only: restricted conversations never initialize a Person.
    pub async fn user_info_readonly(&self, user: String) -> Result<Json, BoxError> {
        let mut request = KipRequest::single(
            "FIND(?person) WHERE {?person CONCEPT {type: \"Person\", key: :key}} LIMIT 1",
        );
        request.parameters = Some(serde_json::Map::from_iter([("key".into(), user.into())]));
        let result = single_kip_result(self.execute_kip_readonly(request).await?)?;
        Ok(result
            .as_array()
            .and_then(|rows| rows.first())
            .cloned()
            .unwrap_or(Json::Null))
    }

    pub async fn brain_status(&self) -> Result<FormationStatus, BoxError> {
        let rt: RpcResponse<FormationStatus> = self.get("/formation_status").await?;
        rpc_result(rt)
    }

    /// Triggers a maintenance cycle. The brain runs the cycle asynchronously
    /// and returns the maintenance conversation id immediately.
    pub async fn maintenance(&self, input: &MaintenanceInput) -> Result<AgentOutput, BoxError> {
        let rt: RpcResponse<AgentOutput> = self.post("/maintenance", input).await?;
        rpc_result(rt)
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
            Err(Box::new(HttpError {
                status,
                message: format!(
                    "[BrainClient] request failed for {} {}: {status}, body: {msg}",
                    method, path
                ),
            }))
        }
    }
}

/// A single operation succeeds only when both envelope levels say so. Partial
/// results are audit data and must never enter the system prompt as a primer.
fn single_kip_result(response: KipResponse) -> Result<Json, BoxError> {
    if response.kip == "2.0"
        && response.status == anda_kip::TopLevelStatus::Succeeded
        && response.error.is_none()
        && response.results.len() == 1
        && response.results[0].status == anda_kip::OperationStatus::Succeeded
        && response.results[0].error.is_none()
        && let Some(result) = response.results[0].result.clone()
    {
        return Ok(result);
    }
    Err(format!(
        "[BrainClient] KIP operation did not succeed: {}",
        serde_json::to_string(&response)?
    )
    .into())
}

impl Tool<BaseCtx> for Client {
    type Args = RecallInput;
    type Output = String;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Recall information from the assistant's long-term memory (the Cognitive Nexus owned by $self). Use only for information that is not already present in the active conversation. Do not call for facts just mentioned or otherwise available in current context. Formation is asynchronous: recall first waits briefly for this conversation's earlier submitted windows and says when some are still being processed.".to_string()
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
                "budget": {
                  "type": ["object", "null"],
                  "description": "Default to null for ordinary natural-language memory recall. Set explicit packet and cumulative input limits when a bounded structured memory packet is needed. Small budgets may return compact partial candidates; inspect coverage and warning items. Required constraints must all fit the packet. A receipt proves delivery only.",
                  "properties": {
                    "tokenizer": {"type":"string", "enum":[anda_brain::recall_budget::TOKENIZER]},
                    "max_tokens": {"type":"integer", "minimum":1, "maximum":65536},
                    "context_tokens": {"type":"integer", "minimum":1, "maximum":131072}
                  },
                  "required":["tokenizer","max_tokens","context_tokens"],
                  "additionalProperties": false
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
                "context",
                "budget"
              ],
              "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        ctx: BaseCtx,
        request: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        use anda_core::StateFeatures;
        if !crate::engine::MemoryPolicy::current(&ctx).may_read() {
            return Err("Memory recall is disabled for this conversation".into());
        }
        // The processing barrier (MI §5): wait, bounded, for this
        // conversation's own observed windows before recalling.
        let namespace = ctx.caller().to_string();
        let conversation = ctx
            .get_state::<super::RecallTurn>()
            .map(|turn| turn.0.lock().conversation)
            .filter(|id| *id > 0);
        let barrier = match (&self.host, &self.journal, conversation) {
            (Some(host), Some(journal), Some(conversation)) => match host
                .memory_barrier(
                    journal,
                    &namespace,
                    conversation,
                    super::memory::RECALL_BARRIER,
                )
                .await
            {
                Ok(barrier) => Some(barrier),
                Err(error) => {
                    log::warn!("recall barrier for conversation {conversation}: {error}");
                    None
                }
            },
            _ => None,
        };
        let rt = self.recall((&request).into()).await?;
        if rt.failed_reason.is_none()
            && let (Some(journal), Some(conversation), Some(barrier)) =
                (&self.journal, conversation, &barrier)
            && !barrier.settled.is_empty()
        {
            let accounted: Vec<&str> = barrier
                .settled
                .iter()
                .map(|p| p.receipt_ref.as_str())
                .collect();
            if let Err(error) = journal
                .update_memory_session(&namespace, &conversation.to_string(), |session| {
                    session.acknowledge_recall(&accounted);
                    Ok(())
                })
                .await
            {
                log::warn!("recall barrier for conversation {conversation}: {error}");
            }
        }
        let note = barrier
            .as_ref()
            .and_then(super::memory::Barrier::note)
            .filter(|_| request.budget.is_none());
        let with_note = |mut output: ToolOutput<String>| {
            if let Some(note) = &note {
                output.output = format!("Note: {note}\n\n{}", output.output);
            }
            output
        };
        if let (Some(host), Some(journal)) = (&self.host, &self.journal) {
            let receipt = match rt.conversation {
                Some(id) => host.recall_receipt(id).await,
                None => Ok(None),
            };
            let (conversation, turn, tool_call) = ctx
                .get_state::<super::RecallTurn>()
                .map(|trace| trace.identify(&request))
                .unwrap_or_default();
            let delivery = super::RecallDelivery {
                invocation: ic_auth_types::Xid::new().to_string(),
                caller: ctx.caller().to_string(),
                bot_conversation: conversation,
                bot_turn: turn,
                tool_call,
                brain_conversation: rt.conversation,
                receipt: receipt.as_ref().ok().cloned().flatten(),
                delivered_at: anda_engine::unix_ms(),
                failed: rt.failed_reason.is_some(),
                usage: rt.usage.clone(),
                tools_usage: rt.tools_usage.clone(),
                accounting_complete: false,
            };
            let persisted = journal.record_recall(&delivery).await;
            if let Err(err) = receipt.map(|_| ()).and(persisted) {
                // Retain measured usage even when host evidence persistence fails.
                let mut output = recall_tool_output(rt, request.budget.is_some());
                output.is_error = Some(true);
                output.output = format!(
                    "Recall delivery could not be recorded: {}",
                    err.to_string().chars().take(256).collect::<String>()
                );
                return Ok(output);
            }
        }
        Ok(with_note(recall_tool_output(rt, request.budget.is_some())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;
    use crate::util::json_schema::assert_openai_strict_parameters;

    #[test]
    fn recall_failure_and_nested_usage_survive_the_tool_boundary() {
        let mut output = AgentOutput {
            content: "partial internal diagnostics".into(),
            failed_reason: Some("failure".repeat(2000)),
            usage: anda_core::Usage {
                input_tokens: 41,
                output_tokens: 7,
                requests: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        output.tools_usage.insert(
            "nested".into(),
            anda_core::Usage {
                input_tokens: 12,
                requests: 1,
                ..Default::default()
            },
        );
        let result = recall_tool_output(output, false);
        assert_eq!(result.is_error, Some(true));
        assert!(!result.output.contains("partial internal"));
        assert!(result.output.len() < 600);
        assert_eq!(result.usage.input_tokens, 41);
        assert_eq!(result.usage.output_tokens, 7);
        assert_eq!(result.tools_usage["nested"].input_tokens, 12);
        let packet = r#"{"format":"anda-brain-recall/1","status":"insufficient"}"#;
        let result = recall_tool_output(
            AgentOutput {
                content: packet.into(),
                failed_reason: Some("insufficient".into()),
                ..Default::default()
            },
            true,
        );
        assert_eq!(result.output, packet);
        assert_eq!(result.is_error, Some(true));
        assert!(result.artifacts.is_empty());
    }

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

    #[test]
    fn primer_rejects_partial_failed_and_missing_operation_results() {
        let payload = json!({"cognitive_identity": {"key": "$self"}});
        assert_eq!(
            single_kip_result(KipResponse::ok(payload.clone())).unwrap(),
            payload
        );
        let mut response = KipResponse::ok(payload.clone());
        response.results[0].status = anda_kip::OperationStatus::Failed;
        assert!(single_kip_result(response).is_err());
        let mut response = KipResponse::ok(payload.clone());
        response.status = anda_kip::TopLevelStatus::Partial;
        assert!(single_kip_result(response).is_err());
        let mut response = KipResponse::ok(payload.clone());
        response.results[0].error =
            Some(anda_kip::KipError::internal_error("partial result").into());
        assert!(single_kip_result(response).is_err());
        assert!(single_kip_result(KipResponse::default()).is_err());
        let mut response = KipResponse::ok(payload);
        response.results[0].result = None;
        assert!(single_kip_result(response).is_err());
    }

    #[tokio::test]
    async fn user_info_rejects_rpc_error_instead_of_injecting_it_as_a_profile() {
        let app = Router::new().route(
            "/v1/anda_bot/get_or_init_user",
            routing::post(|| async {
                axum::Json(json!({"error": {"code": 500, "message": "profile unavailable"}, "result": {"name": "partial profile must not be used"}}))
            }),
        );
        let client = Client::new(spawn_brain_mock(app).await, None);
        assert!(
            client
                .user_info("alice".into(), None)
                .await
                .unwrap_err()
                .to_string()
                .contains("profile unavailable")
        );
    }

    async fn spawn_brain_mock(app: Router) -> String {
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/v1/anda_bot")
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
                "maintenance_at": {"daydream": 0, "full": 0, "quick": 0, "start_at": 1700000000000u64},
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
        assert_eq!(status.maintenance_at.start_at, 1700000000000);

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
                budget: &None,
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
                budget: &None,
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
                assert!(body.get("kip").is_none());
                assert_eq!(body["operations"][0]["command"], "DESCRIBE PRIMER");
                assert!(body.get("command").is_none());
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
                    serde_json::to_value(KipResponse::failed(anda_kip::KipError::internal_error(
                        "nexus unavailable",
                    )))
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
                axum::Json(json!({"result": {"user": body["user"], "trust": "high"}}))
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
    async fn maintenance_posts_input_and_unwraps_output() {
        let app = Router::new().route(
            "/v1/anda_bot/maintenance",
            routing::post(|axum::Json(body): axum::Json<Value>| async move {
                assert_eq!(body["trigger"], "scheduled");
                assert_eq!(body["scope"], "full");
                axum::Json(json!({
                    "result": serde_json::to_value(AgentOutput {
                        conversation: Some(7),
                        ..Default::default()
                    })
                    .unwrap()
                }))
            }),
        );
        let base_url = spawn_brain_mock(app).await;
        let client = Client::new(base_url, None);

        let output = client
            .maintenance(&MaintenanceInput {
                trigger: "scheduled".to_string(),
                scope: MaintenanceScope::Full,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(output.conversation, Some(7));
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
                    budget: None,
                    query: "What is the project status?".to_string(),
                    context: None,
                },
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.output, "memory found");
    }

    #[tokio::test]
    async fn recall_and_maintenance_do_not_replay_received_errors() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for status in [
            http::StatusCode::SERVICE_UNAVAILABLE,
            http::StatusCode::GATEWAY_TIMEOUT,
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let seen = calls.clone();
            let app = Router::new().fallback(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                async move { (status, "result unavailable") }
            });
            let client = Client::new(spawn_brain_mock(app).await, None);
            assert!(
                client
                    .recall(RecallInputRef {
                        query: "earlier preference",
                        context: &None,
                        budget: &None
                    })
                    .await
                    .is_err()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(
                client
                    .maintenance(&MaintenanceInput::default())
                    .await
                    .is_err()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }
    }

    #[tokio::test]
    async fn maintenance_rejects_rpc_error_with_partial_result() {
        let app = Router::new().route("/v1/anda_bot/maintenance", routing::post(|| async {
            axum::Json(json!({"result": AgentOutput::default(), "error":{"message":"maintenance rejected"}}))
        }));
        let client = Client::new(spawn_brain_mock(app).await, None);
        let error = client
            .maintenance(&MaintenanceInput::default())
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<RpcFailure>().is_some());
        assert!(error.to_string().contains("maintenance rejected"));
    }
}
