//! Echo bot — replies with the same text it receives.
//!
//! ```bash
//! cargo run --example echo_bot -- --state-dir /path/to/state
//! ```

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use weixin_agent::{MessageContext, MessageHandler, Result, WeixinClient, WeixinConfig};

struct EchoBot {
    state_dir: PathBuf,
    client: Option<Arc<WeixinClient>>,
}

#[async_trait::async_trait]
impl MessageHandler for EchoBot {
    async fn on_message(&self, ctx: &MessageContext) -> Result<()> {
        if let Some(text) = &ctx.body {
            tracing::info!(from = %ctx.from, text = %text, "received");
            ctx.reply_text(text).await?;
        }
        Ok(())
    }

    async fn on_sync_buf_updated(&self, sync_buf: &str) -> Result<()> {
        if let Err(e) = common::save_sync_buf(&self.state_dir, sync_buf).await {
            tracing::error!(error = %e, "failed to save sync_buf");
        }
        Ok(())
    }

    async fn on_shutdown(&self) -> Result<()> {
        if let Some(c) = &self.client {
            let tokens = c.context_tokens().export_all();
            if let Err(e) = common::save_context_tokens(&self.state_dir, &tokens).await {
                tracing::error!(error = %e, "failed to save context tokens");
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = common::BotArgs::parse();
    tokio::fs::create_dir_all(&args.state_dir).await?;

    let token = common::resolve_token(&args).await?;
    let sync_buf = common::load_sync_buf(&args.state_dir).await;
    let ctx_tokens = common::load_context_tokens(&args.state_dir).await;

    let mut config_builder = WeixinConfig::builder().token(&token);
    if let Some(url) = &args.base_url {
        config_builder = config_builder.base_url(url);
    }
    let config = config_builder.build()?;

    let client = WeixinClient::builder(config)
        .on_message(EchoBot {
            state_dir: args.state_dir.clone(),
            client: None,
        })
        .build()?;

    client.context_tokens().import(ctx_tokens);

    tracing::info!(state_dir = %args.state_dir.display(), "echo bot starting");
    client.start(sync_buf).await?;
    Ok(())
}
