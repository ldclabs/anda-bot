//! Per-channel context tokens. Load legacy files once; persist token and age together.
use super::*;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const STATE_FILE: &str = "context_tokens_v2.json";

#[derive(Clone, Deserialize, Serialize)]
struct Entry {
    token: String,
    updated_at: u64,
}

pub(super) struct ContextTokens {
    workspace: Arc<ChannelWorkspace>,
    entries: Mutex<Option<HashMap<String, Entry>>>,
}

impl ContextTokens {
    pub fn new(workspace: Arc<ChannelWorkspace>) -> Self {
        Self {
            workspace,
            entries: Mutex::new(None),
        }
    }

    async fn load(&self) -> HashMap<String, Entry> {
        let Some(root) = self.workspace.path() else {
            return HashMap::new();
        };
        if let Ok(data) = read_text_file(root.join(STATE_FILE)).await {
            match serde_json::from_str(&data) {
                Ok(entries) => return entries,
                Err(err) => log::warn!("invalid WeChat context token state: {err}"),
            }
        }
        let tokens: HashMap<String, String> = read_text_file(root.join("context_tokens.json"))
            .await
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let meta: HashMap<String, u64> = read_text_file(root.join("context_tokens_meta.json"))
            .await
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let modified = tokio::fs::metadata(root.join("context_tokens.json"))
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let entries = tokens
            .into_iter()
            .map(|(id, token)| {
                let updated_at = meta.get(&id).copied().unwrap_or(modified);
                (id, Entry { token, updated_at })
            })
            .collect();
        self.persist(&entries).await;
        entries
    }

    async fn persist(&self, entries: &HashMap<String, Entry>) {
        let Some(root) = self.workspace.path() else {
            return;
        };
        let result: Result<(), BoxError> = async {
            tokio::fs::create_dir_all(&root).await?;
            let path = root.join("context_tokens_v2.json.tmp");
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&path).await?;
            use tokio::io::AsyncWriteExt;
            file.write_all(&serde_json::to_vec(entries)?).await?;
            file.flush().await?;
            drop(file);
            tokio::fs::rename(path, root.join(STATE_FILE)).await?;
            Ok(())
        }
        .await;
        if let Err(err) = result {
            log::warn!("failed to save WeChat context tokens: {err}");
        }
    }

    pub async fn get(&self, user: &str) -> Option<String> {
        let mut guard = self.entries.lock().await;
        if guard.is_none() {
            *guard = Some(self.load().await);
        }
        let entries = guard.as_mut().unwrap();
        let entry = entries.get(user.trim())?;
        if context_token_is_stale(Some(entry.updated_at)) {
            entries.remove(user.trim());
            self.persist(entries).await;
            None
        } else {
            Some(entry.token.clone())
        }
    }

    pub async fn put(&self, user: &str, token: &str) {
        let (user, token) = (user.trim(), token.trim());
        if user.is_empty() || token.is_empty() {
            return;
        }
        let mut guard = self.entries.lock().await;
        if guard.is_none() {
            *guard = Some(self.load().await);
        }
        let entries = guard.as_mut().unwrap();
        entries.insert(
            user.to_owned(),
            Entry {
                token: token.to_owned(),
                updated_at: unix_ms(),
            },
        );
        self.persist(entries).await;
    }

    pub async fn remove(&self, user: &str, expected: &str) {
        let mut guard = self.entries.lock().await;
        if guard.is_none() {
            *guard = Some(self.load().await);
        }
        let entries = guard.as_mut().unwrap();
        if entries
            .get(user.trim())
            .is_some_and(|e| e.token == expected)
        {
            entries.remove(user.trim());
            self.persist(entries).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrate_merge_expire_and_reopen_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_owned());
        tokio::fs::write(
            dir.path().join("context_tokens.json"),
            r#"{"alice":"old","bob":"stale"}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            dir.path().join("context_tokens_meta.json"),
            serde_json::json!({"alice":unix_ms(),"bob":1}).to_string(),
        )
        .await
        .unwrap();
        let store = ContextTokens::new(workspace.clone());
        assert_eq!(store.get("alice").await.as_deref(), Some("old"));
        assert_eq!(store.get("bob").await, None);
        tokio::join!(store.put("alice", "new"), store.put("carol", "c"));
        store.remove("alice", "old").await;
        let reopened = ContextTokens::new(workspace);
        assert_eq!(reopened.get("alice").await.as_deref(), Some("new"));
        assert_eq!(reopened.get("carol").await.as_deref(), Some("c"));
        assert_eq!(reopened.get("bob").await, None);
        reopened.remove("alice", "new").await;
        assert_eq!(reopened.get("alice").await, None);
    }

    #[tokio::test]
    async fn unchanged_token_refreshes_age_in_memory_and_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Arc::new(ChannelWorkspace::default());
        workspace.set_path(dir.path().to_owned());
        let store = ContextTokens::new(workspace.clone());
        store.put("alice", "same").await;
        store
            .entries
            .lock()
            .await
            .as_mut()
            .unwrap()
            .get_mut("alice")
            .unwrap()
            .updated_at = 1;
        store.put("alice", "same").await;
        assert_eq!(
            ContextTokens::new(workspace).get("alice").await.as_deref(),
            Some("same")
        );
    }
}
