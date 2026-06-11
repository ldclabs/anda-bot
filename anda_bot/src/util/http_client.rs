use anda_core::BoxError;
use anda_engine::model::{Proxy, request_client_builder, reqwest};
use std::time::Duration;

/// Default `no_proxy` value for Anda Engine HTTP clients, covering common local and private network addresses.
pub static NO_PROXY: &str =
    "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,.local";

/// The default local/private exemptions merged with the `NO_PROXY` env var.
fn no_proxy_with_env() -> Option<reqwest::NoProxy> {
    let merged = match std::env::var("no_proxy").or_else(|_| std::env::var("NO_PROXY")) {
        Ok(env) if !env.trim().is_empty() => format!("{NO_PROXY},{env}"),
        _ => NO_PROXY.to_string(),
    };
    reqwest::NoProxy::from_string(&merged)
}

/// Proxies from the standard environment variables, each exempting local and
/// private network addresses. reqwest's built-in env-proxy support only
/// honors `$NO_PROXY`, which routes loopback traffic (daemon gateway, brain,
/// test mocks) through the proxy on machines where that variable is unset.
fn env_proxies() -> Vec<Proxy> {
    fn env_var(names: [&str; 2]) -> Option<String> {
        names.iter().find_map(|name| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                // Env proxy values commonly omit the scheme ("127.0.0.1:7890").
                .map(|v| {
                    if v.contains("://") {
                        v
                    } else {
                        format!("http://{v}")
                    }
                })
        })
    }

    let mut proxies = Vec::new();
    if let Some(url) = env_var(["http_proxy", "HTTP_PROXY"])
        && let Ok(proxy) = Proxy::http(&url)
    {
        proxies.push(proxy.no_proxy(no_proxy_with_env()));
    }
    if let Some(url) = env_var(["https_proxy", "HTTPS_PROXY"])
        && let Ok(proxy) = Proxy::https(&url)
    {
        proxies.push(proxy.no_proxy(no_proxy_with_env()));
    }
    if let Some(url) = env_var(["all_proxy", "ALL_PROXY"])
        && let Ok(proxy) = Proxy::all(&url)
    {
        proxies.push(proxy.no_proxy(no_proxy_with_env()));
    }
    proxies
}

/// Drop-in replacement for `reqwest::Client::new()` that keeps proxy env vars
/// working for external hosts but never proxies local or private addresses.
pub fn new_reqwest_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    for proxy in env_proxies() {
        builder = builder.proxy(proxy);
    }
    builder.build().expect("failed to build reqwest client")
}

#[derive(Clone, Copy, Debug)]
struct AnyHost;

impl PartialEq<&str> for AnyHost {
    fn eq(&self, _other: &&str) -> bool {
        true
    }
}

pub fn build_http_client<F>(https_proxy: Option<String>, f: F) -> Result<reqwest::Client, BoxError>
where
    F: FnOnce(reqwest::ClientBuilder) -> reqwest::ClientBuilder,
{
    let mut http_client = request_client_builder()
        .https_only(false)
        .timeout(Duration::from_secs(120))
        .retry(
            reqwest::retry::for_host(AnyHost)
                .max_retries_per_request(2)
                .classify_fn(|req_rep| {
                    if let Some(err) = req_rep.error() {
                        // Only replay requests that never reached the server.
                        // Retrying after a timeout or mid-response failure can
                        // double-submit non-idempotent calls (agent prompts,
                        // IM messages, memory formation); those layers have
                        // their own idempotency-aware retries.
                        let connect_failed = err
                            .downcast_ref::<reqwest::Error>()
                            .is_some_and(reqwest::Error::is_connect);
                        return if connect_failed {
                            req_rep.retryable()
                        } else {
                            req_rep.success()
                        };
                    }

                    match req_rep.status() {
                        Some(
                            http::StatusCode::REQUEST_TIMEOUT
                            | http::StatusCode::TOO_MANY_REQUESTS
                            | http::StatusCode::BAD_GATEWAY
                            | http::StatusCode::SERVICE_UNAVAILABLE
                            | http::StatusCode::GATEWAY_TIMEOUT,
                        ) => req_rep.retryable(),
                        _ => req_rep.success(),
                    }
                }),
        );
    if let Some(proxy) = &https_proxy {
        http_client = http_client.proxy(Proxy::all(proxy)?.no_proxy(no_proxy_with_env()));
    } else {
        for proxy in env_proxies() {
            http_client = http_client.proxy(proxy);
        }
    }
    let http_client = f(http_client).build()?;
    Ok(http_client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_host_matches_every_host() {
        assert!(AnyHost == "api.openai.com");
        assert!(AnyHost == "localhost");
        assert!(AnyHost == "");
    }

    #[test]
    fn build_http_client_without_proxy() {
        let client = build_http_client(None, |builder| builder).unwrap();
        // The client is usable; just make sure construction settles its config.
        let _ = format!("{client:?}");
    }

    #[test]
    fn build_http_client_with_proxy_applies_customizer() {
        let mut customized = false;
        let client = build_http_client(Some("http://127.0.0.1:7890".to_string()), |builder| {
            customized = true;
            builder.user_agent("anda-test")
        });

        assert!(client.is_ok());
        assert!(customized);
    }

    #[test]
    fn build_http_client_rejects_invalid_proxy() {
        let result = build_http_client(Some("://not-a-proxy".to_string()), |builder| builder);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_retries_retryable_status_codes() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let attempts = Arc::new(AtomicUsize::new(0));
        let handler_attempts = attempts.clone();
        let app = axum::Router::new().route(
            "/flaky",
            axum::routing::get(move || {
                let attempts = handler_attempts.clone();
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        (http::StatusCode::SERVICE_UNAVAILABLE, "warming up")
                    } else {
                        (http::StatusCode::OK, "ready")
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = build_http_client(None, |builder| builder).unwrap();
        let response = client
            .get(format!("http://{addr}/flaky"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::OK);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn client_does_not_retry_non_retryable_status_codes() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let attempts = Arc::new(AtomicUsize::new(0));
        let handler_attempts = attempts.clone();
        let app = axum::Router::new().route(
            "/broken",
            axum::routing::get(move || {
                let attempts = handler_attempts.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    (http::StatusCode::BAD_REQUEST, "no")
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = build_http_client(None, |builder| builder).unwrap();
        let response = client
            .get(format!("http://{addr}/broken"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn client_classifies_connect_failures_as_retryable() {
        let client = build_http_client(None, |builder| {
            builder.connect_timeout(Duration::from_millis(200))
        })
        .unwrap();

        // Nothing listens on port 1; the connect error path classifies the
        // request as retryable and the call still fails after retries.
        let err = client
            .get("http://127.0.0.1:1/unreachable")
            .send()
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.is_connect() || err.is_request());
    }
}
