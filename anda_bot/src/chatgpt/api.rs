//! Owner-only control surface; also mounted before the model runtime exists.
use super::ChatGptService;
use anda_core::{BoxError, Principal};
use anda_engine_server::handler::AppState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ChatGptApi {
    pub service: Arc<ChatGptService>,
    pub home: PathBuf,
    pub auth: AppState,
    pub owner: Principal,
    pub config_lock: Arc<Mutex<()>>,
    pub runtime: Option<crate::engine::RuntimeModels>,
    pub setup_complete: CancellationToken,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Accounts,
    TransferExport {
        profile_id: String,
        path: PathBuf,
    },
    TransferImport {
        path: PathBuf,
    },
    LoginStart {
        #[serde(default)]
        profile_id: Option<String>,
        #[serde(default)]
        port: u16,
        #[serde(default)]
        consent: bool,
    },
    LoginStatus {
        flow_id: String,
    },
    LoginCancel {
        flow_id: String,
    },
    AccountSelect {
        profile_id: String,
    },
    Logout {
        profile_id: String,
    },
    Models {
        profile_id: String,
    },
    ModelSelect {
        profile_id: String,
        model: String,
    },
}
impl ChatGptApi {
    pub fn router(self) -> Router {
        Router::new()
            .route("/daemon/chatgpt", post(handle))
            .layer(DefaultBodyLimit::max(16 * 1024))
            .with_state(self)
    }
    pub async fn execute(&self, request: Request) -> Result<Value, BoxError> {
        Ok(match request {
            Request::TransferExport { profile_id, path } => {
                self.service.export_session(&profile_id, &path).await?;
                json!({"exported":true,"source_disconnected":true})
            }
            Request::TransferImport { path } => {
                json!({"profile_id":self.service.import_session(&path).await?})
            }
            Request::Accounts => {
                let mut result = serde_json::to_value(self.service.accounts().await)?;
                result["needs_setup"] = json!(self.runtime.is_none());
                result
            }
            Request::LoginStart {
                profile_id,
                port,
                consent,
            } => serde_json::to_value(self.service.start_login(profile_id, port, consent).await?)?,
            Request::LoginStatus { flow_id } => {
                serde_json::to_value(self.service.login_status(&flow_id).await?)?
            }
            Request::LoginCancel { flow_id } => {
                serde_json::to_value(self.service.cancel_login(&flow_id).await?)?
            }
            Request::AccountSelect { profile_id } => {
                let catalog = self.service.models(&profile_id).await?;
                // Return to the model this account was last configured with, if still offered.
                let model = self
                    .saved_model(&profile_id)
                    .await
                    .filter(|saved| catalog.iter().any(|m| &m.slug == saved))
                    .or_else(|| catalog.first().map(|m| m.slug.clone()))
                    .ok_or("No models are available for this ChatGPT account")?;
                self.select_model(profile_id, model).await?
            }
            Request::Logout { profile_id } => self.service.logout(&profile_id).await?,
            Request::Models { profile_id } => {
                json!({"models":self.service.models(&profile_id).await?})
            }
            Request::ModelSelect { profile_id, model } => {
                if !self
                    .service
                    .models(&profile_id)
                    .await?
                    .iter()
                    .any(|m| m.slug == model)
                {
                    return Err(
                        "This model is not available to the selected ChatGPT account".into(),
                    );
                }
                self.select_model(profile_id, model).await?
            }
        })
    }
    async fn saved_model(&self, profile_id: &str) -> Option<String> {
        let path = crate::config::Config::file_path(&self.home);
        let content = crate::util::text::read_text_file(&path).await.ok()?;
        let config = crate::config::Config::from_contents(&content).ok()?;
        let mut saved: Vec<_> = config
            .model
            .providers
            .into_iter()
            .filter(|p| {
                matches!(&p.auth, crate::config::ModelAuth::Chatgpt { profile } if profile == profile_id)
            })
            .collect();
        saved.sort_by_key(|p| p.disabled);
        saved.into_iter().next().map(|p| p.model)
    }
    async fn select_model(&self, profile_id: String, model: String) -> Result<Value, BoxError> {
        if let Some(runtime) = &self.runtime {
            runtime.check_plan_switch(true)?;
        }
        let _guard = self.config_lock.lock().await;
        let path = crate::config::Config::file_path(&self.home);
        let original = crate::util::text::read_text_file(&path).await?;
        crate::config::Config::from_contents(&original)?;
        let mut config: Value = serde_saphyr::from_str(&original)?;
        let mut provider = crate::config::ModelProvider {
            family: "openai-response".into(),
            model,
            api_base: super::API_BASE.into(),
            auth: crate::config::ModelAuth::Chatgpt {
                profile: profile_id.clone(),
            },
            stream: true,
            ..Default::default()
        };
        let id = provider.selection_id();
        if config.get("model").is_none() {
            config["model"] = json!({});
        }
        if config["model"].get("providers").is_none() {
            config["model"]["providers"] = json!([]);
        }
        let providers = config["model"]["providers"]
            .as_array_mut()
            .ok_or("model.providers must be an array")?;
        if let Some(existing) = providers.iter_mut().find(|p| {
            p.get("model").and_then(Value::as_str) == Some(&provider.model)
                && p.pointer("/auth/profile").and_then(Value::as_str) == Some(&profile_id)
        }) {
            // Keep existing labels and budgets when selecting a saved provider.
            provider = serde_json::from_value(existing.clone())?;
            provider.disabled = false;
            *existing = serde_json::to_value(&provider)?;
        } else {
            providers.push(serde_json::to_value(provider)?);
        }
        config["model"]["active"] = json!(id);
        let content = serde_saphyr::to_string(&config)?;
        let parsed = crate::config::Config::from_contents(&content)?;
        if !parsed.setup_issues().is_empty() {
            return Err(format!(
                "Other configuration needs attention: {}",
                parsed.setup_issues().join(", ")
            )
            .into());
        }
        let backup = path.with_extension(format!("yaml.chatgpt-{}.bak", super::random_id()));
        super::store::atomic_write(&backup, original.as_bytes())?;
        super::store::atomic_write(&path, content.as_bytes())?;
        self.service.select(&profile_id).await?;
        let mut result = json!({"active_model":id,"restart_required":false});
        if let Some(runtime) = &self.runtime {
            // The selection is already saved; report a failed reload like config saves do.
            if let Err(error) = runtime.reload_from_config().await {
                log::warn!("failed to reload models after ChatGPT model selection: {error}");
                result["models_error"] = json!(error.to_string());
            }
        } else {
            self.setup_complete.cancel();
        }
        Ok(result)
    }
}
async fn handle(
    State(api): State<ChatGptApi>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> impl IntoResponse {
    if api
        .auth
        .verify_user(&headers, anda_engine::unix_ms(), None, None)
        .ok()
        != Some(api.owner)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Only the local owner may manage ChatGPT accounts"})),
        )
            .into_response();
    }
    if matches!(
        request,
        Request::TransferExport { .. } | Request::TransferImport { .. }
    ) && headers.contains_key("origin")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Use the local CLI for session transfers"})),
        )
            .into_response();
    }
    match api.execute(request).await {
        Ok(result) => (
            StatusCode::OK,
            [("cache-control", "no-store")],
            Json(result),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}
