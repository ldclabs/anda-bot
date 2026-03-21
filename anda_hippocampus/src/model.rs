use anda_engine::{
    model::{Model, reqwest},
    model::{anthropic, deepseek, gemini, mimo, openai},
};
use std::sync::Arc;

use crate::types::ModelConfig;

/// Builds a model instance based on the provided configuration
pub fn build_model(http_client: reqwest::Client, cfg: ModelConfig) -> Model {
    if cfg.disabled {
        return Model::not_implemented();
    }

    match cfg.family.as_str() {
        "gemini" => Model::with_completer(Arc::new(
            gemini::Client::new(&cfg.api_key, Some(cfg.api_base))
                .with_client(http_client)
                .completion_model(&cfg.model),
        )),
        "anthropic" => Model::with_completer(Arc::new(
            anthropic::Client::new(&cfg.api_key, Some(cfg.api_base))
                .with_client(http_client)
                .completion_model(&cfg.model),
        )),
        "openai" => Model::with_completer(Arc::new(
            openai::Client::new(&cfg.api_key, Some(cfg.api_base))
                .with_client(http_client)
                .completion_model_v2(&cfg.model),
        )),
        "deepseek" => Model::with_completer(Arc::new(
            deepseek::Client::new(&cfg.api_key, Some(cfg.api_base))
                .with_client(http_client)
                .completion_model(&cfg.model),
        )),
        "mimo" => Model::with_completer(Arc::new(
            mimo::Client::new(&cfg.api_key, Some(cfg.api_base))
                .with_client(http_client)
                .completion_model(&cfg.model),
        )),
        _ => Model::not_implemented(),
    }
}
