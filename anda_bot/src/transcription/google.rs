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

    fn supported_audio_formats(&self) -> &'static [&'static str] {
        &["webm", "ogg", "opus", "mp3", "wav", "flac"]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;

    #[test]
    fn from_config_rejects_empty_api_key() {
        let config = config::GoogleSttConfig {
            api_key: "\t".to_string(),
            language_code: "en-US".to_string(),
        };

        let err = GoogleSttProvider::from_config(&config, new_reqwest_client())
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Missing Google STT API key"));
    }

    #[test]
    fn from_config_trims_api_key_and_copies_language() {
        let config = config::GoogleSttConfig {
            api_key: " key-1 ".to_string(),
            language_code: "zh-CN".to_string(),
        };

        let provider = GoogleSttProvider::from_config(&config, new_reqwest_client()).unwrap();
        assert_eq!(provider.api_key, "key-1");
        assert_eq!(provider.language_code, "zh-CN");
        assert_eq!(provider.name(), "google");
        assert_eq!(
            provider.supported_audio_formats(),
            &["webm", "ogg", "opus", "mp3", "wav", "flac"]
        );
    }

    #[tokio::test]
    async fn transcribe_rejects_extensions_google_does_not_support() {
        let config = config::GoogleSttConfig {
            api_key: "key-1".to_string(),
            language_code: "en-US".to_string(),
        };
        let provider = GoogleSttProvider::from_config(&config, new_reqwest_client()).unwrap();

        // `.m4a` passes the generic audio validation but is not accepted by
        // Google STT, so the error surfaces before any network request.
        let err = provider.transcribe(b"data", "voice.m4a").await.unwrap_err();
        assert!(err.to_string().contains("does not support '.m4a'"));
    }
}
