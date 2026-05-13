use anda_core::BoxError;
use ic_auth_types::Xid;

use super::{TTS_HTTP_TIMEOUT, TtsProvider};
use crate::config;

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
            let _ = tokio::fs::remove_file(&output_file).await;
            return Err(format!("edge-tts failed (exit {}): {}", output.status, stderr).into());
        }

        let bytes = tokio::fs::read(&output_file)
            .await
            .map_err(|_| "Failed to read edge-tts output file")?;

        let _ = tokio::fs::remove_file(&output_file).await;

        Ok(bytes)
    }
}
