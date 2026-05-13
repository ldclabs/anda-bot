use anda_core::BoxError;
use reqwest::header::ACCEPT;
use serde_json::json;

use super::{TTS_HTTP_TIMEOUT, TtsProvider, normalize_audio_format};
use crate::config;

/// StepFun rejects TTS input longer than 1000 characters.
const STEPFUN_MAX_INPUT_LENGTH: usize = 1000;

const STEPFUN_TTS_25_MODEL: &str = "stepaudio-2.5-tts";

/// StepFun TTS provider (`POST /v1/audio/speech`).
pub struct StepFunTtsProvider {
    api_url: String,
    api_key: String,
    model: String,
    voice: String,
    response_format: String,
    speed: f64,
    volume: f64,
    voice_label: Option<serde_json::Value>,
    instruction: Option<String>,
    sample_rate: u32,
    pronunciation_map: Vec<String>,
    markdown_filter: Option<bool>,
    http: reqwest::Client,
}

impl StepFunTtsProvider {
    pub fn new(
        config: &config::StepFunTtsConfig,
        default_format: &str,
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

        let voice_label = normalize_stepfun_voice_label(config.voice_label.as_ref())?;
        let instruction = config::normalize_optional(&config.instruction);
        let is_tts_25 = model == STEPFUN_TTS_25_MODEL;
        if is_tts_25 && voice_label.is_some() {
            return Err("stepfun tts: `voice_label` is not supported by stepaudio-2.5-tts; use `instruction` instead".into());
        }
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
            response_format: normalize_stepfun_response_format(default_format)?.to_string(),
            speed: config.speed,
            volume: config.volume,
            voice_label,
            instruction,
            sample_rate: config.sample_rate,
            pronunciation_map: config::normalize_list(&config.pronunciation_map.tone),
            markdown_filter: config.markdown_filter,
            http,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for StepFunTtsProvider {
    fn name(&self) -> &str {
        "stepfun"
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

        let body = build_stepfun_tts_request_body(text, self);
        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .header(ACCEPT, "audio/*")
            .json(&body)
            .timeout(TTS_HTTP_TIMEOUT)
            .send()
            .await
            .map_err(|_| "Failed to send StepFun TTS request")?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            let msg = parse_stepfun_tts_error_message(&error_body);
            return Err(format!("StepFun TTS API error ({}): {}", status, msg).into());
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|_| "Failed to read StepFun TTS response body")?;
        if bytes.is_empty() {
            return Err("StepFun TTS response body was empty".into());
        }

        Ok(bytes.to_vec())
    }
}

fn normalize_stepfun_response_format(format: &str) -> Result<&'static str, BoxError> {
    let format = normalize_audio_format(format);
    match format {
        "mp3" | "wav" | "flac" | "opus" | "pcm" => Ok(format),
        "ogg" => {
            Err("StepFun TTS does not support `default_format: ogg`; use `opus` instead".into())
        }
        _ => unreachable!("normalize_audio_format only returns known formats"),
    }
}

fn normalize_stepfun_voice_label(
    voice_label: Option<&config::StepFunTtsVoiceLabel>,
) -> Result<Option<serde_json::Value>, BoxError> {
    let Some(voice_label) = voice_label else {
        return Ok(None);
    };

    let mut values = Vec::new();
    if let Some(language) = config::normalize_optional(&voice_label.language) {
        values.push(("language", language));
    }
    if let Some(emotion) = config::normalize_optional(&voice_label.emotion) {
        values.push(("emotion", emotion));
    }
    if let Some(style) = config::normalize_optional(&voice_label.style) {
        values.push(("style", style));
    }

    if values.is_empty() {
        return Ok(None);
    }
    if values.len() > 1 {
        return Err("stepfun tts: only one of `voice_label.language`, `voice_label.emotion`, or `voice_label.style` may be set".into());
    }

    let mut label = serde_json::Map::new();
    let (key, value) = values.remove(0);
    label.insert(key.to_string(), json!(value));
    Ok(Some(serde_json::Value::Object(label)))
}

fn build_stepfun_tts_request_body(text: &str, provider: &StepFunTtsProvider) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("model".to_string(), json!(&provider.model));
    body.insert("input".to_string(), json!(text));
    body.insert("voice".to_string(), json!(&provider.voice));
    body.insert(
        "response_format".to_string(),
        json!(&provider.response_format),
    );
    body.insert("speed".to_string(), json!(provider.speed));
    body.insert("volume".to_string(), json!(provider.volume));
    body.insert("sample_rate".to_string(), json!(provider.sample_rate));

    if let Some(ref voice_label) = provider.voice_label {
        body.insert("voice_label".to_string(), voice_label.clone());
    }
    if let Some(ref instruction) = provider.instruction {
        body.insert("instruction".to_string(), json!(instruction));
    }
    if !provider.pronunciation_map.is_empty() {
        body.insert(
            "pronunciation_map".to_string(),
            json!({ "tone": &provider.pronunciation_map }),
        );
    }
    if let Some(markdown_filter) = provider.markdown_filter {
        body.insert("markdown_filter".to_string(), json!(markdown_filter));
    }

    serde_json::Value::Object(body)
}

fn parse_stepfun_tts_error_message(raw_body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw_body) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.get("message").and_then(serde_json::Value::as_str))
            .or_else(|| value.get("error").and_then(serde_json::Value::as_str))
        {
            return message.to_string();
        }
    }

    let raw_body = raw_body.trim();
    if raw_body.is_empty() {
        "unknown error".to_string()
    } else {
        raw_body.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::mime_for_audio_format;
    use super::*;

    fn test_stepfun_provider(
        config: config::StepFunTtsConfig,
        default_format: &str,
    ) -> StepFunTtsProvider {
        StepFunTtsProvider::new(&config, default_format, reqwest::Client::new()).unwrap()
    }

    #[test]
    fn stepfun_tts_request_body_includes_documented_fields() {
        let provider = test_stepfun_provider(
            config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                speed: 1.25,
                volume: 1.5,
                voice_label: Some(config::StepFunTtsVoiceLabel {
                    style: Some("慢速".to_string()),
                    ..Default::default()
                }),
                pronunciation_map: config::StepFunTtsPronunciationMap {
                    tone: vec!["阿胶/e1胶".to_string(), "扁舟/偏舟".to_string()],
                },
                markdown_filter: Some(true),
                ..Default::default()
            },
            "wav",
        );

        let body = build_stepfun_tts_request_body("智能阶跃", &provider);

        assert_eq!(body["model"], "step-tts-mini");
        assert_eq!(body["input"], "智能阶跃");
        assert_eq!(body["voice"], "cixingnansheng");
        assert_eq!(body["response_format"], "wav");
        assert_eq!(body["speed"], json!(1.25));
        assert_eq!(body["volume"], json!(1.5));
        assert_eq!(body["sample_rate"], 24000);
        assert_eq!(body["voice_label"]["style"], "慢速");
        assert_eq!(body["pronunciation_map"]["tone"][0], "阿胶/e1胶");
        assert_eq!(body["markdown_filter"], true);
    }

    #[test]
    fn stepaudio_tts_25_accepts_instruction_without_voice_label() {
        let provider = test_stepfun_provider(
            config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                model: STEPFUN_TTS_25_MODEL.to_string(),
                instruction: Some("语气极其愤怒，压迫感强，语速偏快".to_string()),
                ..Default::default()
            },
            "mp3",
        );

        let body = build_stepfun_tts_request_body("你以为这是开玩笑的吗", &provider);

        assert_eq!(body["model"], STEPFUN_TTS_25_MODEL);
        assert_eq!(body["instruction"], "语气极其愤怒，压迫感强，语速偏快");
        assert!(body.get("voice_label").is_none());
    }

    #[test]
    fn stepfun_tts_rejects_invalid_voice_label_combination() {
        let result = StepFunTtsProvider::new(
            &config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                voice_label: Some(config::StepFunTtsVoiceLabel {
                    language: Some("粤语".to_string()),
                    emotion: Some("高兴".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            "mp3",
            reqwest::Client::new(),
        );

        let err = result.err().unwrap();
        assert!(err.to_string().contains("only one of `voice_label"));
    }

    #[test]
    fn stepfun_tts_25_rejects_voice_label() {
        let result = StepFunTtsProvider::new(
            &config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                model: STEPFUN_TTS_25_MODEL.to_string(),
                voice_label: Some(config::StepFunTtsVoiceLabel {
                    emotion: Some("高兴".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            "mp3",
            reqwest::Client::new(),
        );

        let err = result.err().unwrap();
        assert!(err.to_string().contains("voice_label"));
    }

    #[test]
    fn stepfun_tts_format_validation_matches_documented_formats() {
        assert_eq!(normalize_audio_format("flac"), "flac");
        assert_eq!(normalize_audio_format("pcm"), "pcm");
        assert_eq!(mime_for_audio_format("flac"), "audio/flac");
        assert_eq!(mime_for_audio_format("pcm"), "audio/pcm");
        assert_eq!(normalize_stepfun_response_format("opus").unwrap(), "opus");
        assert!(normalize_stepfun_response_format("ogg").is_err());
    }
}
