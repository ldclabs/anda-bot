use anda_core::BoxError;
use anda_db::database::AndaDB;
use anda_engine::unix_ms;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{io::AsyncReadExt, sync::Mutex};

#[cfg(unix)]
use std::{
    process::{Command, Stdio},
    time::Duration,
};

use crate::{cli::updater, daemon::Daemon};

pub const AUTO_UPDATE_EXTENSION_KEY: &str = "anda_auto_update";
const CHECK_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateStatus {
    #[default]
    Idle,
    Checking,
    Current,
    Downloading,
    Downloaded,
    Failed,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoUpdateState {
    pub status: AutoUpdateStatus,
    pub current_tag: String,
    pub latest_tag: Option<String>,
    pub last_checked_ms: Option<u64>,
    pub downloaded_at_ms: Option<u64>,
    pub installed_at_ms: Option<u64>,
    pub target: Option<String>,
    pub asset_name: Option<String>,
    pub downloaded_path: Option<String>,
    pub sha256: Option<String>,
    pub checksum_verified: bool,
    pub error: Option<String>,
}

impl Default for AutoUpdateState {
    fn default() -> Self {
        Self {
            status: AutoUpdateStatus::Idle,
            current_tag: current_version_tag(),
            latest_tag: None,
            last_checked_ms: None,
            downloaded_at_ms: None,
            installed_at_ms: None,
            target: None,
            asset_name: None,
            downloaded_path: None,
            sha256: None,
            checksum_verified: false,
            error: None,
        }
    }
}

impl AutoUpdateState {
    pub fn downloaded_update_available(&self) -> bool {
        self.status == AutoUpdateStatus::Downloaded
            && self
                .latest_tag
                .as_deref()
                .is_some_and(|latest| latest != self.current_tag)
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
}

#[derive(Clone)]
pub struct AutoUpdater {
    db: Arc<AndaDB>,
    home_dir: PathBuf,
    http: reqwest::Client,
    lock: Arc<Mutex<()>>,
}

impl AutoUpdater {
    pub fn new(db: Arc<AndaDB>, home_dir: PathBuf, http: reqwest::Client) -> Self {
        Self {
            db,
            home_dir,
            http,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn state(&self) -> AutoUpdateState {
        read_state(self.db.as_ref())
    }

    pub async fn check_if_due(&self) -> AutoUpdateState {
        let _guard = self.lock.lock().await;
        match self.run_check(false).await {
            Ok(state) => state,
            Err(err) => self.record_failure(err.to_string()).await,
        }
    }

    pub async fn check_now(&self) -> AutoUpdateState {
        let _guard = self.lock.lock().await;
        match self.run_check(true).await {
            Ok(state) => state,
            Err(err) => self.record_failure(err.to_string()).await,
        }
    }

    #[cfg(unix)]
    pub async fn install_and_restart(&self) -> Result<AutoUpdateState, BoxError> {
        let _guard = self.lock.lock().await;
        match self.run_install_and_restart().await {
            Ok(state) => Ok(state),
            Err(err) => {
                let message = err.to_string();
                self.record_failure(message.clone()).await;
                Err(message.into())
            }
        }
    }

    #[cfg(not(unix))]
    pub async fn install_and_restart(&self) -> Result<AutoUpdateState, BoxError> {
        Err("automatic install and restart is only supported on unix platforms; run `anda update` and restart manually".into())
    }

    async fn run_check(&self, force: bool) -> Result<AutoUpdateState, BoxError> {
        let mut state = self.state();
        let now_ms = unix_ms();
        if !force && !check_due(&state, now_ms) {
            return Ok(state);
        }

        state.status = AutoUpdateStatus::Checking;
        state.current_tag = current_version_tag();
        state.last_checked_ms = Some(now_ms);
        state.error = None;
        self.save_state(&state).await?;

        let latest_tag = updater::fetch_latest_version(&self.http).await?;
        state.latest_tag = Some(latest_tag.clone());
        state.last_checked_ms = Some(unix_ms());

        if !is_newer_release(&latest_tag, &state.current_tag) {
            state.status = AutoUpdateStatus::Current;
            state.downloaded_at_ms = None;
            state.installed_at_ms = None;
            state.target = None;
            state.asset_name = None;
            state.downloaded_path = None;
            state.sha256 = None;
            state.checksum_verified = false;
            self.save_state(&state).await?;
            return Ok(state);
        }

        let target = updater::ReleaseTarget::detect()?;
        let asset_name = target.asset_name();
        let target_name = target.name();
        let downloaded_path = auto_download_path(&self.home_dir, &latest_tag, &asset_name);

        if usable_downloaded_file(&state, &latest_tag, &asset_name, &downloaded_path).await {
            state.status = AutoUpdateStatus::Downloaded;
            state.target = Some(target_name);
            state.asset_name = Some(asset_name);
            state.downloaded_path = Some(downloaded_path.to_string_lossy().to_string());
            self.save_state(&state).await?;
            return Ok(state);
        }

        state.status = AutoUpdateStatus::Downloading;
        state.target = Some(target_name);
        state.asset_name = Some(asset_name.clone());
        state.downloaded_path = None;
        state.sha256 = None;
        state.checksum_verified = false;
        self.save_state(&state).await?;

        let actual_hash =
            download_release_asset(&self.http, &latest_tag, &asset_name, &downloaded_path).await?;

        state.status = AutoUpdateStatus::Downloaded;
        state.downloaded_at_ms = Some(unix_ms());
        state.downloaded_path = Some(downloaded_path.to_string_lossy().to_string());
        state.sha256 = Some(actual_hash);
        state.checksum_verified = true;
        state.error = None;
        self.save_state(&state).await?;
        Ok(state)
    }

    #[cfg(unix)]
    async fn run_install_and_restart(&self) -> Result<AutoUpdateState, BoxError> {
        let mut state = self.state();
        if !state.downloaded_update_available() {
            return Err("no downloaded update is available".into());
        }

        let latest_tag = state.latest_tag.clone().ok_or("missing update tag")?;
        let asset_name = state
            .asset_name
            .clone()
            .ok_or("missing update asset name")?;
        let downloaded_path = state
            .downloaded_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or("missing downloaded update path")?;
        if !usable_downloaded_file(&state, &latest_tag, &asset_name, &downloaded_path).await {
            return Err("downloaded update is missing or failed checksum validation".into());
        }

        let current_exe = std::env::current_exe()?;
        let install_dir = current_exe
            .parent()
            .ok_or("could not detect the current executable directory")?
            .to_path_buf();
        let staged_path = updater::staged_update_path(&install_dir, &current_exe);

        let install_result = async {
            updater::stage_update(&downloaded_path, &staged_path).await?;
            updater::prepare_executable(&staged_path, &current_exe).await?;
            updater::install_update(&staged_path, &current_exe).await?;
            Ok::<(), BoxError>(())
        }
        .await;
        if let Err(err) = install_result {
            let _ = tokio::fs::remove_file(&staged_path).await;
            return Err(err);
        }

        state.status = AutoUpdateStatus::Installed;
        state.installed_at_ms = Some(unix_ms());
        state.error = None;
        self.save_state(&state).await?;
        schedule_restart(current_exe, self.home_dir.clone());
        Ok(state)
    }

    async fn save_state(&self, state: &AutoUpdateState) -> Result<(), BoxError> {
        self.db
            .save_extension_from(AUTO_UPDATE_EXTENSION_KEY.to_string(), state)
            .await?;
        Ok(())
    }

    async fn record_failure(&self, error: String) -> AutoUpdateState {
        let mut state = self.state();
        state.status = AutoUpdateStatus::Failed;
        state.current_tag = current_version_tag();
        state.last_checked_ms = Some(unix_ms());
        state.error = Some(error);
        if let Err(err) = self.save_state(&state).await {
            log::warn!("failed to persist auto update failure state: {err}");
        }
        state
    }
}

#[cfg(unix)]
fn schedule_restart(current_exe: PathBuf, home_dir: PathBuf) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(750)).await;
        let mut command = Command::new(current_exe);
        command
            .arg("--home")
            .arg(home_dir)
            .arg("restart")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Err(err) = command.spawn() {
            log::error!("failed to spawn anda restart after update: {err}");
        }
    });
}

pub async fn downloaded_update_path(
    daemon: &Daemon,
    latest_tag: &str,
    asset_name: &str,
) -> Option<PathBuf> {
    let db = daemon.open_bot_db().await.ok()?;
    let state = read_state(db.as_ref());
    let path = state.downloaded_path.as_deref().map(PathBuf::from)?;
    if !usable_downloaded_file(&state, latest_tag, asset_name, &path).await {
        return None;
    }
    Some(path)
}

pub async fn mark_installed(daemon: &Daemon, latest_tag: &str) {
    let Ok(db) = daemon.open_bot_db().await else {
        return;
    };
    let mut state = read_state(db.as_ref());
    if state.latest_tag.as_deref() != Some(latest_tag) {
        return;
    }
    state.status = AutoUpdateStatus::Installed;
    state.current_tag = current_version_tag();
    state.installed_at_ms = Some(unix_ms());
    state.error = None;
    if let Err(err) = db
        .save_extension_from(AUTO_UPDATE_EXTENSION_KEY.to_string(), &state)
        .await
    {
        log::warn!("failed to persist installed auto update state: {err}");
    }
}

fn read_state(db: &AndaDB) -> AutoUpdateState {
    let mut state = db
        .get_extension_as::<AutoUpdateState>(AUTO_UPDATE_EXTENSION_KEY)
        .unwrap_or_default();
    state.current_tag = current_version_tag();
    state
}

fn current_version_tag() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn check_due(state: &AutoUpdateState, now_ms: u64) -> bool {
    if matches!(
        state.status,
        AutoUpdateStatus::Checking | AutoUpdateStatus::Downloading
    ) {
        return true;
    }
    match state.last_checked_ms {
        Some(last_checked_ms) => now_ms.saturating_sub(last_checked_ms) >= CHECK_INTERVAL_MS,
        None => true,
    }
}

fn is_newer_release(latest_tag: &str, current_tag: &str) -> bool {
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

fn parse_release_version(tag: &str) -> Option<Vec<u64>> {
    let tag = tag.trim().trim_start_matches('v');
    if tag.is_empty() {
        return None;
    }
    let version = tag.split(['-', '+']).next().unwrap_or_default();
    let parts = version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then_some(parts)
}

async fn download_release_asset(
    client: &reqwest::Client,
    latest_tag: &str,
    asset_name: &str,
    destination: &Path,
) -> Result<String, BoxError> {
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let base_url = updater::release_download_base_url(latest_tag);
    let asset_url = format!("{base_url}/{asset_name}");
    let checksum_url = format!("{asset_url}.sha256");
    let temp_path = updater::temporary_download_path(asset_name);
    let actual_hash = updater::download_binary(client, &asset_url, &temp_path).await?;

    match updater::fetch_expected_checksum(client, &checksum_url).await? {
        Some(expected_hash) if expected_hash != actual_hash => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(format!("Checksum verification failed for {asset_name}").into());
        }
        Some(_) | None => {}
    }

    if tokio::fs::rename(&temp_path, destination).await.is_err() {
        tokio::fs::copy(&temp_path, destination).await?;
        tokio::fs::remove_file(&temp_path).await?;
    }
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
        || !path.is_file()
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

async fn sha256_file(path: &Path) -> Result<String, BoxError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(updater::hex_lower(&hasher.finalize()))
}

fn auto_download_path(home_dir: &Path, latest_tag: &str, asset_name: &str) -> PathBuf {
    home_dir
        .join("updates")
        .join(sanitize_path_segment(latest_tag))
        .join(asset_name)
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

    #[test]
    fn version_comparison_detects_newer_release() {
        assert!(is_newer_release("v0.7.8", "v0.7.7"));
        assert!(is_newer_release("v0.8.0", "v0.7.99"));
        assert!(!is_newer_release("v0.7.7", "v0.7.7"));
        assert!(!is_newer_release("v0.7.6", "v0.7.7"));
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
    fn sanitize_path_segments_keep_release_tags_readable() {
        assert_eq!(sanitize_path_segment("v0.7.8"), "v0.7.8");
        assert_eq!(sanitize_path_segment("v0/7/8"), "v0_7_8");
    }

    use anda_db::{database::DBConfig, storage::StorageConfig};
    use object_store::memory::InMemory;

    async fn test_updater() -> AutoUpdater {
        let object_store: Arc<dyn object_store::ObjectStore> = Arc::new(InMemory::new());
        let db = AndaDB::connect(
            object_store,
            DBConfig {
                name: "auto_update_test_db".to_string(),
                description: "auto update test db".to_string(),
                storage: StorageConfig {
                    cache_max_capacity: 1024,
                    compress_level: 1,
                    object_chunk_size: 256 * 1024,
                    bucket_overload_size: 256 * 1024,
                    max_small_object_size: 1024 * 1024,
                },
                lock: None,
            },
        )
        .await
        .unwrap();
        // All HTTP requests are routed through a dead proxy so checks fail
        // fast without touching the network.
        let http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap();
        AutoUpdater::new(Arc::new(db), std::env::temp_dir(), http)
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

        // The same tag as the running build is not an update.
        state.latest_tag = Some(state.current_tag.clone());
        assert!(!state.downloaded_update_available());
    }

    #[tokio::test]
    async fn failed_checks_persist_state_in_db() {
        let updater = test_updater().await;

        let initial = updater.state();
        assert_eq!(initial.status, AutoUpdateStatus::Idle);

        // A forced check hits the dead proxy and records the failure.
        let failed = updater.check_now().await;
        assert_eq!(failed.status, AutoUpdateStatus::Failed);
        assert!(failed.error.is_some());
        assert!(failed.last_checked_ms.is_some());

        // The failure state is durable across reads.
        let reread = updater.state();
        assert_eq!(reread.status, AutoUpdateStatus::Failed);
    }

    #[tokio::test]
    async fn due_checks_are_skipped_when_recently_current() {
        let updater = test_updater().await;
        let mut state = updater.state();
        state.status = AutoUpdateStatus::Current;
        state.last_checked_ms = Some(unix_ms());
        updater.save_state(&state).await.unwrap();

        // Not yet due: returns the stored state without any network call.
        let result = updater.check_if_due().await;
        assert_eq!(result.status, AutoUpdateStatus::Current);
        assert!(result.error.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_requires_a_downloaded_update() {
        let updater = test_updater().await;

        let err = updater.install_and_restart().await.map(|_| ()).unwrap_err();
        assert!(
            err.to_string().contains("no downloaded update"),
            "got: {err}"
        );
    }
}
