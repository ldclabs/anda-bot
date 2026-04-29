use anda_core::{AgentInput, BoxError, ByteBufB64, Json, RequestMeta, Resource, ToolInput};
use clap::{Parser, Subcommand};
use ic_auth_types::Xid;
use mimalloc::MiMalloc;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

mod brain;
mod channel;
mod cli;
mod config;
mod cron;
mod daemon;
mod engine;
mod gateway;
mod transcription;
mod tts;
mod tui;
mod util;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
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
    /// Tool-related operations against the running daemon.
    #[command(subcommand)]
    Tool(ToolCommand),
    /// Agent-related operations against the running daemon.
    #[command(subcommand)]
    Agent(AgentCommand),
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
    /// Run an agent with the given prompt.
    Run {
        /// Agent name. Empty value uses the default agent.
        #[arg(long, default_value = "")]
        name: String,
        /// User prompt sent to the agent.
        #[arg(long)]
        prompt: Option<String>,
        /// Audio file(s) sent as voice input resources.
        #[arg(long, value_name = "PATH")]
        audio: Vec<PathBuf>,
        /// Record microphone audio before sending and play returned speech audio.
        #[arg(long)]
        voice: bool,
        /// Recording duration in seconds when --voice is used without --audio.
        #[arg(long, default_value_t = 5)]
        record_secs: u64,
        /// Do not play returned speech audio artifacts.
        #[arg(long)]
        no_playback: bool,
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
    let home = if let Some(home) = cli.home {
        PathBuf::from(home)
    } else {
        default_home()
    };

    tokio::fs::create_dir_all(&home).await?;

    let cfg = Cli::parse();

    match cfg.command {
        None => {
            let daemon = load_daemon(home).await?;
            let ed25519_secret =
                load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
            let ed25519_key = util::key::Ed25519Key::new(ed25519_secret);
            let cli = cli::Cli::new(ed25519_key, daemon);
            cli.run().await?
        }
        Some(Commands::Daemon) => {
            let daemon = load_daemon(home).await?;
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
            let daemon = load_daemon(home).await?;
            match daemon.stop_background(Duration::from_secs(10)).await? {
                daemon::StopState::NotRunning => println!("anda daemon is not running"),
                daemon::StopState::Stopped(pid) => {
                    println!("Stopped anda daemon (pid {pid})")
                }
            }
        }
        Some(Commands::Restart) | Some(Commands::Reload) => {
            let daemon = load_daemon(home).await?;
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
        Some(Commands::Tool(cmd)) => {
            let daemon = load_daemon(home).await?;
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
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
            }
        }
        Some(Commands::Agent(cmd)) => {
            let daemon = load_daemon(home).await?;
            let client = build_control_client(&daemon).await?;
            client.ensure_daemon_running(&daemon).await?;

            match cmd {
                AgentCommand::Run {
                    name,
                    prompt,
                    mut audio,
                    voice,
                    record_secs,
                    no_playback,
                    meta,
                } => {
                    let voice_mode = voice || !audio.is_empty();
                    let mut recorded_audio = None;
                    if voice && audio.is_empty() {
                        eprintln!("Recording microphone for {record_secs}s...");
                        let path = record_microphone_audio(record_secs).await?;
                        recorded_audio = Some(path.clone());
                        audio.push(path);
                    }

                    let prompt = prompt.unwrap_or_default();
                    if prompt.trim().is_empty() && audio.is_empty() {
                        return Err(
                            "--prompt is required unless --voice or --audio is provided".into()
                        );
                    }

                    let mut input = AgentInput::new(name, prompt);
                    let mut audio_resources = Vec::with_capacity(audio.len());
                    for path in &audio {
                        match audio_file_resource(path).await {
                            Ok(resource) => audio_resources.push(resource),
                            Err(err) => {
                                if let Some(path) = recorded_audio.take() {
                                    let _ = tokio::fs::remove_file(path).await;
                                }
                                return Err(err);
                            }
                        }
                    }
                    input.resources = audio_resources;
                    if let Some(path) = recorded_audio.take() {
                        let _ = tokio::fs::remove_file(path).await;
                    }

                    let has_meta = meta.is_some();
                    let mut request_meta: RequestMeta = if let Some(meta) = meta {
                        serde_json::from_str(&meta)
                            .map_err(|e| format!("invalid --meta JSON: {e}"))?
                    } else {
                        RequestMeta::default()
                    };
                    if voice_mode {
                        request_meta
                            .extra
                            .insert("voice_response".to_string(), true.into());
                        request_meta
                            .extra
                            .insert("wait_completion".to_string(), true.into());
                    }
                    if has_meta || voice_mode {
                        input.meta = Some(request_meta);
                    }

                    let output = client.agent_run(&input).await?;
                    println!("{}", serde_json::to_string_pretty(&output)?);
                    if voice_mode && !no_playback {
                        play_audio_artifacts(&output.artifacts).await?;
                    }
                }
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

async fn audio_file_resource(path: &Path) -> Result<Resource, BoxError> {
    let bytes = tokio::fs::read(path).await?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("audio path must include a valid UTF-8 file name")?
        .to_string();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .ok_or("audio file must have an extension")?;
    let mime_type = audio_mime_for_extension(&extension)
        .ok_or_else(|| format!("unsupported audio file extension: .{extension}"))?;
    let size = bytes.len() as u64;

    Ok(Resource {
        tags: vec!["audio".to_string(), extension],
        name,
        description: Some("Voice input captured by anda CLI".to_string()),
        mime_type: Some(mime_type.to_string()),
        blob: Some(ByteBufB64(bytes)),
        size: Some(size),
        ..Default::default()
    })
}

async fn record_microphone_audio(duration_secs: u64) -> Result<PathBuf, BoxError> {
    if duration_secs == 0 {
        return Err("--record-secs must be greater than zero".into());
    }

    let output_path = std::env::temp_dir().join(format!("anda_bot_voice_{}.wav", Xid::new()));
    let output = output_path
        .to_str()
        .ok_or("failed to create temporary recording path")?;
    let duration = duration_secs.to_string();

    if command_available("rec") {
        let mut command = tokio::process::Command::new("rec");
        command.args([
            "-q", "-r", "16000", "-c", "1", "-b", "16", output, "trim", "0", &duration,
        ]);
        run_process(command, "rec").await?;
        return Ok(output_path);
    }

    if command_available("ffmpeg") {
        let mut command = tokio::process::Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-y"]);
        add_ffmpeg_microphone_args(&mut command, &duration, output)?;
        run_process(command, "ffmpeg").await?;
        return Ok(output_path);
    }

    Err("voice recording requires `rec` (SoX) or `ffmpeg` on PATH".into())
}

#[cfg(target_os = "macos")]
fn add_ffmpeg_microphone_args(
    command: &mut tokio::process::Command,
    duration: &str,
    output: &str,
) -> Result<(), BoxError> {
    command.args([
        "-f",
        "avfoundation",
        "-i",
        ":0",
        "-t",
        duration,
        "-ac",
        "1",
        "-ar",
        "16000",
        output,
    ]);
    Ok(())
}

#[cfg(target_os = "linux")]
fn add_ffmpeg_microphone_args(
    command: &mut tokio::process::Command,
    duration: &str,
    output: &str,
) -> Result<(), BoxError> {
    command.args([
        "-f", "pulse", "-i", "default", "-t", duration, "-ac", "1", "-ar", "16000", output,
    ]);
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn add_ffmpeg_microphone_args(
    _command: &mut tokio::process::Command,
    _duration: &str,
    _output: &str,
) -> Result<(), BoxError> {
    Err("ffmpeg microphone recording is only wired for macOS and Linux".into())
}

async fn play_audio_artifacts(artifacts: &[Resource]) -> Result<(), BoxError> {
    let mut played = false;
    for artifact in artifacts {
        if transcription::is_audio_resource(artifact)
            && let Some(blob) = &artifact.blob
        {
            let path = write_temp_audio_artifact(artifact, &blob.0).await?;
            play_audio_file(&path).await?;
            let _ = tokio::fs::remove_file(path).await;
            played = true;
        }
    }

    if !played {
        eprintln!(
            "No playable audio artifact was returned. Check tts.enabled and provider config."
        );
    }
    Ok(())
}

async fn write_temp_audio_artifact(resource: &Resource, bytes: &[u8]) -> Result<PathBuf, BoxError> {
    let extension = resource
        .name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .or_else(|| {
            resource
                .mime_type
                .as_deref()
                .and_then(audio_extension_for_mime)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "mp3".to_string());
    let path = std::env::temp_dir().join(format!("anda_bot_play_{}.{}", Xid::new(), extension));
    tokio::fs::write(&path, bytes).await?;
    Ok(path)
}

async fn play_audio_file(path: &Path) -> Result<(), BoxError> {
    let output = path.to_str().ok_or("invalid temporary playback path")?;
    if cfg!(target_os = "macos") && command_available("afplay") {
        let mut command = tokio::process::Command::new("afplay");
        command.arg(output);
        return run_process(command, "afplay").await;
    }

    if command_available("ffplay") {
        let mut command = tokio::process::Command::new("ffplay");
        command.args(["-nodisp", "-autoexit", "-loglevel", "quiet", output]);
        return run_process(command, "ffplay").await;
    }

    if command_available("play") {
        let mut command = tokio::process::Command::new("play");
        command.args(["-q", output]);
        return run_process(command, "play").await;
    }

    Err("audio playback requires `afplay`, `ffplay`, or `play` on PATH".into())
}

async fn run_process(mut command: tokio::process::Command, label: &str) -> Result<(), BoxError> {
    let output = command.output().await?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{label} failed ({}): {}", output.status, stderr.trim()).into())
}

fn command_available(command: &str) -> bool {
    std::process::Command::new("which")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn audio_mime_for_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "flac" => Some("audio/flac"),
        "mp3" | "mpeg" | "mpga" => Some("audio/mpeg"),
        "mp4" | "m4a" => Some("audio/mp4"),
        "oga" | "ogg" => Some("audio/ogg"),
        "opus" => Some("audio/opus"),
        "wav" => Some("audio/wav"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

fn audio_extension_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type.to_ascii_lowercase().as_str() {
        "audio/flac" => Some("flac"),
        "audio/mp4" | "audio/x-m4a" => Some("m4a"),
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/ogg" | "audio/oga" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/webm" => Some("webm"),
        _ => None,
    }
}
