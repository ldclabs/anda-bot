use anda_core::BoxError;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Serialize;

use super::{
    Base64, MAX_AUDIO_BYTES, TRANSCRIPTION_TIMEOUT, TranscriptionProvider, audio_extension,
    check_audio_size, http_url, required,
};
use crate::{config, util::http_client::check_http_response};

/// StepFun Stepaudio ASR provider using HTTP+SSE.
pub struct StepFunProvider {
    api_url: String,
    api_key: String,
    transcription: TranscriptionSettings,
    pcm: PcmFormat,
    http: reqwest::Client,
}

/// `audio.input.transcription` of the request body.
#[derive(Serialize)]
struct TranscriptionSettings {
    language: String,
    hotwords: Vec<String>,
    model: String,
    enable_itn: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
}

/// Format details StepFun requires for raw `.pcm` audio.
#[derive(Serialize)]
struct PcmFormat {
    codec: String,
    rate: u32,
    bits: u32,
    channel: u32,
}

#[derive(Serialize)]
struct AsrRequest<'a> {
    audio: AsrAudio<'a>,
}

#[derive(Serialize)]
struct AsrAudio<'a> {
    data: Base64<'a>,
    input: AsrInput<'a>,
}

#[derive(Serialize)]
struct AsrInput<'a> {
    transcription: &'a TranscriptionSettings,
    format: AsrFormat<'a>,
}

#[derive(Serialize)]
struct AsrFormat<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    pcm: Option<&'a PcmFormat>,
}

impl StepFunProvider {
    pub fn new(config: &config::StepFunSttConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        let api_key = required(&config.api_key, "stepfun.api_key")?;
        for (value, field) in [
            (config.pcm_rate, "pcm_rate"),
            (config.pcm_bits, "pcm_bits"),
            (config.pcm_channel, "pcm_channel"),
        ] {
            if value == 0 {
                return Err(
                    format!("`transcription.stepfun.{field}` must be greater than zero").into(),
                );
            }
        }

        Ok(Self {
            api_url: http_url(&config.api_url, "stepfun.api_url")?,
            api_key,
            transcription: TranscriptionSettings {
                language: required(&config.language, "stepfun.language")?,
                hotwords: config::normalize_list(&config.hotwords),
                model: required(&config.model, "stepfun.model")?,
                enable_itn: config.enable_itn,
                prompt: config::normalize_optional(&config.prompt),
            },
            pcm: PcmFormat {
                codec: required(&config.pcm_codec, "stepfun.pcm_codec")?,
                rate: config.pcm_rate,
                bits: config.pcm_bits,
                channel: config.pcm_channel,
            },
            http,
        })
    }

    fn request_body(&self, audio: &[u8], format: &'static str) -> Result<Vec<u8>, BoxError> {
        Ok(serde_json::to_vec(&AsrRequest {
            audio: AsrAudio {
                data: Base64(audio),
                input: AsrInput {
                    transcription: &self.transcription,
                    format: AsrFormat {
                        kind: format,
                        pcm: (format == "pcm").then_some(&self.pcm),
                    },
                },
            },
        })?)
    }
}

#[async_trait]
impl TranscriptionProvider for StepFunProvider {
    fn supported_audio_formats(&self) -> &'static [&'static str] {
        &["ogg", "mp3", "wav", "pcm"]
    }

    async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError> {
        check_audio_size(&audio, MAX_AUDIO_BYTES)?;
        let body = self.request_body(&audio, stepfun_audio_format(file_name)?)?;
        drop(audio); // The body carries the audio from here on.

        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .header(ACCEPT, "text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .timeout(TRANSCRIPTION_TIMEOUT)
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send transcription request to StepFun: {:?}",
                    err.without_url()
                )
            })?;

        parse_stepfun_sse_response(resp).await
    }
}

fn stepfun_audio_format(file_name: &str) -> Result<&'static str, BoxError> {
    let extension = audio_extension(file_name).ok_or("StepFun ASR requires a file extension")?;
    match extension.as_str() {
        "ogg" | "oga" => Ok("ogg"),
        "mp3" | "mpeg" | "mpga" => Ok("mp3"),
        "wav" => Ok("wav"),
        "pcm" => Ok("pcm"),
        ext => Err(format!("StepFun ASR does not support '.{ext}' input").into()),
    }
}

async fn parse_stepfun_sse_response(resp: reqwest::Response) -> Result<String, BoxError> {
    let resp = check_http_response(resp, "StepFun ASR").await?;

    let mut stream = resp.bytes_stream();
    let mut line_buf = Vec::new();
    let mut event_data = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| {
            format!(
                "Failed to read StepFun ASR SSE stream: {:?}",
                err.without_url()
            )
        })?;
        for &byte in chunk.iter() {
            if byte == b'\n' {
                if line_buf.ends_with(b"\r") {
                    line_buf.pop();
                }
                let line = std::str::from_utf8(&line_buf)
                    .map_err(|_| "StepFun ASR SSE stream contained invalid UTF-8")?;
                if let Some(done_text) = consume_stepfun_sse_line(line, &mut event_data)? {
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
        if let Some(done_text) = consume_stepfun_sse_line(line, &mut event_data)? {
            return Ok(done_text);
        }
    }

    if !event_data.is_empty()
        && let Some(done_text) = parse_stepfun_sse_event(&event_data)?
    {
        return Ok(done_text);
    }

    Err("StepFun ASR stream ended without a transcript.text.done event".into())
}

fn consume_stepfun_sse_line(
    line: &str,
    event_data: &mut String,
) -> Result<Option<String>, BoxError> {
    if line.is_empty() {
        if event_data.is_empty() {
            return Ok(None);
        }

        let result = parse_stepfun_sse_event(event_data)?;
        event_data.clear();
        return Ok(result);
    }

    if let Some(data) = line.strip_prefix("data:") {
        let data = data.strip_prefix(' ').unwrap_or(data);
        if data == "[DONE]" {
            return Err("StepFun ASR stream ended without a transcript.text.done event".into());
        }
        if !event_data.is_empty() {
            event_data.push('\n');
        }
        event_data.push_str(data);
    }

    Ok(None)
}

fn parse_stepfun_sse_event(data: &str) -> Result<Option<String>, BoxError> {
    let body: serde_json::Value =
        serde_json::from_str(data).map_err(|_| "Failed to parse StepFun ASR SSE event")?;

    match body["type"].as_str() {
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
    use crate::util::http_client::new_reqwest_client;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;

    fn test_stepfun_provider() -> StepFunProvider {
        StepFunProvider::new(
            &config::StepFunSttConfig {
                api_key: "sk-test".to_string(),
                ..Default::default()
            },
            new_reqwest_client(),
        )
        .unwrap()
    }

    fn request_json(
        provider: &StepFunProvider,
        audio: &[u8],
        file_name: &str,
    ) -> serde_json::Value {
        let format = stepfun_audio_format(file_name).unwrap();
        serde_json::from_slice(&provider.request_body(audio, format).unwrap()).unwrap()
    }

    #[test]
    fn stepfun_audio_format_maps_supported_containers() {
        for (name, format) in [
            ("voice.oga", "ogg"),
            ("voice.mp3", "mp3"),
            ("voice.mpeg", "mp3"),
            ("voice.WAV", "wav"),
            ("voice.pcm", "pcm"),
        ] {
            assert_eq!(stepfun_audio_format(name).unwrap(), format, "{name}");
        }
        assert!(stepfun_audio_format("voice.webm").is_err());
        assert!(stepfun_audio_format("voice").is_err());
    }

    #[test]
    fn request_format_includes_pcm_details_only_for_pcm() {
        let provider = test_stepfun_provider();
        let format = &request_json(&provider, &[0, 1, 2], "voice.pcm")["audio"]["input"]["format"];
        assert_eq!(
            format,
            &json!({"type": "pcm", "codec": "pcm_s16le", "rate": 16000, "bits": 16, "channel": 1})
        );

        let format = &request_json(&provider, b"audio", "voice.wav")["audio"]["input"]["format"];
        assert_eq!(format, &json!({"type": "wav"}));
    }

    #[test]
    fn parse_stepfun_sse_event_returns_done_text() {
        let text = parse_stepfun_sse_event(
            r#"{"type":"transcript.text.done","text":"识别的完整文字内容"}"#,
        )
        .unwrap();

        assert_eq!(text.as_deref(), Some("识别的完整文字内容"));
    }

    #[test]
    fn parse_stepfun_sse_event_waits_for_final_text() {
        assert!(
            parse_stepfun_sse_event(r#"{"type":"transcript.text.delta","delta":"部分"}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn parse_stepfun_sse_event_reports_error_event() {
        let err = parse_stepfun_sse_event(r#"{"type":"error","message":"bad audio"}"#).unwrap_err();

        assert!(err.to_string().contains("bad audio"));
    }

    use axum::{Router, routing};

    fn config_error(mutate: impl FnOnce(&mut config::StepFunSttConfig)) -> String {
        let mut config = config::StepFunSttConfig {
            api_key: "sk-test".to_string(),
            ..Default::default()
        };
        mutate(&mut config);
        StepFunProvider::new(&config, new_reqwest_client())
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn new_validates_every_field() {
        let empty = |field: &str| format!("`transcription.stepfun.{field}` must not be empty");
        assert!(config_error(|c| c.api_key = " ".into()).contains(&empty("api_key")));
        assert!(config_error(|c| c.api_url = " ".into()).contains(&empty("api_url")));
        assert!(
            config_error(|c| c.api_url = "ftp://example.com".into())
                .contains("must use http or https")
        );
        assert!(config_error(|c| c.model = " ".into()).contains(&empty("model")));
        assert!(config_error(|c| c.language = " ".into()).contains(&empty("language")));
        assert!(config_error(|c| c.pcm_codec = " ".into()).contains(&empty("pcm_codec")));
        let zero = "` must be greater than zero";
        assert!(config_error(|c| c.pcm_rate = 0).contains(&format!("pcm_rate{zero}")));
        assert!(config_error(|c| c.pcm_bits = 0).contains(&format!("pcm_bits{zero}")));
        assert!(config_error(|c| c.pcm_channel = 0).contains(&format!("pcm_channel{zero}")));
    }

    #[test]
    fn request_body_carries_normalized_settings_and_audio() {
        let provider = StepFunProvider::new(
            &config::StepFunSttConfig {
                api_key: "sk-test".to_string(),
                hotwords: vec![" 阶跃 ".to_string(), "  ".to_string()],
                prompt: Some("  领域词提示  ".to_string()),
                ..Default::default()
            },
            new_reqwest_client(),
        )
        .unwrap();
        assert_eq!(
            provider.supported_audio_formats(),
            &["ogg", "mp3", "wav", "pcm"]
        );

        let body = request_json(&provider, b"audio-bytes", "voice.mp3");
        assert_eq!(body["audio"]["data"], STANDARD.encode(b"audio-bytes"));
        assert_eq!(body["audio"]["input"]["format"]["type"], "mp3");
        assert_eq!(
            body["audio"]["input"]["transcription"],
            json!({
                "language": "zh",
                "hotwords": ["阶跃"],
                "model": "stepaudio-2.5-asr",
                "enable_itn": true,
                "prompt": "领域词提示"
            })
        );

        let body = request_json(&test_stepfun_provider(), b"audio", "voice.mp3");
        assert!(
            body["audio"]["input"]["transcription"]
                .get("prompt")
                .is_none()
        );
    }

    #[test]
    fn consume_sse_line_handles_done_sentinel_and_comments() {
        let mut event_data = String::new();

        // Blank line with no pending data is a no-op.
        assert!(
            consume_stepfun_sse_line("", &mut event_data)
                .unwrap()
                .is_none()
        );
        // Non-data lines (event names, comments) are ignored.
        assert!(
            consume_stepfun_sse_line("event: transcript", &mut event_data)
                .unwrap()
                .is_none()
        );
        // A generic sentinel cannot substitute for the authoritative done text.
        assert!(
            consume_stepfun_sse_line("data: [DONE]", &mut event_data)
                .unwrap_err()
                .to_string()
                .contains("without a transcript.text.done")
        );

        // Multi-line data accumulates with newlines until the blank separator.
        let mut event_data = String::new();
        consume_stepfun_sse_line(r#"data: {"type":"transcript.text.delta","#, &mut event_data)
            .unwrap();
        consume_stepfun_sse_line(r#"data: "delta":"x"}"#, &mut event_data).unwrap();
        assert!(event_data.contains('\n'));
    }

    async fn spawn_sse_mock(body: &'static str, status: http::StatusCode) -> String {
        let app = Router::new().route(
            "/asr",
            routing::post(move || async move {
                (
                    status,
                    [(http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
            }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/asr")
    }

    async fn provider_for(api_url: String) -> StepFunProvider {
        StepFunProvider::new(
            &config::StepFunSttConfig {
                api_key: "sk-test".to_string(),
                api_url,
                ..Default::default()
            },
            new_reqwest_client(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn transcribe_collects_sse_done_event() {
        let body = "data: {\"type\":\"transcript.text.delta\",\"delta\":\"你好\"}\n\ndata: {\"type\":\"transcript.text.done\",\"text\":\"你好，世界\"}\n\n";
        let url = spawn_sse_mock(body, http::StatusCode::OK).await;
        let provider = provider_for(url).await;

        let text = provider
            .transcribe(b"data".to_vec(), "voice.mp3")
            .await
            .unwrap();
        assert_eq!(text, "你好，世界");
    }

    #[tokio::test]
    async fn transcribe_rejects_incomplete_delta_only_stream() {
        let body = "data: {\"type\":\"transcript.text.delta\",\"delta\":\"你好\"}\n\n";
        let url = spawn_sse_mock(body, http::StatusCode::OK).await;
        let provider = provider_for(url).await;

        let err = provider
            .transcribe(b"data".to_vec(), "voice.wav")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("without a transcript.text.done"));
    }

    #[tokio::test]
    async fn transcribe_handles_done_event_without_trailing_blank_line() {
        // The final event arrives without the trailing separator; the leftover
        // buffer is parsed after the stream ends.
        let body = "data: {\"type\":\"transcript.text.done\",\"text\":\"完整\"}";
        let url = spawn_sse_mock(body, http::StatusCode::OK).await;
        let provider = provider_for(url).await;

        let text = provider
            .transcribe(b"data".to_vec(), "voice.ogg")
            .await
            .unwrap();
        assert_eq!(text, "完整");
    }

    #[tokio::test]
    async fn transcribe_reports_stream_and_status_errors() {
        let url = spawn_sse_mock("", http::StatusCode::OK).await;
        let provider = provider_for(url).await;
        let err = provider
            .transcribe(b"data".to_vec(), "voice.mp3")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("ended without"));

        let url = spawn_sse_mock("quota exceeded", http::StatusCode::TOO_MANY_REQUESTS).await;
        let provider = provider_for(url).await;
        let err = provider
            .transcribe(b"data".to_vec(), "voice.mp3")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("StepFun ASR API error (429"), "got: {msg}");

        let url = spawn_sse_mock("data: not json\n\n", http::StatusCode::OK).await;
        let provider = provider_for(url).await;
        let err = provider
            .transcribe(b"data".to_vec(), "voice.mp3")
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to parse StepFun ASR SSE event")
        );

        // Unsupported container is rejected before any request is sent.
        let provider = test_stepfun_provider();
        let err = provider
            .transcribe(b"data".to_vec(), "voice.webm")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not support '.webm'"));
    }

    #[tokio::test]
    async fn transcribe_handles_utf8_and_crlf_split_across_chunks() {
        let body = "data: {\"type\":\"transcript.text.delta\",\"delta\":\"旧\"}\r\n\r\ndata: {\"type\":\"transcript.text.done\",\"text\":\"完整\"}\r\n\r\n";
        let app = Router::new().route(
            "/asr",
            routing::post(move || async move {
                let chunks = body.bytes().map(|byte| Ok::<_, std::io::Error>(vec![byte]));
                axum::body::Body::from_stream(futures::stream::iter(chunks))
            }),
        );
        let base = crate::test_support::spawn_http_mock(app).await;
        let provider = provider_for(format!("{base}/asr")).await;
        assert_eq!(
            provider
                .transcribe(b"audio".to_vec(), "voice.wav")
                .await
                .unwrap(),
            "完整"
        );
    }
}
