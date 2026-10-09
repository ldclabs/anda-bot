use anda_core::BoxError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    Base64, MAX_AUDIO_BYTES, TRANSCRIPTION_TIMEOUT, TranscriptionProvider, audio_extension,
    check_audio_size, required,
};
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
    pub fn new(config: &config::GoogleSttConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        Ok(Self {
            api_key: required(&config.api_key, "google.api_key")?,
            language_code: required(&config.language_code, "google.language_code")?,
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for GoogleSttProvider {
    fn supported_audio_formats(&self) -> &'static [&'static str] {
        &["wav", "flac"]
    }

    async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError> {
        let body = build_request_body(&audio, file_name, &self.language_code)?;
        drop(audio); // The body carries the audio from here on.

        let resp = self
            .http
            .post("https://speech.googleapis.com/v1/speech:recognize")
            .header("x-goog-api-key", &self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .timeout(TRANSCRIPTION_TIMEOUT)
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

/// `speech:recognize` request body. WAV/FLAC headers describe both encoding
/// and sample rate, so neither is set: overriding the encoding with LINEAR16
/// would reject otherwise supported MULAW WAV files.
#[derive(Serialize)]
struct RecognizeRequest<'a> {
    config: RecognitionConfig<'a>,
    audio: RecognitionAudio<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecognitionConfig<'a> {
    language_code: &'a str,
    enable_automatic_punctuation: bool,
}

#[derive(Serialize)]
struct RecognitionAudio<'a> {
    content: Base64<'a>,
}

fn build_request_body(audio: &[u8], file_name: &str, language: &str) -> Result<Vec<u8>, BoxError> {
    check_audio_size(audio, MAX_AUDIO_BYTES)?;
    let extension = audio_extension(file_name).unwrap_or_default();
    if !matches!(extension.as_str(), "wav" | "flac") {
        return Err(
            format!("Google STT does not support '.{extension}' input; use WAV or FLAC").into(),
        );
    }
    // Reject oversized content before encoding its Base64 representation.
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

    let body = serde_json::to_vec(&RecognizeRequest {
        config: RecognitionConfig {
            language_code: language,
            enable_automatic_punctuation: true,
        },
        audio: RecognitionAudio {
            content: Base64(audio),
        },
    })?;
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
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;

    fn google_config(api_key: &str, language_code: &str) -> config::GoogleSttConfig {
        config::GoogleSttConfig {
            api_key: api_key.to_string(),
            language_code: language_code.to_string(),
        }
    }

    #[test]
    fn new_requires_api_key_and_language() {
        for (config, field) in [
            (google_config("\t", "en-US"), "google.api_key"),
            (google_config("key-1", " "), "google.language_code"),
        ] {
            let err = GoogleSttProvider::new(&config, new_reqwest_client())
                .map(|_| ())
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("`transcription.{field}` must not be empty"))
            );
        }
    }

    #[test]
    fn new_trims_api_key_and_copies_language() {
        let provider =
            GoogleSttProvider::new(&google_config(" key-1 ", "zh-CN"), new_reqwest_client())
                .unwrap();
        assert_eq!(provider.api_key, "key-1");
        assert_eq!(provider.language_code, "zh-CN");
        assert_eq!(provider.supported_audio_formats(), &["wav", "flac"]);
    }

    #[tokio::test]
    async fn transcribe_rejects_extensions_google_does_not_support() {
        let provider =
            GoogleSttProvider::new(&google_config("key-1", "en-US"), new_reqwest_client()).unwrap();

        // `.m4a` is a Whisper-compatible format but Google STT rejects it, so
        // the error surfaces before any network request.
        let err = provider
            .transcribe(b"data".to_vec(), "voice.m4a")
            .await
            .unwrap_err();
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
