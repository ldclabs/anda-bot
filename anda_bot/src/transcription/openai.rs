use anda_core::BoxError;
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};

use super::{
    TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, parse_whisper_response, validate_audio,
};
use crate::config;

/// OpenAI Whisper API provider.
pub struct OpenAiWhisperProvider {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl OpenAiWhisperProvider {
    pub fn from_config(
        config: &config::OpenAiSttConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err("Missing OpenAI STT API key: set [transcription.openai].api_key".into());
        }

        Ok(Self {
            api_key: api_key.to_string(),
            model: config.model.clone(),
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
        let (normalized_name, mime) = validate_audio(audio_data, file_name)?;

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let form = Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        let resp = self
            .http
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to OpenAI")?;

        parse_whisper_response(resp).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_rejects_empty_api_key() {
        let config = config::OpenAiSttConfig {
            api_key: "  ".to_string(),
            model: "whisper-1".to_string(),
        };

        let err = OpenAiWhisperProvider::from_config(&config, reqwest::Client::new())
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

        let provider = OpenAiWhisperProvider::from_config(&config, reqwest::Client::new()).unwrap();
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
        let provider = OpenAiWhisperProvider::from_config(&config, reqwest::Client::new()).unwrap();

        let err = provider.transcribe(b"data", "voice.xyz").await.unwrap_err();
        assert!(err.to_string().contains("Unsupported audio format '.xyz'"));
    }
}
