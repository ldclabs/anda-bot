use anda_core::BoxError;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;

use super::{TTS_HTTP_TIMEOUT, TtsProvider};
use crate::{config, util::http_client::check_http_response};

const GOOGLE_MAX_TEXT_BYTES: usize = 5000;

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
        if text.len() > GOOGLE_MAX_TEXT_BYTES {
            return Err(format!(
                "Google TTS text too long ({} bytes, max {GOOGLE_MAX_TEXT_BYTES})",
                text.len()
            )
            .into());
        }
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
            .timeout(TTS_HTTP_TIMEOUT)
            .send()
            .await
            .map_err(|err| format!("Failed to send Google TTS request: {:?}", err.without_url()))?;

        parse_response(resp).await
    }
}

#[derive(Deserialize)]
struct SynthesisResponse {
    #[serde(rename = "audioContent")]
    audio_content: String,
}

async fn parse_response(resp: reqwest::Response) -> Result<Vec<u8>, BoxError> {
    let body: SynthesisResponse = check_http_response(resp, "Google TTS")
        .await?
        .json()
        .await
        .map_err(|err| {
            format!(
                "Failed to parse Google TTS response: {:?}",
                err.without_url()
            )
        })?;
    let bytes = STANDARD
        .decode(body.audio_content)
        .map_err(|err| format!("Failed to decode Google TTS base64 audio: {err}"))?;
    if bytes.is_empty() {
        return Err("Google TTS response body contained empty audio".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;

    #[test]
    fn new_rejects_empty_api_key() {
        let config = config::GoogleTtsConfig {
            api_key: " ".to_string(),
            ..Default::default()
        };

        let err = GoogleTtsProvider::new(&config, new_reqwest_client())
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

        let provider = GoogleTtsProvider::new(&config, new_reqwest_client()).unwrap();
        assert_eq!(provider.api_key, "key-1");
        assert_eq!(provider.language_code, "zh-CN");
        assert_eq!(provider.voice, "zh-CN-Standard-A");
        assert_eq!(provider.name(), "google");
    }

    #[tokio::test]
    async fn rejects_text_by_utf8_bytes_before_network_io() {
        let provider = GoogleTtsProvider::new(
            &config::GoogleTtsConfig {
                api_key: "test".into(),
                ..Default::default()
            },
            new_reqwest_client(),
        )
        .unwrap();
        let text = "好".repeat(1667);
        assert!(text.chars().count() < 4096);
        let error = provider.synthesize(&text).await.unwrap_err().to_string();
        assert!(error.contains("5001 bytes, max 5000"));
    }

    #[tokio::test]
    async fn response_requires_nonempty_valid_audio() {
        use axum::{Router, routing};
        let app = Router::new()
            .route(
                "/audio",
                routing::get(|| async {
                    axum::Json(json!({"audioContent": STANDARD.encode(b"MP3")}))
                }),
            )
            .route(
                "/empty",
                routing::get(|| async { axum::Json(json!({"audioContent": ""})) }),
            )
            .route(
                "/invalid",
                routing::get(|| async { axum::Json(json!({"audioContent": "!"})) }),
            )
            .route("/missing", routing::get(|| async { axum::Json(json!({})) }))
            .route(
                "/proxy",
                routing::get(|| async { (http::StatusCode::BAD_GATEWAY, "upstream unavailable") }),
            );
        let base = crate::test_support::spawn_http_mock(app).await;
        let client = new_reqwest_client();
        let response = client.get(format!("{base}/audio")).send().await.unwrap();
        assert_eq!(parse_response(response).await.unwrap(), b"MP3");
        for (path, message) in [
            ("empty", "empty audio"),
            ("invalid", "decode"),
            ("missing", "audioContent"),
            ("proxy", "502"),
        ] {
            let response = client.get(format!("{base}/{path}")).send().await.unwrap();
            assert!(
                parse_response(response)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
    }
}
