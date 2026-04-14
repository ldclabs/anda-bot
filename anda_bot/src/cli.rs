use anda_core::BoxError;
use anda_engine::model::reqwest;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::{Instant, sleep},
};

use crate::{daemon, util::http_client::build_http_client};

pub struct Cli {
    http: reqwest::Client,
    // Base URL of the Hippocampus space, e.g., "http://localhost:8042"
    base_url: String,
    daemon: daemon::Daemon,
}

impl Cli {
    pub fn new(daemon: daemon::Daemon) -> Self {
        Self {
            http: build_http_client(None, |client| client.no_proxy())
                .expect("failed to build HTTP client for CLI"),
            base_url: daemon.base_url(),
            daemon,
        }
    }

    pub async fn run(self) -> Result<(), BoxError> {
        match self.ensure_daemon_running().await? {
            LaunchState::AlreadyRunning => {
                println!("Connected to anda daemon at {}.", self.base_url);
            }
            LaunchState::Started { pid, log_path } => {
                println!(
                    "Started anda daemon in the background (pid {}). Logs: {}",
                    pid,
                    log_path.display()
                );
            }
        }

        println!("Interactive CLI ready. Type 'help' to list commands.");
        self.run_repl().await
    }

    async fn run_repl(self) -> Result<(), BoxError> {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        loop {
            print!("anda> ");
            io::stdout().flush()?;

            let Some(line) = lines.next_line().await? else {
                println!();
                break;
            };
            let input = line.trim();
            if input.is_empty() {
                continue;
            }

            let command = input
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            match command.as_str() {
                "help" => print_help(),
                "exit" | "quit" => break,
                _ => {
                    println!(
                        "Command '{}' is not implemented yet. Type 'help' to see the available commands.",
                        input
                    );
                }
            }
        }
        Ok(())
    }

    async fn ensure_daemon_running(&self) -> Result<LaunchState, BoxError> {
        if self.probe_daemon_status().await.is_ok() {
            return Ok(LaunchState::AlreadyRunning);
        }

        let pid_path = self.daemon.pid_file_path();
        if let Some(pid) = self.daemon.read_pid_file().await? {
            if daemon::process_exists(pid) {
                self.wait_for_daemon_ready(Duration::from_secs(10)).await?;
                return Ok(LaunchState::AlreadyRunning);
            }
            let _ = tokio::fs::remove_file(&pid_path).await;
        }

        let child = spawn_background_daemon(&self.daemon)?;
        if let Err(err) = self.wait_for_daemon_ready(Duration::from_secs(20)).await {
            return Err(format!(
                "{}; inspect {} for daemon logs",
                err,
                child.log_path.display()
            )
            .into());
        }

        Ok(LaunchState::Started {
            pid: child.pid,
            log_path: child.log_path,
        })
    }

    async fn wait_for_daemon_ready(&self, timeout: Duration) -> Result<(), BoxError> {
        let deadline = Instant::now() + timeout;
        let detail = loop {
            match self.probe_daemon_status().await {
                Ok(_) => return Ok(()),
                Err(err) if Instant::now() >= deadline => break err.to_string(),
                Err(_) => {}
            }

            sleep(Duration::from_millis(250)).await;
        };

        Err(format!(
            "anda daemon did not become ready on {} within {:?}: {}",
            self.base_url, timeout, detail
        )
        .into())
    }

    async fn probe_daemon_status(&self) -> Result<(), BoxError> {
        let response = self.http.get(&self.base_url).send().await?;
        match response.status() {
            http::StatusCode::OK => Ok(()),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(format!("status probe failed, status: {status}, body: {body}").into())
            }
        }
    }
}

enum LaunchState {
    AlreadyRunning,
    Started { pid: u32, log_path: PathBuf },
}

struct BackgroundDaemon {
    pid: u32,
    log_path: PathBuf,
}

fn spawn_background_daemon(daemon: &daemon::Daemon) -> Result<BackgroundDaemon, BoxError> {
    let exe = std::env::current_exe()?;
    let logs_dir = daemon.logs_dir_path();
    std::fs::create_dir_all(&logs_dir)?;

    let log_path = daemon.log_file_path();
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let mut command = Command::new(exe);
    command
        .arg("--workspace")
        .arg(&daemon.workspace)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    configure_background_daemon_command(&mut command);

    let child = command.spawn()?;

    Ok(BackgroundDaemon {
        pid: child.id(),
        log_path,
    })
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

fn print_help() {
    println!("Available commands:");
    println!("  help   Show this help message");
    println!("  status Show local daemon status");
    println!("  exit   Exit the interactive CLI");
    println!("  quit   Exit the interactive CLI");
}
