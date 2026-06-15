use anda_core::{BoxError, Json, ToolInput};
use clap::{Args, Parser, Subcommand};
use mimalloc::MiMalloc;
use serde::Serialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

rust_i18n::i18n!("locales", fallback = "en");

mod auto_update;
mod autostart;
mod brain;
mod channel;
mod cli;
mod config;
mod cron;
mod daemon;
mod engine;
mod gateway;
mod logger;
mod transcription;
mod tts;
mod tui;
mod util;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser)]
#[command(author, version)]
#[command(
    about = "I am Anda Bot: a local AI agent with a long-term memory brain. Run `anda` to interact with me."
)]
#[command(long_about = None)]
#[command(after_help = r#"Examples:
    DEEPSEEK_API_KEY=**** anda
    anda

PowerShell:
    $env:DEEPSEEK_API_KEY="****"; anda

On first launch, Anda creates ~/.anda/config.yaml. You can leave provider api_key empty when a matching environment variable is set."#)]
struct Cli {
    /// Path to a directory for storing state (defaults to '~/.anda')
    #[arg(long)]
    home: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the anda daemon in the foreground.
    Daemon,
    /// Stop the anda daemon if it's running.
    Stop,
    /// Start the anda daemon if it's not running.
    Start,
    /// Show whether the anda daemon is running.
    Status(StatusCommand),
    /// Restart the anda daemon. If the daemon is not running, this will start it.
    Restart,
    /// Equal to running `anda restart`.
    Reload,
    /// Update the anda binary to the latest release.
    Update(cli::updater::UpdateCommand),
    /// Tool-related operations against the running daemon.
    #[command(subcommand)]
    Tool(ToolCommand),
    /// Agent-related operations against the running daemon.
    #[command(subcommand)]
    Agent(cli::agent::AgentCommand),
    /// Browser (chrome) extension helper commands.
    #[command(subcommand)]
    Browser(BrowserCommand),
    /// Model-related operations against the running daemon.
    #[command(subcommand)]
    Models(ModelsCommand),
    /// Manage login autostart for the current user.
    #[command(subcommand)]
    Autostart(autostart::AutostartCommand),
    /// Channel-related operations that run directly from this CLI.
    #[command(subcommand)]
    Channel(cli::channel::ChannelCommand),
    /// Manage trusted users and Ed25519 keys.
    User(cli::user::UserCommand),
    /// Inspect currently active agent sessions in the daemon.
    Session(cli::session::SessionCommand),
    /// Start a continuous voice conversation with the agent.
    Voice(cli::voice::VoiceCommand),
}

#[derive(Subcommand)]
pub enum ToolCommand {
    /// Invoke a tool by name with JSON arguments.
    Call {
        /// Tool name registered with the engine.
        #[arg(long)]
        name: String,
        /// Tool arguments as a JSON value (object/array/scalar). Defaults to `{}`.
        #[arg(long, default_value = "{}")]
        args: String,
        /// Optional request metadata as a JSON object.
        #[arg(long)]
        meta: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum BrowserCommand {
    /// Generate a bearer token for the Anda Chrome extension.
    Token {
        /// Number of days before the token expires.
        #[arg(long, default_value_t = 30)]
        days: u64,
    },
}

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// Reload model providers from config.yaml without restarting the daemon.
    Reload,
}

#[derive(Args)]
pub struct StatusCommand {
    /// Print daemon status as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Serialize)]
struct DaemonStatusReport {
    state: DaemonStatusState,
    summary: String,
    pid: Option<u32>,
    pid_file: Option<String>,
    gateway_url: Option<String>,
    log_file: Option<String>,
    conversations: Option<u64>,
    memory_nodes: Option<u64>,
    memory_links: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DaemonStatusState {
    Running,
    GatewayRunning,
    ProcessUnresponsive,
    NotRunning,
}

/// ```bash
/// cargo run -p anda_bot -- --help
/// ```
#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let result = run().await;
    if let Err(err) = &result {
        log::error!("{err}");
    }
    result
}

async fn run() -> Result<(), BoxError> {
    let cli = Cli::parse();
    let Cli { home, command } = cli;

    let home = if let Some(home) = home {
        PathBuf::from(home)
    } else {
        default_home()
    };

    tokio::fs::create_dir_all(&home).await?;
    let daemon = load_daemon(home).await?;

    if let Some(Commands::Update(cmd)) = command.as_ref() {
        let http_client =
            util::http_client::build_http_client(daemon.cfg.https_proxy.clone(), |client| client)?;
        cli::updater::run(&http_client, &daemon, cmd).await?;
        return Ok(());
    }

    if matches!(command, Some(Commands::Daemon)) {
        logger::init_daily_json_logger(
            &daemon.cfg.log_level,
            daemon.logs_dir_path(),
            logger::DAEMON_LOG_FILE_PREFIX,
        )?;
    } else {
        logger::init_daily_json_logger(
            &daemon.cfg.log_level,
            daemon.logs_dir_path(),
            logger::CLI_LOG_FILE_PREFIX,
        )?;
    }

    match command {
        None => {
            log::info!("Starting CLI at {}", daemon.base_url());
            let client = build_control_client(&daemon).await?;
            let cli = cli::Cli::new(client, daemon);
            cli.run().await?
        }
        Some(Commands::Daemon) => {
            log::info!("Starting daemon at {}", daemon.base_url());

            daemon.ensure_directories().await?;
            daemon.ensure_config_file_exists().await?;

            let ed25519_secret = util::key::load_or_init_ed25519_secret(
                &daemon.keys_dir_path().join("anda_bot.key"),
            )
            .await?;
            let ed25519_key = util::key::Ed25519Key::new(ed25519_secret);
            let user_secret =
                util::key::load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key"))
                    .await?;
            let user_key = util::key::Ed25519Key::new(user_secret);
            daemon.serve(ed25519_key, user_key.pubkey()).await?
        }
        Some(Commands::Stop) => {
            log::info!("Starting CLI with command 'stop' at {}", daemon.base_url());

            let client = build_control_client(&daemon).await?;
            match stop_daemon(&daemon, &client, Duration::from_secs(10)).await? {
                daemon::StopState::NotRunning => println!("anda daemon is not running"),
                daemon::StopState::Stopped(pid) => {
                    println!("Stopped anda daemon (pid {pid})")
                }
                daemon::StopState::StoppedUnknown => println!("Stopped anda daemon"),
            }
        }

        Some(Commands::Start) => {
            log::info!("Starting CLI with command 'start' at {}", daemon.base_url());

            let client = build_control_client(&daemon).await?;
            match client.ensure_daemon_running(&daemon).await? {
                daemon::LaunchState::AlreadyRunning => {
                    println!("anda daemon is already running at {}", daemon.base_url())
                }
                daemon::LaunchState::Started(child) => {
                    println!(
                        "Started anda daemon (pid {}). Logs: {}",
                        child.pid,
                        child.log_path.display()
                    );
                }
            }
        }

        Some(Commands::Status(cmd)) => {
            log::info!(
                "Starting CLI with command 'status' at {}",
                daemon.base_url()
            );

            let client = build_control_client(&daemon).await?;
            print_daemon_status(&daemon, &client, cmd.json).await?;
        }

        Some(Commands::Restart) | Some(Commands::Reload) => {
            log::info!(
                "Starting CLI with command 'restart' at {}",
                daemon.base_url()
            );

            let client = build_control_client(&daemon).await?;
            let stop_state = stop_daemon(&daemon, &client, Duration::from_secs(10)).await?;

            match (stop_state, client.ensure_daemon_running(&daemon).await?) {
                (daemon::StopState::Stopped(old_pid), daemon::LaunchState::Started(child)) => {
                    println!(
                        "Restarted anda daemon (old pid {old_pid}, new pid {}). Logs: {}",
                        child.pid,
                        child.log_path.display()
                    );
                }
                (daemon::StopState::NotRunning, daemon::LaunchState::Started(child)) => {
                    println!(
                        "Started anda daemon (pid {}). Logs: {}",
                        child.pid,
                        child.log_path.display()
                    );
                }
                (daemon::StopState::Stopped(old_pid), daemon::LaunchState::AlreadyRunning) => {
                    println!(
                        "Stopped anda daemon (pid {old_pid}) and connected to daemon at {}",
                        daemon.base_url()
                    );
                }
                (daemon::StopState::NotRunning, daemon::LaunchState::AlreadyRunning) => {
                    println!("anda daemon is already running at {}", daemon.base_url());
                }
                (daemon::StopState::StoppedUnknown, daemon::LaunchState::Started(child)) => {
                    println!(
                        "Restarted anda daemon (new pid {}). Logs: {}",
                        child.pid,
                        child.log_path.display()
                    );
                }
                (daemon::StopState::StoppedUnknown, daemon::LaunchState::AlreadyRunning) => {
                    println!("Connected to daemon at {}", daemon.base_url());
                }
            }
        }
        Some(Commands::Update(_)) => unreachable!("update command is handled before daemon setup"),
        Some(Commands::Tool(cmd)) => {
            log::info!("Starting CLI with command 'tool' at {}", daemon.base_url());

            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;

            match cmd {
                ToolCommand::Call { name, args, meta } => {
                    let args: Json = serde_json::from_str(&args)
                        .map_err(|e| format!("invalid --args JSON: {e}"))?;
                    let mut input = ToolInput::new(name, args);
                    if let Some(meta) = meta {
                        input.meta = Some(
                            serde_json::from_str(&meta)
                                .map_err(|e| format!("invalid --meta JSON: {e}"))?,
                        );
                    }
                    let output = client.tool_call::<Json, Json>(&input).await?;
                    println!("\n{}", serde_json::to_string_pretty(&output)?);
                }
            }
        }
        Some(Commands::Agent(cmd)) => {
            log::info!("Starting CLI with command 'agent' at {}", daemon.base_url());

            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;
            cli::agent::run(&client, cmd).await?;
        }
        Some(Commands::Browser(cmd)) => {
            log::info!(
                "Starting CLI with command 'browser' at {}",
                daemon.base_url()
            );
            match cmd {
                BrowserCommand::Token { days } => {
                    let token = build_browser_extension_token(&daemon, days).await?;
                    println!("Gateway URL: {}", daemon.base_url());
                    println!("Bearer token: {token}");
                    println!("Extension directory: chrome_extension");
                }
            }
        }
        Some(Commands::Models(cmd)) => {
            log::info!(
                "Starting CLI with command 'models' at {}",
                daemon.base_url()
            );

            let client = build_control_client(&daemon).await?;
            match cmd {
                ModelsCommand::Reload => {
                    let models = client.reload_models().await?;
                    println!("{}", serde_json::to_string_pretty(&models)?);
                }
            }
        }
        Some(Commands::Autostart(cmd)) => {
            log::info!("Starting CLI with command 'autostart'");
            run_autostart_command(&daemon, cmd).await?;
        }
        Some(Commands::Channel(cmd)) => {
            log::info!(
                "Starting CLI with command 'channel' at {}",
                daemon.base_url()
            );
            cli::channel::run(&daemon, cmd).await?;
        }
        Some(Commands::User(cmd)) => {
            log::info!("Starting CLI with command 'user' at {}", daemon.base_url());
            cli::user::run(&daemon, cmd).await?;
        }
        Some(Commands::Session(cmd)) => {
            log::info!(
                "Starting CLI with command 'sessions' at {}",
                daemon.base_url()
            );

            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;
            cli::session::run(&client, cmd).await?;
        }
        Some(Commands::Voice(cmd)) => {
            log::info!("Starting CLI with command 'voice' at {}", daemon.base_url());

            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;
            cli::voice::run_voice_loop(&client, &daemon.cfg, cmd).await?;
        }
    }
    Ok(())
}

async fn stop_daemon(
    daemon: &daemon::Daemon,
    client: &gateway::Client,
    timeout: Duration,
) -> Result<daemon::StopState, BoxError> {
    let pid = daemon.read_pid_file().await?;
    let gateway_running = client.status().await.is_ok();

    match pid {
        Some(pid) if daemon::process_exists(pid) => {
            if gateway_running {
                if let Err(err) = client.shutdown().await {
                    log::warn!("Failed to request graceful daemon shutdown: {err}");
                } else if let Err(err) = daemon.wait_for_background_exit(pid, timeout).await {
                    log::warn!("Graceful daemon shutdown timed out: {err}");
                } else {
                    daemon.remove_pid_file_if_exists().await?;
                    return Ok(daemon::StopState::Stopped(pid));
                }
            }

            daemon.stop_background(timeout).await
        }
        Some(_) => {
            daemon.remove_pid_file_if_exists().await?;
            if gateway_running {
                client.shutdown().await?;
                wait_for_gateway_down(client, timeout).await?;
                Ok(daemon::StopState::StoppedUnknown)
            } else {
                Ok(daemon::StopState::NotRunning)
            }
        }
        None if gateway_running => {
            client.shutdown().await?;
            wait_for_gateway_down(client, timeout).await?;
            Ok(daemon::StopState::StoppedUnknown)
        }
        None => Ok(daemon::StopState::NotRunning),
    }
}

async fn wait_for_gateway_down(
    client: &gateway::Client,
    timeout: Duration,
) -> Result<(), BoxError> {
    let deadline = Instant::now() + timeout;
    loop {
        if client.status().await.is_err() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for anda daemon gateway to stop after {timeout:?}"
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn print_daemon_status(
    daemon: &daemon::Daemon,
    client: &gateway::Client,
    json: bool,
) -> Result<(), BoxError> {
    let report = daemon_status_report(daemon, client).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_daemon_status_text(&report);
    }

    Ok(())
}

async fn daemon_status_report(
    daemon: &daemon::Daemon,
    client: &gateway::Client,
) -> Result<DaemonStatusReport, BoxError> {
    let pid = daemon.read_pid_file().await?;
    let status = client.status().await.ok();
    let alive_pid = pid.filter(|pid| daemon::process_exists(*pid));

    if pid.is_some() && alive_pid.is_none() {
        daemon.remove_pid_file_if_exists().await?;
    }

    Ok(match (status, alive_pid) {
        (Some(status), Some(pid)) => DaemonStatusReport {
            state: DaemonStatusState::Running,
            summary: format!("anda daemon is running (pid {pid})"),
            pid: Some(pid),
            pid_file: None,
            gateway_url: Some(daemon.base_url().to_string()),
            log_file: Some(daemon.log_file_path().display().to_string()),
            conversations: Some(status.conversations),
            memory_nodes: Some(status.memory_nodes),
            memory_links: Some(status.memory_links),
        },
        (Some(status), None) => DaemonStatusReport {
            state: DaemonStatusState::GatewayRunning,
            summary: "anda daemon gateway is running".to_string(),
            pid: None,
            pid_file: Some("missing".to_string()),
            gateway_url: Some(daemon.base_url().to_string()),
            log_file: None,
            conversations: Some(status.conversations),
            memory_nodes: Some(status.memory_nodes),
            memory_links: Some(status.memory_links),
        },
        (None, Some(pid)) => DaemonStatusReport {
            state: DaemonStatusState::ProcessUnresponsive,
            summary: format!(
                "anda daemon process exists but gateway is not responding (pid {pid})"
            ),
            pid: Some(pid),
            pid_file: None,
            gateway_url: None,
            log_file: Some(daemon.log_file_path().display().to_string()),
            conversations: None,
            memory_nodes: None,
            memory_links: None,
        },
        (None, None) => DaemonStatusReport {
            state: DaemonStatusState::NotRunning,
            summary: "anda daemon is not running".to_string(),
            pid: None,
            pid_file: None,
            gateway_url: None,
            log_file: None,
            conversations: None,
            memory_nodes: None,
            memory_links: None,
        },
    })
}

fn print_daemon_status_text(report: &DaemonStatusReport) {
    println!("{}", report.summary);
    if let Some(url) = report.gateway_url.as_deref() {
        println!("Gateway URL: {url}");
    }
    if let Some(log_file) = report.log_file.as_deref() {
        println!("Logs: {log_file}");
    }
    if let Some(pid_file) = report.pid_file.as_deref() {
        println!("PID file: {pid_file}");
    }
    if let Some(conversations) = report.conversations {
        println!("Conversations: {conversations}");
    }
    if let Some(memory_nodes) = report.memory_nodes {
        println!("Memory nodes: {memory_nodes}");
    }
    if let Some(memory_links) = report.memory_links {
        println!("Memory links: {memory_links}");
    }
}

async fn run_autostart_command(
    daemon: &daemon::Daemon,
    cmd: autostart::AutostartCommand,
) -> Result<(), BoxError> {
    match cmd {
        autostart::AutostartCommand::Install => {
            daemon.ensure_directories().await?;
            daemon.ensure_config_file_exists().await?;
            autostart::install(&daemon.home)?;
            println!("Registered Anda to start when the current user logs in.");
        }
        autostart::AutostartCommand::Uninstall => match autostart::uninstall()? {
            autostart::AutostartStatus::NotInstalled => {
                println!("Anda autostart is not registered.")
            }
            autostart::AutostartStatus::Installed => unreachable!(),
            autostart::AutostartStatus::Unsupported => {
                println!("anda autostart is not supported on this platform.")
            }
        },
        autostart::AutostartCommand::Status => match autostart::status()? {
            autostart::AutostartStatus::Installed => {
                println!("Anda autostart is registered.")
            }
            autostart::AutostartStatus::NotInstalled => {
                println!("Anda autostart is not registered.")
            }
            autostart::AutostartStatus::Unsupported => {
                println!("anda autostart is not supported on this platform.")
            }
        },
    }
    Ok(())
}

fn default_home() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".anda")
}

async fn load_daemon(home: PathBuf) -> Result<daemon::Daemon, BoxError> {
    config::Config::ensure_file_exists(&home).await?;
    let config_path = config::Config::file_path(&home);
    let config = config::Config::from_file(&config_path).await?;
    Ok(daemon::Daemon::new(home, config))
}

async fn build_control_client(daemon: &daemon::Daemon) -> Result<gateway::Client, BoxError> {
    daemon.ensure_directories().await?;

    let user_secret =
        util::key::load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
    let user_key = util::key::Ed25519Key::new(user_secret);
    let mut claims = util::key::Claims::default();
    claims.extra.insert(util::key::iana::CWTClaimScope, "*");
    let gateway_token = user_key.sign_cwt(claims)?;
    let http_client = util::http_client::build_http_client(None, |client| client.no_proxy())?;

    Ok(gateway::Client::new(daemon.base_url(), gateway_token).with_http_client(http_client))
}

async fn build_browser_extension_token(
    daemon: &daemon::Daemon,
    days: u64,
) -> Result<String, BoxError> {
    daemon.ensure_directories().await?;

    let user_secret =
        util::key::load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
    let user_key = util::key::Ed25519Key::new(user_secret);
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let days = days.clamp(1, 3650);
    let expires_secs = now_secs.saturating_add(days * 24 * 60 * 60);

    let mut claims = util::key::Claims {
        issued_at: Some(now_secs),
        expiration: Some(expires_secs),
        ..Default::default()
    };
    claims.extra.insert(util::key::iana::CWTClaimScope, "*");
    claims.extra.insert("client", "chrome_extension");
    user_key.sign_cwt(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_command_accepts_json_flag() {
        let cli = Cli::try_parse_from(["anda", "status", "--json"]).unwrap();
        let Some(Commands::Status(cmd)) = cli.command else {
            panic!("expected status command");
        };

        assert!(cmd.json);
    }

    #[test]
    fn status_command_defaults_to_text_output() {
        let cli = Cli::try_parse_from(["anda", "status"]).unwrap();
        let Some(Commands::Status(cmd)) = cli.command else {
            panic!("expected status command");
        };

        assert!(!cmd.json);
    }

    #[test]
    fn models_reload_command_parses() {
        let cli = Cli::try_parse_from(["anda", "models", "reload"]).unwrap();
        let Some(Commands::Models(ModelsCommand::Reload)) = cli.command else {
            panic!("expected models reload command");
        };
    }

    #[test]
    fn user_create_command_parses() {
        let cli = Cli::try_parse_from(["anda", "user", "create", "alice"]).unwrap();
        let Some(Commands::User(_cmd)) = cli.command else {
            panic!("expected user command");
        };
    }

    #[test]
    fn daemon_status_report_serializes_stable_json() {
        let report = DaemonStatusReport {
            state: DaemonStatusState::Running,
            summary: "anda daemon is running (pid 12345)".to_string(),
            pid: Some(12345),
            pid_file: None,
            gateway_url: Some("http://127.0.0.1:8042".to_string()),
            log_file: Some("/tmp/anda.log".to_string()),
            conversations: Some(7),
            memory_nodes: Some(11),
            memory_links: Some(13),
        };

        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({
                "state": "running",
                "summary": "anda daemon is running (pid 12345)",
                "pid": 12345,
                "pid_file": null,
                "gateway_url": "http://127.0.0.1:8042",
                "log_file": "/tmp/anda.log",
                "conversations": 7,
                "memory_nodes": 11,
                "memory_links": 13
            })
        );
    }
}
