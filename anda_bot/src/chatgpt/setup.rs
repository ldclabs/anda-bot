//! Minimal authenticated setup gateway. Never starts Brain, IM or cron.
use super::api::ChatGptApi;
use anda_core::BoxError;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use serde_json::json;

pub async fn serve(
    api: ChatGptApi,
    addr: std::net::SocketAddr,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<bool, BoxError> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let complete = api.setup_complete.clone();
    let app=Router::new()
        .route("/",get(||async{Json(json!({"name":crate::config::APP_NAME,"version":crate::config::APP_VERSION,"needs_setup":true}))}))
        .route("/daemon/status",get(||async{Json(json!({"conversations":0,"memory_nodes":0,"memory_links":0,"needs_setup":true}))}))
        .route("/daemon/config",get(config).put(update_config))
        .route("/daemon/shutdown",axum::routing::post(shutdown))
        .with_state((api.clone(),cancel.clone()))
        .merge(api.router());
    let exit = cancel.clone();
    let ready = complete.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {_=exit.cancelled()=>{},_=ready.cancelled()=>{}}
        })
        .await?;
    Ok(complete.is_cancelled() && !cancel.is_cancelled())
}
fn authorized(api: &ChatGptApi, headers: &HeaderMap) -> bool {
    api.auth
        .verify_user(headers, anda_engine::unix_ms(), None, None)
        .ok()
        == Some(api.owner)
}
async fn config(
    State((api, _)): State<(ChatGptApi, tokio_util::sync::CancellationToken)>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !authorized(&api, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = crate::config::Config::file_path(&api.home);
    let content = match read_config(&path).await {
        Ok(content) => content.unwrap_or_else(|| crate::config::Config::default_template().into()),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let config: serde_json::Value = serde_saphyr::from_str(&content).unwrap_or_default();
    let setup_issues = crate::config::Config::from_contents(&content)
        .map(|config| config.setup_issues())
        .unwrap_or_else(|_| vec!["config".into()]);
    Json(json!({"path":path,"revision":revision(&content),"content":content,"config":config,"needs_setup":true,"setup_issues":setup_issues})).into_response()
}
async fn read_config(path: &std::path::Path) -> std::io::Result<Option<String>> {
    match crate::util::text::read_text_file(path).await {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}
#[derive(Deserialize)]
struct ConfigUpdate {
    content: String,
    expected_revision: Option<String>,
}
async fn update_config(
    State((api, _)): State<(ChatGptApi, tokio_util::sync::CancellationToken)>,
    headers: HeaderMap,
    Json(update): Json<ConfigUpdate>,
) -> axum::response::Response {
    if !authorized(&api, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let content = crate::engine::normalize_config_file_content(update.content);
    let _guard = api.config_lock.lock().await;
    let path = crate::config::Config::file_path(&api.home);
    let original = match read_config(&path).await {
        Ok(v) => v,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let current = original
        .as_deref()
        .unwrap_or(crate::config::Config::default_template());
    if update
        .expected_revision
        .as_ref()
        .is_some_and(|r| r != &revision(current))
    {
        return (
            StatusCode::CONFLICT,
            "Configuration changed; reload before saving",
        )
            .into_response();
    }
    let config = match crate::config::Config::from_contents(&content) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let backup = match &original {
        Some(original) if original != &content => {
            super::store::atomic_write(&path.with_extension("yaml.setup.bak"), original.as_bytes())
        }
        _ => Ok(()),
    };
    if let Err(e) = backup.and_then(|_| super::store::atomic_write(&path, content.as_bytes())) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    if config.setup_issues().is_empty() {
        api.setup_complete.cancel();
    }
    let setup_issues = config.setup_issues();
    Json(json!({"path":path,"revision":revision(&content),"content":content,"config":config,"needs_setup":!setup_issues.is_empty(),"setup_issues":setup_issues})).into_response()
}
async fn shutdown(
    State((api, cancel)): State<(ChatGptApi, tokio_util::sync::CancellationToken)>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !authorized(&api, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    cancel.cancel();
    Json(json!({"ok":true})).into_response()
}
// The same revision as the full daemon and Anda Desktop's offline editor.
fn revision(content: &str) -> String {
    crate::engine::daemon_config_revision(content)
}
