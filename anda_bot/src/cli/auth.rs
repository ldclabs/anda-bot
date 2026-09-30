//! ChatGPT authentication through the shared daemon, including first-run setup.
use crate::{
    chatgpt::{AccountsView, LoginView, api::Request},
    daemon::Daemon,
    gateway::Client,
};
use anda_core::BoxError;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct AuthCommand {
    #[command(subcommand)]
    pub command: AuthAction,
}
#[derive(Subcommand)]
pub enum AuthAction {
    /// Connect a ChatGPT plan and select a model (starts the setup daemon if needed).
    Login {
        #[arg(value_parser=["chatgpt"])]
        provider: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Print the authorization URL instead of opening the system browser.
        #[arg(long)]
        no_browser: bool,
        /// Fixed loopback callback port, useful for SSH port forwarding.
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Ask again for permission to use the ChatGPT plan.
        #[arg(long)]
        consent: bool,
    },
    /// Show saved accounts and authorization state, without printing tokens.
    Status,
    /// Move a session to a protected file for transfer over SSH. Disconnects this host.
    Export {
        profile: String,
        #[arg(long)]
        output: std::path::PathBuf,
    },
    /// Move a protected transfer file into this host; removes the file after import.
    Import { path: std::path::PathBuf },
    /// List saved accounts.
    Accounts,
    /// Select a saved ChatGPT account.
    Select { profile: String },
    /// Revoke the renewable session and clear local tokens.
    Logout { profile: String },
    /// List models available to an account.
    Models { profile: String },
    /// Activate a model for this account in config.yaml.
    Use { profile: String, model: String },
}
pub async fn run(daemon: &Daemon, client: &Client, cmd: AuthCommand) -> Result<(), BoxError> {
    client.ensure_daemon_running(daemon).await?;
    let request = match cmd.command {
        AuthAction::Login {
            profile,
            model,
            no_browser,
            port,
            consent,
            ..
        } => {
            let flow: LoginView = serde_json::from_value(
                client
                    .chatgpt(&Request::LoginStart {
                        profile_id: profile,
                        port,
                        consent,
                    })
                    .await?,
            )?;
            let url = flow
                .authorization_url
                .as_deref()
                .ok_or("missing sign-in URL")?;
            eprintln!("Continue with ChatGPT: {url}");
            if !no_browser && let Err(e) = open_browser(url) {
                eprintln!("Could not open the browser: {e}. Open the URL above manually.");
            }
            let completed = loop {
                tokio::select! {
                    _=tokio::signal::ctrl_c()=>{let _=client.chatgpt(&Request::LoginCancel{flow_id:flow.flow_id.clone()}).await;return Err("ChatGPT sign-in cancelled".into());}
                    _=tokio::time::sleep(std::time::Duration::from_secs(1))=>{}
                }
                let current: LoginView = serde_json::from_value(
                    client
                        .chatgpt(&Request::LoginStatus {
                            flow_id: flow.flow_id.clone(),
                        })
                        .await?,
                )?;
                match current.status.as_str() {
                    "completed" => break current,
                    "pending" | "exchanging" => {}
                    _ => {
                        return Err(current
                            .error
                            .unwrap_or_else(|| format!("ChatGPT sign-in {}", current.status))
                            .into());
                    }
                }
            };
            let id = completed
                .account_id
                .ok_or("sign-in did not return an account")?;
            let accounts: AccountsView =
                serde_json::from_value(client.chatgpt(&Request::Accounts).await?)?;
            if !accounts
                .accounts
                .iter()
                .any(|a| a.id == id && a.plan_enabled)
            {
                println!(
                    "Signed in. ChatGPT plan permission is disabled. Run `anda auth login chatgpt --profile {id} --consent` to enable it."
                );
                return Ok(());
            }
            let models = client
                .chatgpt(&Request::Models {
                    profile_id: id.clone(),
                })
                .await?;
            let selected = model
                .or_else(|| {
                    models
                        .pointer("/models/0/slug")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .ok_or("No models are available for this account")?;
            Request::ModelSelect {
                profile_id: id,
                model: selected,
            }
        }
        AuthAction::Export { profile, output } => Request::TransferExport {
            profile_id: profile,
            path: std::path::absolute(output)?,
        },
        AuthAction::Import { path } => Request::TransferImport {
            path: std::path::absolute(path)?,
        },
        AuthAction::Status | AuthAction::Accounts => Request::Accounts,
        AuthAction::Select { profile } => Request::AccountSelect {
            profile_id: profile,
        },
        AuthAction::Logout { profile } => Request::Logout {
            profile_id: profile,
        },
        AuthAction::Models { profile } => Request::Models {
            profile_id: profile,
        },
        AuthAction::Use { profile, model } => Request::ModelSelect {
            profile_id: profile,
            model,
        },
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&client.chatgpt(&request).await?)?
    );
    Ok(())
}
pub(crate) fn open_browser(url: &str) -> Result<(), BoxError> {
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("auth.openai.com") {
        return Err("Unexpected ChatGPT authorization URL".into());
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    result?;
    Ok(())
}
