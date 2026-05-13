use anda_core::BoxError;
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};

use super::{TranscriptionProvider, parse_whisper_response, resolve_audio_format};
use crate::config;

/// Self-hosted faster-whisper-compatible STT provider.
///
/// POSTs audio as `multipart/form-data` (field name `file`) to a configurable
/// HTTP endpoint (e.g. `http://localhost:8000` or a private network host). The endpoint
/// must return `{"text": "..."}`. No cloud API key required. Size limit is
/// configurable — not constrained by the 25 MB cloud API cap.
pub struct LocalWhisperProvider {
    url: String,
    bearer_token: Option<String>,
    max_audio_bytes: usize,
    timeout_secs: u64,
    http: reqwest::Client,
}

impl LocalWhisperProvider {
    /// Build from config. Fails if `url` is empty or invalid, if `url` is not
    /// HTTP/HTTPS, if `max_audio_bytes` is zero, or if `timeout_secs` is zero.
    pub fn from_config(
        config: &config::LocalWhisperConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let url = config.url.trim().to_string();
        if url.is_empty() {
            return Err("local_whisper: `url` must not be empty".into());
        }

        let parsed = url
            .parse::<reqwest::Url>()
            .map_err(|e| format!("local_whisper: invalid `url` {url:?}: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!(
                "local_whisper: `url` must use http or https scheme, got {:?}",
                parsed.scheme()
            )
            .into());
        }

        let bearer_token = config
            .bearer_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned);

        if config.max_audio_bytes == 0 {
            return Err("local_whisper: `max_audio_bytes` must be greater than zero".into());
        }

        if config.timeout_secs == 0 {
            return Err("local_whisper: `timeout_secs` must be greater than zero".into());
        }

        Ok(Self {
            url,
            bearer_token,
            max_audio_bytes: config.max_audio_bytes,
            timeout_secs: config.timeout_secs,
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for LocalWhisperProvider {
    fn name(&self) -> &str {
        "local_whisper"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        if audio_data.len() > self.max_audio_bytes {
            return Err(format!(
                "Audio file too large ({} bytes, local_whisper max {})",
                audio_data.len(),
                self.max_audio_bytes
            )
            .into());
        }

        let (normalized_name, mime) = resolve_audio_format(file_name)?;

        // to_vec() clones the buffer for the multipart payload; peak memory per
        // call is ~2× max_audio_bytes. TODO: replace with streaming upload once
        // reqwest supports body streaming in multipart parts.
        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let mut request = self.http.post(&self.url);
        if let Some(ref bearer_token) = self.bearer_token {
            request = request.bearer_auth(bearer_token);
        }

        let resp = request
            .multipart(Form::new().part("file", file_part))
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .map_err(|_| "Failed to send audio to local Whisper endpoint")?;

        parse_whisper_response(resp).await
    }
}
