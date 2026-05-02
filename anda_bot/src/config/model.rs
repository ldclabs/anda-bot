use anda_engine::model::ModelConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub context_window: usize,

    #[serde(default)]
    pub max_output: usize,

    #[serde(default)]
    pub labels: Vec<String>,

    #[serde(default)]
    pub disabled: bool,

    #[serde(default)]
    pub bearer_auth: bool,
}

impl From<&ModelProviderConfig> for ModelConfig {
    fn from(provider: &ModelProviderConfig) -> Self {
        ModelConfig {
            family: provider.family.trim().to_string(),
            model: provider.model.trim().to_string(),
            api_base: provider.api_base.trim().to_string(),
            api_key: provider.api_key.trim().to_string(),
            context_window: provider.context_window,
            max_output: provider.max_output,
            labels: provider.labels.clone(),
            disabled: provider.disabled,
            bearer_auth: provider.bearer_auth,
        }
    }
}
