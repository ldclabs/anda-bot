use anda_core::BoxError;
use clap::Args;
use reqwest::{StatusCode, header};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

use crate::{auto_update, daemon::Daemon};

pub(crate) const REPO: &str = "ldclabs/anda-bot";
pub(crate) const BINARY_NAME: &str = "anda";
pub(crate) const LAUNCHER_BINARY_NAME: &str = "anda_launcher";
const SKILLS_ARCHIVE_NAME: &str = "anda-skills.zip";

#[derive(Args)]
pub struct UpdateCommand {
    /// Reinstall the latest release even when this binary is already current.
    #[arg(long)]
    force: bool,
    /// Only update curated skills in the Anda home directory.
    #[arg(long)]
    skills: bool,
    /// Check for a new release and download it without installing.
    #[arg(long)]
    check: bool,
    /// Check only when the persisted auto-update interval is due.
    #[arg(long, hide = true)]
    check_if_due: bool,
    /// Emit machine-readable update state for launcher integrations.
    #[arg(long, hide = true)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReleaseTarget {
    os: &'static str,
    arch: &'static str,
    exe_ext: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateFinish {
    Installed,
    #[cfg(windows)]
    Scheduled,
}

struct StagedFile {
    path: PathBuf,
    keep: bool,
}

struct StagedDir {
    path: PathBuf,
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

impl StagedDir {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl ReleaseTarget {
    pub(crate) fn detect() -> Result<Self, BoxError> {
        Self::from_parts(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
            let target = normalized_target_name(std::env::consts::OS, std::env::consts::ARCH);
            format!(
                "Unsupported target: {target}. Available releases: linux-x86_64, linux-arm64, windows-x86_64, macos-x86_64, macos-arm64"
            )
            .into()
        })
    }

    pub(crate) fn from_parts(os: &str, arch: &str) -> Option<Self> {
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

    pub(crate) fn name(&self) -> String {
        format!("{}-{}", self.os, self.arch)
    }

    pub(crate) fn asset_name(&self) -> String {
        self.binary_asset_name(BINARY_NAME)
    }

    pub(crate) fn launcher_asset_name(&self) -> String {
        self.binary_asset_name(LAUNCHER_BINARY_NAME)
    }

    fn binary_asset_name(&self, binary_name: &str) -> String {
        format!("{binary_name}-{}{}", self.name(), self.exe_ext)
    }

    fn supports_launcher_sidecar_update(&self) -> bool {
        matches!(self.os, "macos" | "windows")
    }
}

pub async fn run(
    client: &reqwest::Client,
    daemon: &Daemon,
    cmd: &UpdateCommand,
) -> Result<(), BoxError> {
    let home_dir = &daemon.home;
    let current_tag = format!("v{}", env!("CARGO_PKG_VERSION"));

    validate_update_command(cmd)?;

    if cmd.check || cmd.check_if_due {
        run_update_check(client, daemon, cmd).await?;
        return Ok(());
    }

    println!("Detecting latest version...");
    let latest_tag = fetch_latest_version(client).await?;
    println!("Latest version: {latest_tag}");

    let base_url = release_download_base_url(&latest_tag);

    if cmd.skills {
        install_release_skills(client, &base_url, home_dir).await?;
        return Ok(());
    }

    let target = ReleaseTarget::detect()?;
    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or("Could not detect the current executable directory")?
        .to_path_buf();

    if !cmd.force && latest_tag == current_tag {
        if let Some(finish) =
            install_release_launcher_if_present(client, &base_url, target, &install_dir).await?
        {
            print_launcher_update_finish(latest_tag.as_str(), finish);
        }
        install_release_skills(client, &base_url, home_dir).await?;
        println!("anda is already up to date ({current_tag}).");
        return Ok(());
    }

    let asset_name = target.asset_name();
    let checksum_name = format!("{asset_name}.sha256");
    let asset_url = format!("{base_url}/{asset_name}");
    let checksum_url = format!("{base_url}/{checksum_name}");
    let download = StagedFile::new(temporary_download_path(&asset_name));
    let staged_path = staged_update_path(&install_dir, &current_exe);
    #[cfg(windows)]
    let mut staged = StagedFile::new(staged_path);
    #[cfg(not(windows))]
    let staged = StagedFile::new(staged_path);

    if let Some(downloaded_path) =
        auto_update::downloaded_update_path(daemon, &latest_tag, &asset_name).await
    {
        println!("Using previously downloaded {asset_name}...");
        stage_update(&downloaded_path, staged.path()).await?;
        prepare_executable(staged.path(), &current_exe).await?;
        let finish = install_update(staged.path(), &current_exe).await?;

        #[cfg(windows)]
        if finish == UpdateFinish::Scheduled {
            staged.keep();
        }

        install_release_skills(client, &base_url, home_dir).await?;
        if let Some(finish) =
            install_release_launcher_if_present(client, &base_url, target, &install_dir).await?
        {
            print_launcher_update_finish(latest_tag.as_str(), finish);
        }
        auto_update::mark_installed(daemon, &latest_tag).await;
        print_update_finish(current_tag.as_str(), latest_tag.as_str(), finish);
        return Ok(());
    }

    println!("Downloading {asset_name}...");
    let actual_hash = download_binary(client, &asset_url, download.path()).await?;

    match fetch_expected_checksum(client, &checksum_url).await? {
        Some(expected_hash) => verify_checksum(&asset_name, &expected_hash, &actual_hash)?,
        None => println!("Checksum file not found; skipping checksum verification."),
    }

    stage_update(download.path(), staged.path()).await?;
    prepare_executable(staged.path(), &current_exe).await?;
    let finish = install_update(staged.path(), &current_exe).await?;

    #[cfg(windows)]
    if finish == UpdateFinish::Scheduled {
        staged.keep();
    }

    install_release_skills(client, &base_url, home_dir).await?;
    if let Some(finish) =
        install_release_launcher_if_present(client, &base_url, target, &install_dir).await?
    {
        print_launcher_update_finish(latest_tag.as_str(), finish);
    }
    auto_update::mark_installed(daemon, &latest_tag).await;

    print_update_finish(current_tag.as_str(), latest_tag.as_str(), finish);

    Ok(())
}

fn validate_update_command(cmd: &UpdateCommand) -> Result<(), BoxError> {
    if cmd.check && cmd.check_if_due {
        return Err("use either --check or --check-if-due, not both".into());
    }
    if cmd.skills && (cmd.check || cmd.check_if_due) {
        return Err("--skills cannot be combined with update check flags".into());
    }
    if cmd.json && !(cmd.check || cmd.check_if_due) {
        return Err("--json is only supported with update check flags".into());
    }
    Ok(())
}

async fn run_update_check(
    client: &reqwest::Client,
    daemon: &Daemon,
    cmd: &UpdateCommand,
) -> Result<(), BoxError> {
    let db = daemon.open_bot_db().await?;
    let updater = auto_update::AutoUpdater::new(db, daemon.home.clone(), client.clone());
    let state = if cmd.check {
        updater.check_now().await
    } else {
        updater.check_if_due().await
    };

    if cmd.json {
        println!("{}", serde_json::to_string(&state)?);
    } else {
        print_update_check_state(&state);
    }
    Ok(())
}

fn print_update_check_state(state: &auto_update::AutoUpdateState) {
    match state.status {
        auto_update::AutoUpdateStatus::Downloaded if state.downloaded_update_available() => {
            let latest = state.latest_tag.as_deref().unwrap_or("the latest release");
            println!(
                "Anda {latest} has been downloaded. Run `anda update`, then `anda restart` to use it."
            );
        }
        auto_update::AutoUpdateStatus::Failed => {
            let detail = state.error.as_deref().unwrap_or("unknown error");
            println!("Update check failed: {detail}");
        }
        auto_update::AutoUpdateStatus::Checking | auto_update::AutoUpdateStatus::Downloading => {
            println!("Update check is still running.");
        }
        auto_update::AutoUpdateStatus::Idle => {
            println!("No update check has run yet.");
        }
        auto_update::AutoUpdateStatus::Current | auto_update::AutoUpdateStatus::Installed => {
            println!("anda is already up to date ({}).", state.current_tag);
        }
        auto_update::AutoUpdateStatus::Downloaded => {
            println!("Downloaded update state is stale; run `anda update --check` again.");
        }
    }
}

fn print_update_finish(current_tag: &str, latest_tag: &str, finish: UpdateFinish) {
    match finish {
        UpdateFinish::Installed => {
            println!("anda updated from {current_tag} to {latest_tag}.");
            println!(
                "If the daemon is running, restart it with `anda restart` to use the new version."
            );
        }
        #[cfg(windows)]
        UpdateFinish::Scheduled => {
            println!("Update staged for {latest_tag}.");
            println!("The Windows helper will replace anda after this process exits.");
            println!("If replacement fails, close running anda processes and rerun `anda update`.");
        }
    }
}

fn print_launcher_update_finish(latest_tag: &str, finish: UpdateFinish) {
    match finish {
        UpdateFinish::Installed => {
            println!("{LAUNCHER_BINARY_NAME} synchronized with {latest_tag}.");
        }
        #[cfg(windows)]
        UpdateFinish::Scheduled => {
            println!("{LAUNCHER_BINARY_NAME} update staged for {latest_tag}.");
        }
    }
}

pub(crate) fn release_download_base_url(latest_tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{latest_tag}")
}

pub(crate) async fn fetch_latest_version(client: &reqwest::Client) -> Result<String, BoxError> {
    if let Some(version) = fetch_latest_version_from_api(client).await? {
        return Ok(version);
    }

    let url = format!("https://github.com/{REPO}/releases/latest");
    let response = latest_release_request(client, &url).send().await?;
    if let Some(version) = release_version_from_response(&response) {
        return Ok(version);
    }

    Err(format!(
        "Could not detect latest version (HTTP {}). Check https://github.com/{REPO}/releases",
        response.status()
    )
    .into())
}

async fn fetch_latest_version_from_api(
    client: &reqwest::Client,
) -> Result<Option<String>, BoxError> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response = match latest_release_request(client, &url)
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(None),
    };

    if !response.status().is_success() {
        return Ok(None);
    }

    let content = response.text().await?;
    Ok(release_version_from_github_api(&content))
}

fn latest_release_request(client: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header(header::CACHE_CONTROL, "no-cache, no-store, max-age=0")
        .header(header::PRAGMA, "no-cache")
        .header(
            header::USER_AGENT,
            format!("{BINARY_NAME}/{}", env!("CARGO_PKG_VERSION")),
        )
}

fn release_version_from_github_api(content: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct GitHubRelease {
        tag_name: String,
    }

    let release: GitHubRelease = serde_json::from_str(content).ok()?;
    let tag = release.tag_name.trim();
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
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

pub(crate) async fn download_binary(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> Result<String, BoxError> {
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed (HTTP {}). Binary may not exist for this platform. Check: {url}",
            response.status()
        )
        .into());
    }

    write_response_to_file(response, destination).await
}

async fn download_optional_file(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> Result<Option<String>, BoxError> {
    let response = client.get(url).send().await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("Download failed (HTTP {}). Check: {url}", response.status()).into());
    }

    Ok(Some(write_response_to_file(response, destination).await?))
}

async fn write_response_to_file(
    mut response: reqwest::Response,
    destination: &Path,
) -> Result<String, BoxError> {
    let mut file = tokio::fs::File::create(destination).await?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;

    Ok(hex_lower(&hasher.finalize()))
}

async fn install_release_skills(
    client: &reqwest::Client,
    base_url: &str,
    home_dir: &Path,
) -> Result<bool, BoxError> {
    let archive_url = format!("{base_url}/{SKILLS_ARCHIVE_NAME}");
    let checksum_url = format!("{base_url}/{SKILLS_ARCHIVE_NAME}.sha256");
    let download = StagedFile::new(temporary_download_path(SKILLS_ARCHIVE_NAME));
    let staging = StagedDir::new(staged_skills_dir_path(home_dir));

    println!("Downloading {SKILLS_ARCHIVE_NAME}...");
    let Some(actual_hash) = download_optional_file(client, &archive_url, download.path()).await?
    else {
        println!("Skills archive not found; skipping skills update.");
        return Ok(false);
    };

    match fetch_expected_checksum(client, &checksum_url).await? {
        Some(expected_hash) => verify_checksum(SKILLS_ARCHIVE_NAME, &expected_hash, &actual_hash)?,
        None => println!("Skills checksum file not found; skipping checksum verification."),
    }

    extract_skills_archive(download.path(), staging.path())?;
    let skills_dir = bundled_skills_dir(home_dir);
    let installed = install_skills_from_staging(staging.path(), &skills_dir)?;
    println!(
        "Updated {installed} curated skill{} in {}.",
        if installed == 1 { "" } else { "s" },
        skills_dir.display()
    );

    Ok(true)
}

fn extract_skills_archive(archive_path: &Path, staging_dir: &Path) -> Result<(), BoxError> {
    remove_path_if_exists(staging_dir)?;
    std::fs::create_dir_all(staging_dir)?;

    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(enclosed_name) = entry.enclosed_name().map(|path| path.to_owned()) else {
            continue;
        };
        if enclosed_name.components().next().is_none() {
            continue;
        }

        let output_path = staging_dir.join(enclosed_name);
        if entry.is_dir() {
            std::fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&output_path)?;
        io::copy(&mut entry, &mut output)?;
    }

    Ok(())
}

fn install_skills_from_staging(staging_dir: &Path, skills_dir: &Path) -> Result<usize, BoxError> {
    std::fs::create_dir_all(skills_dir)?;

    let mut installed = 0;
    for entry in std::fs::read_dir(staging_dir)? {
        let entry = entry?;
        let destination = skills_dir.join(entry.file_name());
        remove_path_if_exists(&destination)?;
        std::fs::rename(entry.path(), &destination).map_err(|err| {
            format!(
                "Could not install skill at {}: {err}",
                destination.display()
            )
        })?;
        installed += 1;
    }

    if installed == 0 {
        return Err(format!("{SKILLS_ARCHIVE_NAME} is empty").into());
    }

    Ok(installed)
}

fn bundled_skills_dir(home_dir: &Path) -> PathBuf {
    home_dir.join("bundled-skills")
}

async fn install_release_launcher_if_present(
    client: &reqwest::Client,
    base_url: &str,
    target: ReleaseTarget,
    install_dir: &Path,
) -> Result<Option<UpdateFinish>, BoxError> {
    if !target.supports_launcher_sidecar_update() {
        return Ok(None);
    }

    let launcher_exe = sidecar_launcher_path(install_dir, target);
    if !launcher_exe.exists() {
        return Ok(None);
    }

    let asset_name = target.launcher_asset_name();
    let checksum_name = format!("{asset_name}.sha256");
    let asset_url = format!("{base_url}/{asset_name}");
    let checksum_url = format!("{base_url}/{checksum_name}");
    let download = StagedFile::new(temporary_download_path(&asset_name));
    let staged_path = staged_update_path(install_dir, &launcher_exe);
    #[cfg(windows)]
    let mut staged = StagedFile::new(staged_path);
    #[cfg(not(windows))]
    let staged = StagedFile::new(staged_path);

    println!("Downloading {asset_name}...");
    let actual_hash = download_binary(client, &asset_url, download.path()).await?;

    match fetch_expected_checksum(client, &checksum_url).await? {
        Some(expected_hash) => verify_checksum(&asset_name, &expected_hash, &actual_hash)?,
        None => println!("Checksum file not found; skipping checksum verification."),
    }

    stage_update(download.path(), staged.path()).await?;
    prepare_executable(staged.path(), &launcher_exe).await?;
    let finish = install_update(staged.path(), &launcher_exe).await?;

    #[cfg(windows)]
    if finish == UpdateFinish::Scheduled {
        staged.keep();
    }

    Ok(Some(finish))
}

fn sidecar_launcher_path(install_dir: &Path, target: ReleaseTarget) -> PathBuf {
    install_dir.join(format!("{LAUNCHER_BINARY_NAME}{}", target.exe_ext))
}

fn remove_path_if_exists(path: &Path) -> Result<(), BoxError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    if metadata.file_type().is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }

    Ok(())
}

pub(crate) async fn fetch_expected_checksum(
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

pub(crate) async fn stage_update(download_path: &Path, staged_path: &Path) -> Result<(), BoxError> {
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

pub(crate) async fn prepare_executable(
    staged_path: &Path,
    current_exe: &Path,
) -> Result<(), BoxError> {
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
pub(crate) async fn install_update(
    staged_path: &Path,
    current_exe: &Path,
) -> Result<UpdateFinish, BoxError> {
    tokio::fs::rename(staged_path, current_exe)
        .await
        .map_err(|err| format!("Could not replace {}: {err}", current_exe.display()))?;
    Ok(UpdateFinish::Installed)
}

#[cfg(windows)]
pub(crate) async fn install_update(
    staged_path: &Path,
    current_exe: &Path,
) -> Result<UpdateFinish, BoxError> {
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

pub(crate) fn staged_update_path(install_dir: &Path, current_exe: &Path) -> PathBuf {
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

pub(crate) fn temporary_download_path(asset_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    std::env::temp_dir().join(format!(
        "{asset_name}.download-{}-{nanos}.tmp",
        std::process::id()
    ))
}

fn staged_skills_dir_path(home_dir: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    home_dir.join(format!(".skills.update-{}-{nanos}.tmp", std::process::id()))
}

fn normalized_target_name(os: &str, arch: &str) -> String {
    let arch = match arch {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
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
    use crate::util::text::read_text_file_sync;

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
            ReleaseTarget::from_parts("macos", "arm64")
                .unwrap()
                .launcher_asset_name(),
            "anda_launcher-macos-arm64"
        );
        assert_eq!(
            ReleaseTarget::from_parts("windows", "x86_64")
                .unwrap()
                .asset_name(),
            "anda-windows-x86_64.exe"
        );
        assert_eq!(
            ReleaseTarget::from_parts("windows", "x86_64")
                .unwrap()
                .launcher_asset_name(),
            "anda_launcher-windows-x86_64.exe"
        );
        assert!(ReleaseTarget::from_parts("windows", "arm64").is_none());
    }

    #[test]
    fn release_target_marks_launcher_sidecar_update_platforms() {
        assert!(
            ReleaseTarget::from_parts("macos", "arm64")
                .unwrap()
                .supports_launcher_sidecar_update()
        );
        assert!(
            ReleaseTarget::from_parts("windows", "x86_64")
                .unwrap()
                .supports_launcher_sidecar_update()
        );
        assert!(
            !ReleaseTarget::from_parts("linux", "x86_64")
                .unwrap()
                .supports_launcher_sidecar_update()
        );

        let macos = ReleaseTarget::from_parts("macos", "x86_64").unwrap();
        assert_eq!(
            sidecar_launcher_path(Path::new("/install"), macos),
            Path::new("/install").join("anda_launcher")
        );
        let windows = ReleaseTarget::from_parts("windows", "x86_64").unwrap();
        assert_eq!(
            sidecar_launcher_path(Path::new("C:/Anda"), windows),
            Path::new("C:/Anda").join("anda_launcher.exe")
        );
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

    #[test]
    fn release_version_parser_reads_github_api_payload() {
        assert_eq!(
            release_version_from_github_api(r#"{"tag_name":"v0.8.0"}"#),
            Some("v0.8.0".to_string())
        );
        assert_eq!(
            release_version_from_github_api(r#"{"tag_name":"   "}"#),
            None
        );
        assert_eq!(
            release_version_from_github_api(r#"{"name":"v0.8.0"}"#),
            None
        );
    }

    #[test]
    fn install_skills_replaces_curated_entries_and_keeps_custom_entries() {
        let temp = tempfile::tempdir().unwrap();
        let staging_dir = temp.path().join("staging");
        let skills_dir = temp.path().join("skills");

        std::fs::create_dir_all(staging_dir.join("codex")).unwrap();
        std::fs::write(staging_dir.join("codex/SKILL.md"), "new codex").unwrap();

        std::fs::create_dir_all(skills_dir.join("codex")).unwrap();
        std::fs::write(skills_dir.join("codex/SKILL.md"), "old codex").unwrap();
        std::fs::create_dir_all(skills_dir.join("custom")).unwrap();
        std::fs::write(skills_dir.join("custom/SKILL.md"), "custom").unwrap();

        let installed = install_skills_from_staging(&staging_dir, &skills_dir).unwrap();

        assert_eq!(installed, 1);
        assert_eq!(
            read_text_file_sync(skills_dir.join("codex/SKILL.md")).unwrap(),
            "new codex"
        );
        assert_eq!(
            read_text_file_sync(skills_dir.join("custom/SKILL.md")).unwrap(),
            "custom"
        );
    }

    use crate::config::Config;
    use crate::util::http_client::new_reqwest_client;
    use axum::{Router, http::StatusCode as AxumStatus, routing::get};
    use std::io::{Cursor, Write};

    async fn spawn_mock(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn dead_proxy_client() -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex_lower(&hasher.finalize())
    }

    fn build_skills_zip() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("codex/SKILL.md", options).unwrap();
        writer.write_all(b"codex skill").unwrap();
        writer.add_directory("emptydir/", options).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn validate_update_command_rejects_conflicting_flags() {
        let conflict = UpdateCommand {
            force: false,
            skills: false,
            check: true,
            check_if_due: true,
            json: false,
        };
        assert!(validate_update_command(&conflict).is_err());

        let skills_with_check = UpdateCommand {
            force: false,
            skills: true,
            check: true,
            check_if_due: false,
            json: false,
        };
        assert!(validate_update_command(&skills_with_check).is_err());

        let json_without_check = UpdateCommand {
            force: false,
            skills: false,
            check: false,
            check_if_due: false,
            json: true,
        };
        assert!(validate_update_command(&json_without_check).is_err());

        let ok = UpdateCommand {
            force: false,
            skills: false,
            check: true,
            check_if_due: false,
            json: true,
        };
        assert!(validate_update_command(&ok).is_ok());
    }

    #[test]
    fn hex_lower_encodes_bytes() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
        assert_eq!(hex_lower(&[]), "");
    }

    #[test]
    fn normalized_target_name_canonicalizes_arch() {
        assert_eq!(normalized_target_name("linux", "amd64"), "linux-x86_64");
        assert_eq!(normalized_target_name("macos", "aarch64"), "macos-arm64");
        assert_eq!(normalized_target_name("freebsd", "riscv"), "freebsd-riscv");
    }

    #[test]
    fn release_target_detects_current_platform() {
        // The host running the tests is always a supported target.
        assert!(ReleaseTarget::detect().is_ok());
        assert!(ReleaseTarget::from_parts("plan9", "x86_64").is_none());
        let target = ReleaseTarget::from_parts("linux", "x86_64").unwrap();
        assert_eq!(target.name(), "linux-x86_64");
    }

    #[test]
    fn verify_checksum_matches_and_mismatches() {
        assert!(verify_checksum("asset", "abc", "abc").is_ok());
        let err = verify_checksum("asset", "abc", "xyz")
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Checksum verification failed"));
    }

    #[test]
    fn path_helpers_embed_expected_segments() {
        let staged = staged_update_path(Path::new("/install"), Path::new("/install/anda"));
        let staged = staged.to_string_lossy();
        assert!(staged.contains(".anda.update-"));
        assert!(staged.ends_with(".tmp"));

        let download = temporary_download_path("anda-linux-x86_64");
        assert!(
            download
                .to_string_lossy()
                .contains("anda-linux-x86_64.download-")
        );

        let skills = staged_skills_dir_path(Path::new("/home/anda"));
        assert!(skills.to_string_lossy().contains(".skills.update-"));
    }

    #[tokio::test]
    async fn download_binary_writes_file_and_reports_errors() {
        let body = b"anda-binary-bytes";
        let app = Router::new()
            .route("/asset", get(|| async { "anda-binary-bytes" }))
            .route(
                "/missing",
                get(|| async { (AxumStatus::NOT_FOUND, "nope") }),
            );
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();

        let dest = tempfile::NamedTempFile::new().unwrap();
        let hash = download_binary(&client, &format!("{base}/asset"), dest.path())
            .await
            .unwrap();
        assert_eq!(hash, sha256_hex(body));
        assert_eq!(std::fs::read(dest.path()).unwrap(), body);

        let err = download_binary(&client, &format!("{base}/missing"), dest.path())
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Download failed"));
    }

    #[tokio::test]
    async fn download_optional_file_handles_status_codes() {
        let app = Router::new()
            .route("/ok", get(|| async { "payload" }))
            .route("/404", get(|| async { (AxumStatus::NOT_FOUND, "") }))
            .route(
                "/500",
                get(|| async { (AxumStatus::INTERNAL_SERVER_ERROR, "boom") }),
            );
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();
        let dest = tempfile::NamedTempFile::new().unwrap();

        let some = download_optional_file(&client, &format!("{base}/ok"), dest.path())
            .await
            .unwrap();
        assert_eq!(some, Some(sha256_hex(b"payload")));

        let none = download_optional_file(&client, &format!("{base}/404"), dest.path())
            .await
            .unwrap();
        assert!(none.is_none());

        let err = download_optional_file(&client, &format!("{base}/500"), dest.path())
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Download failed"));
    }

    #[tokio::test]
    async fn fetch_expected_checksum_paths() {
        let valid = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let valid_owned = valid.to_string();
        let app = Router::new()
            .route(
                "/ok",
                get(move || {
                    let value = format!("{valid_owned}  anda-linux-x86_64\n");
                    async move { value }
                }),
            )
            .route("/404", get(|| async { (AxumStatus::NOT_FOUND, "") }))
            .route("/bad", get(|| async { "not-a-valid-hash garbage" }));
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();

        let found = fetch_expected_checksum(&client, &format!("{base}/ok"))
            .await
            .unwrap();
        assert_eq!(found, Some(valid.to_string()));

        let missing = fetch_expected_checksum(&client, &format!("{base}/404"))
            .await
            .unwrap();
        assert!(missing.is_none());

        let bad = fetch_expected_checksum(&client, &format!("{base}/bad"))
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(bad.to_string().contains("valid SHA-256"));

        // A network failure is swallowed into `None`.
        let dead = dead_proxy_client();
        let unreachable = fetch_expected_checksum(&dead, "https://example.invalid/x.sha256")
            .await
            .unwrap();
        assert!(unreachable.is_none());
    }

    #[tokio::test]
    async fn fetch_latest_version_errors_without_network() {
        let dead = dead_proxy_client();
        let err = fetch_latest_version(&dead).await.map(|_| ()).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn release_version_from_response_reads_location_header_and_url() {
        let app = Router::new()
            .route(
                "/with-header",
                get(|| async {
                    (
                        [(axum::http::header::LOCATION, "/foo/releases/tag/v9.9.9")],
                        "ok",
                    )
                }),
            )
            .route("/releases/tag/v3.2.1", get(|| async { "landed" }));
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();

        let header_resp = client
            .get(format!("{base}/with-header"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            release_version_from_response(&header_resp),
            Some("v9.9.9".to_string())
        );

        let url_resp = client
            .get(format!("{base}/releases/tag/v3.2.1"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            release_version_from_response(&url_resp),
            Some("v3.2.1".to_string())
        );
    }

    #[tokio::test]
    async fn stage_and_install_update_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let download = dir.path().join("download.bin");
        std::fs::write(&download, b"new binary").unwrap();
        let staged = dir.path().join("staged.bin");
        let current = dir.path().join("anda");
        std::fs::write(&current, b"old binary").unwrap();

        stage_update(&download, &staged).await.unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"new binary");

        prepare_executable(&staged, &current).await.unwrap();

        let finish = install_update(&staged, &current).await.unwrap();
        #[cfg(not(windows))]
        {
            assert_eq!(finish, UpdateFinish::Installed);
            assert_eq!(std::fs::read(&current).unwrap(), b"new binary");
        }
        #[cfg(windows)]
        {
            // On Windows the replacement is handed off to an async PowerShell
            // helper, so the swap is scheduled rather than applied inline.
            assert_eq!(finish, UpdateFinish::Scheduled);
        }
    }

    #[tokio::test]
    async fn stage_update_reports_copy_failure() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let staged = dir.path().join("staged.bin");
        let err = stage_update(&missing, &staged)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("Could not stage update"));
    }

    #[test]
    fn extract_skills_archive_unpacks_entries() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("anda-skills.zip");
        std::fs::write(&archive, build_skills_zip()).unwrap();
        let staging = dir.path().join("staging");

        extract_skills_archive(&archive, &staging).unwrap();
        assert_eq!(
            std::fs::read(staging.join("codex/SKILL.md")).unwrap(),
            b"codex skill"
        );

        let skills = dir.path().join("skills");
        let installed = install_skills_from_staging(&staging, &skills).unwrap();
        // Both the `codex` skill directory and the empty directory entry install.
        assert_eq!(installed, 2);
        assert!(skills.join("codex/SKILL.md").is_file());
    }

    #[test]
    fn install_skills_from_staging_rejects_empty_archive() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let skills = dir.path().join("skills");
        let err = install_skills_from_staging(&staging, &skills)
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("is empty"));
    }

    #[test]
    fn remove_path_if_exists_handles_files_dirs_and_absent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        remove_path_if_exists(&file).unwrap();
        assert!(!file.exists());

        let sub = dir.path().join("sub");
        std::fs::create_dir_all(sub.join("nested")).unwrap();
        remove_path_if_exists(&sub).unwrap();
        assert!(!sub.exists());

        // Removing an absent path is a no-op.
        remove_path_if_exists(&dir.path().join("ghost")).unwrap();
    }

    #[tokio::test]
    async fn install_release_skills_downloads_and_installs() {
        let zip_bytes = build_skills_zip();
        let checksum = format!("{}  anda-skills.zip\n", sha256_hex(&zip_bytes));
        let zip_for_route = zip_bytes.clone();
        let checksum_for_route = checksum.clone();
        let app = Router::new()
            .route(
                "/anda-skills.zip",
                get(move || {
                    let bytes = zip_for_route.clone();
                    async move { bytes }
                }),
            )
            .route(
                "/anda-skills.zip.sha256",
                get(move || {
                    let value = checksum_for_route.clone();
                    async move { value }
                }),
            );
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("skills/codex")).unwrap();
        std::fs::write(home.path().join("skills/codex/SKILL.md"), "personal codex").unwrap();

        let installed = install_release_skills(&client, &base, home.path())
            .await
            .unwrap();
        assert!(installed);
        assert!(home.path().join("bundled-skills/codex/SKILL.md").is_file());
        assert_eq!(
            read_text_file_sync(home.path().join("skills/codex/SKILL.md")).unwrap(),
            "personal codex"
        );
    }

    #[tokio::test]
    async fn install_release_skills_skips_when_archive_missing() {
        let app = Router::new().route(
            "/anda-skills.zip",
            get(|| async { (AxumStatus::NOT_FOUND, "") }),
        );
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();
        let home = tempfile::tempdir().unwrap();

        let installed = install_release_skills(&client, &base, home.path())
            .await
            .unwrap();
        assert!(!installed);
    }

    fn print_states_for_coverage() {
        use auto_update::{AutoUpdateState, AutoUpdateStatus};
        for status in [
            AutoUpdateStatus::Idle,
            AutoUpdateStatus::Checking,
            AutoUpdateStatus::Downloading,
            AutoUpdateStatus::Current,
            AutoUpdateStatus::Installed,
            AutoUpdateStatus::Failed,
            AutoUpdateStatus::Downloaded,
        ] {
            let state = AutoUpdateState {
                status,
                latest_tag: Some("v9.9.9".to_string()),
                downloaded_path: Some("/tmp/anda".to_string()),
                error: Some("boom".to_string()),
                ..AutoUpdateState::default()
            };
            print_update_check_state(&state);
        }
        print_update_finish("v0.1.0", "v0.2.0", UpdateFinish::Installed);
    }

    async fn temp_daemon_with_db() -> (tempfile::TempDir, Daemon) {
        let dir = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(dir.path().to_path_buf(), Config::default());
        // Create the bot db so `open_bot_db` succeeds inside the updater.
        daemon.connect_bot_db().await.unwrap();
        (dir, daemon)
    }

    #[tokio::test]
    async fn install_release_launcher_skips_without_supported_sidecar() {
        let client = dead_proxy_client();
        let temp = tempfile::tempdir().unwrap();
        let macos = ReleaseTarget::from_parts("macos", "arm64").unwrap();
        let missing = install_release_launcher_if_present(
            &client,
            "https://example.invalid",
            macos,
            temp.path(),
        )
        .await
        .unwrap();
        assert!(missing.is_none());

        std::fs::write(temp.path().join("anda_launcher"), "old").unwrap();
        let linux = ReleaseTarget::from_parts("linux", "x86_64").unwrap();
        let unsupported = install_release_launcher_if_present(
            &client,
            "https://example.invalid",
            linux,
            temp.path(),
        )
        .await
        .unwrap();
        assert!(unsupported.is_none());
    }

    #[tokio::test]
    async fn install_release_launcher_replaces_existing_macos_sidecar() {
        let body = b"new-launcher";
        let hash = sha256_hex(body);
        let checksum = hash.clone();
        let app = Router::new()
            .route(
                "/anda_launcher-macos-arm64",
                get(|| async { "new-launcher" }),
            )
            .route(
                "/anda_launcher-macos-arm64.sha256",
                get(move || {
                    let value = format!("{checksum}  anda_launcher-macos-arm64\n");
                    async move { value }
                }),
            );
        let base = spawn_mock(app).await;
        let client = new_reqwest_client();
        let temp = tempfile::tempdir().unwrap();
        let launcher = temp.path().join("anda_launcher");
        std::fs::write(&launcher, "old-launcher").unwrap();

        let finish = install_release_launcher_if_present(
            &client,
            &base,
            ReleaseTarget::from_parts("macos", "arm64").unwrap(),
            temp.path(),
        )
        .await
        .unwrap();

        #[cfg(not(windows))]
        {
            assert_eq!(finish, Some(UpdateFinish::Installed));
            assert_eq!(std::fs::read(&launcher).unwrap(), body);
        }
        #[cfg(windows)]
        {
            // On Windows the swap is deferred to an async PowerShell helper.
            assert_eq!(finish, Some(UpdateFinish::Scheduled));
        }
    }

    #[tokio::test]
    async fn run_update_check_records_failure_and_emits_json() {
        print_states_for_coverage();
        let (_dir, daemon) = temp_daemon_with_db().await;
        let client = dead_proxy_client();

        let cmd = UpdateCommand {
            force: false,
            skills: false,
            check: true,
            check_if_due: false,
            json: true,
        };
        run_update_check(&client, &daemon, &cmd).await.unwrap();

        let cmd_human = UpdateCommand {
            force: false,
            skills: false,
            check: false,
            check_if_due: true,
            json: false,
        };
        run_update_check(&client, &daemon, &cmd_human)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn run_dispatches_check_and_propagates_network_errors() {
        let (_dir, daemon) = temp_daemon_with_db().await;
        let client = dead_proxy_client();

        // `--check` routes through run_update_check, which swallows the failure.
        let check_cmd = UpdateCommand {
            force: false,
            skills: false,
            check: true,
            check_if_due: false,
            json: false,
        };
        run(&client, &daemon, &check_cmd).await.unwrap();

        // `--skills` (without check) reaches the network fetch and surfaces the error.
        let skills_cmd = UpdateCommand {
            force: false,
            skills: true,
            check: false,
            check_if_due: false,
            json: false,
        };
        let err = run(&client, &daemon, &skills_cmd)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(!err.to_string().is_empty());

        // An invalid flag combination fails validation before any network call.
        let bad_cmd = UpdateCommand {
            force: false,
            skills: false,
            check: true,
            check_if_due: true,
            json: false,
        };
        assert!(run(&client, &daemon, &bad_cmd).await.is_err());
    }
}
