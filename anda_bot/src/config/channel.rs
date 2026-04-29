use serde::{Deserialize, Serialize};

use super::{default_true, normalize_list, normalize_optional, normalize_string};

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
