//! Upgrade through the configured HTTP client so proxies and TLS settings apply.
use anda_core::BoxError;
use reqwest::{Client, StatusCode, header};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{handshake::derive_accept_key, protocol::Role},
};

pub(crate) async fn connect(
    client: &Client,
    url: &str,
) -> Result<WebSocketStream<reqwest::Upgraded>, BoxError> {
    let mut url = reqwest::Url::parse(url)?;
    let scheme = match url.scheme() {
        "ws" => "http",
        "wss" => "https",
        _ => return Err("expected ws or wss URL".into()),
    };
    url.set_scheme(scheme)
        .map_err(|_| "invalid websocket URL")?;
    let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
    let response = client
        .get(url)
        .version(reqwest::Version::HTTP_11)
        .header(header::CONNECTION, "Upgrade")
        .header(header::UPGRADE, "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", &key)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(reqwest::Error::without_url)?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS
        || response
            .headers()
            .get("Sec-WebSocket-Accept")
            .and_then(|v| v.to_str().ok())
            != Some(derive_accept_key(key.as_bytes()).as_str())
        || !response
            .headers()
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    {
        return Err(format!("invalid websocket upgrade response: {}", response.status()).into());
    }
    Ok(WebSocketStream::from_raw_socket(
        response
            .upgrade()
            .await
            .map_err(reqwest::Error::without_url)?,
        Role::Client,
        None,
    )
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    // The callback error type is fixed by tungstenite's server handshake API.
    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn websocket_uses_configured_http_proxy_and_headers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(
                socket,
                |req: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(req.headers()["x-channel-test"], "configured");
                    assert!(req.uri().to_string().contains("websocket.invalid/socket"));
                    Ok(response)
                },
            )
            .await
            .unwrap();
            ws.send(Message::Text("connected".into())).await.unwrap();
        });
        let mut headers = header::HeaderMap::new();
        headers.insert("x-channel-test", "configured".parse().unwrap());
        let client = Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(proxy).unwrap())
            .default_headers(headers)
            .build()
            .unwrap();
        let mut ws = connect(&client, "ws://websocket.invalid/socket")
            .await
            .unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap().into_text().unwrap(),
            "connected"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_upgrade_is_rejected() {
        let app = axum::Router::new().route("/", axum::routing::get(|| async { "ordinary HTTP" }));
        let url = crate::test_support::spawn_http_mock(app)
            .await
            .replace("http://", "ws://");
        assert!(
            connect(&crate::util::http_client::new_reqwest_client(), &url)
                .await
                .is_err()
        );
    }
}
