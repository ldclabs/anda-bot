use std::process::Command;

#[cfg(any(windows, test))]
use std::{fs, path::Path};

#[cfg(windows)]
use std::path::PathBuf;

use crate::core::{
    LauncherContext, LauncherResult, WizardConfig, default_model_for_provider, provider_by_id,
    provider_ids, text, write_initial_minimal_config, write_minimal_config,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(any(windows, test))]
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

pub fn run_wizard(ctx: &LauncherContext) -> LauncherResult<bool> {
    let Some(config) = show_settings_dialog(ctx)? else {
        return Ok(false);
    };
    write_minimal_config(ctx, &config)?;
    Ok(true)
}

pub fn run_initial_setup_wizard(ctx: &LauncherContext) -> LauncherResult<bool> {
    let Some(config) = show_settings_dialog(ctx)? else {
        return Ok(false);
    };
    write_initial_minimal_config(ctx, &config)?;
    Ok(true)
}

#[cfg(windows)]
pub fn show_settings_dialog(ctx: &LauncherContext) -> LauncherResult<Option<WizardConfig>> {
    let script_path = settings_script_path(ctx, "settings.ps1")?;
    write_windows_settings_script(&script_path)?;

    let output = run_windows_powershell(&script_path)?;
    if !output.status.success() {
        let stderr = decode_powershell_output(&output.stderr);
        if stderr.contains("Anda setup cancelled") {
            return Ok(None);
        }
        return Err(text().settings_wizard_failed(stderr.trim()).into());
    }

    let stdout = decode_powershell_output(&output.stdout);
    parse_settings_payload(&stdout)
}

#[cfg(target_os = "macos")]
pub fn show_settings_dialog(_ctx: &LauncherContext) -> LauncherResult<Option<WizardConfig>> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(macos_settings_script())
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("User canceled") || stderr.contains("-128") {
            return Ok(None);
        }
        return Err(text().settings_wizard_failed(stderr.trim()).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_settings_payload(&stdout)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn show_settings_dialog(_ctx: &LauncherContext) -> LauncherResult<Option<WizardConfig>> {
    Err(text().settings_not_supported.into())
}

pub fn parse_settings_payload(payload: &str) -> LauncherResult<Option<WizardConfig>> {
    let mut provider_id = None;
    let mut api_key = None;
    let mut model = None;

    for line in payload.lines() {
        if let Some(value) = line.strip_prefix("ANDA_PROVIDER=") {
            provider_id = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("ANDA_API_KEY=") {
            api_key = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("ANDA_MODEL=") {
            model = Some(value.trim().to_string());
        }
    }

    let Some(provider_id) = provider_id else {
        return Ok(None);
    };
    let Some(provider) = provider_by_id(&provider_id) else {
        return Err(text().unsupported_provider_from_wizard(&provider_id).into());
    };
    let api_key = api_key.unwrap_or_default();
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| provider.model.to_string());

    if provider.requires_api_key() && api_key.trim().is_empty() {
        return Err(text().env_required(provider.env_var).into());
    }

    Ok(Some(WizardConfig {
        provider_id,
        api_key,
        model,
    }))
}

#[cfg(windows)]
fn settings_script_path(ctx: &LauncherContext, file_name: &str) -> LauncherResult<PathBuf> {
    let dir = ctx.home.join("launcher");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(file_name))
}

#[cfg(any(windows, test))]
fn write_windows_settings_script(script_path: &Path) -> std::io::Result<()> {
    let script = windows_settings_script();
    let mut bytes = Vec::with_capacity(UTF8_BOM.len() + script.len());
    bytes.extend_from_slice(UTF8_BOM);
    bytes.extend_from_slice(script.as_bytes());
    fs::write(script_path, bytes)
}

#[cfg(windows)]
fn run_windows_powershell(script_path: &PathBuf) -> LauncherResult<std::process::Output> {
    let mut last_err = None;
    for exe in ["powershell.exe", "pwsh.exe"] {
        let mut command = Command::new(exe);
        command
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(script_path)
            .creation_flags(CREATE_NO_WINDOW);
        match command.output() {
            Ok(output) => return Ok(output),
            Err(err) => last_err = Some(err),
        }
    }

    let detail = last_err
        .map(|err| err.to_string())
        .unwrap_or_else(|| text().powershell_not_found);
    Err(text().powershell_launch_failed(&detail).into())
}

#[cfg(windows)]
fn decode_powershell_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    if let Some(text) =
        decode_bytes_with_windows_code_page(bytes, windows_console_output_code_page())
    {
        return text;
    }

    anda_core::text_from_bytes(bytes)
        .map(|text| text.into_owned())
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(any(windows, test))]
fn decode_bytes_with_windows_code_page(bytes: &[u8], code_page: u32) -> Option<String> {
    anda_core::text_from_bytes_with_encoding(
        bytes,
        anda_core::windows_code_page_encoding(code_page),
    )
    .map(|text| text.into_owned())
}

#[cfg(windows)]
fn windows_console_output_code_page() -> u32 {
    unsafe { windows_sys::Win32::Globalization::GetOEMCP() }
}

#[cfg(any(windows, test))]
fn windows_settings_script() -> String {
    let copy = text();
    let provider_items = provider_ids()
        .into_iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let model_entries = provider_ids()
        .into_iter()
        .map(|id| format!("  {id} = '{}'", ps_single(default_model_for_provider(id))))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$models = @{{
{model_entries}
}}

$form = New-Object System.Windows.Forms.Form
$form.Text = '{setup_title}'
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$form.ClientSize = New-Object System.Drawing.Size(430, 230)
$form.Font = New-Object System.Drawing.Font('Segoe UI', 9)

$providerLabel = New-Object System.Windows.Forms.Label
$providerLabel.Text = '{provider_label}'
$providerLabel.Location = New-Object System.Drawing.Point(18, 24)
$providerLabel.Size = New-Object System.Drawing.Size(110, 24)
$form.Controls.Add($providerLabel)

$provider = New-Object System.Windows.Forms.ComboBox
$provider.DropDownStyle = [System.Windows.Forms.ComboBoxStyle]::DropDownList
$provider.Location = New-Object System.Drawing.Point(140, 20)
$provider.Size = New-Object System.Drawing.Size(250, 28)
[void]$provider.Items.AddRange(@({provider_items}))
$provider.SelectedIndex = 0
$form.Controls.Add($provider)

$modelLabel = New-Object System.Windows.Forms.Label
$modelLabel.Text = '{model_label}'
$modelLabel.Location = New-Object System.Drawing.Point(18, 74)
$modelLabel.Size = New-Object System.Drawing.Size(110, 24)
$form.Controls.Add($modelLabel)

$model = New-Object System.Windows.Forms.TextBox
$model.Location = New-Object System.Drawing.Point(140, 70)
$model.Size = New-Object System.Drawing.Size(250, 28)
$model.Text = $models[$provider.SelectedItem]
$form.Controls.Add($model)

$apiKeyLabel = New-Object System.Windows.Forms.Label
$apiKeyLabel.Text = '{api_key_label}'
$apiKeyLabel.Location = New-Object System.Drawing.Point(18, 124)
$apiKeyLabel.Size = New-Object System.Drawing.Size(110, 24)
$form.Controls.Add($apiKeyLabel)

$apiKey = New-Object System.Windows.Forms.TextBox
$apiKey.Location = New-Object System.Drawing.Point(140, 120)
$apiKey.Size = New-Object System.Drawing.Size(250, 28)
$apiKey.UseSystemPasswordChar = $true
$form.Controls.Add($apiKey)

$provider.Add_SelectedIndexChanged({{
  $model.Text = $models[$provider.SelectedItem]
}})

$ok = New-Object System.Windows.Forms.Button
$ok.Text = '{save_label}'
$ok.Location = New-Object System.Drawing.Point(220, 176)
$ok.Size = New-Object System.Drawing.Size(80, 30)
$ok.Add_Click({{
  $apiKeyRequired = $provider.SelectedItem -ne 'codex'
  if ([string]::IsNullOrWhiteSpace($model.Text) -or ($apiKeyRequired -and [string]::IsNullOrWhiteSpace($apiKey.Text))) {{
    [System.Windows.Forms.MessageBox]::Show('{setup_required_message}', '{setup_title}', 'OK', 'Warning') | Out-Null
    return
  }}
  $form.DialogResult = [System.Windows.Forms.DialogResult]::OK
  $form.Close()
}})
$form.Controls.Add($ok)
$form.AcceptButton = $ok

$cancel = New-Object System.Windows.Forms.Button
$cancel.Text = '{cancel_label}'
$cancel.Location = New-Object System.Drawing.Point(310, 176)
$cancel.Size = New-Object System.Drawing.Size(80, 30)
$cancel.Add_Click({{
  $form.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
  $form.Close()
}})
$form.Controls.Add($cancel)
$form.CancelButton = $cancel

$result = $form.ShowDialog()
if ($result -ne [System.Windows.Forms.DialogResult]::OK) {{
  Write-Error 'Anda setup cancelled'
  exit 2
}}

Write-Output ('ANDA_PROVIDER=' + $provider.SelectedItem)
Write-Output ('ANDA_MODEL=' + $model.Text)
Write-Output ('ANDA_API_KEY=' + $apiKey.Text)
"#,
        setup_title = ps_single(&copy.setup_title),
        provider_label = ps_single(&copy.provider),
        model_label = ps_single(&copy.model),
        api_key_label = ps_single(&copy.api_key),
        save_label = ps_single(&copy.save),
        setup_required_message = ps_single(&copy.setup_required_message),
        cancel_label = ps_single(&copy.cancel),
    )
}

#[cfg(target_os = "macos")]
fn macos_settings_script() -> String {
    let copy = text();
    let providers = provider_ids()
        .into_iter()
        .map(applescript_string)
        .collect::<Vec<_>>()
        .join(", ");
    let cases = provider_ids()
        .into_iter()
        .map(|id| {
            format!(
                "if providerId is {} then set defaultModel to {}",
                applescript_string(id),
                applescript_string(default_model_for_provider(id))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"
set providerChoices to {{{providers}}}
set selectedProvider to choose from list providerChoices with title {setup_title} with prompt {choose_provider_prompt} default items {{{default_provider}}}
if selectedProvider is false then error number -128
set providerId to item 1 of selectedProvider
set defaultModel to ""
{cases}
set modelDialog to display dialog {model_prompt} default answer defaultModel with title {setup_title}
set modelText to text returned of modelDialog
set keyDialog to display dialog {api_key_prompt} default answer "" with hidden answer with title {setup_title}
set keyText to text returned of keyDialog
return "ANDA_PROVIDER=" & providerId & linefeed & "ANDA_MODEL=" & modelText & linefeed & "ANDA_API_KEY=" & keyText
"#,
        default_provider = applescript_string(provider_ids()[0]),
        setup_title = applescript_string(&copy.setup_title),
        choose_provider_prompt = applescript_string(&copy.choose_provider_prompt),
        model_prompt = applescript_string(&format!("{}:", copy.model)),
        api_key_prompt = applescript_string(&format!("{}:", copy.api_key)),
    )
}

#[cfg(any(windows, test))]
fn ps_single(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_payload() {
        let parsed = parse_settings_payload(
            "ANDA_PROVIDER=openai\nANDA_MODEL=gpt-test\nANDA_API_KEY=sk-test\n",
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            parsed,
            WizardConfig {
                provider_id: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_key: "sk-test".to_string(),
            }
        );
    }

    #[test]
    fn rejects_empty_api_key() {
        assert!(parse_settings_payload("ANDA_PROVIDER=openai\nANDA_API_KEY=\n").is_err());
    }

    #[test]
    fn allows_empty_api_key_for_codex_auth_provider() {
        let parsed =
            parse_settings_payload("ANDA_PROVIDER=codex\nANDA_MODEL=gpt-5.5\nANDA_API_KEY=\n")
                .unwrap()
                .unwrap();

        assert_eq!(
            parsed,
            WizardConfig {
                provider_id: "codex".to_string(),
                model: "gpt-5.5".to_string(),
                api_key: String::new(),
            }
        );
    }

    #[test]
    fn writes_windows_settings_script_with_utf8_bom() {
        let temp = tempfile::tempdir().unwrap();
        let script_path = temp.path().join("settings.ps1");

        write_windows_settings_script(&script_path).unwrap();

        let bytes = fs::read(script_path).unwrap();
        assert!(bytes.starts_with(UTF8_BOM));
        assert_eq!(
            &bytes[UTF8_BOM.len()..],
            windows_settings_script().as_bytes()
        );
    }

    #[test]
    fn decodes_windows_power_shell_output_with_oem_code_page() {
        let gbk = [0xC9, 0xE8, 0xD6, 0xC3, 0xCF, 0xF2, 0xB5, 0xBC];

        assert_eq!(
            decode_bytes_with_windows_code_page(&gbk, 936).as_deref(),
            Some("设置向导")
        );
    }

    #[test]
    fn parse_settings_payload_handles_missing_and_unsupported_providers() {
        // No provider line -> the wizard was cancelled (None).
        assert!(parse_settings_payload("ANDA_MODEL=x\n").unwrap().is_none());

        // An unknown provider id surfaces an error.
        let err = parse_settings_payload("ANDA_PROVIDER=does-not-exist\n")
            .map(|_| ())
            .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn ps_single_doubles_single_quotes() {
        assert_eq!(ps_single("it's a 'test'"), "it''s a ''test''");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_string_escapes_quotes_and_backslashes() {
        assert_eq!(applescript_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_settings_script_lists_providers() {
        let script = macos_settings_script();
        assert!(!script.is_empty());
        // The generated AppleScript references the configured providers.
        for id in provider_ids() {
            assert!(script.contains(id), "script missing provider {id}");
        }
    }
}
