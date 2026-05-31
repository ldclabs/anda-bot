use serde::{Deserialize, Serialize};

/// Text-to-Speech configuration (`[tts]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TtsConfig {
    /// Enable TTS synthesis.
    #[serde(default)]
    pub enabled: bool,
    /// Default TTS provider (`"openai"`, `"google"`, `"edge"`, `"stepfun"`).
    #[serde(default = "default_tts_provider")]
    pub default_provider: String,
    /// Default audio output format (`"mp3"`, `"opus"`, `"wav"`).
    #[serde(default = "default_tts_format")]
    pub default_format: String,
    /// Maximum input text length in characters (default 4096).
    #[serde(default = "default_tts_max_text_length")]
    pub max_text_length: usize,
    /// OpenAI TTS provider configuration.
    #[serde(default)]
    pub openai: Option<OpenAiTtsConfig>,
    /// Google Cloud TTS provider configuration.
    #[serde(default)]
    pub google: Option<GoogleTtsConfig>,
    /// Edge TTS provider configuration.
    #[serde(default)]
    pub edge: Option<EdgeTtsConfig>,
    /// StepFun TTS provider configuration.
    #[serde(default)]
    pub stepfun: Option<StepFunTtsConfig>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: default_tts_provider(),
            default_format: default_tts_format(),
            max_text_length: default_tts_max_text_length(),
            openai: None,
            google: None,
            edge: None,
            stepfun: None,
        }
    }
}

/// StepFun TTS provider configuration (`[tts.stepfun]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StepFunTtsConfig {
    /// StepFun API key.
    #[serde(default)]
    pub api_key: String,
    /// StepFun TTS endpoint.
    #[serde(default = "default_stepfun_tts_api_url")]
    pub api_url: String,
    /// TTS model name (default `"step-tts-mini"`).
    #[serde(default = "default_stepfun_tts_model")]
    pub model: String,
    /// Voice ID, either an official voice or a generated custom voice.
    #[serde(default = "default_stepfun_tts_voice")]
    pub voice: String,
    /// Playback speed multiplier, from 0.5 to 2.0.
    #[serde(default = "default_stepfun_tts_speed")]
    pub speed: f64,
    /// Output volume multiplier, from 0.1 to 2.0.
    #[serde(default = "default_stepfun_tts_volume")]
    pub volume: f64,
    /// Optional global natural-language instruction for `stepaudio-2.5-tts`.
    #[serde(default)]
    pub instruction: Option<String>,
    /// Audio sample rate. StepFun supports 8000, 16000, 22050, 24000, and 48000.
    #[serde(default = "default_stepfun_tts_sample_rate")]
    pub sample_rate: u32,
    /// Optional pronunciation replacement map.
    #[serde(default)]
    pub pronunciation_map: StepFunTtsPronunciationMap,
    /// Whether StepFun should filter Markdown before synthesis.
    #[serde(default)]
    pub markdown_filter: Option<bool>,
}

impl Default for StepFunTtsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: default_stepfun_tts_api_url(),
            model: default_stepfun_tts_model(),
            voice: default_stepfun_tts_voice(),
            speed: default_stepfun_tts_speed(),
            volume: default_stepfun_tts_volume(),
            instruction: None,
            sample_rate: default_stepfun_tts_sample_rate(),
            pronunciation_map: StepFunTtsPronunciationMap::default(),
            markdown_filter: None,
        }
    }
}

/// StepFun pronunciation map. Each `tone` entry uses `source/replacement` syntax.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StepFunTtsPronunciationMap {
    #[serde(default)]
    pub tone: Vec<String>,
}

/// OpenAI TTS provider configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenAiTtsConfig {
    /// API key for OpenAI TTS.
    #[serde(default)]
    pub api_key: String,
    /// Model name (default `"tts-1"`).
    #[serde(default = "default_openai_tts_model")]
    pub model: String,
    /// Playback speed multiplier (default `1.0`).
    #[serde(default = "default_openai_tts_speed")]
    pub speed: f64,
    /// Voice ID (default `"alloy"`).
    #[serde(default = "default_openai_tts_voice")]
    pub voice: String,
}

impl Default for OpenAiTtsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_openai_tts_model(),
            speed: default_openai_tts_speed(),
            voice: default_openai_tts_voice(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GoogleTtsConfig {
    /// API key for Google Cloud TTS.
    #[serde(default)]
    pub api_key: String,
    /// Language code (default `"en-US"`).
    #[serde(default = "default_google_tts_language_code")]
    pub language_code: String,
    /// Voice ID (default `"en-US-Standard-A"`).
    #[serde(default = "default_google_tts_voice")]
    pub voice: String,
}

impl Default for GoogleTtsConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            language_code: default_google_tts_language_code(),
            voice: default_google_tts_voice(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EdgeTtsConfig {
    /// Path to the `edge-tts` binary (default `"edge-tts"`).
    #[serde(default = "default_edge_tts_binary_path")]
    pub binary_path: String,
    /// Voice ID (default `"en-US-AriaNeural"`).
    #[serde(default = "default_edge_tts_voice")]
    pub voice: String,
}

impl Default for EdgeTtsConfig {
    fn default() -> Self {
        Self {
            binary_path: default_edge_tts_binary_path(),
            voice: default_edge_tts_voice(),
        }
    }
}

fn default_tts_provider() -> String {
    "edge".into()
}

fn default_tts_format() -> String {
    "mp3".into()
}

fn default_tts_max_text_length() -> usize {
    4096
}

fn default_stepfun_tts_api_url() -> String {
    "https://api.stepfun.com/v1/audio/speech".into()
}

fn default_stepfun_tts_model() -> String {
    "stepaudio-2.5-tts".into()
}

fn default_stepfun_tts_voice() -> String {
    "ruyananshi".into()
}

fn default_stepfun_tts_speed() -> f64 {
    1.0
}

fn default_stepfun_tts_volume() -> f64 {
    1.0
}

fn default_stepfun_tts_sample_rate() -> u32 {
    24000
}

fn default_openai_tts_model() -> String {
    "tts-1".into()
}

fn default_openai_tts_speed() -> f64 {
    1.0
}

fn default_openai_tts_voice() -> String {
    "alloy".into()
}

fn default_google_tts_language_code() -> String {
    "en-US".into()
}

fn default_google_tts_voice() -> String {
    "en-US-Standard-A".into()
}

fn default_edge_tts_binary_path() -> String {
    "edge-tts".into()
}

fn default_edge_tts_voice() -> String {
    "en-US-AriaNeural".into()
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
