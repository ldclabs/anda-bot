# anda-bot installer for Windows PowerShell
# Usage: irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\AndaBot"),
    [string]$AndaHome = $env:ANDA_HOME,
    [switch]$NoAutostart,
    [switch]$NoStart
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Repo = "ldclabs/anda-bot"
$BinaryName = "anda"
$InstallName = "$BinaryName.exe"
$LauncherBinaryName = "anda_launcher"
$LauncherInstallName = "$LauncherBinaryName.exe"
$DaemonTaskName = "Anda Bot"
$LauncherTaskName = "Anda Bot Launcher"
$RunKeyPath = "Software\Microsoft\Windows\CurrentVersion\Run"
$LauncherRunValueName = "AndaBotLauncher"
$SkillsArchiveName = "anda-skills.zip"
$BannerArt = @(
    '      _     _   _   ____      _      '
    '     / \   | \ | | |  _ \    / \     '
    '    / _ \  |  \| | | | | |  / _ \    '
    '   / ___ \ | |\  | | |_| | / ___ \   '
    '  /_/   \_\|_| \_| |____/ /_/   \_\  '
)

$Red = "Red"
$Green = "Green"
$Cyan = "Cyan"

function Write-Info($Message) {
    Write-Host $Message -ForegroundColor $Cyan
}

function Write-Success($Message) {
    Write-Host $Message -ForegroundColor $Green
}

function Fail($Message) {
    Write-Host "Error: $Message" -ForegroundColor $Red -ErrorAction Continue
    exit 1
}

function Write-Banner {
    foreach ($line in $BannerArt) {
        Write-Host $line -ForegroundColor $Cyan
    }
    Write-Host ""
}

function Get-LatestVersion {
    $request = [System.Net.WebRequest]::Create("https://github.com/$Repo/releases/latest")
    $request.Method = "HEAD"
    $request.AllowAutoRedirect = $false
    $request.UserAgent = "anda-bot-installer"

    $response = $null
    try {
        $response = $request.GetResponse()
        $location = $response.Headers["Location"]
    } finally {
        if ($response) {
            $response.Close()
        }
    }

    if ([string]::IsNullOrWhiteSpace($location)) {
        Fail "Could not detect latest version. Check https://github.com/$Repo/releases"
    }

    return ($location.TrimEnd("/") -split "/")[-1]
}

function Get-TargetArch {
    $arch = $env:PROCESSOR_ARCHITEW6432
    if ([string]::IsNullOrWhiteSpace($arch)) {
        $arch = $env:PROCESSOR_ARCHITECTURE
    }

    switch -Regex ($arch) {
        "^(AMD64|IA64)$" { return "x86_64" }
        "^ARM64$" { return "arm64" }
        default { Fail "Unsupported architecture: $arch" }
    }
}

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

function Ensure-UserPath($Directory) {
    $processPath = [Environment]::GetEnvironmentVariable("Path", "Process")
    $updatedProcessPath = Add-PathPrefix $processPath $Directory
    if ($updatedProcessPath -ne $processPath) {
        [Environment]::SetEnvironmentVariable("Path", $updatedProcessPath, "Process")
    }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $updatedUserPath = Add-PathPrefix $userPath $Directory
    if ($updatedUserPath -eq $userPath) {
        return $false
    }

    [Environment]::SetEnvironmentVariable("Path", $updatedUserPath, "User")
    return $true
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

function Install-Binary($SourcePath, $Directory, $Name) {
    New-Item -ItemType Directory -Force -Path $Directory | Out-Null

    $installPath = Join-Path $Directory $Name
    $stagingPath = Join-Path $Directory ".$Name.$([Guid]::NewGuid().ToString('N')).tmp"

    Remove-Item -Force $stagingPath -ErrorAction SilentlyContinue
    Move-Item -Force $SourcePath $stagingPath

    try {
        Move-Item -Force $stagingPath $installPath
    } catch {
        Remove-Item -Force $stagingPath -ErrorAction SilentlyContinue
        Fail "Could not replace $installPath. If $Name is running, stop it and rerun the installer."
    }

    return $installPath
}

function Verify-Checksum($FilePath, $ChecksumPath) {
    $checksumContent = (Get-Content -Raw $ChecksumPath).Trim()
    if ([string]::IsNullOrWhiteSpace($checksumContent)) {
        Fail "Checksum file is empty: $ChecksumPath"
    }

    $expectedHash = ($checksumContent -split "\s+")[0].ToLowerInvariant()
    $actualHash = (Get-FileHash -Algorithm SHA256 $FilePath).Hash.ToLowerInvariant()

    if ($expectedHash -ne $actualHash) {
        Fail "Checksum verification failed for $(Split-Path -Leaf $FilePath)"
    }

    Write-Success "Checksum verified."
}

function Write-IcoFromPng($PngPath, $IcoPath) {
    [byte[]]$png = [System.IO.File]::ReadAllBytes($PngPath)
    [byte[]]$ico = New-Object byte[] (22 + $png.Length)
    [BitConverter]::GetBytes([UInt16]0).CopyTo($ico, 0)
    [BitConverter]::GetBytes([UInt16]1).CopyTo($ico, 2)
    [BitConverter]::GetBytes([UInt16]1).CopyTo($ico, 4)
    $ico[6] = 32
    $ico[7] = 32
    $ico[8] = 0
    $ico[9] = 0
    [BitConverter]::GetBytes([UInt16]1).CopyTo($ico, 10)
    [BitConverter]::GetBytes([UInt16]32).CopyTo($ico, 12)
    [BitConverter]::GetBytes([UInt32]$png.Length).CopyTo($ico, 14)
    [BitConverter]::GetBytes([UInt32]22).CopyTo($ico, 18)
    [Array]::Copy($png, 0, $ico, 22, $png.Length)
    [System.IO.File]::WriteAllBytes($IcoPath, $ico)
}

function Install-LauncherIcon($Version, $Directory, $TempRoot) {
    $iconPath = Join-Path $Directory "anda.ico"
    $logoPath = Join-Path $TempRoot "anda-logo.png"
    $logoUrl = "https://raw.githubusercontent.com/$Repo/$Version/anda_bot/assets/logo.png"

    try {
        Invoke-WebRequest -Uri $logoUrl -OutFile $logoPath -UseBasicParsing
        Write-IcoFromPng $logoPath $iconPath
    } catch {
        Write-Info "Launcher icon could not be installed; shortcuts may use the default Windows icon."
    }

    return $iconPath
}

function Install-Skills($ArchivePath, $HomeDir, $TempRoot) {
    $skillsDir = Join-Path $HomeDir "skills"
    $stagingDir = Join-Path $TempRoot "skills-staging"

    Remove-Item -Recurse -Force -LiteralPath $stagingDir -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null

    try {
        Expand-Archive -LiteralPath $ArchivePath -DestinationPath $stagingDir -Force
    } catch {
        Fail "Could not extract $SkillsArchiveName. $($_.Exception.Message)"
    }

    New-Item -ItemType Directory -Force -Path $skillsDir | Out-Null
    $entries = Get-ChildItem -Force -LiteralPath $stagingDir
    if (-not $entries) {
        Fail "$SkillsArchiveName is empty"
    }

    foreach ($entry in $entries) {
        $destination = Join-Path $skillsDir $entry.Name
        Remove-Item -Recurse -Force -LiteralPath $destination -ErrorAction SilentlyContinue
        Move-Item -Force -LiteralPath $entry.FullName -Destination $destination
    }

    Write-Success "Installed curated skills to $skillsDir"
}

function Stop-ExistingAndaInstall($Directory, $HomeDir) {
    Stop-Process -Name $LauncherBinaryName -Force -ErrorAction SilentlyContinue

    $candidatePaths = New-Object 'System.Collections.Generic.List[string]'
    $candidatePaths.Add((Join-Path $Directory $InstallName))
    if (-not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
        $candidatePaths.Add((Join-Path $env:USERPROFILE "bin\$InstallName"))
    }

    $seen = @{}
    foreach ($candidatePath in $candidatePaths) {
        if ([string]::IsNullOrWhiteSpace($candidatePath)) {
            continue
        }

        $normalizedPath = [Environment]::ExpandEnvironmentVariables($candidatePath)
        if ($seen.ContainsKey($normalizedPath.ToLowerInvariant())) {
            continue
        }
        $seen[$normalizedPath.ToLowerInvariant()] = $true

        if (Test-Path -LiteralPath $normalizedPath) {
            try {
                & $normalizedPath --home $HomeDir stop 2>$null | Out-Null
            } catch {
            }
        }
    }
}

function Remove-LegacyScheduledTasks {
    try {
        & schtasks.exe /Delete /TN $DaemonTaskName /F 2>$null | Out-Null
    } catch {
    }
    try {
        & schtasks.exe /Delete /TN $LauncherTaskName /F 2>$null | Out-Null
    } catch {
    }
}

function Register-LauncherAutostart($LauncherPath) {
    Remove-LegacyScheduledTasks
    $runCommand = '"' + $LauncherPath + '"'
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($RunKeyPath)
    if (-not $key) {
        Fail "Could not open HKCU Run registry key."
    }

    try {
        $key.SetValue($LauncherRunValueName, $runCommand, [Microsoft.Win32.RegistryValueKind]::String)
    } catch {
        Fail "Could not register launcher autostart. $($_.Exception.Message)"
    } finally {
        $key.Close()
    }
}

function Create-StartMenuShortcuts($InstallDir, $LauncherPath, $IconPath) {
    $shell = New-Object -ComObject WScript.Shell
    $programsDir = [Environment]::GetFolderPath("Programs")
    $shortcutDir = Join-Path $programsDir "Anda Bot"
    $desktopDir = [Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory)
    $shortcutTargets = @(
        @{ Directory = $shortcutDir; Name = "Anda Bot.lnk" },
        @{ Directory = $desktopDir; Name = "Anda Bot.lnk" }
    )

    foreach ($target in $shortcutTargets) {
        if ([string]::IsNullOrWhiteSpace($target.Directory)) {
            continue
        }
        New-Item -ItemType Directory -Force -Path $target.Directory | Out-Null

        $launcherShortcut = $shell.CreateShortcut((Join-Path $target.Directory $target.Name))
        $launcherShortcut.TargetPath = $LauncherPath
        $launcherShortcut.Arguments = ""
        $launcherShortcut.WorkingDirectory = $InstallDir
        if (Test-Path -LiteralPath $IconPath) {
            $launcherShortcut.IconLocation = $IconPath
        }
        $launcherShortcut.WindowStyle = 7
        $launcherShortcut.Save()
    }
}

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
}

if ([string]::IsNullOrWhiteSpace($AndaHome)) {
    if ([string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
        Fail "Could not detect USERPROFILE. Set -AndaHome or ANDA_HOME and rerun."
    }
    $AndaHome = Join-Path $env:USERPROFILE ".anda"
}

$target = "windows-$(Get-TargetArch)"
if ($target -ne "windows-x86_64") {
    Fail "Unsupported target: $target. Available Windows release: windows-x86_64"
}

$assetName = "$BinaryName-$target.exe"
$checksumName = "$assetName.sha256"
$launcherAssetName = "$LauncherBinaryName-$target.exe"
$launcherChecksumName = "$launcherAssetName.sha256"

Write-Banner

Write-Info "Detecting latest version..."
$version = Get-LatestVersion
Write-Info "Latest version: $version"

$url = "https://github.com/$Repo/releases/download/$version/$assetName"
$checksumUrl = "https://github.com/$Repo/releases/download/$version/$checksumName"
$launcherUrl = "https://github.com/$Repo/releases/download/$version/$launcherAssetName"
$launcherChecksumUrl = "https://github.com/$Repo/releases/download/$version/$launcherChecksumName"
$skillsUrl = "https://github.com/$Repo/releases/download/$version/$SkillsArchiveName"
$skillsChecksumName = "$SkillsArchiveName.sha256"
$skillsChecksumUrl = "https://github.com/$Repo/releases/download/$version/$skillsChecksumName"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "anda-install-$([Guid]::NewGuid().ToString('N'))"

New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

try {
    $downloadPath = Join-Path $tempDir $assetName
    $checksumPath = Join-Path $tempDir $checksumName
    $launcherDownloadPath = Join-Path $tempDir $launcherAssetName
    $launcherChecksumPath = Join-Path $tempDir $launcherChecksumName
    $skillsArchivePath = Join-Path $tempDir $SkillsArchiveName
    $skillsChecksumPath = Join-Path $tempDir $skillsChecksumName

    Write-Info "Downloading $assetName..."
    try {
        Invoke-WebRequest -Uri $url -OutFile $downloadPath -UseBasicParsing
    } catch {
        Fail "Download failed. Binary may not exist for $target.`nCheck: https://github.com/$Repo/releases/tag/$version"
    }

    try {
        Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath -UseBasicParsing
        Verify-Checksum $downloadPath $checksumPath
    } catch {
        if ($_.Exception.Message -like "Checksum verification failed*") {
            throw
        }
        Write-Info "Checksum file not found; skipping checksum verification."
    }

    Write-Info "Downloading $launcherAssetName..."
    try {
        Invoke-WebRequest -Uri $launcherUrl -OutFile $launcherDownloadPath -UseBasicParsing
    } catch {
        Fail "Download failed. Launcher binary may not exist for $target.`nCheck: https://github.com/$Repo/releases/tag/$version"
    }

    try {
        Invoke-WebRequest -Uri $launcherChecksumUrl -OutFile $launcherChecksumPath -UseBasicParsing
        Verify-Checksum $launcherDownloadPath $launcherChecksumPath
    } catch {
        if ($_.Exception.Message -like "Checksum verification failed*") {
            throw
        }
        Write-Info "Launcher checksum file not found; skipping checksum verification."
    }

    Stop-ExistingAndaInstall $InstallDir $AndaHome
    $installPath = Install-Binary $downloadPath $InstallDir $InstallName
    $launcherInstallPath = Install-Binary $launcherDownloadPath $InstallDir $LauncherInstallName
    $launcherIconPath = Install-LauncherIcon $version $InstallDir $tempDir
    Create-StartMenuShortcuts $InstallDir $launcherInstallPath $launcherIconPath

    Write-Info "Downloading $SkillsArchiveName..."
    $skillsDownloaded = $false
    try {
        Invoke-WebRequest -Uri $skillsUrl -OutFile $skillsArchivePath -UseBasicParsing
        $skillsDownloaded = $true
    } catch {
        Write-Info "Skills archive not found; skipping skills install."
    }

    if ($skillsDownloaded) {
        try {
            Invoke-WebRequest -Uri $skillsChecksumUrl -OutFile $skillsChecksumPath -UseBasicParsing
            Verify-Checksum $skillsArchivePath $skillsChecksumPath
        } catch {
            if ($_.Exception.Message -like "Checksum verification failed*") {
                throw
            }
            Write-Info "Skills checksum file not found; skipping checksum verification."
        }

        Install-Skills $skillsArchivePath $AndaHome $tempDir
    }

    $pathChanged = Ensure-UserPath $InstallDir
    if ($pathChanged) {
        Send-EnvironmentChanged
        Write-Success "Added $InstallDir to your Windows user PATH."
        Write-Info "Open a new terminal for the PATH change to take effect."
    }

    $installedVersion = "unknown"
    try {
        $installedVersion = & $installPath --version 2>$null
    } catch {
    }

    Write-Success "$InstallName installed successfully! ($installedVersion)"

    Remove-LegacyScheduledTasks

    if (-not $NoAutostart) {
        Write-Info "Registering Anda launcher to start when you log in..."
        Register-LauncherAutostart $launcherInstallPath
        Write-Success "Launcher autostart registered."
    }

    if (-not $NoStart) {
        Write-Info "Starting Anda launcher..."
        Start-Process -FilePath $launcherInstallPath -WorkingDirectory $InstallDir -WindowStyle Hidden
        Write-Success "Anda launcher started."
    }

    Write-Host ""
    Write-Host "  Manage Anda:"
    Write-Host "    $BinaryName status"
    Write-Host "    $BinaryName start"
    Write-Host "    $BinaryName stop"
    Write-Host "    $LauncherInstallName"
    Write-Host "    reg.exe query HKCU\$RunKeyPath /v $LauncherRunValueName"
    Write-Host "    $BinaryName --help"
} finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}
