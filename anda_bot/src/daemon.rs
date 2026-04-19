use anda_core::BoxError;
use anda_engine_server::shutdown_signal;
use anda_hippocampus::types::ModelConfig;
use anda_object_store::MetaStoreBuilder;
use clap::Args;
use object_store::{ObjectStore, local::LocalFileSystem};
use std::{
    fs::OpenOptions as StdOpenOptions,
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use structured_logger::{Builder, async_json::new_writer, get_env_level};
use tokio::{
    fs::OpenOptions,
    io::AsyncWriteExt,
    time::{Instant, sleep},
};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::{brain, cron, engine, gateway, util, util::env::*};

const DAEMON_PID_FILE: &str = "anda-daemon.pid";
const DAEMON_LOG_FILE: &str = "anda-daemon.log";
const DEFAULT_GATEWAY_ADDR: &str = "127.0.0.1:8042";
const ENV_GATEWAY_ADDR: &str = "GATEWAY_ADDR";
const ENV_SANDBOX: &str = "SANDBOX";
const ENV_MODEL_FAMILY: &str = "MODEL_FAMILY";
const ENV_MODEL_NAME: &str = "MODEL_NAME";
const ENV_MODEL_API_KEY: &str = "MODEL_API_KEY";
const ENV_MODEL_API_BASE: &str = "MODEL_API_BASE";
const ENV_HTTPS_PROXY: &str = "HTTPS_PROXY";

pub struct Daemon {
    pub home: PathBuf,
    pub cfg: DaemonArgs,
}

pub struct BackgroundDaemon {
    pub pid: u32,
    pub log_path: PathBuf,
}

#[derive(Args, Clone, Debug, PartialEq, Eq)]
pub struct DaemonArgs {
    #[clap(long, env = ENV_GATEWAY_ADDR, default_value = DEFAULT_GATEWAY_ADDR)]
    pub addr: String,

    #[arg(long, env = ENV_SANDBOX, default_value = "false")]
    pub sandbox: bool,

    /// AI model family (e.g., "gemini", "anthropic", "openai", "deepseek")
    #[arg(long, env = ENV_MODEL_FAMILY, default_value = "")]
    pub model_family: String,

    /// AI model name (e.g., "gemini-3-flash-preview", "claude-sonnet-4-6")
    #[arg(long, env = ENV_MODEL_NAME, default_value = "")]
    pub model_name: String,

    /// API key for AI model
    #[arg(long, env = ENV_MODEL_API_KEY, default_value = "")]
    pub model_api_key: String,

    /// API base URL for AI model
    #[arg(long, env = ENV_MODEL_API_BASE, default_value = "")]
    pub model_api_base: String,

    /// Optional HTTPS proxy URL (e.g., "http://127.0.0.1:23456")
    #[arg(long, env = ENV_HTTPS_PROXY)]
    pub https_proxy: Option<String>,
}

impl DaemonArgs {
    pub async fn save_to_env_file(&self, path: &Path) -> Result<(), BoxError> {
        let existing = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err.into()),
        };

        let merged = merge_env_file(&existing, &self.env_updates());
        tokio::fs::write(path, merged).await?;
        Ok(())
    }

    pub fn apply_command_env(&self, command: &mut Command) {
        command.env(ENV_GATEWAY_ADDR, &self.addr);
        command.env(ENV_SANDBOX, if self.sandbox { "true" } else { "false" });
        command.env(ENV_MODEL_FAMILY, &self.model_family);
        command.env(ENV_MODEL_NAME, &self.model_name);
        command.env(ENV_MODEL_API_KEY, &self.model_api_key);
        command.env(ENV_MODEL_API_BASE, &self.model_api_base);
        if let Some(proxy) = self.https_proxy.as_deref() {
            command.env(ENV_HTTPS_PROXY, proxy);
        } else {
            command.env_remove(ENV_HTTPS_PROXY);
        }
    }

    pub fn from_env() -> Self {
        Self {
            addr: env_string(ENV_GATEWAY_ADDR, DEFAULT_GATEWAY_ADDR),
            sandbox: env_bool(ENV_SANDBOX),
            model_family: env_string(ENV_MODEL_FAMILY, ""),
            model_name: env_string(ENV_MODEL_NAME, ""),
            model_api_key: env_string(ENV_MODEL_API_KEY, ""),
            model_api_base: env_string(ENV_MODEL_API_BASE, ""),
            https_proxy: env_option(ENV_HTTPS_PROXY),
        }
    }

    pub fn model_config(&self) -> ModelConfig {
        ModelConfig {
            family: self.model_family.clone(),
            model: self.model_name.clone(),
            api_base: self.model_api_base.clone(),
            api_key: self.model_api_key.clone(),
            disabled: false,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn brain_base_url(&self) -> String {
        format!("http://{}/v1/{}", self.addr, brain::ANDA_BOT_SPACE_ID)
    }

    fn env_updates(&self) -> Vec<EnvUpdate> {
        vec![
            EnvUpdate::set(ENV_GATEWAY_ADDR, self.addr.clone()),
            EnvUpdate::set(
                ENV_SANDBOX,
                if self.sandbox { "true" } else { "false" }.to_string(),
            ),
            EnvUpdate::set(ENV_MODEL_FAMILY, self.model_family.clone()),
            EnvUpdate::set(ENV_MODEL_NAME, self.model_name.clone()),
            EnvUpdate::set(ENV_MODEL_API_KEY, self.model_api_key.clone()),
            EnvUpdate::set(ENV_MODEL_API_BASE, self.model_api_base.clone()),
            EnvUpdate::optional(ENV_HTTPS_PROXY, self.https_proxy.clone()),
        ]
    }
}

impl Daemon {
    pub fn new(home: PathBuf, cfg: DaemonArgs) -> Self {
        Daemon { home, cfg }
    }

    pub fn base_url(&self) -> String {
        self.cfg.base_url()
    }

    pub fn env_file_path(&self) -> PathBuf {
        self.home.join(".env")
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

    pub fn skills_dir_path(&self) -> PathBuf {
        self.home.join("skills")
    }

    pub fn sandbox_dir_path(&self) -> PathBuf {
        self.home.join("sandbox")
    }

    pub fn logs_dir_path(&self) -> PathBuf {
        self.home.join("logs")
    }

    pub fn log_file_path(&self) -> PathBuf {
        self.logs_dir_path().join(DAEMON_LOG_FILE)
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
        Ok(())
    }

    pub async fn persist_config(&self) -> Result<(), BoxError> {
        self.cfg.save_to_env_file(&self.env_file_path()).await
    }

    pub fn spawn_background(&self) -> Result<BackgroundDaemon, BoxError> {
        let exe = std::env::current_exe()?;
        let logs_dir = self.logs_dir_path();
        std::fs::create_dir_all(&logs_dir)?;

        let log_path = self.log_file_path();
        let stdout = StdOpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let stderr = StdOpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let mut command = Command::new(exe);
        command
            .arg("--home")
            .arg(&self.home)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        self.cfg.apply_command_env(&mut command);
        configure_background_daemon_command(&mut command);

        let child = command.spawn()?;

        Ok(BackgroundDaemon {
            pid: child.id(),
            log_path,
        })
    }

    pub async fn stop_background(&self, timeout: Duration) -> Result<bool, BoxError> {
        let pid_path = self.pid_file_path();
        let Some(pid) = self.read_pid_file().await? else {
            return Ok(false);
        };

        if !process_exists(pid) {
            let _ = tokio::fs::remove_file(&pid_path).await;
            return Ok(false);
        }

        terminate_process(pid)?;
        if wait_for_process_exit(pid, timeout).await {
            let _ = tokio::fs::remove_file(&pid_path).await;
            return Ok(true);
        }

        force_kill_process(pid)?;
        if wait_for_process_exit(pid, Duration::from_secs(2)).await {
            let _ = tokio::fs::remove_file(&pid_path).await;
            return Ok(true);
        }

        Err(format!(
            "anda daemon with pid {pid} did not stop within {:?}",
            timeout
        )
        .into())
    }

    pub async fn serve(
        self,
        id_key: util::key::Ed25519Key,
        user_pubkey: util::key::Ed25519PubKey,
    ) -> Result<(), BoxError> {
        // Initialize structured logging with JSON format
        Builder::with_level(&get_env_level().to_string())
            .with_target_writer("*", new_writer(tokio::io::stdout()))
            .init();

        let _pid_guard = acquire_pid_file(self.pid_file_path()).await?;

        // Create global cancellation token for graceful shutdown
        let global_cancel_token = CancellationToken::new();

        let brain_cfg = brain::HippocampusConfig {
            managers: vec![id_key.pubkey(), user_pubkey.clone()],
            model: self.cfg.model_config(),
            https_proxy: self.cfg.https_proxy.clone(),
        };
        let engine_cfg = engine::EngineConfig {
            id_key,
            managers: vec![user_pubkey],
            model: self.cfg.model_config(),
            brain_base_url: self.cfg.brain_base_url(),
            work_dir: std::env::current_dir()?,
            skills_dir: self.skills_dir_path(),
            sandbox_dir: if self.cfg.sandbox {
                Some(self.sandbox_dir_path())
            } else {
                None
            },
            https_proxy: self.cfg.https_proxy.clone(),
        };
        let db_path = self.db_dir_path();
        let object_store: Arc<dyn ObjectStore> = {
            let os = LocalFileSystem::new_with_prefix(db_path)?;
            let os = MetaStoreBuilder::new(os, 100000).build();
            Arc::new(os)
        };

        let cron_handle = cron::serve(global_cancel_token.child_token()).await?;
        let gateway_handle = gateway::serve(
            global_cancel_token.child_token(),
            object_store,
            self.cfg.addr.clone(),
            brain_cfg,
            engine_cfg,
        )
        .await?;

        let terminate_handle = shutdown_signal(global_cancel_token);
        let _ = tokio::join!(cron_handle, gateway_handle, terminate_handle);

        Ok(())
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

async fn wait_for_process_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !process_exists(pid) {
            return true;
        }

        if Instant::now() >= deadline {
            return false;
        }

        sleep(Duration::from_millis(250)).await;
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
fn terminate_process(pid: u32) -> Result<(), BoxError> {
    send_signal(pid, libc::SIGTERM)
}

#[cfg(not(unix))]
fn terminate_process(pid: u32) -> Result<(), BoxError> {
    taskkill(pid, false)
}

#[cfg(unix)]
fn force_kill_process(pid: u32) -> Result<(), BoxError> {
    send_signal(pid, libc::SIGKILL)
}

#[cfg(not(unix))]
fn force_kill_process(pid: u32) -> Result<(), BoxError> {
    taskkill(pid, true)
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: i32) -> Result<(), BoxError> {
    let rt = unsafe { libc::kill(pid as i32, signal) };
    if rt == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    if matches!(err.raw_os_error(), Some(code) if code == libc::ESRCH) {
        return Ok(());
    }

    Err(err.into())
}

#[cfg(not(unix))]
fn taskkill(pid: u32, force: bool) -> Result<(), BoxError> {
    let mut command = Command::new("taskkill");
    command.arg("/PID").arg(pid.to_string()).arg("/T");
    if force {
        command.arg("/F");
    }

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill failed for pid {pid} with status {status}").into())
    }
}

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

    #[test]
    fn merge_env_file_preserves_unknown_lines_and_updates_targets() {
        let args = DaemonArgs {
            addr: "127.0.0.1:9999".to_string(),
            sandbox: true,
            model_family: "anthropic".to_string(),
            model_name: "claude-sonnet-4-6".to_string(),
            model_api_key: "secret".to_string(),
            model_api_base: "https://example.com/v1".to_string(),
            https_proxy: Some("http://127.0.0.1:7890".to_string()),
        };
        let existing = "# comment\nOTHER=keep\nMODEL_NAME=old\nHTTPS_PROXY=http://old\n";

        let merged = merge_env_file(existing, &args.env_updates());

        assert!(merged.contains("# comment"));
        assert!(merged.contains("OTHER=keep"));
        assert!(merged.contains("GATEWAY_ADDR=127.0.0.1:9999"));
        assert!(merged.contains("SANDBOX=true"));
        assert!(merged.contains("MODEL_NAME=claude-sonnet-4-6"));
        assert!(merged.contains("MODEL_API_BASE=https://example.com/v1"));
        assert!(merged.contains("HTTPS_PROXY=http://127.0.0.1:7890"));
        assert!(!merged.contains("MODEL_NAME=old"));
        assert!(!merged.contains("HTTPS_PROXY=http://old"));
    }

    #[test]
    fn merge_env_file_removes_optional_values_when_cleared() {
        let args = DaemonArgs {
            addr: DEFAULT_GATEWAY_ADDR.to_string(),
            sandbox: false,
            model_family: String::new(),
            model_name: String::new(),
            model_api_key: String::new(),
            model_api_base: String::new(),
            https_proxy: None,
        };
        let existing = "HTTPS_PROXY=http://127.0.0.1:7890\nMODEL_API_KEY=abc\n";

        let merged = merge_env_file(existing, &args.env_updates());

        assert!(!merged.contains("HTTPS_PROXY="));
        assert!(merged.contains("MODEL_API_KEY=''"));
        assert!(merged.contains("MODEL_FAMILY=''"));
    }
}
