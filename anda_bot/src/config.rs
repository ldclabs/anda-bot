use anda_core::BoxError;
use anda_hippocampus::types::ModelConfig;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const ANDA_BOT_SPACE_ID: &str = "anda_bot";
pub const CONFIG_FILE_NAME: &str = "config.yaml";
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_GATEWAY_ADDR: &str = "127.0.0.1:8042";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_gateway_addr")]
    pub addr: String,

    #[serde(default)]
    pub sandbox: bool,

    #[serde(default)]
    pub https_proxy: Option<String>,

    #[serde(default)]
    pub model: ModelSettings,

    #[serde(default)]
    pub channels: ChannelSettings,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModelSettings {
    #[serde(default)]
    pub active: String,

    #[serde(default)]
    pub providers: BTreeMap<String, ModelProviderConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModelProviderConfig {
    #[serde(default)]
    pub family: String,

    #[serde(default)]
    pub model: String,

    #[serde(default)]
    pub api_base: String,

    #[serde(default)]
    pub api_key: String,

    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ChannelSettings {
    #[serde(default)]
    pub irc: Vec<IrcChannelSettings>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IrcChannelSettings {
    #[serde(default)]
    pub id: Option<String>,

    #[serde(default)]
    pub server: String,

    #[serde(default = "default_irc_port")]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            addr: default_gateway_addr(),
            sandbox: false,
            https_proxy: None,
            model: ModelSettings::default(),
            channels: ChannelSettings::default(),
        }
    }
}

impl Default for IrcChannelSettings {
    fn default() -> Self {
        Self {
            id: None,
            server: String::new(),
            port: default_irc_port(),
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

fn default_gateway_addr() -> String {
    DEFAULT_GATEWAY_ADDR.to_string()
}

fn default_irc_port() -> u16 {
    6697
}

fn default_true() -> bool {
    true
}

impl Config {
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
        format!("http://{}", self.addr.trim())
    }

    pub fn brain_base_url(&self) -> String {
        format!("http://{}/v1/{}", self.addr.trim(), ANDA_BOT_SPACE_ID)
    }

    pub fn setup_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let active = self.model.active.trim();

        if active.is_empty() {
            issues.push("model.active".to_string());
        } else if let Some(provider) = self.model.providers.get(active) {
            let base = format!("model.providers.{active}");
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
            issues.push(format!("model.providers.{active}"));
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

        issues
    }

    pub fn model_config(&self) -> ModelConfig {
        self.model
            .providers
            .get(self.model.active.trim())
            .map(ModelProviderConfig::to_model_config)
            .unwrap_or_default()
    }
}

impl ModelProviderConfig {
    fn to_model_config(&self) -> ModelConfig {
        ModelConfig {
            family: self.family.trim().to_string(),
            model: self.model.trim().to_string(),
            api_base: self.api_base.trim().to_string(),
            api_key: self.api_key.trim().to_string(),
            disabled: self.disabled,
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
            && self.port == default_irc_port()
            && self.verify_tls
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_contents_read_selected_provider_and_irc_channels() {
        let config = Config::from_contents(
            r##"
addr: 127.0.0.1:9000
sandbox: true
https_proxy: http://127.0.0.1:7890
model:
  active: anthropic
  providers:
    anthropic:
      family: anthropic
      model: claude-sonnet-4-6
      api_base: https://api.anthropic.com/v1
      api_key: sk-test
    openai:
      family: openai
      model: gpt-4.1-mini
      api_base: https://api.openai.com/v1
      api_key: sk-openai
channels:
  irc:
    - id: libera
      server: irc.libera.chat
      port: 6697
      nickname: anda-bot
      username: anda
      channels:
        - "#anda"
        - "#ops"
      allowed_users:
        - alice
        - bob
      server_password: serverpass
      nickserv_password: nickservpass
      sasl_password: saslpass
      verify_tls: false
"##,
        )
        .unwrap();

        let model = config.model_config();
        assert_eq!(config.addr, "127.0.0.1:9000");
        assert!(config.sandbox);
        assert_eq!(config.https_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(config.model.active, "anthropic");
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
        assert!(!config.channels.irc[0].verify_tls);
        assert!(config.setup_issues().is_empty());
    }

    #[test]
    fn setup_issues_report_missing_active_provider_fields() {
        let mut config = Config::default();
        config.model.active = "anthropic".to_string();

        assert_eq!(config.setup_issues(), vec!["model.providers.anthropic"]);

        config.model.providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                family: "anthropic".to_string(),
                ..Default::default()
            },
        );

        assert_eq!(
            config.setup_issues(),
            vec![
                "model.providers.anthropic.model",
                "model.providers.anthropic.api_base",
                "model.providers.anthropic.api_key"
            ]
        );
    }

    #[test]
    fn default_template_contains_setup_guidance() {
        let template = Config::default_template();

        assert!(template.contains("addr:"));
        assert!(template.contains("sandbox:"));
        assert!(template.contains("https_proxy:"));
        assert!(template.contains("model:"));
        assert!(template.contains("channels:"));
        assert!(template.contains("irc:"));
    }
}
