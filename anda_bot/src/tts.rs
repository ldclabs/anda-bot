use anda_core::{BoxError, ByteBufB64, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use base64::{Engine, engine::general_purpose::STANDARD};
use ic_auth_types::Xid;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::config;

/// Maximum text length before synthesis is rejected (default: 4096 chars).
const DEFAULT_MAX_TEXT_LENGTH: usize = 4096;

/// Default HTTP request timeout for TTS API calls.
const TTS_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

// ── TtsProvider trait ────────────────────────────────────────────

/// Trait for pluggable TTS backends.
#[async_trait::async_trait]
pub trait TtsProvider: Send + Sync {
    /// Provider identifier (e.g. `"openai"`, `"google"`).
    fn name(&self) -> &str;

    /// Synthesize `text`, returning raw audio bytes.
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError>;
}

// ── TtsManager ───────────────────────────────────────────────────

/// Central manager for multi-provider TTS synthesis.
pub struct TtsManager {
    providers: HashMap<String, Box<dyn TtsProvider>>,
    default_provider: String,
    default_format: String,
    max_text_length: usize,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TtsArgs {
    pub text: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub artifact_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TtsOutput {
    pub provider: String,
    pub artifact: String,
    pub mime_type: String,
    pub format: String,
    pub size: u64,
}

impl TtsManager {
    pub const NAME: &'static str = "synthesize_speech";

    /// Build a `TtsManager` from config, initializing all configured providers.
    pub fn new(config: &config::TtsConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        let mut providers: HashMap<String, Box<dyn TtsProvider>> = HashMap::new();

        let max_text_length = if config.max_text_length == 0 {
            DEFAULT_MAX_TEXT_LENGTH
        } else {
            config.max_text_length
        };

        if !config.enabled {
            return Ok(Self {
                providers,
                default_provider: config.default_provider.clone(),
                default_format: config.default_format.clone(),
                max_text_length,
            });
        }

        if let Some(ref openai_cfg) = config.openai {
            match OpenAiTtsProvider::new(openai_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping OpenAI TTS provider: {e}");
                }
            }
        }

        if let Some(ref google_cfg) = config.google {
            match GoogleTtsProvider::new(google_cfg, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping Google TTS provider: {e}");
                }
            }
        }

        if let Some(ref edge_cfg) = config.edge {
            match EdgeTtsProvider::new(edge_cfg) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping Edge TTS provider: {e}");
                }
            }
        }

        let default_provider = config.default_provider.clone();
        if !providers.contains_key(&default_provider) {
            let available: Vec<&str> = providers.keys().map(|key| key.as_str()).collect();
            return Err(format!(
                "Default TTS provider '{}' is not configured. Available: {available:?}",
                default_provider
            )
            .into());
        }

        Ok(Self {
            providers,
            default_provider,
            default_format: config.default_format.clone(),
            max_text_length,
        })
    }

    pub fn is_enabled(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Synthesize text using the default provider and voice.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        self.synthesize_with_provider(text, &self.default_provider)
            .await
    }

    /// Synthesize text using a specific provider and voice.
    pub async fn synthesize_with_provider(
        &self,
        text: &str,
        provider: &str,
    ) -> Result<Vec<u8>, BoxError> {
        if text.is_empty() {
            return Err("TTS text must not be empty".into());
        }
        let char_count = text.chars().count();
        if char_count > self.max_text_length {
            return Err(format!(
                "TTS text too long ({} chars, max {})",
                char_count, self.max_text_length
            )
            .into());
        }

        let tts = self.providers.get(provider).ok_or_else(|| {
            format!(
                "TTS provider '{}' not configured (available: {})",
                provider,
                self.available_providers().join(", ")
            )
        })?;

        tts.synthesize(text).await
    }

    /// List names of all initialized providers.
    pub fn available_providers(&self) -> Vec<String> {
        let mut names: Vec<_> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn audio_format(&self) -> &str {
        normalize_audio_format(&self.default_format)
    }

    pub fn audio_mime_type(&self) -> &'static str {
        mime_for_audio_format(self.audio_format())
    }

    pub fn audio_artifact(&self, bytes: Vec<u8>, name: Option<String>) -> Resource {
        let format = self.audio_format();
        let name = normalize_artifact_name(name, format);
        let size = bytes.len() as u64;
        Resource {
            tags: vec!["audio".to_string(), format.to_string()],
            name,
            description: Some("Synthesized speech from anda_bot".to_string()),
            mime_type: Some(self.audio_mime_type().to_string()),
            blob: Some(ByteBufB64(bytes)),
            size: Some(size),
            ..Default::default()
        }
    }
}

impl Tool<BaseCtx> for TtsManager {
    type Args = TtsArgs;
    type Output = TtsOutput;

    fn name(&self) -> String {
        Self::NAME.to_string()
    }

    fn description(&self) -> String {
        "Convert text into speech audio. Returns the synthesized audio as an artifact resource that callers can play or attach.".to_string()
    }

    fn definition(&self) -> FunctionDefinition {
        FunctionDefinition {
            name: self.name(),
            description: self.description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to synthesize into speech.",
                        "minLength": 1
                    },
                    "provider": {
                        "type": ["string", "null"],
                        "description": "Optional TTS provider name. Omit to use the configured default provider."
                    },
                    "artifact_name": {
                        "type": ["string", "null"],
                        "description": "Optional output artifact file name. The configured audio extension is appended when missing."
                    }
                },
                "required": ["text"],
                "additionalProperties": false
            }),
            strict: Some(true),
        }
    }

    async fn call(
        &self,
        _ctx: BaseCtx,
        args: Self::Args,
        _resources: Vec<Resource>,
    ) -> Result<ToolOutput<Self::Output>, BoxError> {
        let provider = config::normalize_optional(&args.provider)
            .unwrap_or_else(|| self.default_provider.clone());
        let bytes = self.synthesize_with_provider(&args.text, &provider).await?;
        let artifact = self.audio_artifact(bytes, args.artifact_name);
        let output = TtsOutput {
            provider,
            artifact: artifact.name.clone(),
            mime_type: artifact.mime_type.clone().unwrap_or_default(),
            format: self.audio_format().to_string(),
            size: artifact.size.unwrap_or_default(),
        };
        let mut result = ToolOutput::new(output);
        result.artifacts.push(artifact);
        Ok(result)
    }
}

fn normalize_audio_format(format: &str) -> &'static str {
    match format.trim().to_ascii_lowercase().as_str() {
        "wav" => "wav",
        "opus" => "opus",
        "ogg" => "ogg",
        _ => "mp3",
    }
}

fn mime_for_audio_format(format: &str) -> &'static str {
    match format {
        "wav" => "audio/wav",
        "opus" => "audio/opus",
        "ogg" => "audio/ogg",
        _ => "audio/mpeg",
    }
}

fn normalize_artifact_name(name: Option<String>, format: &str) -> String {
    let fallback = format!("anda_bot_tts_{}.{}", Xid::new(), format);
    let Some(name) = name.and_then(|value| config::normalize_string(&value)) else {
        return fallback;
    };
    if name.rsplit_once('.').is_some() {
        name
    } else {
        format!("{name}.{format}")
    }
}

// ── OpenAI TTS ───────────────────────────────────────────────────

/// OpenAI TTS provider (`POST /v1/audio/speech`).
pub struct OpenAiTtsProvider {
    api_key: String,
    model: String,
    speed: f64,
    voice: String,
    http: reqwest::Client,
}

impl OpenAiTtsProvider {
    pub fn new(config: &config::OpenAiTtsConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("OpenAI TTS API key must not be empty".into());
        }

        Ok(Self {
            api_key: config.api_key.trim().to_string(),
            model: config.model.clone(),
            speed: config.speed,
            voice: config.voice.clone(),
            http,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for OpenAiTtsProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
            "voice": self.voice,
            "speed": self.speed,
            "response_format": "mp3",
        });

        let resp = self
            .http
            .post("https://api.openai.com/v1/audio/speech")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Failed to send OpenAI TTS request")?;

        let status = resp.status();
        if !status.is_success() {
            let error_body: serde_json::Value = resp
                .json()
                .await
                .unwrap_or_else(|_| serde_json::json!({"error": "unknown"}));
            let msg = error_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(format!("OpenAI TTS API error ({}): {}", status, msg).into());
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|_| "Failed to read OpenAI TTS response body")?;
        Ok(bytes.to_vec())
    }
}

// ── Google Cloud TTS ─────────────────────────────────────────────

/// Google Cloud TTS provider (`POST /v1/text:synthesize`).
pub struct GoogleTtsProvider {
    api_key: String,
    language_code: String,
    voice: String,
    http: reqwest::Client,
}

impl GoogleTtsProvider {
    pub fn new(config: &config::GoogleTtsConfig, http: reqwest::Client) -> Result<Self, BoxError> {
        if config.api_key.trim().is_empty() {
            return Err("Google TTS API key must not be empty".into());
        }

        Ok(Self {
            api_key: config.api_key.trim().to_string(),
            language_code: config.language_code.clone(),
            voice: config.voice.clone(),
            http,
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for GoogleTtsProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let url = "https://texttospeech.googleapis.com/v1/text:synthesize";
        let body = serde_json::json!({
            "input": { "text": text },
            "voice": {
                "languageCode": self.language_code,
                "name": self.voice,
            },
            "audioConfig": {
                "audioEncoding": "MP3",
            },
        });

        let resp = self
            .http
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Failed to send Google TTS request")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .map_err(|_| "Failed to parse Google TTS response")?;

        if !status.is_success() {
            let msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(format!("Google TTS API error ({}): {}", status, msg).into());
        }

        let audio_b64 = resp_body["audioContent"]
            .as_str()
            .ok_or("Google TTS response missing 'audioContent' field")?;

        let bytes = STANDARD
            .decode(audio_b64)
            .map_err(|_| "Failed to decode Google TTS base64 audio")?;
        Ok(bytes)
    }
}

// ── Edge TTS (subprocess) ────────────────────────────────────────

/// Edge TTS provider — free, uses the `edge-tts` CLI subprocess.
pub struct EdgeTtsProvider {
    binary_path: String,
    voice: String,
}

impl EdgeTtsProvider {
    /// Allowed basenames for the Edge TTS binary.
    const ALLOWED_BINARIES: &[&str] = &["edge-tts", "edge-playback"];

    /// Create a new Edge TTS provider from config.
    ///
    /// `binary_path` must be a bare command name (no path separators) matching
    /// one of [`Self::ALLOWED_BINARIES`]. This prevents arbitrary executable
    /// paths like `/tmp/malicious/edge-tts` from passing the basename check.
    pub fn new(config: &config::EdgeTtsConfig) -> Result<Self, BoxError> {
        let path = &config.binary_path;
        if path.contains('/') || path.contains('\\') {
            return Err(format!(
                "Edge TTS binary_path must be a bare command name without path separators, got: {path}"
            )
            .into());
        }
        if !Self::ALLOWED_BINARIES.contains(&path.as_str()) {
            return Err(format!(
                "Edge TTS binary_path must be one of {:?}, got: {path}",
                Self::ALLOWED_BINARIES,
            )
            .into());
        }
        Ok(Self {
            binary_path: config.binary_path.clone(),
            voice: config.voice.clone(),
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for EdgeTtsProvider {
    fn name(&self) -> &str {
        "edge"
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let temp_dir = std::env::temp_dir();
        let output_file = temp_dir.join(format!("anda_bot_tts_{}.mp3", Xid::new()));
        let output_path = output_file
            .to_str()
            .ok_or("Failed to build temp file path for Edge TTS")?;

        let output = tokio::time::timeout(
            TTS_HTTP_TIMEOUT,
            tokio::process::Command::new(&self.binary_path)
                .arg("--text")
                .arg(text)
                .arg("--voice")
                .arg(&self.voice)
                .arg("--write-media")
                .arg(output_path)
                .output(),
        )
        .await
        .map_err(|_| "Edge TTS subprocess timed out")?
        .map_err(|_| "Failed to spawn edge-tts subprocess")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Clean up temp file on failure.
            let _ = tokio::fs::remove_file(&output_file).await;
            return Err(format!("edge-tts failed (exit {}): {}", output.status, stderr).into());
        }

        let bytes = tokio::fs::read(&output_file)
            .await
            .map_err(|_| "Failed to read edge-tts output file")?;

        // Clean up temp file.
        let _ = tokio::fs::remove_file(&output_file).await;

        Ok(bytes)
    }
}
