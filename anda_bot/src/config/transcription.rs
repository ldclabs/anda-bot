use serde::{Deserialize, Serialize};

/// Voice transcription configuration with multi-provider support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    /// Enable voice transcription for channels that support it.
    pub enabled: bool,
    /// Default STT provider: "groq", "openai", "google", "stepfun", "local_whisper".
    pub default_provider: String,
    /// Optional initial prompt to bias transcription toward expected vocabulary
    /// (proper nouns, technical terms, etc.). Sent as the `prompt` field in the
    /// Groq/OpenAI Whisper API request.
    pub initial_prompt: Option<String>,
    /// Groq Whisper STT provider configuration.
    pub groq: Option<GroqSttConfig>,
    /// OpenAI Whisper STT provider configuration.
    pub openai: Option<OpenAiSttConfig>,
    /// Google Cloud Speech-to-Text provider configuration.
    pub google: Option<GoogleSttConfig>,
    /// StepFun Stepaudio ASR provider configuration.
    pub stepfun: Option<StepFunSttConfig>,
    /// Local/self-hosted Whisper-compatible STT provider.
    pub local_whisper: Option<LocalWhisperConfig>,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: "groq".into(),
            initial_prompt: None,
            groq: None,
            openai: None,
            google: None,
            stepfun: None,
            local_whisper: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GroqSttConfig {
    /// Groq API key.
    pub api_key: String,
    pub api_url: String,
    pub model: String,
    pub language: Option<String>,
}

impl Default for GroqSttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: "https://api.groq.com/openai/v1/audio/transcriptions".into(),
            model: "whisper-large-v3-turbo".into(),
            language: None,
        }
    }
}

/// OpenAI Whisper STT provider configuration (`[transcription.openai]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAiSttConfig {
    /// OpenAI API key for Whisper transcription.
    pub api_key: String,
    /// Whisper model name (default: "whisper-1").
    pub model: String,
}

impl Default for OpenAiSttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "whisper-1".into(),
        }
    }
}

/// Google Cloud Speech-to-Text provider configuration (`[transcription.google]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleSttConfig {
    /// Google Cloud API key.
    pub api_key: String,
    /// BCP-47 language code (default: "en-US").
    pub language_code: String,
}

impl Default for GoogleSttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            language_code: "en-US".into(),
        }
    }
}

/// StepFun Stepaudio ASR provider configuration (`[transcription.stepfun]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StepFunSttConfig {
    /// StepFun API key.
    pub api_key: String,
    /// StepFun HTTP+SSE ASR endpoint.
    pub api_url: String,
    /// ASR model name (default: "stepaudio-2.5-asr").
    pub model: String,
    /// Recognition language (default: "zh").
    pub language: String,
    /// Hotwords to bias recognition.
    pub hotwords: Vec<String>,
    /// Optional transcription prompt. StepFun documents this as effective for
    /// `stepaudio-2-asr-pro`.
    pub prompt: Option<String>,
    /// Whether to enable inverse text normalization.
    pub enable_itn: bool,
    /// PCM codec when transcribing raw `.pcm` audio.
    pub pcm_codec: String,
    /// PCM sample rate when transcribing raw `.pcm` audio.
    pub pcm_rate: u32,
    /// PCM bit depth when transcribing raw `.pcm` audio.
    pub pcm_bits: u32,
    /// PCM channel count when transcribing raw `.pcm` audio.
    pub pcm_channel: u32,
}

impl Default for StepFunSttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: "https://api.stepfun.com/v1/audio/asr/sse".into(),
            model: "stepaudio-2.5-asr".into(),
            language: "zh".into(),
            hotwords: Vec::new(),
            prompt: None,
            enable_itn: true,
            pcm_codec: "pcm_s16le".into(),
            pcm_rate: 16000,
            pcm_bits: 16,
            pcm_channel: 1,
        }
    }
}

/// Local/self-hosted Whisper-compatible STT endpoint (`[transcription.local_whisper]`).
///
/// Configures a self-hosted STT endpoint. Can be on localhost, a private network host, or any reachable URL.
/// `url` is required, so this struct keeps per-field defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalWhisperConfig {
    /// HTTP or HTTPS endpoint URL, e.g. `"http://10.10.0.1:8001/v1/transcribe"`.
    pub url: String,
    /// Bearer token for endpoint authentication.
    /// Omit for unauthenticated local endpoints.
    #[serde(default)]
    pub bearer_token: Option<String>,
    /// Maximum audio file size in bytes accepted by this endpoint.
    /// Defaults to 25 MB — matching the cloud API cap for a safe out-of-the-box
    /// experience. Self-hosted endpoints can accept much larger files; raise this
    /// as needed, but note that each transcription call clones the audio buffer
    /// into a multipart payload, so peak memory per request is ~2× this value.
    #[serde(default = "default_local_whisper_max_audio_bytes")]
    pub max_audio_bytes: usize,
    /// Request timeout in seconds. Defaults to 300 (large files on local GPU).
    #[serde(default = "default_local_whisper_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for LocalWhisperConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            bearer_token: None,
            max_audio_bytes: default_local_whisper_max_audio_bytes(),
            timeout_secs: default_local_whisper_timeout_secs(),
        }
    }
}

fn default_local_whisper_max_audio_bytes() -> usize {
    25 * 1024 * 1024
}

fn default_local_whisper_timeout_secs() -> u64 {
    300
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_transcription_config_uses_groq() {
        let config = TranscriptionConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.default_provider, "groq");
        assert_eq!(config.initial_prompt, None);
        assert!(config.groq.is_none());
        assert!(config.openai.is_none());
        assert!(config.google.is_none());
        assert!(config.stepfun.is_none());
        assert!(config.local_whisper.is_none());
    }

    #[test]
    fn partial_transcription_config_deserializes_with_defaults() {
        let config: TranscriptionConfig = serde_json::from_value(json!({
            "enabled": true,
            "initial_prompt": "project names"
        }))
        .unwrap();

        assert!(config.enabled);
        assert_eq!(config.default_provider, "groq");
        assert_eq!(config.initial_prompt.as_deref(), Some("project names"));
    }

    #[test]
    fn stt_provider_defaults_match_expected_models() {
        let groq: GroqSttConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(
            groq.api_url,
            "https://api.groq.com/openai/v1/audio/transcriptions"
        );
        assert_eq!(groq.model, "whisper-large-v3-turbo");
        assert_eq!(groq.language, None);

        let openai: OpenAiSttConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(openai.api_key, "");
        assert_eq!(openai.model, "whisper-1");

        let google: GoogleSttConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(google.api_key, "");
        assert_eq!(google.language_code, "en-US");

        // `Default` and serde agree, so `..Default::default()` builds the same
        // provider a config file with that section left empty would.
        assert_eq!(GroqSttConfig::default().api_url, groq.api_url);
        assert_eq!(GroqSttConfig::default().model, groq.model);
        assert_eq!(OpenAiSttConfig::default().model, openai.model);
        assert_eq!(
            GoogleSttConfig::default().language_code,
            google.language_code
        );
    }

    #[test]
    fn stepfun_and_local_whisper_defaults_are_complete() {
        let stepfun = StepFunSttConfig::default();
        assert_eq!(stepfun.api_url, "https://api.stepfun.com/v1/audio/asr/sse");
        assert_eq!(stepfun.model, "stepaudio-2.5-asr");
        assert_eq!(stepfun.language, "zh");
        assert!(stepfun.hotwords.is_empty());
        assert_eq!(stepfun.prompt, None);
        assert!(stepfun.enable_itn);
        assert_eq!(stepfun.pcm_codec, "pcm_s16le");
        assert_eq!(stepfun.pcm_rate, 16000);
        assert_eq!(stepfun.pcm_bits, 16);
        assert_eq!(stepfun.pcm_channel, 1);

        let local: LocalWhisperConfig = serde_json::from_value(json!({
            "url": "http://127.0.0.1:8001/v1/transcribe"
        }))
        .unwrap();
        assert_eq!(local.url, "http://127.0.0.1:8001/v1/transcribe");
        assert_eq!(local.bearer_token, None);
        assert_eq!(local.max_audio_bytes, 25 * 1024 * 1024);
        assert_eq!(local.timeout_secs, 300);
        assert_eq!(
            LocalWhisperConfig::default().max_audio_bytes,
            local.max_audio_bytes
        );
        assert_eq!(
            LocalWhisperConfig::default().timeout_secs,
            local.timeout_secs
        );
        assert!(serde_json::from_value::<LocalWhisperConfig>(json!({})).is_err());
    }

    #[test]
    fn removed_settings_are_ignored_when_loading_old_config() {
        let config: TranscriptionConfig = serde_json::from_value(json!({
            "max_duration_secs": 120,
            "transcribe_non_ptt_audio": true,
            "groq": {"api_key": "test", "language_code": "en-US", "language": "en"}
        }))
        .unwrap();
        let serialized = serde_json::to_value(config).unwrap();
        assert!(serialized.get("max_duration_secs").is_none());
        assert!(serialized.get("transcribe_non_ptt_audio").is_none());
        assert!(serialized["groq"].get("language_code").is_none());
        assert_eq!(serialized["groq"]["language"], "en");
    }
}
