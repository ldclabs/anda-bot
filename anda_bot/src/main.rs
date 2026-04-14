use anda_core::BoxError;
use anda_web3_client::client::identity_from_secret;
use clap::{Parser, Subcommand};
use mimalloc::MiMalloc;
use std::{collections::BTreeSet, path::PathBuf};

mod brain;
mod cron;
mod daemon;
mod engine;
mod gateway;
mod util;

use anda_hippocampus::types::ModelConfig;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to workspace directory for storing data (defaults to '~/.anda')
    #[arg(long, short)]
    workspace: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    Daemon {
        #[clap(long, env = "GATEWAY_ADDR", default_value = "127.0.0.1:8042")]
        addr: String,

        #[arg(long, env = "SANDBOX")]
        sandbox: Option<String>,

        /// AI model family (e.g., "gemini", "anthropic", "openai", "deepseek", "mimo")
        #[arg(long, env = "MODEL_FAMILY", default_value = "")]
        model_family: String,

        /// AI model name (e.g., "gemini-3-flash-preview", "claude-sonnet-4-6")
        #[arg(long, env = "MODEL_NAME", default_value = "")]
        model_name: String,

        /// API key for AI model
        #[arg(long, env = "MODEL_API_KEY", default_value = "")]
        model_api_key: String,

        /// API base URL for AI model
        #[arg(long, env = "MODEL_API_BASE", default_value = "")]
        model_api_base: String,

        /// Optional HTTPS proxy URL (e.g., "http://127.0.0.1:23456")
        #[arg(long, env = "HTTPS_PROXY")]
        https_proxy: Option<String>,
    },
}

/// ```bash
/// cargo run -p anda_bot -- --help
/// ```
#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let cli = Cli::parse();
    let workspace = if let Some(ws) = cli.workspace {
        PathBuf::from(ws)
    } else {
        std::env::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".anda")
    };

    let env_path = workspace.join(".env");
    dotenv::from_path(env_path).ok();
    // Load configuration from environment variables again.
    let cfg = Cli::parse();

    tokio::fs::create_dir_all(&workspace).await?;
    let db_path = workspace.join("db");
    tokio::fs::create_dir_all(&db_path).await?;
    let keys_path = workspace.join("keys");
    tokio::fs::create_dir_all(&keys_path).await?;

    match cfg.command {
        Some(Commands::Daemon {
            addr,
            sandbox,
            model_family,
            model_name,
            model_api_key,
            model_api_base,
            https_proxy,
        }) => {
            let ed25519_secret =
                load_or_init_ed25519_secret(&keys_path.join("anda_bot.key")).await?;
            let ed25519_pubkey = util::cose::to_ed25519_pubkey(&ed25519_secret);
            let id = identity_from_secret(ed25519_secret).sender().unwrap();

            let brain_cfg = brain::HippocampusConfig {
                ed25519_pubkey,
                https_proxy,
                managers: BTreeSet::from_iter([id]),
                model: ModelConfig {
                    family: model_family,
                    model: model_name,
                    api_base: model_api_base,
                    api_key: model_api_key,
                    disabled: false,
                },
            };
            let engine_cfg = engine::EngineConfig {
                ed25519_secret,
                model: brain_cfg.model.clone(),
                brain_base_url: format!("http://{}/v1/{}", addr, brain::ANDA_BOT_SPACE_ID),
                https_proxy: brain_cfg.https_proxy.clone(),
                sandbox,
            };
            let rt = daemon::serve(workspace, addr, brain_cfg, engine_cfg).await;
            if let Err(err) = rt {
                log::error!(name = "daemon"; "Service exited with error: {:?}", err);
            }
        }
        None => {
            println!("TODO: anda cli. Use --help for usage information.");
        }
    }
    Ok(())
}

async fn load_or_init_ed25519_secret(key_path: &PathBuf) -> Result<[u8; 32], BoxError> {
    match tokio::fs::read_to_string(key_path).await {
        Ok(content) => {
            let secret = util::cose::parse_ed25519_privkey(content.trim())?;
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
            let secret = util::cose::random_ed25519_privkey();
            let encoded = util::cose::encode_ed25519_privkey(&secret)?;
            tokio::fs::write(key_path, encoded).await?;
            Ok(secret)
        }
    }
}
