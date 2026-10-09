use anda_core::BoxError;
use anda_db::{
    database::{AndaDB, DBConfig},
    storage::StorageConfig,
};
use anda_engine::engine::EngineRef;
use anda_engine_server::shutdown_signal;
use anda_object_store::MetaStoreBuilder;
use object_store::{ObjectStore, local::LocalFileSystem};
use std::{
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinError;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, STILL_ACTIVE},
    System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS, GetExitCodeProcess,
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
    },
};

use crate::{
    auto_update, brain, channel,
    config::{Config, McpSettings},
    cron, engine, gateway, identity, logger, util,
};

const DAEMON_PID_FILE: &str = "anda-daemon.pid";
// Held (flocked / exclusively opened) for the daemon's lifetime. It, not the
// pid file, decides whether a daemon is running: a daemon that died without
// cleanup leaves its pid file behind, and that pid may since belong to an
// unrelated process. The file itself is never deleted — only the lock matters.
const DAEMON_LOCK_FILE: &str = "anda-daemon.lock";

pub struct Daemon {
    pub home: PathBuf,
    pub cfg: Config,
}

pub struct BackgroundDaemon {
    pub pid: u32,
    pub log_path: PathBuf,
    process: Child,
}

impl BackgroundDaemon {
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }
}

pub enum LaunchState {
    AlreadyRunning,
    Started(BackgroundDaemon),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopState {
    NotRunning,
    Stopped(u32),
    StoppedUnknown,
}

impl Daemon {
    pub fn new(home: PathBuf, cfg: Config) -> Self {
        Daemon { home, cfg }
    }

    pub fn base_url(&self) -> String {
        self.cfg.base_url()
    }

    pub fn config_file_path(&self) -> PathBuf {
        Config::file_path(&self.home)
    }

    pub fn pid_file_path(&self) -> PathBuf {
        self.home.join(DAEMON_PID_FILE)
    }

    fn lock_file_path(&self) -> PathBuf {
        self.home.join(DAEMON_LOCK_FILE)
    }

    pub fn keys_dir_path(&self) -> PathBuf {
        self.home.join("keys")
    }

    pub fn db_dir_path(&self) -> PathBuf {
        self.home.join("db")
    }

    pub fn bot_db_config() -> DBConfig {
        DBConfig {
            name: "bot_db".to_string(),
            description: "Anda Brain database".to_string(),
            storage: StorageConfig {
                cache_max_capacity: 100000,
                cache_max_bytes: None,
                compress_level: 3,
                object_chunk_size: 256 * 1024,
                bucket_overload_size: 1024 * 1024,
                max_small_object_size: 1024 * 1024 * 10,
            },
            lock: None,
        }
    }

    fn bot_object_store(&self) -> Result<Arc<dyn ObjectStore>, BoxError> {
        let os = LocalFileSystem::new_with_prefix(self.db_dir_path())?;
        let os = MetaStoreBuilder::new(os, 100000).build();
        Ok(Arc::new(os))
    }

    pub async fn connect_bot_db(&self) -> Result<Arc<AndaDB>, BoxError> {
        tokio::fs::create_dir_all(self.db_dir_path()).await?;
        let db = AndaDB::connect(self.bot_object_store()?, Self::bot_db_config()).await?;
        Ok(Arc::new(db))
    }

    pub fn skills_dir_path(&self) -> PathBuf {
        self.home.join("skills")
    }

    pub fn bundled_skills_dir_path(&self) -> PathBuf {
        self.home.join("bundled-skills")
    }

    pub fn sandbox_dir_path(&self) -> PathBuf {
        self.home.join("sandbox")
    }

    pub fn logs_dir_path(&self) -> PathBuf {
        self.home.join("logs")
    }

    pub fn channels_dir_path(&self) -> PathBuf {
        self.home.join("channels")
    }

    pub fn workspace_dir_path(&self) -> PathBuf {
        self.home.join("workspace")
    }

    pub fn workspaces(&self) -> Vec<PathBuf> {
        let mut workspaces = Vec::new();
        for workspace in &self.cfg.workspaces {
            let path = if workspace.is_absolute() {
                workspace.clone()
            } else {
                self.home.join(workspace)
            };
            push_unique_workspace(&mut workspaces, path);
        }

        for path in [
            self.workspace_dir_path(),
            self.sandbox_dir_path(),
            self.channels_dir_path(),
            self.skills_dir_path(),
        ] {
            push_unique_workspace(&mut workspaces, path);
        }
        workspaces
    }

    pub fn log_file_path(&self) -> PathBuf {
        logger::current_daily_log_file_path(&self.logs_dir_path(), logger::DAEMON_LOG_FILE_PREFIX)
    }

    async fn read_pid_file(&self) -> Result<Option<u32>, BoxError> {
        match util::text::read_text_file(&self.pid_file_path()).await {
            // kill(2) reads 0 and values above i32::MAX as a process group or
            // every process, never as one pid.
            Ok(content) => Ok(content
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|pid| (1..=i32::MAX as u32).contains(pid))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// The pid of the daemon serving this home, if one is running. A pid file
    /// whose daemon lock is free was left by a daemon that died without
    /// cleanup; it is removed instead of trusted.
    pub async fn running_pid(&self) -> Result<Option<u32>, BoxError> {
        let pid = self.read_pid_file().await?;
        if pid.is_some() && !daemon_lock_held(&self.lock_file_path()) {
            remove_file_if_exists(&self.pid_file_path()).await?;
            return Ok(None);
        }
        Ok(pid)
    }

    pub async fn ensure_directories(&self) -> Result<(), BoxError> {
        for dir in [
            self.keys_dir_path(),
            self.db_dir_path(),
            self.skills_dir_path(),
            self.bundled_skills_dir_path(),
            self.sandbox_dir_path(),
            self.logs_dir_path(),
            self.channels_dir_path(),
            self.workspace_dir_path(),
        ] {
            tokio::fs::create_dir_all(dir).await?;
        }
        Ok(())
    }

    pub async fn ensure_config_file_exists(&self) -> Result<bool, BoxError> {
        Config::ensure_file_exists(&self.home).await
    }

    pub async fn load_config_from_disk(&self) -> Result<Config, BoxError> {
        Config::from_file(&self.config_file_path()).await
    }

    pub fn spawn_background_with_identity_secrets(
        &self,
        identity_secrets: Option<&identity::LocalIdentitySecrets>,
    ) -> Result<BackgroundDaemon, BoxError> {
        let exe = std::env::current_exe()?;
        let identity_payload = identity_secrets
            .map(|secrets| secrets.to_encoded())
            .transpose()?;
        let logs_dir = self.logs_dir_path();
        std::fs::create_dir_all(&logs_dir)?;

        let log_path = self.log_file_path();

        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let stdout = stderr.try_clone()?;

        let mut command = Command::new(exe);
        command.arg("--home").arg(&self.home);
        if identity_payload.is_some() {
            command
                .arg("--identity-secrets-stdin")
                .stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::null());
        }
        command
            .arg("daemon")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        configure_background_daemon_command(&mut command);

        let mut child = command.spawn()?;

        if let Some(payload) = identity_payload {
            let mut stdin = child
                .stdin
                .take()
                .ok_or("failed to open daemon identity handoff pipe")?;
            if let Err(err) = stdin.write_all(payload.as_bytes()) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(err.into());
            }
        }

        Ok(BackgroundDaemon {
            pid: child.id(),
            log_path,
            process: child,
        })
    }

    /// Terminates the running daemon `pid` (from [`Self::running_pid`]) and
    /// waits for it to exit.
    pub async fn terminate(&self, pid: u32, timeout: Duration) -> Result<(), BoxError> {
        terminate_process(pid)?;
        wait_for_process_exit(pid, timeout).await?;
        // A forced kill skips the daemon's own pid file cleanup.
        remove_file_if_exists(&self.pid_file_path()).await
    }

    pub async fn wait_for_background_exit(
        &self,
        pid: u32,
        timeout: Duration,
    ) -> Result<(), BoxError> {
        wait_for_process_exit(pid, timeout).await
    }

    /// Takes the daemon singleton lock and records this process's pid. The
    /// guard removes the pid file and releases the lock when dropped.
    pub(crate) async fn acquire_pid_file(&self) -> Result<PidFileGuard, BoxError> {
        // With the lock held no other daemon is running, so any existing pid
        // file is stale and overwritten rather than trusted.
        let lock = acquire_daemon_lock(&self.lock_file_path())?;
        let path = self.pid_file_path();
        tokio::fs::write(&path, std::process::id().to_string()).await?;
        Ok(PidFileGuard { path, _lock: lock })
    }

    pub async fn serve(
        mut self,
        id_key: identity::Ed25519Key,
        user_pubkey: identity::Ed25519PubKey,
    ) -> Result<(), BoxError> {
        let _pid_guard = self.acquire_pid_file().await?;

        let mut addr = self.cfg.socket_addr()?;
        if !addr.ip().is_loopback() {
            log::warn!(
                name = "daemon";
                "gateway binds non-loopback address {addr}: /daemon/status, / and the MCP OAuth callback are reachable without authentication from the network"
            );
        }

        let global_cancel_token = CancellationToken::new();
        tokio::spawn(shutdown_signal(global_cancel_token.clone()));
        let outer_http_client =
            util::http_client::build_http_client(self.cfg.https_proxy.clone(), |client| client)?;
        let auth_http =
            util::http_client::build_http_client(self.cfg.https_proxy.clone(), |client| {
                client
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
            })?;
        let chatgpt =
            crate::chatgpt::ChatGptService::open(&self.home, id_key.as_bytes(), auth_http)?;
        if !self.cfg.setup_issues().is_empty() {
            let api = crate::chatgpt::api::ChatGptApi::for_setup(
                chatgpt.clone(),
                self.home.clone(),
                id_key.id(),
                &user_pubkey,
            );
            if !crate::chatgpt::setup::serve(api, addr, global_cancel_token.clone()).await? {
                return Ok(());
            }
            self.cfg = Config::from_file(&self.config_file_path()).await?;
            addr = self.cfg.socket_addr()?;
        }
        let models = Arc::new(
            self.cfg
                .models_with_chatgpt(outer_http_client.clone(), Some(chatgpt.clone())),
        );
        let mcp = McpSettings::from_file(&self.home).await?;
        let engine_ref: Arc<EngineRef> = Arc::new(EngineRef::new());
        let user_registry = self.cfg.user_registry(user_pubkey.clone())?;
        let default_user = user_registry.default_user();
        let user_pubkeys = user_registry.pubkeys();
        let channel_users = self.cfg.channels.user_bindings(&user_registry)?;
        let mut brain_managers = Vec::with_capacity(user_pubkeys.len() + 1);
        brain_managers.push(id_key.pubkey());
        brain_managers.extend(user_pubkeys.clone());
        let brain_models =
            Arc::new(engine::brain_models_from(models.as_ref()).ok_or("No model found for brain")?);
        let brain_cfg = brain::BrainConfig {
            runtime_config: self.cfg.brain.load_runtime_config(&self.home).await?,
            managers: brain_managers,
            models: brain_models.clone(),
            http_client: outer_http_client.clone(),
        };
        let bot_db = self.connect_bot_db().await?;
        let auto_updater = Arc::new(auto_update::AutoUpdater::new(
            self.home.clone(),
            outer_http_client.clone(),
        ));
        let engine_cfg = engine::EngineConfig {
            id_key,
            managers: user_pubkeys,
            owner: user_pubkey.id(),
            models: models.clone(),
            brain_models,
            brain_base_url: self.cfg.brain_base_url(),
            home_dir: self.home.clone(),
            skills_dir: self.skills_dir_path(),
            workspaces: self.workspaces(),
            tts: self.cfg.tts.clone(),
            transcription: self.cfg.transcription.clone(),
            mcp,
            https_proxy: self.cfg.https_proxy.clone(),
            http_client: outer_http_client.clone(),
            auto_updater,
            gateway_addr: addr,
            chatgpt: Some(chatgpt),
        };

        let cron_runtime =
            Arc::new(cron::CronRuntime::connect(engine_ref.clone(), bot_db.clone()).await?);
        let cron_handle = cron_runtime
            .as_ref()
            .clone()
            .serve(global_cancel_token.child_token())
            .await?;

        let channel_runtime = channel::ChannelRuntime::connect(
            bot_db.clone(),
            engine_ref.clone(),
            default_user,
            channel_users,
            channel::build_channels(&self.cfg.channels, outer_http_client)?,
            self.channels_dir_path(),
        )
        .await?
        .with_admission(cron_runtime.admission.clone());
        let channel_hook = channel_runtime.hook();
        let channel_sender = channel_runtime.sender();

        // The gateway gets the root token (not a child) because its
        // /daemon/shutdown route cancels it, and that must propagate to the
        // cron and channel child tokens as well.
        let gateway_handle = gateway::serve(
            global_cancel_token.clone(),
            bot_db,
            brain_cfg,
            engine_cfg,
            engine_ref,
            cron_runtime,
            vec![channel_hook],
            channel_sender,
        )
        .await?;

        // Start channel listeners only after gateway::serve returned, which
        // guarantees the engine is built and bound: IM messages that arrive
        // before the engine is ready would be acked upstream and then dropped.
        let channel_handle = channel_runtime.serve(global_cancel_token.child_token());

        // shutdown_signal only completes on an OS signal; joining it would
        // keep the process alive forever after an HTTP-triggered shutdown.
        // Fail fast: if any subsystem exits (error or panic), cancel the rest
        // instead of leaving a half-alive daemon (e.g. cron and channels
        // running with no HTTP gateway).
        let (first, _, remaining) =
            futures::future::select_all([cron_handle, channel_handle, gateway_handle]).await;
        global_cancel_token.cancel();

        let mut first_error = task_error(first);
        for handle in remaining {
            if let Some(error) = task_error(handle.await) {
                if first_error.is_none() {
                    first_error = Some(error);
                } else {
                    log::error!(name = "daemon"; "daemon subsystem failed during shutdown: {error}");
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }
}

fn task_error(result: Result<Result<(), BoxError>, JoinError>) -> Option<BoxError> {
    match result {
        Ok(result) => result.err(),
        Err(join_err) => Some(join_err.into()),
    }
}

fn push_unique_workspace(workspaces: &mut Vec<PathBuf>, path: PathBuf) {
    if !workspaces.contains(&path) {
        workspaces.push(path);
    }
}

pub(crate) struct PidFileGuard {
    path: PathBuf,
    // Keeps the daemon lock file exclusively held for the process lifetime.
    _lock: std::fs::File,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn acquire_daemon_lock(lock_path: &Path) -> Result<std::fs::File, BoxError> {
    use std::os::fd::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    let rt = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rt == 0 {
        return Ok(file);
    }

    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
    {
        return Err("anda daemon is already running (daemon lock is held)".into());
    }
    Err(err.into())
}

#[cfg(windows)]
fn acquire_daemon_lock(lock_path: &Path) -> Result<std::fs::File, BoxError> {
    use std::os::windows::fs::OpenOptionsExt;

    // share_mode(0): no other process can open the file while we hold it.
    match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(lock_path)
    {
        Ok(file) => Ok(file),
        Err(err) if err.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
            Err("anda daemon is already running (daemon lock is held)".into())
        }
        Err(err) => Err(err.into()),
    }
}

/// Whether a daemon holds the lock. The shared probe conflicts only with the
/// daemon's exclusive lock, so concurrent probes never fail each other.
#[cfg(unix)]
fn daemon_lock_held(lock_path: &Path) -> bool {
    use std::os::fd::AsRawFd;

    let Ok(file) = std::fs::File::open(lock_path) else {
        return false;
    };
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) != 0 }
}

/// Whether a daemon holds the lock: it opened the file with share_mode(0).
#[cfg(windows)]
fn daemon_lock_held(lock_path: &Path) -> bool {
    matches!(
        std::fs::File::open(lock_path),
        Err(err) if err.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
    )
}

#[cfg(windows)]
const ERROR_SHARING_VIOLATION: i32 = 32;

async fn remove_file_if_exists(path: &Path) -> Result<(), BoxError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> Result<(), BoxError> {
    let rt = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if rt == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(code) if code == libc::ESRCH) {
        return Ok(());
    }

    Err(err.into())
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> Result<(), BoxError> {
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        if !process_exists(pid) {
            return Ok(());
        }
        return Err(io::Error::last_os_error().into());
    }

    let result = unsafe { TerminateProcess(handle, 1) };
    let err = io::Error::last_os_error();
    unsafe {
        CloseHandle(handle);
    }

    if result != 0 || !process_exists(pid) {
        return Ok(());
    }

    Err(err.into())
}

async fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), BoxError> {
    let deadline = Instant::now() + timeout;

    loop {
        if !process_exists(pid) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for anda daemon pid {pid} to stop after {timeout:?}"
            )
            .into());
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(unix)]
fn configure_background_daemon_command(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_background_daemon_command(command: &mut Command) {
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    // Signal 0 sends nothing: it only checks that the pid exists and whether
    // this process may signal it (EPERM still means it exists).
    let rt = unsafe { libc::kill(pid as i32, 0) };
    if rt == 0 {
        return true;
    }

    matches!(std::io::Error::last_os_error().raw_os_error(), Some(code) if code == libc::EPERM)
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }

    let mut exit_code = 0;
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe {
        CloseHandle(handle);
    }

    ok && exit_code == STILL_ACTIVE as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon_at(home: &str) -> Daemon {
        Daemon::new(PathBuf::from(home), Config::default())
    }

    #[test]
    fn daemon_paths_are_rooted_under_home() {
        let daemon = daemon_at("/tmp/anda-home");

        assert_eq!(
            daemon.config_file_path(),
            PathBuf::from("/tmp/anda-home/config.yaml")
        );
        assert_eq!(
            daemon.pid_file_path(),
            PathBuf::from("/tmp/anda-home/anda-daemon.pid")
        );
        assert_eq!(daemon.keys_dir_path(), PathBuf::from("/tmp/anda-home/keys"));
        assert_eq!(daemon.db_dir_path(), PathBuf::from("/tmp/anda-home/db"));
        assert_eq!(
            daemon.skills_dir_path(),
            PathBuf::from("/tmp/anda-home/skills")
        );
        assert_eq!(
            daemon.sandbox_dir_path(),
            PathBuf::from("/tmp/anda-home/sandbox")
        );
        assert_eq!(daemon.logs_dir_path(), PathBuf::from("/tmp/anda-home/logs"));
        assert_eq!(
            daemon.channels_dir_path(),
            PathBuf::from("/tmp/anda-home/channels")
        );
        assert_eq!(
            daemon.workspace_dir_path(),
            PathBuf::from("/tmp/anda-home/workspace")
        );
    }

    #[test]
    fn workspaces_include_runtime_writable_directories() {
        let daemon = daemon_at("/tmp/anda-home");

        assert_eq!(
            daemon.workspaces(),
            vec![
                PathBuf::from("/tmp/anda-home/workspace"),
                PathBuf::from("/tmp/anda-home/sandbox"),
                PathBuf::from("/tmp/anda-home/channels"),
                PathBuf::from("/tmp/anda-home/skills"),
            ]
        );
    }

    #[test]
    fn configured_workspaces_are_first_and_deduplicated() {
        let config = Config {
            workspaces: vec![
                PathBuf::from("/workspace/task"),
                PathBuf::from("sandbox"),
                PathBuf::from("/workspace/task"),
            ],
            ..Default::default()
        };
        let daemon = Daemon::new(PathBuf::from("/tmp/anda-home"), config);

        assert_eq!(
            daemon.workspaces(),
            vec![
                PathBuf::from("/workspace/task"),
                PathBuf::from("/tmp/anda-home/sandbox"),
                PathBuf::from("/tmp/anda-home/workspace"),
                PathBuf::from("/tmp/anda-home/channels"),
                PathBuf::from("/tmp/anda-home/skills"),
            ]
        );
    }

    #[test]
    fn bot_db_config_matches_runtime_storage_defaults() {
        let config = Daemon::bot_db_config();

        assert_eq!(config.name, "bot_db");
        assert_eq!(config.description, "Anda Brain database");
        assert_eq!(config.storage.cache_max_capacity, 100000);
        assert_eq!(config.storage.compress_level, 3);
        assert_eq!(config.storage.object_chunk_size, 256 * 1024);
        assert_eq!(config.storage.bucket_overload_size, 1024 * 1024);
        assert_eq!(config.storage.max_small_object_size, 1024 * 1024 * 10);
        assert!(config.lock.is_none());
    }

    #[test]
    fn base_url_delegates_to_config_address() {
        let config = Config {
            addr: "0.0.0.0:9000".to_string(),
            ..Config::default()
        };
        let daemon = Daemon::new(PathBuf::from("/tmp/anda-home"), config);

        assert_eq!(daemon.base_url(), "http://127.0.0.1:9000");
    }

    #[cfg(unix)]
    #[test]
    fn current_process_is_detected_as_existing() {
        assert!(process_exists(std::process::id()));
    }

    fn temp_daemon() -> (tempfile::TempDir, Daemon) {
        let dir = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(dir.path().to_path_buf(), Config::default());
        (dir, daemon)
    }

    // A pid that almost certainly refers to no live process: pid_max on Linux
    // defaults to 4 million and macOS pids stay below 100k.
    const DEAD_PID: u32 = 4_000_000;

    #[tokio::test]
    async fn read_pid_file_accepts_only_single_process_pids() {
        let (_dir, daemon) = temp_daemon();

        assert_eq!(daemon.read_pid_file().await.unwrap(), None);
        for content in ["not a pid", "0", "4294967295", "-1"] {
            tokio::fs::write(daemon.pid_file_path(), content)
                .await
                .unwrap();
            assert_eq!(daemon.read_pid_file().await.unwrap(), None, "{content}");
        }

        tokio::fs::write(daemon.pid_file_path(), " 12345 \n")
            .await
            .unwrap();
        assert_eq!(daemon.read_pid_file().await.unwrap(), Some(12345));
    }

    #[tokio::test]
    async fn ensure_directories_creates_runtime_layout() {
        let (_dir, daemon) = temp_daemon();

        daemon.ensure_directories().await.unwrap();

        for path in [
            daemon.keys_dir_path(),
            daemon.db_dir_path(),
            daemon.skills_dir_path(),
            daemon.bundled_skills_dir_path(),
            daemon.sandbox_dir_path(),
            daemon.logs_dir_path(),
            daemon.channels_dir_path(),
            daemon.workspace_dir_path(),
        ] {
            assert!(path.is_dir(), "missing directory {path:?}");
        }

        let log_path = daemon.log_file_path();
        assert!(log_path.starts_with(daemon.logs_dir_path()));
    }

    #[tokio::test]
    async fn ensure_config_file_round_trips_from_disk() {
        let (_dir, daemon) = temp_daemon();

        let created = daemon.ensure_config_file_exists().await.unwrap();
        assert!(created);
        let created_again = daemon.ensure_config_file_exists().await.unwrap();
        assert!(!created_again);

        let config = daemon.load_config_from_disk().await.unwrap();
        assert!(!config.addr.is_empty());
    }

    #[tokio::test]
    async fn connect_bot_db_creates_database_directory() {
        let (_dir, daemon) = temp_daemon();

        let db = daemon.connect_bot_db().await.unwrap();
        assert!(daemon.db_dir_path().is_dir());
        drop(db);
    }

    #[tokio::test]
    async fn acquire_pid_file_writes_and_cleans_up_pid() {
        let (_dir, daemon) = temp_daemon();
        let pid_path = daemon.pid_file_path();

        let guard = daemon.acquire_pid_file().await.unwrap();
        let content = tokio::fs::read_to_string(&pid_path).await.unwrap();
        assert_eq!(content, std::process::id().to_string());
        assert_eq!(
            daemon.running_pid().await.unwrap(),
            Some(std::process::id())
        );

        // A second daemon is refused while the lock is held.
        let err = daemon.acquire_pid_file().await.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("already running"));

        drop(guard);
        assert!(!pid_path.exists());
        assert_eq!(daemon.running_pid().await.unwrap(), None);
    }

    #[tokio::test]
    async fn pid_file_without_the_daemon_lock_is_stale_even_if_its_pid_lives() {
        let (_dir, daemon) = temp_daemon();
        let pid_path = daemon.pid_file_path();

        // A daemon that died without cleanup left its pid behind, and that
        // pid now belongs to an unrelated live process (this test process
        // stands in for it: it does not hold the daemon lock).
        let reused = std::process::id().to_string();
        tokio::fs::write(&pid_path, &reused).await.unwrap();
        assert_eq!(daemon.running_pid().await.unwrap(), None);
        assert!(!pid_path.exists());

        // A starting daemon takes over instead of refusing to start.
        tokio::fs::write(&pid_path, &reused).await.unwrap();
        let guard = daemon.acquire_pid_file().await.unwrap();
        assert_eq!(
            daemon.running_pid().await.unwrap(),
            Some(std::process::id())
        );
        drop(guard);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminate_stops_the_process_and_removes_its_pid_file() {
        let (_dir, daemon) = temp_daemon();
        let pid = crate::test_support::spawn_orphan_sleeper();
        assert!(process_exists(pid));
        tokio::fs::write(daemon.pid_file_path(), pid.to_string())
            .await
            .unwrap();

        daemon
            .terminate(pid, Duration::from_secs(10))
            .await
            .unwrap();

        assert!(!process_exists(pid));
        assert!(!daemon.pid_file_path().exists());
    }

    #[tokio::test]
    async fn wait_for_background_exit_times_out_on_live_process() {
        let (_dir, daemon) = temp_daemon();

        daemon
            .wait_for_background_exit(DEAD_PID, Duration::from_secs(1))
            .await
            .unwrap();

        let err = daemon
            .wait_for_background_exit(std::process::id(), Duration::ZERO)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serve_exits_promptly_after_http_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let config = Config {
            addr: format!("127.0.0.1:{port}"),
            model: crate::config::ModelSettings {
                active: "fake-model".to_string(),
                providers: vec![crate::config::ModelProvider {
                    family: "openai".to_string(),
                    model: "fake-model".to_string(),
                    api_base: "http://127.0.0.1:1/v1".to_string(),
                    api_key: "fake-key".to_string(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let daemon = Daemon::new(dir.path().to_path_buf(), config);
        daemon.ensure_directories().await.unwrap();
        let base_url = daemon.base_url();

        let id_key = identity::Ed25519Key::new(identity::random_ed25519_privkey());
        let user_key = identity::Ed25519Key::new(identity::random_ed25519_privkey());
        let mut claims = identity::expiring_claims(Duration::from_secs(60)).unwrap();
        claims.extra.insert(identity::iana::CWTClaimScope, "*");
        let token = user_key.sign_cwt(claims).unwrap();
        let user_pubkey = user_key.pubkey();

        let mut serve_handle = tokio::spawn(daemon.serve(id_key, user_pubkey));

        let client = crate::gateway::Client::new(base_url, token);
        // The gateway binds only after the Brain space and every engine are
        // built on this fresh home: under a second on a dev machine, ~9s on a
        // Windows CI runner that also stalls for seconds at a time. Startup
        // speed is not under test, so the cap only tells slow from dead, and a
        // daemon that dies early is reported as itself, not as a timeout.
        tokio::select! {
            ready = client.wait_for_daemon_ready(Duration::from_secs(120)) => ready.unwrap(),
            exited = &mut serve_handle => {
                panic!("daemon exited before it became ready: {exited:?}")
            }
        }
        client.shutdown().await.unwrap();

        tokio::time::timeout(Duration::from_secs(5), serve_handle)
            .await
            .expect("daemon did not exit within 5s after HTTP shutdown")
            .unwrap()
            .unwrap();
    }
}
