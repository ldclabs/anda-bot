use anda_engine::model::ModelConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::util::text::read_text_file_sync;

pub use crate::provider_env::CODEX_API_BASE;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModelSettings {
    #[serde(default)]
    pub active: String,

    #[serde(default)]
    pub providers: Vec<ModelConfig>,
}

impl ModelSettings {
    pub fn try_load_codex_token(&mut self, home: &Path) {
        for provider in &mut self.providers {
            if provider.api_key.trim().is_empty() && Self::uses_codex_auth(provider) {
                let token_path = home.join(".codex/auth.json");
                if let Ok(token_str) = read_text_file_sync(token_path)
                    && let Ok(token) = serde_json::from_str::<CodexAuth>(&token_str)
                    && !token.tokens.access_token.is_empty()
                {
                    provider.api_key = token.tokens.access_token;
                }
            }
        }
    }

    pub fn uses_codex_auth(provider: &ModelConfig) -> bool {
        provider.api_base.trim() == CODEX_API_BASE
    }

    pub fn providers_with_env_api_keys(&self) -> Vec<ModelConfig> {
        self.providers
            .iter()
            .map(provider_with_env_api_key)
            .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CodexAuth {
    #[allow(unused)]
    #[serde(default)]
    pub auth_mode: String,

    #[serde(default)]
    pub tokens: OAuthToken,

    #[allow(unused)]
    #[serde(default)]
    pub last_refresh: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct OAuthToken {
    #[serde(default)]
    pub id_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub account_id: String,
}

fn provider_with_env_api_key(provider: &ModelConfig) -> ModelConfig {
    let mut provider = provider.clone();
    if provider.api_key.trim().is_empty()
        && let Some(api_key) =
            crate::provider_env::env_api_key(&provider.family, &provider.model, &provider.api_base)
    {
        provider.api_key = api_key;
    }

    provider
}

#[cfg(test)]
pub(crate) const MODEL_API_KEY_ENV_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "BIGMODEL_API_KEY",
    "DASHSCOPE_API_KEY",
    "DEEPSEEK_API_KEY",
    "GEMINI_API_KEY",
    "GLM_API_KEY",
    "GOOGLE_API_KEY",
    "GROQ_API_KEY",
    "KIMI_API_KEY",
    "MIMO_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAXI_API_KEY",
    "MOONSHOT_API_KEY",
    "OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "QWEN_API_KEY",
    "SILICONFLOW_API_KEY",
    "XIAOMI_MIMO_API_KEY",
    "ZHIPUAI_API_KEY",
];

#[cfg(test)]
pub(crate) struct ModelApiKeyEnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for ModelApiKeyEnvGuard {
    fn drop(&mut self) {
        for &name in MODEL_API_KEY_ENV_VARS {
            unsafe { std::env::remove_var(name) };
        }
        for (name, value) in &self.saved {
            if let Some(value) = value {
                unsafe { std::env::set_var(name, value) };
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn guard_model_api_key_env() -> ModelApiKeyEnvGuard {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let lock = LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap();
    let saved = MODEL_API_KEY_ENV_VARS
        .iter()
        .map(|&name| (name, std::env::var_os(name)))
        .collect();

    for &name in MODEL_API_KEY_ENV_VARS {
        unsafe { std::env::remove_var(name) };
    }

    ModelApiKeyEnvGuard { saved, _lock: lock }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_api_key_prefers_config_value() {
        let _env = guard_model_api_key_env();
        unsafe { std::env::set_var("DEEPSEEK_API_KEY", "from-env") };

        let settings = ModelSettings {
            active: "deepseek-v4-pro".to_string(),
            providers: vec![ModelConfig {
                family: "anthropic".to_string(),
                model: "deepseek-v4-pro".to_string(),
                api_base: "https://api.deepseek.com/anthropic".to_string(),
                api_key: "from-config".to_string(),
                ..Default::default()
            }],
        };

        assert_eq!(
            settings.providers_with_env_api_keys()[0].api_key,
            "from-config"
        );
    }

    #[test]
    fn provider_api_key_reads_known_provider_env() {
        let _env = guard_model_api_key_env();
        unsafe { std::env::set_var("DEEPSEEK_API_KEY", "from-env") };

        let settings = ModelSettings {
            active: "deepseek-v4-pro".to_string(),
            providers: vec![ModelConfig {
                family: "anthropic".to_string(),
                model: "deepseek-v4-pro".to_string(),
                api_base: "https://api.deepseek.com/anthropic".to_string(),
                ..Default::default()
            }],
        };

        assert_eq!(
            settings.providers_with_env_api_keys()[0].api_key,
            "from-env"
        );
    }

    #[test]
    fn compatible_endpoint_does_not_fall_back_to_family_env() {
        let _env = guard_model_api_key_env();
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", "wrong-provider") };

        let settings = ModelSettings {
            active: "deepseek-v4-pro".to_string(),
            providers: vec![ModelConfig {
                family: "anthropic".to_string(),
                model: "deepseek-v4-pro".to_string(),
                api_base: "https://api.deepseek.com/anthropic".to_string(),
                ..Default::default()
            }],
        };

        assert!(settings.providers_with_env_api_keys()[0].api_key.is_empty());
    }

    #[test]
    fn gemini_provider_accepts_google_api_key_alias() {
        let _env = guard_model_api_key_env();
        unsafe { std::env::set_var("GOOGLE_API_KEY", "from-google") };

        let settings = ModelSettings {
            active: "gemini-flash-latest".to_string(),
            providers: vec![ModelConfig {
                family: "gemini".to_string(),
                model: "gemini-flash-latest".to_string(),
                api_base: "https://generativelanguage.googleapis.com/v1beta/models".to_string(),
                ..Default::default()
            }],
        };

        assert_eq!(
            settings.providers_with_env_api_keys()[0].api_key,
            "from-google"
        );
    }

    #[test]
    fn codex_provider_loads_api_key_from_auth_json() {
        let home = tempfile::tempdir().unwrap();
        let codex_dir = home.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("auth.json"),
            r#"{"tokens":{"access_token":"codex-token"}}"#,
        )
        .unwrap();

        let mut settings = ModelSettings {
            active: "gpt-5.5".to_string(),
            providers: vec![ModelConfig {
                family: "openai".to_string(),
                model: "gpt-5.5".to_string(),
                api_base: CODEX_API_BASE.to_string(),
                api_key: String::new(),
                ..Default::default()
            }],
        };

        settings.try_load_codex_token(home.path());

        assert_eq!(settings.providers[0].api_key, "codex-token");
        assert!(ModelSettings::uses_codex_auth(&settings.providers[0]));
    }

    #[test]
    fn api_key_env_candidates_cover_known_brands() {
        fn candidates(family: &str, model: &str, api_base: &str) -> Vec<&'static str> {
            crate::provider_env::api_key_env_candidates(family, model, api_base)
        }

        assert_eq!(
            candidates("", "deepseek-v3", "https://api.deepseek.com/v1"),
            vec!["DEEPSEEK_API_KEY"]
        );
        assert_eq!(
            candidates("", "minimax-01", "https://api.minimaxi.com/v1"),
            vec!["MINIMAX_API_KEY", "MINIMAXI_API_KEY"]
        );
        assert_eq!(
            candidates("", "mimo-v2", "https://api.xiaomimimo.com/v1"),
            vec!["MIMO_API_KEY", "XIAOMI_MIMO_API_KEY"]
        );
        assert_eq!(
            candidates("", "kimi-k2", "https://api.moonshot.cn/v1"),
            vec!["MOONSHOT_API_KEY", "KIMI_API_KEY"]
        );
        assert_eq!(
            candidates("", "glm-5", "https://open.bigmodel.cn/api"),
            vec!["BIGMODEL_API_KEY", "ZHIPUAI_API_KEY", "GLM_API_KEY"]
        );
        assert_eq!(
            candidates("", "any", "https://openrouter.ai/api/v1"),
            vec!["OPENROUTER_API_KEY"]
        );
        assert_eq!(
            candidates("", "any", "https://api.groq.com/openai/v1"),
            vec!["GROQ_API_KEY"]
        );
        assert_eq!(
            candidates("", "any", "https://api.siliconflow.cn/v1"),
            vec!["SILICONFLOW_API_KEY"]
        );
        assert_eq!(
            candidates("", "qwen-max", "https://dashscope.aliyuncs.com/api"),
            vec!["DASHSCOPE_API_KEY", "QWEN_API_KEY"]
        );
        assert_eq!(
            candidates("", "any", "https://api.anthropic.com"),
            vec!["ANTHROPIC_API_KEY"]
        );
        assert_eq!(
            candidates("", "any", "https://api.openai.com/v1"),
            vec!["OPENAI_API_KEY"]
        );
        assert_eq!(
            candidates("", "gemini-3", "https://generativelanguage.googleapis.com"),
            vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"]
        );

        // Unrecognized endpoints fall back to the provider family.
        assert_eq!(
            candidates("anthropic", "custom", "https://llm.internal"),
            vec!["ANTHROPIC_API_KEY"]
        );
        assert_eq!(
            candidates("openai", "custom", "https://llm.internal"),
            vec!["OPENAI_API_KEY"]
        );
        assert_eq!(
            candidates("google", "custom", "https://llm.internal"),
            vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"]
        );
        assert!(candidates("unknown", "custom", "https://llm.internal").is_empty());
    }
}
