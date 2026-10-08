//! The retired Anda Bot tray launcher.
//!
//! Anda Desktop now owns the tray. This binary still ships under the old
//! release asset name so older launchers and `anda update` can deliver it.
//! When an old install starts it, it runs `anda install`, which removes the
//! launcher's login entry, app bundle, shortcuts and sidecar and keeps the
//! daemon starting at login; then it starts the daemon, points the user to
//! Anda Desktop once, and exits.
#![cfg_attr(windows, windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

#[path = "../util/locale.rs"]
mod locale;

use rust_i18n::t;
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const DESKTOP_URL: &str = "https://anda.bot";
/// Marks that the one-time retirement notice was shown for this home.
const RETIRED_STAMP: &str = "retired";
/// Spares this process when `anda install` stops launcher instances.
const SPARE_PID_ENV: &str = "ANDA_RETIRING_LAUNCHER_PID";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn main() {
    let home = detect_home(env::args_os().skip(1));
    let anda = detect_anda(&launcher_paths());
    // Keep the anda this launcher ran with; without `--dir`, a custom install
    // directory would get a second copy at the default location.
    let dir = anda
        .parent()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.to_string_lossy().into_owned());
    let mut install = vec!["install"];
    if let Some(dir) = dir.as_deref() {
        install.extend(["--dir", dir]);
    }
    run_anda(&anda, &home, &install);
    run_anda(&anda, &home, &["start"]);
    notify_once(&home);
}

fn detect_home(args: impl IntoIterator<Item = OsString>) -> PathBuf {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--home" {
            if let Some(home) = args.next().filter(|home| !home.is_empty()) {
                return PathBuf::from(home);
            }
        } else if let Some(home) = arg.to_str().and_then(|arg| arg.strip_prefix("--home="))
            && !home.is_empty()
        {
            return PathBuf::from(home);
        }
    }
    if let Some(home) = env::var_os("ANDA_HOME").filter(|home| !home.is_empty()) {
        return PathBuf::from(home);
    }
    env::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".anda")
}

/// Every launcher binary this install may have: the running one and, for the
/// macOS app copy, the sidecar it was copied from.
fn launcher_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(current) = env::current_exe() {
        #[cfg(target_os = "macos")]
        if let Some(sidecar) = app_bundle_of(&current)
            .and_then(|app| fs::read_to_string(app.join("Contents/Resources/LauncherPath")).ok())
            .map(|path| PathBuf::from(path.trim()))
            .filter(|path| !path.as_os_str().is_empty())
        {
            paths.push(sidecar);
        }
        paths.push(current);
    }
    paths
}

#[cfg(target_os = "macos")]
fn app_bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

fn detect_anda(launcher_paths: &[PathBuf]) -> PathBuf {
    let name = if cfg!(windows) { "anda.exe" } else { "anda" };
    let siblings = launcher_paths
        .iter()
        .filter_map(|path| path.parent().map(|dir| dir.join(name)));
    let mut known = Vec::new();
    if let Some(home) = env::home_dir() {
        known.push(home.join(".local/bin/anda"));
    }
    if let Some(base) = env::var_os("LOCALAPPDATA") {
        known.push(PathBuf::from(base).join("Programs\\AndaBot\\anda.exe"));
    }
    known.extend(["/opt/homebrew/bin/anda", "/usr/local/bin/anda"].map(PathBuf::from));
    siblings
        .chain(known)
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn run_anda(anda: &Path, home: &Path, args: &[&str]) {
    let mut command = Command::new(anda);
    command
        .arg("--home")
        .arg(home)
        .args(args)
        .env(SPARE_PID_ENV, std::process::id().to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command.output();
}

fn notify_once(home: &Path) {
    let stamp_dir = home.join("launcher");
    let stamp = stamp_dir.join(RETIRED_STAMP);
    if stamp.exists() {
        return;
    }
    let _ = fs::create_dir_all(&stamp_dir);
    let _ = fs::write(&stamp, env!("CARGO_PKG_VERSION"));
    let locale = locale::ui_locale(home);
    let title = t!("launcher.retired_title", locale = locale);
    let message = t!("launcher.retired_message", locale = locale);
    let download = t!("launcher.retired_download", locale = locale);
    let later = t!("launcher.retired_later", locale = locale);
    if confirm(&title, &message, &download, &later) {
        open_url(DESKTOP_URL);
    }
}

#[cfg(target_os = "macos")]
fn confirm(title: &str, message: &str, accept: &str, decline: &str) -> bool {
    let quote = |value: &str| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!(
        "button returned of (display dialog {} with title {} buttons {{{}, {}}} default button 2 with icon note)",
        quote(message),
        quote(title),
        quote(decline),
        quote(accept),
    );
    Command::new("osascript")
        .args(["-e", &script])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == accept)
}

#[cfg(windows)]
fn confirm(title: &str, message: &str, accept: &str, _decline: &str) -> bool {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONINFORMATION, MB_YESNO, MessageBoxW,
    };

    let wide =
        |value: &str| -> Vec<u16> { OsStr::new(value).encode_wide().chain(Some(0)).collect() };
    let text = format!("{message}\n\n{accept}?");
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(&text).as_ptr(),
            wide(title).as_ptr(),
            MB_YESNO | MB_ICONINFORMATION,
        ) == IDYES
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn confirm(title: &str, message: &str, _accept: &str, _decline: &str) -> bool {
    eprintln!("{title}\n{message}\n{DESKTOP_URL}");
    false
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let _ = Command::new("explorer.exe").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_comes_from_args_then_default() {
        assert_eq!(
            detect_home(["--home", "/tmp/a"].map(OsString::from)),
            PathBuf::from("/tmp/a")
        );
        assert_eq!(
            detect_home(["--home=/tmp/b"].map(OsString::from)),
            PathBuf::from("/tmp/b")
        );
        assert!(
            detect_home(Vec::<OsString>::new()).ends_with(".anda")
                || env::var_os("ANDA_HOME").is_some()
        );
    }

    #[test]
    fn every_notice_string_is_translated() {
        for locale in ["en", "zh-Hans", "ru", "ar", "fr", "es"] {
            for key in [
                "launcher.retired_title",
                "launcher.retired_message",
                "launcher.retired_download",
                "launcher.retired_later",
            ] {
                assert_ne!(t!(key, locale = locale), key, "{locale} {key}");
            }
        }
    }
}
