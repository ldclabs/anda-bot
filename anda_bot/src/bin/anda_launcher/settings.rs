use std::process::Command;

#[cfg(windows)]
use std::{fs, path::PathBuf};

use crate::core::{
    LauncherContext, LauncherResult, WizardConfig, default_model_for_provider, provider_by_id,
    provider_ids, write_minimal_config,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn run_wizard(ctx: &LauncherContext) -> LauncherResult<bool> {
    let Some(config) = show_settings_dialog(ctx)? else {
        return Ok(false);
    };
    write_minimal_config(ctx, &config)?;
    Ok(true)
}

#[cfg(windows)]
pub fn show_settings_dialog(ctx: &LauncherContext) -> LauncherResult<Option<WizardConfig>> {
    let script_path = settings_script_path(ctx, "settings.ps1")?;
    fs::write(&script_path, windows_settings_script())?;

    let output = run_windows_powershell(&script_path)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Anda setup cancelled") {
            return Ok(None);
        }
        return Err(format!("settings wizard failed: {}", stderr.trim()).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
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
        return Err(format!("settings wizard failed: {}", stderr.trim()).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_settings_payload(&stdout)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn show_settings_dialog(_ctx: &LauncherContext) -> LauncherResult<Option<WizardConfig>> {
    Err("Anda Launcher settings are not supported on this platform".into())
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
        return Err(format!("unsupported provider returned by wizard: {provider_id}").into());
    };
    let api_key = api_key.unwrap_or_default();
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| provider.model.to_string());

    if api_key.trim().is_empty() {
        return Err(format!("{} is required", provider.env_var).into());
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

    Err(format!(
        "could not launch PowerShell for settings wizard: {}",
        last_err
            .map(|err| err.to_string())
            .unwrap_or_else(|| "not found".to_string())
    )
    .into())
}

#[cfg(windows)]
fn windows_settings_script() -> String {
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
$form.Text = 'Anda Bot Setup'
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$form.ClientSize = New-Object System.Drawing.Size(430, 230)
$form.Font = New-Object System.Drawing.Font('Segoe UI', 9)

$providerLabel = New-Object System.Windows.Forms.Label
$providerLabel.Text = 'Provider'
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
$modelLabel.Text = 'Model'
$modelLabel.Location = New-Object System.Drawing.Point(18, 74)
$modelLabel.Size = New-Object System.Drawing.Size(110, 24)
$form.Controls.Add($modelLabel)

$model = New-Object System.Windows.Forms.TextBox
$model.Location = New-Object System.Drawing.Point(140, 70)
$model.Size = New-Object System.Drawing.Size(250, 28)
$model.Text = $models[$provider.SelectedItem]
$form.Controls.Add($model)

$apiKeyLabel = New-Object System.Windows.Forms.Label
$apiKeyLabel.Text = 'API key'
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
$ok.Text = 'Save'
$ok.Location = New-Object System.Drawing.Point(220, 176)
$ok.Size = New-Object System.Drawing.Size(80, 30)
$ok.Add_Click({{
  if ([string]::IsNullOrWhiteSpace($apiKey.Text) -or [string]::IsNullOrWhiteSpace($model.Text)) {{
    [System.Windows.Forms.MessageBox]::Show('Model and API key are required.', 'Anda Bot Setup', 'OK', 'Warning') | Out-Null
    return
  }}
  $form.DialogResult = [System.Windows.Forms.DialogResult]::OK
  $form.Close()
}})
$form.Controls.Add($ok)
$form.AcceptButton = $ok

$cancel = New-Object System.Windows.Forms.Button
$cancel.Text = 'Cancel'
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
"#
    )
}

#[cfg(target_os = "macos")]
fn macos_settings_script() -> String {
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
set selectedProvider to choose from list providerChoices with title "Anda Bot Setup" with prompt "Choose a model provider:" default items {{{default_provider}}}
if selectedProvider is false then error number -128
set providerId to item 1 of selectedProvider
set defaultModel to ""
{cases}
set modelDialog to display dialog "Model:" default answer defaultModel with title "Anda Bot Setup"
set modelText to text returned of modelDialog
set keyDialog to display dialog "API key:" default answer "" with hidden answer with title "Anda Bot Setup"
set keyText to text returned of keyDialog
return "ANDA_PROVIDER=" & providerId & linefeed & "ANDA_MODEL=" & modelText & linefeed & "ANDA_API_KEY=" & keyText
"#,
        default_provider = applescript_string(provider_ids()[0]),
    )
}

#[cfg(windows)]
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
}
