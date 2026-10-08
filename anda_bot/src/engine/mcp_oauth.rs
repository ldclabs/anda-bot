//! Gateway-hosted OAuth redirect capture for MCP servers.
//!
//! An MCP authorization server redirects the browser to a URI the client
//! registered up front, and for a native application that URI has to be
//! loopback (RFC 8252) — a `http://<server-ip>:…` redirect is rejected by most
//! authorization servers. Binding an ephemeral port per attempt satisfies that
//! rule only when the browser runs on the same machine as the daemon: over SSH
//! the redirect lands on the *user's* loopback, and a tunnel cannot be opened
//! in advance for a port that is chosen at call time and changes on every
//! retry.
//!
//! So the redirect lands on the gateway instead, at a fixed path on the port
//! the daemon already serves. `ssh -N -L 8042:127.0.0.1:8042 user@host` — the
//! same tunnel a remote user already opens to reach the side panel — carries
//! the callback with no extra setup, and the URI stays loopback from the
//! browser's point of view either way.
//!
//! The route cannot be authenticated: a browser following a redirect carries
//! no daemon credentials. It is safe by construction instead — a callback is
//! matched against a pending flow by its `state`, consumed once, and anything
//! unrecognised is rejected without side effects. The token exchange itself
//! re-validates `state`, PKCE, and the RFC 9207 issuer, so this endpoint is a
//! router rather than a trust boundary.

use anda_core::BoxError;
use anda_engine::extension::mcp::{McpServerConfig, McpToolProvider};
use axum::{
    extract::{RawQuery, State},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, oneshot},
    time::Instant,
};

use super::mcp_server::{McpServerConfigs, persist_oauth_server};

/// Path the gateway serves the MCP OAuth redirect on.
pub const CALLBACK_PATH: &str = "/mcp/oauth/callback";

/// How long a started authorization stays completable.
///
/// Generous because the ceremony may include a sign-in step before the consent
/// page, and a remote user may still be opening their tunnel.
const FLOW_TTL: Duration = Duration::from_secs(600);

/// What a caller blocked on a flow learns: whether the server was newly
/// written to mcp.json, or why the flow failed.
pub type FlowOutcome = Result<bool, String>;

/// An authorization that has been started and is waiting for its redirect.
struct PendingFlow {
    server: McpServerConfig,
    /// Handed out again while the flow is pending, so asking twice does not
    /// invalidate the URL the user is already working through.
    auth_url: String,
    expires_at: Instant,
    /// Present while a caller is blocked on this flow; absent once the caller
    /// handed the authorization URL to the user and returned.
    waiter: Option<oneshot::Sender<FlowOutcome>>,
}

/// Outcome of a completed redirect.
#[derive(Debug)]
pub struct CompletedFlow {
    pub server_id: String,
    /// Whether the server was newly written to mcp.json.
    pub persisted: bool,
}

/// Registry of in-flight MCP authorizations, keyed by the `state` the
/// authorization server echoes back on the redirect.
#[derive(Clone)]
pub struct McpOAuthFlows {
    inner: Arc<Inner>,
}

struct Inner {
    provider: Arc<McpToolProvider>,
    configs: McpServerConfigs,
    redirect_uri: String,
    config_path: PathBuf,
    config_write_lock: Arc<Mutex<()>>,
    pending: Mutex<HashMap<String, PendingFlow>>,
}

impl McpOAuthFlows {
    pub fn new(
        provider: Arc<McpToolProvider>,
        configs: McpServerConfigs,
        gateway_addr: std::net::SocketAddr,
        config_path: PathBuf,
        config_write_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                provider,
                configs,
                // Always loopback, whatever the gateway binds: this is the
                // address the *browser* resolves, reaching the daemon directly
                // on the desktop and through the user's tunnel over SSH.
                redirect_uri: oauth_redirect_uri(gateway_addr),
                config_path,
                config_write_lock,
                pending: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The redirect URI every server registers with its authorization server.
    pub fn redirect_uri(&self) -> &str {
        &self.inner.redirect_uri
    }

    /// Records a started authorization so its redirect can be matched later.
    ///
    /// Returns a receiver that resolves when the redirect is handled, for a
    /// caller that wants to block; dropping it leaves the flow completable in
    /// the background, which is what the headless path does.
    pub async fn begin(
        &self,
        server: McpServerConfig,
        auth_url: &str,
    ) -> Result<oneshot::Receiver<FlowOutcome>, BoxError> {
        let state = state_from_url(auth_url)
            .ok_or("authorization URL carries no state parameter to match its redirect against")?;
        let (tx, rx) = oneshot::channel();
        let mut pending = self.inner.pending.lock().await;
        // A fresh authorization replaces any older attempt for this server.
        // Its later timeout must not remove the replacement registration.
        pending.retain(|_, flow| flow.server.id != server.id);
        self.expire_locked(&mut pending);
        pending.insert(
            state,
            PendingFlow {
                server,
                auth_url: auth_url.to_string(),
                expires_at: Instant::now() + FLOW_TTL,
                waiter: Some(tx),
            },
        );
        Ok(rx)
    }

    /// The authorization URL of the flow still waiting for this server, if any.
    pub async fn pending_url(&self, server_id: &str) -> Option<String> {
        let mut pending = self.inner.pending.lock().await;
        self.expire_locked(&mut pending);
        pending
            .values()
            .find(|flow| flow.server.id == server_id)
            .map(|flow| flow.auth_url.clone())
    }

    /// Finishes the flow the redirect belongs to: exchanges the code, persists
    /// the server, and connects it.
    pub async fn complete(&self, redirect_url: &str) -> Result<CompletedFlow, BoxError> {
        let state =
            state_from_url(redirect_url).ok_or("redirect URL carries no state parameter")?;
        let flow = {
            let mut pending = self.inner.pending.lock().await;
            self.expire_locked(&mut pending);
            pending.remove(&state)
        };
        // An unknown state is the normal shape of a stray or replayed request:
        // say nothing about which servers exist.
        let Some(flow) = flow else {
            return Err("no pending MCP authorization matches this redirect".into());
        };

        // The flow is consumed now, so it must run to the end even if the
        // browser closes the tab or the tool call is cancelled meanwhile:
        // stopping halfway would strand a stored grant with no server.
        let flows = self.clone();
        let redirect_url = redirect_url.to_string();
        tokio::spawn(async move { flows.finish(flow, &redirect_url).await })
            .await
            .map_err(|err| format!("MCP authorization task failed: {err}"))?
    }

    /// Drops a flow that was started but will not be finished.
    pub async fn abandon(&self, auth_url: &str) {
        let Some(state) = state_from_url(auth_url) else {
            return;
        };
        let flow = self.inner.pending.lock().await.remove(&state);
        if let Some(flow) = flow {
            self.inner.provider.remove_server(&flow.server.id);
        }
    }

    async fn finish(
        &self,
        mut flow: PendingFlow,
        redirect_url: &str,
    ) -> Result<CompletedFlow, BoxError> {
        let waiter = flow.waiter.take();
        let result = self.authorize(&flow.server, redirect_url).await;
        if let Some(waiter) = waiter {
            let _ = waiter.send(
                result
                    .as_ref()
                    .map(|completed| completed.persisted)
                    .map_err(|err| err.to_string()),
            );
        }
        result
    }

    async fn authorize(
        &self,
        server: &McpServerConfig,
        redirect_url: &str,
    ) -> Result<CompletedFlow, BoxError> {
        let provider = &self.inner.provider;
        let id = &server.id;
        if let Err(err) = provider.complete_authorization(id, redirect_url).await {
            // The pending PKCE state is consumed either way, so leaving the
            // registration behind would only produce a server that can never
            // connect.
            provider.remove_server(id);
            return Err(err);
        }

        // The grant is stored from here on. Record and persist the server
        // before connecting, so a server that is slow to answer right after
        // consent keeps its registration and reconnects on the next call
        // instead of needing the browser again.
        self.inner
            .configs
            .write()
            .insert(id.clone(), server.clone());
        let config_path = &self.inner.config_path;
        let persisted = persist_oauth_server(config_path, &self.inner.config_write_lock, server)
            .await
            .map_err(|err| {
                format!(
                    "MCP server {id} is authorized, but failed to persist to {}: {err}",
                    config_path.display()
                )
            })?;
        provider.refresh_server(id).await.map_err(|err| {
            format!(
                "MCP server {id} is authorized and saved, but connecting failed: {err}; \
                 call connect_mcp_server again to retry"
            )
        })?;
        Ok(CompletedFlow {
            server_id: id.clone(),
            persisted,
        })
    }

    /// Drops flows whose authorization window has closed, releasing the
    /// half-registered server each one left behind.
    fn expire_locked(&self, pending: &mut HashMap<String, PendingFlow>) {
        let now = Instant::now();
        pending.retain(|_, flow| {
            if flow.expires_at > now {
                return true;
            }
            self.inner.provider.remove_server(&flow.server.id);
            false
        });
    }
}

fn oauth_redirect_uri(addr: std::net::SocketAddr) -> String {
    let loopback = if addr.is_ipv6() { "[::1]" } else { "127.0.0.1" };
    format!("http://{loopback}:{}{CALLBACK_PATH}", addr.port())
}

/// Reads the `state` query parameter out of an authorization or redirect URL.
fn state_from_url(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
}

/// Handles the authorization server's redirect.
///
/// Unauthenticated by necessity, and deliberately uninformative: the response
/// tells the person at the browser whether they are done, and never echoes the
/// authorization code or names a server.
pub async fn mcp_oauth_callback(
    State(flows): State<McpOAuthFlows>,
    RawQuery(query): RawQuery,
) -> impl IntoResponse {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return page(StatusCode::BAD_REQUEST, "Missing authorization response.");
    };
    // Rebuild the URL the authorization server was told to redirect to. Taking
    // it from the request instead would lose the scheme and host, and the token
    // exchange must see the registered redirect URI.
    let redirect_url = format!("{}?{}", flows.redirect_uri(), query);

    match flows.complete(&redirect_url).await {
        Ok(completed) => {
            log::info!(
                "MCP `{}` authorized (persisted: {})",
                completed.server_id,
                completed.persisted
            );
            page(
                StatusCode::OK,
                "Authorization complete. You can close this window and return to Anda.",
            )
        }
        Err(err) => {
            log::warn!("MCP authorization callback failed: {err}");
            page(
                StatusCode::BAD_REQUEST,
                "Authorization could not be completed. Return to Anda and start it again.",
            )
        }
    }
}

fn page(status: StatusCode, message: &str) -> (StatusCode, Html<String>) {
    (
        status,
        Html(format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Anda</title>\
             <body style=\"font:16px system-ui;margin:4rem auto;max-width:32rem\">{message}</body>"
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flows() -> McpOAuthFlows {
        McpOAuthFlows::new(
            Arc::new(McpToolProvider::new(Vec::new()).unwrap()),
            Default::default(),
            "127.0.0.1:8042".parse().unwrap(),
            PathBuf::from("/tmp/anda-mcp-oauth-test/mcp.json"),
            Arc::new(Mutex::new(())),
        )
    }

    #[test]
    fn redirect_uri_is_loopback_on_the_gateway_port() {
        assert_eq!(
            flows().redirect_uri(),
            "http://127.0.0.1:8042/mcp/oauth/callback"
        );
    }

    #[test]
    fn state_is_read_from_authorization_and_redirect_urls() {
        assert_eq!(
            state_from_url("https://as.example.com/authorize?client_id=x&state=abc&scope=y")
                .as_deref(),
            Some("abc")
        );
        assert_eq!(
            state_from_url("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=abc").as_deref(),
            Some("abc")
        );
        assert!(state_from_url("https://as.example.com/authorize?client_id=x").is_none());
        assert!(state_from_url("not a url").is_none());
    }

    #[tokio::test]
    async fn an_unmatched_redirect_is_rejected_without_side_effects() {
        let flows = flows();
        let err = flows
            .complete("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=nope")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "no pending MCP authorization matches this redirect"
        );

        // A redirect with no state at all cannot even name a flow.
        let err = flows
            .complete("http://127.0.0.1:8042/mcp/oauth/callback?code=c")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no state parameter"), "{err}");
    }

    #[tokio::test]
    async fn a_started_flow_is_matched_by_state_and_consumed_once() {
        let flows = flows();
        let server = McpServerConfig::streamable_http("srv", "https://mcp.example.com/mcp");
        flows
            .inner
            .provider
            .register_server(server.clone())
            .unwrap();
        let auth_url = "https://as.example.com/authorize?client_id=x&state=s-1";
        let waiter = flows.begin(server, auth_url).await.unwrap();
        assert_eq!(flows.pending_url("srv").await.as_deref(), Some(auth_url));

        // The exchange fails (there is no real authorization server), but it got
        // as far as naming the server — which is the point: `state` routed it.
        let err = flows
            .complete("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=s-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("srv"), "{err}");
        // A failed exchange leaves nothing to connect with, so the
        // registration goes too, and a blocked caller hears why.
        assert!(!flows.inner.provider.contains_server("srv"));
        assert!(flows.pending_url("srv").await.is_none());
        assert!(waiter.await.unwrap().unwrap_err().contains("srv"));

        // And it is gone afterwards, so a replayed redirect matches nothing.
        let err = flows
            .complete("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=s-1")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "no pending MCP authorization matches this redirect"
        );
    }

    #[tokio::test]
    async fn a_grant_that_landed_keeps_its_server_when_connecting_fails() {
        use anda_engine::extension::mcp::{
            McpOAuthConfig, McpTransportConfig, OAuthAuthorizationCodeConfig,
        };
        use axum::{Json, http::HeaderMap, routing};

        // An authorization server that grants any code, in front of an MCP
        // endpoint that is down right after consent.
        let app = axum::Router::new()
            .route(
                "/.well-known/oauth-authorization-server",
                routing::get(|headers: HeaderMap| async move {
                    let host = headers["host"].to_str().unwrap().to_string();
                    Json(serde_json::json!({
                        "issuer": format!("http://{host}"),
                        "authorization_endpoint": format!("http://{host}/authorize"),
                        "token_endpoint": format!("http://{host}/token"),
                        "response_types_supported": ["code"],
                        "code_challenge_methods_supported": ["S256"],
                    }))
                }),
            )
            .route(
                "/token",
                routing::post(|| async {
                    Json(serde_json::json!({
                        "access_token": "access",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "refresh_token": "refresh",
                    }))
                }),
            )
            .route(
                "/mcp",
                routing::post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            );
        let base_url = crate::test_support::spawn_http_mock(app).await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("mcp.json");
        let provider = Arc::new(McpToolProvider::new(Vec::new()).unwrap());
        let configs = McpServerConfigs::default();
        let flows = McpOAuthFlows::new(
            provider.clone(),
            configs.clone(),
            "127.0.0.1:8042".parse().unwrap(),
            config_path.clone(),
            Arc::new(Mutex::new(())),
        );
        let mut server = McpServerConfig::streamable_http("srv", format!("{base_url}/mcp"));
        if let McpTransportConfig::StreamableHttp(http) = &mut server.transport {
            http.auth = Some(McpOAuthConfig::AuthorizationCode(
                OAuthAuthorizationCodeConfig {
                    redirect_uri: flows.redirect_uri().to_string(),
                    scopes: vec!["read".to_string()],
                    client_name: None,
                    client_id: Some("test-client".to_string()),
                },
            ));
        }
        provider.register_server(server.clone()).unwrap();
        let auth_url = provider.begin_authorization("srv").await.unwrap();
        let waiter = flows.begin(server, &auth_url).await.unwrap();
        let state = state_from_url(&auth_url).unwrap();

        let err = flows
            .complete(&format!("{}?code=c&state={state}", flows.redirect_uri()))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("authorized and saved"), "{err}");
        assert!(waiter.await.unwrap().is_err());
        // The grant landed, so the server stays: registered, remembered, and
        // in mcp.json, ready for the next connect to retry without consent.
        assert!(provider.contains_server("srv"));
        assert!(configs.read().contains_key("srv"));
        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            json["mcpServers"]["srv"]["oauth"]["client_id"],
            "test-client"
        );
    }

    #[tokio::test]
    async fn abandoning_a_flow_makes_its_redirect_unmatched() {
        let flows = flows();
        let auth_url = "https://as.example.com/authorize?client_id=x&state=s-2";
        let _waiter = flows
            .begin(
                McpServerConfig::streamable_http("srv", "https://mcp.example.com/mcp"),
                auth_url,
            )
            .await
            .unwrap();
        flows.abandon(auth_url).await;

        let err = flows
            .complete("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=s-2")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "no pending MCP authorization matches this redirect"
        );
    }

    #[tokio::test]
    async fn callback_handler_reports_a_missing_query_without_touching_the_registry() {
        let response = mcp_oauth_callback(State(flows()), RawQuery(None))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_route_answers_a_browser_carrying_no_credentials() {
        // Mounted the way `Engines::into_router` mounts it, and reached the way
        // the authorization server's redirect reaches it: a plain GET with no
        // Authorization header. A 401 here would break every OAuth flow.
        let app = axum::Router::new()
            .route(CALLBACK_PATH, axum::routing::get(mcp_oauth_callback))
            .with_state(flows());
        let base_url = crate::test_support::spawn_http_mock(app).await;

        let response = reqwest::get(format!("{base_url}{CALLBACK_PATH}?code=abc&state=unknown"))
            .await
            .unwrap();
        // Reached the handler, and an unmatched state is refused there rather
        // than at an auth layer.
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let body = response.text().await.unwrap();
        assert!(
            body.contains("Authorization could not be completed"),
            "{body}"
        );
        // The page must not hand the authorization code back to whoever asked.
        assert!(!body.contains("abc"), "{body}");
    }

    #[test]
    fn oauth_callback_uses_the_gateway_address_family() {
        assert_eq!(
            oauth_redirect_uri("[::1]:8042".parse().unwrap()),
            "http://[::1]:8042/mcp/oauth/callback"
        );
        assert_eq!(
            oauth_redirect_uri("[::]:8042".parse().unwrap()),
            "http://[::1]:8042/mcp/oauth/callback"
        );
    }
}
