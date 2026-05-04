pub mod discord;
pub mod irc;
pub mod lark;
pub mod telegram;
pub mod wechat;

mod attachments;
mod runtime;
mod types;

use anda_core::BoxError;
use reqwest::Client;
use std::{collections::HashMap, sync::Arc};

pub use attachments::*;
pub use runtime::*;
pub use types::*;

use crate::config;

pub fn build_channels(
    cfg: &config::ChannelSettings,
    client: Client,
) -> Result<HashMap<String, Arc<dyn Channel>>, BoxError> {
    let mut channels = irc::build_irc_channels(&cfg.irc)?;

    for (channel_id, channel) in telegram::build_telegram_channels(&cfg.telegram, client.clone())? {
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate channel id '{channel_id}'").into());
        }
    }

    for (channel_id, channel) in wechat::build_wechat_channels(&cfg.wechat)? {
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate channel id '{channel_id}'").into());
        }
    }

    for (channel_id, channel) in discord::build_discord_channels(&cfg.discord, client.clone())? {
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate channel id '{channel_id}'").into());
        }
    }

    for (channel_id, channel) in lark::build_lark_channels(&cfg.lark, client)? {
        if channels.insert(channel_id.clone(), channel).is_some() {
            return Err(format!("duplicate channel id '{channel_id}'").into());
        }
    }

    Ok(channels)
}
