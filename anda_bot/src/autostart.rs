use anda_core::BoxError;
use clap::Subcommand;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(windows)]
const TASK_NAME: &str = "Anda Bot";
#[cfg(any(target_os = "macos", test))]
const MACOS_LAUNCH_AGENT_LABEL: &str = "com.ldclabs.anda-bot";
#[cfg(target_os = "linux")]
const LINUX_SYSTEMD_SERVICE: &str = "anda-bot.service";
#[cfg(target_os = "linux")]
const LINUX_DESKTOP_FILE: &str = "anda-bot.desktop";

#[derive(Subcommand)]
pub enum AutostartCommand {
    /// Register Anda to start when the current user logs in.
    Install,
    /// Remove the current user's Anda startup registration.
    Uninstall,
    /// Show whether the current user's Anda startup registration exists.
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AutostartStatus {
    Installed,
    NotInstalled,
    Unsupported,
}

pub fn install(home: &Path) -> Result<(), BoxError> {
    #[cfg(windows)]
    {
        install_windows(home)
    }

    #[cfg(target_os = "macos")]
    {
        install_macos(home)
    }

    #[cfg(target_os = "linux")]
    {
        install_linux(home)
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = home;
        Err("anda autostart is not supported on this platform".into())
    }
}

pub fn uninstall() -> Result<AutostartStatus, BoxError> {
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
        Ok(AutostartStatus::Unsupported)
    }
}

pub fn status() -> Result<AutostartStatus, BoxError> {
    #[cfg(windows)]
    {
        status_windows()
    }

    #[cfg(target_os = "macos")]
    {
        status_macos()
    }

    #[cfg(target_os = "linux")]
    {
        status_linux()
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Ok(AutostartStatus::Unsupported)
    }
}

fn home_dir() -> Result<PathBuf, BoxError> {
    std::env::home_dir().ok_or_else(|| "could not detect current user's home directory".into())
}

fn current_exe() -> Result<PathBuf, BoxError> {
    std::env::current_exe()
        .map_err(|err| format!("could not detect current executable path: {err}").into())
}

#[cfg(windows)]
fn install_windows(home: &Path) -> Result<(), BoxError> {
    let exe = current_exe()?;
    let task_command = task_command_line(&exe, home);
    run_schtasks(&[
        "/Create",
        "/TN",
        TASK_NAME,
        "/SC",
        "ONLOGON",
        "/TR",
        &task_command,
        "/F",
    ])?;
    Ok(())
}

#[cfg(windows)]
fn uninstall_windows() -> Result<AutostartStatus, BoxError> {
    match run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]) {
        Ok(()) => Ok(AutostartStatus::NotInstalled),
        Err(err) if is_missing_task_error(&err.to_string()) => Ok(AutostartStatus::NotInstalled),
        Err(err) => Err(err),
    }
}

#[cfg(windows)]
fn status_windows() -> Result<AutostartStatus, BoxError> {
    match run_schtasks(&["/Query", "/TN", TASK_NAME]) {
        Ok(()) => Ok(AutostartStatus::Installed),
        Err(err) if is_missing_task_error(&err.to_string()) => Ok(AutostartStatus::NotInstalled),
        Err(err) => Err(err),
    }
}

#[cfg(windows)]
fn run_schtasks(args: &[&str]) -> Result<(), BoxError> {
    let output = Command::new("schtasks.exe").args(args).output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!("schtasks.exe failed: {detail}").into())
}

#[cfg(windows)]
fn is_missing_task_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("cannot find the file specified")
        || message.contains("the system cannot find the file specified")
        || message.contains("not exist")
        || message.contains("does not exist")
}

#[cfg(any(windows, test))]
fn task_command_line(exe: &Path, home: &Path) -> String {
    windows_command_line([
        exe.to_path_buf(),
        PathBuf::from("--home"),
        home.to_path_buf(),
        PathBuf::from("daemon"),
    ])
}

#[cfg(any(windows, test))]
fn windows_command_line<I>(args: I) -> String
where
    I: IntoIterator<Item = PathBuf>,
{
    args.into_iter()
        .map(|arg| quote_windows_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(windows, test))]
fn quote_windows_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }

    if !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return value.to_string();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(target_os = "macos")]
fn install_macos(home: &Path) -> Result<(), BoxError> {
    let plist_path = macos_launch_agent_path()?;
    let plist_dir = plist_path
        .parent()
        .ok_or("could not resolve LaunchAgents directory")?;
    std::fs::create_dir_all(plist_dir)?;
    std::fs::write(&plist_path, macos_launch_agent_plist(&current_exe()?, home))?;
    let _ = macos_launchctl_bootout(&plist_path);
    let _ = macos_launchctl_bootstrap(&plist_path);
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_macos() -> Result<AutostartStatus, BoxError> {
    let plist_path = macos_launch_agent_path()?;
    let _ = macos_launchctl_bootout(&plist_path);
    match std::fs::remove_file(&plist_path) {
        Ok(()) => Ok(AutostartStatus::NotInstalled),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AutostartStatus::NotInstalled),
        Err(err) => Err(err.into()),
    }
}

#[cfg(target_os = "macos")]
fn status_macos() -> Result<AutostartStatus, BoxError> {
    if macos_launch_agent_path()?.exists() {
        Ok(AutostartStatus::Installed)
    } else {
        Ok(AutostartStatus::NotInstalled)
    }
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_path() -> Result<PathBuf, BoxError> {
    Ok(home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{MACOS_LAUNCH_AGENT_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn macos_launchctl_bootstrap(plist_path: &Path) -> Result<(), BoxError> {
    run_command_status(
        Command::new("launchctl")
            .arg("bootstrap")
            .arg(format!("gui/{}", current_uid()))
            .arg(plist_path),
    )
}

#[cfg(target_os = "macos")]
fn macos_launchctl_bootout(plist_path: &Path) -> Result<(), BoxError> {
    run_command_status(
        Command::new("launchctl")
            .arg("bootout")
            .arg(format!("gui/{}", current_uid()))
            .arg(plist_path),
    )
}

#[cfg(target_os = "macos")]
fn current_uid() -> u32 {
    unsafe { libc::geteuid() }
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
fn install_linux(home: &Path) -> Result<(), BoxError> {
    if install_linux_systemd(home).is_ok() {
        return Ok(());
    }
    install_linux_xdg(home)
}

#[cfg(target_os = "linux")]
fn uninstall_linux() -> Result<AutostartStatus, BoxError> {
    if linux_systemd_service_path()?.exists() {
        let _ = run_command_status(
            Command::new("systemctl")
                .arg("--user")
                .arg("disable")
                .arg("--now")
                .arg(LINUX_SYSTEMD_SERVICE),
        );
        match std::fs::remove_file(linux_systemd_service_path()?) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        let _ = run_command_status(Command::new("systemctl").arg("--user").arg("daemon-reload"));
    }

    if linux_xdg_desktop_path()?.exists() {
        match std::fs::remove_file(linux_xdg_desktop_path()?) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok(AutostartStatus::NotInstalled)
}

#[cfg(target_os = "linux")]
fn status_linux() -> Result<AutostartStatus, BoxError> {
    if linux_systemd_is_enabled() || linux_xdg_desktop_path()?.exists() {
        Ok(AutostartStatus::Installed)
    } else {
        Ok(AutostartStatus::NotInstalled)
    }
}

#[cfg(target_os = "linux")]
fn install_linux_systemd(home: &Path) -> Result<(), BoxError> {
    let service_path = linux_systemd_service_path()?;
    let service_dir = service_path
        .parent()
        .ok_or("could not resolve systemd user service directory")?;
    std::fs::create_dir_all(service_dir)?;
    std::fs::write(&service_path, linux_systemd_service(&current_exe()?, home))?;
    run_command_status(Command::new("systemctl").arg("--user").arg("daemon-reload"))?;
    run_command_status(
        Command::new("systemctl")
            .arg("--user")
            .arg("enable")
            .arg(LINUX_SYSTEMD_SERVICE),
    )
}

#[cfg(target_os = "linux")]
fn install_linux_xdg(home: &Path) -> Result<(), BoxError> {
    let desktop_path = linux_xdg_desktop_path()?;
    let desktop_dir = desktop_path
        .parent()
        .ok_or("could not resolve XDG autostart directory")?;
    std::fs::create_dir_all(desktop_dir)?;
    std::fs::write(&desktop_path, linux_xdg_desktop_file(&current_exe()?, home))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_systemd_is_enabled() -> bool {
    Command::new("systemctl")
        .arg("--user")
        .arg("is-enabled")
        .arg("--quiet")
        .arg(LINUX_SYSTEMD_SERVICE)
        .status()
        .is_ok_and(|status| status.success())
        || linux_systemd_wants_path().is_ok_and(|path| path.exists())
}

#[cfg(target_os = "linux")]
fn linux_systemd_service_path() -> Result<PathBuf, BoxError> {
    Ok(home_dir()?
        .join(".config")
        .join("systemd")
        .join("user")
        .join(LINUX_SYSTEMD_SERVICE))
}

#[cfg(target_os = "linux")]
fn linux_systemd_wants_path() -> Result<PathBuf, BoxError> {
    Ok(home_dir()?
        .join(".config")
        .join("systemd")
        .join("user")
        .join("default.target.wants")
        .join(LINUX_SYSTEMD_SERVICE))
}

#[cfg(target_os = "linux")]
fn linux_xdg_desktop_path() -> Result<PathBuf, BoxError> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home_dir()?.join(".config"));
    Ok(base.join("autostart").join(LINUX_DESKTOP_FILE))
}

#[cfg(any(target_os = "linux", test))]
fn linux_systemd_service(exe: &Path, home: &Path) -> String {
    let exe = systemd_quote_arg(&exe.to_string_lossy());
    let home_arg = systemd_quote_arg(&home.to_string_lossy());
    let home_env = systemd_quote_arg(&format!("ANDA_HOME={}", home.to_string_lossy()));
    let working_dir = systemd_quote_arg(&home.to_string_lossy());
    format!(
        "[Unit]\n\
Description=Anda Bot daemon\n\
After=network-online.target\n\
Wants=network-online.target\n\n\
[Service]\n\
Type=simple\n\
ExecStart={exe} --home {home_arg} daemon\n\
WorkingDirectory={working_dir}\n\
Environment={home_env}\n\
Restart=no\n\n\
[Install]\n\
WantedBy=default.target\n"
    )
}

#[cfg(any(target_os = "linux", test))]
fn linux_xdg_desktop_file(exe: &Path, home: &Path) -> String {
    let exec = desktop_exec_line(&[
        exe.to_path_buf(),
        "--home".into(),
        home.into(),
        "daemon".into(),
    ]);
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
fn desktop_exec_line(args: &[PathBuf]) -> String {
    args.iter()
        .map(|arg| desktop_quote_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
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
mod windows_tests {
    use super::*;

    #[test]
    fn quotes_windows_args_with_spaces_and_quotes() {
        assert_eq!(
            quote_windows_arg("C:\\Anda Bot\\anda.exe"),
            "\"C:\\Anda Bot\\anda.exe\""
        );
        assert_eq!(quote_windows_arg("plain"), "plain");
        assert_eq!(quote_windows_arg("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn task_command_runs_daemon_with_home() {
        let command = task_command_line(
            Path::new("C:\\Program Files\\Anda Bot\\anda.exe"),
            Path::new("C:\\Users\\me\\.anda"),
        );

        assert_eq!(
            command,
            "\"C:\\Program Files\\Anda Bot\\anda.exe\" --home C:\\Users\\me\\.anda daemon"
        );
    }
}

#[cfg(test)]
mod macos_tests {
    use super::*;

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
}

#[cfg(test)]
mod linux_tests {
    use super::*;

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
        assert!(service.contains("Environment=\"ANDA_HOME=/home/me/.anda prod\""));
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
}
