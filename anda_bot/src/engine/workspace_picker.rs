//! The native folder picker behind the `pick_workspace` RPC.

use rust_i18n::t;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::util::locale;
#[cfg(target_os = "windows")]
use crate::util::windows_process::suppress_tokio_console_window;

/// Asks the user for a workspace folder, titled in the UI language; `None`
/// when they cancel.
pub(super) async fn pick_workspace_path(home_dir: &Path) -> Result<Option<PathBuf>, String> {
    let title = workspace_picker_title(locale::ui_locale(home_dir));

    #[cfg(target_os = "macos")]
    {
        pick_workspace_path_macos(&title).await
    }

    #[cfg(target_os = "windows")]
    {
        return pick_workspace_path_windows(&title).await;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        pick_workspace_path_linux(&title).await
    }
}

fn workspace_picker_title(locale: &str) -> String {
    t!("browser.workspace_picker_title", locale = locale).into_owned()
}

#[cfg(target_os = "macos")]
async fn pick_workspace_path_macos(prompt: &str) -> Result<Option<PathBuf>, String> {
    let script = workspace_picker_macos_script(prompt);
    let output = Command::new("osascript")
        .args(["-e", script.as_str()])
        .output()
        .await
        .map_err(|err| format!("failed to launch macOS folder picker: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") {
            return Ok(None);
        }
        return Err(format!(
            "macOS folder picker failed: {}",
            stderr.trim().trim_matches('"')
        ));
    }

    parse_selected_workspace_path(&output.stdout)
}

#[cfg(any(target_os = "macos", test))]
fn workspace_picker_macos_script(prompt: &str) -> String {
    format!(
        "POSIX path of (choose folder with prompt {})",
        applescript_string(prompt)
    )
}

#[cfg(any(target_os = "macos", test))]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "windows")]
async fn pick_workspace_path_windows(title: &str) -> Result<Option<PathBuf>, String> {
    let script = workspace_picker_windows_script(title);
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-STA")
        .arg("-Command")
        .arg(&script);
    suppress_tokio_console_window(&mut command);
    let output = command
        .output()
        .await
        .map_err(|err| format!("failed to launch Windows folder picker: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Windows folder picker failed: {}",
            stderr.trim().trim_matches('"')
        ));
    }

    parse_selected_workspace_path(&output.stdout)
}

#[cfg(any(target_os = "windows", test))]
fn workspace_picker_windows_script(title: &str) -> String {
    format!(
        concat!(
            "$utf8 = [System.Text.UTF8Encoding]::new($false); ",
            "try {{ [Console]::OutputEncoding = $utf8 }} catch {{ }}; ",
            "$OutputEncoding = $utf8; ",
            "$title = {title}; ",
            "Add-Type -AssemblyName System.Windows.Forms > $null; ",
            "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog; ",
            "$dialog.Description = $title; ",
            "$dialog.UseDescriptionForTitle = $true; ",
            "$owner = New-Object System.Windows.Forms.Form; ",
            "$owner.Text = 'Anda Bot'; ",
            "$owner.StartPosition = 'CenterScreen'; ",
            "$owner.ShowInTaskbar = $false; ",
            "$owner.TopMost = $true; ",
            "$owner.Width = 1; ",
            "$owner.Height = 1; ",
            "$owner.Opacity = 0; ",
            "try {{ ",
            "$owner.Show(); ",
            "$owner.Activate(); ",
            "[void]$owner.Focus(); ",
            "$result = $dialog.ShowDialog($owner); ",
            "if ($result -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $dialog.SelectedPath }} ",
            "}} finally {{ $owner.Close(); $owner.Dispose(); $dialog.Dispose(); }}"
        ),
        title = powershell_single_quoted_string(title)
    )
}

#[cfg(any(target_os = "windows", test))]
fn powershell_single_quoted_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn pick_workspace_path_linux(title: &str) -> Result<Option<PathBuf>, String> {
    let mut errors = Vec::new();

    for (program, args) in [
        (
            "zenity",
            vec![
                "--file-selection".to_string(),
                "--directory".to_string(),
                format!("--title={title}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--getexistingdirectory".to_string(),
                ".".to_string(),
                title.to_string(),
            ],
        ),
    ] {
        let output = match Command::new(program).args(&args).output().await {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                errors.push(format!("{program}: {err}"));
                continue;
            }
        };

        if output.status.success() {
            return parse_selected_workspace_path(&output.stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.code() == Some(1) && stderr.trim().is_empty() {
            return Ok(None);
        }

        errors.push(format!("{program}: {}", stderr.trim()));
    }

    if errors.is_empty() {
        Err("no supported folder picker found; install zenity or kdialog".to_string())
    } else {
        Err(errors.join("; "))
    }
}

fn parse_selected_workspace_path(stdout: &[u8]) -> Result<Option<PathBuf>, String> {
    let selected = decode_selected_workspace_stdout(stdout)
        .ok_or_else(|| "folder picker returned a non-text workspace path".to_string())?;
    Ok(normalize_selected_workspace_path(&selected))
}

fn decode_selected_workspace_stdout(stdout: &[u8]) -> Option<String> {
    if let Ok(text) = std::str::from_utf8(stdout) {
        return Some(text.to_string());
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(text) =
            decode_bytes_with_windows_code_page(stdout, windows_console_output_code_page())
        {
            return Some(text);
        }
        return anda_core::text_from_bytes(stdout).map(|text| text.into_owned());
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
fn decode_bytes_with_windows_code_page(bytes: &[u8], code_page: u32) -> Option<String> {
    anda_core::text_from_bytes_with_encoding(
        bytes,
        anda_core::windows_code_page_encoding(code_page),
    )
    .map(|text| text.into_owned())
}

#[cfg(target_os = "windows")]
fn windows_console_output_code_page() -> u32 {
    unsafe { windows_sys::Win32::Globalization::GetOEMCP() }
}

fn normalize_selected_workspace_path(selected: &str) -> Option<PathBuf> {
    let trimmed = selected.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path: PathBuf = std::path::Path::new(trimmed).components().collect();
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return None;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, path::MAIN_SEPARATOR};

    #[test]
    fn workspace_picker_title_uses_locale_resources() {
        let en = workspace_picker_title("en");
        let zh = workspace_picker_title("zh-Hans");

        assert_eq!(en, "Open a workspace folder for Anda");
        assert_ne!(zh, en);
        assert!(zh.contains("Anda"));
        assert_ne!(workspace_picker_title("fr"), en);
    }

    #[test]
    fn macos_workspace_picker_script_escapes_prompt_text() {
        let script = workspace_picker_macos_script("Choose \"Anda\" \\ folder");

        assert_eq!(
            script,
            "POSIX path of (choose folder with prompt \"Choose \\\"Anda\\\" \\\\ folder\")"
        );
    }

    #[test]
    fn windows_workspace_picker_script_uses_localized_title_and_owner() {
        let script = workspace_picker_windows_script("Choose Anda's workspace");

        assert!(script.contains("$title = 'Choose Anda''s workspace';"));
        assert!(script.contains("$dialog.Description = $title;"));
        assert!(script.contains("$owner.TopMost = $true;"));
        assert!(script.contains("$dialog.ShowDialog($owner);"));
    }

    #[test]
    fn powershell_single_quoted_string_escapes_quotes() {
        assert_eq!(
            powershell_single_quoted_string("Anda's workspace"),
            "'Anda''s workspace'"
        );
    }

    #[test]
    fn normalize_selected_workspace_path_trims_and_drops_trailing_separator() {
        let expected = env::temp_dir().join("anda").join("workspace");
        let selected = format!("  {}{}  ", expected.display(), MAIN_SEPARATOR);
        let path = normalize_selected_workspace_path(&selected).unwrap();

        assert_eq!(path, expected);
    }

    #[test]
    fn normalize_selected_workspace_path_rejects_empty_or_relative_values() {
        assert_eq!(normalize_selected_workspace_path("   "), None);
        assert_eq!(normalize_selected_workspace_path("workspace/project"), None);
    }

    #[test]
    fn selected_workspace_stdout_decodes_legacy_chinese_windows_bytes() {
        let gbk_path = [b'E', b':', b'\\', 0xD6, 0xD0, 0xCE, 0xC4, b'\r', b'\n'];

        assert_eq!(
            decode_bytes_with_windows_code_page(&gbk_path, 936).as_deref(),
            Some("E:\\中文\r\n")
        );
    }
}
