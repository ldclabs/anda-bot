use anda_engine::model::ModelConfig;
use serde::{Deserialize, Serialize};

use super::normalize_string;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModelSettings {
    #[serde(default)]
    pub active: String,

    #[serde(default)]
    pub providers: Vec<ModelConfig>,
}

impl ModelSettings {
    pub fn providers_with_env_api_keys(&self) -> Vec<ModelConfig> {
        self.providers
            .iter()
            .map(provider_with_env_api_key)
            .collect()
    }
}

fn provider_with_env_api_key(provider: &ModelConfig) -> ModelConfig {
    let mut provider = provider.clone();
    if provider.api_key.trim().is_empty()
        && let Some(api_key) = env_api_key_for_provider(&provider)
    {
        provider.api_key = api_key;
    }
    provider
}

fn env_api_key_for_provider(provider: &ModelConfig) -> Option<String> {
    api_key_env_candidates(provider)
        .into_iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .and_then(|value| normalize_string(&value))
        })
}

fn api_key_env_candidates(provider: &ModelConfig) -> Vec<&'static str> {
    let family = provider.family.trim().to_ascii_lowercase();
    let model = provider.model.trim().to_ascii_lowercase();
    let api_base = provider.api_base.trim().to_ascii_lowercase();
    let mut candidates = Vec::new();

    if api_base.contains("deepseek") || model.contains("deepseek") {
        push_candidate(&mut candidates, "DEEPSEEK_API_KEY");
    } else if api_base.contains("minimaxi") || model.contains("minimax") {
        push_candidate(&mut candidates, "MINIMAX_API_KEY");
        push_candidate(&mut candidates, "MINIMAXI_API_KEY");
    } else if api_base.contains("xiaomimimo") || model.contains("mimo") {
        push_candidate(&mut candidates, "MIMO_API_KEY");
        push_candidate(&mut candidates, "XIAOMI_MIMO_API_KEY");
    } else if api_base.contains("moonshot") || model.contains("kimi") {
        push_candidate(&mut candidates, "MOONSHOT_API_KEY");
        push_candidate(&mut candidates, "KIMI_API_KEY");
    } else if api_base.contains("bigmodel") || model.contains("glm") {
        push_candidate(&mut candidates, "BIGMODEL_API_KEY");
        push_candidate(&mut candidates, "ZHIPUAI_API_KEY");
        push_candidate(&mut candidates, "GLM_API_KEY");
    } else if api_base.contains("openrouter") {
        push_candidate(&mut candidates, "OPENROUTER_API_KEY");
    } else if api_base.contains("groq") {
        push_candidate(&mut candidates, "GROQ_API_KEY");
    } else if api_base.contains("siliconflow") {
        push_candidate(&mut candidates, "SILICONFLOW_API_KEY");
    } else if api_base.contains("dashscope") || model.contains("qwen") {
        push_candidate(&mut candidates, "DASHSCOPE_API_KEY");
        push_candidate(&mut candidates, "QWEN_API_KEY");
    } else if api_base.contains("anthropic.com") {
        push_candidate(&mut candidates, "ANTHROPIC_API_KEY");
    } else if api_base.contains("openai.com") {
        push_candidate(&mut candidates, "OPENAI_API_KEY");
    } else if api_base.contains("googleapis.com") || model.contains("gemini") {
        push_candidate(&mut candidates, "GEMINI_API_KEY");
        push_candidate(&mut candidates, "GOOGLE_API_KEY");
    }

    if candidates.is_empty() {
        match family.as_str() {
            "anthropic" => push_candidate(&mut candidates, "ANTHROPIC_API_KEY"),
            "openai" => push_candidate(&mut candidates, "OPENAI_API_KEY"),
            "gemini" | "google" => {
                push_candidate(&mut candidates, "GEMINI_API_KEY");
                push_candidate(&mut candidates, "GOOGLE_API_KEY");
            }
            _ => {}
        }
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<&'static str>, name: &'static str) {
    if !candidates.contains(&name) {
        candidates.push(name);
    }
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
}
