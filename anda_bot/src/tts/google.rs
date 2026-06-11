use anda_core::BoxError;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

use super::TtsProvider;
use crate::config;

/// Google Cloud TTS provider (`POST /v1/text:synthesize`).
pub struct GoogleTtsProvider {
    api_key: String,
    language_code: String,
    voice: String,
    http: reqwest::Client,
}

impl GoogleTtsProvider {
    pub fn new(config: &config::GoogleTtsConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("Google TTS API key must not be empty".into());
        }

        Ok(Self {
            api_key: config.api_key.trim().to_string(),
            language_code: config.language_code.clone(),
            voice: config.voice.clone(),
            http,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for GoogleTtsProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let url = "https://texttospeech.googleapis.com/v1/text:synthesize";
        let body = json!({
            "input": { "text": text },
            "voice": {
                "languageCode": self.language_code,
                "name": self.voice,
            },
            "audioConfig": {
                "audioEncoding": "MP3",
            },
        });

        let resp = self
            .http
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Failed to send Google TTS request")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| "Failed to parse Google TTS response")?;

        if !status.is_success() {
            let msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(format!("Google TTS API error ({}): {}", status, msg).into());
        }

        let audio_b64 = resp_body["audioContent"]
            .as_str()
            .ok_or("Google TTS response missing 'audioContent' field")?;

        let bytes = STANDARD
            .decode(audio_b64)
            .map_err(|_| "Failed to decode Google TTS base64 audio")?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_empty_api_key() {
        let config = config::GoogleTtsConfig {
            api_key: " ".to_string(),
            ..Default::default()
        };

        let err = GoogleTtsProvider::new(&config, reqwest::Client::new())
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("API key must not be empty"));
    }

    #[test]
    fn new_trims_api_key_and_copies_config() {
        let config = config::GoogleTtsConfig {
            api_key: " key-1 ".to_string(),
            language_code: "zh-CN".to_string(),
            voice: "zh-CN-Standard-A".to_string(),
        };

        let provider = GoogleTtsProvider::new(&config, reqwest::Client::new()).unwrap();
        assert_eq!(provider.api_key, "key-1");
        assert_eq!(provider.language_code, "zh-CN");
        assert_eq!(provider.voice, "zh-CN-Standard-A");
        assert_eq!(provider.name(), "google");
    }
}
