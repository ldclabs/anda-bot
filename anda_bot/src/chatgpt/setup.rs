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
    match crate::util::text::read_text_file(&crate::config::Config::file_path(&api.home)).await {
        Ok(content) => {
            let config: serde_json::Value = serde_saphyr::from_str(&content).unwrap_or_default();
            Json(json!({"path":crate::config::Config::file_path(&api.home),"revision":revision(&content),"content":content,"config":config,"needs_setup":true})).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
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
    let _guard = api.config_lock.lock().await;
    let path = crate::config::Config::file_path(&api.home);
    let original = match crate::util::text::read_text_file(&path).await {
        Ok(v) => v,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if update
        .expected_revision
        .as_ref()
        .is_some_and(|r| r != &revision(&original))
    {
        return (
            StatusCode::CONFLICT,
            "Configuration changed; reload before saving",
        )
            .into_response();
    }
    let config = match crate::config::Config::from_contents(&update.content) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    if let Err(e) =
        super::store::atomic_write(&path.with_extension("yaml.setup.bak"), original.as_bytes())
            .and_then(|_| super::store::atomic_write(&path, update.content.as_bytes()))
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    if config.setup_issues().is_empty() {
        api.setup_complete.cancel();
    }
    Json(json!({"path":path,"content":update.content,"revision":revision(&update.content),"config":config,"needs_setup":!config.setup_issues().is_empty()})).into_response()
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
fn revision(content: &str) -> String {
    use sha3::Digest;
    sha3::Sha3_256::digest(content.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
