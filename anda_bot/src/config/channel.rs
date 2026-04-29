use serde::{Deserialize, Serialize};

use super::{default_true, normalize_list, normalize_optional, normalize_string};

pub const DEFAULT_TELEGRAM_API_BASE: &str = "https://api.telegram.org";
pub const DEFAULT_DISCORD_API_BASE: &str = "https://discord.com/api/v10";
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
    pub discord: Vec<DiscordChannelSettings>,

    #[serde(default)]
    pub lark: Vec<LarkChannelSettings>,
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
    pub mention_only: bool,

    #[serde(default)]
    pub platform: LarkPlatform,

    #[serde(default)]
    pub receive_mode: LarkReceiveMode,

    #[serde(default)]
    pub api_base: Option<String>,

    #[serde(default)]
    pub ws_base: Option<String>,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,
}

impl Default for LarkChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            app_id: String::new(),
            app_secret: String::new(),
            username: None,
            verification_token: None,
            port: None,
            allowed_users: Vec::new(),
            mention_only: false,
            platform: LarkPlatform::default(),
            receive_mode: LarkReceiveMode::default(),
            api_base: None,
            ws_base: None,
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
            && self.app_secret.trim().is_empty()
            && normalize_optional(&self.username).is_none()
            && normalize_optional(&self.verification_token).is_none()
            && self.port.is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.mention_only
            && self.platform == LarkPlatform::default()
            && self.receive_mode == LarkReceiveMode::default()
            && normalize_optional(&self.api_base).is_none()
            && normalize_optional(&self.ws_base).is_none()
            && self.ack_reactions
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DiscordChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub bot_token: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub guild_id: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub listen_to_bots: bool,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default = "default_discord_api_base")]
    pub api_base: String,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,
}

impl Default for DiscordChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            bot_token: String::new(),
            username: None,
            guild_id: None,
            allowed_users: Vec::new(),
            listen_to_bots: false,
            mention_only: false,
            api_base: default_discord_api_base(),
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
            && normalize_optional(&self.username).is_none()
            && normalize_optional(&self.guild_id).is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.listen_to_bots
            && !self.mention_only
            && self.api_base.trim() == DEFAULT_DISCORD_API_BASE
            && self.ack_reactions
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TelegramChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub bot_token: String,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub allowed_users: Vec<String>,

    #[serde(default)]
    pub mention_only: bool,

    #[serde(default = "default_telegram_api_base")]
    pub api_base: String,

    #[serde(default = "default_true")]
    pub ack_reactions: bool,
}

impl Default for TelegramChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            bot_token: String::new(),
            username: None,
            allowed_users: Vec::new(),
            mention_only: false,
            api_base: default_telegram_api_base(),
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
            && normalize_optional(&self.username).is_none()
            && normalize_list(&self.allowed_users).is_empty()
            && !self.mention_only
            && self.api_base.trim() == DEFAULT_TELEGRAM_API_BASE
            && self.ack_reactions
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IrcChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

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
            server: String::new(),
            port: default_port(),
            nickname: String::new(),
            username: None,
            channels: Vec::new(),
            allowed_users: Vec::new(),
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
            && self.nickname.trim().is_empty()
            && normalize_optional(&self.username).is_none()
            && normalize_list(&self.channels).is_empty()
            && normalize_list(&self.allowed_users).is_empty()
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

fn default_telegram_api_base() -> String {
    DEFAULT_TELEGRAM_API_BASE.to_string()
}

fn default_discord_api_base() -> String {
    DEFAULT_DISCORD_API_BASE.to_string()
}
