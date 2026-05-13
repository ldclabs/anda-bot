use anda_core::BoxError;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

use super::{TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, validate_audio};
use crate::config;

/// Google Cloud Speech-to-Text API provider.
pub struct GoogleSttProvider {
    api_key: String,
    language_code: String,
    http: reqwest::Client,
}

impl GoogleSttProvider {
    pub fn from_config(
        config: &config::GoogleSttConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err("Missing Google STT API key: set [transcription.google].api_key".into());
        }

        Ok(Self {
            api_key: api_key.to_string(),
            language_code: config.language_code.clone(),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for GoogleSttProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let (normalized_name, _) = validate_audio(audio_data, file_name)?;

        let encoding = match normalized_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("flac") => "FLAC",
            Some("wav") => "LINEAR16",
            Some("ogg" | "opus") => "OGG_OPUS",
            Some("mp3") => "MP3",
            Some("webm") => "WEBM_OPUS",
            Some(ext) => return Err(format!("Google STT does not support '.{ext}' input").into()),
            None => return Err("Google STT requires a file extension".into()),
        };

        let audio_content = STANDARD.encode(audio_data);

        let request_body = json!({
            "config": {
                "encoding": encoding,
                "languageCode": &self.language_code,
                "enableAutomaticPunctuation": true,
            },
            "audio": {
                "content": audio_content,
            }
        });

        let url = format!(
            "https://speech.googleapis.com/v1/speech:recognize?key={}",
            self.api_key
        );

        let resp = self
            .http
            .post(&url)
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to Google STT")?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| "Failed to parse Google STT response")?;

        if !status.is_success() {
            let error_msg = body["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(format!("Google STT API error ({}): {}", status, error_msg).into());
        }

        let text = body["results"][0]["alternatives"][0]["transcript"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }
}
