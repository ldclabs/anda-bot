use anda_core::BoxError;
use anda_engine::unix_ms;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tokio::sync::Mutex;

use crate::{cli::updater, daemon_protocol::current_version_tag};

pub use crate::daemon_protocol::{AutoUpdateState, AutoUpdateStatus};

/// The daemon and `anda update` (run by Anda Desktop) are separate processes
/// that share this state, so it lives in a file both read on every access.
const STATE_FILE: &str = "auto_update.json";
/// Holds only the newest downloaded release.
const DOWNLOADS_DIR: &str = "updates";
const CHECK_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;
/// A failed check, often offline right after login, retries sooner.
const RETRY_INTERVAL_MS: u64 = 60 * 60 * 1000;

impl AutoUpdateState {
    pub fn downloaded_update_available(&self) -> bool {
        self.status == AutoUpdateStatus::Downloaded
            && self.latest_is_newer()
            && self
                .downloaded_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
    }

    pub fn cli_notice(&self) -> Option<String> {
        if !self.downloaded_update_available() {
            return None;
        }

        let latest = self.latest_tag.as_deref().unwrap_or("the latest release");
        Some(format!(
            "Anda {latest} has been downloaded. Run `anda update`, then `anda restart` to use it."
        ))
    }

    fn latest_is_newer(&self) -> bool {
        self.latest_tag
            .as_deref()
            .is_some_and(|latest| is_newer_release(latest, &self.current_tag))
    }
}

pub struct AutoUpdater {
    home_dir: PathBuf,
    http: reqwest::Client,
    lock: Mutex<()>,
    homebrew_managed: bool,
}

impl AutoUpdater {
    pub fn new(home_dir: PathBuf, http: reqwest::Client) -> Self {
        Self {
            home_dir,
            http,
            lock: Mutex::new(()),
            homebrew_managed: std::env::current_exe()
                .is_ok_and(|exe| updater::is_homebrew_managed(&exe)),
        }
    }

    /// Checks for a newer release and downloads it without installing it.
    /// Unless `force`, only when the persisted check interval is due.
    pub async fn check(&self, force: bool) -> AutoUpdateState {
        if self.homebrew_managed {
            return homebrew_update_state();
        }
        let _guard = self.lock.lock().await;
        match self.run_check(force).await {
            Ok(state) => state,
            Err(err) => self.record_failure(err.to_string()).await,
        }
    }

    async fn run_check(&self, force: bool) -> Result<AutoUpdateState, BoxError> {
        let mut state = read_state(&self.home_dir).await;
        let now_ms = unix_ms();
        if !force && !check_due(&state, now_ms) {
            return Ok(state);
        }

        state.status = AutoUpdateStatus::Checking;
        state.last_checked_ms = Some(now_ms);
        state.error = None;
        save_state(&self.home_dir, &state).await?;

        let latest_tag = updater::fetch_latest_version(&self.http).await?;
        if !is_newer_release(&latest_tag, &state.current_tag) {
            remove_downloads(&self.home_dir).await;
            let state = AutoUpdateState {
                status: AutoUpdateStatus::Current,
                latest_tag: Some(latest_tag),
                last_checked_ms: Some(unix_ms()),
                ..AutoUpdateState::default()
            };
            save_state(&self.home_dir, &state).await?;
            return Ok(state);
        }

        let target = updater::ReleaseTarget::detect()?;
        let asset_name = target.asset_name();
        let downloaded_path = download_path(&self.home_dir, &latest_tag, &asset_name);
        if !usable_downloaded_file(&state, &latest_tag, &asset_name, &downloaded_path).await {
            // Keep only this release; this also clears the partial file of an
            // interrupted download.
            remove_downloads(&self.home_dir).await;
            state = AutoUpdateState {
                status: AutoUpdateStatus::Downloading,
                latest_tag: Some(latest_tag.clone()),
                last_checked_ms: Some(now_ms),
                target: Some(target.name()),
                asset_name: Some(asset_name.clone()),
                ..AutoUpdateState::default()
            };
            save_state(&self.home_dir, &state).await?;

            let sha256 =
                download_release_asset(&self.http, &latest_tag, &asset_name, &downloaded_path)
                    .await?;
            state.downloaded_at_ms = Some(unix_ms());
            state.sha256 = Some(sha256);
            state.checksum_verified = true;
        }

        state.status = AutoUpdateStatus::Downloaded;
        state.downloaded_path = Some(downloaded_path.to_string_lossy().to_string());
        state.last_checked_ms = Some(unix_ms());
        save_state(&self.home_dir, &state).await?;
        Ok(state)
    }

    /// Records a failed check. A verified download of a newer release stays
    /// installable: a check that fails offline must not hide an update that
    /// is already on disk.
    async fn record_failure(&self, error: String) -> AutoUpdateState {
        let mut state = read_state(&self.home_dir).await;
        state.status = if usable_download(&state).await {
            AutoUpdateStatus::Downloaded
        } else {
            AutoUpdateStatus::Failed
        };
        state.last_checked_ms = Some(unix_ms());
        state.error = Some(error);
        if let Err(err) = save_state(&self.home_dir, &state).await {
            log::warn!("failed to persist auto update failure state: {err}");
        }
        state
    }
}

/// The verified download of `latest_tag` that `anda update` can install
/// without downloading it again.
pub async fn downloaded_update_path(
    home_dir: &Path,
    latest_tag: &str,
    asset_name: &str,
) -> Option<PathBuf> {
    let state = read_state(home_dir).await;
    let path = PathBuf::from(state.downloaded_path.as_deref()?);
    usable_downloaded_file(&state, latest_tag, asset_name, &path)
        .await
        .then_some(path)
}

/// Records that `anda update` installed `latest_tag`, whose download is no
/// longer needed.
pub async fn mark_installed(home_dir: &Path, latest_tag: &str) {
    remove_downloads(home_dir).await;
    let mut state = read_state(home_dir).await;
    if state.latest_tag.as_deref() != Some(latest_tag) {
        return;
    }
    state.status = AutoUpdateStatus::Installed;
    state.installed_at_ms = Some(unix_ms());
    state.error = None;
    if let Err(err) = save_state(home_dir, &state).await {
        log::warn!("failed to persist installed auto update state: {err}");
    }
}

/// Homebrew installs never download or stage releases themselves.
fn homebrew_update_state() -> AutoUpdateState {
    AutoUpdateState {
        error: Some(updater::HOMEBREW_UPDATE_MESSAGE.to_string()),
        ..AutoUpdateState::default()
    }
}

async fn read_state(home_dir: &Path) -> AutoUpdateState {
    let mut state = match tokio::fs::read(home_dir.join(STATE_FILE)).await {
        Ok(bytes) => serde_json::from_slice::<AutoUpdateState>(&bytes).unwrap_or_default(),
        Err(_) => AutoUpdateState::default(),
    };
    state.current_tag = current_version_tag();
    // Another install (Anda Desktop, an install script) may already run the
    // downloaded release or a newer one; it is no update to offer.
    if state.status == AutoUpdateStatus::Downloaded && !state.latest_is_newer() {
        state.status = AutoUpdateStatus::Current;
    }
    state
}

async fn save_state(home_dir: &Path, state: &AutoUpdateState) -> Result<(), BoxError> {
    let temp = home_dir.join(format!(
        ".{STATE_FILE}.{}.tmp",
        updater::unique_path_suffix()
    ));
    tokio::fs::write(&temp, serde_json::to_vec(state)?).await?;
    // Readers in the other process see the old or the new state, never a
    // partial one.
    if let Err(err) = tokio::fs::rename(&temp, home_dir.join(STATE_FILE)).await {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(err.into());
    }
    Ok(())
}

fn check_due(state: &AutoUpdateState, now_ms: u64) -> bool {
    let interval_ms = match state.status {
        // Left behind by a check that did not finish.
        AutoUpdateStatus::Checking | AutoUpdateStatus::Downloading => return true,
        AutoUpdateStatus::Failed => RETRY_INTERVAL_MS,
        _ => CHECK_INTERVAL_MS,
    };
    state
        .last_checked_ms
        .is_none_or(|last_checked_ms| now_ms.saturating_sub(last_checked_ms) >= interval_ms)
}

pub(crate) fn is_newer_release(latest_tag: &str, current_tag: &str) -> bool {
    if latest_tag == current_tag {
        return false;
    }

    match (
        parse_release_version(latest_tag),
        parse_release_version(current_tag),
    ) {
        (Some(latest), Some(current)) => latest > current,
        _ => true,
    }
}

/// Numeric parts, then whether this is a final release: `v0.13.0-beta.1`
/// sorts before `v0.13.0`.
fn parse_release_version(tag: &str) -> Option<(Vec<u64>, bool)> {
    let tag = tag.trim().trim_start_matches('v');
    let (version, release) = match tag.split_once('-') {
        Some((version, _)) => (version, false),
        None => (tag, true),
    };
    let version = version.split('+').next().unwrap_or_default();
    if version.is_empty() {
        return None;
    }
    let parts = version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    Some((parts, release))
}

async fn download_release_asset(
    client: &reqwest::Client,
    latest_tag: &str,
    asset_name: &str,
    destination: &Path,
) -> Result<String, BoxError> {
    let base_url = updater::release_download_base_url(latest_tag);
    download_release_asset_from_base_url(client, &base_url, asset_name, destination).await
}

/// Downloads next to `destination`, so the final rename never crosses file
/// systems. The next download or check clears a partial `.part` file.
async fn download_release_asset_from_base_url(
    client: &reqwest::Client,
    base_url: &str,
    asset_name: &str,
    destination: &Path,
) -> Result<String, BoxError> {
    let asset_url = format!("{base_url}/{asset_name}");
    // The checksum is tiny; a missing one fails before the binary download.
    let expected_hash =
        updater::fetch_expected_checksum(client, &format!("{asset_url}.sha256")).await?;

    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut partial = destination.as_os_str().to_os_string();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let actual_hash = updater::download_binary(client, &asset_url, &partial).await?;
    if expected_hash != actual_hash {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(format!("Checksum verification failed for {asset_name}").into());
    }

    tokio::fs::rename(&partial, destination).await?;
    Ok(actual_hash)
}

async fn usable_downloaded_file(
    state: &AutoUpdateState,
    latest_tag: &str,
    asset_name: &str,
    path: &Path,
) -> bool {
    if state.latest_tag.as_deref() != Some(latest_tag)
        || state.asset_name.as_deref() != Some(asset_name)
        || !state.checksum_verified
    {
        return false;
    }

    let Some(expected_hash) = state.sha256.as_deref() else {
        return false;
    };
    sha256_file(path)
        .await
        .is_ok_and(|actual_hash| actual_hash == expected_hash)
}

/// Whether `state` records a verified download of a newer release that is
/// still on disk.
async fn usable_download(state: &AutoUpdateState) -> bool {
    let (Some(latest_tag), Some(asset_name), Some(path)) = (
        state.latest_tag.as_deref(),
        state.asset_name.as_deref(),
        state.downloaded_path.as_deref(),
    ) else {
        return false;
    };
    state.latest_is_newer()
        && usable_downloaded_file(state, latest_tag, asset_name, Path::new(path)).await
}

async fn sha256_file(path: &Path) -> Result<String, BoxError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(updater::hex_lower(&hasher.finalize()))
    })
    .await?
}

fn download_path(home_dir: &Path, latest_tag: &str, asset_name: &str) -> PathBuf {
    home_dir
        .join(DOWNLOADS_DIR)
        .join(sanitize_path_segment(latest_tag))
        .join(asset_name)
}

/// Removes every downloaded release, including partial downloads.
async fn remove_downloads(home_dir: &Path) {
    match tokio::fs::remove_dir_all(home_dir.join(DOWNLOADS_DIR)).await {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            log::warn!("failed to remove old update downloads: {err}");
        }
        _ => {}
    }
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::http_client::new_reqwest_client;
    use axum::{Router, routing::get};

    #[test]
    fn version_comparison_detects_newer_release() {
        assert!(is_newer_release("v0.7.8", "v0.7.7"));
        assert!(is_newer_release("v0.8.0", "v0.7.99"));
        assert!(!is_newer_release("v0.7.7", "v0.7.7"));
        assert!(!is_newer_release("v0.7.6", "v0.7.7"));
    }

    #[test]
    fn final_release_is_newer_than_its_prerelease() {
        assert!(is_newer_release("v0.13.0", "v0.13.0-beta.1"));
        assert!(!is_newer_release("v0.13.0-beta.1", "v0.13.0"));
        assert!(is_newer_release("v0.13.1-beta.1", "v0.13.0"));
    }

    #[test]
    fn transient_states_are_always_due() {
        let state = AutoUpdateState {
            status: AutoUpdateStatus::Downloading,
            last_checked_ms: Some(100),
            ..AutoUpdateState::default()
        };
        assert!(check_due(&state, 101));
    }

    #[test]
    fn stable_states_wait_twenty_four_hours() {
        let state = AutoUpdateState {
            status: AutoUpdateStatus::Current,
            last_checked_ms: Some(1000),
            ..AutoUpdateState::default()
        };
        assert!(!check_due(&state, 1000 + CHECK_INTERVAL_MS - 1));
        assert!(check_due(&state, 1000 + CHECK_INTERVAL_MS));
    }

    #[test]
    fn failed_checks_retry_within_the_hour() {
        let state = AutoUpdateState {
            status: AutoUpdateStatus::Failed,
            last_checked_ms: Some(1000),
            ..AutoUpdateState::default()
        };
        assert!(!check_due(&state, 1000 + RETRY_INTERVAL_MS - 1));
        assert!(check_due(&state, 1000 + RETRY_INTERVAL_MS));
    }

    #[test]
    fn sanitize_path_segments_keep_release_tags_readable() {
        assert_eq!(sanitize_path_segment("v0.7.8"), "v0.7.8");
        assert_eq!(sanitize_path_segment("v0/7/8"), "v0_7_8");
    }

    fn test_updater() -> (tempfile::TempDir, AutoUpdater) {
        let home = tempfile::tempdir().unwrap();
        // All HTTP requests are routed through a dead proxy so checks fail
        // fast without touching the network.
        let http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap();
        let updater = AutoUpdater::new(home.path().to_path_buf(), http);
        (home, updater)
    }

    /// A verified download of `latest_tag` under `home`.
    fn downloaded_state(home: &Path, latest_tag: &str) -> AutoUpdateState {
        let asset = "anda-macos-arm64";
        let path = download_path(home, latest_tag, asset);
        AutoUpdateState {
            status: AutoUpdateStatus::Downloaded,
            latest_tag: Some(latest_tag.to_string()),
            asset_name: Some(asset.to_string()),
            downloaded_path: Some(path.to_string_lossy().to_string()),
            sha256: Some(write_file(&path, b"the new binary")),
            checksum_verified: true,
            ..AutoUpdateState::default()
        }
    }

    #[tokio::test]
    async fn homebrew_install_does_not_check_or_download_updates() {
        let (home, mut updater) = test_updater();
        updater.homebrew_managed = true;
        let state = updater.check(true).await;
        assert_eq!(state.status, AutoUpdateStatus::Idle);
        assert!(state.last_checked_ms.is_none());
        assert!(state.error.as_deref().unwrap().contains("brew upgrade"));
        assert!(!home.path().join(STATE_FILE).exists());
    }

    #[test]
    fn downloaded_update_notice_requires_complete_state() {
        let mut state = AutoUpdateState::default();
        assert!(!state.downloaded_update_available());
        assert!(state.cli_notice().is_none());

        state.status = AutoUpdateStatus::Downloaded;
        state.latest_tag = Some("v999.0.0".to_string());
        state.downloaded_path = Some("/tmp/anda-update".to_string());
        assert!(state.downloaded_update_available());
        let notice = state.cli_notice().expect("update notice");
        assert!(notice.contains("v999.0.0"));

        // The running build, or an older one, is not an update.
        state.latest_tag = Some(state.current_tag.clone());
        assert!(!state.downloaded_update_available());
        state.latest_tag = Some("v0.0.1".to_string());
        assert!(!state.downloaded_update_available());
    }

    #[tokio::test]
    async fn state_file_is_shared_and_normalizes_outdated_downloads() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(read_state(home.path()).await.status, AutoUpdateStatus::Idle);

        // Another process sees what this one saved.
        let state = downloaded_state(home.path(), "v9999.0.0");
        save_state(home.path(), &state).await.unwrap();
        assert!(read_state(home.path()).await.downloaded_update_available());

        // A download the running build has already caught up with is no
        // update, so neither the CLI nor Anda Desktop offers it.
        let state = downloaded_state(home.path(), "v0.0.1");
        save_state(home.path(), &state).await.unwrap();
        assert_eq!(
            read_state(home.path()).await.status,
            AutoUpdateStatus::Current
        );

        // Unreadable state starts over.
        std::fs::write(home.path().join(STATE_FILE), b"{not json").unwrap();
        assert_eq!(read_state(home.path()).await.status, AutoUpdateStatus::Idle);
    }

    #[tokio::test]
    async fn failed_checks_persist_state() {
        let (home, updater) = test_updater();

        // A forced check hits the dead proxy and records the failure.
        let failed = updater.check(true).await;
        assert_eq!(failed.status, AutoUpdateStatus::Failed);
        assert!(failed.error.is_some());
        assert!(failed.last_checked_ms.is_some());

        // The failure state is durable across reads.
        let reread = read_state(home.path()).await;
        assert_eq!(reread.status, AutoUpdateStatus::Failed);
    }

    #[tokio::test]
    async fn failures_keep_a_verified_download_installable() {
        let (home, updater) = test_updater();
        let mut state = downloaded_state(home.path(), "v9999.0.0");
        save_state(home.path(), &state).await.unwrap();

        // An offline check reports its error and keeps the release ready.
        let checked = updater.check(true).await;
        assert_eq!(checked.status, AutoUpdateStatus::Downloaded);
        assert!(checked.downloaded_update_available());
        assert!(checked.error.is_some());
        assert!(checked.last_checked_ms.is_some());
        assert!(read_state(home.path()).await.downloaded_update_available());

        // A download of the running release is not an update to keep.
        state.latest_tag = Some(current_version_tag());
        save_state(home.path(), &state).await.unwrap();
        let failed = updater.record_failure("boom".to_string()).await;
        assert_eq!(failed.status, AutoUpdateStatus::Failed);

        // Neither is a download that no longer matches its checksum.
        state.latest_tag = Some("v9999.0.0".to_string());
        save_state(home.path(), &state).await.unwrap();
        std::fs::write(state.downloaded_path.as_deref().unwrap(), b"tampered").unwrap();
        let failed = updater.check(true).await;
        assert_eq!(failed.status, AutoUpdateStatus::Failed);
        assert!(!read_state(home.path()).await.downloaded_update_available());
    }

    #[tokio::test]
    async fn due_checks_are_skipped_when_recently_current() {
        let (home, updater) = test_updater();
        let state = AutoUpdateState {
            status: AutoUpdateStatus::Current,
            last_checked_ms: Some(unix_ms()),
            ..AutoUpdateState::default()
        };
        save_state(home.path(), &state).await.unwrap();

        // Not yet due: returns the stored state without any network call.
        let result = updater.check(false).await;
        assert_eq!(result.status, AutoUpdateStatus::Current);
        assert!(result.error.is_none());
    }

    fn write_file(path: &Path, bytes: &[u8]) -> String {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        updater::hex_lower(&hasher.finalize())
    }

    #[test]
    fn download_path_sanitizes_tag() {
        let path = download_path(Path::new("/home"), "v1/2", "anda-macos-arm64");
        assert_eq!(
            path,
            Path::new("/home/updates/v1_2/anda-macos-arm64").to_path_buf()
        );
    }

    #[test]
    fn parse_release_version_handles_prerelease_and_garbage() {
        assert_eq!(
            parse_release_version("v1.2.3-rc.1"),
            Some((vec![1, 2, 3], false))
        );
        assert_eq!(
            parse_release_version("0.4.0+build"),
            Some((vec![0, 4, 0], true))
        );
        assert!(parse_release_version("v").is_none());
        assert!(parse_release_version("not-a-version").is_none());
    }

    #[test]
    fn is_newer_release_falls_back_when_unparseable() {
        // When either side cannot be parsed but they differ, treat as newer.
        assert!(is_newer_release("nightly", "v0.1.0"));
        assert!(!is_newer_release("same", "same"));
    }

    #[tokio::test]
    async fn sha256_file_hashes_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob.bin");
        let expected = write_file(&path, b"hello anda update");
        assert_eq!(sha256_file(&path).await.unwrap(), expected);
        assert!(sha256_file(&dir.path().join("missing")).await.is_err());
    }

    #[tokio::test]
    async fn download_release_asset_requires_remote_checksum() {
        let app = Router::new().route(
            "/anda-macos-arm64",
            get(|| async { (axum::http::StatusCode::OK, "new-binary") }),
        );
        let base = crate::test_support::spawn_http_mock(app).await;
        let client = new_reqwest_client();
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("v1").join("anda-macos-arm64");

        let err =
            download_release_asset_from_base_url(&client, &base, "anda-macos-arm64", &destination)
                .await
                .map(|_| ())
                .unwrap_err();

        assert!(
            err.to_string().contains("Checksum file not found"),
            "got: {err}"
        );
        // The checksum is fetched first, so nothing was downloaded.
        assert!(!dir.path().join("v1").exists());
    }

    #[tokio::test]
    async fn download_release_asset_verifies_and_replaces_partial_file() {
        let body = b"new-binary";
        let mut hasher = Sha256::new();
        hasher.update(body);
        let hash = updater::hex_lower(&hasher.finalize());
        let checksum = format!("{hash}  anda-macos-arm64\n");
        let app = Router::new()
            .route("/anda-macos-arm64", get(|| async { "new-binary" }))
            .route(
                "/anda-macos-arm64.sha256",
                get(move || {
                    let checksum = checksum.clone();
                    async move { checksum }
                }),
            );
        let base = crate::test_support::spawn_http_mock(app).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("v1").join("anda-macos-arm64");

        let actual = download_release_asset_from_base_url(
            &new_reqwest_client(),
            &base,
            "anda-macos-arm64",
            &destination,
        )
        .await
        .unwrap();

        assert_eq!(actual, hash);
        assert_eq!(std::fs::read(&destination).unwrap(), body);
        assert!(!dir.path().join("v1").join("anda-macos-arm64.part").exists());
    }

    #[tokio::test]
    async fn usable_downloaded_file_validates_state_and_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anda-macos-arm64");
        let hash = write_file(&path, b"binary contents");

        let mut state = AutoUpdateState {
            status: AutoUpdateStatus::Downloaded,
            latest_tag: Some("v9.9.9".to_string()),
            asset_name: Some("anda-macos-arm64".to_string()),
            sha256: Some(hash.clone()),
            checksum_verified: true,
            ..AutoUpdateState::default()
        };
        assert!(usable_downloaded_file(&state, "v9.9.9", "anda-macos-arm64", &path).await);

        // Tag mismatch is rejected.
        assert!(!usable_downloaded_file(&state, "v0.0.1", "anda-macos-arm64", &path).await);

        // Missing file is rejected.
        assert!(
            !usable_downloaded_file(
                &state,
                "v9.9.9",
                "anda-macos-arm64",
                &dir.path().join("nope")
            )
            .await
        );

        // Unverified checksum is rejected.
        state.checksum_verified = false;
        assert!(!usable_downloaded_file(&state, "v9.9.9", "anda-macos-arm64", &path).await);
        state.checksum_verified = true;

        // Missing recorded hash is rejected.
        state.sha256 = None;
        assert!(!usable_downloaded_file(&state, "v9.9.9", "anda-macos-arm64", &path).await);

        // Wrong recorded hash is rejected.
        state.sha256 = Some("0".repeat(64));
        assert!(!usable_downloaded_file(&state, "v9.9.9", "anda-macos-arm64", &path).await);
    }

    #[tokio::test]
    async fn mark_installed_updates_matching_tag_and_drops_downloads() {
        let home = tempfile::tempdir().unwrap();
        let state = downloaded_state(home.path(), "v9.9.9");
        save_state(home.path(), &state).await.unwrap();

        // A non-matching tag leaves the state untouched.
        mark_installed(home.path(), "v0.0.1").await;
        let after = read_state(home.path()).await;
        assert_eq!(after.status, AutoUpdateStatus::Downloaded);

        // The matching tag flips it to Installed.
        mark_installed(home.path(), "v9.9.9").await;
        let installed = read_state(home.path()).await;
        assert_eq!(installed.status, AutoUpdateStatus::Installed);
        assert!(installed.installed_at_ms.is_some());
        assert!(!home.path().join(DOWNLOADS_DIR).exists());
    }

    #[tokio::test]
    async fn downloaded_update_path_requires_usable_file() {
        let home = tempfile::tempdir().unwrap();
        let asset = "anda-macos-arm64";
        let state = downloaded_state(home.path(), "v9.9.9");
        save_state(home.path(), &state).await.unwrap();

        let resolved = downloaded_update_path(home.path(), "v9.9.9", asset).await;
        assert_eq!(resolved, Some(download_path(home.path(), "v9.9.9", asset)));

        // A different tag is not usable.
        assert!(
            downloaded_update_path(home.path(), "v0.0.1", asset)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn remove_downloads_clears_every_release() {
        let home = tempfile::tempdir().unwrap();
        downloaded_state(home.path(), "v1.0.0");
        write_file(
            &download_path(home.path(), "v1.1.0", "anda-macos-arm64.part"),
            b"partial",
        );

        remove_downloads(home.path()).await;
        assert!(!home.path().join(DOWNLOADS_DIR).exists());
        // Nothing to remove is fine.
        remove_downloads(home.path()).await;
    }
}
