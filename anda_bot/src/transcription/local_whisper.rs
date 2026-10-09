use anda_core::BoxError;
use async_trait::async_trait;
use reqwest::multipart::Form;
use std::time::Duration;

use super::{
    TranscriptionProvider, check_audio_size, http_url, parse_whisper_response, whisper_file_part,
};
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
    timeout: Duration,
    http: reqwest::Client,
}

impl LocalWhisperProvider {
    /// Fails if `url` is empty, invalid or not HTTP/HTTPS, or if
    /// `max_audio_bytes` or `timeout_secs` is zero.
    pub fn new(
        config: &config::LocalWhisperConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let url = http_url(&config.url, "local_whisper.url")?;
        if config.max_audio_bytes == 0 {
            return Err(
                "`transcription.local_whisper.max_audio_bytes` must be greater than zero".into(),
            );
        }
        if config.timeout_secs == 0 {
            return Err(
                "`transcription.local_whisper.timeout_secs` must be greater than zero".into(),
            );
        }

        Ok(Self {
            url,
            bearer_token: config::normalize_optional(&config.bearer_token),
            max_audio_bytes: config.max_audio_bytes,
            timeout: Duration::from_secs(config.timeout_secs),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for LocalWhisperProvider {
    async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError> {
        check_audio_size(&audio, self.max_audio_bytes)?;
        let form = Form::new().part("file", whisper_file_part(audio, file_name)?);

        let mut request = self.http.post(&self.url);
        if let Some(bearer_token) = &self.bearer_token {
            request = request.bearer_auth(bearer_token);
        }
        let resp = request
            .multipart(form)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send audio to local Whisper endpoint: {:?}",
                    err.without_url()
                )
            })?;

        parse_whisper_response(resp).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;
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
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/v1/transcribe")
    }

    fn config_error(config: &config::LocalWhisperConfig) -> String {
        LocalWhisperProvider::new(config, new_reqwest_client())
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn new_validates_url_and_limits() {
        assert!(
            config_error(&whisper_config("  "))
                .contains("`transcription.local_whisper.url` must not be empty")
        );
        assert!(
            config_error(&whisper_config("ftp://localhost/transcribe"))
                .contains("must use http or https")
        );

        let mut config = whisper_config("http://localhost:8000");
        config.max_audio_bytes = 0;
        assert!(config_error(&config).contains("`transcription.local_whisper.max_audio_bytes`"));

        let mut config = whisper_config("http://localhost:8000");
        config.timeout_secs = 0;
        assert!(config_error(&config).contains("`transcription.local_whisper.timeout_secs`"));
    }

    #[test]
    fn new_normalizes_url_and_bearer_token() {
        let mut config = whisper_config(" http://localhost:8000 ");
        config.bearer_token = Some(" secret ".to_string());
        let provider = LocalWhisperProvider::new(&config, new_reqwest_client()).unwrap();
        assert_eq!(provider.url, "http://localhost:8000");
        assert_eq!(provider.bearer_token.as_deref(), Some("secret"));
        assert_eq!(provider.timeout, Duration::from_secs(5));

        config.bearer_token = Some("   ".to_string());
        let provider = LocalWhisperProvider::new(&config, new_reqwest_client()).unwrap();
        assert_eq!(provider.bearer_token, None);
    }

    #[tokio::test]
    async fn transcribe_rejects_audio_over_configured_limit() {
        let mut config = whisper_config("http://localhost:8000");
        config.max_audio_bytes = 4;
        let provider = LocalWhisperProvider::new(&config, new_reqwest_client()).unwrap();

        let err = provider
            .transcribe(b"12345".to_vec(), "voice.mp3")
            .await
            .unwrap_err();
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
        let provider = LocalWhisperProvider::new(&config, new_reqwest_client()).unwrap();
        let text = provider
            .transcribe(b"data".to_vec(), "voice.ogg")
            .await
            .unwrap();
        assert_eq!(text, "local transcript");

        // Without the token the mock rejects the request and the status error
        // is surfaced to the caller.
        let provider =
            LocalWhisperProvider::new(&whisper_config(&url), new_reqwest_client()).unwrap();
        let err = provider
            .transcribe(b"data".to_vec(), "voice.ogg")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Transcription API error (401"));
    }

    #[tokio::test]
    async fn connection_errors_keep_cause_without_url_credentials() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let config = whisper_config(&format!("http://{addr}/transcribe?api_key=do-not-expose"));
        let provider = LocalWhisperProvider::new(&config, new_reqwest_client()).unwrap();
        let error = provider
            .transcribe(b"audio".to_vec(), "voice.wav")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.to_lowercase().contains("connect"), "{error}");
        assert!(!error.contains("do-not-expose"), "{error}");
    }
}
