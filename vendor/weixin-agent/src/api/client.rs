//! HTTP API client for the Weixin iLink Bot API.

use std::time::Duration;

use crate::config::WeixinConfig;
use crate::error::{Error, Result};
use crate::types::{
    BaseInfo, CHANNEL_VERSION, DEFAULT_CONFIG_TIMEOUT_MS, GetConfigRequest, GetConfigResponse,
    GetUpdatesRequest, GetUpdatesResponse, GetUploadUrlRequest, GetUploadUrlResponse, ILINK_APP_ID,
    SESSION_EXPIRED_ERRCODE, SendMessageRequest, SendTypingRequest, build_base_info_with_agent,
};
use crate::util::redact;

/// Encode version string as `(major<<16)|(minor<<8)|patch`.
fn build_client_version(version: &str) -> u32 {
    let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    let major = parts.first().copied().unwrap_or(0) & 0xff;
    let minor = parts.get(1).copied().unwrap_or(0) & 0xff;
    let patch = parts.get(2).copied().unwrap_or(0) & 0xff;
    (major << 16) | (minor << 8) | patch
}

/// Generate a random `X-WECHAT-UIN` header value.
fn random_wechat_uin() -> String {
    use base64::Engine;
    use rand::Rng;
    let n: u32 = rand::rng().random();
    base64::engine::general_purpose::STANDARD.encode(n.to_string().as_bytes())
}

fn ensure_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

fn api_error_from_json(value: &serde_json::Value) -> Option<Error> {
    let ret = value.get("ret").and_then(serde_json::Value::as_i64);
    let errcode = value.get("errcode").and_then(serde_json::Value::as_i64);
    let code = errcode.filter(|code| *code != 0).or(ret)?;

    if code == 0 {
        return None;
    }

    if code == i64::from(SESSION_EXPIRED_ERRCODE) {
        return Some(Error::SessionExpired);
    }

    let errmsg = value
        .get("errmsg")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("msg"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| value.to_string(), ToOwned::to_owned);

    Some(Error::Api {
        errcode: i32::try_from(code).unwrap_or_else(|_| {
            if code.is_negative() {
                i32::MIN
            } else {
                i32::MAX
            }
        }),
        errmsg,
    })
}

fn ensure_api_success(value: &serde_json::Value) -> Result<()> {
    if let Some(error) = api_error_from_json(value) {
        Err(error)
    } else {
        Ok(())
    }
}

/// Low-level HTTP client for all iLink Bot API endpoints.
pub(crate) struct HttpApiClient {
    base_url: String,
    token: String,
    route_tag: Option<u32>,
    api_timeout: Duration,
    bot_agent: String,
    http: reqwest::Client,
}

impl HttpApiClient {
    /// Shared API/CDN transport.
    pub fn http_client(&self) -> &reqwest::Client {
        &self.http
    }

    /// Create a new API client from config.
    pub fn new(config: &WeixinConfig) -> Self {
        Self {
            base_url: ensure_trailing_slash(&config.base_url),
            token: config.token.clone(),
            route_tag: config.route_tag,
            api_timeout: config.api_timeout,
            bot_agent: config.bot_agent.clone(),
            http: config.http_client.clone(),
        }
    }

    fn common_headers(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![
            ("iLink-App-Id", ILINK_APP_ID.to_owned()),
            (
                "iLink-App-ClientVersion",
                build_client_version(CHANNEL_VERSION).to_string(),
            ),
        ];
        if let Some(tag) = self.route_tag {
            h.push(("SKRouteTag", tag.to_string()));
        }
        h
    }

    fn post_headers(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![
            ("Content-Type", "application/json".to_owned()),
            ("AuthorizationType", "ilink_bot_token".to_owned()),
            ("X-WECHAT-UIN", random_wechat_uin()),
        ];
        if !self.token.is_empty() {
            h.push(("Authorization", format!("Bearer {}", self.token.trim())));
        }
        h.extend(self.common_headers());
        h
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &impl serde::Serialize,
        timeout: Option<Duration>,
    ) -> Result<T> {
        let url = format!("{}{endpoint}", self.base_url);
        let body_str = serde_json::to_string(body)?;
        tracing::debug!(
            url = redact::redact_url(&url),
            body = redact::redact_body_default(&body_str),
            "POST"
        );

        let mut builder = self.http.post(&url).body(body_str);
        if let Some(t) = timeout {
            builder = builder.timeout(t);
        }
        for (k, v) in self.post_headers() {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;
        let status = response.status();
        let raw = response.text().await?;
        tracing::debug!(
            status = %status,
            body = redact::redact_body_default(&raw),
            "response"
        );
        if !status.is_success() {
            return Err(Error::Api {
                errcode: i32::from(status.as_u16()),
                errmsg: raw,
            });
        }
        Ok(serde_json::from_str(&raw)?)
    }

    /// Long-poll `getUpdates`. On client-side timeout, returns an empty response.
    pub async fn get_updates(
        &self,
        request: &GetUpdatesRequest,
        timeout: Duration,
    ) -> Result<GetUpdatesResponse> {
        let url = format!("{}ilink/bot/getupdates", self.base_url);
        let body_str = serde_json::to_string(request)?;

        let mut builder = self.http.post(&url).timeout(timeout).body(body_str);
        for (k, v) in self.post_headers() {
            builder = builder.header(k, v);
        }

        match builder.send().await {
            Ok(response) => {
                let raw = response.text().await?;
                Ok(serde_json::from_str(&raw)?)
            }
            Err(e) if e.is_timeout() => {
                tracing::debug!("getUpdates: client-side timeout, returning empty response");
                Ok(GetUpdatesResponse {
                    ret: Some(0),
                    msgs: Some(Vec::new()),
                    get_updates_buf: Some(request.get_updates_buf.clone()),
                    ..Default::default()
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Send a message.
    pub async fn send_message(&self, request: &SendMessageRequest) -> Result<()> {
        let value: serde_json::Value = self
            .post_json("ilink/bot/sendmessage", request, Some(self.api_timeout))
            .await?;
        ensure_api_success(&value)?;
        Ok(())
    }

    /// Get a pre-signed CDN upload URL.
    pub async fn get_upload_url(
        &self,
        request: &GetUploadUrlRequest,
    ) -> Result<GetUploadUrlResponse> {
        self.post_json("ilink/bot/getuploadurl", request, Some(self.api_timeout))
            .await
    }

    /// Fetch bot config (`typing_ticket`).
    pub async fn get_config(
        &self,
        user_id: &str,
        context_token: Option<&str>,
    ) -> Result<GetConfigResponse> {
        let body = GetConfigRequest {
            ilink_user_id: user_id.to_owned(),
            context_token: context_token.map(String::from),
            base_info: self.base_info(),
        };
        self.post_json(
            "ilink/bot/getconfig",
            &body,
            Some(Duration::from_millis(DEFAULT_CONFIG_TIMEOUT_MS)),
        )
        .await
    }

    /// Send a typing indicator.
    pub async fn send_typing(&self, request: &SendTypingRequest) -> Result<()> {
        let value: serde_json::Value = self
            .post_json(
                "ilink/bot/sendtyping",
                request,
                Some(Duration::from_millis(DEFAULT_CONFIG_TIMEOUT_MS)),
            )
            .await?;
        ensure_api_success(&value)?;
        Ok(())
    }

    /// Build a `BaseInfo` with the configured `bot_agent`.
    pub fn base_info(&self) -> BaseInfo {
        build_base_info_with_agent(&self.bot_agent)
    }

    /// POST request without auth token (for QR login endpoint).
    pub async fn api_post(
        &self,
        endpoint: &str,
        body: &impl serde::Serialize,
        timeout: Option<Duration>,
    ) -> Result<String> {
        let url = format!("{}{endpoint}", self.base_url);
        let body_str = serde_json::to_string(body)?;
        tracing::debug!(
            url = redact::redact_url(&url),
            body = redact::redact_body_default(&body_str),
            "POST (no-auth)"
        );

        let mut builder = self.http.post(&url).body(body_str);
        if let Some(t) = timeout {
            builder = builder.timeout(t);
        }
        builder = builder
            .header("Content-Type", "application/json")
            .header("AuthorizationType", "ilink_bot_token")
            .header("X-WECHAT-UIN", random_wechat_uin());
        for (k, v) in self.common_headers() {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;
        let status = response.status();
        let raw = response.text().await?;
        if !status.is_success() {
            return Err(Error::Api {
                errcode: i32::from(status.as_u16()),
                errmsg: raw,
            });
        }
        Ok(raw)
    }

    /// Notify server that this bot is starting (best-effort).
    pub async fn notify_start(&self) -> Result<()> {
        let body = serde_json::json!({ "base_info": self.base_info() });
        let value: serde_json::Value = self
            .post_json(
                "ilink/bot/msg/notifystart",
                &body,
                Some(Duration::from_secs(10)),
            )
            .await?;
        ensure_api_success(&value)?;
        Ok(())
    }

    /// Notify server that this bot is stopping (best-effort).
    pub async fn notify_stop(&self) -> Result<()> {
        let body = serde_json::json!({ "base_info": self.base_info() });
        let value: serde_json::Value = self
            .post_json(
                "ilink/bot/msg/notifystop",
                &body,
                Some(Duration::from_secs(10)),
            )
            .await?;
        ensure_api_success(&value)?;
        Ok(())
    }

    /// GET request for QR code endpoints.
    pub async fn api_get(&self, endpoint: &str, timeout: Duration) -> Result<String> {
        let url = format!("{}{endpoint}", self.base_url);
        tracing::debug!(url = redact::redact_url(&url), "GET");

        let mut builder = self.http.get(&url).timeout(timeout);
        for (k, v) in self.common_headers() {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;
        let status = response.status();
        let raw = response.text().await?;
        if !status.is_success() {
            return Err(Error::Api {
                errcode: i32::from(status.as_u16()),
                errmsg: raw,
            });
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn build_client_version_encoding() {
        assert_eq!(build_client_version("2.1.1"), (2 << 16) | (1 << 8) | 1);
        assert_eq!(build_client_version("1.0.0"), 1 << 16);
        assert_eq!(build_client_version("0.0.1"), 1);
        assert_eq!(build_client_version(""), 0);
    }

    #[test]
    fn ensure_trailing_slash_adds() {
        assert_eq!(
            ensure_trailing_slash("https://example.com"),
            "https://example.com/"
        );
    }

    #[test]
    fn ensure_trailing_slash_noop() {
        assert_eq!(
            ensure_trailing_slash("https://example.com/"),
            "https://example.com/"
        );
    }

    #[test]
    fn random_wechat_uin_format() {
        use base64::Engine;
        let uin = random_wechat_uin();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&uin)
            .unwrap();
        let s = std::str::from_utf8(&decoded).unwrap();
        assert!(s.parse::<u32>().is_ok());
    }

    #[test]
    fn api_status_accepts_success_body() {
        let value = serde_json::json!({ "ret": 0, "errmsg": "" });

        ensure_api_success(&value).unwrap();
    }

    #[test]
    fn api_status_rejects_nonzero_body() {
        let value = serde_json::json!({ "ret": 123, "errmsg": "bad request" });
        let err = ensure_api_success(&value).unwrap_err();

        assert!(matches!(
            err,
            Error::Api {
                errcode: 123,
                errmsg
            } if errmsg == "bad request"
        ));
    }

    #[test]
    fn api_status_maps_session_expired_body() {
        let value = serde_json::json!({ "errcode": SESSION_EXPIRED_ERRCODE });

        assert!(matches!(
            ensure_api_success(&value).unwrap_err(),
            Error::SessionExpired
        ));
    }
}
