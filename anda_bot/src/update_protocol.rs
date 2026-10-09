//! Completion handoff for Windows executable replacement. `anda update` writes
//! the status next to the executable and a detached helper replaces it after
//! the updater exits. Anda Desktop (`applyRuntimeUpdate` in
//! `desktop/src/main/daemon-client.ts`) or an older launcher waits for the
//! status; detached helpers must never inherit the caller's pipes.

use std::path::{Path, PathBuf};

pub const PENDING: &str = "pending";
pub const INSTALLED: &str = "installed";
/// Set by older launchers so the helper waits for them to exit.
#[cfg(windows)]
pub const LAUNCHER_PID_ENV: &str = "ANDA_LAUNCHER_UPDATE_PID";

pub fn completion_path(executable: &Path) -> PathBuf {
    let mut name = executable.as_os_str().to_os_string();
    name.push(".update-status");
    PathBuf::from(name)
}

pub fn windows_install_script(source: &Path, target: &Path, updater_pid: Option<u32>) -> String {
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "''");
    let wait = updater_pid
        .map(|pid| format!("Wait-Process -Id {pid} -ErrorAction SilentlyContinue"))
        .unwrap_or_default();
    format!(
        r#"$ErrorActionPreference = 'Stop'
$source = '{source}'
$target = '{target}'
$status = '{status}'
function Complete-Update([string]$result) {{
  [System.IO.File]::WriteAllText($status + '.tmp', $result)
  Move-Item -Force -LiteralPath ($status + '.tmp') -Destination $status
}}
# Wait for the executable owner (updater or launcher) before starting the
# replacement deadline. Downloads and daemon restarts can take longer.
{wait}
$deadline = (Get-Date).AddSeconds(60)
while ($true) {{
  try {{
    Move-Item -Force -LiteralPath $source -Destination $target
    Complete-Update '{INSTALLED}'
    exit 0
  }} catch {{
    if ((Get-Date) -ge $deadline) {{
      # The staged copy is useless now; do not leave it next to the target.
      Remove-Item -Force -LiteralPath $source -ErrorAction SilentlyContinue
      Complete-Update ('Could not replace ' + $target + ': ' + $_.Exception.Message)
      exit 1
    }}
    Start-Sleep -Milliseconds 100
  }}
}}
"#,
        source = quote(source),
        target = quote(target),
        status = quote(&completion_path(target))
    )
}

/// How long callers wait for the helper; its own deadline is 60 seconds.
#[cfg(test)]
pub const COMPLETION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(70);

/// Waits for the helper's result, as Anda Desktop does. Only tests drive this
/// from Rust.
#[cfg(test)]
pub fn wait_for_completion(executable: &Path, timeout: std::time::Duration) -> std::io::Result<()> {
    use std::{io, time::Instant};

    let path = completion_path(executable);
    let deadline = Instant::now() + timeout;
    loop {
        let result = match std::fs::read_to_string(&path) {
            Ok(result) => result,
            // An up-to-date binary or an unavailable optional sidecar does not
            // schedule a replacement. Callers clear old markers before update.
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        match result.trim().trim_start_matches('\u{feff}') {
            INSTALLED => return Ok(()),
            PENDING if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100))
            }
            PENDING => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timed out replacing {}", executable.display()),
                ));
            }
            error => return Err(io::Error::other(error.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io, thread, time::Duration};

    #[test]
    fn completion_distinguishes_skipped_installed_failed_and_pending() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("anda.exe");
        let path = completion_path(&executable);
        assert!(wait_for_completion(&executable, Duration::ZERO).is_ok());
        fs::write(&path, INSTALLED).unwrap();
        assert!(wait_for_completion(&executable, Duration::ZERO).is_ok());
        fs::write(&path, "access denied").unwrap();
        assert!(
            wait_for_completion(&executable, Duration::ZERO)
                .unwrap_err()
                .to_string()
                .contains("access denied")
        );
        fs::write(&path, PENDING).unwrap();
        assert_eq!(
            wait_for_completion(&executable, Duration::ZERO)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, INSTALLED).unwrap();
            fs::rename(tmp, path).unwrap();
        });
        wait_for_completion(&executable, COMPLETION_TIMEOUT).unwrap();
        writer.join().unwrap();
    }

    #[test]
    fn helper_quotes_paths_and_waits_for_updater_before_replacement() {
        let script = windows_install_script(
            Path::new("C:/it's/new.exe"),
            Path::new("C:/Anda Bot/anda.exe"),
            Some(1234),
        );
        assert!(script.contains("$source = 'C:/it''s/new.exe'"));
        assert!(script.contains("$status = 'C:/Anda Bot/anda.exe.update-status'"));
        assert!(script.contains("Complete-Update 'installed'"));
        assert!(
            script.find("Wait-Process -Id 1234").unwrap() < script.find("$deadline =").unwrap()
        );
    }

    #[test]
    fn helper_removes_the_staged_copy_when_replacement_fails() {
        let script =
            windows_install_script(Path::new("C:/new.exe"), Path::new("C:/anda.exe"), None);
        let failure = script.find("Could not replace").unwrap();
        let cleanup = script
            .find("Remove-Item -Force -LiteralPath $source")
            .unwrap();
        assert!(cleanup < failure);
    }
}
