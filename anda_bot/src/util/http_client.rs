use anda_core::BoxError;
use anda_engine::model::{Proxy, request_client_builder, reqwest};
use std::{net::SocketAddr, time::Duration};

/// Default `no_proxy` value for Anda Engine HTTP clients, covering common local and private network addresses.
pub static NO_PROXY: &str =
    "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,.local";

/// Install the rustls process-level crypto provider used by all reqwest clients.
///
/// rustls links `aws-lc-rs` through reqwest and may also link `ring` through
/// other transitive deps. With more than one provider compiled in, rustls cannot
/// choose automatically and panics on the first TLS handshake unless a default
/// provider is installed first. Ignoring the error is fine: it only fails if a
/// default provider was already installed.
pub fn install_default_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

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
    install_default_crypto_provider();
    let mut builder = reqwest::Client::builder().no_proxy();
    for proxy in env_proxies() {
        builder = builder.proxy(proxy);
    }
    builder.build().expect("failed to build reqwest client")
}

/// Whether an IP address is a public (globally routable) unicast address.
/// Used to reject model-controlled URLs that point at loopback, private,
/// link-local (cloud metadata), CGN, or otherwise internal addresses.
pub fn ip_is_public(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;

    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                // 0.0.0.0/8 "this network"
                || octets[0] == 0
                // 100.64.0.0/10 carrier-grade NAT
                || (octets[0] == 100 && (64..128).contains(&octets[1]))
                // 192.0.0.0/24 IETF protocol assignments
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                // 198.18.0.0/15 benchmarking
                || (octets[0] == 198 && (18..20).contains(&octets[1]))
                // 240.0.0.0/4 reserved
                || octets[0] >= 240)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            let seg0 = segments[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                // fc00::/7 unique local
                || (seg0 & 0xfe00) == 0xfc00
                // fe80::/10 link local
                || (seg0 & 0xffc0) == 0xfe80
                // fec0::/10 deprecated site-local addresses can still be
                // routed inside private IPv6 networks.
                || (seg0 & 0xffc0) == 0xfec0
                // 2001:db8::/32 documentation range
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicUrlPolicy {
    PublicOnly,
    #[cfg(test)]
    AllowPrivateForTests,
}

impl PublicUrlPolicy {
    fn permits(self, ip: std::net::IpAddr) -> bool {
        #[cfg(test)]
        if self == Self::AllowPrivateForTests {
            return true;
        }
        ip_is_public(ip)
    }
}

/// Resolve and validate a model-controlled HTTP target. The returned
/// addresses are subsequently installed as reqwest DNS overrides, binding the
/// actual connection to the exact addresses checked here instead of resolving
/// the hostname a second time (which would permit DNS rebinding).
async fn resolve_public_http_target(
    url: &reqwest::Url,
    policy: PublicUrlPolicy,
) -> Result<(String, Vec<SocketAddr>), BoxError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("unsupported public URL scheme: {}", url.scheme()).into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| format!("URL has no host: {url}"))?;
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| format!("URL does not have a known or explicit port: {url}"))?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !policy.permits(ip) {
            return Err(format!("URL {url} points at a private or internal address").into());
        }
        return Ok((host, vec![SocketAddr::new(ip, port)]));
    }

    let mut resolved = Vec::new();
    for addr in tokio::net::lookup_host((host.as_str(), port)).await? {
        // Every resolved address must be public: a rebinding name that mixes
        // public and private records must not slip through.
        if !policy.permits(addr.ip()) {
            return Err(format!("URL {url} resolves to a private or internal address").into());
        }
        if !resolved.contains(&addr) {
            resolved.push(addr);
        }
    }
    if resolved.is_empty() {
        return Err(format!("URL host does not resolve: {url}").into());
    }
    Ok((host, resolved))
}

const MAX_PUBLIC_URL_REDIRECTS: usize = 5;

fn pinned_public_http_client(
    host: &str,
    addresses: &[SocketAddr],
) -> Result<reqwest::Client, BoxError> {
    install_default_crypto_provider();
    // Model-controlled fetches intentionally bypass proxies: a proxy resolves
    // the target in its own network, which would break the guarantee that the
    // connection uses the addresses validated above.
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(300));
    if host.parse::<std::net::IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(host, addresses);
    }
    Ok(builder.build()?)
}

/// Fetches a model-controlled URL while rejecting private/internal targets.
/// Redirects are followed manually so every hop is validated before any
/// request is sent, and DNS overrides pin each connection to the addresses
/// that passed validation.
pub async fn fetch_public_url(
    mut url: reqwest::Url,
    policy: PublicUrlPolicy,
) -> Result<reqwest::Response, BoxError> {
    for redirect_count in 0..=MAX_PUBLIC_URL_REDIRECTS {
        let (host, addresses) = resolve_public_http_target(&url, policy).await?;
        let client = pinned_public_http_client(&host, &addresses)?;
        let response = client.get(url.clone()).send().await?;
        if !matches!(
            response.status(),
            http::StatusCode::MOVED_PERMANENTLY
                | http::StatusCode::FOUND
                | http::StatusCode::SEE_OTHER
                | http::StatusCode::TEMPORARY_REDIRECT
                | http::StatusCode::PERMANENT_REDIRECT
        ) {
            return Ok(response);
        }

        let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
            return Ok(response);
        };
        if redirect_count == MAX_PUBLIC_URL_REDIRECTS {
            return Err(format!("public URL exceeded {MAX_PUBLIC_URL_REDIRECTS} redirects").into());
        }
        let location = location
            .to_str()
            .map_err(|_| "public URL redirect location is not valid UTF-8")?;
        url = url.join(location)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!("unsupported public URL redirect scheme: {}", url.scheme()).into());
        }
    }

    unreachable!("redirect loop exits by response or error")
}

/// Reads a response body while enforcing `max_bytes` as the body streams in,
/// so an oversized (or Content-Length-less chunked) response can never be
/// fully buffered in memory before the size check.
pub async fn read_limited_body(
    mut response: reqwest::Response,
    max_bytes: u64,
    context: &str,
) -> Result<Vec<u8>, BoxError> {
    if let Some(content_length) = response.content_length()
        && content_length > max_bytes
    {
        return Err(format!("{context} exceeds {max_bytes} bytes: {content_length}").into());
    }

    let capacity = response
        .content_length()
        .unwrap_or(0)
        .min(max_bytes)
        .min(1024 * 1024) as usize;
    let mut body = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await? {
        if body.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(format!("{context} exceeds {max_bytes} bytes").into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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
    install_default_crypto_provider();
    let mut http_client = request_client_builder()
        .no_proxy()
        .https_only(false)
        .timeout(Duration::from_secs(300))
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
    fn ip_is_public_rejects_internal_ranges() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "192.0.0.1",
            "198.18.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "fec0::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!ip_is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["93.184.216.34", "8.8.8.8", "2606:4700::1111"] {
            assert!(ip_is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn public_target_guard_rejects_internal_urls() {
        for url in [
            "http://127.0.0.1:8080/secret",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://10.0.0.2/",
            "http://localhost/",
        ] {
            let url = reqwest::Url::parse(url).unwrap();
            assert!(
                resolve_public_http_target(&url, PublicUrlPolicy::PublicOnly)
                    .await
                    .is_err(),
                "{url}"
            );
        }

        let url = reqwest::Url::parse("http://8.8.8.8/").unwrap();
        assert!(
            resolve_public_http_target(&url, PublicUrlPolicy::PublicOnly)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn public_fetch_follows_only_validated_http_redirects() {
        use axum::{Router, response::Redirect, routing::get};

        let app = Router::new()
            .route("/redirect", get(|| async { Redirect::temporary("/final") }))
            .route("/final", get(|| async { "redirected body" }))
            .route(
                "/file-redirect",
                get(|| async { Redirect::temporary("file:///etc/passwd") }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let response = fetch_public_url(
            reqwest::Url::parse(&format!("http://{addr}/redirect")).unwrap(),
            PublicUrlPolicy::AllowPrivateForTests,
        )
        .await
        .unwrap();
        assert_eq!(response.url().path(), "/final");
        assert_eq!(response.text().await.unwrap(), "redirected body");

        let err = fetch_public_url(
            reqwest::Url::parse(&format!("http://{addr}/file-redirect")).unwrap(),
            PublicUrlPolicy::AllowPrivateForTests,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("redirect scheme"), "{err}");
    }

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
