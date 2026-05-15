use anda_core::{BoxError, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::config::TranscriptionConfig;

mod google;
mod groq;
mod local_whisper;
mod openai;
mod stepfun;

pub use google::GoogleSttProvider;
pub use groq::GroqProvider;
pub use local_whisper::LocalWhisperProvider;
pub use openai::OpenAiWhisperProvider;
pub use stepfun::StepFunProvider;

/// Maximum upload size accepted by most Whisper-compatible APIs (25 MB).
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

/// Request timeout for transcription API calls (seconds).
const TRANSCRIPTION_TIMEOUT_SECS: u64 = 120;

const WHISPER_COMPATIBLE_AUDIO_FORMATS: &[&str] = &[
    "webm", "ogg", "mp4", "m4a", "mp3", "mpeg", "mpga", "wav", "flac", "opus",
];

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
        "audio", "flac", "mp3", "mp4", "m4a", "mpeg", "mpga", "oga", "ogg", "opus", "pcm", "wav",
        "webm",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

pub fn is_audio_resource(resource: &Resource) -> bool {
    resource.tags.iter().any(|tag| {
        let tag = tag.to_ascii_lowercase();
        tag == "audio" || tag == "pcm" || mime_for_audio(&tag).is_some()
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
        (tag == "pcm" || mime_for_audio(&tag).is_some()).then_some(tag)
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
        "audio/pcm" | "audio/l16" => Some("pcm"),
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

fn audio_extension(file_name: &str) -> Option<String> {
    normalize_audio_filename(file_name)
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

// ── TranscriptionProvider trait ─────────────────────────────────

/// Trait for speech-to-text provider implementations.
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Human-readable provider name (e.g. "groq", "openai").
    fn name(&self) -> &str;

    /// Audio container/extension names accepted by this provider.
    fn supported_audio_formats(&self) -> &'static [&'static str] {
        WHISPER_COMPATIBLE_AUDIO_FORMATS
    }

    /// Transcribe raw audio bytes. `file_name` includes the extension for
    /// format detection (e.g. "voice.ogg").
    async fn transcribe(&self, audio_data: &[u8], file_name: &str) -> Result<String, BoxError>;
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

        if let Some(ref stepfun_cfg) = config.stepfun {
            match StepFunProvider::from_config(stepfun_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping StepFun STT provider: {e}");
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

    pub fn supported_audio_formats(&self) -> Vec<String> {
        self.providers
            .get(&self.default_provider)
            .map(|provider| {
                provider
                    .supported_audio_formats()
                    .iter()
                    .map(|format| (*format).to_string())
                    .collect()
            })
            .unwrap_or_default()
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
    if config.default_provider == "stepfun" {
        stepfun::validate_audio(&audio_data, file_name)?;
    } else {
        validate_audio(&audio_data, file_name)?;
    }

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
        "stepfun" => {
            let stepfun_cfg = config.stepfun.as_ref().ok_or(
                "Default transcription provider 'stepfun' is not configured. Add [transcription.stepfun]",
            )?;
            let stepfun = StepFunProvider::from_config(stepfun_cfg, http)?;
            stepfun.transcribe(&audio_data, file_name).await
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
