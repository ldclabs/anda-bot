use serde::{Deserialize, Serialize};

/// Text-to-Speech configuration (`[tts]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct TtsConfig {
    /// Enable TTS synthesis.
    pub enabled: bool,
    /// Default TTS provider (`"openai"`, `"google"`, `"edge"`, `"stepfun"`).
    pub default_provider: String,
    /// StepFun audio output format (`"mp3"`, `"opus"`, `"wav"`, `"flac"`, `"pcm"`).
    /// Edge, OpenAI, and Google always return MP3.
    pub default_format: String,
    /// Maximum input text length in characters (default 4096).
    pub max_text_length: usize,
    /// OpenAI TTS provider configuration.
    pub openai: Option<OpenAiTtsConfig>,
    /// Google Cloud TTS provider configuration.
    pub google: Option<GoogleTtsConfig>,
    /// Edge TTS provider configuration.
    pub edge: Option<EdgeTtsConfig>,
    /// StepFun TTS provider configuration.
    pub stepfun: Option<StepFunTtsConfig>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: "edge".into(),
            default_format: "mp3".into(),
            max_text_length: 4096,
            openai: None,
            google: None,
            edge: None,
            stepfun: None,
        }
    }
}

/// StepFun TTS provider configuration (`[tts.stepfun]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct StepFunTtsConfig {
    /// StepFun API key.
    pub api_key: String,
    /// StepFun TTS endpoint.
    pub api_url: String,
    /// TTS model name (default `"stepaudio-2.5-tts"`).
    pub model: String,
    /// Voice ID, either an official voice or a generated custom voice.
    pub voice: String,
    /// Playback speed multiplier, from 0.5 to 2.0.
    pub speed: f64,
    /// Output volume multiplier, from 0.1 to 2.0.
    pub volume: f64,
    /// Optional global natural-language instruction for `stepaudio-2.5-tts`.
    pub instruction: Option<String>,
    /// Audio sample rate. StepFun supports 8000, 16000, 22050, 24000, and 48000.
    pub sample_rate: u32,
    /// Optional pronunciation replacement map.
    pub pronunciation_map: StepFunTtsPronunciationMap,
    /// Whether StepFun should filter Markdown before synthesis.
    pub markdown_filter: Option<bool>,
}

impl Default for StepFunTtsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: "https://api.stepfun.com/v1/audio/speech".into(),
            model: "stepaudio-2.5-tts".into(),
            voice: "ruyananshi".into(),
            speed: 1.0,
            volume: 1.0,
            instruction: None,
            sample_rate: 24000,
            pronunciation_map: StepFunTtsPronunciationMap::default(),
            markdown_filter: None,
        }
    }
}

/// StepFun pronunciation map. Each `tone` entry uses `source/replacement` syntax.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StepFunTtsPronunciationMap {
    pub tone: Vec<String>,
}

/// OpenAI TTS provider configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenAiTtsConfig {
    /// API key for OpenAI TTS.
    pub api_key: String,
    /// Model name (default `"tts-1"`).
    pub model: String,
    /// Playback speed multiplier (default `1.0`).
    pub speed: f64,
    /// Voice ID (default `"alloy"`).
    pub voice: String,
}

impl Default for OpenAiTtsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "tts-1".into(),
            speed: 1.0,
            voice: "alloy".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct GoogleTtsConfig {
    /// API key for Google Cloud TTS.
    pub api_key: String,
    /// Language code (default `"en-US"`).
    pub language_code: String,
    /// Voice ID (default `"en-US-Standard-A"`).
    pub voice: String,
}

impl Default for GoogleTtsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            language_code: "en-US".into(),
            voice: "en-US-Standard-A".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct EdgeTtsConfig {
    /// Command name: `"edge-tts"` (must be available on PATH).
    pub binary_path: String,
    /// Voice ID (default `"en-US-AriaNeural"`).
    pub voice: String,
}

impl Default for EdgeTtsConfig {
    fn default() -> Self {
        Self {
            binary_path: "edge-tts".into(),
            voice: "en-US-AriaNeural".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_tts_config_uses_edge_mp3_limits() {
        let config = TtsConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.default_provider, "edge");
        assert_eq!(config.default_format, "mp3");
        assert_eq!(config.max_text_length, 4096);
        assert!(config.openai.is_none());
        assert!(config.google.is_none());
        assert!(config.edge.is_none());
        assert!(config.stepfun.is_none());
    }

    #[test]
    fn partial_tts_config_deserializes_with_defaults() {
        let config: TtsConfig = serde_json::from_value(json!({ "enabled": true })).unwrap();

        assert!(config.enabled);
        assert_eq!(config.default_provider, "edge");
        assert_eq!(config.default_format, "mp3");
        assert_eq!(config.max_text_length, 4096);
    }

    #[test]
    fn provider_configs_deserialize_partial_values_with_defaults() {
        let openai: OpenAiTtsConfig = serde_json::from_value(json!({
            "api_key": "sk-test"
        }))
        .unwrap();
        assert_eq!(openai.api_key, "sk-test");
        assert_eq!(openai.model, "tts-1");
        assert_eq!(openai.speed, 1.0);
        assert_eq!(openai.voice, "alloy");

        let google: GoogleTtsConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(google.api_key, "");
        assert_eq!(google.language_code, "en-US");
        assert_eq!(google.voice, "en-US-Standard-A");

        let edge: EdgeTtsConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(edge.binary_path, "edge-tts");
        assert_eq!(edge.voice, "en-US-AriaNeural");
    }

    #[test]
    fn stepfun_tts_default_matches_documented_endpoint() {
        let config = StepFunTtsConfig::default();

        assert_eq!(config.api_url, "https://api.stepfun.com/v1/audio/speech");
        assert_eq!(config.model, "stepaudio-2.5-tts");
        assert_eq!(config.voice, "ruyananshi");
        assert_eq!(config.speed, 1.0);
        assert_eq!(config.volume, 1.0);
        assert_eq!(config.sample_rate, 24000);
        assert!(config.pronunciation_map.tone.is_empty());
        assert!(config.markdown_filter.is_none());
    }
}
