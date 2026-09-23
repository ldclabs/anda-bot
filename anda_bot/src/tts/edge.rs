use anda_core::BoxError;
use std::time::Duration;

use super::{TTS_HTTP_TIMEOUT, TtsProvider};
use crate::config;

/// Edge TTS provider — free, uses the `edge-tts` CLI subprocess.
pub struct EdgeTtsProvider {
    binary_path: String,
    voice: String,
}

impl EdgeTtsProvider {
    /// Allowed basenames for the Edge TTS binary.
    const ALLOWED_BINARIES: &[&str] = &["edge-tts"];

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

    async fn synthesize_with_timeout(
        &self,
        text: &str,
        timeout: Duration,
    ) -> Result<Vec<u8>, BoxError> {
        let output = tokio::time::timeout(
            timeout,
            tokio::process::Command::new(&self.binary_path)
                .kill_on_drop(true)
                .arg(format!("--text={text}"))
                .arg(format!("--voice={}", self.voice))
                .arg("--write-media")
                .arg("-")
                .output(),
        )
        .await
        .map_err(|_| "Edge TTS subprocess timed out")?
        .map_err(|err| format!("Failed to run edge-tts subprocess: {err}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("edge-tts failed (exit {}): {}", output.status, stderr).into());
        }
        if output.stdout.is_empty() {
            return Err("edge-tts returned empty audio".into());
        }
        Ok(output.stdout)
    }
}

#[async_trait::async_trait]
impl TtsProvider for EdgeTtsProvider {
    fn name(&self) -> &str {
        "edge"
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        self.synthesize_with_timeout(text, TTS_HTTP_TIMEOUT).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge_config(binary_path: &str) -> config::EdgeTtsConfig {
        config::EdgeTtsConfig {
            binary_path: binary_path.to_string(),
            voice: "en-US-AriaNeural".to_string(),
        }
    }

    #[test]
    fn new_rejects_paths_with_separators() {
        for path in ["/tmp/edge-tts", "tools\\edge-tts", "./edge-tts"] {
            let err = EdgeTtsProvider::new(&edge_config(path))
                .map(|_| ())
                .unwrap_err();
            assert!(
                err.to_string().contains("without path separators"),
                "expected separator error for {path:?}, got: {err}"
            );
        }
    }

    #[test]
    fn new_rejects_unknown_binary_names() {
        let err = EdgeTtsProvider::new(&edge_config("malicious-tts"))
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("must be one of"));
    }

    #[test]
    fn new_accepts_allowed_binaries() {
        for path in EdgeTtsProvider::ALLOWED_BINARIES {
            let provider = EdgeTtsProvider::new(&edge_config(path)).unwrap();
            assert_eq!(provider.binary_path, *path);
            assert_eq!(provider.voice, "en-US-AriaNeural");
            assert_eq!(provider.name(), "edge");
        }
    }

    #[test]
    fn rejects_playback_wrapper() {
        assert!(EdgeTtsProvider::new(&edge_config("edge-playback")).is_err());
    }

    #[cfg(unix)]
    fn fake_cli(script: &str) -> (tempfile::TempDir, EdgeTtsProvider) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge-tts");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let provider = EdgeTtsProvider {
            binary_path: path.to_str().unwrap().into(),
            voice: "test-voice".into(),
        };
        (dir, provider)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn synthesis_reads_stdout_and_reports_process_failures() {
        let (dir, provider) = fake_cli(
            r#"
[ "$1" = '--text=-hello' ] && [ "$2" = '--voice=test-voice' ] && [ "$3" = '--write-media' ] && [ "$4" = '-' ] || exit 2
printf 'MP3DATA'
"#,
        );
        assert_eq!(provider.synthesize("-hello").await.unwrap(), b"MP3DATA");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let (_dir, provider) = fake_cli("printf 'service failed' >&2; exit 7");
        let err = provider.synthesize("hi").await.unwrap_err().to_string();
        assert!(err.contains('7') && err.contains("service failed"));
        let (_dir, provider) = fake_cli("exit 0");
        assert!(
            provider
                .synthesize("hi")
                .await
                .unwrap_err()
                .to_string()
                .contains("empty audio")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_and_cancellation_terminate_child() {
        for cancel in [false, true] {
            let (dir, provider) = fake_cli("printf '%s' \"$$\" > \"$0.pid\"\nexec sleep 5");
            let task = tokio::spawn(async move {
                provider
                    .synthesize_with_timeout("hi", Duration::from_secs(1))
                    .await
            });
            let pid_path = dir.path().join("edge-tts.pid");
            let pid: i32 = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if let Ok(value) = tokio::fs::read_to_string(&pid_path).await
                        && let Ok(pid) = value.parse()
                    {
                        break pid;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            if cancel {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            } else {
                assert!(
                    task.await
                        .unwrap()
                        .unwrap_err()
                        .to_string()
                        .contains("timed out")
                );
            }
            let exited = tokio::time::timeout(Duration::from_secs(2), async {
                while unsafe { libc::kill(pid, 0) } == 0 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            if exited.is_err() {
                // Clean up the test-owned process even when the regression fails.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
            assert!(exited.is_ok(), "child survived cancellation/timeout: {pid}");
        }
    }
}
