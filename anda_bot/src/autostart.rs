#[cfg(any(windows, test))]
use crate::util::windows_process::quote_windows_arg;
use anda_core::BoxError;
use clap::Subcommand;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(windows)]
const RUN_VALUE: &str = "AndaBot";
#[cfg(windows)]
const LEGACY_TASK_NAME: &str = "Anda Bot";
#[cfg(any(target_os = "macos", test))]
const MACOS_LAUNCH_AGENT_LABEL: &str = "ai.anda.anda-bot";
#[cfg(target_os = "linux")]
const LINUX_SYSTEMD_SERVICE: &str = "anda-bot.service";
#[cfg(target_os = "linux")]
const LINUX_DESKTOP_FILE: &str = "anda-bot.desktop";
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
const UNSUPPORTED: &str = "anda autostart is not supported on this platform";

#[derive(Subcommand)]
pub enum AutostartCommand {
    /// Register Anda to start when the current user logs in.
    Install,
    /// Remove the current user's Anda startup registration. A running daemon
    /// keeps running.
    Uninstall,
    /// Show whether the current user's Anda startup registration exists.
    Status,
}

/// Registers `exe` (an installed `anda`) to start the daemon at login.
pub fn install(exe: &Path, home: &Path) -> Result<(), BoxError> {
    let home = absolute_home(home)?;
    let home = home.as_path();
    #[cfg(windows)]
    {
        install_windows(exe, home)
    }

    #[cfg(target_os = "macos")]
    {
        install_macos(exe, home)
    }

    #[cfg(target_os = "linux")]
    {
        install_linux(exe, home)
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (exe, home);
        Err(UNSUPPORTED.into())
    }
}

/// Login services have a different working directory than this CLI.
/// `canonicalize` would give Windows paths the `\\?\` prefix, which cmd.exe
/// cannot use as a working directory.
fn absolute_home(home: &Path) -> Result<PathBuf, BoxError> {
    Ok(std::path::absolute(home)?)
}

/// Removes the login registration only; it does not stop a daemon that the
/// registration already started.
pub fn uninstall() -> Result<(), BoxError> {
    #[cfg(windows)]
    {
        uninstall_windows()
    }

    #[cfg(target_os = "macos")]
    {
        uninstall_macos()
    }

    #[cfg(target_os = "linux")]
    {
        uninstall_linux()
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(UNSUPPORTED.into())
    }
}

/// Whether the current user's login registration exists.
pub fn status() -> Result<bool, BoxError> {
    #[cfg(windows)]
    {
        Ok(crate::util::windows_run_key::get(RUN_VALUE).is_some())
    }

    #[cfg(target_os = "macos")]
    {
        Ok(macos_launch_agent_path()?.exists())
    }

    #[cfg(target_os = "linux")]
    {
        Ok(linux_systemd_is_enabled() || linux_xdg_desktop_path()?.exists())
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(UNSUPPORTED.into())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_dir() -> Result<PathBuf, BoxError> {
    std::env::home_dir().ok_or_else(|| "could not detect current user's home directory".into())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_file_if_exists(path: &Path) -> Result<(), BoxError> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

#[cfg(windows)]
fn install_windows(exe: &Path, home: &Path) -> Result<(), BoxError> {
    // A per-user Run value needs no elevation, unlike a logon scheduled task.
    // `start` launches the daemon detached without a console and then exits.
    crate::util::windows_run_key::set(RUN_VALUE, &run_command_line(exe, home))?;
    delete_legacy_task();
    Ok(())
}

#[cfg(windows)]
fn uninstall_windows() -> Result<(), BoxError> {
    crate::util::windows_run_key::delete(RUN_VALUE)?;
    delete_legacy_task();
    Ok(())
}

/// Removes the logon scheduled task that older releases registered.
#[cfg(windows)]
fn delete_legacy_task() {
    let mut command = Command::new("schtasks.exe");
    command.args(["/Delete", "/TN", LEGACY_TASK_NAME, "/F"]);
    crate::util::windows_process::suppress_console_window(&mut command);
    let _ = command.output();
}

#[cfg(any(windows, test))]
fn run_command_line(exe: &Path, home: &Path) -> String {
    [exe, Path::new("--home"), home, Path::new("start")]
        .iter()
        .map(|arg| quote_windows_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "macos")]
fn install_macos(exe: &Path, home: &Path) -> Result<(), BoxError> {
    let plist_path = macos_launch_agent_path()?;
    let plist_dir = plist_path
        .parent()
        .ok_or("could not resolve LaunchAgents directory")?;
    std::fs::create_dir_all(plist_dir)?;
    std::fs::write(&plist_path, macos_launch_agent_plist(exe, home))?;
    let _ = macos_launchctl("bootout", &plist_path);
    macos_launchctl("bootstrap", &plist_path)
}

#[cfg(target_os = "macos")]
fn uninstall_macos() -> Result<(), BoxError> {
    // No `launchctl bootout`: it would also stop a daemon launchd started at
    // login. Without the plist, the job is not loaded at the next login.
    remove_file_if_exists(&macos_launch_agent_path()?)
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_path() -> Result<PathBuf, BoxError> {
    Ok(home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{MACOS_LAUNCH_AGENT_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn macos_launchctl(action: &str, plist_path: &Path) -> Result<(), BoxError> {
    let uid = unsafe { libc::geteuid() };
    run_command_status(
        Command::new("launchctl")
            .arg(action)
            .arg(format!("gui/{uid}"))
            .arg(plist_path),
    )
}

#[cfg(any(target_os = "macos", test))]
fn macos_launch_agent_plist(exe: &Path, home: &Path) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let home = xml_escape(&home.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{MACOS_LAUNCH_AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--home</string>
    <string>{home}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#
    )
}

#[cfg(target_os = "linux")]
fn install_linux(exe: &Path, home: &Path) -> Result<(), BoxError> {
    // Without a user systemd (containers, some SSH sessions), fall back to
    // the desktop session's XDG autostart.
    if install_linux_systemd(exe, home).is_ok() {
        return Ok(());
    }
    install_linux_xdg(exe, home)
}

#[cfg(target_os = "linux")]
fn uninstall_linux() -> Result<(), BoxError> {
    let service_path = linux_systemd_service_path()?;
    if service_path.exists() {
        // No `--now`: a running daemon keeps running.
        let _ = run_command_status(Command::new("systemctl").args([
            "--user",
            "disable",
            LINUX_SYSTEMD_SERVICE,
        ]));
        std::fs::remove_file(&service_path)?;
        let _ = run_command_status(Command::new("systemctl").args(["--user", "daemon-reload"]));
    }
    remove_file_if_exists(&linux_xdg_desktop_path()?)
}

#[cfg(target_os = "linux")]
fn install_linux_systemd(exe: &Path, home: &Path) -> Result<(), BoxError> {
    let service_dir = linux_systemd_user_dir()?;
    std::fs::create_dir_all(&service_dir)?;
    std::fs::write(
        service_dir.join(LINUX_SYSTEMD_SERVICE),
        linux_systemd_service(exe, home),
    )?;
    run_command_status(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    run_command_status(Command::new("systemctl").args(["--user", "enable", LINUX_SYSTEMD_SERVICE]))
}

#[cfg(target_os = "linux")]
fn install_linux_xdg(exe: &Path, home: &Path) -> Result<(), BoxError> {
    let desktop_path = linux_xdg_desktop_path()?;
    let desktop_dir = desktop_path
        .parent()
        .ok_or("could not resolve XDG autostart directory")?;
    std::fs::create_dir_all(desktop_dir)?;
    std::fs::write(&desktop_path, linux_xdg_desktop_file(exe, home))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_systemd_is_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "--quiet", LINUX_SYSTEMD_SERVICE])
        .status()
        .is_ok_and(|status| status.success())
        || linux_systemd_user_dir().is_ok_and(|dir| {
            dir.join("default.target.wants")
                .join(LINUX_SYSTEMD_SERVICE)
                .exists()
        })
}

/// `$XDG_CONFIG_HOME`, which both systemd user units and XDG autostart use.
#[cfg(target_os = "linux")]
fn linux_config_dir() -> Result<PathBuf, BoxError> {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Ok(home_dir()?.join(".config")),
    }
}

#[cfg(target_os = "linux")]
fn linux_systemd_user_dir() -> Result<PathBuf, BoxError> {
    Ok(linux_config_dir()?.join("systemd").join("user"))
}

#[cfg(target_os = "linux")]
fn linux_systemd_service_path() -> Result<PathBuf, BoxError> {
    Ok(linux_systemd_user_dir()?.join(LINUX_SYSTEMD_SERVICE))
}

#[cfg(target_os = "linux")]
fn linux_xdg_desktop_path() -> Result<PathBuf, BoxError> {
    Ok(linux_config_dir()?
        .join("autostart")
        .join(LINUX_DESKTOP_FILE))
}

/// `--home` is all the daemon needs: it exports `ANDA_HOME` to the commands
/// it runs itself.
#[cfg(any(target_os = "linux", test))]
fn linux_systemd_service(exe: &Path, home: &Path) -> String {
    let exe = systemd_quote_arg(&exe.to_string_lossy());
    let home = systemd_quote_arg(&home.to_string_lossy());
    format!(
        "[Unit]\n\
Description=Anda Bot daemon\n\n\
[Service]\n\
Type=simple\n\
ExecStart={exe} --home {home} daemon\n\
WorkingDirectory={home}\n\
Restart=no\n\n\
[Install]\n\
WantedBy=default.target\n"
    )
}

#[cfg(any(target_os = "linux", test))]
fn linux_xdg_desktop_file(exe: &Path, home: &Path) -> String {
    let exec = [exe, Path::new("--home"), home, Path::new("daemon")]
        .iter()
        .map(|arg| desktop_quote_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name=Anda Bot\n\
Comment=Start the Anda Bot daemon\n\
Exec={exec}\n\
Terminal=false\n\
X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run_command_status(command: &mut Command) -> Result<(), BoxError> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!("command failed: {detail}").into())
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "linux", test))]
fn systemd_quote_arg(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

#[cfg(any(target_os = "linux", test))]
fn desktop_quote_arg(value: &str) -> String {
    if !value
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\'))
    {
        return value.to_string();
    }

    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_starts_daemon_with_home() {
        let command = run_command_line(
            Path::new("C:\\Program Files\\Anda Bot\\anda.exe"),
            Path::new("C:\\Users\\me\\.anda"),
        );

        assert_eq!(
            command,
            "\"C:\\Program Files\\Anda Bot\\anda.exe\" --home C:\\Users\\me\\.anda start"
        );
    }

    #[test]
    fn status_reports_registration_without_side_effects() {
        // status() only inspects the user's autostart registration; it must
        // never error on a normal developer machine.
        status().unwrap();
    }

    #[test]
    fn macos_plist_escapes_paths() {
        let plist = macos_launch_agent_plist(
            Path::new("/Applications/Anda & Bot/anda"),
            Path::new("/Users/me/.anda\"prod\""),
        );

        assert!(plist.contains("/Applications/Anda &amp; Bot/anda"));
        assert!(plist.contains("/Users/me/.anda&quot;prod&quot;"));
        assert!(plist.contains("<string>daemon</string>"));
    }

    #[test]
    fn linux_systemd_service_quotes_paths() {
        let service = linux_systemd_service(
            Path::new("/home/me/bin/anda bot"),
            Path::new("/home/me/.anda prod"),
        );

        assert!(
            service.contains(
                "ExecStart=\"/home/me/bin/anda bot\" --home \"/home/me/.anda prod\" daemon"
            )
        );
        assert!(service.contains("WorkingDirectory=\"/home/me/.anda prod\""));
        // A user manager has no network-online.target to order against.
        assert!(!service.contains("network-online.target"));
    }

    #[test]
    fn linux_desktop_exec_quotes_paths() {
        let desktop = linux_xdg_desktop_file(
            Path::new("/home/me/bin/anda bot"),
            Path::new("/home/me/.anda prod"),
        );

        assert!(
            desktop
                .contains("Exec=\"/home/me/bin/anda bot\" --home \"/home/me/.anda prod\" daemon")
        );
    }

    #[test]
    fn relative_autostart_home_is_resolved_without_verbatim_prefix() {
        let cwd = std::env::current_dir().unwrap();
        let home = absolute_home(Path::new("anda-autostart-home")).unwrap();
        assert_eq!(home, cwd.join("anda-autostart-home"));
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_startup_registration_command_is_reported() {
        assert!(run_command_status(Command::new("sh").args(["-c", "exit 7"])).is_err());
    }
}
