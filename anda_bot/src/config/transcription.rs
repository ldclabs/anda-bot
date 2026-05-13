use serde::{Deserialize, Serialize};

/// Voice transcription configuration with multi-provider support.
///
/// The top-level `api_url`, `model`, and `api_key` fields remain for backward
/// compatibility with existing Groq-based configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    /// Enable voice transcription for channels that support it.
    #[serde(default)]
    pub enabled: bool,
    /// Default STT provider: "groq", "openai", "google", "stepfun", "local_whisper".
    #[serde(default = "default_transcription_provider")]
    pub default_provider: String,
    /// Optional initial prompt to bias transcription toward expected vocabulary
    /// (proper nouns, technical terms, etc.). Sent as the `prompt` field in the
    /// Whisper API request.
    #[serde(default)]
    pub initial_prompt: Option<String>,
    /// Maximum voice duration in seconds (messages longer than this are skipped).
    #[serde(default = "default_transcription_max_duration_secs")]
    pub max_duration_secs: u64,
    /// Groq Whisper STT provider configuration.
    #[serde(default)]
    pub groq: Option<GroqSttConfig>,
    /// OpenAI Whisper STT provider configuration.
    #[serde(default)]
    pub openai: Option<OpenAiSttConfig>,
    /// Google Cloud Speech-to-Text provider configuration.
    #[serde(default)]
    pub google: Option<GoogleSttConfig>,
    /// StepFun Stepaudio ASR provider configuration.
    #[serde(default)]
    pub stepfun: Option<StepFunSttConfig>,
    /// Local/self-hosted Whisper-compatible STT provider.
    #[serde(default)]
    pub local_whisper: Option<LocalWhisperConfig>,
    /// Also transcribe non-PTT (forwarded/regular) audio messages on WhatsApp,
    /// not just voice notes.  Default: `false` (preserves legacy behavior).
    #[serde(default)]
    pub transcribe_non_ptt_audio: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_provider: default_transcription_provider(),
            initial_prompt: None,
            max_duration_secs: default_transcription_max_duration_secs(),
            groq: None,
            openai: None,
            google: None,
            stepfun: None,
            local_whisper: None,
            transcribe_non_ptt_audio: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroqSttConfig {
    /// Groq API key.
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_transcription_api_url")]
    pub api_url: String,
    #[serde(default = "default_transcription_model")]
    pub model: String,
    pub language: Option<String>,
    /// BCP-47 language code (default: "en-US").
    #[serde(default = "default_google_stt_language_code")]
    pub language_code: String,
}

/// OpenAI Whisper STT provider configuration (`[transcription.openai]`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAiSttConfig {
    /// OpenAI API key for Whisper transcription.
    #[serde(default)]
    pub api_key: String,
    /// Whisper model name (default: "whisper-1").
    #[serde(default = "default_openai_stt_model")]
    pub model: String,
}

/// Google Cloud Speech-to-Text provider configuration (`[transcription.google]`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoogleSttConfig {
    /// Google Cloud API key.
    #[serde(default)]
    pub api_key: String,
    /// BCP-47 language code (default: "en-US").
    #[serde(default = "default_google_stt_language_code")]
    pub language_code: String,
}

/// StepFun Stepaudio ASR provider configuration (`[transcription.stepfun]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepFunSttConfig {
    /// StepFun API key.
    #[serde(default)]
    pub api_key: String,
    /// StepFun HTTP+SSE ASR endpoint.
    #[serde(default = "default_stepfun_stt_api_url")]
    pub api_url: String,
    /// ASR model name (default: "stepaudio-2.5-asr").
    #[serde(default = "default_stepfun_stt_model")]
    pub model: String,
    /// Recognition language (default: "zh").
    #[serde(default = "default_stepfun_stt_language")]
    pub language: String,
    /// Hotwords to bias recognition.
    #[serde(default)]
    pub hotwords: Vec<String>,
    /// Optional transcription prompt. StepFun documents this as effective for
    /// `stepaudio-2-asr-pro`.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Whether to enable inverse text normalization.
    #[serde(default = "default_stepfun_enable_itn")]
    pub enable_itn: bool,
    /// PCM codec when transcribing raw `.pcm` audio.
    #[serde(default = "default_stepfun_pcm_codec")]
    pub pcm_codec: String,
    /// PCM sample rate when transcribing raw `.pcm` audio.
    #[serde(default = "default_stepfun_pcm_rate")]
    pub pcm_rate: u32,
    /// PCM bit depth when transcribing raw `.pcm` audio.
    #[serde(default = "default_stepfun_pcm_bits")]
    pub pcm_bits: u32,
    /// PCM channel count when transcribing raw `.pcm` audio.
    #[serde(default = "default_stepfun_pcm_channel")]
    pub pcm_channel: u32,
}

impl Default for StepFunSttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: default_stepfun_stt_api_url(),
            model: default_stepfun_stt_model(),
            language: default_stepfun_stt_language(),
            hotwords: Vec::new(),
            prompt: None,
            enable_itn: default_stepfun_enable_itn(),
            pcm_codec: default_stepfun_pcm_codec(),
            pcm_rate: default_stepfun_pcm_rate(),
            pcm_bits: default_stepfun_pcm_bits(),
            pcm_channel: default_stepfun_pcm_channel(),
        }
    }
}

/// Local/self-hosted Whisper-compatible STT endpoint (`[transcription.local_whisper]`).
///
/// Configures a self-hosted STT endpoint. Can be on localhost, a private network host, or any reachable URL.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

fn default_local_whisper_max_audio_bytes() -> usize {
    25 * 1024 * 1024
}

fn default_local_whisper_timeout_secs() -> u64 {
    300
}

fn default_transcription_api_url() -> String {
    "https://api.groq.com/openai/v1/audio/transcriptions".into()
}

fn default_transcription_model() -> String {
    "whisper-large-v3-turbo".into()
}

fn default_transcription_max_duration_secs() -> u64 {
    120
}

fn default_transcription_provider() -> String {
    "groq".into()
}

fn default_stepfun_stt_api_url() -> String {
    "https://api.stepfun.com/v1/audio/asr/sse".into()
}

fn default_stepfun_stt_model() -> String {
    "stepaudio-2.5-asr".into()
}

fn default_stepfun_stt_language() -> String {
    "zh".into()
}

fn default_stepfun_enable_itn() -> bool {
    true
}

fn default_stepfun_pcm_codec() -> String {
    "pcm_s16le".into()
}

fn default_stepfun_pcm_rate() -> u32 {
    16000
}

fn default_stepfun_pcm_bits() -> u32 {
    16
}

fn default_stepfun_pcm_channel() -> u32 {
    1
}

fn default_openai_stt_model() -> String {
    "whisper-1".into()
}

fn default_google_stt_language_code() -> String {
    "en-US".into()
}
