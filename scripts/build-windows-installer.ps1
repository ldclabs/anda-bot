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

$staging = Join-Path $env:TEMP ("anda-bot-installer-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $staging | Out-Null

Copy-Item $andaAsset (Join-Path $staging "anda.exe")
Copy-Item $launcherAsset (Join-Path $staging "anda_launcher.exe")
Copy-Item $skillsAsset (Join-Path $staging "anda-skills.zip")

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

$installCmd = @'
@echo off
setlocal EnableExtensions

set "INSTALL_DIR=%LOCALAPPDATA%\Programs\AndaBot"
set "ANDA_HOME=%USERPROFILE%\.anda"
set "START_MENU_DIR=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Anda Bot"

if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"
if not exist "%ANDA_HOME%" mkdir "%ANDA_HOME%"
if not exist "%START_MENU_DIR%" mkdir "%START_MENU_DIR%"

taskkill.exe /IM anda_launcher.exe /F >nul 2>nul
if exist "%INSTALL_DIR%\anda.exe" "%INSTALL_DIR%\anda.exe" --home "%ANDA_HOME%" stop >nul 2>nul
if exist "%USERPROFILE%\bin\anda.exe" "%USERPROFILE%\bin\anda.exe" --home "%ANDA_HOME%" stop >nul 2>nul

copy /Y "%~dp0anda.exe" "%INSTALL_DIR%\anda.exe" >nul
if errorlevel 1 exit /b 1
copy /Y "%~dp0anda_launcher.exe" "%INSTALL_DIR%\anda_launcher.exe" >nul
if errorlevel 1 exit /b 1

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0set-user-path.ps1" -InstallDir "%INSTALL_DIR%"
if errorlevel 1 exit /b 1

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "try { $skillsZip = '%~dp0anda-skills.zip'; $dest = Join-Path $env:USERPROFILE '.anda\skills'; New-Item -ItemType Directory -Force -Path $dest | Out-Null; $tmp = Join-Path $env:TEMP ('anda-skills-' + [guid]::NewGuid().ToString('N')); Expand-Archive -LiteralPath $skillsZip -DestinationPath $tmp -Force; Copy-Item -Path (Join-Path $tmp '*') -Destination $dest -Recurse -Force; Remove-Item -LiteralPath $tmp -Recurse -Force } catch { Write-Host $_; exit 1 }"

set "UNINSTALL=%INSTALL_DIR%\uninstall.cmd"
(
  echo @echo off
  echo setlocal EnableExtensions
  echo set "INSTALL_DIR=%%LOCALAPPDATA%%\Programs\AndaBot"
  echo set "ANDA_HOME=%%USERPROFILE%%\.anda"
  echo set "START_MENU_DIR=%%APPDATA%%\Microsoft\Windows\Start Menu\Programs\Anda Bot"
  echo reg.exe delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v "AndaBotLauncher" /F ^>nul 2^>nul
  echo schtasks.exe /Delete /TN "Anda Bot" /F ^>nul 2^>nul
  echo schtasks.exe /Delete /TN "Anda Bot Launcher" /F ^>nul 2^>nul
  echo if exist "%%INSTALL_DIR%%\anda.exe" "%%INSTALL_DIR%%\anda.exe" --home "%%ANDA_HOME%%" stop ^>nul 2^>nul
  echo taskkill.exe /IM anda_launcher.exe /F ^>nul 2^>nul
  echo if exist "%%START_MENU_DIR%%" rmdir /S /Q "%%START_MENU_DIR%%"
  echo choice.exe /M "Delete Anda data in %%ANDA_HOME%%?"
  echo if errorlevel 2 goto keep_data
  echo if exist "%%ANDA_HOME%%" rmdir /S /Q "%%ANDA_HOME%%"
  echo :keep_data
  echo cd /D "%%TEMP%%"
  echo rmdir /S /Q "%%INSTALL_DIR%%"
) > "%UNINSTALL%"

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$w = New-Object -ComObject WScript.Shell; $dir = [Environment]::GetFolderPath('Programs') + '\Anda Bot'; New-Item -ItemType Directory -Force -Path $dir | Out-Null; $s = $w.CreateShortcut((Join-Path $dir 'Anda Bot.lnk')); $s.TargetPath = Join-Path $env:LOCALAPPDATA 'Programs\AndaBot\anda_launcher.exe'; $s.WorkingDirectory = Join-Path $env:LOCALAPPDATA 'Programs\AndaBot'; $s.IconLocation = $s.TargetPath; $s.Save(); $u = $w.CreateShortcut((Join-Path $dir 'Uninstall Anda Bot.lnk')); $u.TargetPath = Join-Path $env:LOCALAPPDATA 'Programs\AndaBot\uninstall.cmd'; $u.WorkingDirectory = Join-Path $env:LOCALAPPDATA 'Programs\AndaBot'; $u.Save()"

schtasks.exe /Delete /TN "Anda Bot" /F >nul 2>nul
schtasks.exe /Delete /TN "Anda Bot Launcher" /F >nul 2>nul
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "try { $launcher = Join-Path $env:LOCALAPPDATA 'Programs\AndaBot\anda_launcher.exe'; $andaHome = Join-Path $env:USERPROFILE '.anda'; $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Windows\CurrentVersion\Run'); if (-not $key) { throw 'Could not open HKCU Run registry key' }; try { $key.SetValue('AndaBotLauncher', (([char]34) + $launcher + ([char]34) + ' --home ' + ([char]34) + $andaHome + ([char]34)), [Microsoft.Win32.RegistryValueKind]::String) } finally { $key.Close() } } catch { Write-Host $_; exit 1 }"
if errorlevel 1 exit /b 1

start "" "%INSTALL_DIR%\anda_launcher.exe"

echo Anda Bot has been installed.
endlocal
'@

Set-Content -Path (Join-Path $staging "install.cmd") -Value $installCmd -Encoding ASCII

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
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=
DisplayLicense=
FinishMessage=Anda Bot has been installed.
TargetName=$outputPath
FriendlyName=Anda Bot Installer
AppLaunched=install.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
SourceFiles=SourceFiles
[Strings]
FILE0=anda.exe
FILE1=anda_launcher.exe
FILE2=anda-skills.zip
FILE3=install.cmd
FILE4=set-user-path.ps1
[SourceFiles]
SourceFiles0=$staging
[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
%FILE3%=
%FILE4%=
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
