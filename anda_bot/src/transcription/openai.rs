use anda_core::BoxError;
use async_trait::async_trait;

use super::{
    TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, parse_whisper_response, whisper_form,
};
use crate::config;

/// OpenAI Whisper API provider.
pub struct OpenAiWhisperProvider {
    api_key: String,
    model: String,
    prompt: Option<String>,
    http: reqwest::Client,
}

impl OpenAiWhisperProvider {
    pub fn from_config(
        config: &config::OpenAiSttConfig,
        initial_prompt: Option<&str>,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err("Missing OpenAI STT API key: set [transcription.openai].api_key".into());
        }

        Ok(Self {
            api_key: api_key.to_string(),
            model: config.model.clone(),
            prompt: initial_prompt.and_then(config::normalize_string),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for OpenAiWhisperProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let form = whisper_form(
            audio_data,
            file_name,
            &self.model,
            None,
            self.prompt.as_deref(),
        )?;

        let resp = self
            .http
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send transcription request to OpenAI: {:?}",
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

    #[test]
    fn from_config_rejects_empty_api_key() {
        let config = config::OpenAiSttConfig {
            api_key: "  ".to_string(),
            model: "whisper-1".to_string(),
        };

        let err = OpenAiWhisperProvider::from_config(&config, None, new_reqwest_client())
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Missing OpenAI STT API key"));
    }

    #[test]
    fn from_config_trims_api_key_and_copies_model() {
        let config = config::OpenAiSttConfig {
            api_key: " sk-test ".to_string(),
            model: "whisper-large".to_string(),
        };

        let provider = OpenAiWhisperProvider::from_config(
            &config,
            Some("  project names  "),
            new_reqwest_client(),
        )
        .unwrap();
        assert_eq!(provider.prompt.as_deref(), Some("project names"));
        assert_eq!(provider.api_key, "sk-test");
        assert_eq!(provider.model, "whisper-large");
        assert_eq!(provider.name(), "openai");
    }

    #[tokio::test]
    async fn transcribe_rejects_unsupported_audio_before_sending() {
        let config = config::OpenAiSttConfig {
            api_key: "sk-test".to_string(),
            model: "whisper-1".to_string(),
        };
        let provider =
            OpenAiWhisperProvider::from_config(&config, None, new_reqwest_client()).unwrap();

        let err = provider.transcribe(b"data", "voice.xyz").await.unwrap_err();
        assert!(err.to_string().contains("Unsupported audio format '.xyz'"));
    }
}
