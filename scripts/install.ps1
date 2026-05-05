# anda-bot installer for Windows PowerShell
# Usage: irm https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.ps1 | iex

param(
    [string]$InstallDir = (Join-Path $env:USERPROFILE "bin"),
    [string]$AndaHome = $env:ANDA_HOME
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Repo = "ldclabs/anda-bot"
$BinaryName = "anda"
$InstallName = "$BinaryName.exe"
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

function Test-PathEntry($PathValue, $Directory) {
    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return $false
    }

    $normalizedDirectory = [Environment]::ExpandEnvironmentVariables($Directory).TrimEnd("\")
    foreach ($entry in ($PathValue -split ";")) {
        if ([string]::IsNullOrWhiteSpace($entry)) {
            continue
        }

        $normalizedEntry = [Environment]::ExpandEnvironmentVariables($entry).TrimEnd("\")
        if ([string]::Equals($normalizedEntry, $normalizedDirectory, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }

    return $false
}

function Ensure-UserPath($Directory) {
    $processPath = [Environment]::GetEnvironmentVariable("Path", "Process")
    if (-not (Test-PathEntry $processPath $Directory)) {
        [Environment]::SetEnvironmentVariable("Path", "$Directory;$processPath", "Process")
    }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (Test-PathEntry $userPath $Directory) {
        return $false
    }

    if ([string]::IsNullOrWhiteSpace($userPath)) {
        [Environment]::SetEnvironmentVariable("Path", $Directory, "User")
    } else {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$Directory", "User")
    }

    return $true
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

Write-Banner

Write-Info "Detecting latest version..."
$version = Get-LatestVersion
Write-Info "Latest version: $version"

$url = "https://github.com/$Repo/releases/download/$version/$assetName"
$checksumUrl = "https://github.com/$Repo/releases/download/$version/$checksumName"
$skillsUrl = "https://github.com/$Repo/releases/download/$version/$SkillsArchiveName"
$skillsChecksumName = "$SkillsArchiveName.sha256"
$skillsChecksumUrl = "https://github.com/$Repo/releases/download/$version/$skillsChecksumName"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "anda-install-$([Guid]::NewGuid().ToString('N'))"

New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

try {
    $downloadPath = Join-Path $tempDir $assetName
    $checksumPath = Join-Path $tempDir $checksumName
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

    $installPath = Install-Binary $downloadPath $InstallDir $InstallName

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
        Write-Success "Added $InstallDir to your Windows user PATH."
        Write-Info "Open a new terminal for the PATH change to take effect."
    }

    $installedVersion = "unknown"
    try {
        $installedVersion = & $installPath --version 2>$null
    } catch {
    }

    Write-Success "$InstallName installed successfully! ($installedVersion)"
    Write-Host ""
    Write-Host "  Run Anda:"
    Write-Host '    $env:DEEPSEEK_API_KEY="****"; anda'
    Write-Host '    # or add api_key to $env:USERPROFILE\.anda\config.yaml, then:'
    Write-Host "    $BinaryName"
    Write-Host "    $BinaryName --help"
} finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}
