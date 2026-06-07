use anda_core::BoxError;
use clap::Subcommand;
use std::path::Path;

#[cfg(windows)]
use std::{path::PathBuf, process::Command};

#[cfg(windows)]
const TASK_NAME: &str = "Anda Bot";

#[derive(Subcommand)]
pub enum AutostartCommand {
    /// Register Anda to start when the current Windows user logs in.
    Install,
    /// Remove the current user's Anda startup task.
    Uninstall,
    /// Show whether the current user's Anda startup task is registered.
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
        let exe = std::env::current_exe()?;
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
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let _ = home;
        Err("anda autostart is only supported on Windows for now".into())
    }
}

pub fn uninstall() -> Result<AutostartStatus, BoxError> {
    #[cfg(windows)]
    {
        match run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]) {
            Ok(()) => Ok(AutostartStatus::NotInstalled),
            Err(err) if is_missing_task_error(&err.to_string()) => {
                Ok(AutostartStatus::NotInstalled)
            }
            Err(err) => Err(err),
        }
    }

    #[cfg(not(windows))]
    {
        Ok(AutostartStatus::Unsupported)
    }
}

pub fn status() -> Result<AutostartStatus, BoxError> {
    #[cfg(windows)]
    {
        match run_schtasks(&["/Query", "/TN", TASK_NAME]) {
            Ok(()) => Ok(AutostartStatus::Installed),
            Err(err) if is_missing_task_error(&err.to_string()) => {
                Ok(AutostartStatus::NotInstalled)
            }
            Err(err) => Err(err),
        }
    }

    #[cfg(not(windows))]
    {
        Ok(AutostartStatus::Unsupported)
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

#[cfg(windows)]
fn task_command_line(exe: &Path, home: &Path) -> String {
    windows_command_line([
        exe.to_path_buf(),
        PathBuf::from("--home"),
        home.to_path_buf(),
        PathBuf::from("daemon"),
    ])
}

#[cfg(windows)]
fn windows_command_line<I>(args: I) -> String
where
    I: IntoIterator<Item = PathBuf>,
{
    args.into_iter()
        .map(|arg| quote_windows_arg(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
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

#[cfg(all(test, windows))]
mod tests {
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
