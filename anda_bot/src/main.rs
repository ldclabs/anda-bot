use anda_core::{BoxError, Json, ToolInput};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use mimalloc::MiMalloc;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

rust_i18n::i18n!("locales", fallback = "en");

mod auto_update;
mod autostart;
mod brain;
mod channel;
mod chatgpt;
mod cli;
mod config;
mod cron;
mod daemon;
mod daemon_protocol;
mod engine;
mod gateway;
mod identity;
mod logger;
#[cfg(feature = "mib")]
mod mib;
mod provider_env;
mod runtime_admission;
#[cfg(test)]
mod test_support;
mod transcription;
mod tts;
mod tui;
#[cfg(any(windows, test))]
mod update_protocol;
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

    /// Read daemon startup identity secrets from stdin.
    #[arg(long, hide = true)]
    identity_secrets_stdin: bool,

    /// Start the interactive CLI in full-access mode: shell commands and MCP
    /// server connections run without approval prompts. Not valid with a
    /// subcommand.
    #[arg(long)]
    full_access: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Validate config YAML from stdin without creating a home or starting services.
    ValidateConfig,
    /// Connect and manage a ChatGPT plan without an API key.
    Auth(cli::auth::AuthCommand),
    /// Serve the isolated MIB evaluation adapter (never starts the daemon).
    #[cfg(feature = "mib")]
    Mib(mib::MibCommand),
    /// Run the anda daemon in the foreground.
    Daemon,
    /// Stop the anda daemon if it's running.
    Stop,
    /// Start the anda daemon if it's not running.
    Start,
    /// Show whether the anda daemon is running.
    Status(StatusCommand),
    /// Get started with long-term memory and inspect the running service.
    Memory(cli::memory::MemoryCommand),
    /// Restart the anda daemon. If the daemon is not running, this will start it.
    Restart,
    /// Equal to running `anda restart`.
    Reload,
    /// Update the anda binary to the latest release.
    Update(cli::updater::UpdateCommand),
    /// Install this anda binary at the CLI location shared with the install
    /// scripts, Homebrew and Anda Desktop (never downgrades).
    Install(cli::installer::InstallCommand),
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
        /// Print the token report as JSON.
        #[arg(long)]
        json: bool,
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

use daemon_protocol::{DaemonStatusReport, DaemonStatusState};

const CHROME_EXTENSION_DIR: &str = "chrome-extension";
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const NO_OWNER_IDENTITY: &str = "No existing owner identity is available. Run `anda start` to initialize Anda. / 无法读取已有身份，请先运行 anda start。";

/// ```bash
/// cargo run -p anda_bot -- --help
/// ```
#[tokio::main]
async fn main() -> Result<(), BoxError> {
    util::http_client::install_default_crypto_provider();
    // `run` has many async command branches. Keep its future off the main
    // thread's small Windows stack (which RUST_MIN_STACK does not enlarge).
    let result = Box::pin(run()).await;
    if let Err(err) = &result {
        log::error!("{err}");
    }
    result
}

async fn run() -> Result<(), BoxError> {
    let matches = Cli::command().get_matches();
    let command_name = matches.subcommand_name();
    let Cli {
        home,
        identity_secrets_stdin,
        full_access,
        command,
    } = Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());

    if identity_secrets_stdin && !matches!(command, Some(Commands::Daemon)) {
        return Err("--identity-secrets-stdin can only be used with `anda daemon`".into());
    }

    // Only the interactive CLI puts the approval mode on its requests. Failing
    // loudly beats dropping it: a subcommand cannot answer an approval card, so
    // the request would just block until the card expires.
    if full_access && command.is_some() {
        return Err("--full-access can only be used with the interactive `anda` CLI".into());
    }

    if matches!(command, Some(Commands::ValidateConfig)) {
        use tokio::io::AsyncReadExt;
        let mut contents = Vec::new();
        tokio::io::stdin()
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut contents)
            .await?;
        if contents.len() > 2 * 1024 * 1024 {
            return Err("configuration exceeds 2 MiB".into());
        }
        config::Config::from_contents(std::str::from_utf8(&contents)?)?;
        println!("{{\"valid\":true}}");
        return Ok(());
    }

    let custom_home = home.is_some();
    let home = home.map(PathBuf::from).unwrap_or_else(default_home);

    // Memory guide/status are read-only and must never initialize a home,
    // credentials, logging or a daemon as a side effect of inspection.
    if let Some(Commands::Memory(cmd)) = command.as_ref() {
        cmd.validate()?;
        if let Some(evaluation) = cmd.evaluation() {
            if custom_home {
                return Err("Memory evaluations are isolated; --home is not supported".into());
            }
            return cli::memory_eval::run(evaluation).await;
        }
        if cmd.is_guide() {
            cmd.print_guide();
            return Ok(());
        }
        let cfg = config::Config::from_file(&config::Config::file_path(&home)).await?;
        let owner = identity::load_identity_secret_with_location_with_store(
            &identity::IdentityKeyRef::owner(&home),
            identity::os_identity_key_store(),
        )
        .await
        .map_err(|_| NO_OWNER_IDENTITY)?;
        let daemon = daemon::Daemon::new(home, cfg);
        let client = build_control_client_from_owner_secret(&daemon, owner.secret)?;
        return cli::memory::run(&client, cmd).await;
    }

    #[cfg(feature = "mib")]
    if let Some(Commands::Mib(cmd)) = command.as_ref() {
        if custom_home {
            return Err("MIB runs use private in-memory state; --home is not supported".into());
        }
        return mib::serve(cmd).await;
    }

    tokio::fs::create_dir_all(&home).await?;
    // Installing only copies files; it must not create a config or identity.
    if let Some(Commands::Install(cmd)) = command.as_ref() {
        return cli::installer::run(&home, cmd).await;
    }
    let daemon = load_daemon(home).await?;

    if let Some(Commands::Update(cmd)) = command.as_ref() {
        let http_client =
            util::http_client::build_http_client(daemon.cfg.https_proxy.clone(), |client| client)?;
        cli::updater::run(&http_client, &daemon.home, cmd).await?;
        return Ok(());
    }

    let log_file_prefix = if matches!(command, Some(Commands::Daemon)) {
        logger::DAEMON_LOG_FILE_PREFIX
    } else {
        logger::CLI_LOG_FILE_PREFIX
    };
    logger::init_daily_json_logger(
        &daemon.cfg.log_level,
        daemon.logs_dir_path(),
        log_file_prefix,
    )?;
    log::info!(
        "Starting anda{} at {}",
        command_name
            .map(|name| format!(" {name}"))
            .unwrap_or_default(),
        daemon.base_url()
    );

    match command {
        Some(Commands::ValidateConfig) => {
            unreachable!("validation dispatches before initialization")
        }
        Some(Commands::Memory(_)) => unreachable!("memory dispatches before daemon initialization"),
        #[cfg(feature = "mib")]
        Some(Commands::Mib(_)) => unreachable!("MIB dispatches before daemon initialization"),
        Some(Commands::Update(_) | Commands::Install(_)) => {
            unreachable!("update and install are handled before daemon setup")
        }
        None => {
            let client = build_control_client(&daemon).await?;
            tui::run(daemon, client, full_access).await?
        }
        Some(Commands::Daemon) => {
            daemon.ensure_directories().await?;

            let local_identity = if identity_secrets_stdin {
                identity::read_local_identity_secrets_from_stdin().await?
            } else {
                identity::load_or_init_local_identity_secrets_with_store(
                    &daemon.home,
                    identity::os_identity_key_store(),
                )
                .await?
            };
            let ed25519_key = identity::Ed25519Key::new(*local_identity.daemon);
            let user_key = identity::Ed25519Key::new(*local_identity.owner);
            daemon.serve(ed25519_key, user_key.pubkey()).await?
        }
        Some(Commands::Stop) => {
            // The owner identity is only needed to ask a live gateway.
            let gateway = if build_status_client(&daemon).status().await.is_ok() {
                Some(build_control_client(&daemon).await?)
            } else {
                None
            };
            print_stop_state(stop_daemon(&daemon, gateway.as_ref(), STOP_TIMEOUT).await?);
        }
        Some(Commands::Start) => {
            let client = build_status_client(&daemon);
            let launch_state = if client.status().await.is_ok() {
                daemon::LaunchState::AlreadyRunning
            } else {
                let local_identity = identity::load_or_init_local_identity_secrets_with_store(
                    &daemon.home,
                    identity::os_identity_key_store(),
                )
                .await?;
                client
                    .ensure_daemon_running_with_identity_secrets(&daemon, Some(&local_identity))
                    .await?
            };
            print_launch_state(&daemon, &launch_state);
        }
        Some(Commands::Status(cmd)) => {
            let client = build_status_client(&daemon);
            print_daemon_status(&daemon, &client, cmd.json).await?;
        }
        Some(Commands::Restart | Commands::Reload) => {
            let local_identity = identity::load_or_init_local_identity_secrets_with_store(
                &daemon.home,
                identity::os_identity_key_store(),
            )
            .await?;
            let client = build_control_client_from_owner_secret(&daemon, *local_identity.owner)?;
            // `/daemon/status` takes no credential, so the control client asks.
            let gateway = client.status().await.is_ok().then_some(&client);
            let stop_state = stop_daemon(&daemon, gateway, STOP_TIMEOUT).await?;
            if stop_state != daemon::StopState::NotRunning {
                print_stop_state(stop_state);
            }
            let launch_state = client
                .ensure_daemon_running_with_identity_secrets(&daemon, Some(&local_identity))
                .await?;
            print_launch_state(&daemon, &launch_state);
        }
        Some(Commands::Tool(cmd)) => {
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
            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;
            cli::agent::run(&client, cmd).await?;
        }
        Some(Commands::Browser(cmd)) => match cmd {
            BrowserCommand::Token { days, json } => {
                let token = build_browser_extension_token(&daemon, days).await?;
                if json {
                    let report = daemon_protocol::BrowserTokenReport {
                        gateway_url: daemon.base_url(),
                        token,
                        extension_dir: CHROME_EXTENSION_DIR.to_string(),
                    };
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!("Gateway URL: {}", daemon.base_url());
                    println!("Bearer token: {token}");
                    println!("Extension directory: {CHROME_EXTENSION_DIR}");
                }
            }
        },
        Some(Commands::Auth(cmd)) => {
            let client = build_control_client(&daemon).await?;
            cli::auth::run(&daemon, &client, cmd).await?;
        }
        Some(Commands::Models(cmd)) => {
            let client = build_control_client(&daemon).await?;
            match cmd {
                ModelsCommand::Reload => {
                    let models = client.reload_models().await?;
                    println!("{}", serde_json::to_string_pretty(&models)?);
                }
            }
        }
        Some(Commands::Autostart(cmd)) => run_autostart_command(&daemon, cmd).await?,
        Some(Commands::Channel(cmd)) => cli::channel::run(&daemon, cmd).await?,
        Some(Commands::User(cmd)) => cli::user::run(&daemon, cmd).await?,
        Some(Commands::Session(cmd)) => {
            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;
            cli::session::run(&client, cmd).await?;
        }
        Some(Commands::Voice(cmd)) => {
            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;
            client
                .register_cli_workspace(&std::env::current_dir()?)
                .await?;
            cli::voice::run_voice_loop(&client, &daemon.cfg, cmd).await?;
        }
    }
    Ok(())
}

/// Stops the daemon serving this home. `gateway` is an authenticated client
/// when the gateway answered; shutdown goes through it before any signal.
async fn stop_daemon(
    daemon: &daemon::Daemon,
    gateway: Option<&gateway::Client>,
    timeout: Duration,
) -> Result<daemon::StopState, BoxError> {
    match (daemon.running_pid().await?, gateway) {
        (Some(pid), gateway) => {
            if let Some(client) = gateway {
                if let Err(err) = client.shutdown().await {
                    log::warn!("Failed to request graceful daemon shutdown: {err}");
                } else if let Err(err) = daemon.wait_for_background_exit(pid, timeout).await {
                    log::warn!("Graceful daemon shutdown timed out: {err}");
                } else {
                    return Ok(daemon::StopState::Stopped(pid));
                }
            }
            daemon.terminate(pid, timeout).await?;
            Ok(daemon::StopState::Stopped(pid))
        }
        // A gateway answers but no daemon of this home holds the lock.
        (None, Some(client)) => {
            client.shutdown().await?;
            wait_for_gateway_down(client, timeout).await?;
            Ok(daemon::StopState::StoppedUnknown)
        }
        (None, None) => Ok(daemon::StopState::NotRunning),
    }
}

fn print_stop_state(state: daemon::StopState) {
    match state {
        daemon::StopState::NotRunning => println!("anda daemon is not running"),
        daemon::StopState::Stopped(pid) => println!("Stopped anda daemon (pid {pid})"),
        daemon::StopState::StoppedUnknown => println!("Stopped anda daemon"),
    }
}

fn print_launch_state(daemon: &daemon::Daemon, state: &daemon::LaunchState) {
    match state {
        daemon::LaunchState::AlreadyRunning => {
            println!("anda daemon is already running at {}", daemon.base_url())
        }
        daemon::LaunchState::Started(child) => println!(
            "Started anda daemon (pid {}). Logs: {}",
            child.pid,
            child.log_path.display()
        ),
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
    let pid = daemon.running_pid().await?;
    let status = client.status().await.ok();

    Ok(match (status, pid) {
        (Some(status), Some(pid)) => DaemonStatusReport {
            state: DaemonStatusState::Running,
            summary: format!("anda daemon is running (pid {pid})"),
            pid: Some(pid),
            pid_file: None,
            gateway_url: Some(daemon.base_url()),
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
            gateway_url: Some(daemon.base_url()),
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
            autostart::install(&std::env::current_exe()?, &daemon.home)?;
            println!("Registered Anda to start when the current user logs in.");
        }
        autostart::AutostartCommand::Uninstall => {
            autostart::uninstall()?;
            println!("Anda autostart is not registered.");
        }
        autostart::AutostartCommand::Status => {
            if autostart::status()? {
                println!("Anda autostart is registered.");
            } else {
                println!("Anda autostart is not registered.");
            }
        }
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
    build_control_client_with_store(daemon, identity::os_identity_key_store()).await
}

async fn build_control_client_with_store(
    daemon: &daemon::Daemon,
    identity_store: std::sync::Arc<dyn identity::IdentityKeyStore>,
) -> Result<gateway::Client, BoxError> {
    let secrets =
        identity::load_or_init_local_identity_secrets_with_store(&daemon.home, identity_store)
            .await?;
    build_control_client_from_owner_secret(daemon, *secrets.owner)
}

fn build_control_client_from_owner_secret(
    daemon: &daemon::Daemon,
    owner_secret: [u8; 32],
) -> Result<gateway::Client, BoxError> {
    // The token is minted once per CLI process and `gateway::Client` has no
    // refresh path, so the lifetime must cover the longest-lived command: the
    // interactive TUI (`anda` with no subcommand) and `anda voice` both hold
    // one client for the whole session. A few minutes would make every request
    // 401 mid-session; one day keeps the credential bounded without that.
    let token = owner_token(owner_secret, Duration::from_secs(24 * 60 * 60), None)?;
    Ok(gateway::Client::new(daemon.base_url(), token))
}

fn build_status_client(daemon: &daemon::Daemon) -> gateway::Client {
    gateway::Client::new(daemon.base_url(), String::new())
}

async fn build_browser_extension_token(
    daemon: &daemon::Daemon,
    days: u64,
) -> Result<String, BoxError> {
    build_browser_extension_token_with_store(daemon, days, identity::os_identity_key_store()).await
}

async fn build_browser_extension_token_with_store(
    daemon: &daemon::Daemon,
    days: u64,
    identity_store: std::sync::Arc<dyn identity::IdentityKeyStore>,
) -> Result<String, BoxError> {
    let secrets =
        identity::load_or_init_local_identity_secrets_with_store(&daemon.home, identity_store)
            .await?;
    let lifetime = Duration::from_secs(days.clamp(1, 3650) * 24 * 60 * 60);
    owner_token(*secrets.owner, lifetime, Some("chrome_extension"))
}

/// An owner bearer for the Anda Bot space; `client` labels who holds it.
fn owner_token(
    owner_secret: [u8; 32],
    lifetime: Duration,
    client: Option<&str>,
) -> Result<String, BoxError> {
    let mut claims = identity::expiring_claims(lifetime)?;
    claims.audience = Some(config::ANDA_BOT_SPACE_ID.into());
    claims.extra.insert(identity::iana::CWTClaimScope, "*");
    if let Some(client) = client {
        claims.extra.insert("client", client);
    }
    identity::Ed25519Key::new(owner_secret).sign_cwt(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_constraints_accept_supported_combinations() {
        Cli::command().debug_assert();
        for args in [
            vec!["anda", "agent", "run", "--prompt", "hello"],
            vec!["anda", "agent", "run", "--prompt-file", "prompt.txt"],
            vec!["anda", "voice", "--record-secs", "1"],
            vec!["anda", "channel", "init", "--all"],
            vec!["anda", "channel", "init", "wechat"],
            vec!["anda", "update", "--check", "--json"],
            vec!["anda", "update", "--check-if-due", "--json"],
            vec!["anda", "update", "--force", "--skills"],
            vec!["anda", "memory", "inbox", "--cursor", "page-two", "--json"],
            vec!["anda", "memory", "inbox", "setup", "--json"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
    }

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
    fn browser_token_output_points_to_extension_directory() {
        assert_eq!(CHROME_EXTENSION_DIR, "chrome-extension");
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

    use axum::{Router, routing};

    async fn spawn_status_mock() -> String {
        let app = Router::new().route(
            "/daemon/status",
            routing::get(|| async {
                axum::Json(serde_json::json!({
                    "conversations": 1u64,
                    "memory_nodes": 2u64,
                    "memory_links": 3u64
                }))
            }),
        );
        crate::test_support::spawn_http_mock(app).await
    }

    fn temp_daemon() -> (tempfile::TempDir, daemon::Daemon) {
        let dir = tempfile::tempdir().unwrap();
        let daemon = daemon::Daemon::new(dir.path().to_path_buf(), config::Config::default());
        (dir, daemon)
    }

    #[test]
    fn default_home_appends_anda_dir() {
        assert!(default_home().ends_with(".anda"));
    }

    #[test]
    fn print_daemon_status_text_handles_all_optional_fields() {
        // Full report exercises every optional branch.
        print_daemon_status_text(&DaemonStatusReport {
            state: DaemonStatusState::Running,
            summary: "running".to_string(),
            pid: Some(1),
            pid_file: Some("missing".to_string()),
            gateway_url: Some("http://x".to_string()),
            log_file: Some("/tmp/x.log".to_string()),
            conversations: Some(1),
            memory_nodes: Some(2),
            memory_links: Some(3),
        });
        // Empty report exercises the None branches.
        print_daemon_status_text(&DaemonStatusReport {
            state: DaemonStatusState::NotRunning,
            summary: "not running".to_string(),
            pid: None,
            pid_file: None,
            gateway_url: None,
            log_file: None,
            conversations: None,
            memory_nodes: None,
            memory_links: None,
        });
    }

    #[tokio::test]
    async fn daemon_status_report_reflects_gateway_and_pid_state() {
        // Gateway responds, no pid file -> GatewayRunning.
        let base = spawn_status_mock().await;
        let (_dir, daemon) = temp_daemon();
        let client = gateway::Client::new(base, "t".to_string());
        let report = daemon_status_report(&daemon, &client).await.unwrap();
        assert!(matches!(report.state, DaemonStatusState::GatewayRunning));

        // No gateway, no pid -> NotRunning.
        let (_dir2, daemon2) = temp_daemon();
        let dead = gateway::Client::new("http://127.0.0.1:1".to_string(), "t".to_string());
        let report = daemon_status_report(&daemon2, &dead).await.unwrap();
        assert!(matches!(report.state, DaemonStatusState::NotRunning));

        // No gateway, a daemon holding the lock -> ProcessUnresponsive.
        let (_dir3, daemon3) = temp_daemon();
        let guard = daemon3.acquire_pid_file().await.unwrap();
        let report = daemon_status_report(&daemon3, &dead).await.unwrap();
        assert!(matches!(
            report.state,
            DaemonStatusState::ProcessUnresponsive
        ));
        drop(guard);

        // A live pid left without the lock was reused -> NotRunning.
        tokio::fs::write(daemon3.pid_file_path(), std::process::id().to_string())
            .await
            .unwrap();
        let report = daemon_status_report(&daemon3, &dead).await.unwrap();
        assert!(matches!(report.state, DaemonStatusState::NotRunning));
        assert!(!daemon3.pid_file_path().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stop_daemon_never_signals_a_pid_no_daemon_holds() {
        let (_dir, daemon) = temp_daemon();
        let pid = crate::test_support::spawn_orphan_sleeper();
        tokio::fs::write(daemon.pid_file_path(), pid.to_string())
            .await
            .unwrap();

        let state = stop_daemon(&daemon, None, Duration::from_secs(1))
            .await
            .unwrap();

        assert_eq!(state, daemon::StopState::NotRunning);
        assert!(!daemon.pid_file_path().exists());
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        assert!(alive, "stop_daemon signalled an unrelated process");
    }

    #[tokio::test]
    async fn wait_for_gateway_down_returns_when_unreachable() {
        let dead = gateway::Client::new("http://127.0.0.1:1".to_string(), "t".to_string());
        wait_for_gateway_down(&dead, Duration::from_millis(500))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn load_daemon_and_clients_build_from_home() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = load_daemon(dir.path().to_path_buf()).await.unwrap();
        assert_eq!(daemon.home, dir.path());

        // Building the control client provisions the user key and succeeds.
        let identity_store = std::sync::Arc::new(identity::MemoryIdentityKeyStore::default());
        let client = build_control_client_with_store(&daemon, identity_store.clone())
            .await
            .unwrap();

        let token = build_browser_extension_token_with_store(&daemon, 9999, identity_store.clone())
            .await
            .unwrap();
        assert!(!token.is_empty());
        let secrets =
            identity::load_or_init_local_identity_secrets_with_store(&daemon.home, identity_store)
                .await
                .unwrap();
        let owner = identity::Ed25519Key::new(*secrets.owner);
        let brain = brain::Brain::new(
            std::sync::Arc::new(object_store::memory::InMemory::new()),
            brain::BrainConfig {
                managers: vec![owner.pubkey()],
                models: std::sync::Arc::new(anda_engine::model::Models::default()),
                http_client: crate::util::http_client::new_reqwest_client(),
                runtime_config: None,
            },
        )
        .await
        .unwrap();
        let space = brain
            .state
            .load_space(config::ANDA_BOT_SPACE_ID, true)
            .await
            .unwrap();
        assert!(
            brain
                .state
                .check_auth(
                    &token,
                    config::ANDA_BOT_SPACE_ID,
                    anda_brain::types::TokenScope::Read,
                    anda_engine::unix_ms()
                )
                .is_ok()
        );
        assert!(
            brain
                .state
                .check_auth(
                    &token,
                    "other_space",
                    anda_brain::types::TokenScope::Read,
                    anda_engine::unix_ms()
                )
                .is_err()
        );
        let url = crate::test_support::spawn_http_mock(brain.into_router()).await;
        assert!(
            !client
                .rebased(url)
                .brain()
                .runtime_status()
                .await
                .unwrap()
                .configured
        );
        space.close().await.unwrap();
    }

    #[tokio::test]
    async fn run_autostart_status_is_read_only() {
        let (_dir, daemon) = temp_daemon();
        run_autostart_command(&daemon, autostart::AutostartCommand::Status)
            .await
            .unwrap();
    }
}
