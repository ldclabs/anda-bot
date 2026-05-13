use anda_core::BoxError;
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};

use super::{
    TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, parse_whisper_response, validate_audio,
};
use crate::config;

/// Groq Whisper API provider (default, backward-compatible with existing config).
pub struct GroqProvider {
    api_url: String,
    model: String,
    api_key: String,
    language: Option<String>,
    http: reqwest::Client,
}

impl GroqProvider {
    /// Build from the Groq provider configuration.
    pub fn from_config(
        config: &config::GroqSttConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("transcription.api_key must not be empty".into());
        }

        Ok(Self {
            api_url: config.api_url.clone(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            language: config.language.clone(),
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
        let (normalized_name, mime) = validate_audio(audio_data, file_name)?;

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let mut form = Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        if let Some(ref lang) = self.language {
            form = form.text("language", lang.clone());
        }

        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to Groq")?;

        parse_whisper_response(resp).await
    }
}
