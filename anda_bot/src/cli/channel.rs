use anda_core::BoxError;
use clap::Subcommand;
use reqwest::Client;
use std::{collections::HashMap, sync::Arc};

use crate::{
    channel::{self as channel_runtime, Channel, ChannelInitOptions},
    config::{self, Config},
    daemon::Daemon,
    util::http_client::build_http_client,
};

#[derive(Subcommand)]
pub enum ChannelCommand {
    /// List channels configured in config.yaml.
    List,
    /// Run a channel-specific direct initialization workflow.
    Init {
        /// Channel id, type, or local id (for example: wechat:personal, wechat, personal).
        #[arg(value_name = "CHANNEL")]
        target: Option<String>,
        /// Initialize every configured channel.
        #[arg(long)]
        all: bool,
        /// Re-run initialization even if saved state already exists.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone)]
struct ChannelRow {
    id: String,
    name: &'static str,
    username: String,
}

pub async fn run(daemon: &Daemon, cmd: ChannelCommand) -> Result<(), BoxError> {
    let cfg = load_cli_config(daemon).await?;
    match cmd {
        ChannelCommand::List => list_channels(daemon, &cfg),
        ChannelCommand::Init { target, all, force } => {
            init_channels(daemon, &cfg, target.as_deref(), all, force).await
        }
    }
}

async fn load_cli_config(daemon: &Daemon) -> Result<Config, BoxError> {
    daemon.ensure_directories().await?;
    daemon.ensure_config_file_exists().await?;
    daemon.load_config_from_disk().await
}

fn list_channels(daemon: &Daemon, cfg: &Config) -> Result<(), BoxError> {
    let rows = configured_channel_rows(cfg);
    if rows.is_empty() {
        println!(
            "No channels are configured in {}",
            daemon.config_file_path().display()
        );
        return Ok(());
    }

    println!("Configured channels:");
    for row in rows {
        println!("- {} ({}, user: {})", row.id, row.name, row.username);
    }
    Ok(())
}

async fn init_channels(
    daemon: &Daemon,
    cfg: &Config,
    target: Option<&str>,
    all: bool,
    force: bool,
) -> Result<(), BoxError> {
    let rows = configured_channel_rows(cfg);
    let target_ids = resolve_channel_targets(&rows, target, all)?;
    let http_client = build_http_client(cfg.https_proxy.clone(), |client| client)?;
    let options = ChannelInitOptions { force };

    for target_id in target_ids {
        let (channel_id, channel) = build_configured_channel(cfg, &target_id, http_client.clone())?;
        channel.set_workspace(daemon.channels_dir_path().join(&channel_id));
        let result = channel.init(options).await?;
        let status = if result.changed { "initialized" } else { "ok" };
        println!("{channel_id}: {status} - {}", result.message);
    }

    Ok(())
}

fn configured_channel_rows(cfg: &Config) -> Vec<ChannelRow> {
    let mut rows = Vec::new();

    for item in cfg.channels.irc.iter().filter(|item| !item.is_empty()) {
        rows.push(ChannelRow {
            id: format!("irc:{}", item.channel_id()),
            name: "irc",
            username: config::normalize_optional(&item.username)
                .unwrap_or_else(|| item.nickname.clone()),
        });
    }

    for item in cfg.channels.telegram.iter().filter(|item| !item.is_empty()) {
        rows.push(ChannelRow {
            id: format!("telegram:{}", item.channel_id()),
            name: "telegram",
            username: item
                .username
                .clone()
                .unwrap_or_else(|| "telegram".to_string()),
        });
    }

    for item in cfg.channels.wechat.iter().filter(|item| !item.is_empty()) {
        rows.push(ChannelRow {
            id: format!("wechat:{}", item.channel_id()),
            name: "wechat",
            username: item
                .username
                .clone()
                .unwrap_or_else(|| "wechat".to_string()),
        });
    }

    for item in cfg.channels.discord.iter().filter(|item| !item.is_empty()) {
        rows.push(ChannelRow {
            id: format!("discord:{}", item.channel_id()),
            name: "discord",
            username: item
                .username
                .clone()
                .unwrap_or_else(|| "discord".to_string()),
        });
    }

    for item in cfg.channels.lark.iter().filter(|item| !item.is_empty()) {
        let name = item.platform.channel_name();
        rows.push(ChannelRow {
            id: format!("{name}:{}", item.channel_id()),
            name,
            username: item.username.clone().unwrap_or_else(|| name.to_string()),
        });
    }

    rows
}

fn resolve_channel_targets(
    rows: &[ChannelRow],
    target: Option<&str>,
    all: bool,
) -> Result<Vec<String>, BoxError> {
    if all && target.is_some() {
        return Err("provide either --all or a channel target, not both".into());
    }

    if rows.is_empty() {
        return Err("no channels are configured".into());
    }

    if all {
        return Ok(rows.iter().map(|row| row.id.clone()).collect());
    }

    let Some(target) = target.map(str::trim).filter(|target| !target.is_empty()) else {
        return if rows.len() == 1 {
            Ok(vec![rows[0].id.clone()])
        } else {
            Err(format!(
                "multiple channels are configured; specify one of: {}",
                available_channel_ids(rows)
            )
            .into())
        };
    };

    let exact: Vec<&ChannelRow> = rows.iter().filter(|row| row.id == target).collect();
    if !exact.is_empty() {
        return selected_channel_ids(exact, target);
    }

    let matches: Vec<&ChannelRow> = rows
        .iter()
        .filter(|row| {
            row.name == target || row.id.split_once(':').is_some_and(|(_, id)| id == target)
        })
        .collect();
    selected_channel_ids(matches, target)
}

fn selected_channel_ids(rows: Vec<&ChannelRow>, target: &str) -> Result<Vec<String>, BoxError> {
    match rows.as_slice() {
        [row] => Ok(vec![row.id.clone()]),
        [] => Err(format!("channel '{target}' is not configured").into()),
        _ => Err(format!(
            "channel target '{target}' is ambiguous; specify one of: {}",
            rows.iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into()),
    }
}

fn available_channel_ids(rows: &[ChannelRow]) -> String {
    rows.iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_configured_channel(
    cfg: &Config,
    id: &str,
    http_client: Client,
) -> Result<(String, Arc<dyn Channel>), BoxError> {
    let Some((kind, local_id)) = id.split_once(':') else {
        return Err(format!("channel id '{id}' must be in '<type>:<id>' form").into());
    };

    match kind {
        "irc" => {
            let settings = cfg
                .channels
                .irc
                .iter()
                .filter(|item| !item.is_empty() && item.channel_id() == local_id)
                .cloned()
                .collect::<Vec<_>>();
            single_built_channel(channel_runtime::irc::build_irc_channels(&settings)?, id)
        }
        "telegram" => {
            let settings = cfg
                .channels
                .telegram
                .iter()
                .filter(|item| !item.is_empty() && item.channel_id() == local_id)
                .cloned()
                .collect::<Vec<_>>();
            single_built_channel(
                channel_runtime::telegram::build_telegram_channels(&settings, http_client)?,
                id,
            )
        }
        "wechat" => {
            let settings = cfg
                .channels
                .wechat
                .iter()
                .filter(|item| !item.is_empty() && item.channel_id() == local_id)
                .cloned()
                .collect::<Vec<_>>();
            single_built_channel(
                channel_runtime::wechat::build_wechat_channels(&settings)?,
                id,
            )
        }
        "discord" => {
            let settings = cfg
                .channels
                .discord
                .iter()
                .filter(|item| !item.is_empty() && item.channel_id() == local_id)
                .cloned()
                .collect::<Vec<_>>();
            single_built_channel(
                channel_runtime::discord::build_discord_channels(&settings, http_client)?,
                id,
            )
        }
        "lark" | "feishu" => {
            let settings = cfg
                .channels
                .lark
                .iter()
                .filter(|item| {
                    !item.is_empty()
                        && item.platform.channel_name() == kind
                        && item.channel_id() == local_id
                })
                .cloned()
                .collect::<Vec<_>>();
            single_built_channel(
                channel_runtime::lark::build_lark_channels(&settings, http_client)?,
                id,
            )
        }
        _ => Err(format!("unsupported channel type '{kind}'").into()),
    }
}

fn single_built_channel(
    channels: HashMap<String, Arc<dyn Channel>>,
    id: &str,
) -> Result<(String, Arc<dyn Channel>), BoxError> {
    let mut channels = channels.into_iter();
    let Some(channel) = channels.next() else {
        return Err(format!("channel '{id}' is not configured").into());
    };
    if channels.next().is_some() {
        return Err(format!("channel '{id}' is configured more than once").into());
    }
    Ok(channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_single_channel_without_target() {
        let rows = vec![ChannelRow {
            id: "wechat:personal".to_string(),
            name: "wechat",
            username: "anda-wechat".to_string(),
        }];

        assert_eq!(
            resolve_channel_targets(&rows, None, false).unwrap(),
            vec!["wechat:personal"]
        );
    }

    #[test]
    fn resolve_channel_by_type_when_unambiguous() {
        let rows = vec![ChannelRow {
            id: "wechat:personal".to_string(),
            name: "wechat",
            username: "anda-wechat".to_string(),
        }];

        assert_eq!(
            resolve_channel_targets(&rows, Some("wechat"), false).unwrap(),
            vec!["wechat:personal"]
        );
    }

    #[test]
    fn reject_ambiguous_channel_target() {
        let rows = vec![
            ChannelRow {
                id: "wechat:personal".to_string(),
                name: "wechat",
                username: "anda-wechat".to_string(),
            },
            ChannelRow {
                id: "telegram:personal".to_string(),
                name: "telegram",
                username: "telegram".to_string(),
            },
        ];

        assert!(resolve_channel_targets(&rows, Some("personal"), false).is_err());
    }
}
