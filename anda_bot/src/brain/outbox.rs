//! Terminal replies use the same save-before-send contract as the browser.
use super::{AttentionItem, AttentionResponse, ResponseReceipt};
use anda_core::BoxError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Pending {
    version: u32,
    scope: String,
    item: String,
    response: AttentionResponse,
}

pub async fn reply(
    client: &crate::gateway::Client,
    home: &Path,
    item: &AttentionItem,
    text: String,
) -> Result<ResponseReceipt, BoxError> {
    if text.trim().is_empty() || text.len() > 8192 {
        return Err("Response must contain 1..8192 bytes of text".into());
    }
    let (scope, path) = pending_path(client, home, &item.id).await?;
    let event_key = ic_auth_types::Xid::new().to_string();
    let response = if item.clarification.is_some() {
        AttentionResponse::Clarification {
            event_key,
            answer: text.clone(),
        }
    } else {
        AttentionResponse::AgentStatement {
            event_key,
            statement: text.clone(),
        }
    };
    let pending = Pending {
        version: 1,
        scope: scope.clone(),
        item: item.id.clone(),
        response,
    };
    let write = super::setup::create_exact(&path, &serde_json::to_vec(&pending)?).await;
    // A concurrent terminal may have created the same item first. Its event
    // identity wins; changed text is never silently substituted.
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 32_768 {
        return Err("Invalid pending response file".into());
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| write.err().unwrap_or_else(|| error.into()))?;
    let saved: Pending = serde_json::from_slice(&bytes)?;
    let (clarification, saved_text) = match &saved.response {
        AttentionResponse::Clarification { answer, .. } => (true, answer),
        AttentionResponse::AgentStatement { statement, .. } => (false, statement),
    };
    if saved.version != 1
        || saved.scope != scope
        || saved.item != item.id
        || clarification != item.clarification.is_some()
        || saved_text != &text
    {
        return Err("A pending response exists with different text. Retry its original text before changing it.".into());
    }
    let receipt = client.brain().respond(&item.id, &saved.response).await?;
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(receipt)
}

async fn pending_path(
    client: &crate::gateway::Client,
    home: &Path,
    item: &str,
) -> Result<(String, PathBuf), BoxError> {
    let caller = client
        .memory_overview()
        .await?
        .caller
        .ok_or("Verified memory identity unavailable")?;
    let scope = anda_cognitive_nexus::content_digest(
        &serde_json::json!({"gateway":client.base_url(),"caller":caller,"space":"anda_bot"}),
    )?;
    let key =
        anda_cognitive_nexus::content_digest(&serde_json::json!({"scope":scope,"item":item}))?;
    let directory = home.join("memory-inbox-outbox");
    tokio::fs::create_dir_all(&directory).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok((scope, directory.join(format!("{}.json", &key[7..]))))
}

pub async fn retry(
    client: &crate::gateway::Client,
    home: &Path,
    item: &AttentionItem,
) -> Result<ResponseReceipt, BoxError> {
    let (scope, path) = pending_path(client, home, &item.id).await?;
    let meta = tokio::fs::symlink_metadata(&path).await?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32_768 {
        return Err("Invalid pending response file".into());
    }
    let pending: Pending = serde_json::from_slice(&tokio::fs::read(path).await?)?;
    if pending.version != 1 || pending.scope != scope || pending.item != item.id {
        return Err("Invalid pending response identity".into());
    }
    let text = match pending.response {
        AttentionResponse::Clarification { answer, .. } => answer,
        AttentionResponse::AgentStatement { statement, .. } => statement,
    };
    reply(client, home, item, text).await
}

pub fn render(page: &super::AttentionPage) -> String {
    let mut text = "Attention inbox / 记忆待办\n".to_string();
    for (index, item) in page.items.iter().enumerate() {
        text.push_str(&format!(
            "\n{}. {} [{}]\n",
            index + 1,
            item.summary,
            item.state
        ));
        if let Some(value) = item.clarification.as_ref().or(item.delivery.as_ref())
            && let Some(question) = item_text(value)
        {
            text.push_str(question);
            text.push('\n');
        }
    }
    if page.items.is_empty() {
        text.push_str("\nNo visible items / 暂无可见事项\n");
    }
    text.push_str("\n/memory answer <number> <text> · /memory retry <number> · /memory inbox\nAnswers are data, not execution permission. / 回答只提供信息，不授予执行权限。\n");
    text
}
fn item_text(value: &serde_json::Value) -> Option<&str> {
    let mut current = value;
    for _ in 0..4 {
        if let Some(text) = current
            .get("question")
            .or_else(|| current.get("message"))
            .and_then(serde_json::Value::as_str)
        {
            return Some(text);
        }
        current = current.get("payload")?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use serde_json::json;
    use std::sync::Arc;
    #[tokio::test]
    async fn memory_outbox_recovers_the_same_event_and_text_after_a_lost_response() {
        let calls = Arc::new(parking_lot::Mutex::new(Vec::<serde_json::Value>::new()));
        let log = calls.clone();
        let app=Router::new().route("/daemon/memory/v1/overview",get(||async {Json(json!({"result":{"schema_version":1,"caller":"owner","observed_at":0,"memory":{"state":"reachable","formation_active":false,"maintenance_active":false,"reason":null},"inbox":{"state":"available","visible_items":1,"inventory_complete":true,"reason":null},"capabilities":{}}}))}))
            .route("/v1/anda_bot/attention/{id}/responses",post(move |Json(value):Json<serde_json::Value>| {let log=log.clone();async move {
                let mut calls=log.lock();calls.push(value);
                if calls.len()==1 {Json(json!({"error":{"code":503,"message":"ack lost"}}))}
                else {Json(json!({"result":{"receipt_id":"receipt-one","status":"acknowledged","evidence_ref":null}}))}
            }}));
        let url = crate::test_support::spawn_http_mock(app).await;
        let client = crate::gateway::Client::new(url, "owner-token".into());
        let home = tempfile::tempdir().unwrap();
        let item:AttentionItem=serde_json::from_value(json!({"id":"a".repeat(64),"wake_ref":"wake","watch_ref":"C-1","fire_activity_ref":"X-1","summary":"Question","state":"completed","clarification":{"question":"Preference?"}})).unwrap();
        assert!(
            reply(&client, home.path(), &item, "Original answer".into())
                .await
                .is_err()
        );
        assert!(
            reply(&client, home.path(), &item, "Changed answer".into())
                .await
                .is_err()
        );
        assert_eq!(calls.lock().len(), 1);
        assert_eq!(
            retry(&client, home.path(), &item).await.unwrap().status,
            "acknowledged"
        );
        let calls = calls.lock();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], calls[1]);
    }
}
