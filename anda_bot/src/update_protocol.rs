//! Completion handoff for Windows executable replacement. Both binaries use
//! the same paths; detached helpers must never inherit the launcher's pipes.

use std::{
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub const PENDING: &str = "pending";
pub const INSTALLED: &str = "installed";
pub const COMPLETION_TIMEOUT: Duration = Duration::from_secs(70);
pub const LAUNCHER_PID_ENV: &str = "ANDA_LAUNCHER_UPDATE_PID";

pub fn completion_path(executable: &Path) -> PathBuf {
    let mut name = executable.as_os_str().to_os_string();
    name.push(".update-status");
    PathBuf::from(name)
}

pub fn clear_completion(executable: &Path) -> io::Result<()> {
    match fs::remove_file(completion_path(executable)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

pub fn wait_for_completion(executable: &Path, timeout: Duration) -> io::Result<()> {
    let path = completion_path(executable);
    let deadline = Instant::now() + timeout;
    loop {
        let result = match fs::read_to_string(&path) {
            Ok(result) => result,
            // An up-to-date binary or an unavailable optional sidecar does not
            // schedule a replacement. Callers clear old markers before update.
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        match result.trim().trim_start_matches('\u{feff}') {
            INSTALLED => return Ok(()),
            PENDING if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
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
    Complete-Update 'installed'
    exit 0
  }} catch {{
    if ((Get-Date) -ge $deadline) {{
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

#[cfg(test)]
mod tests {
    use super::*;

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
        clear_completion(&executable).unwrap();
        assert!(!completion_path(&executable).exists());
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
        assert!(
            script.find("Wait-Process -Id 1234").unwrap() < script.find("$deadline =").unwrap()
        );
    }
}
