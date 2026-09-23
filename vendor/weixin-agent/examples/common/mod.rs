//! Shared utilities for examples: CLI args, QR login flow, state persistence.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use weixin_agent::{LoginStatus, StandaloneQrLogin, WeixinConfig};

/// Common CLI arguments for example bots.
#[derive(Parser, Debug)]
pub struct BotArgs {
    /// State directory for persisting sync_buf, token, etc. (required)
    #[arg(long)]
    pub state_dir: PathBuf,

    /// Bot token (overrides saved token). Can also use WEIXIN_TOKEN env var.
    #[arg(long, env = "WEIXIN_TOKEN")]
    pub token: Option<String>,

    /// API base URL override.
    #[arg(long)]
    pub base_url: Option<String>,
}

// ── State file paths ────────────────────────────────────────────────

pub fn token_path(dir: &Path) -> PathBuf {
    dir.join("token.txt")
}
pub fn sync_buf_path(dir: &Path) -> PathBuf {
    dir.join("sync_buf.json")
}
pub fn context_tokens_path(dir: &Path) -> PathBuf {
    dir.join("context_tokens.json")
}

// ── Token resolution ────────────────────────────────────────────────

/// Resolve bot token: CLI arg / env > saved file > QR login.
#[allow(dead_code)]
pub async fn resolve_token(args: &BotArgs) -> anyhow::Result<String> {
    if let Some(t) = &args.token {
        return Ok(t.clone());
    }

    let tp = token_path(&args.state_dir);
    if tp.exists() {
        let saved = tokio::fs::read_to_string(&tp).await?.trim().to_owned();
        if !saved.is_empty() {
            tracing::info!(path = %tp.display(), "loaded saved token");
            return Ok(saved);
        }
    }

    tracing::info!("no token found, starting QR login...");
    let (token, _bot_id) = qr_login(&args.state_dir, args.base_url.as_deref()).await?;
    Ok(token)
}

// ── QR login ────────────────────────────────────────────────────────

/// Interactive QR login: display QR in terminal, poll until confirmed, save token.
/// Returns (token, ilink_bot_id).
pub async fn qr_login(
    state_dir: &Path,
    base_url: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let mut builder = WeixinConfig::builder().token("");
    if let Some(url) = base_url {
        builder = builder.base_url(url);
    }
    let config = builder.build()?;
    let qr = StandaloneQrLogin::new(&config);

    let local_tokens = load_existing_tokens(state_dir).await;
    let mut session = qr.start(None, &local_tokens).await?;
    print_qr(&session.qrcode_img_content);

    let mut refresh_count = 0u32;
    let mut verify_code: Option<String> = None;
    loop {
        match qr.poll_status(&session, verify_code.as_deref()).await? {
            LoginStatus::Confirmed {
                bot_token,
                ilink_bot_id,
                base_url,
                ilink_user_id,
            } => {
                tracing::info!(bot_id = %ilink_bot_id, user_id = %ilink_user_id, base_url = %base_url, "login confirmed");
                tokio::fs::write(token_path(state_dir), &bot_token).await?;
                return Ok((bot_token, ilink_bot_id));
            }
            LoginStatus::Scanned => {
                tracing::info!("scanned, waiting for confirmation...");
                verify_code = None;
            }
            LoginStatus::Expired => {
                refresh_count += 1;
                if refresh_count >= 3 {
                    anyhow::bail!("QR code expired 3 times, giving up");
                }
                tracing::warn!("QR expired, refreshing ({refresh_count}/3)...");
                session = qr.start(None, &local_tokens).await?;
                print_qr(&session.qrcode_img_content);
                verify_code = None;
            }
            LoginStatus::NeedVerifyCode => {
                println!("请输入验证码:");
                let mut code = String::new();
                std::io::stdin().read_line(&mut code)?;
                verify_code = Some(code.trim().to_owned());
            }
            LoginStatus::VerifyCodeBlocked => {
                refresh_count += 1;
                if refresh_count >= 3 {
                    anyhow::bail!("verify code blocked 3 times, giving up");
                }
                tracing::warn!("verify code blocked, refreshing QR ({refresh_count}/3)...");
                session = qr.start(None, &local_tokens).await?;
                print_qr(&session.qrcode_img_content);
                verify_code = None;
            }
            LoginStatus::BindedRedirect => {
                anyhow::bail!("account already bound to another bot, cannot login");
            }
            LoginStatus::Wait | LoginStatus::ScannedButRedirect { .. } => {
                verify_code = None;
            }
            _ => {
                verify_code = None;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn print_qr(content: &str) {
    println!("\n请使用微信扫描以下二维码登录:\n");
    if let Err(e) = qr2term::print_qr(content) {
        eprintln!("无法生成终端二维码: {e}");
        println!("请手动访问: {content}");
    }
    println!();
}

/// Load existing bot tokens from all user directories under `state_dir`.
pub async fn load_existing_tokens(state_dir: &Path) -> Vec<String> {
    let mut tokens = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(state_dir).await else {
        return tokens;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            let token_file = path.join("token.txt");
            if let Ok(t) = tokio::fs::read_to_string(&token_file).await {
                let t = t.trim().to_owned();
                if !t.is_empty() {
                    tokens.push(t);
                }
            }
        }
    }
    tokens
}

// ── State persistence helpers ───────────────────────────────────────

pub async fn load_sync_buf(state_dir: &Path) -> Option<String> {
    let data = tokio::fs::read_to_string(sync_buf_path(state_dir))
        .await
        .ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
    parsed
        .get("get_updates_buf")
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub async fn save_sync_buf(state_dir: &Path, sync_buf: &str) -> anyhow::Result<()> {
    let json = serde_json::json!({ "get_updates_buf": sync_buf });
    tokio::fs::write(sync_buf_path(state_dir), json.to_string()).await?;
    Ok(())
}

pub async fn load_context_tokens(state_dir: &Path) -> HashMap<String, String> {
    let Ok(data) = tokio::fs::read_to_string(context_tokens_path(state_dir)).await else {
        return HashMap::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

#[allow(dead_code)]
pub async fn save_context_tokens(
    state_dir: &Path,
    tokens: &HashMap<String, String>,
) -> anyhow::Result<()> {
    tokio::fs::write(
        context_tokens_path(state_dir),
        serde_json::to_string(tokens)?,
    )
    .await?;
    Ok(())
}
