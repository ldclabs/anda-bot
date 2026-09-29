//! Removes the retired tray launcher (`anda_launcher`, "Anda Bot") from an
//! install: its login entry, app bundle, shortcuts, sidecar binary and running
//! instances. Anda Desktop owns the tray now; `anda install` runs this so a
//! desktop install, a script reinstall and the transitional launcher binary
//! all converge on one tray. Every step is idempotent.

use std::path::Path;
#[cfg(any(target_os = "macos", windows))]
use std::{path::PathBuf, process::Command};

/// Set by the transitional launcher to its own pid, so it is not stopped
/// while it runs `anda install`.
pub(crate) const SPARE_PID_ENV: &str = "ANDA_RETIRING_LAUNCHER_PID";

#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "ai.anda.anda-bot.launcher";
#[cfg(target_os = "macos")]
const LAUNCHER_APP_NAME: &str = "Anda Bot.app";
#[cfg(windows)]
const RUN_VALUE: &str = "AndaBotLauncher";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Retirement {
    /// Some launcher entry point was found and removed.
    pub retired: bool,
    /// The launcher was registered to start at login.
    pub started_at_login: bool,
}

/// Retires the launcher that shipped next to an `anda` in `install_dir`.
pub(crate) fn retire(install_dir: &Path) -> Retirement {
    #[cfg(target_os = "macos")]
    {
        match std::env::home_dir() {
            Some(user_home) => retire_macos(&user_home, install_dir, stop_launchers),
            None => Retirement::default(),
        }
    }
    #[cfg(windows)]
    {
        retire_windows(install_dir)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = install_dir;
        Retirement::default()
    }
}

#[cfg(target_os = "macos")]
fn retire_macos(user_home: &Path, install_dir: &Path, stop_running: fn(&[&str])) -> Retirement {
    let plist = user_home
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let app = user_home.join("Applications").join(LAUNCHER_APP_NAME);
    let mut sidecars = vec![install_dir.join(super::updater::LAUNCHER_BINARY_NAME)];
    // The app bundle records the sidecar it copies itself from.
    if let Ok(path) = std::fs::read_to_string(app.join("Contents/Resources/LauncherPath")) {
        let path = PathBuf::from(path.trim());
        if !path.as_os_str().is_empty() {
            sidecars.push(path);
        }
    }
    let sidecars: Vec<PathBuf> = sidecars.into_iter().filter(|path| path.is_file()).collect();
    let started_at_login = plist.exists();
    if !started_at_login && !app.exists() && sidecars.is_empty() {
        return Retirement::default();
    }

    stop_running(&["Anda Bot", super::updater::LAUNCHER_BINARY_NAME]);
    // Deleting the plist is enough: the loaded job has no KeepAlive, so it
    // is not started again and is not loaded at the next login.
    let _ = std::fs::remove_file(&plist);
    let _ = std::fs::remove_dir_all(&app);
    for sidecar in sidecars {
        // Homebrew removes its own copy when the formula drops the launcher.
        if !super::updater::is_homebrew_managed(&sidecar) {
            let _ = std::fs::remove_file(sidecar);
        }
    }
    Retirement {
        retired: true,
        started_at_login,
    }
}

/// Stops launcher processes, sparing this process and the launcher that asked
/// for the retirement.
#[cfg(target_os = "macos")]
fn stop_launchers(names: &[&str]) {
    let spared = spared_pids();
    for name in names {
        let Ok(output) = Command::new("pgrep").args(["-x", name]).output() else {
            continue;
        };
        for pid in String::from_utf8_lossy(&output.stdout).split_whitespace() {
            if pid.parse::<u32>().is_ok_and(|pid| !spared.contains(&pid)) {
                let _ = Command::new("kill").arg(pid).output();
            }
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn spared_pids() -> Vec<u32> {
    let mut pids = vec![std::process::id()];
    if let Some(pid) = std::env::var(SPARE_PID_ENV)
        .ok()
        .and_then(|pid| pid.parse().ok())
    {
        pids.push(pid);
    }
    pids
}

#[cfg(windows)]
fn retire_windows(install_dir: &Path) -> Retirement {
    let sidecar = install_dir.join(format!("{}.exe", super::updater::LAUNCHER_BINARY_NAME));
    let started_at_login = crate::util::windows_run_key::get(RUN_VALUE).is_some();
    let shortcuts: Vec<PathBuf> = launcher_shortcuts()
        .into_iter()
        .filter(|path| path.exists())
        .collect();
    if !started_at_login && shortcuts.is_empty() && !sidecar.exists() {
        return Retirement::default();
    }

    let mut kill = Command::new("taskkill.exe");
    kill.args(["/IM", "anda_launcher.exe", "/F"]);
    for pid in spared_pids() {
        kill.arg("/FI").arg(format!("PID ne {pid}"));
    }
    crate::util::windows_process::suppress_console_window(&mut kill);
    let _ = kill.output();

    let _ = crate::util::windows_run_key::delete(RUN_VALUE);
    for shortcut in shortcuts {
        let _ = std::fs::remove_file(shortcut);
    }
    // Fails while the transitional launcher itself runs; it is harmless then,
    // because nothing starts it any more.
    let _ = std::fs::remove_file(&sidecar);
    Retirement {
        retired: true,
        started_at_login,
    }
}

/// Start Menu and desktop shortcuts the launcher installers created.
#[cfg(windows)]
fn launcher_shortcuts() -> Vec<PathBuf> {
    let mut shortcuts = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        shortcuts.push(
            PathBuf::from(appdata)
                .join("Microsoft\\Windows\\Start Menu\\Programs\\Anda Bot\\Anda Bot.lnk"),
        );
    }
    for base in ["USERPROFILE", "OneDrive"] {
        if let Some(dir) = std::env::var_os(base) {
            shortcuts.push(PathBuf::from(dir).join("Desktop\\Anda Bot.lnk"));
        }
    }
    shortcuts
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    // Tests must never stop a launcher running on the developer's machine.
    fn keep_processes(_: &[&str]) {}

    #[test]
    fn retiring_a_clean_install_is_a_no_op() {
        let user_home = tempfile::tempdir().unwrap();
        let install_dir = user_home.path().join(".local/bin");
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::write(install_dir.join("anda"), "anda").unwrap();
        assert_eq!(
            retire_macos(user_home.path(), &install_dir, keep_processes),
            Retirement::default()
        );
        assert!(install_dir.join("anda").exists());
    }

    #[test]
    fn retires_login_entry_app_bundle_and_sidecars() {
        let user_home = tempfile::tempdir().unwrap();
        let install_dir = user_home.path().join(".local/bin");
        let agents = user_home.path().join("Library/LaunchAgents");
        let app = user_home.path().join("Applications/Anda Bot.app");
        let other = user_home.path().join("custom/anda_launcher");
        for dir in [
            &install_dir,
            &agents,
            &app.join("Contents/Resources"),
            &other.parent().unwrap().to_path_buf(),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(install_dir.join("anda"), "anda").unwrap();
        std::fs::write(install_dir.join("anda_launcher"), "launcher").unwrap();
        std::fs::write(&other, "launcher").unwrap();
        std::fs::write(
            app.join("Contents/Resources/LauncherPath"),
            format!("{}\n", other.display()),
        )
        .unwrap();
        std::fs::write(agents.join("ai.anda.anda-bot.launcher.plist"), "plist").unwrap();
        // The daemon's own login entry is not the launcher's.
        std::fs::write(agents.join("ai.anda.anda-bot.plist"), "plist").unwrap();

        let retirement = retire_macos(user_home.path(), &install_dir, keep_processes);
        assert_eq!(
            retirement,
            Retirement {
                retired: true,
                started_at_login: true
            }
        );
        assert!(!agents.join("ai.anda.anda-bot.launcher.plist").exists());
        assert!(agents.join("ai.anda.anda-bot.plist").exists());
        assert!(!app.exists());
        assert!(!install_dir.join("anda_launcher").exists());
        assert!(!other.exists());
        assert!(install_dir.join("anda").exists());
    }
}
