//! Protocol-level configuration for the SDK.

use std::time::Duration;

use crate::error::{Error, Result};

/// Default API base URL.
pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com/";
/// Default CDN base URL (no trailing slash).
pub const DEFAULT_CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
/// Default long-poll timeout.
pub const DEFAULT_LONG_POLL_TIMEOUT: Duration = Duration::from_secs(35);
/// Default API request timeout.
pub const DEFAULT_API_TIMEOUT: Duration = Duration::from_secs(15);

/// SDK protocol configuration — no persistence paths.
#[derive(Debug, Clone)]
pub struct WeixinConfig {
    /// Shared HTTP transport (including proxies and connection pooling).
    pub http_client: reqwest::Client,
    /// iLink API base URL.
    pub base_url: String,
    /// CDN base URL.
    pub cdn_base_url: String,
    /// Bot authentication token.
    pub token: String,
    /// Optional route tag header value.
    pub route_tag: Option<u32>,
    /// Long-poll timeout for `getUpdates`.
    pub long_poll_timeout: Duration,
    /// Timeout for regular API calls.
    pub api_timeout: Duration,
    /// Bot agent string sent in all API requests (UA-style).
    pub bot_agent: String,
    /// Whether to apply markdown filter on outbound text. Default: `true`.
    pub markdown_filter_enabled: bool,
}

/// Builder for [`WeixinConfig`].
#[derive(Debug, Default)]
#[must_use]
pub struct WeixinConfigBuilder {
    http_client: Option<reqwest::Client>,
    base_url: Option<String>,
    cdn_base_url: Option<String>,
    token: Option<String>,
    route_tag: Option<u32>,
    long_poll_timeout: Option<Duration>,
    api_timeout: Option<Duration>,
    bot_agent: Option<String>,
    markdown_filter_enabled: Option<bool>,
}

impl WeixinConfig {
    /// Create a new builder.
    pub fn builder() -> WeixinConfigBuilder {
        WeixinConfigBuilder::default()
    }
}

impl WeixinConfigBuilder {
    /// Inject a shared HTTP client for all API and CDN traffic.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Set the API base URL (default: `https://ilinkai.weixin.qq.com/`).
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the CDN base URL.
    pub fn cdn_base_url(mut self, url: impl Into<String>) -> Self {
        self.cdn_base_url = Some(url.into());
        self
    }

    /// Set the bot token (required).
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Set the optional route tag.
    pub fn route_tag(mut self, tag: u32) -> Self {
        self.route_tag = Some(tag);
        self
    }

    /// Set the long-poll timeout.
    pub fn long_poll_timeout(mut self, d: Duration) -> Self {
        self.long_poll_timeout = Some(d);
        self
    }

    /// Set the regular API timeout.
    pub fn api_timeout(mut self, d: Duration) -> Self {
        self.api_timeout = Some(d);
        self
    }

    /// Set the bot agent string (default: `"weixin-agent-rs"`).
    pub fn bot_agent(mut self, agent: impl Into<String>) -> Self {
        self.bot_agent = Some(agent.into());
        self
    }

    /// Enable or disable the outbound markdown filter (default: `true`).
    pub fn markdown_filter(mut self, enabled: bool) -> Self {
        self.markdown_filter_enabled = Some(enabled);
        self
    }

    /// Build the config. Returns an error if `token` is missing.
    pub fn build(self) -> Result<WeixinConfig> {
        let token = self
            .token
            .ok_or_else(|| Error::Config("token is required".into()))?;
        Ok(WeixinConfig {
            http_client: self.http_client.unwrap_or_default(),
            base_url: self.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            cdn_base_url: self
                .cdn_base_url
                .unwrap_or_else(|| DEFAULT_CDN_BASE_URL.to_owned()),
            token,
            route_tag: self.route_tag,
            long_poll_timeout: self.long_poll_timeout.unwrap_or(DEFAULT_LONG_POLL_TIMEOUT),
            api_timeout: self.api_timeout.unwrap_or(DEFAULT_API_TIMEOUT),
            bot_agent: self
                .bot_agent
                .unwrap_or_else(|| "weixin-agent-rs".to_owned()),
            markdown_filter_enabled: self.markdown_filter_enabled.unwrap_or(true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let cfg = WeixinConfig::builder().token("tok").build().unwrap();
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
        assert_eq!(cfg.cdn_base_url, DEFAULT_CDN_BASE_URL);
        assert_eq!(cfg.long_poll_timeout, DEFAULT_LONG_POLL_TIMEOUT);
        assert_eq!(cfg.api_timeout, DEFAULT_API_TIMEOUT);
        assert!(cfg.route_tag.is_none());
        assert_eq!(cfg.bot_agent, "weixin-agent-rs");
        assert!(cfg.markdown_filter_enabled);
    }

    #[test]
    fn missing_token_error() {
        let result = WeixinConfig::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn custom_values() {
        let cfg = WeixinConfig::builder()
            .token("my_token")
            .base_url("https://custom.example.com/")
            .cdn_base_url("https://cdn.example.com")
            .route_tag(42)
            .long_poll_timeout(Duration::from_secs(10))
            .api_timeout(Duration::from_secs(5))
            .bot_agent("my-bot/1.0")
            .markdown_filter(false)
            .build()
            .unwrap();
        assert_eq!(cfg.token, "my_token");
        assert_eq!(cfg.base_url, "https://custom.example.com/");
        assert_eq!(cfg.cdn_base_url, "https://cdn.example.com");
        assert_eq!(cfg.route_tag, Some(42));
        assert_eq!(cfg.long_poll_timeout, Duration::from_secs(10));
        assert_eq!(cfg.api_timeout, Duration::from_secs(5));
        assert_eq!(cfg.bot_agent, "my-bot/1.0");
        assert!(!cfg.markdown_filter_enabled);
    }
}
