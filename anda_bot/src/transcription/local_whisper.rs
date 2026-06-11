use anda_core::BoxError;
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};

use super::{TranscriptionProvider, parse_whisper_response, resolve_audio_format};
use crate::config;

/// Self-hosted faster-whisper-compatible STT provider.
///
/// POSTs audio as `multipart/form-data` (field name `file`) to a configurable
/// HTTP endpoint (e.g. `http://localhost:8000` or a private network host). The endpoint
/// must return `{"text": "..."}`. No cloud API key required. Size limit is
/// configurable — not constrained by the 25 MB cloud API cap.
pub struct LocalWhisperProvider {
    url: String,
    bearer_token: Option<String>,
    max_audio_bytes: usize,
    timeout_secs: u64,
    http: reqwest::Client,
}

impl LocalWhisperProvider {
    /// Build from config. Fails if `url` is empty or invalid, if `url` is not
    /// HTTP/HTTPS, if `max_audio_bytes` is zero, or if `timeout_secs` is zero.
    pub fn from_config(
        config: &config::LocalWhisperConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let url = config.url.trim().to_string();
        if url.is_empty() {
            return Err("local_whisper: `url` must not be empty".into());
        }

        let parsed = url
            .parse::<reqwest::Url>()
            .map_err(|e| format!("local_whisper: invalid `url` {url:?}: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!(
                "local_whisper: `url` must use http or https scheme, got {:?}",
                parsed.scheme()
            )
            .into());
        }

        let bearer_token = config
            .bearer_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned);

        if config.max_audio_bytes == 0 {
            return Err("local_whisper: `max_audio_bytes` must be greater than zero".into());
        }

        if config.timeout_secs == 0 {
            return Err("local_whisper: `timeout_secs` must be greater than zero".into());
        }

        Ok(Self {
            url,
            bearer_token,
            max_audio_bytes: config.max_audio_bytes,
            timeout_secs: config.timeout_secs,
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for LocalWhisperProvider {
    fn name(&self) -> &str {
        "local_whisper"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        if audio_data.len() > self.max_audio_bytes {
            return Err(format!(
                "Audio file too large ({} bytes, local_whisper max {})",
                audio_data.len(),
                self.max_audio_bytes
            )
            .into());
        }

        let (normalized_name, mime) = resolve_audio_format(file_name)?;

        // to_vec() clones the buffer for the multipart payload; peak memory per
        // call is ~2× max_audio_bytes. TODO: replace with streaming upload once
        // reqwest supports body streaming in multipart parts.
        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let mut request = self.http.post(&self.url);
        if let Some(ref bearer_token) = self.bearer_token {
            request = request.bearer_auth(bearer_token);
        }

        let resp = request
            .multipart(Form::new().part("file", file_part))
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .map_err(|_| "Failed to send audio to local Whisper endpoint")?;

        parse_whisper_response(resp).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing};

    fn whisper_config(url: &str) -> config::LocalWhisperConfig {
        config::LocalWhisperConfig {
            url: url.to_string(),
            bearer_token: None,
            max_audio_bytes: 1024,
            timeout_secs: 5,
        }
    }

    async fn spawn_mock(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/v1/transcribe")
    }

    fn config_error(config: &config::LocalWhisperConfig) -> String {
        LocalWhisperProvider::from_config(config, reqwest::Client::new())
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn from_config_rejects_empty_url() {
        assert!(config_error(&whisper_config("  ")).contains("`url` must not be empty"));
    }

    #[test]
    fn from_config_rejects_invalid_url() {
        assert!(config_error(&whisper_config("not a url")).contains("invalid `url`"));
    }

    #[test]
    fn from_config_rejects_non_http_scheme() {
        assert!(
            config_error(&whisper_config("ftp://localhost/transcribe"))
                .contains("must use http or https")
        );
    }

    #[test]
    fn from_config_rejects_zero_limits() {
        let mut config = whisper_config("http://localhost:8000");
        config.max_audio_bytes = 0;
        assert!(config_error(&config).contains("`max_audio_bytes`"));

        let mut config = whisper_config("http://localhost:8000");
        config.timeout_secs = 0;
        assert!(config_error(&config).contains("`timeout_secs`"));
    }

    #[test]
    fn from_config_normalizes_bearer_token() {
        let mut config = whisper_config("http://localhost:8000");
        config.bearer_token = Some(" secret ".to_string());
        let provider =
            LocalWhisperProvider::from_config(&config, reqwest::Client::new()).unwrap();
        assert_eq!(provider.bearer_token.as_deref(), Some("secret"));
        assert_eq!(provider.name(), "local_whisper");

        config.bearer_token = Some("   ".to_string());
        let provider =
            LocalWhisperProvider::from_config(&config, reqwest::Client::new()).unwrap();
        assert_eq!(provider.bearer_token, None);
    }

    #[tokio::test]
    async fn transcribe_rejects_audio_over_configured_limit() {
        let mut config = whisper_config("http://localhost:8000");
        config.max_audio_bytes = 4;
        let provider =
            LocalWhisperProvider::from_config(&config, reqwest::Client::new()).unwrap();

        let err = provider.transcribe(b"12345", "voice.mp3").await.unwrap_err();
        assert!(err.to_string().contains("Audio file too large"));
    }

    #[tokio::test]
    async fn transcribe_sends_bearer_token_and_parses_response() {
        let app = Router::new().route(
            "/v1/transcribe",
            routing::post(|headers: http::HeaderMap| async move {
                if headers
                    .get(http::header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    != Some("Bearer secret")
                {
                    return (
                        http::StatusCode::UNAUTHORIZED,
                        axum::Json(serde_json::json!({"error": "unauthorized"})),
                    );
                }
                (
                    http::StatusCode::OK,
                    axum::Json(serde_json::json!({"text": "local transcript"})),
                )
            }),
        );
        let url = spawn_mock(app).await;

        let mut config = whisper_config(&url);
        config.bearer_token = Some("secret".to_string());
        let provider =
            LocalWhisperProvider::from_config(&config, reqwest::Client::new()).unwrap();
        let text = provider.transcribe(b"data", "voice.ogg").await.unwrap();
        assert_eq!(text, "local transcript");

        // Without the token the mock rejects the request and the status error
        // is surfaced to the caller.
        let provider =
            LocalWhisperProvider::from_config(&whisper_config(&url), reqwest::Client::new())
                .unwrap();
        let err = provider.transcribe(b"data", "voice.ogg").await.unwrap_err();
        assert!(err.to_string().contains("Transcription API error (401"));
    }
}
