pub mod discord;
pub mod lark;
pub mod telegram;
pub mod wechat;

mod attachments;
mod runtime;
mod tools;
mod types;

use anda_core::BoxError;
use reqwest::Client;
use std::{collections::HashMap, sync::Arc};

pub use attachments::*;
pub use runtime::*;
pub use tools::*;
pub use types::*;

use crate::config;

pub fn build_channels(
    cfg: &config::ChannelSettings,
    client: Client,
) -> Result<HashMap<String, Arc<dyn Channel>>, BoxError> {
    let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();

    let mut register = |built: HashMap<String, Arc<dyn Channel>>| -> Result<(), BoxError> {
        for (channel_id, channel) in built {
            if channels.insert(channel_id.clone(), channel).is_some() {
                return Err(format!("duplicate channel id '{channel_id}'").into());
            }
        }
        Ok(())
    };

    register(telegram::build_telegram_channels(
        &cfg.telegram,
        client.clone(),
    )?)?;
    register(wechat::build_wechat_channels(&cfg.wechat)?)?;
    register(discord::build_discord_channels(
        &cfg.discord,
        client.clone(),
    )?)?;
    register(lark::build_lark_channels(&cfg.lark, client)?)?;

    Ok(channels)
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn build_channels_with_empty_settings_yields_no_channels() {
        let channels = build_channels(&config::ChannelSettings::default(), Client::new()).unwrap();
        assert!(channels.is_empty());
    }
}
