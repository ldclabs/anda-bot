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
use anda_engine::{extension::mcp::McpToolProvider, unix_ms};
use axum::{
    extract::{RawQuery, State},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, oneshot};

use super::mcp_server::persist_oauth_server;

/// Path the gateway serves the MCP OAuth redirect on.
pub const CALLBACK_PATH: &str = "/mcp/oauth/callback";

/// How long a started authorization stays completable.
///
/// Generous because the ceremony may include a sign-in step before the consent
/// page, and a remote user may still be opening their tunnel.
const FLOW_TTL: Duration = Duration::from_secs(600);

/// An authorization that has been started and is waiting for its redirect.
struct PendingFlow {
    server_id: String,
    /// Endpoint URL, kept so the entry can be written to mcp.json on success.
    url: String,
    scopes: Vec<String>,
    expires_at: u64,
    /// Present while a caller is blocked on this flow; absent once the caller
    /// handed the authorization URL to the user and returned.
    waiter: Option<oneshot::Sender<Result<(), String>>>,
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
    redirect_uri: String,
    config_path: PathBuf,
    config_write_lock: Arc<Mutex<()>>,
    pending: Mutex<HashMap<String, PendingFlow>>,
}

impl McpOAuthFlows {
    pub fn new(
        provider: Arc<McpToolProvider>,
        gateway_port: u16,
        config_path: PathBuf,
        config_write_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                provider,
                // Always loopback, whatever the gateway binds: this is the
                // address the *browser* resolves, reaching the daemon directly
                // on the desktop and through the user's tunnel over SSH.
                redirect_uri: format!("http://127.0.0.1:{gateway_port}{CALLBACK_PATH}"),
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
        server_id: String,
        url: String,
        scopes: Vec<String>,
        auth_url: &str,
    ) -> Result<oneshot::Receiver<Result<(), String>>, BoxError> {
        let state = state_from_url(auth_url)
            .ok_or("authorization URL carries no state parameter to match its redirect against")?;
        let (tx, rx) = oneshot::channel();
        let mut pending = self.inner.pending.lock().await;
        self.expire_locked(&mut pending);
        pending.insert(
            state,
            PendingFlow {
                server_id,
                url,
                scopes,
                expires_at: unix_ms() + FLOW_TTL.as_millis() as u64,
                waiter: Some(tx),
            },
        );
        Ok(rx)
    }

    /// Finishes the flow the redirect belongs to: exchanges the code, connects,
    /// and persists the server.
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
        let Some(mut flow) = flow else {
            return Err("no pending MCP authorization matches this redirect".into());
        };

        let waiter = flow.waiter.take();
        let result = self.finish(&flow, redirect_url).await;
        if let Some(waiter) = waiter {
            let _ = waiter.send(
                result
                    .as_ref()
                    .map(|_| ())
                    .map_err(|err: &BoxError| err.to_string()),
            );
        }
        if result.is_err() {
            // The pending PKCE state is consumed either way, so leaving the
            // registration behind would only produce a server that can never
            // connect.
            self.inner.provider.remove_server(&flow.server_id);
        }
        result
    }

    /// Drops a flow that was started but will not be finished.
    pub async fn abandon(&self, auth_url: &str) {
        let Some(state) = state_from_url(auth_url) else {
            return;
        };
        let flow = self.inner.pending.lock().await.remove(&state);
        if let Some(flow) = flow {
            self.inner.provider.cancel_authorization(&flow.server_id);
            self.inner.provider.remove_server(&flow.server_id);
        }
    }

    async fn finish(
        &self,
        flow: &PendingFlow,
        redirect_url: &str,
    ) -> Result<CompletedFlow, BoxError> {
        self.inner
            .provider
            .complete_authorization(&flow.server_id, redirect_url)
            .await?;
        self.inner.provider.refresh_server(&flow.server_id).await?;
        let persisted = persist_oauth_server(
            &self.inner.config_path,
            &self.inner.config_write_lock,
            &flow.server_id,
            flow.url.clone(),
            flow.scopes.clone(),
        )
        .await?;
        Ok(CompletedFlow {
            server_id: flow.server_id.clone(),
            persisted,
        })
    }

    /// Drops flows whose authorization window has closed, releasing the
    /// half-registered server each one left behind.
    fn expire_locked(&self, pending: &mut HashMap<String, PendingFlow>) {
        let now = unix_ms();
        pending.retain(|_, flow| {
            if flow.expires_at > now {
                return true;
            }
            self.inner.provider.cancel_authorization(&flow.server_id);
            self.inner.provider.remove_server(&flow.server_id);
            false
        });
    }
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
            8042,
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
        let auth_url = "https://as.example.com/authorize?client_id=x&state=s-1";
        let _waiter = flows
            .begin(
                "srv".to_string(),
                "https://mcp.example.com/mcp".to_string(),
                vec![],
                auth_url,
            )
            .await
            .unwrap();

        // The exchange fails (there is no real authorization server), but it got
        // as far as naming the server — which is the point: `state` routed it.
        let err = flows
            .complete("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=s-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("srv"), "{err}");

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
    async fn abandoning_a_flow_makes_its_redirect_unmatched() {
        let flows = flows();
        let auth_url = "https://as.example.com/authorize?client_id=x&state=s-2";
        let _waiter = flows
            .begin(
                "srv".to_string(),
                "https://mcp.example.com/mcp".to_string(),
                vec![],
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
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let response = reqwest::get(format!(
            "http://{addr}{CALLBACK_PATH}?code=abc&state=unknown"
        ))
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
}
