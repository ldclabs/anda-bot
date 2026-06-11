use anda_core::{BoxError, ByteBufB64, FunctionDefinition, Resource, Tool, ToolOutput};
use anda_engine::context::BaseCtx;
use ic_auth_types::Xid;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::config;

mod edge;
mod google;
mod openai;
mod stepfun;

pub use edge::EdgeTtsProvider;
pub use google::GoogleTtsProvider;
pub use openai::OpenAiTtsProvider;
pub use stepfun::StepFunTtsProvider;

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

    /// Audio format returned by this provider.
    fn audio_format(&self) -> &str {
        "mp3"
    }

    /// Audio format names this provider can currently return through this manager.
    fn supported_audio_formats(&self) -> Vec<String> {
        vec![normalize_audio_format(self.audio_format()).to_string()]
    }

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

        if let Some(ref stepfun_cfg) = config.stepfun {
            match StepFunTtsProvider::new(stepfun_cfg, &config.default_format, http.clone()) {
                Ok(p) => {
                    providers.insert(p.name().to_string(), Box::new(p));
                }
                Err(e) => {
                    log::warn!("Skipping StepFun TTS provider: {e}");
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
        self.providers
            .get(&self.default_provider)
            .map(|provider| normalize_audio_format(provider.audio_format()))
            .unwrap_or_else(|| normalize_audio_format(&self.default_format))
    }

    pub fn supported_audio_formats(&self) -> Vec<String> {
        self.providers
            .get(&self.default_provider)
            .map(|provider| provider.supported_audio_formats())
            .unwrap_or_default()
    }

    #[allow(unused)]
    pub fn audio_mime_type(&self) -> &'static str {
        mime_for_audio_format(self.audio_format())
    }

    pub fn audio_artifact(&self, bytes: Vec<u8>, name: Option<String>) -> Resource {
        audio_artifact_with_format(bytes, name, self.audio_format())
    }

    pub fn audio_artifact_for_provider(
        &self,
        provider: &str,
        bytes: Vec<u8>,
        name: Option<String>,
    ) -> Result<Resource, BoxError> {
        let tts = self.providers.get(provider).ok_or_else(|| {
            format!(
                "TTS provider '{}' not configured (available: {})",
                provider,
                self.available_providers().join(", ")
            )
        })?;
        Ok(audio_artifact_with_format(bytes, name, tts.audio_format()))
    }
}

fn audio_artifact_with_format(bytes: Vec<u8>, name: Option<String>, format: &str) -> Resource {
    let format = normalize_audio_format(format);
    let name = normalize_artifact_name(name, format);
    let size = bytes.len() as u64;
    Resource {
        tags: vec!["audio".to_string(), format.to_string()],
        name,
        description: Some("Synthesized speech from anda_bot".to_string()),
        mime_type: Some(mime_for_audio_format(format).to_string()),
        blob: Some(ByteBufB64(bytes)),
        size: Some(size),
        ..Default::default()
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
                        "description": "Text to synthesize into speech."
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
                "required": ["text", "provider", "artifact_name"],
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
        let artifact = self.audio_artifact_for_provider(&provider, bytes, args.artifact_name)?;
        let format = artifact
            .tags
            .iter()
            .find(|tag| tag.as_str() != "audio")
            .cloned()
            .unwrap_or_else(|| self.audio_format().to_string());
        let output = TtsOutput {
            provider,
            artifact: artifact.name.clone(),
            mime_type: artifact.mime_type.clone().unwrap_or_default(),
            format,
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
        "flac" => "flac",
        "pcm" => "pcm",
        _ => "mp3",
    }
}

fn mime_for_audio_format(format: &str) -> &'static str {
    match format {
        "wav" => "audio/wav",
        "opus" => "audio/opus",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pcm" => "audio/pcm",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json_schema::assert_openai_strict_parameters;

    struct StaticTtsProvider {
        name: &'static str,
        format: &'static str,
    }

    #[async_trait::async_trait]
    impl TtsProvider for StaticTtsProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn audio_format(&self) -> &str {
            self.format
        }

        async fn synthesize(&self, _text: &str) -> Result<Vec<u8>, BoxError> {
            Ok(vec![1, 2, 3])
        }
    }

    fn manager_with_provider(provider: StaticTtsProvider, default_format: &str) -> TtsManager {
        let default_provider = provider.name.to_string();
        let mut providers: HashMap<String, Box<dyn TtsProvider>> = HashMap::new();
        providers.insert(default_provider.clone(), Box::new(provider));
        TtsManager {
            providers,
            default_provider,
            default_format: default_format.to_string(),
            max_text_length: DEFAULT_MAX_TEXT_LENGTH,
        }
    }

    #[test]
    fn tts_tool_schema_is_openai_strict() {
        let manager = manager_with_provider(
            StaticTtsProvider {
                name: "edge",
                format: "mp3",
            },
            "mp3",
        );
        let definition = manager.definition();

        assert_eq!(definition.strict, Some(true));
        assert_openai_strict_parameters(&definition.parameters);
    }

    #[test]
    fn audio_artifact_uses_default_provider_actual_format() {
        let manager = manager_with_provider(
            StaticTtsProvider {
                name: "edge",
                format: "mp3",
            },
            "wav",
        );

        let artifact = manager.audio_artifact(vec![1, 2, 3], Some("voice".to_string()));

        assert_eq!(artifact.name, "voice.mp3");
        assert_eq!(artifact.tags, vec!["audio", "mp3"]);
        assert_eq!(artifact.mime_type.as_deref(), Some("audio/mpeg"));
    }

    #[test]
    fn audio_artifact_for_provider_uses_requested_provider_format() {
        let manager = manager_with_provider(
            StaticTtsProvider {
                name: "stepfun",
                format: "wav",
            },
            "mp3",
        );

        let artifact = manager
            .audio_artifact_for_provider("stepfun", vec![1, 2, 3], Some("voice".to_string()))
            .unwrap();

        assert_eq!(artifact.name, "voice.wav");
        assert_eq!(artifact.tags, vec!["audio", "wav"]);
        assert_eq!(artifact.mime_type.as_deref(), Some("audio/wav"));
    }

    use anda_engine::engine::EngineBuilder;

    fn empty_manager(default_provider: &str, default_format: &str) -> TtsManager {
        TtsManager {
            providers: HashMap::new(),
            default_provider: default_provider.to_string(),
            default_format: default_format.to_string(),
            max_text_length: DEFAULT_MAX_TEXT_LENGTH,
        }
    }

    #[test]
    fn manager_disabled_config_registers_no_providers() {
        let manager =
            TtsManager::new(&config::TtsConfig::default(), reqwest::Client::new()).unwrap();

        assert!(!manager.is_enabled());
        assert!(manager.available_providers().is_empty());
        assert!(manager.supported_audio_formats().is_empty());
    }

    #[test]
    fn manager_zero_max_text_length_falls_back_to_default() {
        let config = config::TtsConfig {
            max_text_length: 0,
            ..Default::default()
        };
        let manager = TtsManager::new(&config, reqwest::Client::new()).unwrap();

        assert_eq!(manager.max_text_length, DEFAULT_MAX_TEXT_LENGTH);
    }

    #[test]
    fn manager_registers_valid_providers_and_skips_invalid_ones() {
        let config = config::TtsConfig {
            enabled: true,
            default_provider: "edge".to_string(),
            // Empty API keys: these providers are skipped with a warning.
            openai: Some(config::OpenAiTtsConfig::default()),
            google: Some(config::GoogleTtsConfig::default()),
            edge: Some(config::EdgeTtsConfig::default()),
            stepfun: Some(config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let manager = TtsManager::new(&config, reqwest::Client::new()).unwrap();

        assert!(manager.is_enabled());
        assert_eq!(manager.available_providers(), vec!["edge", "stepfun"]);
    }

    #[test]
    fn manager_rejects_unavailable_default_provider() {
        let config = config::TtsConfig {
            enabled: true,
            default_provider: "openai".to_string(),
            openai: Some(config::OpenAiTtsConfig::default()),
            ..Default::default()
        };

        let err = TtsManager::new(&config, reqwest::Client::new())
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Default TTS provider 'openai'"));
    }

    #[tokio::test]
    async fn synthesize_validates_text_and_provider() {
        let manager = manager_with_provider(
            StaticTtsProvider {
                name: "edge",
                format: "mp3",
            },
            "mp3",
        );

        assert_eq!(manager.synthesize("hello").await.unwrap(), vec![1, 2, 3]);

        let err = manager.synthesize("").await.unwrap_err();
        assert!(err.to_string().contains("must not be empty"));

        let err = manager
            .synthesize_with_provider("hello", "missing")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("TTS provider 'missing'"));

        let mut short_manager = manager_with_provider(
            StaticTtsProvider {
                name: "edge",
                format: "mp3",
            },
            "mp3",
        );
        short_manager.max_text_length = 3;
        let err = short_manager.synthesize("hello").await.unwrap_err();
        assert!(err.to_string().contains("TTS text too long"));
    }

    #[test]
    fn audio_format_falls_back_to_default_format_without_provider() {
        let manager = manager_with_provider(
            StaticTtsProvider {
                name: "edge",
                format: "WAV",
            },
            "mp3",
        );
        assert_eq!(manager.audio_format(), "wav");
        assert_eq!(manager.audio_mime_type(), "audio/wav");
        assert_eq!(manager.supported_audio_formats(), vec!["wav"]);

        let empty = empty_manager("edge", "opus");
        assert_eq!(empty.audio_format(), "opus");
        assert!(empty.supported_audio_formats().is_empty());
        let err = empty
            .audio_artifact_for_provider("edge", vec![1], None)
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("TTS provider 'edge'"));
    }

    #[test]
    fn normalize_audio_format_maps_known_formats_and_defaults_to_mp3() {
        assert_eq!(normalize_audio_format(" WAV "), "wav");
        assert_eq!(normalize_audio_format("opus"), "opus");
        assert_eq!(normalize_audio_format("ogg"), "ogg");
        assert_eq!(normalize_audio_format("flac"), "flac");
        assert_eq!(normalize_audio_format("pcm"), "pcm");
        assert_eq!(normalize_audio_format("anything"), "mp3");

        assert_eq!(mime_for_audio_format("ogg"), "audio/ogg");
        assert_eq!(mime_for_audio_format("pcm"), "audio/pcm");
        assert_eq!(mime_for_audio_format("mp3"), "audio/mpeg");
    }

    #[test]
    fn normalize_artifact_name_appends_extension_when_missing() {
        assert_eq!(
            normalize_artifact_name(Some("voice".to_string()), "mp3"),
            "voice.mp3"
        );
        assert_eq!(
            normalize_artifact_name(Some("voice.ogg".to_string()), "mp3"),
            "voice.ogg"
        );

        let fallback = normalize_artifact_name(None, "wav");
        assert!(fallback.starts_with("anda_bot_tts_"));
        assert!(fallback.ends_with(".wav"));

        let blank = normalize_artifact_name(Some("  ".to_string()), "wav");
        assert!(blank.starts_with("anda_bot_tts_"));
    }

    #[tokio::test]
    async fn tts_tool_call_returns_artifact_and_metadata() {
        let manager = manager_with_provider(
            StaticTtsProvider {
                name: "edge",
                format: "mp3",
            },
            "mp3",
        );
        let ctx = EngineBuilder::new().mock_ctx().base;

        let result = manager
            .call(
                ctx.clone(),
                TtsArgs {
                    text: "hello".to_string(),
                    provider: None,
                    artifact_name: Some("greeting".to_string()),
                },
                Vec::new(),
            )
            .await
            .unwrap();

        assert_eq!(result.output.provider, "edge");
        assert_eq!(result.output.artifact, "greeting.mp3");
        assert_eq!(result.output.mime_type, "audio/mpeg");
        assert_eq!(result.output.format, "mp3");
        assert_eq!(result.output.size, 3);
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "greeting.mp3");

        let err = manager
            .call(
                ctx,
                TtsArgs {
                    text: "hello".to_string(),
                    provider: Some("missing".to_string()),
                    artifact_name: None,
                },
                Vec::new(),
            )
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("TTS provider 'missing'"));
    }
}
