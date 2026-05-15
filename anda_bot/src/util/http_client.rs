use anda_core::BoxError;
use anda_engine::model::{Proxy, request_client_builder, reqwest};
use std::time::Duration;

/// Default `no_proxy` value for Anda Engine HTTP clients, covering common local and private network addresses.
pub static NO_PROXY: &str =
    "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,.local";

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
                    if req_rep.error().is_some() {
                        return req_rep.retryable();
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
        http_client =
            http_client.proxy(Proxy::all(proxy)?.no_proxy(reqwest::NoProxy::from_string(NO_PROXY)));
    }
    let http_client = f(http_client).build()?;
    Ok(http_client)
}
