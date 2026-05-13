use anda_core::BoxError;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::header::ACCEPT;
use serde_json::json;

use super::{MAX_AUDIO_BYTES, TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, audio_extension};
use crate::config;

/// StepFun Stepaudio ASR provider using HTTP+SSE.
pub struct StepFunProvider {
    api_url: String,
    api_key: String,
    model: String,
    language: String,
    hotwords: Vec<String>,
    prompt: Option<String>,
    enable_itn: bool,
    pcm_codec: String,
    pcm_rate: u32,
    pcm_bits: u32,
    pcm_channel: u32,
    http: reqwest::Client,
}

impl StepFunProvider {
    pub fn from_config(
        config: &config::StepFunSttConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err("Missing StepFun STT API key: set [transcription.stepfun].api_key".into());
        }

        let api_url = config.api_url.trim().to_string();
        if api_url.is_empty() {
            return Err("stepfun: `api_url` must not be empty".into());
        }
        let parsed = api_url
            .parse::<reqwest::Url>()
            .map_err(|e| format!("stepfun: invalid `api_url` {api_url:?}: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!(
                "stepfun: `api_url` must use http or https scheme, got {:?}",
                parsed.scheme()
            )
            .into());
        }

        let model = config.model.trim().to_string();
        if model.is_empty() {
            return Err("stepfun: `model` must not be empty".into());
        }

        let language = config.language.trim().to_string();
        if language.is_empty() {
            return Err("stepfun: `language` must not be empty".into());
        }

        if config.pcm_rate == 0 {
            return Err("stepfun: `pcm_rate` must be greater than zero".into());
        }
        if config.pcm_bits == 0 {
            return Err("stepfun: `pcm_bits` must be greater than zero".into());
        }
        if config.pcm_channel == 0 {
            return Err("stepfun: `pcm_channel` must be greater than zero".into());
        }

        let pcm_codec = config.pcm_codec.trim().to_string();
        if pcm_codec.is_empty() {
            return Err("stepfun: `pcm_codec` must not be empty".into());
        }

        let hotwords = config
            .hotwords
            .iter()
            .map(|hotword| hotword.trim())
            .filter(|hotword| !hotword.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        let prompt = config.prompt.as_deref().and_then(config::normalize_string);

        Ok(Self {
            api_url,
            api_key: api_key.to_string(),
            model,
            language,
            hotwords,
            prompt,
            enable_itn: config.enable_itn,
            pcm_codec,
            pcm_rate: config.pcm_rate,
            pcm_bits: config.pcm_bits,
            pcm_channel: config.pcm_channel,
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for StepFunProvider {
    fn name(&self) -> &str {
        "stepfun"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        validate_audio(audio_data, file_name)?;

        let request_body = build_stepfun_request_body(audio_data, file_name, self)?;
        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .header(ACCEPT, "text/event-stream")
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to StepFun")?;

        parse_stepfun_sse_response(resp).await
    }
}

pub(super) fn validate_audio(audio_data: &[u8], file_name: &str) -> Result<(), BoxError> {
    if audio_data.len() > MAX_AUDIO_BYTES {
        return Err(format!(
            "Audio file too large ({} bytes, max {MAX_AUDIO_BYTES})",
            audio_data.len()
        )
        .into());
    }

    stepfun_audio_format_type(file_name).map(|_| ())
}

fn stepfun_audio_format_type(file_name: &str) -> Result<&'static str, BoxError> {
    let extension = audio_extension(file_name).ok_or("StepFun ASR requires a file extension")?;
    match extension.as_str() {
        "ogg" | "oga" => Ok("ogg"),
        "mp3" | "mpeg" | "mpga" => Ok("mp3"),
        "wav" => Ok("wav"),
        "pcm" => Ok("pcm"),
        ext => Err(format!("StepFun ASR does not support '.{ext}' input").into()),
    }
}

fn build_stepfun_request_body(
    audio_data: &[u8],
    file_name: &str,
    provider: &StepFunProvider,
) -> Result<serde_json::Value, BoxError> {
    let mut transcription = serde_json::Map::new();
    transcription.insert("language".to_string(), json!(&provider.language));
    transcription.insert("hotwords".to_string(), json!(&provider.hotwords));
    transcription.insert("model".to_string(), json!(&provider.model));
    transcription.insert("enable_itn".to_string(), json!(provider.enable_itn));
    if let Some(ref prompt) = provider.prompt {
        transcription.insert("prompt".to_string(), json!(prompt));
    }

    Ok(json!({
        "audio": {
            "data": STANDARD.encode(audio_data),
            "input": {
                "transcription": transcription,
                "format": stepfun_audio_format(file_name, provider)?,
            },
        }
    }))
}

fn stepfun_audio_format(
    file_name: &str,
    provider: &StepFunProvider,
) -> Result<serde_json::Value, BoxError> {
    let format_type = stepfun_audio_format_type(file_name)?;

    let mut format = serde_json::Map::new();
    format.insert("type".to_string(), json!(format_type));

    if format_type == "pcm" {
        format.insert("codec".to_string(), json!(&provider.pcm_codec));
        format.insert("rate".to_string(), json!(provider.pcm_rate));
        format.insert("bits".to_string(), json!(provider.pcm_bits));
        format.insert("channel".to_string(), json!(provider.pcm_channel));
    }

    Ok(serde_json::Value::Object(format))
}

async fn parse_stepfun_sse_response(resp: reqwest::Response) -> Result<String, BoxError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("StepFun ASR API error ({}): {}", status, body.trim()).into());
    }

    let mut stream = resp.bytes_stream();
    let mut line_buf = Vec::new();
    let mut event_data = String::new();
    let mut delta_text = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Failed to read StepFun ASR SSE stream")?;
        for &byte in chunk.iter() {
            if byte == b'\n' {
                if line_buf.ends_with(b"\r") {
                    line_buf.pop();
                }
                let line = std::str::from_utf8(&line_buf)
                    .map_err(|_| "StepFun ASR SSE stream contained invalid UTF-8")?;
                if let Some(done_text) =
                    consume_stepfun_sse_line(line, &mut event_data, &mut delta_text)?
                {
                    return Ok(done_text);
                }
                line_buf.clear();
            } else {
                line_buf.push(byte);
            }
        }
    }

    if !line_buf.is_empty() {
        let line = std::str::from_utf8(&line_buf)
            .map_err(|_| "StepFun ASR SSE stream contained invalid UTF-8")?;
        if let Some(done_text) = consume_stepfun_sse_line(line, &mut event_data, &mut delta_text)? {
            return Ok(done_text);
        }
    }

    if !event_data.is_empty()
        && let Some(done_text) = parse_stepfun_sse_event(&event_data, &mut delta_text)?
    {
        return Ok(done_text);
    }

    if delta_text.is_empty() {
        Err("StepFun ASR stream ended without a transcript.text.done event".into())
    } else {
        Ok(delta_text)
    }
}

fn consume_stepfun_sse_line(
    line: &str,
    event_data: &mut String,
    delta_text: &mut String,
) -> Result<Option<String>, BoxError> {
    if line.is_empty() {
        if event_data.is_empty() {
            return Ok(None);
        }

        let result = parse_stepfun_sse_event(event_data, delta_text)?;
        event_data.clear();
        return Ok(result);
    }

    if let Some(data) = line.strip_prefix("data:") {
        let data = data.strip_prefix(' ').unwrap_or(data);
        if data == "[DONE]" {
            return Ok((!delta_text.is_empty()).then(|| delta_text.clone()));
        }
        if !event_data.is_empty() {
            event_data.push('\n');
        }
        event_data.push_str(data);
    }

    Ok(None)
}

fn parse_stepfun_sse_event(
    data: &str,
    delta_text: &mut String,
) -> Result<Option<String>, BoxError> {
    let body: serde_json::Value =
        serde_json::from_str(data).map_err(|_| "Failed to parse StepFun ASR SSE event")?;

    match body["type"].as_str() {
        Some("transcript.text.delta") => {
            if let Some(delta) = body["delta"].as_str() {
                delta_text.push_str(delta);
            }
            Ok(None)
        }
        Some("transcript.text.done") => {
            let text = body["text"]
                .as_str()
                .ok_or("StepFun ASR done event missing 'text' field")?;
            Ok(Some(text.to_string()))
        }
        Some("error") => {
            let message = body["message"].as_str().unwrap_or("unknown error");
            Err(format!("StepFun ASR API error: {message}").into())
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_stepfun_provider() -> StepFunProvider {
        StepFunProvider::from_config(
            &config::StepFunSttConfig {
                api_key: "sk-test".to_string(),
                ..Default::default()
            },
            reqwest::Client::new(),
        )
        .unwrap()
    }

    #[test]
    fn stepfun_audio_format_maps_supported_containers() {
        let provider = test_stepfun_provider();

        assert_eq!(
            stepfun_audio_format("voice.oga", &provider).unwrap()["type"],
            "ogg"
        );
        assert_eq!(
            stepfun_audio_format("voice.mp3", &provider).unwrap()["type"],
            "mp3"
        );
        assert_eq!(
            stepfun_audio_format("voice.mpeg", &provider).unwrap()["type"],
            "mp3"
        );
        assert!(stepfun_audio_format("voice.webm", &provider).is_err());
    }

    #[test]
    fn stepfun_audio_format_includes_pcm_details() {
        let provider = test_stepfun_provider();
        let format = stepfun_audio_format("voice.pcm", &provider).unwrap();

        assert_eq!(format["type"], "pcm");
        assert_eq!(format["codec"], "pcm_s16le");
        assert_eq!(format["rate"], 16000);
        assert_eq!(format["bits"], 16);
        assert_eq!(format["channel"], 1);
        assert!(validate_audio(&[0, 1, 2], "voice.pcm").is_ok());
    }

    #[test]
    fn parse_stepfun_sse_event_returns_done_text() {
        let mut delta_text = String::new();
        let text = parse_stepfun_sse_event(
            r#"{"type":"transcript.text.done","text":"识别的完整文字内容"}"#,
            &mut delta_text,
        )
        .unwrap();

        assert_eq!(text.as_deref(), Some("识别的完整文字内容"));
    }

    #[test]
    fn parse_stepfun_sse_event_accumulates_delta_text() {
        let mut delta_text = String::new();

        assert!(
            parse_stepfun_sse_event(
                r#"{"type":"transcript.text.delta","delta":"识别的"}"#,
                &mut delta_text,
            )
            .unwrap()
            .is_none()
        );
        assert!(
            parse_stepfun_sse_event(
                r#"{"type":"transcript.text.delta","delta":"文字"}"#,
                &mut delta_text,
            )
            .unwrap()
            .is_none()
        );

        assert_eq!(delta_text, "识别的文字");
    }

    #[test]
    fn parse_stepfun_sse_event_reports_error_event() {
        let mut delta_text = String::new();
        let err =
            parse_stepfun_sse_event(r#"{"type":"error","message":"bad audio"}"#, &mut delta_text)
                .unwrap_err();

        assert!(err.to_string().contains("bad audio"));
    }
}
