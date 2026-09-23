use anda_core::BoxError;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;

use super::{TRANSCRIPTION_TIMEOUT_SECS, TranscriptionProvider, audio_extension, validate_audio};
use crate::{config, util::http_client::check_http_response};

const GOOGLE_MAX_REQUEST_BYTES: usize = 10_000_000;
const GOOGLE_MAX_DURATION_SECS: f64 = 60.0;

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
        &["wav", "flac"]
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let body = build_request_body(audio_data, file_name, &self.language_code)?;

        let resp = self
            .http
            .post("https://speech.googleapis.com/v1/speech:recognize")
            .header("x-goog-api-key", &self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|err| {
                format!(
                    "Failed to send transcription request to Google STT: {:?}",
                    err.without_url()
                )
            })?;

        parse_response(resp).await
    }
}

fn build_request_body(audio: &[u8], file_name: &str, language: &str) -> Result<Vec<u8>, BoxError> {
    validate_audio(audio, file_name)?;
    let extension = audio_extension(file_name).unwrap_or_default();
    if !matches!(extension.as_str(), "wav" | "flac") {
        return Err(
            format!("Google STT does not support '.{extension}' input; use WAV or FLAC").into(),
        );
    }
    // Reject oversized content before allocating its Base64 representation.
    if audio.len().div_ceil(3) * 4 > GOOGLE_MAX_REQUEST_BYTES {
        return Err("Google STT request exceeds the 10 MB limit (including Base64 audio)".into());
    }
    if let Some(duration) = audio_duration_secs(audio, &extension)
        && duration > GOOGLE_MAX_DURATION_SECS
    {
        return Err(
            "Google STT synchronous recognition accepts at most 60 seconds of audio".into(),
        );
    }

    // WAV/FLAC headers describe both encoding and sample rate. Overriding the
    // encoding with LINEAR16 would reject otherwise supported MULAW WAV files.
    let body = serde_json::to_vec(&json!({
        "config": { "languageCode": language, "enableAutomaticPunctuation": true },
        "audio": { "content": STANDARD.encode(audio) }
    }))?;
    if body.len() > GOOGLE_MAX_REQUEST_BYTES {
        return Err("Google STT request exceeds the 10 MB limit (including Base64 audio)".into());
    }
    Ok(body)
}

/// Read duration from ordinary WAV/FLAC headers without decoding audio.
/// Unknown or incomplete headers are left for the API to validate.
fn audio_duration_secs(audio: &[u8], extension: &str) -> Option<f64> {
    match extension {
        "wav" if audio.get(..4)? == b"RIFF" && audio.get(8..12)? == b"WAVE" => {
            let mut chunks = audio.get(12..)?;
            let mut byte_rate = None;
            let mut data_size = None;
            while chunks.len() >= 8 {
                let len = u32::from_le_bytes(chunks[4..8].try_into().ok()?) as usize;
                let data = chunks.get(8..8 + len)?;
                match &chunks[..4] {
                    b"fmt " => {
                        byte_rate = Some(u32::from_le_bytes(data.get(8..12)?.try_into().ok()?))
                    }
                    b"data" => data_size = Some(len),
                    _ => {}
                }
                if let (Some(rate), Some(size)) = (byte_rate, data_size) {
                    return (rate > 0).then(|| size as f64 / f64::from(rate));
                }
                chunks = chunks.get(8 + len + len % 2..)?;
            }
            None
        }
        "flac" if audio.get(..4)? == b"fLaC" => {
            // STREAMINFO is the first metadata block and has a 34-byte body.
            let header = audio.get(4..8)?;
            if header[0] & 0x7f != 0 || header[1..] != [0, 0, 34] {
                return None;
            }
            let info = audio.get(8..42)?;
            let packed = u64::from_be_bytes(info[10..18].try_into().ok()?);
            let rate = packed >> 44;
            let samples = packed & ((1 << 36) - 1);
            (rate > 0 && samples > 0).then(|| samples as f64 / rate as f64)
        }
        _ => None,
    }
}

#[derive(Deserialize)]
struct RecognitionResponse {
    #[serde(default)]
    results: Vec<RecognitionResult>,
}

#[derive(Deserialize)]
struct RecognitionResult {
    alternatives: Vec<RecognitionAlternative>,
}

#[derive(Deserialize)]
struct RecognitionAlternative {
    transcript: String,
}

async fn parse_response(resp: reqwest::Response) -> Result<String, BoxError> {
    let body: RecognitionResponse = check_http_response(resp, "Google STT")
        .await?
        .json()
        .await
        .map_err(|err| {
            format!(
                "Failed to parse Google STT response: {:?}",
                err.without_url()
            )
        })?;
    // Google supplies any leading space needed between consecutive segments.
    Ok(body
        .results
        .into_iter()
        .filter_map(|result| result.alternatives.into_iter().next())
        .map(|alternative| alternative.transcript)
        .collect())
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
        assert_eq!(provider.supported_audio_formats(), &["wav", "flac"]);
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

    fn wav(seconds: u32) -> Vec<u8> {
        let data_len = seconds * 32_000;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&32_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(bytes.len() + data_len as usize, 0);
        bytes
    }

    fn flac_header(seconds: u64) -> Vec<u8> {
        let mut bytes = vec![0; 42];
        bytes[..8].copy_from_slice(b"fLaC\x80\x00\x00\x22");
        let rate = 16_000u64;
        let info = (rate << 44) | (15 << 36) | (rate * seconds);
        bytes[18..26].copy_from_slice(&info.to_be_bytes());
        bytes
    }

    #[test]
    fn request_uses_header_encoding_and_rejects_unsupported_formats() {
        let audio = wav(1);
        let request: serde_json::Value =
            serde_json::from_slice(&build_request_body(&audio, "voice.WAV", "zh-CN").unwrap())
                .unwrap();
        assert_eq!(request["config"]["languageCode"], "zh-CN");
        assert!(request["config"].get("encoding").is_none());
        assert!(request["config"].get("sampleRateHertz").is_none());
        assert_eq!(
            STANDARD
                .decode(request["audio"]["content"].as_str().unwrap())
                .unwrap(),
            audio
        );
        for name in ["voice.webm", "voice.ogg", "voice.opus", "voice.mp3"] {
            assert!(
                build_request_body(b"audio", name, "en-US")
                    .unwrap_err()
                    .to_string()
                    .contains("use WAV or FLAC")
            );
        }
    }

    #[test]
    fn rejects_known_wav_and_flac_duration_above_one_minute() {
        for (audio, extension) in [(wav(61), "wav"), (flac_header(61), "flac")] {
            assert_eq!(audio_duration_secs(&audio, extension), Some(61.0));
            assert!(
                build_request_body(&audio, &format!("voice.{extension}"), "en-US")
                    .unwrap_err()
                    .to_string()
                    .contains("60 seconds")
            );
        }
        assert!(build_request_body(&wav(60), "voice.wav", "en-US").is_ok());
        assert!(build_request_body(&flac_header(60), "voice.flac", "en-US").is_ok());
        assert_eq!(audio_duration_secs(&flac_header(0), "flac"), None);
        assert_eq!(audio_duration_secs(b"RIFF", "wav"), None);
    }

    #[test]
    fn request_limit_includes_base64_and_json_overhead() {
        let overhead = build_request_body(b"a", "voice.flac", "en-US")
            .unwrap()
            .len()
            - 4;
        let max_audio = (GOOGLE_MAX_REQUEST_BYTES - overhead) / 4 * 3;
        let mut audio = vec![0; max_audio];
        assert!(
            build_request_body(&audio, "voice.flac", "en-US")
                .unwrap()
                .len()
                <= GOOGLE_MAX_REQUEST_BYTES
        );
        audio.extend_from_slice(&[0; 3]);
        assert!(
            build_request_body(&audio, "voice.flac", "en-US")
                .unwrap_err()
                .to_string()
                .contains("10 MB")
        );
        assert!(
            build_request_body(&vec![0; GOOGLE_MAX_REQUEST_BYTES], "voice.flac", "en-US").is_err()
        );
    }

    #[tokio::test]
    async fn response_preserves_all_segments_and_legitimate_silence() {
        use axum::{Router, routing};
        let app = Router::new()
            .route(
                "/segments",
                routing::get(|| async {
                    axum::Json(json!({"results": [
                        {"alternatives": [{"transcript": "Hello"}, {"transcript": "wrong"}]},
                        {"alternatives": [{"transcript": " world."}]},
                        {"alternatives": [{"transcript": "你好。"}]}
                    ]}))
                }),
            )
            .route("/silence", routing::get(|| async { axum::Json(json!({})) }))
            .route(
                "/bad",
                routing::get(|| async {
                    axum::Json(json!({"results": [{"alternatives": [{"wrong": "text"}]}]}))
                }),
            )
            .route(
                "/proxy",
                routing::get(|| async { (http::StatusCode::BAD_GATEWAY, "upstream unavailable") }),
            );
        let base = crate::test_support::spawn_http_mock(app).await;
        let client = new_reqwest_client();
        let response = client.get(format!("{base}/segments")).send().await.unwrap();
        assert_eq!(
            parse_response(response).await.unwrap(),
            "Hello world.你好。"
        );
        let response = client.get(format!("{base}/silence")).send().await.unwrap();
        assert_eq!(parse_response(response).await.unwrap(), "");
        let response = client.get(format!("{base}/bad")).send().await.unwrap();
        assert!(
            parse_response(response)
                .await
                .unwrap_err()
                .to_string()
                .contains("parse Google STT")
        );
        let response = client.get(format!("{base}/proxy")).send().await.unwrap();
        let error = parse_response(response).await.unwrap_err().to_string();
        assert!(error.contains("502") && error.contains("upstream unavailable"));
    }
}
