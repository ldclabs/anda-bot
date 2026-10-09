use anda_core::{BoxError, Principal};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{UserRegistry, default_true, normalize_list, normalize_optional};

pub const DEFAULT_TELEGRAM_API_BASE: &str = "https://api.telegram.org";
pub const DEFAULT_DISCORD_API_BASE: &str = "https://discord.com/api/v10";
pub const DEFAULT_WECHAT_API_BASE: &str = weixin_agent::config::DEFAULT_BASE_URL;
pub const DEFAULT_WECHAT_CDN_BASE: &str = weixin_agent::config::DEFAULT_CDN_BASE_URL;
pub const DEFAULT_LARK_API_BASE: &str = "https://open.larksuite.com/open-apis";
pub const DEFAULT_LARK_WS_BASE: &str = "https://open.larksuite.com";
pub const DEFAULT_FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
pub const DEFAULT_FEISHU_WS_BASE: &str = "https://open.feishu.cn";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChannelSettings {
    #[serde(default)]
    pub telegram: Vec<TelegramChannelSettings>,

    #[serde(default)]
    pub wechat: Vec<WechatChannelSettings>,

    #[serde(default)]
    pub discord: Vec<DiscordChannelSettings>,

    #[serde(default)]
    pub lark: Vec<LarkChannelSettings>,
}

impl ChannelSettings {
    pub fn user_refs(&self) -> Vec<String> {
        self.bound_users().map(|(_, user)| user).collect()
    }

    pub fn user_bindings(
        &self,
        users: &UserRegistry,
    ) -> Result<HashMap<String, Principal>, BoxError> {
        self.bound_users()
            .map(|(channel_id, user)| Ok((channel_id, users.resolve(Some(&user))?)))
            .collect()
    }

    /// Channels with a `user`, keyed like `Channel::id()`. A channel without
    /// one is never bound, so empty placeholder entries drop out here too.
    fn bound_users(&self) -> impl Iterator<Item = (String, String)> + '_ {
        let telegram = self
            .telegram
            .iter()
            .map(|c| (format!("telegram:{}", c.channel_id()), &c.user));
        let wechat = self
            .wechat
            .iter()
            .map(|c| (format!("wechat:{}", c.channel_id()), &c.user));
        let discord = self
            .discord
            .iter()
            .map(|c| (format!("discord:{}", c.channel_id()), &c.user));
        let lark = self.lark.iter().map(|c| {
            let channel_id = format!("{}:{}", c.platform.channel_name(), c.channel_id());
            (channel_id, &c.user)
        });
        telegram
            .chain(wechat)
            .chain(discord)
            .chain(lark)
            .filter_map(|(channel_id, user)| Some((channel_id, normalize_optional(user)?)))
    }
}

/// A channel's configured id, or `default` when it has none.
fn channel_id_or_default(id: &Option<String>) -> String {
    normalize_optional(id).unwrap_or_else(|| "default".to_string())
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LarkPlatform {
    #[default]
    Lark,
    Feishu,
}

impl LarkPlatform {
    pub fn api_base(self) -> &'static str {
        match self {
            Self::Lark => DEFAULT_LARK_API_BASE,
            Self::Feishu => DEFAULT_FEISHU_API_BASE,
        }
    }

    pub fn ws_base(self) -> &'static str {
        match self {
            Self::Lark => DEFAULT_LARK_WS_BASE,
            Self::Feishu => DEFAULT_FEISHU_WS_BASE,
        }
    }

    pub fn locale_header(self) -> &'static str {
        match self {
            Self::Lark => "en",
            Self::Feishu => "zh",
        }
    }

    pub fn channel_name(self) -> &'static str {
        match self {
            Self::Lark => "lark",
            Self::Feishu => "feishu",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LarkReceiveMode {
    #[default]
    Websocket,
    Webhook,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LarkChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub user: Option<String>,

    #[serde(default)]
    pub app_id: String,

    #[serde(default)]
    pub app_secret: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub verification_token: Option<String>,

    #[serde(default)]
    pub port: Option<u16>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub allow_external_users: bool,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default)]
    pub platform: LarkPlatform,

    #[serde(default)]
    pub receive_mode: LarkReceiveMode,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,
}

impl Default for LarkChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            user: None,
            app_id: String::new(),
            app_secret: String::new(),
            username: None,
            verification_token: None,
            port: None,
            allowed_users: Vec::new(),
            allow_external_users: false,
            mention_only: false,
            platform: LarkPlatform::default(),
            receive_mode: LarkReceiveMode::default(),
            ack_reactions: true,
        }
    }
}

impl LarkChannelSettings {
    pub fn channel_id(&self) -> String {
        channel_id_or_default(&self.id)
    }

    pub fn is_empty(&self) -> bool {
        normalize_optional(&self.id).is_none()
            && self.app_id.trim().is_empty()
            && normalize_optional(&self.user).is_none()
            && self.app_secret.trim().is_empty()
            && normalize_optional(&self.username).is_none()
            && normalize_optional(&self.verification_token).is_none()
            && self.port.is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.allow_external_users
            && !self.mention_only
            && self.platform == LarkPlatform::default()
            && self.receive_mode == LarkReceiveMode::default()
            && self.ack_reactions
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DiscordChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub user: Option<String>,

    #[serde(default)]
    pub bot_token: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub guild_id: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub allow_external_users: bool,

    #[serde(default)]
    pub listen_to_bots: bool,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,
}

impl Default for DiscordChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            user: None,
            bot_token: String::new(),
            username: None,
            guild_id: None,
            allowed_users: Vec::new(),
            allow_external_users: false,
            listen_to_bots: false,
            mention_only: false,
            ack_reactions: true,
        }
    }
}

impl DiscordChannelSettings {
    pub fn channel_id(&self) -> String {
        channel_id_or_default(&self.id)
    }

    pub fn is_empty(&self) -> bool {
        normalize_optional(&self.id).is_none()
            && self.bot_token.trim().is_empty()
            && normalize_optional(&self.user).is_none()
            && normalize_optional(&self.username).is_none()
            && normalize_optional(&self.guild_id).is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.allow_external_users
            && !self.listen_to_bots
            && !self.mention_only
            && self.ack_reactions
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TelegramChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub user: Option<String>,

    #[serde(default)]
    pub bot_token: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub allow_external_users: bool,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,
}

impl Default for TelegramChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            user: None,
            bot_token: String::new(),
            username: None,
            allowed_users: Vec::new(),
            allow_external_users: false,
            mention_only: false,
            ack_reactions: true,
        }
    }
}

impl TelegramChannelSettings {
    pub fn channel_id(&self) -> String {
        channel_id_or_default(&self.id)
    }

    pub fn is_empty(&self) -> bool {
        normalize_optional(&self.id).is_none()
            && self.bot_token.trim().is_empty()
            && normalize_optional(&self.user).is_none()
            && normalize_optional(&self.username).is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.allow_external_users
            && !self.mention_only
            && self.ack_reactions
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct WechatChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub user: Option<String>,

    #[serde(default)]
    pub bot_token: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub allow_external_users: bool,

    #[serde(default)]
    pub route_tag: Option<u32>,
}

impl WechatChannelSettings {
    pub fn channel_id(&self) -> String {
        channel_id_or_default(&self.id)
    }

    pub fn is_empty(&self) -> bool {
        normalize_optional(&self.id).is_none()
            && self.bot_token.trim().is_empty()
            && normalize_optional(&self.user).is_none()
            && normalize_optional(&self.username).is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.allow_external_users
            && self.route_tag.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lark_platform_maps_to_correct_endpoints_and_headers() {
        assert_eq!(LarkPlatform::Lark.api_base(), DEFAULT_LARK_API_BASE);
        assert_eq!(LarkPlatform::Lark.ws_base(), DEFAULT_LARK_WS_BASE);
        assert_eq!(LarkPlatform::Lark.locale_header(), "en");
        assert_eq!(LarkPlatform::Lark.channel_name(), "lark");

        assert_eq!(LarkPlatform::Feishu.api_base(), DEFAULT_FEISHU_API_BASE);
        assert_eq!(LarkPlatform::Feishu.ws_base(), DEFAULT_FEISHU_WS_BASE);
        assert_eq!(LarkPlatform::Feishu.locale_header(), "zh");
        assert_eq!(LarkPlatform::Feishu.channel_name(), "feishu");
    }

    #[test]
    fn channel_ids_trim_or_fall_back_to_defaults() {
        let lark = LarkChannelSettings {
            id: Some("  work  ".to_string()),
            ..Default::default()
        };
        assert_eq!(lark.channel_id(), "work");
        assert_eq!(LarkChannelSettings::default().channel_id(), "default");

        let discord = DiscordChannelSettings {
            id: Some("  server  ".to_string()),
            ..Default::default()
        };
        assert_eq!(discord.channel_id(), "server");
        assert_eq!(DiscordChannelSettings::default().channel_id(), "default");

        let telegram = TelegramChannelSettings {
            id: Some("  personal  ".to_string()),
            ..Default::default()
        };
        assert_eq!(telegram.channel_id(), "personal");
        assert_eq!(TelegramChannelSettings::default().channel_id(), "default");

        let wechat = WechatChannelSettings {
            id: Some("  wx  ".to_string()),
            ..Default::default()
        };
        assert_eq!(wechat.channel_id(), "wx");
        assert_eq!(WechatChannelSettings::default().channel_id(), "default");
    }

    #[test]
    fn default_channel_settings_are_empty_until_meaningful_fields_are_set() {
        assert!(LarkChannelSettings::default().is_empty());
        assert!(DiscordChannelSettings::default().is_empty());
        assert!(TelegramChannelSettings::default().is_empty());
        assert!(WechatChannelSettings::default().is_empty());

        assert!(
            !LarkChannelSettings {
                app_id: "cli_a".to_string(),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !DiscordChannelSettings {
                listen_to_bots: true,
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !TelegramChannelSettings {
                mention_only: true,
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !WechatChannelSettings {
                route_tag: Some(7),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn serde_defaults_preserve_channel_defaults() {
        let settings: ChannelSettings = serde_json::from_value(json!({
            "lark": [{}],
            "discord": [{}],
            "telegram": [{}],
            "wechat": [{}]
        }))
        .unwrap();

        assert!(settings.lark[0].ack_reactions);
        assert!(settings.discord[0].ack_reactions);
        assert!(settings.telegram[0].ack_reactions);
        assert_eq!(settings.wechat[0].route_tag, None);
    }

    #[test]
    fn user_bindings_cover_all_channel_kinds() {
        use crate::identity::Ed25519Key;
        use ic_auth_types::ByteBufB64;

        let default_key = Ed25519Key::new([1; 32]);
        let teammate = Ed25519Key::new([2; 32]);
        let teammate_ref = ByteBufB64(teammate.pubkey().as_bytes().to_vec()).to_string();

        let settings = ChannelSettings {
            telegram: vec![TelegramChannelSettings {
                id: Some("tg".to_string()),
                user: Some(teammate_ref.clone()),
                bot_token: "token".to_string(),
                ..Default::default()
            }],
            wechat: vec![WechatChannelSettings {
                id: Some("wc".to_string()),
                user: Some(teammate_ref.clone()),
                bot_token: "token".to_string(),
                ..Default::default()
            }],
            discord: vec![DiscordChannelSettings {
                id: Some("dc".to_string()),
                user: Some(teammate_ref.clone()),
                bot_token: "token".to_string(),
                ..Default::default()
            }],
            lark: vec![
                LarkChannelSettings {
                    id: Some("lk".to_string()),
                    user: Some(teammate_ref.clone()),
                    app_id: "app".to_string(),
                    app_secret: "secret".to_string(),
                    ..Default::default()
                },
                LarkChannelSettings {
                    id: Some("fs".to_string()),
                    user: Some(teammate_ref),
                    platform: LarkPlatform::Feishu,
                    app_id: "app".to_string(),
                    app_secret: "secret".to_string(),
                    ..Default::default()
                },
            ],
        };
        let config = crate::config::Config {
            channels: settings.clone(),
            ..Default::default()
        };

        let registry = config.user_registry(default_key.pubkey()).unwrap();
        let bindings = settings.user_bindings(&registry).unwrap();

        let expected = teammate.pubkey().id();
        assert_eq!(bindings.get("telegram:tg"), Some(&expected));
        assert_eq!(bindings.get("wechat:wc"), Some(&expected));
        assert_eq!(bindings.get("discord:dc"), Some(&expected));
        assert_eq!(bindings.get("lark:lk"), Some(&expected));
        assert_eq!(bindings.get("feishu:fs"), Some(&expected));
    }
}
