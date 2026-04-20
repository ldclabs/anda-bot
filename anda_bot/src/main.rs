use anda_core::BoxError;
use clap::{Parser, Subcommand};
use mimalloc::MiMalloc;
use std::path::PathBuf;

mod brain;
mod cli;
mod cron;
mod daemon;
mod engine;
mod gateway;
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
    Daemon(daemon::DaemonArgs),
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

    let env_path = home.join(".env");
    dotenv::from_path(&env_path).ok();
    // Load configuration from environment variables again.
    let cfg = Cli::parse();

    match cfg.command {
        Some(Commands::Daemon(args)) => {
            let daemon = daemon::Daemon::new(home, args);
            daemon.ensure_directories().await?;

            let ed25519_secret =
                load_or_init_ed25519_secret(&daemon.keys_dir_path().join("anda_bot.key")).await?;
            let ed25519_key = util::key::Ed25519Key::new(ed25519_secret);
            let user_secret =
                load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
            let user_key = util::key::Ed25519Key::new(user_secret);
            daemon.serve(ed25519_key, user_key.pubkey()).await?
        }
        None => {
            let daemon =
                daemon::Daemon::new(home, daemon::DaemonArgs::from_env_file(&env_path).await?);
            let ed25519_secret =
                load_or_init_ed25519_secret(&daemon.keys_dir_path().join("user.key")).await?;
            let ed25519_key = util::key::Ed25519Key::new(ed25519_secret);
            let cli = cli::Cli::new(ed25519_key, daemon);
            cli.run().await?
        }
    }
    Ok(())
}

fn default_home() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".anda")
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
