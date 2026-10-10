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
use anda_engine::extension::mcp::{
    McpOAuthConfig, McpServerConfig, McpToolProvider, McpTransportConfig,
    OAuthAuthorizationCodeConfig,
};
use axum::{
    extract::{RawQuery, State},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, oneshot},
    time::Instant,
};

use super::McpManager;
use crate::config::normalize_string;

/// Path the gateway serves the MCP OAuth redirect on.
pub const CALLBACK_PATH: &str = "/mcp/oauth/callback";

/// How long a started authorization stays completable.
///
/// Generous because the ceremony may include a sign-in step before the consent
/// page, and a remote user may still be opening their tunnel.
const FLOW_TTL: Duration = Duration::from_secs(600);

/// Name an OAuth client registers under, shown on the consent page.
pub(super) const OAUTH_CLIENT_NAME: &str = "Anda Bot";

/// What a caller blocked on a flow learns: whether the server was newly
/// written to mcp.json, or why the flow failed.
pub(super) type FlowOutcome = Result<bool, String>;

/// An authorization that has been started and is waiting for its redirect.
pub(super) struct PendingFlow {
    pub server: McpServerConfig,
    /// Handed out again while the flow is pending, so asking twice does not
    /// invalidate the URL the user is already working through.
    auth_url: String,
    expires_at: Instant,
    /// Present while a caller is blocked on this flow; absent once the caller
    /// handed the authorization URL to the user and returned.
    pub waiter: Option<oneshot::Sender<FlowOutcome>>,
}

/// Registry of in-flight MCP authorizations, keyed by the `state` the
/// authorization server echoes back on the redirect.
///
/// A started flow has registered its server with the provider; a flow that
/// expires or is abandoned removes that registration again.
#[derive(Clone)]
pub(super) struct McpOAuthFlows {
    inner: Arc<Inner>,
}

struct Inner {
    provider: Arc<McpToolProvider>,
    redirect_uri: String,
    pending: Mutex<HashMap<String, PendingFlow>>,
}

impl McpOAuthFlows {
    pub fn new(provider: Arc<McpToolProvider>, gateway_addr: std::net::SocketAddr) -> Self {
        Self {
            inner: Arc::new(Inner {
                provider,
                // Always loopback, whatever the gateway binds: this is the
                // address the *browser* resolves, reaching the daemon directly
                // on the desktop and through the user's tunnel over SSH.
                redirect_uri: oauth_redirect_uri(gateway_addr),
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

    /// The servers with an authorization in progress. Their registrations
    /// belong to the flow until it ends.
    pub async fn pending_servers(&self) -> Vec<McpServerConfig> {
        let mut pending = self.inner.pending.lock().await;
        self.expire_locked(&mut pending);
        pending.values().map(|flow| flow.server.clone()).collect()
    }

    pub async fn pending_ids(&self) -> BTreeSet<String> {
        self.pending_servers()
            .await
            .into_iter()
            .map(|server| server.id)
            .collect()
    }

    /// Takes the flow a redirect belongs to, matched by its `state`.
    pub async fn take(&self, redirect_url: &str) -> Result<PendingFlow, BoxError> {
        let state =
            state_from_url(redirect_url).ok_or("redirect URL carries no state parameter")?;
        let mut pending = self.inner.pending.lock().await;
        self.expire_locked(&mut pending);
        // An unknown state is the normal shape of a stray or replayed request:
        // say nothing about which servers exist.
        pending
            .remove(&state)
            .ok_or_else(|| "no pending MCP authorization matches this redirect".into())
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

    /// Drops any flow for `server_id`, with its registration.
    pub async fn cancel(&self, server_id: &str) {
        let mut pending = self.inner.pending.lock().await;
        let before = pending.len();
        pending.retain(|_, flow| flow.server.id != server_id);
        if pending.len() != before {
            self.inner.provider.remove_server(server_id);
        }
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

/// Only authentication changes during re-consent; filters and transport tuning
/// are the operator's configuration and must survive token replacement.
pub(super) fn configure_authorization(
    config: &mut McpServerConfig,
    redirect_uri: &str,
    scopes: Vec<String>,
    advertised_scopes: Vec<String>,
) -> Result<(), BoxError> {
    let McpTransportConfig::StreamableHttp(http) = &mut config.transport else {
        return Err("OAuth requires an HTTP MCP server".into());
    };
    let mut auth = match http.auth.take() {
        Some(McpOAuthConfig::AuthorizationCode(auth)) => auth,
        _ => OAuthAuthorizationCodeConfig {
            redirect_uri: redirect_uri.into(),
            scopes: vec![],
            client_name: None,
            client_id: None,
        },
    };
    auth.redirect_uri = redirect_uri.into();
    // Servers reloaded from mcp.json carry no name; without one, a dynamic
    // registration would show the engine's generic default on the consent page.
    auth.client_name
        .get_or_insert_with(|| OAUTH_CLIENT_NAME.into());
    if !scopes.is_empty() {
        auth.scopes = scopes;
    } else if auth.scopes.is_empty() {
        auth.scopes = advertised_scopes;
    }
    http.bearer_token = None;
    http.auth = Some(McpOAuthConfig::AuthorizationCode(auth));
    Ok(())
}

/// Derives a server id from a URL host, e.g. `https://api.al.ink/mcp` -> `api.al.ink`.
pub(super) fn default_server_id_from_url(url: &reqwest::Url) -> Result<String, BoxError> {
    let host = url.host_str().unwrap_or_default();
    // A bracketed IPv6 literal: the colons inside are part of the host.
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .trim_end_matches('.');
    normalize_string(host)
        .ok_or_else(|| "cannot derive an MCP server id from the url; pass an explicit id".into())
}

/// Best-effort launch of the user's default browser at `url`.
///
/// Only http/https URLs are opened: the authorization URL is built from the
/// remote server's discovery metadata, so treat it as untrusted input to the
/// local system.
pub(crate) async fn open_in_browser(url: &str) -> std::io::Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to open a non-http(s) URL in the browser",
        ));
    }

    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    // Not `cmd /C start`: cmd.exe would reparse the URL, splitting on `&`
    // (executing the rest as commands) and expanding `%..%` sequences.
    // rundll32 receives the URL as a plain argument.
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = tokio::process::Command::new("xdg-open");

    command.arg(url);
    run_browser_opener(&mut command).await
}

async fn run_browser_opener(command: &mut tokio::process::Command) -> std::io::Result<()> {
    let status = tokio::time::timeout(
        Duration::from_secs(10),
        command
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "browser opener timed out"))??;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "browser opener exited with {status}"
        )));
    }
    Ok(())
}

/// Handles the authorization server's redirect.
///
/// Unauthenticated by necessity, and deliberately uninformative: the response
/// tells the person at the browser whether they are done, and never echoes the
/// authorization code or names a server.
pub async fn mcp_oauth_callback(
    State(manager): State<McpManager>,
    RawQuery(query): RawQuery,
) -> impl IntoResponse {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return page(StatusCode::BAD_REQUEST, "Missing authorization response.");
    };
    // Rebuild the URL the authorization server was told to redirect to. Taking
    // it from the request instead would lose the scheme and host, and the token
    // exchange must see the registered redirect URI.
    let redirect_url = format!("{}?{}", manager.redirect_uri(), query);

    match manager.complete_authorization(&redirect_url).await {
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
    use anda_engine::extension::mcp::{McpLifecycle, McpTasksConfig};
    use axum::response::IntoResponse;

    fn flows() -> McpOAuthFlows {
        McpOAuthFlows::new(
            Arc::new(McpToolProvider::new(Vec::new()).unwrap()),
            "127.0.0.1:8042".parse().unwrap(),
        )
    }

    #[test]
    fn redirect_uri_is_loopback_on_the_gateway_port() {
        assert_eq!(
            flows().redirect_uri(),
            "http://127.0.0.1:8042/mcp/oauth/callback"
        );
        assert_eq!(
            oauth_redirect_uri("[::1]:8042".parse().unwrap()),
            "http://[::1]:8042/mcp/oauth/callback"
        );
        assert_eq!(
            oauth_redirect_uri("[::]:8042".parse().unwrap()),
            "http://[::1]:8042/mcp/oauth/callback"
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
    async fn abandoned_and_cancelled_flows_release_their_registration() {
        let flows = flows();
        for (id, state) in [("srv", "s-2"), ("other", "s-3")] {
            let server = McpServerConfig::streamable_http(id, "https://mcp.example.com/mcp");
            flows
                .inner
                .provider
                .register_server(server.clone())
                .unwrap();
            let auth_url = format!("https://as.example.com/authorize?client_id=x&state={state}");
            let _waiter = flows.begin(server, &auth_url).await.unwrap();
        }
        assert_eq!(
            flows.pending_ids().await,
            BTreeSet::from(["other".to_string(), "srv".to_string()])
        );

        flows
            .abandon("https://as.example.com/authorize?client_id=x&state=s-2")
            .await;
        flows.cancel("other").await;
        assert!(flows.pending_ids().await.is_empty());
        assert!(flows.inner.provider.server_ids().is_empty());
        let err = flows
            .take("http://127.0.0.1:8042/mcp/oauth/callback?code=c&state=s-2")
            .await
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "no pending MCP authorization matches this redirect"
        );
    }

    #[test]
    fn authorization_names_the_client_and_keeps_operator_settings() {
        let reloaded = crate::config::McpOAuthSettings {
            client_id: Some("registered-client".to_string()),
            scopes: vec!["read".to_string()],
        };
        let mut config = McpServerConfig::streamable_http("srv", "https://mcp.example.test/mcp");
        config.include.insert("read".into());
        config.exclude.insert("delete".into());
        config.lifecycle = McpLifecycle::Initialize;
        config.tasks = Some(McpTasksConfig::default());
        if let McpTransportConfig::StreamableHttp(http) = &mut config.transport {
            http.headers.insert("X-Tenant".into(), "tenant".into());
            http.auth = Some(reloaded.to_auth_config());
        }
        let before = serde_json::to_value(&config).unwrap();

        configure_authorization(&mut config, "http://127.0.0.1:8042/cb", vec![], vec![]).unwrap();
        let after = serde_json::to_value(&config).unwrap();
        for key in ["include", "exclude", "lifecycle", "tasks"] {
            assert_eq!(after[key], before[key]);
        }
        let McpTransportConfig::StreamableHttp(http) = &config.transport else {
            panic!("expected HTTP")
        };
        assert_eq!(http.headers["X-Tenant"], "tenant");
        let Some(McpOAuthConfig::AuthorizationCode(auth)) = &http.auth else {
            panic!("expected authorization code")
        };
        // Servers reloaded from mcp.json carry no client name of their own.
        assert_eq!(auth.client_name.as_deref(), Some(OAUTH_CLIENT_NAME));
        assert_eq!(auth.redirect_uri, "http://127.0.0.1:8042/cb");
        assert_eq!(auth.client_id.as_deref(), Some("registered-client"));
        assert_eq!(auth.scopes, vec!["read".to_string()]);

        configure_authorization(
            &mut config,
            "http://127.0.0.1:8042/cb",
            vec!["new".into()],
            vec!["advertised".into()],
        )
        .unwrap();
        let McpTransportConfig::StreamableHttp(http) = &config.transport else {
            panic!("expected HTTP")
        };
        let Some(McpOAuthConfig::AuthorizationCode(auth)) = &http.auth else {
            panic!("expected authorization code")
        };
        assert_eq!(auth.scopes, vec!["new".to_string()]);
    }

    #[test]
    fn default_server_id_from_url_uses_host() {
        let id = |url: &str| default_server_id_from_url(&reqwest::Url::parse(url).unwrap());
        assert_eq!(id("https://api.al.ink/mcp").unwrap(), "api.al.ink");
        assert_eq!(id("http://127.0.0.1:8080/mcp?x=1").unwrap(), "127.0.0.1");
        assert_eq!(
            id("https://user:pass@api.al.ink/mcp").unwrap(),
            "api.al.ink"
        );
        assert_eq!(id("https://API.al.ink./mcp").unwrap(), "api.al.ink");
        assert_eq!(id("http://[::1]:8080/mcp").unwrap(), "::1");
        assert!(id("http://./mcp").is_err());
    }

    #[tokio::test]
    async fn open_in_browser_rejects_non_http_urls() {
        assert!(open_in_browser("javascript:alert(1)").await.is_err());
        assert!(open_in_browser("file:///etc/passwd").await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn browser_opener_reports_process_failure_and_reaps_success() {
        assert!(
            run_browser_opener(tokio::process::Command::new("sh").args(["-c", "exit 7"]))
                .await
                .is_err()
        );
        run_browser_opener(tokio::process::Command::new("sh").args(["-c", "exit 0"]))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn callback_handler_reports_a_missing_query() {
        let dir = tempfile::tempdir().unwrap();
        let manager = McpManager::for_test(dir.path()).await;
        let response = mcp_oauth_callback(State(manager), RawQuery(None))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_route_answers_a_browser_carrying_no_credentials() {
        // Mounted the way `Engines::into_router` mounts it, and reached the way
        // the authorization server's redirect reaches it: a plain GET with no
        // Authorization header. A 401 here would break every OAuth flow.
        let dir = tempfile::tempdir().unwrap();
        let app = axum::Router::new()
            .route(CALLBACK_PATH, axum::routing::get(mcp_oauth_callback))
            .with_state(McpManager::for_test(dir.path()).await);
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
}
