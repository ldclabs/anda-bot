use anda_core::BoxError;
use async_trait::async_trait;

use super::{
    TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, parse_whisper_response, whisper_form,
};
use crate::config;

/// Groq Whisper API provider (default, backward-compatible with existing config).
pub struct GroqProvider {
    api_url: String,
    model: String,
    api_key: String,
    language: Option<String>,
    prompt: Option<String>,
    http: reqwest::Client,
}

impl GroqProvider {
    /// Build from the Groq provider configuration.
    pub fn from_config(
        config: &config::GroqSttConfig,
        initial_prompt: Option<&str>,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("transcription.groq.api_key must not be empty".into());
        }

        Ok(Self {
            api_url: config.api_url.clone(),
            model: config.model.clone(),
            api_key: config.api_key.trim().to_string(),
            language: config::normalize_optional(&config.language),
            prompt: initial_prompt.and_then(config::normalize_string),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for GroqProvider {
    fn name(&self) -> &str {
        "groq"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let form = whisper_form(
            audio_data,
            file_name,
            &self.model,
            self.language.as_deref(),
            self.prompt.as_deref(),
        )?;

        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send transcription request to Groq: {:?}",
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

    async fn spawn_mock(app: Router) -> String {
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/transcribe")
    }

    fn groq_config(api_url: String, language: Option<String>) -> config::GroqSttConfig {
        config::GroqSttConfig {
            api_key: "gsk-test".to_string(),
            api_url,
            model: "whisper-large-v3".to_string(),
            language,
        }
    }

    #[test]
    fn from_config_rejects_empty_api_key() {
        let config = config::GroqSttConfig::default();

        let err = GroqProvider::from_config(&config, None, new_reqwest_client())
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("api_key must not be empty"));
    }

    #[test]
    fn from_config_copies_fields() {
        let config = groq_config(
            "https://api.groq.com/v1".to_string(),
            Some("zh".to_string()),
        );

        let provider = GroqProvider::from_config(&config, None, new_reqwest_client()).unwrap();
        assert_eq!(provider.api_url, "https://api.groq.com/v1");
        assert_eq!(provider.model, "whisper-large-v3");
        assert_eq!(provider.language, Some("zh".to_string()));
        assert_eq!(provider.name(), "groq");
    }

    #[tokio::test]
    async fn transcribe_parses_whisper_json_response() {
        let app = Router::new().route(
            "/transcribe",
            routing::post(|| async { axum::Json(serde_json::json!({"text": "你好，世界"})) }),
        );
        let url = spawn_mock(app).await;
        let provider = GroqProvider::from_config(
            &groq_config(url, Some("zh".to_string())),
            None,
            new_reqwest_client(),
        )
        .unwrap();

        let text = provider.transcribe(b"data", "voice.oga").await.unwrap();
        assert_eq!(text, "你好，世界");
    }

    #[tokio::test]
    async fn transcribe_surfaces_api_error_status_and_body() {
        let app = Router::new().route(
            "/transcribe",
            routing::post(|| async {
                (http::StatusCode::INTERNAL_SERVER_ERROR, "model overloaded")
            }),
        );
        let url = spawn_mock(app).await;
        let provider =
            GroqProvider::from_config(&groq_config(url, None), None, new_reqwest_client()).unwrap();

        let err = provider.transcribe(b"data", "voice.mp3").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Transcription API error (500"), "got: {msg}");
        assert!(msg.contains("model overloaded"), "got: {msg}");
    }

    #[tokio::test]
    async fn transcribe_rejects_response_without_text_field() {
        let app = Router::new().route(
            "/transcribe",
            routing::post(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        );
        let url = spawn_mock(app).await;
        let provider =
            GroqProvider::from_config(&groq_config(url, None), None, new_reqwest_client()).unwrap();

        let err = provider.transcribe(b"data", "voice.wav").await.unwrap_err();
        assert!(err.to_string().contains("missing 'text' field"));
    }
}
