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
    io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::{auto_update, brain, channel, config::Config, cron, engine, gateway, logger, util};

const DAEMON_PID_FILE: &str = "anda-daemon.pid";

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

    pub fn default_config_template() -> &'static str {
        Config::default_template()
    }

    pub fn pid_file_path(&self) -> PathBuf {
        self.home.join(DAEMON_PID_FILE)
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

    pub async fn open_bot_db(&self) -> Result<Arc<AndaDB>, BoxError> {
        let db = AndaDB::open(self.bot_object_store()?, Self::bot_db_config()).await?;
        Ok(Arc::new(db))
    }

    pub fn skills_dir_path(&self) -> PathBuf {
        self.home.join("skills")
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
        logger::current_daily_log_file_path(self.logs_dir_path(), logger::DAEMON_LOG_FILE_PREFIX)
    }

    pub async fn read_pid_file(&self) -> Result<Option<u32>, BoxError> {
        let pid_path = self.pid_file_path();
        match tokio::fs::read_to_string(pid_path).await {
            Ok(content) => Ok(content.trim().parse::<u32>().ok()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn ensure_directories(&self) -> Result<(), BoxError> {
        tokio::fs::create_dir_all(self.keys_dir_path()).await?;
        tokio::fs::create_dir_all(self.db_dir_path()).await?;
        tokio::fs::create_dir_all(self.skills_dir_path()).await?;
        tokio::fs::create_dir_all(self.sandbox_dir_path()).await?;
        tokio::fs::create_dir_all(self.logs_dir_path()).await?;
        tokio::fs::create_dir_all(self.channels_dir_path()).await?;
        tokio::fs::create_dir_all(self.workspace_dir_path()).await?;
        Ok(())
    }

    pub async fn ensure_config_file_exists(&self) -> Result<bool, BoxError> {
        let config_path = self.config_file_path();
        match tokio::fs::metadata(&config_path).await {
            Ok(_) => Ok(false),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir_all(&self.home).await?;
                tokio::fs::write(&config_path, Self::default_config_template()).await?;
                Ok(true)
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn load_config_from_disk(&self) -> Result<Config, BoxError> {
        Config::from_file(&self.config_file_path()).await
    }

    pub fn spawn_background(&self) -> Result<BackgroundDaemon, BoxError> {
        let exe = std::env::current_exe()?;
        let logs_dir = self.logs_dir_path();
        std::fs::create_dir_all(&logs_dir)?;

        let log_path = self.log_file_path();

        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let stdout = stderr.try_clone()?;

        let mut command = Command::new(exe);
        command
            .arg("--home")
            .arg(&self.home)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        configure_background_daemon_command(&mut command);

        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                log::error!("Failed to spawn background daemon process: {err}");
                return Err(err.into());
            }
        };

        Ok(BackgroundDaemon {
            pid: child.id(),
            log_path,
            process: child,
        })
    }

    #[cfg(unix)]
    pub async fn stop_background(&self, timeout: Duration) -> Result<StopState, BoxError> {
        let pid_path = self.pid_file_path();
        let Some(pid) = self.read_pid_file().await? else {
            remove_file_if_exists(&pid_path).await?;
            return Ok(StopState::NotRunning);
        };

        if !process_exists(pid) {
            remove_file_if_exists(&pid_path).await?;
            return Ok(StopState::NotRunning);
        }

        terminate_process(pid)?;
        wait_for_process_exit(pid, timeout).await?;
        remove_file_if_exists(&pid_path).await?;
        Ok(StopState::Stopped(pid))
    }

    #[cfg(not(unix))]
    pub async fn stop_background(&self, _timeout: Duration) -> Result<StopState, BoxError> {
        Err("anda daemon stop/restart is only supported on unix platforms".into())
    }

    pub async fn serve(
        self,
        id_key: util::key::Ed25519Key,
        user_pubkey: util::key::Ed25519PubKey,
    ) -> Result<(), BoxError> {
        let _pid_guard = acquire_pid_file(self.pid_file_path()).await?;

        let setup_issues = self.cfg.setup_issues();
        if !setup_issues.is_empty() {
            return Err(format!(
                "runtime configuration is incomplete: {}",
                setup_issues.join(", ")
            )
            .into());
        }

        // Create global cancellation token for graceful shutdown
        let global_cancel_token = CancellationToken::new();
        let outer_http_client =
            util::http_client::build_http_client(self.cfg.https_proxy.clone(), |client| client)?;
        let models = self.cfg.models(outer_http_client.clone());
        let engine_ref: Arc<EngineRef> = Arc::new(EngineRef::new());
        let user_id = user_pubkey.id();
        let brain_cfg = brain::BrainConfig {
            managers: vec![id_key.pubkey(), user_pubkey.clone()],
            model: models
                .get("brain")
                .or_else(|| models.get("memory"))
                .or_else(|| models.get_model())
                .ok_or("No model found for brain")?,
            https_proxy: self.cfg.https_proxy.clone(),
        };
        let bot_db = self.connect_bot_db().await?;
        let auto_updater = Arc::new(auto_update::AutoUpdater::new(
            bot_db.clone(),
            self.home.clone(),
            outer_http_client.clone(),
        ));
        let engine_cfg = engine::EngineConfig {
            id_key,
            managers: vec![user_pubkey],
            models,
            brain_base_url: self.cfg.brain_base_url(),
            home_dir: self.home.clone(),
            skills_dir: self.skills_dir_path(),
            workspaces: self.workspaces(),
            tts: self.cfg.tts.clone(),
            transcription: self.cfg.transcription.clone(),
            https_proxy: self.cfg.https_proxy.clone(),
            auto_updater,
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
            user_id,
            channel::build_channels(&self.cfg.channels, outer_http_client)?,
            self.channels_dir_path(),
        )
        .await?;
        let channel_hook = channel_runtime.hook();
        let active_channels = channel_runtime.active_channels();
        let channel_handle = channel_runtime
            .serve(global_cancel_token.child_token())
            .await?;

        let gateway_handle = gateway::serve(
            global_cancel_token.child_token(),
            bot_db,
            self.cfg.addr.clone(),
            brain_cfg,
            engine_cfg,
            engine_ref,
            cron_runtime,
            vec![channel_hook],
            active_channels,
        )
        .await?;

        let terminate_handle = shutdown_signal(global_cancel_token);
        let _ = tokio::join!(
            cron_handle,
            channel_handle,
            gateway_handle,
            terminate_handle
        );

        Ok(())
    }
}

fn push_unique_workspace(workspaces: &mut Vec<PathBuf>, path: PathBuf) {
    if !workspaces.contains(&path) {
        workspaces.push(path);
    }
}

struct PidFileGuard {
    path: PathBuf,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn acquire_pid_file(pid_path: PathBuf) -> Result<PidFileGuard, BoxError> {
    loop {
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pid_path)
            .await
        {
            Ok(mut file) => {
                let pid = std::process::id().to_string();
                if let Err(err) = file.write_all(pid.as_bytes()).await {
                    let _ = tokio::fs::remove_file(&pid_path).await;
                    return Err(err.into());
                }
                return Ok(PidFileGuard { path: pid_path });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                match tokio::fs::read_to_string(&pid_path).await {
                    Ok(content) => {
                        let existing_pid = content.trim().parse::<u32>().ok();
                        if let Some(pid) = existing_pid
                            && process_exists(pid)
                        {
                            return Err(
                                format!("anda daemon is already running with pid {pid}").into()
                            );
                        }
                        let _ = tokio::fs::remove_file(&pid_path).await;
                    }
                    Err(read_err) if read_err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(read_err) => return Err(read_err.into()),
                }
            }
            Err(err) => return Err(err.into()),
        }
    }
}

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

#[cfg(unix)]
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

#[cfg(not(unix))]
fn configure_background_daemon_command(_command: &mut Command) {}

#[cfg(unix)]
pub fn process_exists(pid: u32) -> bool {
    // Unix 的一个约定：当信号值是 0 时，kill(pid, 0) 不会真的发送信号，只会让内核检查两件事：
    // 1. 这个 pid 对应的进程是否存在。
    // 2. 当前进程有没有权限向它发信号。
    let rt = unsafe { libc::kill(pid as i32, 0) };
    if rt == 0 {
        return true;
    }

    matches!(std::io::Error::last_os_error().raw_os_error(), Some(code) if code == libc::EPERM)
}

#[cfg(not(unix))]
pub fn process_exists(_pid: u32) -> bool {
    false
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
}
