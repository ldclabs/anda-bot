//! End-to-end test bot — multi-user, interactive command testing.
//!
//! # Login (scan QR for each user):
//! ```bash
//! cargo run --example e2e_test -- --state-dir /path/to/state login
//! cargo run --example e2e_test -- --state-dir /path/to/state login  # repeat for more users
//! ```
//!
//! # Start all logged-in users:
//! ```bash
//! cargo run --example e2e_test -- --state-dir /path/to/state start
//! cargo run --example e2e_test -- --state-dir /path/to/state start --debug
//! cargo run --example e2e_test -- --state-dir /path/to/state start --download-dir /tmp/downloads
//! ```
//!
//! # Commands (send via WeChat):
//!   ping              → pong
//!   echo <text>       → echo back
//!   typing            → show typing 3s then reply
//!   info              → message details (debug)
//!   help              → command list
//!   [图片]            → reply image info + optional download
//!   [视频]            → reply video info + optional download
//!   [文件]            → reply file info + optional download
//!   [语音]            → reply transcription or voice info
//!   [引用消息]        → reply with quoted context

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use weixin_agent::{
    MediaInfo, MediaType, MessageContext, MessageHandler, Result, WeixinClient, WeixinConfig,
};

// ─── CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "e2e_test", about = "WeChat Agent SDK E2E Test Bot")]
struct Cli {
    /// State directory (required)
    #[arg(long)]
    state_dir: PathBuf,

    /// API base URL override
    #[arg(long)]
    base_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Login a new user via QR code (repeat to add more users)
    Login,
    /// Start all logged-in users
    Start {
        /// Enable debug logging
        #[arg(long)]
        debug: bool,
        /// Directory to save downloaded media
        #[arg(long)]
        download_dir: Option<PathBuf>,
    },
    /// Send a message to a specific user
    Send {
        /// User ID (from_user_id) to send to
        #[arg(long)]
        to: String,
        /// Text message to send
        #[arg(long)]
        text: String,
        /// Which bot to send from (directory name under state-dir, defaults to first)
        #[arg(long)]
        from: Option<String>,
    },
}

// ─── Handler ────────────────────────────────────────────────────────

struct E2eHandler {
    user_dir: PathBuf,
    download_dir: Option<PathBuf>,
}

#[async_trait::async_trait]
impl MessageHandler for E2eHandler {
    async fn on_message(&self, ctx: &MessageContext) -> Result<()> {
        let text = ctx.body.as_deref().unwrap_or("").trim();
        let has_media = ctx.media.is_some();
        let has_ref = ctx.ref_message.is_some();

        println!(
            "[{}] from={}, text='{}', media={}, ref={}",
            user_label(&self.user_dir),
            ctx.from,
            truncate(text, 40),
            has_media,
            has_ref,
        );

        // Media messages
        if let Some(media) = &ctx.media {
            self.handle_media(ctx, media).await?;
            return Ok(());
        }

        // Quoted messages
        if let Some(ref_msg) = &ctx.ref_message {
            let mut reply = format!(
                "📎 引用消息:\n标题: {}",
                ref_msg.title.as_deref().unwrap_or("(无)")
            );
            if let Some(body) = &ref_msg.body {
                reply += &format!("\n内容: {}", truncate(body, 100));
            }
            reply += &format!("\n\n你的回复: {text}");
            ctx.reply_text(&reply).await?;
            return Ok(());
        }

        // Text commands
        self.handle_command(ctx, text).await
    }

    async fn on_sync_buf_updated(&self, sync_buf: &str) -> Result<()> {
        if let Err(e) = common::save_sync_buf(&self.user_dir, sync_buf).await {
            tracing::error!(error = %e, "failed to save sync_buf");
        }
        Ok(())
    }

    async fn on_start(&self) -> Result<()> {
        println!("[{}] started", user_label(&self.user_dir));
        Ok(())
    }

    async fn on_shutdown(&self) -> Result<()> {
        println!("[{}] shutting down", user_label(&self.user_dir));
        Ok(())
    }
}

impl E2eHandler {
    async fn handle_command(&self, ctx: &MessageContext, text: &str) -> Result<()> {
        match text.to_lowercase().as_str() {
            "ping" => {
                ctx.reply_text("pong 🏓").await?;
            }
            t if t.starts_with("echo ") => {
                ctx.reply_text(&text[5..]).await?;
            }
            "typing" => {
                ctx.send_typing().await?;
                tokio::time::sleep(Duration::from_secs(3)).await;
                ctx.cancel_typing().await?;
                ctx.reply_text("✅ typing 测试完成").await?;
            }
            "info" => {
                let info = format!(
                    "📋 消息详情\n\n\
                     message_id: {}\n\
                     server_message_id: {:?}\n\
                     from: {}\n\
                     to: {}\n\
                     timestamp: {}\n\
                     session_id: {:?}\n\
                     context_token: {}\n\
                     body_len: {}\n\
                     has_media: {}\n\
                     has_ref: {}",
                    ctx.message_id,
                    ctx.server_message_id,
                    ctx.from,
                    ctx.to,
                    ctx.timestamp,
                    ctx.session_id,
                    ctx.context_token
                        .as_deref()
                        .map(|t| format!("{}...", &t[..t.len().min(8)]))
                        .unwrap_or_else(|| "(none)".into()),
                    ctx.body.as_deref().map_or(0, str::len),
                    ctx.media.is_some(),
                    ctx.ref_message.is_some(),
                );
                ctx.reply_text(&info).await?;
            }
            "help" | "" => {
                ctx.reply_text(
                    "🤖 E2E 测试机器人\n\n\
                     命令:\n\
                     • ping → pong\n\
                     • echo <文本> → 回声\n\
                     • typing → 输入状态测试\n\
                     • info → 消息详情\n\
                     • help → 本帮助\n\n\
                     媒体:\n\
                     • 发送图片/视频/文件/语音 → 回复详情\n\
                     • 引用消息 → 回复引用内容",
                )
                .await?;
            }
            _ => {
                ctx.reply_text(&format!("未知命令: {text}\n发送 help 查看帮助"))
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_media(&self, ctx: &MessageContext, media: &MediaInfo) -> Result<()> {
        let type_name = match media.media_type {
            MediaType::Image => "🖼️ 图片",
            MediaType::Video => "🎬 视频",
            MediaType::File => "📄 文件",
            MediaType::Voice => "🎤 语音",
        };

        let mut reply = format!(
            "{type_name}\n\n\
             文件名: {}\n\
             大小: {}",
            media.file_name.as_deref().unwrap_or("(未知)"),
            media
                .file_size
                .map(|s| format!("{:.1}KB", s as f64 / 1024.0))
                .unwrap_or_else(|| "(未知)".into()),
        );

        // Voice with transcription
        if media.media_type == MediaType::Voice {
            // Voice items with text are filtered out of media, so if we get here
            // it means no transcription. The text would be in ctx.body instead.
        }

        // Try download
        if let Some(dir) = &self.download_dir {
            reply += &self.try_download(ctx, media, dir).await;
        }

        ctx.reply_text(&reply).await?;
        Ok(())
    }

    async fn try_download(&self, ctx: &MessageContext, media: &MediaInfo, dir: &Path) -> String {
        let ext = match media.media_type {
            MediaType::Image => ".jpg",
            MediaType::Video => ".mp4",
            MediaType::Voice => ".silk",
            MediaType::File => media
                .file_name
                .as_deref()
                .and_then(|n| n.rfind('.').map(|i| &n[i..]))
                .unwrap_or(".bin"),
        };
        let filename = weixin_agent::util::random::temp_file_name("download", ext);
        let dest = dir.join(&filename);

        let start = std::time::Instant::now();
        match ctx.download_media(media, &dest).await {
            Ok(path) => {
                let elapsed = start.elapsed();
                let size = tokio::fs::metadata(&path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let msg = format!(
                    "\n\n✅ 下载成功: {:.1}KB, {elapsed:.1?}\n→ {}",
                    size as f64 / 1024.0,
                    path.display()
                );
                println!(
                    "[{}] downloaded {} → {} ({:.1}KB, {elapsed:.1?})",
                    user_label(&self.user_dir),
                    ext,
                    path.display(),
                    size as f64 / 1024.0,
                );
                msg
            }
            Err(e) => format!("\n\n❌ 下载失败: {e}"),
        }
    }
}

// ─── Multi-user management ──────────────────────────────────────────

/// Normalize ilink_bot_id to a filesystem-safe directory name.
/// e.g. "b0f5860fdecb@im.bot" → "b0f5860fdecb-im-bot"
fn normalize_bot_id(bot_id: &str) -> String {
    bot_id
        .replace(['@', '.'], "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// List user directories under state_dir (each containing token.txt)
async fn list_user_dirs(state_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    let mut entries = tokio::fs::read_dir(state_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() && path.join("token.txt").exists() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn user_label(user_dir: &Path) -> String {
    user_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!(
            "{}…",
            &s[..s
                .char_indices()
                .take_while(|(i, _)| *i <= max)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0)]
        )
    }
}

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tokio::fs::create_dir_all(&cli.state_dir).await?;

    match cli.command {
        Command::Login => {
            tracing_subscriber::fmt().with_env_filter("info").init();

            println!("开始 QR 登录...");
            let tmp_dir = cli.state_dir.join(".login_tmp");
            tokio::fs::create_dir_all(&tmp_dir).await?;

            // Load existing tokens from all user dirs for the QR login request
            let existing_tokens = common::load_existing_tokens(&cli.state_dir).await;
            tracing::info!(count = existing_tokens.len(), "loaded existing tokens");

            let (_token, bot_id) = common::qr_login(&tmp_dir, cli.base_url.as_deref()).await?;

            let dir_name = normalize_bot_id(&bot_id);
            let user_dir = cli.state_dir.join(&dir_name);

            if user_dir.exists() {
                tokio::fs::rename(tmp_dir.join("token.txt"), user_dir.join("token.txt")).await?;
                let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
                println!("✅ 用户 {dir_name} token 已更新");
            } else {
                tokio::fs::rename(&tmp_dir, &user_dir).await?;
                println!("✅ 新用户 {dir_name} 登录成功");
            }
        }

        Command::Start {
            debug,
            download_dir,
        } => {
            let filter = if debug { "debug" } else { "info" };
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| filter.into()),
                )
                .init();

            if let Some(dir) = &download_dir {
                tokio::fs::create_dir_all(dir).await?;
            }

            let user_dirs = list_user_dirs(&cli.state_dir).await?;
            if user_dirs.is_empty() {
                anyhow::bail!(
                    "没有已登录的用户。请先运行:\n  cargo run --example e2e_test -- --state-dir {} login",
                    cli.state_dir.display()
                );
            }

            println!("╔══════════════════════════════════════════╗");
            println!("║   WeChat Agent SDK — E2E Test Bot        ║");
            println!("╠══════════════════════════════════════════╣");
            println!("║ Users: {:<33}║", user_dirs.len());
            for d in &user_dirs {
                println!("║   {:<37}║", user_label(d));
            }
            println!("╠══════════════════════════════════════════╣");
            println!("║ Commands: ping echo typing info help     ║");
            println!("║ Media: 🖼️ 📄 🎬 🎤  Ref: 📎             ║");
            println!("╠══════════════════════════════════════════╣");
            println!(
                "║ Download: {:<30}║",
                download_dir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "disabled".into())
            );
            println!(
                "║ Debug: {:<33}║",
                if debug { "enabled" } else { "disabled" }
            );
            println!("╚══════════════════════════════════════════╝");

            // Start all users concurrently
            let mut handles = Vec::new();
            for user_dir in user_dirs {
                let download_dir = download_dir.clone();
                let base_url = cli.base_url.clone();

                handles.push(tokio::spawn(async move {
                    if let Err(e) = run_user(user_dir.clone(), base_url, download_dir).await {
                        eprintln!("[{}] error: {e}", user_label(&user_dir));
                    }
                }));
            }

            // Wait for all (Ctrl+C to stop)
            futures_util::future::join_all(handles).await;
        }

        Command::Send { to, text, from } => {
            tracing_subscriber::fmt().with_env_filter("info").init();

            let user_dirs = list_user_dirs(&cli.state_dir).await?;
            let user_dir = if let Some(name) = &from {
                let dir = cli.state_dir.join(name);
                anyhow::ensure!(dir.join("token.txt").exists(), "用户 {name} 不存在");
                dir
            } else {
                user_dirs
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("没有已登录的用户"))?
            };

            let token = tokio::fs::read_to_string(common::token_path(&user_dir))
                .await?
                .trim()
                .to_owned();
            let ctx_tokens = common::load_context_tokens(&user_dir).await;

            let mut builder = WeixinConfig::builder().token(&token);
            if let Some(url) = &cli.base_url {
                builder = builder.base_url(url);
            }
            let config = builder.build()?;

            // Need a dummy handler to build the client
            struct NoopHandler;
            #[async_trait::async_trait]
            impl MessageHandler for NoopHandler {
                async fn on_message(&self, _ctx: &MessageContext) -> Result<()> {
                    Ok(())
                }
            }

            let client = WeixinClient::builder(config)
                .on_message(NoopHandler)
                .build()?;
            client.context_tokens().import(ctx_tokens);

            let ct = client.context_tokens().get(&to);
            let result = client.send_text(&to, &text, ct.as_deref()).await?;

            println!(
                "✅ 消息已发送 (from={}, to={}, id={})",
                user_label(&user_dir),
                to,
                result.message_id
            );
        }
    }

    Ok(())
}

async fn run_user(
    user_dir: PathBuf,
    base_url: Option<String>,
    download_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let token = tokio::fs::read_to_string(common::token_path(&user_dir))
        .await?
        .trim()
        .to_owned();
    let sync_buf = common::load_sync_buf(&user_dir).await;
    let ctx_tokens = common::load_context_tokens(&user_dir).await;

    let mut builder = WeixinConfig::builder().token(&token);
    if let Some(url) = &base_url {
        builder = builder.base_url(url);
    }
    let config = builder.build()?;

    let client = WeixinClient::builder(config)
        .on_message(E2eHandler {
            user_dir: user_dir.clone(),
            download_dir,
        })
        .build()?;

    client.context_tokens().import(ctx_tokens);
    client.start(sync_buf).await?;
    Ok(())
}
