use anda_core::{BoxError, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::config::{self, TranscriptionConfig};

/// Maximum upload size accepted by most Whisper-compatible APIs (25 MB).
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

/// Request timeout for transcription API calls (seconds).
const TRANSCRIPTION_TIMEOUT_SECS: u64 = 120;

// ── Audio utilities ─────────────────────────────────────────────

/// Map file extension to MIME type for Whisper-compatible transcription APIs.
fn mime_for_audio(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "flac" => Some("audio/flac"),
        "mp3" | "mpeg" | "mpga" => Some("audio/mpeg"),
        "mp4" | "m4a" => Some("audio/mp4"),
        "ogg" | "oga" => Some("audio/ogg"),
        "opus" => Some("audio/opus"),
        "wav" => Some("audio/wav"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

pub fn supported_audio_resource_tags() -> Vec<String> {
    [
        "audio", "flac", "mp3", "mp4", "m4a", "mpeg", "mpga", "oga", "ogg", "opus", "wav", "webm",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

pub fn is_audio_resource(resource: &Resource) -> bool {
    resource.tags.iter().any(|tag| {
        let tag = tag.to_ascii_lowercase();
        tag == "audio" || mime_for_audio(&tag).is_some()
    }) || resource
        .mime_type
        .as_deref()
        .is_some_and(|mime| mime.to_ascii_lowercase().starts_with("audio/"))
}

pub fn audio_resource_file_name(resource: &Resource, fallback_stem: &str) -> String {
    if !resource.name.trim().is_empty() && resource.name.rsplit_once('.').is_some() {
        return resource.name.clone();
    }

    if let Some(ext) = resource.tags.iter().find_map(|tag| {
        let tag = tag.to_ascii_lowercase();
        mime_for_audio(&tag).map(|_| tag)
    }) {
        return format!("{fallback_stem}.{ext}");
    }

    if let Some(ext) = resource
        .mime_type
        .as_deref()
        .and_then(extension_for_audio_mime)
    {
        return format!("{fallback_stem}.{ext}");
    }

    format!("{fallback_stem}.wav")
}

fn extension_for_audio_mime(mime: &str) -> Option<&'static str> {
    match mime.to_ascii_lowercase().as_str() {
        "audio/flac" => Some("flac"),
        "audio/mp4" | "audio/x-m4a" => Some("m4a"),
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/ogg" | "audio/oga" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/webm" => Some("webm"),
        _ => None,
    }
}

/// Normalize audio filename for Whisper-compatible APIs.
///
/// Groq validates the filename extension — `.oga` (Opus-in-Ogg) is not in
/// its accepted list, so we rewrite it to `.ogg`.
fn normalize_audio_filename(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("oga") => format!("{stem}.ogg"),
        _ => file_name.to_string(),
    }
}

/// Resolve MIME type and normalize filename from extension.
///
/// No size check — callers enforce their own limits.
fn resolve_audio_format(file_name: &str) -> Result<(String, &'static str), BoxError> {
    let normalized_name = normalize_audio_filename(file_name);
    let extension = normalized_name
        .rsplit_once('.')
        .map(|(_, e)| e)
        .unwrap_or("");
    let mime = mime_for_audio(extension).ok_or_else(|| {
        format!(
            "Unsupported audio format '.{extension}' — \
                 accepted: flac, mp3, mp4, mpeg, mpga, m4a, ogg, opus, wav, webm"
        )
    })?;
    Ok((normalized_name, mime))
}

/// Validate audio data and resolve MIME type from file name.
///
/// Enforces the 25 MB cloud API cap. Returns `(normalized_filename, mime_type)` on success.
fn validate_audio(audio_data: &[u8], file_name: &str) -> Result<(String, &'static str), BoxError> {
    if audio_data.len() > MAX_AUDIO_BYTES {
        Err(format!(
            "Audio file too large ({} bytes, max {MAX_AUDIO_BYTES})",
            audio_data.len()
        )
        .into())
    } else {
        resolve_audio_format(file_name)
    }
}

// ── TranscriptionProvider trait ─────────────────────────────────

/// Trait for speech-to-text provider implementations.
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Human-readable provider name (e.g. "groq", "openai").
    fn name(&self) -> &str;

    /// Transcribe raw audio bytes. `file_name` includes the extension for
    /// format detection (e.g. "voice.ogg").
    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError>;
}

// ── GroqProvider ────────────────────────────────────────────────

/// Groq Whisper API provider (default, backward-compatible with existing config).
pub struct GroqProvider {
    api_url: String,
    model: String,
    api_key: String,
    language: Option<String>,
    http: reqwest::Client,
}

impl GroqProvider {
    /// Build from the Groq provider configuration.
    pub fn from_config(
        config: &config::GroqSttConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("transcription.api_key must not be empty".into());
        }

        Ok(Self {
            api_url: config.api_url.clone(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            language: config.language.clone(),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for GroqProvider {
    fn name(&self) -> &str {
        "groq"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let (normalized_name, mime) = validate_audio(audio_data, file_name)?;

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let mut form = Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        if let Some(ref lang) = self.language {
            form = form.text("language", lang.clone());
        }

        let resp = self
            .http
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to Groq")?;

        parse_whisper_response(resp).await
    }
}

// ── OpenAiWhisperProvider ───────────────────────────────────────

/// OpenAI Whisper API provider.
pub struct OpenAiWhisperProvider {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl OpenAiWhisperProvider {
    pub fn from_config(
        config: &crate::config::OpenAiSttConfig,
        http: reqwest::Client,
    ) -> Result<Self, BoxError> {
        let api_key = config.api_key.trim();
        if api_key.is_empty() {
            return Err("Missing OpenAI STT API key: set [transcription.openai].api_key".into());
        }

        Ok(Self {
            api_key: api_key.to_string(),
            model: config.model.clone(),
            http,
        })
    }
}

#[async_trait]
impl TranscriptionProvider for OpenAiWhisperProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let (normalized_name, mime) = validate_audio(audio_data, file_name)?;

        let file_part = Part::bytes(audio_data.to_vec())
            .file_name(normalized_name)
            .mime_str(mime)?;

        let form = Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "json");

        let resp = self
            .http
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to OpenAI")?;

        parse_whisper_response(resp).await
    }
}

// ── GoogleSttProvider ───────────────────────────────────────────

/// Google Cloud Speech-to-Text API provider.
pub struct GoogleSttProvider {
    api_key: String,
    language_code: String,
    http: reqwest::Client,
}

impl GoogleSttProvider {
    pub fn from_config(
        config: &crate::config::GoogleSttConfig,
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

    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        let (normalized_name, _) = validate_audio(audio_data, file_name)?;

        let encoding = match normalized_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("flac") => "FLAC",
            Some("wav") => "LINEAR16",
            Some("ogg" | "opus") => "OGG_OPUS",
            Some("mp3") => "MP3",
            Some("webm") => "WEBM_OPUS",
            Some(ext) => return Err(format!("Google STT does not support '.{ext}' input").into()),
            None => return Err("Google STT requires a file extension".into()),
        };

        let audio_content = STANDARD.encode(audio_data);

        let request_body = serde_json::json!({
            "config": {
                "encoding": encoding,
                "languageCode": &self.language_code,
                "enableAutomaticPunctuation": true,
            },
            "audio": {
                "content": audio_content,
            }
        });

        let url = format!(
            "https://speech.googleapis.com/v1/speech:recognize?key={}",
            self.api_key
        );

        let resp = self
            .http
            .post(&url)
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(TRANSCRIPTION_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|_| "Failed to send transcription request to Google STT")?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| "Failed to parse Google STT response")?;

        if !status.is_success() {
            let error_msg = body["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(format!("Google STT API error ({}): {}", status, error_msg).into());
        }

        let text = body["results"][0]["alternatives"][0]["transcript"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }
}

// ── LocalWhisperProvider ────────────────────────────────────────

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
        config: &crate::config::LocalWhisperConfig,
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

// ── Shared response parsing ─────────────────────────────────────

/// Parse a faster-whisper-compatible JSON response (`{ "text": "..." }`).
///
/// Checks HTTP status before attempting JSON parsing so that non-JSON error
/// bodies (plain text, HTML, empty 5xx) produce a readable status error
/// rather than a confusing "Failed to parse transcription response".
async fn parse_whisper_response(resp: reqwest::Response) -> Result<String, BoxError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Transcription API error ({}): {}", status, body.trim()).into());
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| "Failed to parse transcription response")?;

    let text = body["text"]
        .as_str()
        .ok_or("Transcription response missing 'text' field")?
        .to_string();

    Ok(text)
}

// ── TranscriptionManager ────────────────────────────────────────

/// Manages multiple STT providers and routes transcription requests.
pub struct TranscriptionManager {
    providers: HashMap<String, Box<dyn TranscriptionProvider>>,
    default_provider: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TranscriptionArgs {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub audio_base64: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscriptionOutput {
    pub text: String,
    pub provider: String,
    pub file_name: String,
}

impl TranscriptionManager {
    pub const NAME: &'static str = "transcribe_audio";

    /// Build a `TranscriptionManager` from config.
    ///
    /// Registers each provider when its config section is present.
    ///
    /// Provider keys with missing API keys are silently skipped — the error
    /// surfaces at transcribe-time so callers that target a different default
    /// provider are not blocked.
    pub fn new(config: &TranscriptionConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        let mut providers: HashMap<String, Box<dyn TranscriptionProvider>> = HashMap::new();

        if !config.enabled {
            return Ok(Self {
                providers,
                default_provider: config.default_provider.clone(),
            });
        }

        if let Some(ref groq_cfg) = config.groq {
            match GroqProvider::from_config(groq_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping Groq STT provider: {e}");
                }
            }
        }

        if let Some(ref openai_cfg) = config.openai {
            match OpenAiWhisperProvider::from_config(openai_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping OpenAI STT provider: {e}");
                }
            }
        }

        if let Some(ref google_cfg) = config.google {
            match GoogleSttProvider::from_config(google_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping Google STT provider: {e}");
                }
            }
        }

        if let Some(ref local_cfg) = config.local_whisper {
            match LocalWhisperProvider::from_config(local_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping local_whisper STT provider: {e}");
                }
            }
        }

        let default_provider = config.default_provider.clone();

        if config.enabled && !providers.contains_key(&default_provider) {
            let available: Vec<&str> = providers.keys().map(|k| k.as_str()).collect();
            return Err(format!(
                "Default transcription provider '{}' is not configured. Available: {available:?}",
                default_provider
            )
            .into());
        }

        Ok(Self {
            providers,
            default_provider,
        })
    }

    pub fn is_enabled(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Transcribe audio using the default provider.
    pub async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError> {
        self.transcribe_with_provider(audio_data, file_name, &self.default_provider)
            .await
    }

    /// Transcribe audio using a specific named provider.
    pub async fn transcribe_with_provider(
        &self,
        audio_data: &[u8],
        file_name: &str,
        provider: &str,
    ) -> Result<String, BoxError> {
        let p = self.providers.get(provider).ok_or_else(|| {
            let available = self.available_providers();
            format!("Transcription provider '{provider}' not configured. Available: {available:?}")
        })?;

        p.transcribe(audio_data, file_name).await
    }

    /// List registered provider names.
    pub fn available_providers(&self) -> Vec<String> {
        let mut names: Vec<_> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Tool<BaseCtx> for TranscriptionManager {
    type Args = TranscriptionArgs;
    type Output = TranscriptionOutput;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Transcribe speech audio into text. Accepts an audio resource artifact, or an audio_base64 payload for direct tool calls.".to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "provider": {
                        "type": ["string", "null"],
                        "description": "Optional transcription provider name. Omit to use the configured default provider."
                    },
                    "file_name": {
                        "type": ["string", "null"],
                        "description": "Audio file name with extension. Required when audio_base64 is provided and useful for format detection."
                    },
                    "audio_base64": {
                        "type": ["string", "null"],
                        "description": "Optional base64-encoded audio data. Prefer passing audio resources when available."
                    }
                },
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    fn supported_resource_tags(&self) -> Vec<String> {
        supported_audio_resource_tags()
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let provider = crate::config::normalize_optional(&args.provider)
            .unwrap_or_else(|| self.default_provider.clone());
        let (audio, file_name) = if let Some(audio_base64) = args
            .audio_base64
            .as_deref()
            .and_then(crate::config::normalize_string)
        {
            let file_name = args
                .file_name
                .as_deref()
                .and_then(crate::config::normalize_string)
                .unwrap_or_else(|| "audio.wav".to_string());
            let audio = STANDARD
                .decode(audio_base64)
                .map_err(|_| "invalid audio_base64 payload")?;
            (audio, file_name)
        } else {
            let resource = resources
                .iter()
                .find(|resource| is_audio_resource(resource))
                .ok_or("no audio resource provided")?;
            let audio = resource
                .blob
                .as_ref()
                .map(|blob| blob.0.clone())
                .ok_or("audio resource missing inline blob data")?;
            let file_name = args
                .file_name
                .as_deref()
                .and_then(crate::config::normalize_string)
                .unwrap_or_else(|| audio_resource_file_name(resource, "audio"));
            (audio, file_name)
        };

        let text = self
            .transcribe_with_provider(&audio, &file_name, &provider)
            .await?;
        Ok(ToolOutput::new(TranscriptionOutput {
            text,
            provider,
            file_name,
        }))
    }
}

// ── Backward-compatible convenience function ────────────────────

/// Transcribe audio bytes via a Whisper-compatible transcription API.
///
/// Returns the transcribed text on success.
///
/// This is the backward-compatible entry point that preserves the original
/// function signature. It routes through `config.default_provider` using the
/// provider-specific config sections.
///
/// The caller is responsible for enforcing duration limits *before* downloading
/// the file; this function enforces the byte-size cap.
#[allow(dead_code)]
pub async fn transcribe_audio(
    audio_data: Vec<u8>,
    file_name: &str,
    config: &TranscriptionConfig,
) -> Result<String, BoxError> {
    // Validate audio before resolving credentials so that size/format errors
    // are reported before missing-key errors (preserves original behavior).
    validate_audio(&audio_data, file_name)?;

    let http = reqwest::Client::new();

    match config.default_provider.as_str() {
        "groq" => {
            let groq_cfg = config.groq.as_ref().ok_or(
                "Default transcription provider 'groq' is not configured. Add [transcription.groq]",
            )?;
            let groq = GroqProvider::from_config(groq_cfg, http)?;
            groq.transcribe(&audio_data, file_name).await
        }
        "openai" => {
            let openai_cfg = config.openai.as_ref().ok_or(
                "Default transcription provider 'openai' is not configured. Add [transcription.openai]",
            )?;
            let openai = OpenAiWhisperProvider::from_config(openai_cfg, http)?;
            openai.transcribe(&audio_data, file_name).await
        }
        "google" => {
            let google_cfg = config.google.as_ref().ok_or(
                "Default transcription provider 'google' is not configured. Add [transcription.google]",
            )?;
            let google = GoogleSttProvider::from_config(google_cfg, http)?;
            google.transcribe(&audio_data, file_name).await
        }
        "local_whisper" => {
            let local_cfg = config.local_whisper.as_ref().ok_or(
                "Default transcription provider 'local_whisper' is not configured. Add [transcription.local_whisper]",
            )?;
            let local = LocalWhisperProvider::from_config(local_cfg, http)?;
            local.transcribe(&audio_data, file_name).await
        }
        other => Err(format!("Unsupported transcription provider '{other}'").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_oga_filename_for_whisper_compatibility() {
        assert_eq!(normalize_audio_filename("voice.oga"), "voice.ogg");
        assert_eq!(normalize_audio_filename("voice.mp3"), "voice.mp3");
    }

    #[test]
    fn resolve_audio_format_rejects_unknown_extensions() {
        assert!(resolve_audio_format("voice.txt").is_err());
        assert_eq!(resolve_audio_format("voice.mp3").unwrap().1, "audio/mpeg");
    }
}
