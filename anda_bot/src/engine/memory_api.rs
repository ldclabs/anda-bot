//! Authenticated control-plane API, separate from model-callable tools.
use anda_core::Principal;
use anda_engine_server::handler::AppState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    brain::{
        MemoryService,
        activity::ActivityQuery,
        catalog::RecordQuery,
        mutation::{ChangeRequest, CommitRequest},
        product::{SearchRequest, WatchRequest},
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

impl MemoryApiState {
    pub async fn websocket_dispatch(
        &self,
        headers: &HeaderMap,
        method: &str,
        params: Value,
    ) -> Value {
        if let Err((_, error)) = self.authenticate(headers) {
            return json!(error);
        }
        if serde_json::to_vec(&params).map_or(true, |bytes| bytes.len() > 64 * 1024) {
            return json!(error(
                "payload_too_large",
                "Memory requests are limited to 64 KiB."
            ));
        }
        match method {
            "memory_overview" => self.websocket(headers, params).await,
            "memory_activity" => self.websocket_activity(headers, params).await,
            "memory_records" => self.websocket_records(headers, params).await,
            "memory_record" => self.websocket_record(headers, params).await,
            "memory_search" => self.websocket_search(headers, params).await,
            "memory_watches" => self.websocket_watches(headers, params).await,
            "memory_watch" | "memory_watch_cancel" => {
                self.websocket_watch(headers, method, params).await
            }
            "memory_inbox_setup_prepare" | "memory_inbox_setup_commit" => {
                self.websocket_setup(headers, method, params).await
            }
            "memory_change_prepare"
            | "memory_change_commit"
            | "memory_change_status"
            | "memory_change_discard" => self.websocket_change(headers, method, params).await,
            _ => json!(error(
                "unsupported_capability",
                "This memory method is unavailable."
            )),
        }
    }
    pub async fn websocket_watches(&self, headers: &HeaderMap, params: Value) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        if params != json!([]) {
            return json!(error("invalid_request", "Expected no parameters."));
        }
        json!(self.watch_list().await.1)
    }
    async fn watch_list(&self) -> (StatusCode, ToolResponse) {
        match self.service.watches(self.owner).await {
            Ok(result) => (
                StatusCode::OK,
                ToolResponse::Ok {
                    result,
                    next_cursor: None,
                },
            ),
            Err(error) => service_error(error),
        }
    }

    async fn search(
        &self,
        headers: &HeaderMap,
        request: SearchRequest,
    ) -> (StatusCode, ToolResponse) {
        let token = match self.authorize(headers) {
            Ok(token) => token,
            Err(error) => return error,
        };
        match self.service.search(self.owner, token, request).await {
            Ok(result) => (
                StatusCode::OK,
                ToolResponse::Ok {
                    result: json!(result),
                    next_cursor: None,
                },
            ),
            Err(error) => service_error(error),
        }
    }
    pub async fn websocket_search(&self, headers: &HeaderMap, params: Value) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        let (request,) = match serde_json::from_value::<(SearchRequest,)>(params) {
            Ok(request) => request,
            Err(_) => return json!(error("invalid_request", "Expected one search request.")),
        };
        json!(self.search(headers, request).await.1)
    }

    pub async fn websocket_watch(&self, headers: &HeaderMap, method: &str, params: Value) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        let result = if method == "memory_watch" {
            match serde_json::from_value::<(WatchRequest,)>(params) {
                Ok((request,)) => self.service.watch(self.owner, request).await,
                Err(_) => {
                    return json!(error(
                        "invalid_request",
                        "Expected one record watch request."
                    ));
                }
            }
        } else {
            match serde_json::from_value::<(String,)>(params) {
                Ok((id,)) => self.service.cancel_watch(self.owner, id).await,
                Err(_) => {
                    return json!(error("invalid_request", "Expected one watch operation id."));
                }
            }
        };
        json!(match result {
            Ok(watch) => ToolResponse::Ok {
                result: json!({"schema_version":1,"watch":watch}),
                next_cursor: None
            },
            Err(error) => service_error(error).1,
        })
    }
    pub async fn websocket_setup(&self, headers: &HeaderMap, method: &str, params: Value) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        let result = if method == "memory_inbox_setup_prepare" {
            if params != json!([]) {
                return json!(error(
                    "invalid_request",
                    "Setup preview accepts no parameters."
                ));
            }
            self.service.setup_preview(self.owner).await
        } else {
            let (request,) = match serde_json::from_value::<(CommitRequest,)>(params) {
                Ok(request) => request,
                Err(_) => {
                    return json!(error(
                        "invalid_request",
                        "Expected the setup preview digest."
                    ));
                }
            };
            self.service
                .setup_commit(self.owner, &request.preview_digest)
                .await
        };
        json!(match result {
            Ok(view) => ToolResponse::Ok {
                result: json!(view),
                next_cursor: None
            },
            Err(error) => service_error(error).1,
        })
    }
    pub async fn websocket_change(
        &self,
        headers: &HeaderMap,
        method: &str,
        params: Value,
    ) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        if method == "memory_change_discard" {
            let (id,) = match serde_json::from_value::<(String,)>(params) {
                Ok(id) => id,
                Err(_) => return json!(error("invalid_request", "Expected one operation id.")),
            };
            return json!(match self.service.discard_change(self.owner, &id).await {
                Ok(()) => ToolResponse::Ok {
                    result: json!({"schema_version":1,"discarded":true}),
                    next_cursor: None
                },
                Err(error) => service_error(error).1,
            });
        }
        let result = match method {
            "memory_change_prepare" => match serde_json::from_value::<(ChangeRequest,)>(params) {
                Ok((request,)) => self.service.prepare_change(self.owner, request).await,
                Err(_) => return json!(error("invalid_request", "Expected one change request.")),
            },
            "memory_change_commit" => {
                match serde_json::from_value::<(String, CommitRequest)>(params) {
                    Ok((id, request)) => self.service.commit_change(self.owner, id, request).await,
                    Err(_) => {
                        return json!(error(
                            "invalid_request",
                            "Expected an operation id and preview digest."
                        ));
                    }
                }
            }
            "memory_change_status" => match serde_json::from_value::<(String,)>(params) {
                Ok((id,)) => self.service.change(self.owner, &id).await,
                Err(_) => return json!(error("invalid_request", "Expected one operation id.")),
            },
            _ => return json!(error("invalid_request", "Unknown memory change operation.")),
        };
        json!(match result {
            Ok(view) => ToolResponse::Ok {
                result: json!(view),
                next_cursor: None
            },
            Err(err) => service_error(err).1,
        })
    }
    async fn records(&self, headers: &HeaderMap, query: RecordQuery) -> (StatusCode, ToolResponse) {
        if let Err(error) = self.authorize(headers) {
            return error;
        }
        match self.service.records(self.owner, query).await {
            Ok((page, next_cursor)) => (
                StatusCode::OK,
                ToolResponse::Ok {
                    result: json!(page),
                    next_cursor,
                },
            ),
            Err(error) => service_error(error),
        }
    }

    async fn record(&self, headers: &HeaderMap, id: &str) -> (StatusCode, ToolResponse) {
        if let Err(error) = self.authorize(headers) {
            return error;
        }
        match self.service.record(self.owner, id).await {
            Ok(record) => (
                StatusCode::OK,
                ToolResponse::Ok {
                    result: json!({"schema_version":1,"record":record}),
                    next_cursor: None,
                },
            ),
            Err(error) => service_error(error),
        }
    }

    pub async fn websocket_records(&self, headers: &HeaderMap, params: Value) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        let (query,) = match serde_json::from_value::<(RecordQuery,)>(params) {
            Ok(query) => query,
            Err(_) => {
                return json!(error(
                    "invalid_request",
                    "memory_records requires one query object."
                ));
            }
        };
        json!(self.records(headers, query).await.1)
    }

    pub async fn websocket_record(&self, headers: &HeaderMap, params: Value) -> Value {
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        let (id,) = match serde_json::from_value::<(String,)>(params) {
            Ok(id) => id,
            Err(_) => {
                return json!(error(
                    "invalid_request",
                    "memory_record requires one record id."
                ));
            }
        };
        json!(self.record(headers, &id).await.1)
    }
    #[allow(clippy::result_large_err)] // Preserve the shared ToolResponse error contract.
    fn authenticate(
        &self,
        headers: &HeaderMap,
    ) -> Result<(Principal, String), (StatusCode, ToolResponse)> {
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
    fn authorize(&self, headers: &HeaderMap) -> Result<String, (StatusCode, ToolResponse)> {
        let (caller, token) = self.authenticate(headers)?;
        if caller != self.owner {
            return Err((
                StatusCode::FORBIDDEN,
                error(
                    "forbidden",
                    "Only the local owner can access the memory overview.",
                ),
            ));
        }
        Ok(token)
    }

    async fn activity(
        &self,
        headers: &HeaderMap,
        query: ActivityQuery,
    ) -> (StatusCode, ToolResponse) {
        let (caller, _) = match self.authenticate(headers) {
            Ok(caller) => caller,
            Err(e) => return e,
        };
        if query.conversation.is_none() && caller != self.owner {
            return (
                StatusCode::FORBIDDEN,
                error("forbidden", "Select one of your conversations."),
            );
        }
        match self.service.activity(caller, query).await {
            Ok(mut page) => {
                let cursor = page.next_cursor.take();
                (
                    StatusCode::OK,
                    ToolResponse::Ok {
                        result: json!(page),
                        next_cursor: cursor,
                    },
                )
            }
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

    pub async fn websocket_activity(&self, headers: &HeaderMap, params: Value) -> Value {
        if let Err((_, err)) = self.authenticate(headers) {
            return json!(err);
        }
        let (query,) = match serde_json::from_value::<(ActivityQuery,)>(params) {
            Ok(query) => query,
            Err(_) => {
                return json!(error(
                    "invalid_request",
                    "memory_activity requires one query object."
                ));
            }
        };
        json!(self.activity(headers, query).await.1)
    }

    pub async fn overview(&self, headers: &HeaderMap) -> (StatusCode, ToolResponse) {
        let token = match self.authorize(headers) {
            Ok(token) => token,
            Err(error) => return error,
        };
        let mut overview = self.service.overview(token).await;
        overview.caller = Some(self.owner.to_string());
        (
            StatusCode::OK,
            ToolResponse::Ok {
                result: json!(overview),
                next_cursor: None,
            },
        )
    }

    pub async fn websocket(&self, headers: &HeaderMap, params: Value) -> Value {
        // Reauthenticate on every invocation, not just at WebSocket upgrade.
        if let Err((_, error)) = self.authorize(headers) {
            return json!(error);
        }
        if params != json!([]) {
            return json!(error(
                "invalid_request",
                "memory_overview requires an empty parameter array."
            ));
        }
        json!(self.overview(headers).await.1)
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

fn service_error(err: anda_core::BoxError) -> (StatusCode, ToolResponse) {
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

async fn setup_preview(State(state): State<MemoryApiState>, headers: HeaderMap) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    setup_response(state.service.setup_preview(state.owner).await)
}
async fn search(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<SearchRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return json_rejection(rejection),
    };
    let (status, result) = state.search(&headers, request).await;
    (status, Json(result)).into_response()
}
fn json_rejection(rejection: axum::extract::rejection::JsonRejection) -> Response {
    let (status, reason) = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
    } else {
        (StatusCode::BAD_REQUEST, "invalid_request")
    };
    (
        status,
        Json(error(reason, "Invalid or oversized memory request.")),
    )
        .into_response()
}

async fn setup_commit(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<CommitRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return json_rejection(rejection),
    };
    setup_response(
        state
            .service
            .setup_commit(state.owner, &request.preview_digest)
            .await,
    )
}
fn setup_response(
    result: Result<crate::brain::setup::SetupPreview, anda_core::BoxError>,
) -> Response {
    let (status, response) = match result {
        Ok(view) => (
            StatusCode::OK,
            ToolResponse::Ok {
                result: json!(view),
                next_cursor: None,
            },
        ),
        Err(error) => service_error(error),
    };
    (status, Json(response)).into_response()
}

async fn prepare_change(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<ChangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return json_rejection(rejection),
    };
    change_response(state.service.prepare_change(state.owner, request).await)
}

async fn commit_change(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Result<Json<CommitRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return json_rejection(rejection),
    };
    change_response(state.service.commit_change(state.owner, id, request).await)
}

async fn change_status(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    change_response(state.service.change(state.owner, &id).await)
}

fn change_response(
    result: Result<crate::brain::mutation::ChangeView, anda_core::BoxError>,
) -> Response {
    let (status, response) = match result {
        Ok(view) => (
            StatusCode::OK,
            ToolResponse::Ok {
                result: json!(view),
                next_cursor: None,
            },
        ),
        Err(error) => service_error(error),
    };
    (status, Json(response)).into_response()
}

async fn discard_change(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let (status, result) = match state.service.discard_change(state.owner, &id).await {
        Ok(()) => (
            StatusCode::OK,
            ToolResponse::Ok {
                result: json!({"schema_version":1,"discarded":true}),
                next_cursor: None,
            },
        ),
        Err(error) => service_error(error),
    };
    (status, Json(result)).into_response()
}

async fn records(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<RecordQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_request", "Invalid memory query.")),
            )
                .into_response();
        }
    };
    let (status, result) = state.records(&headers, query).await;
    (status, Json(result)).into_response()
}

async fn record(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (status, result) = state.record(&headers, &id).await;
    (status, Json(result)).into_response()
}

async fn activity(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<ActivityQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    if let Err((status, error)) = state.authenticate(&headers) {
        return (status, Json(error)).into_response();
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_request", "Invalid activity query.")),
            )
                .into_response();
        }
    };
    let (status, result) = state.activity(&headers, query).await;
    (status, Json(result)).into_response()
}

async fn overview(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    query: Result<Query<OverviewQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    if query.is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_request",
                "The overview accepts no query parameters.",
            )),
        )
            .into_response();
    }
    let (status, result) = state.overview(&headers).await;
    (status, Json(result)).into_response()
}

async fn create_watch(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    request: Result<Json<WatchRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let request = match request {
        Ok(Json(request)) => request,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_request", "Invalid record watch request.")),
            )
                .into_response();
        }
    };
    watch_response(state.service.watch(state.owner, request).await)
}
async fn cancel_watch(
    State(state): State<MemoryApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    watch_response(state.service.cancel_watch(state.owner, id).await)
}
fn watch_response(
    result: Result<anda_brain::runtime_api::RecordWatch, anda_core::BoxError>,
) -> Response {
    let (status, response) = match result {
        Ok(watch) => (
            StatusCode::OK,
            ToolResponse::Ok {
                result: json!({"schema_version":1,"watch":watch}),
                next_cursor: None,
            },
        ),
        Err(error) => service_error(error),
    };
    (status, Json(response)).into_response()
}

async fn list_watches(State(state): State<MemoryApiState>, headers: HeaderMap) -> Response {
    if let Err((status, error)) = state.authorize(&headers) {
        return (status, Json(error)).into_response();
    }
    let (status, result) = state.watch_list().await;
    (status, Json(result)).into_response()
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
            state.websocket(&headers(&owner, true), json!([])).await["error"]["code"],
            "unauthorized"
        );
        assert_eq!(
            state
                .websocket(&headers(&owner, false), json!([{"caller":"forged"}]))
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
}
