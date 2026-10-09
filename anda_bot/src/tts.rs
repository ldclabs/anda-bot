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

use edge::EdgeTtsProvider;
use google::GoogleTtsProvider;
use openai::OpenAiTtsProvider;
use stepfun::StepFunTtsProvider;

/// Maximum text length before synthesis is rejected (default: 4096 chars).
const DEFAULT_MAX_TEXT_LENGTH: usize = 4096;

/// Timeout for one synthesis: an HTTP request or an `edge-tts` run.
const TTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

// ── TtsProvider trait ────────────────────────────────────────────

/// Trait for pluggable TTS backends.
#[async_trait::async_trait]
pub trait TtsProvider: Send + Sync {
    /// Canonical lowercase audio format returned by this provider.
    fn audio_format(&self) -> &'static str {
        "mp3"
    }

    /// Synthesize `text`, returning raw audio bytes.
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError>;
}

// ── TtsManager ───────────────────────────────────────────────────

/// Central manager for multi-provider TTS synthesis.
pub struct TtsManager {
    providers: HashMap<String, Box<dyn TtsProvider>>,
    /// Always a key of `providers`.
    default_provider: String,
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

    /// Build a `TtsManager` from config; `None` when TTS is disabled.
    ///
    /// An invalid default provider fails with its own error; other invalid
    /// providers are skipped with a warning.
    pub fn new(
        config: &config::TtsConfig,
        http: reqwest::Client,
    ) -> Result<Option<Self>, BoxError> {
        if !config.enabled {
            return Ok(None);
        }

        let default_format = parse_audio_format(&config.default_format)?;
        let default_provider = config.default_provider.clone();
        let mut providers = HashMap::new();
        if let Some(cfg) = &config.openai {
            let provider = OpenAiTtsProvider::new(cfg, http.clone());
            register(&mut providers, &default_provider, "openai", provider)?;
        }
        if let Some(cfg) = &config.google {
            let provider = GoogleTtsProvider::new(cfg, http.clone());
            register(&mut providers, &default_provider, "google", provider)?;
        }
        if let Some(cfg) = &config.edge {
            let provider = EdgeTtsProvider::new(cfg);
            register(&mut providers, &default_provider, "edge", provider)?;
        }
        if let Some(cfg) = &config.stepfun {
            let provider = StepFunTtsProvider::new(cfg, default_format, http);
            register(&mut providers, &default_provider, "stepfun", provider)?;
        }

        let manager = Self {
            providers,
            default_provider,
            max_text_length: if config.max_text_length == 0 {
                DEFAULT_MAX_TEXT_LENGTH
            } else {
                config.max_text_length
            },
        };
        if !manager.providers.contains_key(&manager.default_provider) {
            return Err(format!(
                "Default TTS provider '{}' is not configured (available: {})",
                manager.default_provider,
                manager.available_providers().join(", ")
            )
            .into());
        }
        Ok(Some(manager))
    }

    /// Synthesize text using the default provider.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        let (audio, _) = self.synthesize_with(&self.default_provider, text).await?;
        Ok(audio)
    }

    /// Synthesize text using `provider`, returning the audio and its format.
    async fn synthesize_with(
        &self,
        provider: &str,
        text: &str,
    ) -> Result<(Vec<u8>, &'static str), BoxError> {
        let tts = self.providers.get(provider).ok_or_else(|| {
            format!(
                "TTS provider '{provider}' not configured (available: {})",
                self.available_providers().join(", ")
            )
        })?;
        if text.trim().is_empty() {
            return Err("TTS text must not be empty".into());
        }
        let char_count = text.chars().count();
        if char_count > self.max_text_length {
            return Err(format!(
                "TTS text too long ({char_count} chars, max {})",
                self.max_text_length
            )
            .into());
        }

        let audio = tts.synthesize(text).await?;
        if audio.is_empty() {
            return Err(format!("TTS provider '{provider}' returned empty audio").into());
        }
        Ok((audio, tts.audio_format()))
    }

    /// List names of all initialized providers.
    pub fn available_providers(&self) -> Vec<String> {
        let mut names: Vec<_> = self.providers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Audio format of the default provider.
    pub fn audio_format(&self) -> &'static str {
        self.providers[&self.default_provider].audio_format()
    }

    pub fn supported_audio_formats(&self) -> Vec<String> {
        vec![self.audio_format().to_string()]
    }

    /// Wraps audio from [`Self::synthesize`] as an artifact.
    pub fn audio_artifact(&self, bytes: Vec<u8>, name: Option<String>) -> Resource {
        audio_artifact_with_format(bytes, name, self.audio_format())
    }
}

fn register<P: TtsProvider + 'static>(
    providers: &mut HashMap<String, Box<dyn TtsProvider>>,
    default_provider: &str,
    name: &str,
    provider: Result<P, BoxError>,
) -> Result<(), BoxError> {
    match provider {
        Ok(provider) => {
            providers.insert(name.to_string(), Box::new(provider));
        }
        Err(err) if name == default_provider => {
            return Err(format!("Default TTS provider '{name}' is invalid: {err}").into());
        }
        Err(err) => log::warn!("Skipping {name} TTS provider: {err}"),
    }
    Ok(())
}

fn audio_artifact_with_format(bytes: Vec<u8>, name: Option<String>, format: &str) -> Resource {
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
        format!(
            "Convert text into speech audio. Returns the synthesized audio as an artifact resource that callers can play or attach. Available providers: {} (default: {}). Text is limited to {} characters and some providers accept less, so split long text across calls.",
            self.available_providers().join(", "),
            self.default_provider,
            self.max_text_length
        )
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
                        "description": "Optional output artifact file name. Its extension is replaced with the actual audio format."
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
        let (bytes, format) = self.synthesize_with(&provider, &args.text).await?;
        let artifact = audio_artifact_with_format(bytes, args.artifact_name, format);
        let output = TtsOutput {
            provider,
            artifact: artifact.name.clone(),
            mime_type: mime_for_audio_format(format).to_string(),
            format: format.to_string(),
            size: artifact.size.unwrap_or_default(),
        };
        let mut result = ToolOutput::new(output);
        result.artifacts.push(artifact);
        Ok(result)
    }
}

/// Parses `tts.default_format`. Raw PCM has no header, so no Anda client can
/// play it; WAV carries the same samples.
fn parse_audio_format(format: &str) -> Result<&'static str, BoxError> {
    match format.trim().to_ascii_lowercase().as_str() {
        "mp3" => Ok("mp3"),
        "wav" => Ok("wav"),
        "opus" => Ok("opus"),
        "flac" => Ok("flac"),
        "pcm" => Err("TTS audio format 'pcm' cannot be played; use 'wav' instead".into()),
        "ogg" => Err("TTS audio format 'ogg' is not supported; use 'opus' instead".into()),
        _ => Err(format!("Unsupported TTS audio format '{format}'").into()),
    }
}

fn mime_for_audio_format(format: &str) -> &'static str {
    match format {
        "wav" => "audio/wav",
        "opus" => "audio/opus",
        "flac" => "audio/flac",
        _ => "audio/mpeg",
    }
}

fn normalize_artifact_name(name: Option<String>, format: &str) -> String {
    let Some(name) = name.and_then(|value| config::normalize_string(&value)) else {
        return format!("anda_bot_tts_{}.{}", Xid::new(), format);
    };
    let mut path = std::path::PathBuf::from(name);
    path.set_extension(format);
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;
    use crate::util::json_schema::assert_openai_strict_parameters;
    use anda_engine::engine::EngineBuilder;

    struct StaticTtsProvider {
        format: &'static str,
        audio: Vec<u8>,
    }

    #[async_trait::async_trait]
    impl TtsProvider for StaticTtsProvider {
        fn audio_format(&self) -> &'static str {
            self.format
        }

        async fn synthesize(&self, _text: &str) -> Result<Vec<u8>, BoxError> {
            Ok(self.audio.clone())
        }
    }

    fn manager_with_provider(name: &str, format: &'static str) -> TtsManager {
        let mut providers: HashMap<String, Box<dyn TtsProvider>> = HashMap::new();
        providers.insert(
            name.to_string(),
            Box::new(StaticTtsProvider {
                format,
                audio: vec![1, 2, 3],
            }),
        );
        TtsManager {
            providers,
            default_provider: name.to_string(),
            max_text_length: DEFAULT_MAX_TEXT_LENGTH,
        }
    }

    fn edge_config() -> config::TtsConfig {
        config::TtsConfig {
            enabled: true,
            default_provider: "edge".to_string(),
            edge: Some(config::EdgeTtsConfig::default()),
            ..Default::default()
        }
    }

    fn new_manager_error(config: config::TtsConfig) -> String {
        TtsManager::new(&config, new_reqwest_client())
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn tts_tool_schema_is_openai_strict() {
        let definition = manager_with_provider("edge", "mp3").definition();

        assert_eq!(definition.strict, Some(true));
        assert_openai_strict_parameters(&definition.parameters);
        assert!(
            definition
                .description
                .contains("Available providers: edge (default: edge)")
        );
        assert!(definition.description.contains("4096 characters"));
    }

    #[test]
    fn audio_artifact_uses_default_provider_actual_format() {
        let manager = manager_with_provider("stepfun", "wav");
        assert_eq!(manager.audio_format(), "wav");
        assert_eq!(manager.supported_audio_formats(), vec!["wav"]);

        let artifact = manager.audio_artifact(vec![1, 2, 3], Some("voice".to_string()));

        assert_eq!(artifact.name, "voice.wav");
        assert_eq!(artifact.tags, vec!["audio", "wav"]);
        assert_eq!(artifact.mime_type.as_deref(), Some("audio/wav"));
        assert_eq!(artifact.size, Some(3));
    }

    #[test]
    fn manager_disabled_config_builds_nothing() {
        let manager = TtsManager::new(&config::TtsConfig::default(), new_reqwest_client());
        assert!(manager.unwrap().is_none());

        // Disabled sections are not validated.
        let config = config::TtsConfig {
            default_format: "typo".into(),
            ..Default::default()
        };
        assert!(
            TtsManager::new(&config, new_reqwest_client())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn manager_zero_max_text_length_falls_back_to_default() {
        let config = config::TtsConfig {
            max_text_length: 0,
            ..edge_config()
        };
        let manager = TtsManager::new(&config, new_reqwest_client())
            .unwrap()
            .unwrap();

        assert_eq!(manager.max_text_length, DEFAULT_MAX_TEXT_LENGTH);
    }

    #[test]
    fn manager_registers_valid_providers_and_skips_invalid_ones() {
        let config = config::TtsConfig {
            // Empty API keys: these providers are skipped with a warning.
            openai: Some(config::OpenAiTtsConfig::default()),
            google: Some(config::GoogleTtsConfig::default()),
            stepfun: Some(config::StepFunTtsConfig {
                api_key: "sk-test".to_string(),
                ..Default::default()
            }),
            ..edge_config()
        };

        let manager = TtsManager::new(&config, new_reqwest_client())
            .unwrap()
            .unwrap();

        assert_eq!(manager.available_providers(), vec!["edge", "stepfun"]);
        assert_eq!(manager.audio_format(), "mp3");
    }

    #[test]
    fn manager_reports_why_the_default_provider_is_unavailable() {
        let err = new_manager_error(config::TtsConfig {
            default_provider: "openai".to_string(),
            openai: Some(config::OpenAiTtsConfig::default()),
            ..edge_config()
        });
        assert!(
            err.contains(
                "Default TTS provider 'openai' is invalid: OpenAI TTS API key must not be empty"
            ),
            "got: {err}"
        );

        let err = new_manager_error(config::TtsConfig {
            default_provider: "stepfun".to_string(),
            stepfun: Some(config::StepFunTtsConfig::default()),
            ..edge_config()
        });
        assert!(err.contains("Missing StepFun TTS API key"), "got: {err}");

        let err = new_manager_error(config::TtsConfig {
            default_provider: "google".to_string(),
            ..edge_config()
        });
        assert!(
            err.contains("Default TTS provider 'google' is not configured (available: edge)"),
            "got: {err}"
        );
    }

    #[test]
    fn enabled_manager_rejects_unplayable_or_unknown_output_formats() {
        for (format, message) in [
            ("pcm", "use 'wav'"),
            ("ogg", "use 'opus'"),
            ("typo", "Unsupported TTS audio format 'typo'"),
        ] {
            let err = new_manager_error(config::TtsConfig {
                default_format: format.into(),
                ..edge_config()
            });
            assert!(err.contains(message), "{format}: {err}");
        }
    }

    #[tokio::test]
    async fn synthesize_validates_text_provider_and_audio() {
        let mut manager = manager_with_provider("edge", "mp3");

        assert_eq!(manager.synthesize("hello").await.unwrap(), vec![1, 2, 3]);

        for text in ["", " \n\t"] {
            let err = manager.synthesize(text).await.unwrap_err();
            assert!(err.to_string().contains("must not be empty"));
        }

        let err = manager
            .synthesize_with("missing", "hello")
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("TTS provider 'missing' not configured (available: edge)")
        );

        manager.max_text_length = 3;
        let err = manager.synthesize("hello").await.unwrap_err();
        assert!(
            err.to_string()
                .contains("TTS text too long (5 chars, max 3)")
        );

        manager.providers.insert(
            "edge".into(),
            Box::new(StaticTtsProvider {
                format: "mp3",
                audio: Vec::new(),
            }),
        );
        let err = manager.synthesize("hi").await.unwrap_err();
        assert!(
            err.to_string()
                .contains("TTS provider 'edge' returned empty audio")
        );
    }

    #[test]
    fn parse_audio_format_accepts_playable_formats() {
        for format in ["mp3", "wav", "opus", "flac"] {
            assert_eq!(parse_audio_format(format).unwrap(), format);
        }
        assert_eq!(parse_audio_format(" WAV ").unwrap(), "wav");

        assert_eq!(mime_for_audio_format("mp3"), "audio/mpeg");
        assert_eq!(mime_for_audio_format("wav"), "audio/wav");
        assert_eq!(mime_for_audio_format("opus"), "audio/opus");
        assert_eq!(mime_for_audio_format("flac"), "audio/flac");
    }

    #[test]
    fn normalize_artifact_name_uses_actual_format() {
        assert_eq!(
            normalize_artifact_name(Some("voice".to_string()), "mp3"),
            "voice.mp3"
        );
        assert_eq!(
            normalize_artifact_name(Some("voice.ogg".to_string()), "mp3"),
            "voice.mp3"
        );

        let fallback = normalize_artifact_name(None, "wav");
        assert!(fallback.starts_with("anda_bot_tts_"));
        assert!(fallback.ends_with(".wav"));

        let blank = normalize_artifact_name(Some("  ".to_string()), "wav");
        assert!(blank.starts_with("anda_bot_tts_"));
    }

    #[tokio::test]
    async fn tts_tool_call_returns_artifact_in_the_requested_providers_format() {
        let mut manager = manager_with_provider("edge", "mp3");
        manager.providers.insert(
            "stepfun".into(),
            Box::new(StaticTtsProvider {
                format: "wav",
                audio: vec![4, 5],
            }),
        );
        let ctx = EngineBuilder::new().mock_ctx().base;

        let result = manager
            .call(
                ctx.clone(),
                TtsArgs {
                    text: "hello".to_string(),
                    provider: None,
                    artifact_name: Some("greeting.wav".to_string()),
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
        assert_eq!(result.artifacts[0].tags, vec!["audio", "mp3"]);

        let result = manager
            .call(
                ctx.clone(),
                TtsArgs {
                    text: "hello".to_string(),
                    provider: Some(" stepfun ".to_string()),
                    artifact_name: Some("greeting".to_string()),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.output.provider, "stepfun");
        assert_eq!(result.output.artifact, "greeting.wav");
        assert_eq!(result.output.mime_type, "audio/wav");
        assert_eq!(result.output.format, "wav");
        assert_eq!(result.output.size, 2);

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
