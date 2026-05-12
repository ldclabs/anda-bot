use anda_core::BoxError;
use anda_engine::model::Models;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    path::{Path, PathBuf},
};

mod channel;
mod model;
mod transcription;
mod tts;

pub use channel::*;
pub use model::*;
pub use transcription::*;
pub use tts::*;

pub const ANDA_BOT_SPACE_ID: &str = "anda_bot";
pub const CONFIG_FILE_NAME: &str = "config.yaml";
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_GATEWAY_ADDR: &str = "127.0.0.1:8042";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_gateway_addr")]
    pub addr: String,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default)]
    pub https_proxy: Option<String>,

    #[serde(default)]
    pub model: ModelSettings,

    #[serde(default)]
    pub tts: TtsConfig,

    #[serde(default)]
    pub transcription: TranscriptionConfig,

    #[serde(default)]
    pub channels: ChannelSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            addr: default_gateway_addr(),
            log_level: default_log_level(),
            https_proxy: None,
            model: ModelSettings::default(),
            channels: ChannelSettings::default(),
            tts: TtsConfig::default(),
            transcription: TranscriptionConfig::default(),
        }
    }
}

impl Config {
    pub fn socket_addr(&self) -> Result<SocketAddr, BoxError> {
        Ok(self.addr.trim().parse()?)
    }

    pub fn file_path(home: &Path) -> PathBuf {
        home.join(CONFIG_FILE_NAME)
    }

    pub fn default_template() -> &'static str {
        include_str!("../assets/config.yaml")
    }

    pub async fn from_file(path: &Path) -> Result<Self, BoxError> {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err.into()),
        };

        Self::from_contents(&content)
    }

    pub fn from_contents(content: &str) -> Result<Self, BoxError> {
        if content.trim().is_empty() {
            return Ok(Self::default());
        }

        Ok(serde_saphyr::from_str(content)?)
    }

    pub fn base_url(&self) -> String {
        match self.socket_addr() {
            Ok(addr) if addr.is_ipv6() => format!("http://[::1]:{}", addr.port()),
            Ok(addr) => format!("http://127.0.0.1:{}", addr.port()),
            Err(_) => format!("http://{}", self.addr.trim()),
        }
    }

    pub fn brain_base_url(&self) -> String {
        format!("{}/v1/{}", self.base_url(), ANDA_BOT_SPACE_ID)
    }

    pub fn setup_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let active = self.model.active.trim();
        let model_providers = self.model.providers_with_env_api_keys();

        if active.is_empty() {
            issues.push("model.active".to_string());
        } else if let Some(provider) = model_providers.iter().find(|m| m.model == active) {
            let pos = model_providers
                .iter()
                .position(|m| m.model == active)
                .unwrap();
            let base = format!("model.providers[{pos}]");
            if provider.disabled {
                issues.push(format!("{base}.disabled"));
            }
            if provider.family.trim().is_empty() {
                issues.push(format!("{base}.family"));
            }
            if provider.model.trim().is_empty() {
                issues.push(format!("{base}.model"));
            }
            if provider.api_base.trim().is_empty() {
                issues.push(format!("{base}.api_base"));
            }
            if provider.api_key.trim().is_empty() {
                issues.push(format!("{base}.api_key"));
            }
        } else {
            issues.push(format!("model.providers: no {active}"));
        }

        let mut seen_ids = BTreeSet::new();
        for (index, irc) in self.channels.irc.iter().enumerate() {
            if irc.is_empty() {
                continue;
            }

            let base = format!("channels.irc[{index}]");
            if irc.server.trim().is_empty() {
                issues.push(format!("{base}.server"));
            }
            if irc.nickname.trim().is_empty() {
                issues.push(format!("{base}.nickname"));
            }

            let channel_id = irc.channel_id();
            if !channel_id.is_empty() && !seen_ids.insert(channel_id) {
                issues.push(format!("{base}.id"));
            }
        }

        let mut seen_telegram_ids = BTreeSet::new();
        for (index, telegram) in self.channels.telegram.iter().enumerate() {
            if telegram.is_empty() {
                continue;
            }

            let base = format!("channels.telegram[{index}]");
            if telegram.bot_token.trim().is_empty() {
                issues.push(format!("{base}.bot_token"));
            }

            let channel_id = telegram.channel_id();
            if !channel_id.is_empty() && !seen_telegram_ids.insert(channel_id) {
                issues.push(format!("{base}.id"));
            }
        }

        let mut seen_wechat_ids = BTreeSet::new();
        for (index, wechat) in self.channels.wechat.iter().enumerate() {
            if wechat.is_empty() {
                continue;
            }

            let base = format!("channels.wechat[{index}]");
            let channel_id = wechat.channel_id();
            if !channel_id.is_empty() && !seen_wechat_ids.insert(channel_id) {
                issues.push(format!("{base}.id"));
            }
        }

        let mut seen_discord_ids = BTreeSet::new();
        for (index, discord) in self.channels.discord.iter().enumerate() {
            if discord.is_empty() {
                continue;
            }

            let base = format!("channels.discord[{index}]");
            if discord.bot_token.trim().is_empty() {
                issues.push(format!("{base}.bot_token"));
            }

            let channel_id = discord.channel_id();
            if !channel_id.is_empty() && !seen_discord_ids.insert(channel_id) {
                issues.push(format!("{base}.id"));
            }
        }

        let mut seen_lark_ids = BTreeSet::new();
        for (index, lark) in self.channels.lark.iter().enumerate() {
            if lark.is_empty() {
                continue;
            }

            let base = format!("channels.lark[{index}]");
            if lark.app_id.trim().is_empty() {
                issues.push(format!("{base}.app_id"));
            }
            if lark.app_secret.trim().is_empty() {
                issues.push(format!("{base}.app_secret"));
            }
            if lark.receive_mode == LarkReceiveMode::Webhook && lark.port.is_none() {
                issues.push(format!("{base}.port"));
            }

            let channel_id = lark.channel_id();
            if !channel_id.is_empty() && !seen_lark_ids.insert(channel_id) {
                issues.push(format!("{base}.id"));
            }
        }

        issues
    }

    pub fn models(&self, http_client: reqwest::Client) -> Models {
        let providers = self.model.providers_with_env_api_keys();
        let models = Models::from_configs(&providers, http_client.clone());

        let active = self.model.active.trim();
        if let Some(model) = models.get(active) {
            models.set_model(model);
        }

        models
    }
}

fn default_gateway_addr() -> String {
    DEFAULT_GATEWAY_ADDR.to_string()
}

fn default_log_level() -> String {
    "warn".to_string()
}

pub fn default_true() -> bool {
    true
}

pub fn normalize_string(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub fn normalize_optional(raw: &Option<String>) -> Option<String> {
    raw.as_deref().and_then(normalize_string)
}

pub fn normalize_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| normalize_string(value))
        .collect()
}

pub fn normalize_identity(value: &str) -> String {
    value.trim().trim_start_matches('@').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_engine::model::ModelConfig;

    #[test]
    fn config_contents_read_selected_provider_and_irc_channels() {
        let config = Config::from_contents(
            r##"
addr: 127.0.0.1:9000
https_proxy: http://127.0.0.1:7890
model:
  active: claude-sonnet-4-6
  providers:
    - family: anthropic
      model: claude-sonnet-4-6
      api_base: https://api.anthropic.com/v1
      api_key: sk-test
    - family: openai
      model: gpt-4.1-mini
      api_base: https://api.openai.com/v1
      api_key: sk-openai
channels:
 irc: [{id: libera, server: irc.libera.chat, port: 6697, nickname: anda-bot, username: anda, channels: ["#anda", "#ops"], allowed_users: [alice, bob], allow_external_users: true, server_password: serverpass, nickserv_password: nickservpass, sasl_password: saslpass, verify_tls: false}]
 telegram: [{id: personal, bot_token: "123456:ABC", username: anda_bot, allowed_users: [alice, "123456789"], allow_external_users: true, mention_only: true, api_base: https://api.telegram.org, ack_reactions: false}]
 wechat: [{id: personal, bot_token: "wx-token", username: anda-wechat, allowed_users: [wx_alice], allow_external_users: true, base_url: https://ilinkai.weixin.qq.com/, cdn_base_url: https://novac2c.cdn.weixin.qq.com/c2c, route_tag: 42}]
 discord: [{id: server, bot_token: "discord-token", username: anda-discord, guild_id: "987654321", allowed_users: ["111", "222"], allow_external_users: true, listen_to_bots: true, mention_only: true, api_base: https://discord.com/api/v10, ack_reactions: false}]
 lark: [{id: work, app_id: cli_a, app_secret: secret, username: anda-lark, verification_token: verify, port: 8090, allowed_users: [ou_alice], allow_external_users: true, mention_only: true, platform: feishu, receive_mode: webhook, api_base: https://open.feishu.cn/open-apis, ws_base: https://open.feishu.cn, ack_reactions: false}]
"##,
        )
        .unwrap();

        assert_eq!(config.addr, "127.0.0.1:9000");
        assert_eq!(config.https_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(config.model.active, "claude-sonnet-4-6");
        let model: ModelConfig = config.model.providers[0].clone();

        assert_eq!(model.family, "anthropic");
        assert_eq!(model.model, "claude-sonnet-4-6");
        assert_eq!(model.api_base, "https://api.anthropic.com/v1");
        assert_eq!(model.api_key, "sk-test");

        assert_eq!(config.channels.irc.len(), 1);
        assert_eq!(config.channels.irc[0].id.as_deref(), Some("libera"));
        assert_eq!(config.channels.irc[0].server, "irc.libera.chat");
        assert_eq!(config.channels.irc[0].nickname, "anda-bot");
        assert_eq!(config.channels.irc[0].username.as_deref(), Some("anda"));
        assert_eq!(config.channels.irc[0].channels, vec!["#anda", "#ops"]);
        assert_eq!(config.channels.irc[0].allowed_users, vec!["alice", "bob"]);
        assert!(config.channels.irc[0].allow_external_users);
        assert!(!config.channels.irc[0].verify_tls);
        assert_eq!(config.channels.telegram.len(), 1);
        assert_eq!(config.channels.telegram[0].id.as_deref(), Some("personal"));
        assert_eq!(config.channels.telegram[0].bot_token, "123456:ABC");
        assert_eq!(
            config.channels.telegram[0].username.as_deref(),
            Some("anda_bot")
        );
        assert_eq!(
            config.channels.telegram[0].allowed_users,
            vec!["alice", "123456789"]
        );
        assert!(config.channels.telegram[0].allow_external_users);
        assert!(config.channels.telegram[0].mention_only);
        assert!(!config.channels.telegram[0].ack_reactions);
        assert_eq!(config.channels.wechat.len(), 1);
        assert_eq!(config.channels.wechat[0].id.as_deref(), Some("personal"));
        assert_eq!(config.channels.wechat[0].bot_token, "wx-token");
        assert_eq!(
            config.channels.wechat[0].username.as_deref(),
            Some("anda-wechat")
        );
        assert_eq!(config.channels.wechat[0].allowed_users, vec!["wx_alice"]);
        assert!(config.channels.wechat[0].allow_external_users);
        assert_eq!(config.channels.wechat[0].route_tag, Some(42));
        assert_eq!(config.channels.discord.len(), 1);
        assert_eq!(config.channels.discord[0].id.as_deref(), Some("server"));
        assert_eq!(config.channels.discord[0].bot_token, "discord-token");
        assert_eq!(
            config.channels.discord[0].username.as_deref(),
            Some("anda-discord")
        );
        assert_eq!(
            config.channels.discord[0].guild_id.as_deref(),
            Some("987654321")
        );
        assert_eq!(config.channels.discord[0].allowed_users, vec!["111", "222"]);
        assert!(config.channels.discord[0].allow_external_users);
        assert!(config.channels.discord[0].listen_to_bots);
        assert!(config.channels.discord[0].mention_only);
        assert!(!config.channels.discord[0].ack_reactions);
        assert_eq!(config.channels.lark.len(), 1);
        assert_eq!(config.channels.lark[0].id.as_deref(), Some("work"));
        assert_eq!(config.channels.lark[0].app_id, "cli_a");
        assert_eq!(config.channels.lark[0].app_secret, "secret");
        assert_eq!(
            config.channels.lark[0].username.as_deref(),
            Some("anda-lark")
        );
        assert_eq!(
            config.channels.lark[0].verification_token.as_deref(),
            Some("verify")
        );
        assert_eq!(config.channels.lark[0].port, Some(8090));
        assert_eq!(config.channels.lark[0].allowed_users, vec!["ou_alice"]);
        assert!(config.channels.lark[0].allow_external_users);
        assert!(config.channels.lark[0].mention_only);
        assert_eq!(config.channels.lark[0].platform, LarkPlatform::Feishu);
        assert_eq!(
            config.channels.lark[0].receive_mode,
            LarkReceiveMode::Webhook
        );
        assert!(!config.channels.lark[0].ack_reactions);
        println!("Config: {:#?}", config.setup_issues());
        assert!(config.setup_issues().is_empty());
    }

    #[test]
    fn setup_issues_report_missing_active_provider_fields() {
        let _env = model::guard_model_api_key_env();
        let mut config = Config::default();
        config.model.active = "deepseek-v4-pro".to_string();

        assert_eq!(
            config.setup_issues(),
            vec!["model.providers: no deepseek-v4-pro"]
        );

        config.model.providers.push(ModelConfig {
            family: "anthropic".to_string(),
            model: "deepseek-v4-pro".to_string(),
            ..Default::default()
        });

        assert_eq!(
            config.setup_issues(),
            vec!["model.providers[0].api_base", "model.providers[0].api_key"]
        );
    }

    #[test]
    fn setup_issues_accepts_api_key_from_environment() {
        let _env = model::guard_model_api_key_env();
        unsafe { std::env::set_var("DEEPSEEK_API_KEY", "sk-env") };

        let mut config = Config::default();
        config.model.active = "deepseek-v4-pro".to_string();
        config.model.providers.push(ModelConfig {
            family: "anthropic".to_string(),
            model: "deepseek-v4-pro".to_string(),
            api_base: "https://api.deepseek.com/anthropic".to_string(),
            ..Default::default()
        });

        assert!(config.setup_issues().is_empty());
    }

    #[test]
    fn default_template_contains_setup_guidance() {
        let template = Config::default_template();

        assert!(template.contains("addr:"));
        assert!(template.contains("https_proxy:"));
        assert!(template.contains("model:"));
        assert!(template.contains("OPENAI_API_KEY"));
        assert!(template.contains("ANTHROPIC_API_KEY"));
        assert!(template.contains("GEMINI_API_KEY"));
        assert!(template.contains("GOOGLE_API_KEY"));
        assert!(template.contains("channels:"));
        assert!(template.contains("irc:"));
        assert!(template.contains("wechat:"));
        assert!(template.contains("discord:"));
        assert!(template.contains("lark:"));
    }
}
