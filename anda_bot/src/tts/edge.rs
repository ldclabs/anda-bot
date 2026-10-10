use anda_core::BoxError;
use std::time::Duration;

use super::{TTS_TIMEOUT, TtsProvider};
use crate::config;

/// The only accepted `binary_path`: a bare command resolved on PATH, so the
/// config cannot point synthesis at an arbitrary executable.
const EDGE_TTS_BINARY: &str = "edge-tts";

/// Edge TTS provider — free, uses the `edge-tts` CLI subprocess.
pub struct EdgeTtsProvider {
    binary_path: String,
    voice: String,
}

impl EdgeTtsProvider {
    pub fn new(config: &config::EdgeTtsConfig) -> Result<Self, BoxError> {
        if config.binary_path != EDGE_TTS_BINARY {
            return Err(format!(
                "Edge TTS binary_path must be `{EDGE_TTS_BINARY}` (resolved on PATH), got: {}",
                config.binary_path
            )
            .into());
        }
        Ok(Self {
            binary_path: EDGE_TTS_BINARY.to_string(),
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
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "`{}` was not found on PATH; install it with `pip install edge-tts`",
                    self.binary_path
                )
            } else {
                format!("Failed to run edge-tts subprocess: {err}")
            }
        })?;

        if !output.status.success() {
            // edge-tts reports failures as a Python traceback whose last line
            // names the error.
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or("no error output");
            return Err(format!("edge-tts failed ({}): {reason}", output.status).into());
        }
        Ok(output.stdout)
    }
}

#[async_trait::async_trait]
impl TtsProvider for EdgeTtsProvider {
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>, BoxError> {
        self.synthesize_with_timeout(text, TTS_TIMEOUT).await
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
    fn new_accepts_only_the_bare_edge_tts_command() {
        let provider = EdgeTtsProvider::new(&edge_config("edge-tts")).unwrap();
        assert_eq!(provider.binary_path, "edge-tts");
        assert_eq!(provider.voice, "en-US-AriaNeural");
        assert_eq!(provider.audio_format(), "mp3");

        for path in [
            "/tmp/edge-tts",
            "tools\\edge-tts",
            "./edge-tts",
            "malicious-tts",
            "edge-playback",
        ] {
            let err = EdgeTtsProvider::new(&edge_config(path))
                .map(|_| ())
                .unwrap_err();
            assert!(
                err.to_string().contains("must be `edge-tts`"),
                "expected rejection for {path:?}, got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn missing_binary_reports_install_hint() {
        let provider = EdgeTtsProvider {
            binary_path: "anda-missing-edge-tts".into(),
            voice: "test-voice".into(),
        };
        let err = provider.synthesize("hi").await.unwrap_err().to_string();
        assert!(err.contains("was not found on PATH"), "got: {err}");
    }

    /// Writes an `edge-tts` stand-in that runs `script`, and launches it once
    /// so the test's own launch cannot fail or stall for reasons of its own.
    #[cfg(unix)]
    fn fake_cli(script: &str) -> (tempfile::TempDir, EdgeTtsProvider) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge-tts");
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = --warm-up ] && exit 0\n{script}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        // On Linux, a process another test forks while the script is open for
        // writing keeps that handle until it execs, and launching the script
        // meanwhile fails with ETXTBSY; once a launch succeeds, no writer is
        // left. On macOS, a new executable's first launch can exceed a test's
        // timeout while the system assesses it.
        let mut busy = 0;
        loop {
            match std::process::Command::new(&path).arg("--warm-up").status() {
                Ok(status) => break assert!(status.success(), "warm-up failed: {status}"),
                Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && busy < 100 => {
                    busy += 1;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("failed to launch {}: {err}", path.display()),
            }
        }
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
        let (_dir, provider) = fake_cli(
            "printf 'Traceback (most recent call last):\\n  File \"x\"\\nNoAudioReceived: service failed\\n\\n' >&2; exit 7",
        );
        let err = provider.synthesize("hi").await.unwrap_err().to_string();
        assert!(err.contains('7'), "got: {err}");
        assert!(
            err.ends_with("NoAudioReceived: service failed"),
            "got: {err}"
        );
        assert!(!err.contains("Traceback"), "got: {err}");
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
