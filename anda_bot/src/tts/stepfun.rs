use anda_core::BoxError;
use reqwest::header::ACCEPT;
use serde::Serialize;

use super::{TTS_TIMEOUT, TtsProvider};
use crate::{config, util::http_client::check_http_response};

/// StepFun rejects TTS input longer than 1000 characters.
const STEPFUN_MAX_INPUT_LENGTH: usize = 1000;

const STEPFUN_TTS_25_MODEL: &str = "stepaudio-2.5-tts";

/// StepFun TTS provider (`POST /v1/audio/speech`).
pub struct StepFunTtsProvider {
    api_url: String,
    api_key: String,
    model: String,
    voice: String,
    response_format: &'static str,
    speed: f64,
    volume: f64,
    instruction: Option<String>,
    sample_rate: u32,
    pronunciation_map: Vec<String>,
    markdown_filter: Option<bool>,
    http: reqwest::Client,
}

impl StepFunTtsProvider {
    /// `response_format` is the already validated `tts.default_format`.
    pub fn new(
        config: &config::StepFunTtsConfig,
        response_format: &'static str,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err("Missing StepFun TTS API key: set [tts.stepfun].api_key".into());
        }

        let api_url = config.api_url.trim().to_string();
        if api_url.is_empty() {
            return Err("stepfun tts: `api_url` must not be empty".into());
        }
        let parsed = api_url
            .parse::<reqwest::Url>()
            .map_err(|e| format!("stepfun tts: invalid `api_url` {api_url:?}: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!(
                "stepfun tts: `api_url` must use http or https scheme, got {:?}",
                parsed.scheme()
            )
            .into());
        }

        let model = config.model.trim().to_string();
        if model.is_empty() {
            return Err("stepfun tts: `model` must not be empty".into());
        }

        let voice = config.voice.trim().to_string();
        if voice.is_empty() {
            return Err("stepfun tts: `voice` must not be empty".into());
        }

        if !(0.5..=2.0).contains(&config.speed) {
            return Err("stepfun tts: `speed` must be between 0.5 and 2.0".into());
        }
        if !(0.1..=2.0).contains(&config.volume) {
            return Err("stepfun tts: `volume` must be between 0.1 and 2.0".into());
        }
        if !matches!(config.sample_rate, 8000 | 16000 | 22050 | 24000 | 48000) {
            return Err(
                "stepfun tts: `sample_rate` must be one of 8000, 16000, 22050, 24000, 48000".into(),
            );
        }

        let instruction = config::normalize_optional(&config.instruction);
        let is_tts_25 = model == STEPFUN_TTS_25_MODEL;
        if !is_tts_25 && instruction.is_some() {
            return Err("stepfun tts: `instruction` is only supported by stepaudio-2.5-tts".into());
        }
        if let Some(ref instruction) = instruction {
            let char_count = instruction.chars().count();
            if char_count > 200 {
                return Err(format!(
                    "stepfun tts: `instruction` too long ({} chars, max 200)",
                    char_count
                )
                .into());
            }
        }

        Ok(Self {
            api_url,
            api_key: api_key.to_string(),
            model,
            voice,
            response_format,
            speed: config.speed,
            volume: config.volume,
            instruction,
            sample_rate: config.sample_rate,
            pronunciation_map: config::normalize_list(&config.pronunciation_map.tone),
            markdown_filter: config.markdown_filter,
            http,
        })
    }

    fn request<'a>(&'a self, text: &'a str) -> SpeechRequest<'a> {
        SpeechRequest {
            model: &self.model,
            input: text,
            voice: &self.voice,
            response_format: self.response_format,
            speed: self.speed,
            volume: self.volume,
            sample_rate: self.sample_rate,
            instruction: self.instruction.as_deref(),
            pronunciation_map: (!self.pronunciation_map.is_empty()).then_some(PronunciationMap {
                tone: &self.pronunciation_map,
            }),
            markdown_filter: self.markdown_filter,
        }
    }
}

#[derive(Serialize)]
struct SpeechRequest<'a> {
    model: &'a str,
    input: &'a str,
    voice: &'a str,
    response_format: &'a str,
    speed: f64,
    volume: f64,
    sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    instruction: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pronunciation_map: Option<PronunciationMap<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    markdown_filter: Option<bool>,
}

#[derive(Serialize)]
struct PronunciationMap<'a> {
    tone: &'a [String],
}

#[async_trait::async_trait]
impl TtsProvider for StepFunTtsProvider {
    fn audio_format(&self) -> &'static str {
        self.response_format
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let char_count = text.chars().count();
        if char_count > STEPFUN_MAX_INPUT_LENGTH {
            return Err(format!(
                "StepFun TTS text too long ({} chars, max {})",
                char_count, STEPFUN_MAX_INPUT_LENGTH
            )
            .into());
        }

        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .header(ACCEPT, "audio/*")
            .json(&self.request(text))
            .timeout(TTS_TIMEOUT)
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send StepFun TTS request: {:?}",
                    err.without_url()
                )
            })?;

        let resp = check_http_response(resp, "StepFun TTS").await?;

        let bytes = resp.bytes().await.map_err(|err| {
            format!(
                "Failed to read StepFun TTS response body: {:?}",
                err.without_url()
            )
        })?;
        Ok(Vec::from(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;
    use serde_json::json;

    fn test_stepfun_provider(
        config: config::StepFunTtsConfig,
        response_format: &'static str,
    ) -> StepFunTtsProvider {
        StepFunTtsProvider::new(&config, response_format, new_reqwest_client()).unwrap()
    }

    fn request_body(provider: &StepFunTtsProvider, text: &str) -> serde_json::Value {
        serde_json::to_value(provider.request(text)).unwrap()
    }

    #[test]
    fn stepfun_tts_request_body_includes_documented_fields() {
        let provider = test_stepfun_provider(
            config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                speed: 1.25,
                volume: 1.5,
                pronunciation_map: config::StepFunTtsPronunciationMap {
                    tone: vec!["阿胶/e1胶".to_string(), "扁舟/偏舟".to_string()],
                },
                markdown_filter: Some(true),
                ..Default::default()
            },
            "wav",
        );

        let body = request_body(&provider, "智能阶跃");

        assert_eq!(body["model"], "stepaudio-2.5-tts");
        assert_eq!(body["input"], "智能阶跃");
        assert_eq!(body["voice"], "ruyananshi");
        assert_eq!(body["response_format"], "wav");
        assert_eq!(body["speed"], json!(1.25));
        assert_eq!(body["volume"], json!(1.5));
        assert_eq!(body["sample_rate"], 24000);
        assert_eq!(body["pronunciation_map"]["tone"][0], "阿胶/e1胶");
        assert_eq!(body["markdown_filter"], true);
        assert!(body.get("instruction").is_none());

        let body = request_body(
            &test_stepfun_provider(
                config::StepFunTtsConfig {
                    api_key: "sk-test".to_string(),
                    ..Default::default()
                },
                "mp3",
            ),
            "hi",
        );
        for field in ["instruction", "pronunciation_map", "markdown_filter"] {
            assert!(body.get(field).is_none(), "{field} should be omitted");
        }
    }

    #[test]
    fn stepaudio_tts_25_accepts_instruction() {
        let provider = test_stepfun_provider(
            config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                model: STEPFUN_TTS_25_MODEL.to_string(),
                instruction: Some("语气极其愤怒，压迫感强，语速偏快".to_string()),
                ..Default::default()
            },
            "mp3",
        );

        let body = request_body(&provider, "你以为这是开玩笑的吗");

        assert_eq!(body["model"], STEPFUN_TTS_25_MODEL);
        assert_eq!(body["instruction"], "语气极其愤怒，压迫感强，语速偏快");
    }

    fn tts_config_error(mutate: impl FnOnce(&mut config::StepFunTtsConfig)) -> String {
        let mut config = config::StepFunTtsConfig {
            api_key: "sk-test".to_string(),
            ..Default::default()
        };
        mutate(&mut config);
        StepFunTtsProvider::new(&config, "mp3", new_reqwest_client())
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn new_validates_every_config_field() {
        assert!(tts_config_error(|c| c.api_key = " ".into()).contains("Missing StepFun TTS"));
        assert!(
            tts_config_error(|c| c.api_url = " ".into()).contains("`api_url` must not be empty")
        );
        assert!(tts_config_error(|c| c.api_url = "not a url".into()).contains("invalid `api_url`"));
        assert!(
            tts_config_error(|c| c.api_url = "ftp://x".into()).contains("must use http or https")
        );
        assert!(tts_config_error(|c| c.model = " ".into()).contains("`model` must not be empty"));
        assert!(tts_config_error(|c| c.voice = " ".into()).contains("`voice` must not be empty"));
        assert!(tts_config_error(|c| c.speed = 3.0).contains("`speed`"));
        assert!(tts_config_error(|c| c.volume = 0.0).contains("`volume`"));
        assert!(tts_config_error(|c| c.sample_rate = 44100).contains("`sample_rate`"));
        assert!(
            tts_config_error(|c| {
                c.model = "step-tts-mini".to_string();
                c.instruction = Some("生气".into());
            })
            .contains("only supported by stepaudio-2.5-tts")
        );
        assert!(
            tts_config_error(|c| {
                c.model = STEPFUN_TTS_25_MODEL.to_string();
                c.instruction = Some("长".repeat(201));
            })
            .contains("`instruction` too long")
        );
    }

    use axum::{Router, routing};

    async fn provider_with_mock(status: u16, body: &'static str) -> StepFunTtsProvider {
        let app = Router::new().route(
            "/tts",
            routing::post(
                move || async move { (http::StatusCode::from_u16(status).unwrap(), body) },
            ),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;

        StepFunTtsProvider::new(
            &config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                api_url: format!("{base_url}/tts"),
                ..Default::default()
            },
            "mp3",
            new_reqwest_client(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn synthesize_returns_audio_bytes_and_reports_errors() {
        let provider = provider_with_mock(200, "MP3DATA").await;
        assert_eq!(provider.synthesize("你好").await.unwrap(), b"MP3DATA");
        assert_eq!(provider.audio_format(), "mp3");

        // Oversized input is rejected before sending.
        let long_text = "好".repeat(STEPFUN_MAX_INPUT_LENGTH + 1);
        let err = provider.synthesize(&long_text).await.unwrap_err();
        assert!(err.to_string().contains("text too long"));

        let provider = provider_with_mock(429, r#"{"error":{"message":"rate limited"}}"#).await;
        let err = provider.synthesize("hi").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("StepFun TTS API error (429"), "got: {msg}");
        assert!(msg.contains("rate limited"), "got: {msg}");
    }
}
