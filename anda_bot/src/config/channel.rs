use anda_core::{BoxError, Principal};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{UserRegistry, default_true, normalize_list, normalize_optional, normalize_string};

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
    pub irc: Vec<IrcChannelSettings>,

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
        let mut refs = Vec::new();
        refs.extend(
            self.irc
                .iter()
                .filter_map(|channel| channel_user_ref(&channel.user)),
        );
        refs.extend(
            self.telegram
                .iter()
                .filter_map(|channel| channel_user_ref(&channel.user)),
        );
        refs.extend(
            self.wechat
                .iter()
                .filter_map(|channel| channel_user_ref(&channel.user)),
        );
        refs.extend(
            self.discord
                .iter()
                .filter_map(|channel| channel_user_ref(&channel.user)),
        );
        refs.extend(
            self.lark
                .iter()
                .filter_map(|channel| channel_user_ref(&channel.user)),
        );
        refs
    }

    pub fn user_bindings(
        &self,
        users: &UserRegistry,
    ) -> Result<HashMap<String, Principal>, BoxError> {
        let mut bindings = HashMap::new();
        for irc in self.irc.iter().filter(|channel| !channel.is_empty()) {
            insert_user_binding(
                &mut bindings,
                format!("irc:{}", irc.channel_id()),
                &irc.user,
                users,
            )?;
        }
        for telegram in self.telegram.iter().filter(|channel| !channel.is_empty()) {
            insert_user_binding(
                &mut bindings,
                format!("telegram:{}", telegram.channel_id()),
                &telegram.user,
                users,
            )?;
        }
        for wechat in self.wechat.iter().filter(|channel| !channel.is_empty()) {
            insert_user_binding(
                &mut bindings,
                format!("wechat:{}", wechat.channel_id()),
                &wechat.user,
                users,
            )?;
        }
        for discord in self.discord.iter().filter(|channel| !channel.is_empty()) {
            insert_user_binding(
                &mut bindings,
                format!("discord:{}", discord.channel_id()),
                &discord.user,
                users,
            )?;
        }
        for lark in self.lark.iter().filter(|channel| !channel.is_empty()) {
            insert_user_binding(
                &mut bindings,
                format!("{}:{}", lark.platform.channel_name(), lark.channel_id()),
                &lark.user,
                users,
            )?;
        }
        Ok(bindings)
    }
}

fn channel_user_ref(user: &Option<String>) -> Option<String> {
    normalize_optional(user)
}

fn insert_user_binding(
    bindings: &mut HashMap<String, Principal>,
    channel_id: String,
    user: &Option<String>,
    users: &UserRegistry,
) -> Result<(), BoxError> {
    if channel_id.ends_with(':') {
        return Ok(());
    }

    if let Some(user) = channel_user_ref(user) {
        bindings.insert(channel_id, users.resolve(Some(&user))?);
    }
    Ok(())
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
        self.id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_string()
    }

    pub fn label(&self, index: usize) -> String {
        let channel_id = self.channel_id();
        if !channel_id.is_empty() {
            channel_id
        } else {
            format!("#{}", index + 1)
        }
    }

    pub fn is_empty(&self) -> bool {
        normalize_string(self.id.as_deref().unwrap_or("")).is_none()
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
        self.id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_string()
    }

    pub fn label(&self, index: usize) -> String {
        let channel_id = self.channel_id();
        if !channel_id.is_empty() {
            channel_id
        } else {
            format!("#{}", index + 1)
        }
    }

    pub fn is_empty(&self) -> bool {
        normalize_string(self.id.as_deref().unwrap_or("")).is_none()
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
        self.id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_string()
    }

    pub fn label(&self, index: usize) -> String {
        let channel_id = self.channel_id();
        if !channel_id.is_empty() {
            channel_id
        } else {
            format!("#{}", index + 1)
        }
    }

    pub fn is_empty(&self) -> bool {
        normalize_string(self.id.as_deref().unwrap_or("")).is_none()
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
        self.id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_string()
    }

    pub fn label(&self, index: usize) -> String {
        let channel_id = self.channel_id();
        if !channel_id.is_empty() {
            channel_id
        } else {
            format!("#{}", index + 1)
        }
    }

    pub fn is_empty(&self) -> bool {
        normalize_string(self.id.as_deref().unwrap_or("")).is_none()
            && self.bot_token.trim().is_empty()
            && normalize_optional(&self.user).is_none()
            && normalize_optional(&self.username).is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.allow_external_users
            && self.route_tag.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IrcChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub user: Option<String>,

    #[serde(default)]
    pub server: String,

    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default)]
    pub nickname: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub channels: Vec<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub allow_external_users: bool,

    #[serde(default)]
    pub server_password: Option<String>,

    #[serde(default)]
    pub nickserv_password: Option<String>,

    #[serde(default)]
    pub sasl_password: Option<String>,

    #[serde(default = "default_true")]
    pub verify_tls: bool,
}

impl Default for IrcChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            user: None,
            server: String::new(),
            port: default_port(),
            nickname: String::new(),
            username: None,
            channels: Vec::new(),
            allowed_users: Vec::new(),
            allow_external_users: false,
            server_password: None,
            nickserv_password: None,
            sasl_password: None,
            verify_tls: true,
        }
    }
}

impl IrcChannelSettings {
    pub fn channel_id(&self) -> String {
        self.id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.server.trim())
            .to_string()
    }

    pub fn label(&self, index: usize) -> String {
        let channel_id = self.channel_id();
        if !channel_id.is_empty() {
            channel_id
        } else {
            format!("#{}", index + 1)
        }
    }

    pub fn is_empty(&self) -> bool {
        normalize_string(self.id.as_deref().unwrap_or("")).is_none()
            && self.server.trim().is_empty()
            && normalize_optional(&self.user).is_none()
            && self.nickname.trim().is_empty()
            && normalize_optional(&self.username).is_none()
            && normalize_list(&self.channels).is_empty()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.allow_external_users
            && normalize_optional(&self.server_password).is_none()
            && normalize_optional(&self.nickserv_password).is_none()
            && normalize_optional(&self.sasl_password).is_none()
            && self.port == default_port()
            && self.verify_tls
    }
}

fn default_port() -> u16 {
    6697
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

        let irc = IrcChannelSettings {
            server: "  irc.libera.chat  ".to_string(),
            ..Default::default()
        };
        assert_eq!(irc.channel_id(), "irc.libera.chat");
        assert_eq!(IrcChannelSettings::default().label(2), "#3");
    }

    #[test]
    fn default_channel_settings_are_empty_until_meaningful_fields_are_set() {
        assert!(LarkChannelSettings::default().is_empty());
        assert!(DiscordChannelSettings::default().is_empty());
        assert!(TelegramChannelSettings::default().is_empty());
        assert!(WechatChannelSettings::default().is_empty());
        assert!(IrcChannelSettings::default().is_empty());

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
        assert!(
            !IrcChannelSettings {
                verify_tls: false,
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn serde_defaults_preserve_ack_and_irc_port_defaults() {
        let settings: ChannelSettings = serde_json::from_value(json!({
            "lark": [{}],
            "discord": [{}],
            "telegram": [{}],
            "irc": [{}],
            "wechat": [{}]
        }))
        .unwrap();

        assert!(settings.lark[0].ack_reactions);
        assert!(settings.discord[0].ack_reactions);
        assert!(settings.telegram[0].ack_reactions);
        assert_eq!(settings.irc[0].port, 6697);
        assert!(settings.irc[0].verify_tls);
        assert_eq!(settings.wechat[0].route_tag, None);
    }
}
