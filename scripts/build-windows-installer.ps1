param(
    [string]$ReleaseDir = "release",
    [string]$Target = "windows-x86_64",
    [string]$OutputName = "AndaBotSetup-windows-x86_64.exe"
)

$ErrorActionPreference = "Stop"

function Fail($Message) {
    Write-Error $Message
    exit 1
}

$releasePath = Resolve-Path $ReleaseDir
$andaAsset = Join-Path $releasePath "anda-$Target.exe"
$launcherAsset = Join-Path $releasePath "anda_launcher-$Target.exe"
$skillsAsset = Join-Path $releasePath "anda-skills.zip"
$outputPath = Join-Path $releasePath $OutputName

if (!(Test-Path $andaAsset)) { Fail "Missing $andaAsset" }
if (!(Test-Path $launcherAsset)) { Fail "Missing $launcherAsset" }
if (!(Test-Path $skillsAsset)) { Fail "Missing $skillsAsset" }

function Write-IcoFromPng($PngPath, $IcoPath) {
    [byte[]]$png = [System.IO.File]::ReadAllBytes($PngPath)
    $width = [System.BitConverter]::ToUInt32([byte[]]@($png[19], $png[18], $png[17], $png[16]), 0)
    $height = [System.BitConverter]::ToUInt32([byte[]]@($png[23], $png[22], $png[21], $png[20]), 0)
    [byte[]]$ico = New-Object byte[] (22 + $png.Length)
    [BitConverter]::GetBytes([UInt16]0).CopyTo($ico, 0)
    [BitConverter]::GetBytes([UInt16]1).CopyTo($ico, 2)
    [BitConverter]::GetBytes([UInt16]1).CopyTo($ico, 4)
    $ico[6] = if ($width -eq 256) { 0 } else { [byte]$width }
    $ico[7] = if ($height -eq 256) { 0 } else { [byte]$height }
    $ico[8] = 0
    $ico[9] = 0
    [BitConverter]::GetBytes([UInt16]1).CopyTo($ico, 10)
    [BitConverter]::GetBytes([UInt16]32).CopyTo($ico, 12)
    [BitConverter]::GetBytes([UInt32]$png.Length).CopyTo($ico, 14)
    [BitConverter]::GetBytes([UInt32]22).CopyTo($ico, 18)
    [Array]::Copy($png, 0, $ico, 22, $png.Length)
    [System.IO.File]::WriteAllBytes($IcoPath, $ico)
}

$staging = Join-Path $env:TEMP ("anda-bot-installer-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $staging | Out-Null

Copy-Item $andaAsset (Join-Path $staging "anda.exe")
Copy-Item $launcherAsset (Join-Path $staging "anda_launcher.exe")
Copy-Item $skillsAsset (Join-Path $staging "anda-skills.zip")
Write-IcoFromPng (Join-Path $PSScriptRoot "..\anda_bot\assets\logo.png") (Join-Path $staging "anda.ico")

$pathScript = @'
param(
    [Parameter(Mandatory=$true)]
    [string]$InstallDir
)

$ErrorActionPreference = "Stop"

function Add-PathPrefix($PathValue, $Directory) {
    $normalizedDirectory = [Environment]::ExpandEnvironmentVariables($Directory).TrimEnd("\")
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($PathValue)) {
        foreach ($entry in ($PathValue -split ";")) {
            if ([string]::IsNullOrWhiteSpace($entry)) {
                continue
            }

            $normalizedEntry = [Environment]::ExpandEnvironmentVariables($entry).TrimEnd("\")
            if ([string]::Equals($normalizedEntry, $normalizedDirectory, [StringComparison]::OrdinalIgnoreCase)) {
                continue
            }

            $entries += $entry
        }
    }

    return (@($Directory) + $entries) -join ";"
}

function Send-EnvironmentChanged {
    try {
        if (-not ("AndaBot.NativeMethods" -as [type])) {
            $signature = @"
using System;
using System.Runtime.InteropServices;

namespace AndaBot {
    public static class NativeMethods {
        [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)]
        public static extern IntPtr SendMessageTimeout(
            IntPtr hWnd,
            UInt32 Msg,
            IntPtr wParam,
            string lParam,
            UInt32 fuFlags,
            UInt32 uTimeout,
            out IntPtr lpdwResult);
    }
}
"@
            Add-Type -TypeDefinition $signature | Out-Null
        }

        $result = [IntPtr]::Zero
        [AndaBot.NativeMethods]::SendMessageTimeout(
            [IntPtr]0xffff,
            0x1a,
            [IntPtr]::Zero,
            "Environment",
            0x0002,
            5000,
            [ref]$result) | Out-Null
    } catch {
    }
}

$processPath = [Environment]::GetEnvironmentVariable("Path", "Process")
$updatedProcessPath = Add-PathPrefix $processPath $InstallDir
if ($updatedProcessPath -ne $processPath) {
    [Environment]::SetEnvironmentVariable("Path", $updatedProcessPath, "Process")
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$updatedUserPath = Add-PathPrefix $userPath $InstallDir
if ($updatedUserPath -ne $userPath) {
    [Environment]::SetEnvironmentVariable("Path", $updatedUserPath, "User")
    Send-EnvironmentChanged
}
'@

Set-Content -Path (Join-Path $staging "set-user-path.ps1") -Value $pathScript -Encoding ASCII

$installScript = @'
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\AndaBot"
$AndaHome = Join-Path $env:USERPROFILE ".anda"
$StartMenuDir = Join-Path ([Environment]::GetFolderPath("Programs")) "Anda Bot"
$DesktopDir = [Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory)
$PowerShellExe = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
if (!(Test-Path -LiteralPath $PowerShellExe)) {
    $PowerShellExe = "powershell.exe"
}

function New-InstallerForm {
    $form = New-Object System.Windows.Forms.Form
    $form.Text = "Anda Bot Setup"
    $form.StartPosition = "CenterScreen"
    $form.FormBorderStyle = "FixedDialog"
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.ClientSize = New-Object System.Drawing.Size(460, 132)

    $iconPath = Join-Path $ScriptRoot "anda.ico"
    if (Test-Path -LiteralPath $iconPath) {
        try {
            $form.Icon = New-Object System.Drawing.Icon($iconPath)
        } catch {
        }
    }

    $label = New-Object System.Windows.Forms.Label
    $label.AutoSize = $false
    $label.Left = 18
    $label.Top = 18
    $label.Width = 424
    $label.Height = 42
    $label.Text = "Preparing Anda Bot..."

    $progress = New-Object System.Windows.Forms.ProgressBar
    $progress.Left = 18
    $progress.Top = 72
    $progress.Width = 424
    $progress.Height = 22
    $progress.Minimum = 0
    $progress.Maximum = 100
    $progress.Style = "Continuous"

    $form.Controls.Add($label)
    $form.Controls.Add($progress)
    $form.Show()
    [System.Windows.Forms.Application]::DoEvents()

    return @{
        Form = $form
        Label = $label
        Progress = $progress
    }
}

function Set-InstallProgress($Value, $Message) {
    $value = [Math]::Max(0, [Math]::Min(100, [int]$Value))
    $script:InstallUi.Progress.Value = $value
    $script:InstallUi.Label.Text = $Message
    [System.Windows.Forms.Application]::DoEvents()
}

function Quote-ProcessArgument($Value) {
    $text = [string]$Value
    if ($text.Length -eq 0) {
        return '""'
    }
    if ($text -notmatch '[\s"]') {
        return $text
    }

    $quoted = '"'
    $backslashes = 0
    foreach ($ch in $text.ToCharArray()) {
        if ($ch -eq '\') {
            $backslashes += 1
            continue
        }
        if ($ch -eq '"') {
            $quoted += ('\' * ($backslashes * 2 + 1))
            $quoted += '"'
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            $quoted += ('\' * $backslashes)
            $backslashes = 0
        }
        $quoted += $ch
    }
    if ($backslashes -gt 0) {
        $quoted += ('\' * ($backslashes * 2))
    }
    $quoted += '"'
    return $quoted
}

function Start-HiddenProcess($FilePath, [string[]]$ArgumentList = @(), [switch]$NoWait, [switch]$IgnoreExitCode) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $FilePath
    $psi.Arguments = ($ArgumentList | ForEach-Object { Quote-ProcessArgument $_ }) -join " "
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden

    $process = [System.Diagnostics.Process]::Start($psi)
    if ($NoWait) {
        return
    }

    $process.WaitForExit()
    if (!$IgnoreExitCode -and $process.ExitCode -ne 0) {
        throw "$FilePath failed with exit code $($process.ExitCode)."
    }
}

function Stop-ExistingAndaInstall {
    Stop-Process -Name "anda_launcher" -Force -ErrorAction SilentlyContinue
    $candidatePaths = @(
        (Join-Path $InstallDir "anda.exe"),
        (Join-Path $env:USERPROFILE "bin\anda.exe")
    )
    foreach ($candidatePath in $candidatePaths) {
        if (Test-Path -LiteralPath $candidatePath) {
            Start-HiddenProcess $candidatePath @("--home", $AndaHome, "stop") -IgnoreExitCode
        }
    }
}

function Restart-AndaDaemon {
    Start-HiddenProcess (Join-Path $InstallDir "anda.exe") @("--home", $AndaHome, "restart") -IgnoreExitCode
}

function Install-Skills($ArchivePath) {
    $skillsDir = Join-Path $AndaHome "bundled-skills"
    $tmp = Join-Path $env:TEMP ("anda-skills-" + [guid]::NewGuid().ToString("N"))
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $tmp -Force
        New-Item -ItemType Directory -Force -Path $skillsDir | Out-Null
        Copy-Item -Path (Join-Path $tmp "*") -Destination $skillsDir -Recurse -Force
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Write-Uninstaller {
    $uninstall = Join-Path $InstallDir "uninstall.cmd"
    $lines = @(
        '@echo off',
        'setlocal EnableExtensions',
        'set "INSTALL_DIR=%LOCALAPPDATA%\Programs\AndaBot"',
        'set "ANDA_HOME=%USERPROFILE%\.anda"',
        'set "START_MENU_DIR=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Anda Bot"',
        'reg.exe delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v "AndaBotLauncher" /F >nul 2>nul',
        'schtasks.exe /Delete /TN "Anda Bot" /F >nul 2>nul',
        'schtasks.exe /Delete /TN "Anda Bot Launcher" /F >nul 2>nul',
        'if exist "%INSTALL_DIR%\anda.exe" "%INSTALL_DIR%\anda.exe" --home "%ANDA_HOME%" stop >nul 2>nul',
        'taskkill.exe /IM anda_launcher.exe /F >nul 2>nul',
        'if exist "%START_MENU_DIR%" rmdir /S /Q "%START_MENU_DIR%"',
        'if exist "%USERPROFILE%\Desktop\Anda Bot.lnk" del /F /Q "%USERPROFILE%\Desktop\Anda Bot.lnk" >nul 2>nul',
        'choice.exe /M "Delete Anda data in %ANDA_HOME%?"',
        'if errorlevel 2 goto keep_data',
        'if exist "%ANDA_HOME%" rmdir /S /Q "%ANDA_HOME%"',
        ':keep_data',
        'cd /D "%TEMP%"',
        'rmdir /S /Q "%INSTALL_DIR%"'
    )
    Set-Content -Path $uninstall -Value $lines -Encoding ASCII
    return $uninstall
}

function Create-Shortcut($Path, $TargetPath, $WorkingDirectory, $IconPath, $Arguments = "") {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    $shortcut.TargetPath = $TargetPath
    $shortcut.Arguments = $Arguments
    $shortcut.WorkingDirectory = $WorkingDirectory
    if (Test-Path -LiteralPath $IconPath) {
        $shortcut.IconLocation = $IconPath
    }
    $shortcut.WindowStyle = 7
    $shortcut.Save()
}

function Create-Shortcuts {
    New-Item -ItemType Directory -Force -Path $StartMenuDir | Out-Null
    $launcher = Join-Path $InstallDir "anda_launcher.exe"
    $icon = Join-Path $InstallDir "anda.ico"
    $uninstall = Write-Uninstaller

    $targets = @(
        (Join-Path $StartMenuDir "Anda Bot.lnk"),
        (Join-Path $DesktopDir "Anda Bot.lnk")
    )
    foreach ($target in $targets) {
        $directory = Split-Path -Parent $target
        if ([string]::IsNullOrWhiteSpace($directory)) {
            continue
        }
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
        Create-Shortcut $target $launcher $InstallDir $icon
    }
    Create-Shortcut (Join-Path $StartMenuDir "Uninstall Anda Bot.lnk") $uninstall $InstallDir $icon
}

function Remove-LegacyScheduledTasks {
    Start-HiddenProcess "schtasks.exe" @("/Delete", "/TN", "Anda Bot", "/F") -IgnoreExitCode
    Start-HiddenProcess "schtasks.exe" @("/Delete", "/TN", "Anda Bot Launcher", "/F") -IgnoreExitCode
}

function Register-LauncherAutostart {
    $launcher = Join-Path $InstallDir "anda_launcher.exe"
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("Software\Microsoft\Windows\CurrentVersion\Run")
    if (-not $key) {
        throw "Could not open HKCU Run registry key."
    }
    try {
        $key.SetValue("AndaBotLauncher", ('"' + $launcher + '"'), [Microsoft.Win32.RegistryValueKind]::String)
    } finally {
        $key.Close()
    }
}

$script:InstallUi = New-InstallerForm

try {
    Set-InstallProgress 8 "Preparing folders..."
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    New-Item -ItemType Directory -Force -Path $AndaHome | Out-Null

    Set-InstallProgress 18 "Stopping any running Anda Bot processes..."
    Stop-ExistingAndaInstall

    Set-InstallProgress 34 "Installing program files..."
    Copy-Item -Force -LiteralPath (Join-Path $ScriptRoot "anda.exe") -Destination (Join-Path $InstallDir "anda.exe")
    Copy-Item -Force -LiteralPath (Join-Path $ScriptRoot "anda_launcher.exe") -Destination (Join-Path $InstallDir "anda_launcher.exe")
    Copy-Item -Force -LiteralPath (Join-Path $ScriptRoot "anda.ico") -Destination (Join-Path $InstallDir "anda.ico")

    Set-InstallProgress 48 "Updating PATH..."
    Start-HiddenProcess $PowerShellExe @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $ScriptRoot "set-user-path.ps1"), "-InstallDir", $InstallDir)

    Set-InstallProgress 62 "Installing bundled skills..."
    Install-Skills (Join-Path $ScriptRoot "anda-skills.zip")

    Set-InstallProgress 76 "Creating shortcuts..."
    Create-Shortcuts

    Set-InstallProgress 84 "Registering launch at login..."
    Remove-LegacyScheduledTasks
    Register-LauncherAutostart

    Set-InstallProgress 92 "Restarting Anda daemon..."
    Restart-AndaDaemon

    Set-InstallProgress 96 "Starting Anda Bot launcher..."
    Start-HiddenProcess (Join-Path $InstallDir "anda_launcher.exe") @() -NoWait

    Set-InstallProgress 100 "Anda Bot has been installed and restarted."
    [System.Windows.Forms.MessageBox]::Show("Anda Bot has been installed, restarted, and started in the system tray.", "Anda Bot Setup", [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
    $script:InstallUi.Form.Close()
    exit 0
} catch {
    Set-InstallProgress 100 "Installation failed."
    [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, "Anda Bot Setup", [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    $script:InstallUi.Form.Close()
    exit 1
}
'@

Set-Content -Path (Join-Path $staging "install.ps1") -Value $installScript -Encoding ASCII

$sedPath = Join-Path $staging "anda-bot.sed"
$cabPath = Join-Path $staging "anda-bot.cab"
$ddfPath = Join-Path $staging "anda-bot.ddf"
$sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=0
HideExtractAnimation=0
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=
TargetName=$outputPath
FriendlyName=Anda Bot Installer
AppLaunched=powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
SourceFiles=SourceFiles
[Strings]
FILE0=anda.exe
FILE1=anda_launcher.exe
FILE2=anda-skills.zip
FILE3=install.ps1
FILE4=set-user-path.ps1
FILE5=anda.ico
[SourceFiles]
SourceFiles0=$staging
[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
%FILE3%=
%FILE4%=
%FILE5%=
"@

Set-Content -Path $sedPath -Value $sed -Encoding ASCII
Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue

$iexpress = Join-Path $env:WINDIR "System32\iexpress.exe"
if (!(Test-Path $iexpress)) { Fail "iexpress.exe not found" }

$process = Start-Process -FilePath $iexpress -ArgumentList @("/N", "/Q", $sedPath) -Wait -PassThru
$exitCode = $process.ExitCode
if ($null -ne $exitCode -and $exitCode -ne 0) {
    Fail "iexpress.exe failed with exit code $exitCode"
}

for ($i = 0; $i -lt 10 -and !(Test-Path $outputPath); $i++) {
    Start-Sleep -Milliseconds 500
}

if (!(Test-Path $outputPath)) { Fail "Installer was not created: $outputPath" }

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $outputPath).Hash.ToLowerInvariant()
Set-Content -Path "$outputPath.sha256" -Value "$hash  $OutputName" -Encoding ASCII

Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "Created $outputPath"
