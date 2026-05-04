use anda_core::BoxError;
use clap::Args;
use reqwest::{StatusCode, header};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

use crate::util;

const REPO: &str = "ldclabs/anda-bot";
const BINARY_NAME: &str = "anda";
const USER_AGENT: &str = "anda-bot-updater";

#[derive(Args)]
pub struct UpdateCommand {
    /// Reinstall the latest release even when this binary is already current.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReleaseTarget {
    os: &'static str,
    arch: &'static str,
    exe_ext: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateFinish {
    Installed,
    #[cfg(windows)]
    Scheduled,
}

struct StagedFile {
    path: PathBuf,
    keep: bool,
}

impl StagedFile {
    fn new(path: PathBuf) -> Self {
        Self { path, keep: false }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(windows)]
    fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl ReleaseTarget {
    fn detect() -> Result<Self, BoxError> {
        Self::from_parts(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
            let target = normalized_target_name(std::env::consts::OS, std::env::consts::ARCH);
            format!(
                "Unsupported target: {target}. Available releases: linux-x86_64, linux-arm64, windows-x86_64, macos-x86_64, macos-arm64"
            )
            .into()
        })
    }

    fn from_parts(os: &str, arch: &str) -> Option<Self> {
        let arch = match arch {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "arm64",
            _ => return None,
        };

        match (os, arch) {
            ("linux", "x86_64" | "arm64") => Some(Self {
                os: "linux",
                arch,
                exe_ext: "",
            }),
            ("macos", "x86_64" | "arm64") => Some(Self {
                os: "macos",
                arch,
                exe_ext: "",
            }),
            ("windows", "x86_64") => Some(Self {
                os: "windows",
                arch,
                exe_ext: ".exe",
            }),
            _ => None,
        }
    }

    fn name(&self) -> String {
        format!("{}-{}", self.os, self.arch)
    }

    fn asset_name(&self) -> String {
        format!("{BINARY_NAME}-{}{}", self.name(), self.exe_ext)
    }
}

pub async fn run(cmd: &UpdateCommand) -> Result<(), BoxError> {
    let current_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let target = ReleaseTarget::detect()?;
    let client =
        util::http_client::build_http_client(None, |client| client.user_agent(USER_AGENT))?;

    println!("Detecting latest version...");
    let latest_tag = fetch_latest_version(&client).await?;
    println!("Latest version: {latest_tag}");

    if !cmd.force && latest_tag == current_tag {
        println!("anda is already up to date ({current_tag}).");
        return Ok(());
    }

    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or("Could not detect the current executable directory")?
        .to_path_buf();
    let asset_name = target.asset_name();
    let checksum_name = format!("{asset_name}.sha256");
    let base_url = format!("https://github.com/{REPO}/releases/download/{latest_tag}");
    let asset_url = format!("{base_url}/{asset_name}");
    let checksum_url = format!("{base_url}/{checksum_name}");
    let download = StagedFile::new(temporary_download_path(&asset_name));
    let staged_path = staged_update_path(&install_dir, &current_exe);
    #[cfg(windows)]
    let mut staged = StagedFile::new(staged_path);
    #[cfg(not(windows))]
    let staged = StagedFile::new(staged_path);

    println!("Downloading {asset_name}...");
    let actual_hash = download_binary(&client, &asset_url, download.path()).await?;

    match fetch_expected_checksum(&client, &checksum_url).await? {
        Some(expected_hash) => verify_checksum(&asset_name, &expected_hash, &actual_hash)?,
        None => println!("Checksum file not found; skipping checksum verification."),
    }

    stage_update(download.path(), staged.path()).await?;
    prepare_executable(staged.path(), &current_exe).await?;
    match install_update(staged.path(), &current_exe).await? {
        UpdateFinish::Installed => {
            println!("anda updated from {current_tag} to {latest_tag}.");
            println!(
                "If the daemon is running, restart it with `anda restart` to use the new version."
            );
        }
        #[cfg(windows)]
        UpdateFinish::Scheduled => {
            staged.keep();
            println!("Update staged for {latest_tag}.");
            println!("The Windows helper will replace anda after this process exits.");
            println!("If replacement fails, close running anda processes and rerun `anda update`.");
        }
    }

    Ok(())
}

async fn fetch_latest_version(client: &reqwest::Client) -> Result<String, BoxError> {
    let url = format!("https://github.com/{REPO}/releases/latest");

    if let Ok(response) = client.head(&url).send().await
        && let Some(version) = release_version_from_response(&response)
    {
        return Ok(version);
    }

    let response = client.get(&url).send().await?;
    if let Some(version) = release_version_from_response(&response) {
        return Ok(version);
    }

    Err(format!(
        "Could not detect latest version (HTTP {}). Check https://github.com/{REPO}/releases",
        response.status()
    )
    .into())
}

fn release_version_from_response(response: &reqwest::Response) -> Option<String> {
    if let Some(version) = response
        .headers()
        .get(header::LOCATION)
        .and_then(|location| location.to_str().ok())
        .and_then(release_version_from_location)
    {
        return Some(version);
    }

    release_version_from_location(response.url().as_str())
}

fn release_version_from_location(location: &str) -> Option<String> {
    let location = location.trim();
    let tag = location
        .split_once("/releases/tag/")?
        .1
        .trim_end_matches('/')
        .split(['/', '?', '#'])
        .next()?
        .trim();
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
}

async fn download_binary(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> Result<String, BoxError> {
    let mut response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed (HTTP {}). Binary may not exist for this platform. Check: {url}",
            response.status()
        )
        .into());
    }

    let mut file = tokio::fs::File::create(destination).await?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;

    Ok(hex_lower(&hasher.finalize()))
}

async fn fetch_expected_checksum(
    client: &reqwest::Client,
    url: &str,
) -> Result<Option<String>, BoxError> {
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(_) => return Ok(None),
    };

    if response.status() == StatusCode::NOT_FOUND || !response.status().is_success() {
        return Ok(None);
    }

    let content = response.text().await?;
    Ok(Some(expected_hash_from_checksum(&content)?))
}

fn expected_hash_from_checksum(content: &str) -> Result<String, BoxError> {
    let expected_hash = content
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if expected_hash.is_empty() {
        return Err("Checksum file is empty".into());
    }
    if expected_hash.len() != 64 || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Checksum file does not contain a valid SHA-256 hash".into());
    }

    Ok(expected_hash)
}

async fn stage_update(download_path: &Path, staged_path: &Path) -> Result<(), BoxError> {
    tokio::fs::copy(download_path, staged_path)
        .await
        .map(|_| ())
        .map_err(|err| format!("Could not stage update at {}: {err}", staged_path.display()).into())
}

fn verify_checksum(
    asset_name: &str,
    expected_hash: &str,
    actual_hash: &str,
) -> Result<(), BoxError> {
    if expected_hash != actual_hash {
        return Err(format!("Checksum verification failed for {asset_name}").into());
    }

    println!("Checksum verified.");
    Ok(())
}

async fn prepare_executable(staged_path: &Path, current_exe: &Path) -> Result<(), BoxError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let current_mode = tokio::fs::metadata(current_exe)
            .await
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0o755);
        let mut permissions = tokio::fs::metadata(staged_path).await?.permissions();
        permissions.set_mode(current_mode | 0o111);
        tokio::fs::set_permissions(staged_path, permissions).await?;
    }

    #[cfg(not(unix))]
    {
        let _ = (staged_path, current_exe);
    }

    Ok(())
}

#[cfg(not(windows))]
async fn install_update(staged_path: &Path, current_exe: &Path) -> Result<UpdateFinish, BoxError> {
    tokio::fs::rename(staged_path, current_exe)
        .await
        .map_err(|err| format!("Could not replace {}: {err}", current_exe.display()))?;
    Ok(UpdateFinish::Installed)
}

#[cfg(windows)]
async fn install_update(staged_path: &Path, current_exe: &Path) -> Result<UpdateFinish, BoxError> {
    use std::process::{Command, Stdio};

    let source = powershell_single_quoted_path(staged_path);
    let target = powershell_single_quoted_path(current_exe);
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         $source = {source}; \
         $target = {target}; \
         $deadline = (Get-Date).AddSeconds(60); \
         while ($true) {{ \
             try {{ Move-Item -Force -LiteralPath $source -Destination $target; break }} \
             catch {{ \
                 if ((Get-Date) -gt $deadline) {{ \
                     Write-Error \"Could not replace $target. Close running anda processes and rerun anda update.\"; \
                     exit 1 \
                 }}; \
                 Start-Sleep -Milliseconds 500 \
             }} \
         }}"
    );

    Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("Could not start Windows update helper: {err}"))?;

    Ok(UpdateFinish::Scheduled)
}

#[cfg(windows)]
fn powershell_single_quoted_path(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

fn staged_update_path(install_dir: &Path, current_exe: &Path) -> PathBuf {
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(BINARY_NAME);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    install_dir.join(format!(
        ".{file_name}.update-{}-{nanos}.tmp",
        std::process::id()
    ))
}

fn temporary_download_path(asset_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    std::env::temp_dir().join(format!(
        "{asset_name}.download-{}-{nanos}.tmp",
        std::process::id()
    ))
}

fn normalized_target_name(os: &str, arch: &str) -> String {
    let arch = match arch {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);

    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_target_matches_published_assets() {
        assert_eq!(
            ReleaseTarget::from_parts("linux", "x86_64")
                .unwrap()
                .asset_name(),
            "anda-linux-x86_64"
        );
        assert_eq!(
            ReleaseTarget::from_parts("linux", "aarch64")
                .unwrap()
                .asset_name(),
            "anda-linux-arm64"
        );
        assert_eq!(
            ReleaseTarget::from_parts("macos", "arm64")
                .unwrap()
                .asset_name(),
            "anda-macos-arm64"
        );
        assert_eq!(
            ReleaseTarget::from_parts("windows", "x86_64")
                .unwrap()
                .asset_name(),
            "anda-windows-x86_64.exe"
        );
        assert!(ReleaseTarget::from_parts("windows", "arm64").is_none());
    }

    #[test]
    fn checksum_parser_reads_first_sha256_field() {
        let checksum =
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  anda-linux-x86_64\n";

        assert_eq!(
            expected_hash_from_checksum(checksum).unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn checksum_parser_rejects_invalid_content() {
        assert!(expected_hash_from_checksum("").is_err());
        assert!(expected_hash_from_checksum("not-a-sha256 anda").is_err());
    }

    #[test]
    fn release_version_parser_reads_redirect_location() {
        assert_eq!(
            release_version_from_location(
                "https://github.com/ldclabs/anda-bot/releases/tag/v0.4.0"
            ),
            Some("v0.4.0".to_string())
        );
        assert_eq!(
            release_version_from_location("https://github.com/ldclabs/anda-bot/releases/latest"),
            None
        );
    }
}
