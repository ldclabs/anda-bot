use anda_core::{AgentInput, BoxError, Json, RequestMeta, ToolInput};
use clap::{Parser, Subcommand};
use mimalloc::MiMalloc;
use std::{path::PathBuf, time::Duration};

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
    Agent(AgentCommand),
    /// Channel-related operations that run directly from this CLI.
    #[command(subcommand)]
    Channel(cli::channel::ChannelCommand),
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
pub enum AgentCommand {
    /// Run an agent once with a text prompt.
    Run {
        /// Agent name. Empty value uses the default agent.
        #[arg(long, default_value = "")]
        name: String,
        /// User prompt sent to the agent.
        #[arg(long)]
        prompt: String,
        /// Optional request metadata as a JSON object.
        #[arg(long)]
        meta: Option<String>,
    },
}

/// ```bash
/// cargo run -p anda_bot -- --help
/// ```
#[tokio::main]
async fn main() -> Result<(), BoxError> {
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
        cli::updater::run(&http_client, cmd).await?;
        return Ok(());
    }

    if matches!(command, Some(Commands::Daemon)) {
        logger::init_daily_json_logger(daemon.logs_dir_path(), logger::DAEMON_LOG_FILE_PREFIX)?;
    } else {
        logger::init_daily_json_logger(daemon.logs_dir_path(), logger::CLI_LOG_FILE_PREFIX)?;
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

            let ed25519_secret =
                load_or_init_ed25519_secret(&daemon.keys_dir_path().join("anda_bot.key")).await?;
            let ed25519_key = util::key::Ed25519Key::new(ed25519_secret);
            let user_secret =
                load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
            let user_key = util::key::Ed25519Key::new(user_secret);
            daemon.serve(ed25519_key, user_key.pubkey()).await?
        }
        Some(Commands::Stop) => {
            log::info!("Starting CLI with command 'stop' at {}", daemon.base_url());

            match daemon.stop_background(Duration::from_secs(10)).await? {
                daemon::StopState::NotRunning => println!("anda daemon is not running"),
                daemon::StopState::Stopped(pid) => {
                    println!("Stopped anda daemon (pid {pid})")
                }
            }
        }
        Some(Commands::Restart) | Some(Commands::Reload) => {
            log::info!(
                "Starting CLI with command 'restart' at {}",
                daemon.base_url()
            );

            let stop_state = daemon.stop_background(Duration::from_secs(10)).await?;
            let client = build_control_client(&daemon).await?;

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

            match cmd {
                AgentCommand::Run { name, prompt, meta } => {
                    let mut input = AgentInput::new(name, prompt);
                    let request_meta = parse_request_meta(meta)?.unwrap_or_default();
                    input.meta = Some(request_meta);

                    let output = client.agent_run(&input).await?;
                    println!("\n{}", serde_json::to_string_pretty(&output)?);
                }
            }
        }
        Some(Commands::Channel(cmd)) => {
            log::info!(
                "Starting CLI with command 'channel' at {}",
                daemon.base_url()
            );
            cli::channel::run(&daemon, cmd).await?;
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

fn parse_request_meta(meta: Option<String>) -> Result<Option<RequestMeta>, BoxError> {
    match meta {
        Some(meta) => Ok(Some(
            serde_json::from_str(&meta).map_err(|e| format!("invalid --meta JSON: {e}"))?,
        )),
        None => Ok(None),
    }
}

fn default_home() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".anda")
}

async fn load_daemon(home: PathBuf) -> Result<daemon::Daemon, BoxError> {
    let config_path = config::Config::file_path(&home);
    let config = config::Config::from_file(&config_path).await?;
    Ok(daemon::Daemon::new(home, config))
}

async fn build_control_client(daemon: &daemon::Daemon) -> Result<gateway::Client, BoxError> {
    daemon.ensure_directories().await?;

    let user_secret = load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
    let user_key = util::key::Ed25519Key::new(user_secret);
    let gateway_token = user_key.sign_cwt(
        util::key::ClaimsSetBuilder::new()
            .claim(util::key::iana::CwtClaimName::Scope, "*".into())
            .build(),
    )?;
    let http_client = util::http_client::build_http_client(None, |client| client.no_proxy())?;

    Ok(gateway::Client::new(daemon.base_url(), gateway_token).with_http_client(http_client))
}

async fn load_or_init_ed25519_secret(key_path: &PathBuf) -> Result<[u8; 32], BoxError> {
    match tokio::fs::read_to_string(key_path).await {
        Ok(content) => {
            let secret = util::key::parse_ed25519_privkey(content.trim())?;
            Ok(secret)
        }
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(err.into());
            }
            log::warn!(
                name = "daemon";
                "ED25519 private key not found at {:?}, generating a new one",
                key_path
            );
            let secret = util::key::random_ed25519_privkey();
            let encoded = util::key::encode_ed25519_privkey(&secret)?;
            tokio::fs::write(key_path, encoded).await?;
            Ok(secret)
        }
    }
}
