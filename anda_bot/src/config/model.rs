use anda_engine::model::ModelConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModelSettings {
    #[serde(default)]
    pub active: String,

    #[serde(default)]
    pub providers: Vec<ModelConfig>,
}
