use anda_core::BoxError;
use async_trait::async_trait;
use reqwest::multipart::Form;

use super::{
    MAX_AUDIO_BYTES, TRANSCRIPTION_TIMEOUT, TranscriptionProvider, check_audio_size, http_url,
    parse_whisper_response, required, whisper_file_part,
};
use crate::config;

const OPENAI_TRANSCRIPTIONS_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

/// Whisper-compatible cloud API (Groq or OpenAI): a multipart upload
/// authenticated with a bearer key, answered with `{ "text": "..." }`.
pub struct WhisperApiProvider {
    /// Service name used in error messages.
    service: &'static str,
    api_url: String,
    api_key: String,
    model: String,
    language: Option<String>,
    prompt: Option<String>,
    http: reqwest::Client,
}

impl WhisperApiProvider {
    pub fn groq(
        config: &config::GroqSttConfig,
        initial_prompt: Option<&str>,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        Ok(Self {
            service: "Groq",
            api_key: required(&config.api_key, "groq.api_key")?,
            api_url: http_url(&config.api_url, "groq.api_url")?,
            model: required(&config.model, "groq.model")?,
            language: config::normalize_optional(&config.language),
            prompt: initial_prompt.and_then(config::normalize_string),
            http,
        })
    }

    pub fn openai(
        config: &config::OpenAiSttConfig,
        initial_prompt: Option<&str>,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        Ok(Self {
            service: "OpenAI",
            api_key: required(&config.api_key, "openai.api_key")?,
            api_url: OPENAI_TRANSCRIPTIONS_URL.to_string(),
            model: required(&config.model, "openai.model")?,
            language: None,
            prompt: initial_prompt.and_then(config::normalize_string),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for WhisperApiProvider {
    async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError> {
        check_audio_size(&audio, MAX_AUDIO_BYTES)?;
        let mut form = Form::new()
            .part("file", whisper_file_part(audio, file_name)?)
            .text("model", self.model.clone())
            .text("response_format", "json");
        if let Some(language) = &self.language {
            form = form.text("language", language.clone());
        }
        if let Some(prompt) = &self.prompt {
            form = form.text("prompt", prompt.clone());
        }

        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(TRANSCRIPTION_TIMEOUT)
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send transcription request to {}: {:?}",
                    self.service,
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

    fn groq(api_url: String) -> WhisperApiProvider {
        WhisperApiProvider::groq(&groq_config(api_url, None), None, new_reqwest_client()).unwrap()
    }

    fn error(provider: Result<WhisperApiProvider, BoxError>) -> String {
        provider.map(|_| ()).unwrap_err().to_string()
    }

    #[test]
    fn constructors_validate_required_fields() {
        let http = new_reqwest_client;
        assert!(
            error(WhisperApiProvider::groq(&Default::default(), None, http()))
                .contains("`transcription.groq.api_key` must not be empty")
        );
        let mut config = groq_config(" ".to_string(), None);
        assert!(
            error(WhisperApiProvider::groq(&config, None, http()))
                .contains("`transcription.groq.api_url` must not be empty")
        );
        config.api_url = "ftp://api.groq.com".to_string();
        assert!(error(WhisperApiProvider::groq(&config, None, http())).contains("http or https"));

        let config = config::OpenAiSttConfig {
            api_key: "  ".to_string(),
            model: "whisper-1".to_string(),
        };
        assert!(
            error(WhisperApiProvider::openai(&config, None, http()))
                .contains("`transcription.openai.api_key` must not be empty")
        );
        let config = config::OpenAiSttConfig {
            api_key: "sk-test".to_string(),
            model: " ".to_string(),
        };
        assert!(
            error(WhisperApiProvider::openai(&config, None, http()))
                .contains("`transcription.openai.model` must not be empty")
        );
    }

    #[test]
    fn constructors_trim_and_copy_fields() {
        let mut config = groq_config(
            " https://api.groq.com/v1 ".to_string(),
            Some(" zh ".to_string()),
        );
        config.api_key = " gsk-test ".to_string();
        let provider = WhisperApiProvider::groq(&config, None, new_reqwest_client()).unwrap();
        assert_eq!(provider.api_url, "https://api.groq.com/v1");
        assert_eq!(provider.api_key, "gsk-test");
        assert_eq!(provider.model, "whisper-large-v3");
        assert_eq!(provider.language.as_deref(), Some("zh"));
        assert_eq!(provider.prompt, None);

        let config = config::OpenAiSttConfig {
            api_key: " sk-test ".to_string(),
            model: "whisper-large".to_string(),
        };
        let provider =
            WhisperApiProvider::openai(&config, Some("  project names  "), new_reqwest_client())
                .unwrap();
        assert_eq!(provider.api_url, OPENAI_TRANSCRIPTIONS_URL);
        assert_eq!(provider.api_key, "sk-test");
        assert_eq!(provider.model, "whisper-large");
        assert_eq!(provider.language, None);
        assert_eq!(provider.prompt.as_deref(), Some("project names"));
    }

    #[tokio::test]
    async fn transcribe_sends_model_and_language_and_parses_the_text() {
        // The mock echoes the multipart body back as the transcript.
        let app = Router::new().route(
            "/transcribe",
            routing::post(|body: axum::body::Bytes| async move {
                axum::Json(serde_json::json!({"text": String::from_utf8_lossy(&body)}))
            }),
        );
        let url = spawn_mock(app).await;
        let provider = WhisperApiProvider::groq(
            &groq_config(url, Some("zh".to_string())),
            None,
            new_reqwest_client(),
        )
        .unwrap();

        let body = provider
            .transcribe("你好".as_bytes().to_vec(), "voice.oga")
            .await
            .unwrap();
        assert!(body.contains("filename=\"voice.ogg\""), "{body}");
        assert!(
            body.contains("Content-Type: audio/ogg\r\n\r\n你好\r\n"),
            "{body}"
        );
        assert!(body.contains("name=\"model\"\r\n\r\nwhisper-large-v3\r\n"));
        assert!(body.contains("name=\"response_format\"\r\n\r\njson\r\n"));
        assert!(body.contains("name=\"language\"\r\n\r\nzh\r\n"));
    }

    #[tokio::test]
    async fn transcribe_surfaces_api_error_status_and_body() {
        let app = Router::new().route(
            "/transcribe",
            routing::post(|| async {
                (http::StatusCode::INTERNAL_SERVER_ERROR, "model overloaded")
            }),
        );
        let provider = groq(spawn_mock(app).await);

        let err = provider
            .transcribe(b"data".to_vec(), "voice.mp3")
            .await
            .unwrap_err();
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
        let provider = groq(spawn_mock(app).await);

        let err = provider
            .transcribe(b"data".to_vec(), "voice.wav")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("missing field `text`"), "{err}");
    }

    #[tokio::test]
    async fn transcribe_rejects_bad_audio_before_sending() {
        // Port 9 is never contacted: validation fails first.
        let provider = groq("http://127.0.0.1:9/transcribe".to_string());

        let err = provider
            .transcribe(b"data".to_vec(), "voice.xyz")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Unsupported audio format '.xyz'"));
        let err = provider
            .transcribe(Vec::new(), "voice.wav")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }
}
