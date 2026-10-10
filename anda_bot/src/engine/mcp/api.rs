//! The owner's MCP API: WebSocket methods `mcp_*` for the apps, and
//! `POST /daemon/mcp/v1` with `{method, params}` for the CLI. Both decode into
//! one [`McpRequest`] and reply with the shared `ToolResponse` envelope.
//! Parameters are one object; the apps, which send every RPC's parameters as
//! a list, may wrap it in a one-element list.
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
use std::{path::PathBuf, sync::Arc};

use super::{
    McpChange, McpError, McpManager,
    import::{McpImportContext, McpImportRequest, McpImportSource},
    manager::SignIn,
    registry::{self, McpRegistryQuery},
    state::{McpOrigin, McpSource},
};
use crate::{
    config::{McpApproval, McpSecretValues, McpServerOptions, McpServerSettings, McpSettings},
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
    /// The daemon's outbound client, for the MCP Registry.
    pub http: reqwest::Client,
    pub registry_url: String,
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
    ToolDiff {
        id: String,
        tool: String,
    },
    Test(McpServerSettings, McpSecretValues),
    Apply {
        change: McpChange,
        expected_revision: Option<String>,
        origin: McpOrigin,
    },
    Reconnect(Option<String>),
    SignIn(SignIn),
    SignOut(String),
    Reload,
    Secrets,
    ImportScan(ImportScanParams),
    Import(McpImportRequest),
    RegistrySearch(McpRegistryQuery),
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportScanParams {
    /// The clients to read; empty reads them all.
    #[serde(default)]
    sources: Vec<McpImportSource>,
    /// Project directories to read beyond the daemon's own workspace.
    #[serde(default)]
    workspaces: Vec<PathBuf>,
}

/// Where an added server came from, as the apps may say: by hand, or from
/// the MCP Registry.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AddSource {
    #[default]
    Manual,
    Registry,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolParams {
    id: String,
    tool: String,
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
    /// Values for secrets the server references that are not stored yet,
    /// used for this test only.
    #[serde(default)]
    secrets: McpSecretValues,
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
        #[serde(default)]
        source: AddSource,
        /// The Registry name and version it was installed from.
        #[serde(default)]
        source_ref: Option<String>,
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
    /// `approval` is `auto`, `ask`, `allow`, or `null` to clear it.
    SetApproval {
        id: String,
        #[serde(default)]
        tool: Option<String>,
        approval: Option<McpApproval>,
    },
    SetExternalUsers {
        id: String,
        allowed: bool,
    },
    /// No tools: every tool the server offers.
    MarkReviewed {
        id: String,
        #[serde(default)]
        tools: Vec<String>,
    },
    /// `value: null` removes the secret.
    SetSecret {
        name: String,
        value: Option<String>,
    },
    /// Replaces the advanced settings; a field left out takes its default.
    SetOptions {
        id: String,
        options: McpServerOptions,
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
            "mcp_tool_diff" => {
                let ToolParams { id, tool } = params_of(method, params)?;
                Self::ToolDiff { id, tool }
            }
            "mcp_test" => {
                let TestParams { server, secrets } = params_of(method, params)?;
                Self::Test(parse_server(server)?, secrets)
            }
            "mcp_apply" => {
                let ApplyParams {
                    change,
                    expected_revision,
                } = params_of(method, params)?;
                let mut origin = McpOrigin::from(McpSource::Manual);
                let change = match change {
                    ChangeParams::Add {
                        server,
                        persist,
                        source,
                        source_ref,
                    } => {
                        if let AddSource::Registry = source {
                            origin = McpOrigin {
                                source: McpSource::Registry,
                                reference: source_ref
                                    .map(|reference| reference.trim().chars().take(256).collect()),
                            };
                        }
                        McpChange::Add {
                            server: parse_server(server)?,
                            persist,
                        }
                    }
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
                    ChangeParams::SetApproval { id, tool, approval } => {
                        McpChange::SetApproval { id, tool, approval }
                    }
                    ChangeParams::SetExternalUsers { id, allowed } => {
                        McpChange::SetExternalUsers { id, allowed }
                    }
                    ChangeParams::MarkReviewed { id, tools } => {
                        McpChange::MarkReviewed { id, tools }
                    }
                    ChangeParams::SetSecret { name, value } => McpChange::SetSecret { name, value },
                    ChangeParams::SetOptions { id, options } => {
                        McpChange::SetOptions { id, options }
                    }
                };
                Self::Apply {
                    change,
                    expected_revision,
                    origin,
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
            "mcp_secrets" => {
                no_params(method, &params)?;
                Self::Secrets
            }
            "mcp_import_scan" => Self::ImportScan(if params.is_null() {
                ImportScanParams::default()
            } else {
                params_of(method, params)?
            }),
            "mcp_import" => Self::Import(params_of(method, params)?),
            "mcp_registry_search" => Self::RegistrySearch(if params.is_null() {
                McpRegistryQuery::default()
            } else {
                params_of(method, params)?
            }),
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
    method.starts_with("mcp_")
        && !matches!(
            method,
            "mcp_list"
                | "mcp_get"
                | "mcp_tool_diff"
                | "mcp_secrets"
                | "mcp_import_scan"
                | "mcp_registry_search"
        )
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
        // The apps' RPC transport sends parameters as a list.
        let params = match params {
            Value::Array(mut items) if items.len() == 1 && items[0].is_object() => items.remove(0),
            params => params,
        };
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
            McpRequest::ToolDiff { id, tool } => respond(manager.tool_diff(&id, &tool)),
            McpRequest::Test(server, secrets) => respond(manager.test(server, secrets).await),
            McpRequest::Apply {
                change,
                expected_revision,
                origin,
            } => respond(
                manager
                    .apply(change, expected_revision.as_deref(), origin)
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
            McpRequest::Secrets => ok(manager.secrets()),
            McpRequest::ImportScan(params) => match self.import_context(params.workspaces) {
                Ok(ctx) => ok(manager.import_scan(&ctx, &params.sources).await),
                Err(err) => respond::<()>(Err(err)),
            },
            McpRequest::Import(request) => match self.import_context(request.workspaces.clone()) {
                Ok(ctx) => respond(manager.import(&ctx, request).await),
                Err(err) => respond::<()>(Err(err)),
            },
            McpRequest::RegistrySearch(query) => {
                respond(registry::search(&self.http, &self.registry_url, &query).await)
            }
        }
    }

    /// The current user's directories, with the workspaces asked for and the
    /// daemon's own.
    fn import_context(&self, mut workspaces: Vec<PathBuf>) -> Result<McpImportContext, BoxError> {
        workspaces.extend(self.manager.default_cwd().map(PathBuf::from));
        McpImportContext::detect(workspaces)
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
            http: reqwest::Client::builder().no_proxy().build().unwrap(),
            registry_url: "http://127.0.0.1:9".to_string(),
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
                "mcp_events_list",
                json!({}),
                StatusCode::BAD_REQUEST,
                "unsupported_capability",
            ),
            (
                "mcp_apply",
                json!({ "change": { "op": "set_secret", "name": "bad-name", "value": "x" } }),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                "mcp_tool_diff",
                json!({ "id": "docs", "tool": "search" }),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                "mcp_apply",
                json!({ "change": { "op": "set_approval", "id": "docs", "approval": "never" } }),
                StatusCode::BAD_REQUEST,
                "invalid_request",
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

    #[tokio::test]
    async fn approval_policies_are_written_to_the_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([99; 32]), Ed25519Key::new([100; 32]));
        let state = state(dir.path(), &owner, &other).await;
        let owner = || headers(&owner);
        let add = json!({ "change": { "op": "add", "server": {
            "id": "docs", "type": "http", "url": "http://127.0.0.1:9/mcp", "enabled": false
        } } });
        assert_eq!(
            call(&state, owner(), "mcp_apply", add).await.0,
            StatusCode::OK
        );

        for change in [
            json!({ "op": "set_approval", "id": "docs", "approval": "allow" }),
            json!({ "op": "set_approval", "id": "docs", "tool": "delete", "approval": "ask" }),
            json!({ "op": "set_external_users", "id": "docs", "allowed": true }),
        ] {
            let (status, body) =
                call(&state, owner(), "mcp_apply", json!({ "change": change })).await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        let (_, body) = call(&state, owner(), "mcp_get", json!({ "id": "docs" })).await;
        assert_eq!(body["result"]["approval"], "allow");
        assert_eq!(body["result"]["allow_external_users"], true);
        assert_eq!(
            body["result"]["settings"]["approval"],
            json!({ "default": "allow", "tools": { "delete": "ask" } })
        );

        // `null` clears a policy.
        let clear = json!({ "change": { "op": "set_approval", "id": "docs", "approval": null } });
        assert_eq!(
            call(&state, owner(), "mcp_apply", clear).await.0,
            StatusCode::OK
        );
        let (_, body) = call(&state, owner(), "mcp_get", json!({ "id": "docs" })).await;
        assert_eq!(body["result"]["approval"], "auto");
        assert_eq!(
            body["result"]["settings"]["approval"],
            json!({ "tools": { "delete": "ask" } })
        );
    }

    #[tokio::test]
    async fn secrets_are_set_and_listed_by_name_only() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([101; 32]), Ed25519Key::new([102; 32]));
        let state = state(dir.path(), &owner, &other).await;
        let owner = || headers(&owner);
        // The apps wrap the parameters in a list.
        let add = json!([{ "change": { "op": "add", "server": {
            "id": "docs", "type": "http", "url": "http://127.0.0.1:9/mcp", "enabled": false,
            "headers": { "Authorization": "Bearer ${secret:DOCS_TOKEN}" }
        } } }]);
        assert_eq!(
            call(&state, owner(), "mcp_apply", add).await.0,
            StatusCode::OK
        );
        let (_, body) = call(&state, owner(), "mcp_secrets", json!([])).await;
        assert_eq!(
            body["result"],
            json!([{ "name": "DOCS_TOKEN", "is_set": false, "used_by": ["docs"] }])
        );

        let set = json!({ "change": { "op": "set_secret", "name": "DOCS_TOKEN", "value": "tok-secret" } });
        let (status, body) = call(&state, owner(), "mcp_apply", set).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, body) = call(&state, owner(), "mcp_secrets", json!({})).await;
        assert_eq!(body["result"][0]["is_set"], true);
        assert!(body["result"][0]["updated_at"].is_u64());
        let (_, detail) = call(&state, owner(), "mcp_get", json!({ "id": "docs" })).await;
        assert_eq!(
            detail["result"]["settings"]["headers"]["Authorization"]["secrets"],
            json!(["DOCS_TOKEN"])
        );
        assert!(!format!("{body}{detail}").contains("tok-secret"));

        // Removing the server takes the secret only it used.
        let remove = json!({ "change": { "op": "remove", "id": "docs" } });
        let (_, body) = call(&state, owner(), "mcp_apply", remove).await;
        assert_eq!(body["result"]["secrets_removed"], json!(["DOCS_TOKEN"]));
        let (_, body) = call(&state, owner(), "mcp_secrets", json!({})).await;
        assert_eq!(body["result"], json!([]));
    }

    #[tokio::test]
    async fn options_registry_installs_and_searches_go_through_the_api() {
        let dir = tempfile::tempdir().unwrap();
        let (owner, other) = (Ed25519Key::new([103; 32]), Ed25519Key::new([104; 32]));
        let mut state = state(dir.path(), &owner, &other).await;
        let registry = axum::Router::new().route(
            "/v0.1/servers",
            axum::routing::get(|| async {
                Json(json!({ "servers": [{ "server": { "name": "io.github.example/docs" } }] }))
            }),
        );
        state.registry_url = crate::test_support::spawn_http_mock(registry).await;
        let owner = || headers(&owner);

        let add = json!({ "change": {
            "op": "add",
            "server": { "id": "docs", "type": "http", "url": "http://127.0.0.1:9/mcp", "enabled": false },
            "source": "registry",
            "source_ref": "io.github.example/docs@1.0.0"
        } });
        let (status, body) = call(&state, owner(), "mcp_apply", add).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let options = json!({ "change": {
            "op": "set_options",
            "id": "docs",
            "options": { "timeouts": { "call_secs": 120 }, "concurrency": "parallel" }
        } });
        let (status, body) = call(&state, owner(), "mcp_apply", options).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, body) = call(&state, owner(), "mcp_get", json!({ "id": "docs" })).await;
        assert_eq!(body["result"]["source"], "registry");
        assert_eq!(body["result"]["source_ref"], "io.github.example/docs@1.0.0");
        assert_eq!(
            body["result"]["options"],
            json!({ "concurrency": "parallel", "timeouts": { "call_secs": 120 } })
        );

        let (status, body) = call(
            &state,
            owner(),
            "mcp_registry_search",
            json!([{ "query": "docs" }]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["result"]["servers"][0]["name"],
            "io.github.example/docs"
        );

        for (method, params) in [
            (
                "mcp_apply",
                json!({ "change": { "op": "set_options", "id": "docs", "options": { "retries": 3 } } }),
            ),
            ("mcp_import_scan", json!({ "sources": ["netscape"] })),
            ("mcp_import", json!({})),
        ] {
            let (status, body) = call(&state, owner(), method, params).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["code"], "invalid_request");
        }
    }

    #[test]
    fn only_reads_skip_admission() {
        for method in [
            "mcp_list",
            "mcp_get",
            "mcp_tool_diff",
            "mcp_secrets",
            "mcp_import_scan",
            "mcp_registry_search",
        ] {
            assert!(!is_write_method(method));
        }
        for method in [
            "mcp_apply",
            "mcp_test",
            "mcp_reconnect",
            "mcp_sign_in",
            "mcp_sign_out",
            "mcp_reload",
            "mcp_import",
        ] {
            assert!(is_write_method(method));
        }
        assert!(!is_write_method("memory_overview"));
    }
}
