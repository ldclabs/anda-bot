use anda_core::BoxError;
use serde_json::json;

use super::TtsProvider;
use crate::config;

/// OpenAI TTS provider (`POST /v1/audio/speech`).
pub struct OpenAiTtsProvider {
    api_key: String,
    model: String,
    speed: f64,
    voice: String,
    http: reqwest::Client,
}

impl OpenAiTtsProvider {
    pub fn new(config: &config::OpenAiTtsConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("OpenAI TTS API key must not be empty".into());
        }

        Ok(Self {
            api_key: config.api_key.trim().to_string(),
            model: config.model.clone(),
            speed: config.speed,
            voice: config.voice.clone(),
            http,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for OpenAiTtsProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let body = json!({
            "model": self.model,
            "input": text,
            "voice": self.voice,
            "speed": self.speed,
            "response_format": "mp3",
        });

        let resp = self
            .http
            .post("https://api.openai.com/v1/audio/speech")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Failed to send OpenAI TTS request")?;

        let status = resp.status();
        if !status.is_success() {
            let error_body: serde_json::Value = resp
                .json()
                .await
                .unwrap_or_else(|_| json!({"error": "unknown"}));
            let msg = error_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(format!("OpenAI TTS API error ({}): {}", status, msg).into());
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|_| "Failed to read OpenAI TTS response body")?;
        Ok(bytes.to_vec())
    }
}
