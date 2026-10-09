use anda_core::{BoxError, FunctionDefinition, Resource, StateFeatures, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use async_trait::async_trait;
use base64::{Engine, display::Base64Display, engine::general_purpose::STANDARD};
use reqwest::multipart::Part;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::config::{self, TranscriptionConfig};
use crate::engine::ResourceStore;
use crate::util::http_client::check_http_response;

mod google;
mod local_whisper;
mod stepfun;
mod whisper;

use google::GoogleSttProvider;
use local_whisper::LocalWhisperProvider;
use stepfun::StepFunProvider;
use whisper::WhisperApiProvider;

/// Maximum upload size accepted by most Whisper-compatible APIs (25 MB).
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

/// Request timeout for cloud transcription API calls.
const TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(120);

const WHISPER_COMPATIBLE_AUDIO_FORMATS: &[&str] = &[
    "webm", "ogg", "mp4", "m4a", "mp3", "mpeg", "mpga", "wav", "flac", "opus",
];

/// MIME types of the audio extensions Whisper-compatible APIs accept. `.oga`
/// is uploaded as `.ogg` (see [`normalize_audio_filename`]).
const AUDIO_MIME_TYPES: &[(&str, &str)] = &[
    ("flac", "audio/flac"),
    ("m4a", "audio/mp4"),
    ("mp3", "audio/mpeg"),
    ("mp4", "audio/mp4"),
    ("mpeg", "audio/mpeg"),
    ("mpga", "audio/mpeg"),
    ("oga", "audio/ogg"),
    ("ogg", "audio/ogg"),
    ("opus", "audio/opus"),
    ("wav", "audio/wav"),
    ("webm", "audio/webm"),
];

// ── Audio utilities ─────────────────────────────────────────────

/// Map file extension to MIME type for Whisper-compatible transcription APIs.
fn mime_for_audio(extension: &str) -> Option<&'static str> {
    AUDIO_MIME_TYPES
        .iter()
        .find(|(ext, _)| ext.eq_ignore_ascii_case(extension))
        .map(|(_, mime)| *mime)
}

/// Whether `extension` names an audio container some provider accepts.
fn is_audio_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("pcm") || mime_for_audio(extension).is_some()
}

pub fn supported_audio_resource_tags() -> Vec<String> {
    ["audio", "pcm"]
        .into_iter()
        .chain(AUDIO_MIME_TYPES.iter().map(|(ext, _)| *ext))
        .map(ToString::to_string)
        .collect()
}

pub fn is_audio_resource(resource: &Resource) -> bool {
    resource
        .tags
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("audio") || is_audio_extension(tag))
        || resource.mime_type.as_deref().is_some_and(|mime| {
            mime.trim()
                .get(..6)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("audio/"))
        })
}

pub fn audio_resource_file_name(resource: &Resource, fallback_stem: &str) -> String {
    let name = resource.name.trim();
    if let Some((_, ext)) = name.rsplit_once('.')
        && is_audio_extension(ext)
    {
        return name.to_string();
    }

    if let Some(ext) = resource.tags.iter().find(|tag| is_audio_extension(tag)) {
        return format!("{fallback_stem}.{}", ext.to_ascii_lowercase());
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
    match mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
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
fn resolve_audio_format(file_name: &str) -> Result<(String, &'static str), BoxError> {
    let normalized_name = normalize_audio_filename(file_name);
    let extension = normalized_name
        .rsplit_once('.')
        .map(|(_, e)| e)
        .unwrap_or("");
    let mime = mime_for_audio(extension).ok_or_else(|| {
        format!(
            "Unsupported audio format '.{extension}' — accepted: {}",
            WHISPER_COMPATIBLE_AUDIO_FORMATS.join(", ")
        )
    })?;
    Ok((normalized_name, mime))
}

fn audio_extension(file_name: &str) -> Option<String> {
    normalize_audio_filename(file_name)
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Rejects empty audio and audio larger than `max_bytes`.
fn check_audio_size(audio: &[u8], max_bytes: usize) -> Result<(), BoxError> {
    if audio.is_empty() {
        return Err("Audio data must not be empty".into());
    }
    if audio.len() > max_bytes {
        return Err(format!(
            "Audio file too large ({} bytes, max {max_bytes})",
            audio.len()
        )
        .into());
    }
    Ok(())
}

/// Serializes bytes as a Base64 string straight into a JSON body, without an
/// intermediate `String` copy of the audio.
struct Base64<'a>(&'a [u8]);

impl Serialize for Base64<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&Base64Display::new(self.0, &STANDARD))
    }
}

// ── Config validation ───────────────────────────────────────────

/// A trimmed, non-empty config value; `field` is its path under `transcription`.
fn required(value: &str, field: &str) -> Result<String, BoxError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("`transcription.{field}` must not be empty").into());
    }
    Ok(value.to_string())
}

/// A required HTTP or HTTPS endpoint URL.
fn http_url(value: &str, field: &str) -> Result<String, BoxError> {
    let url = required(value, field)?;
    let parsed = url
        .parse::<reqwest::Url>()
        .map_err(|err| format!("invalid `transcription.{field}` {url:?}: {err}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "`transcription.{field}` must use http or https, got {:?}",
            parsed.scheme()
        )
        .into());
    }
    Ok(url)
}

// ── TranscriptionProvider trait ─────────────────────────────────

/// Trait for speech-to-text provider implementations.
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Audio container/extension names accepted by this provider.
    fn supported_audio_formats(&self) -> &'static [&'static str] {
        WHISPER_COMPATIBLE_AUDIO_FORMATS
    }

    /// Transcribe audio bytes. `file_name` includes the extension for format
    /// detection (e.g. "voice.ogg").
    async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError>;
}

// ── Shared Whisper requests and responses ───────────────────────

/// The multipart `file` part; the audio moves into the request body.
fn whisper_file_part(audio: Vec<u8>, file_name: &str) -> Result<Part, BoxError> {
    let (name, mime) = resolve_audio_format(file_name)?;
    Ok(Part::bytes(audio).file_name(name).mime_str(mime)?)
}

/// Parse a Whisper-compatible JSON response (`{ "text": "..." }`).
///
/// Checks HTTP status before attempting JSON parsing so that non-JSON error
/// bodies (plain text, HTML, empty 5xx) produce a readable status error
/// rather than a confusing "Failed to parse transcription response".
async fn parse_whisper_response(resp: reqwest::Response) -> Result<String, BoxError> {
    #[derive(Deserialize)]
    struct WhisperResponse {
        text: String,
    }

    let body: WhisperResponse = check_http_response(resp, "Transcription")
        .await?
        .json()
        .await
        .map_err(|err| {
            format!(
                "Failed to parse transcription response: {:?}",
                err.without_url()
            )
        })?;
    Ok(body.text)
}

// ── TranscriptionManager ────────────────────────────────────────

/// Manages multiple STT providers and routes transcription requests.
pub struct TranscriptionManager {
    providers: HashMap<String, Box<dyn TranscriptionProvider>>,
    /// Always a key of `providers`.
    default_provider: String,
    /// Loads the audio attachments the model names by `resource_id`.
    resource_store: Option<Arc<ResourceStore>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TranscriptionArgs {
    pub provider: Option<String>,
    pub resource_id: Option<u64>,
    pub file_name: Option<String>,
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

    /// Build a `TranscriptionManager` from config; `None` when transcription
    /// is disabled.
    ///
    /// An invalid default provider fails with its own error; other invalid
    /// providers are skipped with a warning.
    pub fn new(
        config: &TranscriptionConfig,
        http: reqwest::Client,
    ) -> Result<Option<Self>, BoxError> {
        if !config.enabled {
            return Ok(None);
        }

        let default_provider = config.default_provider.clone();
        let prompt = config.initial_prompt.as_deref();
        let mut providers = HashMap::new();
        if let Some(cfg) = &config.groq {
            let provider = WhisperApiProvider::groq(cfg, prompt, http.clone());
            register(&mut providers, &default_provider, "groq", provider)?;
        }
        if let Some(cfg) = &config.openai {
            let provider = WhisperApiProvider::openai(cfg, prompt, http.clone());
            register(&mut providers, &default_provider, "openai", provider)?;
        }
        if let Some(cfg) = &config.google {
            let provider = GoogleSttProvider::new(cfg, http.clone());
            register(&mut providers, &default_provider, "google", provider)?;
        }
        if let Some(cfg) = &config.stepfun {
            let provider = StepFunProvider::new(cfg, http.clone());
            register(&mut providers, &default_provider, "stepfun", provider)?;
        }
        if let Some(cfg) = &config.local_whisper {
            let provider = LocalWhisperProvider::new(cfg, http);
            register(&mut providers, &default_provider, "local_whisper", provider)?;
        }

        let manager = Self {
            providers,
            default_provider,
            resource_store: None,
        };
        if !manager.providers.contains_key(&manager.default_provider) {
            return Err(format!(
                "Default transcription provider '{}' is not configured (available: {})",
                manager.default_provider,
                manager.available_providers().join(", ")
            )
            .into());
        }
        Ok(Some(manager))
    }

    pub fn with_resource_store(mut self, resource_store: Arc<ResourceStore>) -> Self {
        self.resource_store = Some(resource_store);
        self
    }

    /// Transcribe audio using the default provider.
    pub async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError> {
        self.transcribe_with(&self.default_provider, audio, file_name)
            .await
    }

    /// Transcribe audio using a specific named provider.
    async fn transcribe_with(
        &self,
        provider: &str,
        audio: Vec<u8>,
        file_name: &str,
    ) -> Result<String, BoxError> {
        let stt = self.providers.get(provider).ok_or_else(|| {
            format!(
                "Transcription provider '{provider}' not configured (available: {})",
                self.available_providers().join(", ")
            )
        })?;
        stt.transcribe(audio, file_name).await
    }

    /// List registered provider names.
    pub fn available_providers(&self) -> Vec<String> {
        let mut names: Vec<_> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Audio formats the default provider accepts.
    pub fn supported_audio_formats(&self) -> Vec<String> {
        self.providers[&self.default_provider]
            .supported_audio_formats()
            .iter()
            .map(|format| (*format).to_string())
            .collect()
    }

    /// The attachment `resource_id` names, else the first attached audio
    /// resource.
    async fn audio_resource(
        &self,
        ctx: &BaseCtx,
        resource_id: Option<u64>,
        resources: Vec<Resource>,
    ) -> Result<Resource, BoxError> {
        let Some(id) = resource_id.filter(|id| *id > 0) else {
            return resources
                .into_iter()
                .find(is_audio_resource)
                .ok_or_else(|| "no audio provided: pass resource_id or audio_base64".into());
        };
        let store = self
            .resource_store
            .as_ref()
            .ok_or("resource_id is not supported here; pass audio_base64")?;
        let resource = store.get_resource_for(id, ctx.caller()).await?;
        if !is_audio_resource(&resource) {
            return Err(format!("resource {id} ({}) is not audio", resource.name).into());
        }
        Ok(resource)
    }
}

fn register<P: TranscriptionProvider + 'static>(
    providers: &mut HashMap<String, Box<dyn TranscriptionProvider>>,
    default_provider: &str,
    name: &str,
    provider: Result<P, BoxError>,
) -> Result<(), BoxError> {
    match provider {
        Ok(provider) => {
            providers.insert(name.to_string(), Box::new(provider));
        }
        Err(err) if name == default_provider => {
            return Err(
                format!("Default transcription provider '{name}' is invalid: {err}").into(),
            );
        }
        Err(err) => log::warn!("Skipping {name} STT provider: {err}"),
    }
    Ok(())
}

impl Tool<BaseCtx> for TranscriptionManager {
    type Args = TranscriptionArgs;
    type Output = TranscriptionOutput;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        format!(
            "Transcribe the speech in an audio attachment into text with a speech-to-text provider; it does not describe music or other sounds. Pass the attachment's resource_id. Available providers: {} (default: {}, accepting {}).",
            self.available_providers().join(", "),
            self.default_provider,
            self.supported_audio_formats().join(", ")
        )
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
                    "resource_id": {
                        "type": ["integer", "null"],
                        "description": "Resource _id of the audio attachment to transcribe."
                    },
                    "file_name": {
                        "type": ["string", "null"],
                        "description": "Audio file name with extension, used for format detection. Required with audio_base64; omit for attachments."
                    },
                    "audio_base64": {
                        "type": ["string", "null"],
                        "description": "Base64-encoded audio sent directly by a client. Omit when passing resource_id."
                    }
                },
                "required": ["provider", "resource_id", "file_name", "audio_base64"],
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
        ctx: BaseCtx,
        args: Self::Args,
        resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let TranscriptionArgs {
            provider,
            resource_id,
            file_name,
            audio_base64,
        } = args;
        let provider =
            config::normalize_optional(&provider).unwrap_or_else(|| self.default_provider.clone());
        let file_name = config::normalize_optional(&file_name);
        // The Base64 payload is dropped once decoded, before the upload starts.
        let (audio, file_name) = match audio_base64.filter(|audio| !audio.trim().is_empty()) {
            Some(audio_base64) => {
                let audio = STANDARD
                    .decode(audio_base64.trim())
                    .map_err(|_| "invalid audio_base64 payload")?;
                (audio, file_name.unwrap_or_else(|| "audio.wav".to_string()))
            }
            None => {
                let resource = self.audio_resource(&ctx, resource_id, resources).await?;
                let file_name =
                    file_name.unwrap_or_else(|| audio_resource_file_name(&resource, "audio"));
                let audio = resource
                    .blob
                    .ok_or("audio resource missing inline blob data")?
                    .0;
                (audio, file_name)
            }
        };

        let text = self.transcribe_with(&provider, audio, &file_name).await?;
        Ok(ToolOutput::new(TranscriptionOutput {
            text,
            provider,
            file_name,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        GoogleSttConfig, GroqSttConfig, LocalWhisperConfig, OpenAiSttConfig, StepFunSttConfig,
    };
    use crate::util::http_client::new_reqwest_client;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_core::ByteBufB64;
    use anda_engine::engine::EngineBuilder;
    use axum::{Router, routing};

    /// Echoes the received file name and checks the audio buffer's address.
    struct EchoProvider(Option<usize>);

    #[async_trait]
    impl TranscriptionProvider for EchoProvider {
        async fn transcribe(&self, audio: Vec<u8>, file_name: &str) -> Result<String, BoxError> {
            if let Some(ptr) = self.0 {
                assert_eq!(audio.as_ptr() as usize, ptr, "audio was copied");
            }
            Ok(format!("{file_name}:{}", audio.len()))
        }
    }

    fn echo_manager(expected_ptr: Option<usize>) -> TranscriptionManager {
        TranscriptionManager {
            providers: HashMap::from([(
                "echo".to_string(),
                Box::new(EchoProvider(expected_ptr)) as Box<dyn TranscriptionProvider>,
            )]),
            default_provider: "echo".to_string(),
            resource_store: None,
        }
    }

    fn mock_ctx() -> BaseCtx {
        EngineBuilder::new().mock_ctx().base
    }

    #[test]
    fn transcription_tool_schema_is_openai_strict() {
        let definition = echo_manager(None).definition();

        assert_eq!(definition.strict, Some(true));
        assert_openai_strict_parameters(&definition.parameters);
        assert!(definition.description.contains("resource_id"));
        assert!(
            definition
                .description
                .contains("Available providers: echo (default: echo, accepting webm, ogg")
        );
    }

    #[test]
    fn normalize_oga_filename_for_whisper_compatibility() {
        assert_eq!(normalize_audio_filename("voice.oga"), "voice.ogg");
        assert_eq!(normalize_audio_filename("voice.mp3"), "voice.mp3");
    }

    #[test]
    fn resolve_audio_format_rejects_unknown_extensions() {
        let err = resolve_audio_format("voice.txt").unwrap_err().to_string();
        assert!(err.contains("Unsupported audio format '.txt'"), "{err}");
        assert!(err.contains("accepted: webm, ogg, mp4"), "{err}");
        assert_eq!(resolve_audio_format("voice.mp3").unwrap().1, "audio/mpeg");
        assert_eq!(resolve_audio_format("voice.OGA").unwrap().1, "audio/ogg");
    }

    #[test]
    fn config_helpers_trim_and_validate() {
        assert_eq!(required(" key ", "groq.api_key").unwrap(), "key");
        assert!(
            required(" ", "groq.api_key")
                .unwrap_err()
                .to_string()
                .contains("`transcription.groq.api_key` must not be empty")
        );
        assert_eq!(
            http_url(" https://example.com/asr ", "stepfun.api_url").unwrap(),
            "https://example.com/asr"
        );
        for (url, expected) in [
            (" ", "must not be empty"),
            ("not a url", "invalid `transcription.stepfun.api_url`"),
            ("ftp://example.com", "must use http or https"),
        ] {
            let err = http_url(url, "stepfun.api_url").unwrap_err().to_string();
            assert!(err.contains(expected), "{url}: {err}");
        }
    }

    #[test]
    fn base64_serializes_without_an_intermediate_string() {
        let json = serde_json::to_string(&json!({ "data": Base64(b"audio") })).unwrap();
        assert_eq!(
            json,
            format!("{{\"data\":\"{}\"}}", STANDARD.encode(b"audio"))
        );
    }

    async fn spawn_whisper_mock(text: &'static str) -> String {
        let app = Router::new().route(
            "/transcribe",
            routing::post(move || async move { axum::Json(json!({"text": text})) }),
        );
        let base_url = crate::test_support::spawn_http_mock(app).await;
        format!("{base_url}/transcribe")
    }

    fn enabled_config_with_groq(api_url: String) -> TranscriptionConfig {
        TranscriptionConfig {
            enabled: true,
            default_provider: "groq".to_string(),
            groq: Some(GroqSttConfig {
                api_key: "gsk-test".to_string(),
                api_url,
                model: "whisper-large-v3".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn new_manager(config: &TranscriptionConfig) -> TranscriptionManager {
        TranscriptionManager::new(config, new_reqwest_client())
            .unwrap()
            .expect("transcription is enabled")
    }

    fn manager_error(config: &TranscriptionConfig) -> String {
        TranscriptionManager::new(config, new_reqwest_client())
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn manager_is_none_when_disabled() {
        assert!(
            TranscriptionManager::new(&TranscriptionConfig::default(), new_reqwest_client())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn manager_registers_valid_providers_and_skips_invalid_ones() {
        let config = TranscriptionConfig {
            enabled: true,
            default_provider: "groq".to_string(),
            groq: Some(GroqSttConfig {
                api_key: "gsk-test".to_string(),
                ..Default::default()
            }),
            // Empty API key: skipped with a warning instead of failing startup.
            openai: Some(OpenAiSttConfig::default()),
            google: Some(GoogleSttConfig {
                api_key: "key".to_string(),
                ..Default::default()
            }),
            stepfun: Some(StepFunSttConfig {
                api_key: "sk".to_string(),
                ..Default::default()
            }),
            local_whisper: Some(LocalWhisperConfig {
                url: "http://localhost:9/transcribe".to_string(),
                bearer_token: None,
                max_audio_bytes: 1024,
                timeout_secs: 5,
            }),
            ..Default::default()
        };

        let manager = new_manager(&config);
        assert_eq!(
            manager.available_providers(),
            vec!["google", "groq", "local_whisper", "stepfun"]
        );
        assert_eq!(
            manager.supported_audio_formats(),
            WHISPER_COMPATIBLE_AUDIO_FORMATS
                .iter()
                .map(|format| format.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn manager_reports_why_the_default_provider_is_unavailable() {
        // A configured but invalid default provider fails with its own cause.
        let config = TranscriptionConfig {
            enabled: true,
            default_provider: "openai".to_string(),
            openai: Some(OpenAiSttConfig::default()),
            ..Default::default()
        };
        assert_eq!(
            manager_error(&config),
            "Default transcription provider 'openai' is invalid: `transcription.openai.api_key` must not be empty"
        );

        let mut config = enabled_config_with_groq("http://localhost:9/transcribe".to_string());
        config.default_provider = "google".to_string();
        assert_eq!(
            manager_error(&config),
            "Default transcription provider 'google' is not configured (available: groq)"
        );
    }

    #[tokio::test]
    async fn manager_routes_transcription_to_default_provider() {
        let url = spawn_whisper_mock("routed text").await;
        let manager = new_manager(&enabled_config_with_groq(url));

        let text = manager
            .transcribe(b"data".to_vec(), "voice.mp3")
            .await
            .unwrap();
        assert_eq!(text, "routed text");

        let err = manager
            .transcribe_with("missing", b"data".to_vec(), "voice.mp3")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Transcription provider 'missing' not configured (available: groq)"
        );
    }

    #[tokio::test]
    async fn transcription_tool_call_decodes_base64_payload() {
        let url = spawn_whisper_mock("from base64").await;
        let manager = new_manager(&enabled_config_with_groq(url));

        let output = manager
            .call(
                mock_ctx(),
                TranscriptionArgs {
                    audio_base64: Some(STANDARD.encode(b"data")),
                    ..Default::default()
                },
                Vec::new(),
            )
            .await
            .unwrap();

        assert_eq!(output.output.text, "from base64");
        assert_eq!(output.output.provider, "groq");
        assert_eq!(output.output.file_name, "audio.wav");
    }

    #[tokio::test]
    async fn transcription_tool_call_reads_audio_resource_blob() {
        let url = spawn_whisper_mock("from resource").await;
        let manager = new_manager(&enabled_config_with_groq(url));

        let resource = Resource {
            name: "note.ogg".to_string(),
            tags: vec!["audio".to_string()],
            blob: Some(ByteBufB64(b"data".to_vec())),
            ..Default::default()
        };
        let output = manager
            .call(mock_ctx(), TranscriptionArgs::default(), vec![resource])
            .await
            .unwrap();

        assert_eq!(output.output.text, "from resource");
        assert_eq!(output.output.file_name, "note.ogg");
    }

    #[tokio::test]
    async fn transcription_tool_call_rejects_bad_input() {
        let manager = echo_manager(None);

        let err = manager
            .call(
                mock_ctx(),
                TranscriptionArgs {
                    audio_base64: Some("not base64!!!".to_string()),
                    ..Default::default()
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("invalid audio_base64 payload"));

        let err = manager
            .call(mock_ctx(), TranscriptionArgs::default(), Vec::new())
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("no audio provided"));

        let err = manager
            .call(
                mock_ctx(),
                TranscriptionArgs {
                    resource_id: Some(1),
                    ..Default::default()
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("resource_id is not supported"));
    }

    #[tokio::test]
    async fn resource_id_loads_an_owned_audio_attachment() {
        let ctx = mock_ctx();
        let store = Arc::new(
            ResourceStore::connect(crate::test_support::memory_db("stt_resources").await)
                .await
                .unwrap(),
        );
        let audio = Resource {
            name: "memo.m4a".to_string(),
            tags: vec!["audio".to_string()],
            blob: Some(ByteBufB64(b"voice".to_vec())),
            ..Default::default()
        };
        let text = Resource {
            name: "notes.txt".to_string(),
            tags: vec!["text".to_string()],
            blob: Some(ByteBufB64(b"hi".to_vec())),
            ..Default::default()
        };
        let saved = store
            .persist_resources(ctx.caller(), vec![audio.clone(), text])
            .await
            .unwrap();
        // Message references carry no bytes: the tool must load them by id.
        assert!(saved[0].blob.is_none());
        // Different bytes, so the store does not dedupe it into ours.
        let mut foreign_audio = audio;
        foreign_audio.blob = Some(ByteBufB64(b"other voice".to_vec()));
        let foreign = store
            .persist_resources(
                &anda_core::Principal::management_canister(),
                vec![foreign_audio],
            )
            .await
            .unwrap()[0]
            ._id;
        let manager = echo_manager(None).with_resource_store(store);
        let call = |id: u64| {
            manager.call(
                ctx.clone(),
                TranscriptionArgs {
                    resource_id: Some(id),
                    ..Default::default()
                },
                Vec::new(),
            )
        };

        let output = call(saved[0]._id).await.unwrap();
        assert_eq!(output.output.text, "memo.m4a:5");
        assert_eq!(output.output.file_name, "memo.m4a");

        let err = call(saved[1]._id).await.map(|_| ()).unwrap_err();
        assert!(
            err.to_string().contains("(notes.txt) is not audio"),
            "{err}"
        );

        let err = call(foreign).await.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("permission denied"), "{err}");
    }

    #[test]
    fn audio_resource_helpers_detect_and_name_audio() {
        let tagged = Resource {
            tags: vec!["Audio".to_string()],
            ..Default::default()
        };
        assert!(is_audio_resource(&tagged));
        assert_eq!(audio_resource_file_name(&tagged, "voice"), "voice.wav");

        let by_mime = Resource {
            mime_type: Some("audio/mpeg".to_string()),
            ..Default::default()
        };
        assert!(is_audio_resource(&by_mime));
        assert_eq!(audio_resource_file_name(&by_mime, "voice"), "voice.mp3");

        let by_extension_tag = Resource {
            tags: vec!["FLAC".to_string()],
            ..Default::default()
        };
        assert!(is_audio_resource(&by_extension_tag));
        assert_eq!(
            audio_resource_file_name(&by_extension_tag, "voice"),
            "voice.flac"
        );

        let named = Resource {
            name: "memo.ogg".to_string(),
            tags: vec!["audio".to_string()],
            ..Default::default()
        };
        assert_eq!(audio_resource_file_name(&named, "voice"), "memo.ogg");

        let not_audio = Resource {
            tags: vec!["image".to_string()],
            mime_type: Some("image/png".to_string()),
            ..Default::default()
        };
        assert!(!is_audio_resource(&not_audio));

        let tags = supported_audio_resource_tags();
        for tag in ["audio", "pcm", "oga", "m4a", "webm"] {
            assert!(tags.contains(&tag.to_string()), "{tag}");
        }
    }

    #[test]
    fn resource_names_handle_mime_parameters_and_unknown_extensions() {
        for (mime, expected) in [
            (" Audio/WebM; codecs=opus ", "audio.webm"),
            ("audio/ogg;codecs=opus", "audio.ogg"),
        ] {
            let resource = Resource {
                name: "recording.bin".to_string(),
                mime_type: Some(mime.to_string()),
                ..Default::default()
            };
            assert!(is_audio_resource(&resource));
            assert_eq!(audio_resource_file_name(&resource, "audio"), expected);
        }
        assert!(
            check_audio_size(b"", 10)
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
        assert!(
            check_audio_size(b"12345", 4)
                .unwrap_err()
                .to_string()
                .contains("too large (5 bytes, max 4)")
        );
    }

    #[tokio::test]
    async fn manager_sends_initial_prompt_only_when_nonblank() {
        let app = Router::new().route(
            "/transcribe",
            routing::post(|body: axum::body::Bytes| async move {
                axum::Json(json!({"text": String::from_utf8(body.to_vec()).unwrap()}))
            }),
        );
        let base = crate::test_support::spawn_http_mock(app).await;
        for prompt in [None, Some("  "), Some("  Anda project names  ")] {
            let mut config = enabled_config_with_groq(format!("{base}/transcribe"));
            config.initial_prompt = prompt.map(str::to_string);
            let manager = new_manager(&config);
            let multipart = manager
                .transcribe(b"audio-data".to_vec(), "voice.oga")
                .await
                .unwrap();
            assert!(multipart.contains("filename=\"voice.ogg\""));
            assert!(multipart.contains("audio-data"));
            assert_eq!(
                multipart.contains("name=\"prompt\""),
                prompt == Some("  Anda project names  ")
            );
            if prompt == Some("  Anda project names  ") {
                assert!(multipart.contains("name=\"prompt\"\r\n\r\nAnda project names\r\n"));
            }
        }
    }

    #[tokio::test]
    async fn tool_moves_audio_into_the_provider_without_copying() {
        let audio = vec![1u8; 1024 * 1024];
        let manager = echo_manager(Some(audio.as_ptr() as usize));
        let resource = Resource {
            name: "voice.wav".into(),
            tags: vec!["audio".into()],
            blob: Some(ByteBufB64(audio)),
            ..Default::default()
        };
        let output = manager
            .call(mock_ctx(), TranscriptionArgs::default(), vec![resource])
            .await
            .unwrap();
        assert_eq!(output.output.text, "voice.wav:1048576");
    }
}
