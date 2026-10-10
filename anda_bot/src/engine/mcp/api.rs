//! The owner's MCP API: WebSocket methods `mcp_*` for the apps, and
//! `POST /daemon/mcp/v1` with `{method, params}` for the CLI. Both decode into
//! one [`McpRequest`] and reply with the shared `ToolResponse` envelope.
//!
//! It is a dedicated RPC rather than a registered tool because the engine
//! cannot hide a tool from the model: removing a server or changing what it
//! may do must never be reachable without the owner.

// Replies carry the shared ToolResponse error contract by value.
#![allow(clippy::result_large_err)]

use anda_core::{BoxError, Principal};
use anda_engine_server::handler::AppState;
use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::sync::Arc;

use super::{McpChange, McpError, McpManager, manager::SignIn, state::McpSource};
use crate::{
    config::{McpServerSettings, McpSettings},
    engine::memory_api::error,
    runtime_admission::Admission,
    util::tool_response::ToolResponse,
};

/// Requests and their parameters are small; this bounds a hostile one.
const MAX_PARAMS_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct McpApiState {
    pub app: AppState,
    pub owner: Principal,
    pub admission: Arc<Admission>,
    pub manager: McpManager,
}

/// The HTTP body: one method and its parameters.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct McpCall {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

type Reply = (StatusCode, ToolResponse);

enum McpRequest {
    List,
    Get(String),
    Test(McpServerSettings),
    Apply {
        change: McpChange,
        expected_revision: Option<String>,
    },
    Reconnect(Option<String>),
    SignIn(SignIn),
    SignOut(String),
    Reload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionalIdParams {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestParams {
    server: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyParams {
    change: ChangeParams,
    #[serde(default)]
    expected_revision: Option<String>,
}

/// A change as it travels: servers are mcp.json entries with an `id`.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ChangeParams {
    Add {
        server: Value,
        #[serde(default = "default_true")]
        persist: bool,
    },
    Update {
        server: Value,
    },
    Remove {
        id: String,
        #[serde(default)]
        keep_credentials: bool,
    },
    SetEnabled {
        id: String,
        enabled: bool,
    },
    SetToolVisible {
        id: String,
        tool: String,
        visible: bool,
    },
}

fn default_true() -> bool {
    true
}

impl McpRequest {
    fn parse(method: &str, params: Value) -> Result<Self, ToolResponse> {
        fn params_of<T: DeserializeOwned>(method: &str, params: Value) -> Result<T, ToolResponse> {
            serde_json::from_value(params).map_err(|err| {
                error(
                    "invalid_request",
                    &format!("Invalid parameters for {method}: {err}"),
                )
            })
        }
        fn no_params(method: &str, params: &Value) -> Result<(), ToolResponse> {
            match params {
                Value::Null => Ok(()),
                Value::Object(object) if object.is_empty() => Ok(()),
                Value::Array(array) if array.is_empty() => Ok(()),
                _ => Err(error(
                    "invalid_request",
                    &format!("{method} takes no parameters."),
                )),
            }
        }

        Ok(match method {
            "mcp_list" => {
                no_params(method, &params)?;
                Self::List
            }
            "mcp_get" => Self::Get(params_of::<IdParams>(method, params)?.id),
            "mcp_test" => Self::Test(parse_server(
                params_of::<TestParams>(method, params)?.server,
            )?),
            "mcp_apply" => {
                let ApplyParams {
                    change,
                    expected_revision,
                } = params_of(method, params)?;
                let change = match change {
                    ChangeParams::Add { server, persist } => McpChange::Add {
                        server: parse_server(server)?,
                        persist,
                    },
                    ChangeParams::Update { server } => McpChange::Update {
                        server: parse_server(server)?,
                    },
                    ChangeParams::Remove {
                        id,
                        keep_credentials,
                    } => McpChange::Remove {
                        id,
                        keep_credentials,
                    },
                    ChangeParams::SetEnabled { id, enabled } => {
                        McpChange::SetEnabled { id, enabled }
                    }
                    ChangeParams::SetToolVisible { id, tool, visible } => {
                        McpChange::SetToolVisible { id, tool, visible }
                    }
                };
                Self::Apply {
                    change,
                    expected_revision,
                }
            }
            "mcp_reconnect" => {
                let id = if params.is_null() {
                    None
                } else {
                    params_of::<OptionalIdParams>(method, params)?.id
                };
                Self::Reconnect(id)
            }
            "mcp_sign_in" => Self::SignIn(params_of(method, params)?),
            "mcp_sign_out" => Self::SignOut(params_of::<IdParams>(method, params)?.id),
            "mcp_reload" => {
                no_params(method, &params)?;
                Self::Reload
            }
            _ => {
                return Err(error(
                    "unsupported_capability",
                    &format!("{method} is not an MCP method."),
                ));
            }
        })
    }
}

/// Whether `method` changes anything, so that it waits for admission.
pub(crate) fn is_write_method(method: &str) -> bool {
    method.starts_with("mcp_") && !matches!(method, "mcp_list" | "mcp_get")
}

/// A server parameter: an mcp.json entry, with its `id` alongside.
fn parse_server(value: Value) -> Result<McpServerSettings, ToolResponse> {
    let Value::Object(mut entry) = value else {
        return Err(error(
            "invalid_request",
            "server must be an mcp.json entry with an id.",
        ));
    };
    let id = entry
        .remove("id")
        .and_then(|id| id.as_str().map(|id| id.trim().to_string()))
        .filter(|id| !id.is_empty())
        .ok_or_else(|| error("invalid_request", "server needs an id."))?;
    McpSettings::parse_entry(&id, &Value::Object(entry))
        .map_err(|err| error("invalid_request", &format!("MCP server {id}: {err}")))
}

impl McpApiState {
    /// Serves a WebSocket method. The connection already proved it is the
    /// owner's and holds admission for a write.
    pub async fn websocket_dispatch(&self, method: &str, params: Value) -> Value {
        json!(self.dispatch(method, params).await.1)
    }

    async fn dispatch(&self, method: &str, params: Value) -> Reply {
        if serde_json::to_vec(&params).map_or(true, |bytes| bytes.len() > MAX_PARAMS_BYTES) {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                error("payload_too_large", "MCP requests are limited to 64 KiB."),
            );
        }
        match McpRequest::parse(method, params) {
            Ok(request) => self.execute(request).await,
            Err(error) => (StatusCode::BAD_REQUEST, error),
        }
    }

    async fn execute(&self, request: McpRequest) -> Reply {
        let manager = &self.manager;
        match request {
            McpRequest::List => ok(manager.snapshot().await),
            McpRequest::Get(id) => respond(manager.server(&id).await),
            McpRequest::Test(server) => respond(manager.test(server).await),
            McpRequest::Apply {
                change,
                expected_revision,
            } => respond(
                manager
                    .apply(change, expected_revision.as_deref(), McpSource::Manual)
                    .await,
            ),
            McpRequest::Reconnect(id) => respond(manager.reconnect(id.as_deref()).await),
            // The caller opens the browser; the daemon only starts the flow.
            McpRequest::SignIn(request) => respond(manager.sign_in(request, false).await),
            McpRequest::SignOut(id) => respond(
                manager
                    .sign_out(&id)
                    .await
                    .map(|()| json!({ "signed_out": id })),
            ),
            McpRequest::Reload => respond(manager.reload().await),
        }
    }
}

/// `POST /daemon/mcp/v1`, for the owner only.
pub(crate) async fn mcp_route(
    State(state): State<McpApiState>,
    headers: HeaderMap,
    body: Result<Json<McpCall>, JsonRejection>,
) -> Response {
    if let Err((status, message)) = crate::engine::verify_owner(
        &state.app,
        &headers,
        state.owner,
        "Only the local owner may manage MCP servers",
    ) {
        let code = if status == StatusCode::FORBIDDEN {
            "forbidden"
        } else {
            "unauthorized"
        };
        return reply((status, error(code, message)));
    }
    let Ok(Json(call)) = body else {
        return reply((
            StatusCode::BAD_REQUEST,
            error(
                "invalid_request",
                "Expected a JSON body with a method and its params.",
            ),
        ));
    };
    let _permit = if is_write_method(&call.method) {
        match state.admission.enter() {
            Ok(permit) => Some(permit),
            Err(message) => {
                return reply((
                    StatusCode::SERVICE_UNAVAILABLE,
                    error("unavailable", message),
                ));
            }
        }
    } else {
        None
    };
    reply(state.dispatch(&call.method, call.params).await)
}

fn reply((status, body): Reply) -> Response {
    (status, Json(body)).into_response()
}

fn ok(result: impl Serialize) -> Reply {
    (
        StatusCode::OK,
        ToolResponse::Ok {
            result: json!(result),
            next_cursor: None,
        },
    )
}

fn respond<T: Serialize>(result: Result<T, BoxError>) -> Reply {
    match result {
        Ok(value) => ok(value),
        Err(err) => {
            let (status, code) = match err.downcast_ref::<McpError>() {
                Some(McpError { code, .. }) => (
                    match *code {
                        "not_found" => StatusCode::NOT_FOUND,
                        "already_exists" | "revision_conflict" => StatusCode::CONFLICT,
                        _ => StatusCode::BAD_REQUEST,
                    },
                    *code,
                ),
                None => (StatusCode::BAD_GATEWAY, "failed"),
            };
            (status, error(code, &err.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Ed25519Key, iana};
    use std::{collections::BTreeMap, time::Duration};

    fn headers(key: &Ed25519Key) -> HeaderMap {
        let mut claims = crate::identity::expiring_claims(Duration::from_secs(60)).unwrap();
        claims.extra.insert(iana::CWTClaimScope, "*");
        let token = key.sign_cwt(claims).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        headers
    }

    async fn state(home: &std::path::Path, owner: &Ed25519Key, other: &Ed25519Key) -> McpApiState {
        McpApiState {
            app: AppState {
                engines: Arc::new(BTreeMap::new()),
                default_engine: Principal::management_canister(),
                start_time_ms: 0,
                extra_info: Arc::new(BTreeMap::new()),
                ed25519_pubkeys: Arc::new(vec![owner.pubkey().into(), other.pubkey().into()]),
            },
            owner: owner.id(),
            admission: Arc::new(Admission::default()),
            manager: McpManager::for_test(home).await,
        }
    }

    async fn call(
        state: &McpApiState,
        headers: HeaderMap,
        method: &str,
        params: Value,
    ) -> (StatusCode, Value) {
        let response = mcp_route(
            State(state.clone()),
            headers,
            Ok(Json(McpCall {
                method: method.to_string(),
                params,
            })),
        )
        .await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn the_route_is_the_owners() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([91; 32]), Ed25519Key::new([92; 32]));
        let state = state(dir.path(), &owner, &other).await;

        let (status, body) = call(&state, HeaderMap::new(), "mcp_list", json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
        // A trusted user who is not the owner sees nothing either.
        let (status, body) = call(&state, headers(&other), "mcp_list", json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");

        let (status, body) = call(&state, headers(&owner), "mcp_list", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["servers"], json!([]));
    }

    #[tokio::test]
    async fn changes_wait_for_admission_and_reads_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([93; 32]), Ed25519Key::new([94; 32]));
        let state = state(dir.path(), &owner, &other).await;
        let _maintenance = state.admission.begin().unwrap();

        let (status, body) = call(&state, headers(&owner), "mcp_reload", json!({})).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "unavailable");
        let (status, _) = call(&state, headers(&owner), "mcp_list", json!(null)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn changes_are_entries_with_ids_and_refusals_carry_codes() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([95; 32]), Ed25519Key::new([96; 32]));
        let state = state(dir.path(), &owner, &other).await;
        let owner = || headers(&owner);

        let add = json!({
            "change": {
                "op": "add",
                "server": {
                    "id": "docs",
                    "type": "http",
                    "url": "http://127.0.0.1:9/mcp",
                    "headers": { "Authorization": "Bearer api-secret" },
                    "enabled": false
                }
            }
        });
        let (status, body) = call(&state, owner(), "mcp_apply", add.clone()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(!body["result"]["revision"].as_str().unwrap().is_empty());
        // `persist` defaults to true for the owner.
        let content = tokio::fs::read_to_string(McpSettings::file_path(dir.path()))
            .await
            .unwrap();
        assert!(content.contains("\"docs\""));

        let (status, body) = call(&state, owner(), "mcp_get", json!({ "id": "docs" })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["source"], "manual");
        assert_eq!(body["result"]["status"], "disabled");
        assert!(!body.to_string().contains("api-secret"), "{body}");

        for (method, params, status, code) in [
            ("mcp_apply", add, StatusCode::CONFLICT, "already_exists"),
            (
                "mcp_get",
                json!({ "id": "missing" }),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                "mcp_apply",
                json!({ "change": { "op": "add", "server": { "url": "http://x.test" } } }),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                "mcp_apply",
                json!({ "change": { "op": "set_enabled", "id": "docs", "enabled": true }, "expected_revision": "stale" }),
                StatusCode::CONFLICT,
                "revision_conflict",
            ),
            (
                "mcp_list",
                json!({ "id": "docs" }),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                "mcp_tool_diff",
                json!({}),
                StatusCode::BAD_REQUEST,
                "unsupported_capability",
            ),
        ] {
            let (got, body) = call(&state, owner(), method, params).await;
            assert_eq!(
                (got, body["error"]["code"].as_str()),
                (status, Some(code)),
                "{body}"
            );
        }
    }

    #[tokio::test]
    async fn the_cli_client_reads_results_and_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([97; 32]), Ed25519Key::new([98; 32]));
        let app = axum::Router::new()
            .route("/daemon/mcp/v1", axum::routing::post(mcp_route))
            .with_state(state(dir.path(), &owner, &other).await);
        let base_url = crate::test_support::spawn_http_mock(app).await;
        let token = headers(&owner)[axum::http::header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .trim_start_matches("Bearer ")
            .to_string();
        let client = crate::gateway::Client::new(base_url, token);

        let snapshot: Value = client.mcp("mcp_list", json!({})).await.unwrap();
        assert_eq!(snapshot["running"], true);
        let err = client
            .mcp::<Value>("mcp_get", json!({ "id": "missing" }))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "MCP server missing is not configured");
    }

    #[test]
    fn only_list_and_get_are_reads() {
        for method in ["mcp_list", "mcp_get"] {
            assert!(!is_write_method(method));
        }
        for method in [
            "mcp_apply",
            "mcp_test",
            "mcp_reconnect",
            "mcp_sign_in",
            "mcp_sign_out",
            "mcp_reload",
        ] {
            assert!(is_write_method(method));
        }
        assert!(!is_write_method("memory_overview"));
    }
}
