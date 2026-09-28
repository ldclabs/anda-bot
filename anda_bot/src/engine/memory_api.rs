//! Authenticated control-plane API, separate from model-callable tools.
//!
//! The HTTP routes and the WebSocket methods decode into one [`MemoryRequest`]
//! and share its authorization, execution and response shape.
use anda_core::{BoxError, Principal};
use anda_engine_server::handler::AppState;
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    brain::{
        MemoryService,
        activity::ActivityQuery,
        catalog::RecordQuery,
        mutation::{ChangeRequest, CommitRequest},
        product::{SearchRequest, WatchQuery, WatchRequest},
    },
    util::tool_response::{ToolError, ToolResponse},
};

#[derive(Clone)]
pub(crate) struct MemoryApiState {
    pub app: AppState,
    pub owner: Principal,
    pub service: MemoryService,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewQuery {}

pub(crate) fn error(code: &str, message: &str) -> ToolResponse {
    ToolResponse::Err {
        error: ToolError {
            code: code.into(),
            message: message.into(),
            data: Some(json!({"reason":code})),
            ..Default::default()
        },
        result: None,
    }
}

type Reply = (StatusCode, ToolResponse);

/// One memory operation, whichever transport carried it.
enum MemoryRequest {
    Overview,
    Activity(ActivityQuery),
    Records(RecordQuery),
    Record(String),
    Search(SearchRequest),
    Watches(WatchQuery),
    Watch(WatchRequest),
    CancelWatch(String),
    SetupPrepare,
    SetupCommit(CommitRequest),
    ChangePrepare(ChangeRequest),
    ChangeCommit(String, CommitRequest),
    ChangeStatus(String),
    ChangeDiscard(String),
}

// The WebSocket reply carries the shared ToolResponse error contract.
#[allow(clippy::result_large_err)]
impl MemoryRequest {
    fn from_websocket(method: &str, params: Value) -> Result<Self, ToolResponse> {
        fn one<T: DeserializeOwned>(params: Value, message: &str) -> Result<T, ToolResponse> {
            serde_json::from_value::<(T,)>(params)
                .map(|(value,)| value)
                .map_err(|_| error("invalid_request", message))
        }
        fn none(params: &Value, message: &str) -> Result<(), ToolResponse> {
            if params == &json!([]) {
                Ok(())
            } else {
                Err(error("invalid_request", message))
            }
        }
        Ok(match method {
            "memory_overview" => {
                none(
                    &params,
                    "memory_overview requires an empty parameter array.",
                )?;
                Self::Overview
            }
            "memory_activity" => {
                Self::Activity(one(params, "memory_activity requires one query object.")?)
            }
            "memory_records" => {
                Self::Records(one(params, "memory_records requires one query object.")?)
            }
            "memory_record" => Self::Record(one(params, "memory_record requires one record id.")?),
            "memory_search" => Self::Search(one(params, "Expected one search request.")?),
            "memory_watches" if params == json!([]) => Self::Watches(WatchQuery::default()),
            "memory_watches" => Self::Watches(one(params, "Expected one watch query.")?),
            "memory_watch" => Self::Watch(one(params, "Expected one record watch request.")?),
            "memory_watch_cancel" => {
                Self::CancelWatch(one(params, "Expected one watch operation id.")?)
            }
            "memory_inbox_setup_prepare" => {
                none(&params, "Setup preview accepts no parameters.")?;
                Self::SetupPrepare
            }
            "memory_inbox_setup_commit" => {
                Self::SetupCommit(one(params, "Expected the setup preview digest.")?)
            }
            "memory_change_prepare" => {
                Self::ChangePrepare(one(params, "Expected one change request.")?)
            }
            "memory_change_commit" => {
                let (id, request) = serde_json::from_value::<(String, CommitRequest)>(params)
                    .map_err(|_| {
                        error(
                            "invalid_request",
                            "Expected an operation id and preview digest.",
                        )
                    })?;
                Self::ChangeCommit(id, request)
            }
            "memory_change_status" => {
                Self::ChangeStatus(one(params, "Expected one operation id.")?)
            }
            "memory_change_discard" => {
                Self::ChangeDiscard(one(params, "Expected one operation id.")?)
            }
            _ => {
                return Err(error(
                    "unsupported_capability",
                    "This memory method is unavailable.",
                ));
            }
        })
    }
}

impl MemoryApiState {
    pub async fn websocket_dispatch(
        &self,
        headers: &HeaderMap,
        method: &str,
        params: Value,
    ) -> Value {
        let (caller, token) = match self.authenticate(headers) {
            Ok(identity) => identity,
            Err((_, error)) => return json!(error),
        };
        if serde_json::to_vec(&params).map_or(true, |bytes| bytes.len() > 64 * 1024) {
            return json!(error(
                "payload_too_large",
                "Memory requests are limited to 64 KiB."
            ));
        }
        if method != "memory_activity"
            && let Err((_, error)) = self.authorize_caller(caller)
        {
            return json!(error);
        }
        match MemoryRequest::from_websocket(method, params) {
            Ok(request) => json!(self.execute(caller, token, request).await.1),
            Err(error) => json!(error),
        }
    }

    /// Runs an authorized request. Activity is scoped to the caller's own
    /// conversations; every other operation acts for the owner.
    async fn execute(&self, caller: Principal, token: String, request: MemoryRequest) -> Reply {
        let owner = self.owner;
        let service = &self.service;
        match request {
            MemoryRequest::Overview => {
                let mut overview = service.overview(token).await;
                overview.caller = Some(owner.to_string());
                ok(json!(overview), None)
            }
            MemoryRequest::Activity(query) => self.activity(caller, query).await,
            MemoryRequest::Records(query) => match service.records(owner, query).await {
                Ok((page, next_cursor)) => ok(json!(page), next_cursor),
                Err(error) => service_error(error),
            },
            MemoryRequest::Record(id) => respond(
                service
                    .record(owner, &id)
                    .await
                    .map(|record| json!({"schema_version":1,"record":record})),
            ),
            MemoryRequest::Search(request) => respond(service.search(owner, token, request).await),
            MemoryRequest::Watches(query) => match service.watches(owner, query).await {
                Ok(mut result) => {
                    let next_cursor = result.next_cursor.take();
                    ok(json!(result), next_cursor)
                }
                Err(error) => service_error(error),
            },
            MemoryRequest::Watch(request) => watch_reply(service.watch(owner, request).await),
            MemoryRequest::CancelWatch(id) => watch_reply(service.cancel_watch(owner, id).await),
            MemoryRequest::SetupPrepare => respond(service.setup_preview(owner).await),
            MemoryRequest::SetupCommit(request) => {
                respond(service.setup_commit(owner, &request.preview_digest).await)
            }
            MemoryRequest::ChangePrepare(request) => {
                respond(service.prepare_change(owner, request).await)
            }
            MemoryRequest::ChangeCommit(id, request) => {
                respond(service.commit_change(owner, id, request).await)
            }
            MemoryRequest::ChangeStatus(id) => respond(service.change(owner, &id).await),
            MemoryRequest::ChangeDiscard(id) => respond(
                service
                    .discard_change(owner, &id)
                    .await
                    .map(|()| json!({"schema_version":1,"discarded":true})),
            ),
        }
    }

    #[allow(clippy::result_large_err)] // Preserve the shared ToolResponse error contract.
    fn authenticate(&self, headers: &HeaderMap) -> Result<(Principal, String), Reply> {
        let caller = self
            .app
            .verify_user(headers, anda_engine::unix_ms(), None, None)
            .ok()
            .filter(|p| *p != Principal::anonymous())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    error("unauthorized", "A valid owner bearer is required."),
                )
            })?;
        headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .filter(|s| !s.is_empty())
            .map(|token| (caller, token.to_string()))
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    error("unauthorized", "A bearer token is required."),
                )
            })
    }

    #[allow(clippy::result_large_err)] // Preserve the shared ToolResponse error contract.
    fn authorize(&self, headers: &HeaderMap) -> Result<(Principal, String), Reply> {
        let (caller, token) = self.authenticate(headers)?;
        self.authorize_caller(caller)?;
        Ok((caller, token))
    }

    #[allow(clippy::result_large_err)]
    fn authorize_caller(&self, caller: Principal) -> Result<(), Reply> {
        if caller != self.owner {
            return Err((
                StatusCode::FORBIDDEN,
                error(
                    "forbidden",
                    "Only the local owner can access this memory operation.",
                ),
            ));
        }
        Ok(())
    }

    async fn activity(&self, caller: Principal, query: ActivityQuery) -> Reply {
        if query.conversation.is_none() && caller != self.owner {
            return (
                StatusCode::FORBIDDEN,
                error("forbidden", "Select one of your conversations."),
            );
        }
        match self.service.activity(caller, query).await {
            Ok(mut page) => {
                let cursor = page.next_cursor.take();
                ok(json!(page), cursor)
            }
            // Activity reads conversations, so its messages say so.
            Err(err) => match err.to_string().as_str() {
                "payload_too_large" => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    error("payload_too_large", "Query exceeds 8192 UTF-8 bytes."),
                ),
                "search_result_unknown" => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    error(
                        "search_result_unknown",
                        "No complete search result was received. A new search may incur another model charge.",
                    ),
                ),
                "invalid_request" => (
                    StatusCode::BAD_REQUEST,
                    error("invalid_request", "Invalid conversation or page limit."),
                ),
                "invalid_cursor" => (
                    StatusCode::CONFLICT,
                    error("invalid_cursor", "Refresh from the first page."),
                ),
                "not_found" => (
                    StatusCode::NOT_FOUND,
                    error("not_found", "Conversation not found."),
                ),
                "unsupported_capability" => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    error(
                        "unsupported_capability",
                        "Activity is unavailable in this server.",
                    ),
                ),
                _ => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    error("service_unavailable", "Memory activity could not be read."),
                ),
            },
        }
    }

    pub fn into_router(self) -> Router {
        Router::new()
            .route("/daemon/memory/v1/overview", routing::get(overview))
            .route("/daemon/memory/v1/search", routing::post(search))
            .route("/daemon/memory/v1/activity", routing::get(activity))
            .route("/daemon/memory/v1/records", routing::get(records))
            .route("/daemon/memory/v1/records/{id}", routing::get(record))
            .route(
                "/daemon/memory/v1/watches",
                routing::post(create_watch).get(list_watches),
            )
            .route(
                "/daemon/memory/v1/watches/{id}/cancel",
                routing::post(cancel_watch),
            )
            .route(
                "/daemon/memory/v1/inbox/setup/prepare",
                routing::post(setup_preview),
            )
            .route(
                "/daemon/memory/v1/inbox/setup/commit",
                routing::post(setup_commit),
            )
            .route(
                "/daemon/memory/v1/changes/prepare",
                routing::post(prepare_change),
            )
            .route(
                "/daemon/memory/v1/changes/{id}",
                routing::get(change_status),
            )
            .route(
                "/daemon/memory/v1/changes/{id}/commit",
                routing::post(commit_change),
            )
            .route(
                "/daemon/memory/v1/changes/{id}/discard",
                routing::post(discard_change),
            )
            .layer(DefaultBodyLimit::max(64 * 1024))
            .with_state(self)
    }
}

fn ok(result: Value, next_cursor: Option<String>) -> Reply {
    (
        StatusCode::OK,
        ToolResponse::Ok {
            result,
            next_cursor,
        },
    )
}

fn respond<T: Serialize>(result: Result<T, BoxError>) -> Reply {
    match result {
        Ok(value) => ok(json!(value), None),
        Err(error) => service_error(error),
    }
}

fn watch_reply(result: Result<anda_brain::runtime_api::RecordWatch, BoxError>) -> Reply {
    respond(result.map(|watch| json!({"schema_version":1,"watch":watch})))
}

fn service_error(err: BoxError) -> Reply {
    if let Some(native) = err.downcast_ref::<anda_brain::runtime_api::RuntimeError>() {
        use anda_brain::runtime_api::RuntimeError;
        let (status, code) = match native {
            RuntimeError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            RuntimeError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            RuntimeError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            RuntimeError::Conflict(_) => (StatusCode::CONFLICT, "revision_conflict"),
            RuntimeError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            _ => (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
        };
        return (
            status,
            error(
                code,
                "The native memory runtime could not authorize or complete this request.",
            ),
        );
    }
    match err.to_string().as_str() {
        "external_config_override" => (
            StatusCode::CONFLICT,
            error(
                "external_config_override",
                "BRAIN_RUNTIME_CONFIG overrides config.yaml. Update that deployment configuration explicitly.",
            ),
        ),
        "custom_runtime_config" | "runtime_file_exists" => (
            StatusCode::CONFLICT,
            error(
                "custom_runtime_config",
                "Existing runtime configuration was preserved. Merge the inbox configuration explicitly.",
            ),
        ),
        "revision_conflict" | "idempotency_conflict" | "preview_expired" => (
            StatusCode::CONFLICT,
            error(
                "revision_conflict",
                "The preview changed or expired. Review a new preview before confirming.",
            ),
        ),
        "unsupported_scope" | "unsupported_correction" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            error(
                "unsupported_scope",
                "The complete source or change scope could not be verified.",
            ),
        ),
        "memory_change_pending" => (
            StatusCode::CONFLICT,
            error(
                "memory_change_pending",
                "Another memory change is still reconciling.",
            ),
        ),
        "capacity" => (
            StatusCode::TOO_MANY_REQUESTS,
            error("capacity", "Too many memory changes are pending."),
        ),
        "payload_too_large" => (
            StatusCode::PAYLOAD_TOO_LARGE,
            error("payload_too_large", "Query exceeds 8192 UTF-8 bytes."),
        ),
        "search_result_unknown" => (
            StatusCode::SERVICE_UNAVAILABLE,
            error(
                "search_result_unknown",
                "No complete search result was received. A new search may incur another model charge.",
            ),
        ),
        "invalid_request" => (
            StatusCode::BAD_REQUEST,
            error("invalid_request", "Invalid memory request."),
        ),
        "invalid_cursor" => (
            StatusCode::CONFLICT,
            error("invalid_cursor", "Refresh from the first page."),
        ),
        "not_found" => (
            StatusCode::NOT_FOUND,
            error("not_found", "Memory record not found."),
        ),
        "unsupported_capability" => (
            StatusCode::SERVICE_UNAVAILABLE,
            error(
                "unsupported_capability",
                "This memory capability is not installed.",
            ),
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            error("service_unavailable", "Memory data could not be read."),
        ),
    }
}

fn reply((status, body): Reply) -> Response {
    (status, Json(body)).into_response()
}

fn invalid(message: &str) -> Response {
    reply((StatusCode::BAD_REQUEST, error("invalid_request", message)))
}

fn json_rejection(rejection: JsonRejection) -> Response {
    let (status, reason) = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
    } else {
        (StatusCode::BAD_REQUEST, "invalid_request")
    };
    reply((
        status,
        error(reason, "Invalid or oversized memory request."),
    ))
}

/// Serves one owner-only HTTP route: authorization first, then the input the
/// route's extractors produced.
async fn owner_route(
    state: &MemoryApiState,
    headers: &HeaderMap,
    request: Result<MemoryRequest, Response>,
) -> Response {
    let (caller, token) = match state.authorize(headers) {
        Ok(identity) => identity,
        Err(rejected) => return reply(rejected),
    };
    match request {
        Ok(request) => reply(state.execute(caller, token, request).await),
        Err(response) => response,
    }
}

async fn overview(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<OverviewQuery>, QueryRejection>,
) -> Response {
    let request = query
        .map(|_| MemoryRequest::Overview)
        .map_err(|_| invalid("The overview accepts no query parameters."));
    owner_route(&state, &headers, request).await
}

async fn search(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<SearchRequest>, JsonRejection>,
) -> Response {
    let request = request
        .map(|Json(request)| MemoryRequest::Search(request))
        .map_err(json_rejection);
    owner_route(&state, &headers, request).await
}

async fn activity(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<ActivityQuery>, QueryRejection>,
) -> Response {
    let (caller, token) = match state.authenticate(&headers) {
        Ok(identity) => identity,
        Err(rejected) => return reply(rejected),
    };
    match query {
        Ok(Query(query)) => reply(
            state
                .execute(caller, token, MemoryRequest::Activity(query))
                .await,
        ),
        Err(_) => invalid("Invalid activity query."),
    }
}

async fn records(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<RecordQuery>, QueryRejection>,
) -> Response {
    let request = query
        .map(|Query(query)| MemoryRequest::Records(query))
        .map_err(|_| invalid("Invalid memory query."));
    owner_route(&state, &headers, request).await
}

async fn record(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    owner_route(&state, &headers, Ok(MemoryRequest::Record(id))).await
}

async fn create_watch(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<WatchRequest>, JsonRejection>,
) -> Response {
    let request = request
        .map(|Json(request)| MemoryRequest::Watch(request))
        .map_err(|_| invalid("Invalid record watch request."));
    owner_route(&state, &headers, request).await
}

async fn list_watches(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Response {
    let request = query
        .map(|Query(query)| MemoryRequest::Watches(query))
        .map_err(|_| invalid("Invalid watch query."));
    owner_route(&state, &headers, request).await
}

async fn cancel_watch(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    owner_route(&state, &headers, Ok(MemoryRequest::CancelWatch(id))).await
}

async fn setup_preview(State(state): State<MemoryApiState>, headers: HeaderMap) -> Response {
    owner_route(&state, &headers, Ok(MemoryRequest::SetupPrepare)).await
}

async fn setup_commit(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<CommitRequest>, JsonRejection>,
) -> Response {
    let request = request
        .map(|Json(request)| MemoryRequest::SetupCommit(request))
        .map_err(json_rejection);
    owner_route(&state, &headers, request).await
}

async fn prepare_change(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<ChangeRequest>, JsonRejection>,
) -> Response {
    let request = request
        .map(|Json(request)| MemoryRequest::ChangePrepare(request))
        .map_err(json_rejection);
    owner_route(&state, &headers, request).await
}

async fn commit_change(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Result<Json<CommitRequest>, JsonRejection>,
) -> Response {
    let request = request
        .map(|Json(request)| MemoryRequest::ChangeCommit(id, request))
        .map_err(json_rejection);
    owner_route(&state, &headers, request).await
}

async fn change_status(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    owner_route(&state, &headers, Ok(MemoryRequest::ChangeStatus(id))).await
}

async fn discard_change(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    owner_route(&state, &headers, Ok(MemoryRequest::ChangeDiscard(id))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{brain::Client, identity::Ed25519Key};
    use std::{collections::BTreeMap, sync::Arc, time::Duration};

    fn fixture() -> (MemoryApiState, Ed25519Key, Ed25519Key) {
        let owner = Ed25519Key::new([71; 32]);
        let other = Ed25519Key::new([72; 32]);
        let app = AppState {
            engines: Arc::new(BTreeMap::new()),
            default_engine: owner.id(),
            start_time_ms: 0,
            extra_info: Arc::new(BTreeMap::new()),
            ed25519_pubkeys: Arc::new(vec![owner.pubkey().into(), other.pubkey().into()]),
        };
        (
            MemoryApiState {
                app,
                owner: owner.id(),
                service: MemoryService::new(Client::new("http://127.0.0.1:0".into(), None)),
            },
            owner,
            other,
        )
    }
    fn headers(key: &Ed25519Key, expired: bool) -> HeaderMap {
        let mut claims = crate::identity::expiring_claims(Duration::from_secs(60)).unwrap();
        if expired {
            claims.issued_at = Some(1i64.into());
            claims.expiration = Some(2i64.into());
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Bearer {}", key.sign_cwt(claims).unwrap())
                .parse()
                .unwrap(),
        );
        headers
    }
    #[tokio::test]
    async fn memory_api_ws_checks_identity_before_payload_limits_and_search_budget() {
        let (state, owner, _) = fixture();
        let oversized = json!([{"query":"x".repeat(70*1024)}]);
        assert_eq!(
            state
                .websocket_dispatch(&HeaderMap::new(), "memory_search", oversized.clone())
                .await["error"]["code"],
            "unauthorized"
        );
        assert_eq!(
            state
                .websocket_dispatch(&headers(&owner, false), "memory_search", oversized)
                .await["error"]["code"],
            "payload_too_large"
        );
        assert_eq!(
            state
                .websocket_dispatch(
                    &headers(&owner, false),
                    "memory_search",
                    json!([{"query":"x","budget":{"tokenizer":"unknown"}}])
                )
                .await["error"]["code"],
            "invalid_request"
        );
    }
    #[tokio::test]
    async fn memory_api_checks_owner_and_expiration_on_every_ws_request() {
        let (state, owner, other) = fixture();
        assert!(state.authorize(&headers(&owner, false)).is_ok());
        assert_eq!(
            state.authorize(&HeaderMap::new()).unwrap_err().0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            state.authorize(&headers(&other, false)).unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            state
                .websocket_dispatch(&headers(&owner, true), "memory_overview", json!([]))
                .await["error"]["code"],
            "unauthorized"
        );
        assert_eq!(
            state
                .websocket_dispatch(
                    &headers(&owner, false),
                    "memory_overview",
                    json!([{"caller":"forged"}])
                )
                .await["error"]["code"],
            "invalid_request"
        );
    }
    #[tokio::test]
    async fn memory_api_http_matches_ws_and_rejects_extra_query_fields() {
        let (state, owner, other) = fixture();
        let url = crate::test_support::spawn_http_mock(state.into_router()).await;
        let client = crate::util::http_client::new_reqwest_client();
        for (headers, status) in [
            (HeaderMap::new(), 401),
            (headers(&other, false), 403),
            (headers(&owner, true), 401),
        ] {
            let response = client
                .get(format!("{url}/daemon/memory/v1/overview"))
                .headers(headers)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert!(response.json::<Value>().await.unwrap()["error"]["code"].is_string());
        }
        let response = client
            .get(format!("{url}/daemon/memory/v1/overview?caller=forged"))
            .headers(headers(&owner, false))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn memory_search_preserves_validation_and_unknown_result_errors() {
        let (mut state, owner, _) = fixture();
        let brain = axum::Router::new().route(
            "/recall_structured",
            axum::routing::post(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"accepted work result unavailable"})),
                )
            }),
        );
        let brain_url = crate::test_support::spawn_http_mock(brain).await;
        state.service = MemoryService::new(Client::new(brain_url, None));
        let auth = headers(&owner, false);
        assert_eq!(
            state
                .websocket_dispatch(&auth, "memory_search", json!([{"query":"x"}]))
                .await["error"]["code"],
            "search_result_unknown"
        );
        assert_eq!(
            state
                .websocket_dispatch(&auth, "memory_search", json!([{"query":"中".repeat(3000)}]))
                .await["error"]["code"],
            "payload_too_large"
        );

        let api_url = crate::test_support::spawn_http_mock(state.into_router()).await;
        let response = crate::util::http_client::new_reqwest_client()
            .post(format!("{api_url}/daemon/memory/v1/search"))
            .headers(auth)
            .json(&json!({"query":"中".repeat(3000)}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            "payload_too_large"
        );
    }
}
