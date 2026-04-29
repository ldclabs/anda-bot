use serde::{Deserialize, Serialize};

/// Text-to-Speech configuration (`[tts]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TtsConfig {
    /// Enable TTS synthesis.
    pub enabled: bool,
    /// Default TTS provider (`"openai"`, `"google"`, `"edge"`).
    pub default_provider: String,
    /// Default voice ID passed to the selected provider.
    pub default_voice: String,
    /// Default audio output format (`"mp3"`, `"opus"`, `"wav"`).
    pub default_format: String,
    /// Maximum input text length in characters (default 4096).
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
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: "edge".into(),
            default_voice: "alloy".into(),
            default_format: "mp3".to_string(),
            max_text_length: 4096,
            openai: None,
            google: None,
            edge: None,
        }
    }
}

/// OpenAI TTS provider configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
pub struct EdgeTtsConfig {
    /// Path to the `edge-tts` binary (default `"edge-tts"`).
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
