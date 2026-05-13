use serde::{Deserialize, Serialize};

/// Text-to-Speech configuration (`[tts]`).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TtsConfig {
    /// Enable TTS synthesis.
    pub enabled: bool,
    /// Default TTS provider (`"openai"`, `"google"`, `"edge"`, `"stepfun"`).
    pub default_provider: String,
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
    /// StepFun TTS provider configuration.
    #[serde(default)]
    pub stepfun: Option<StepFunTtsConfig>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: "edge".into(),
            default_format: "mp3".to_string(),
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
