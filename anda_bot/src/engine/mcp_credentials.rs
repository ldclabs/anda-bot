//! Persistent storage for MCP OAuth credentials.
//!
//! mcp.json only records *that* a server authenticates via OAuth
//! (`transport.oauth`); the secrets — the dynamically registered client id and
//! the access/refresh tokens — live here, one JSON file per server under
//! `mcp_credentials/` in ANDA_HOME. With both halves present the daemon
//! reconnects an OAuth server after a restart without any human in the loop:
//! the engine rebuilds its authorization from the stored refresh token.

use anda_core::BoxError;
use anda_engine::extension::mcp::{McpCredentialStore, StoredCredentials};
use async_trait::async_trait;
use rmcp::transport::auth::CredentialRefreshGuard;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use super::write_daemon_config_atomically;
use crate::util::fs::restrict_secret_dir_permissions;

/// Directory name under ANDA_HOME holding per-server credential files.
pub const MCP_CREDENTIALS_DIR_NAME: &str = "mcp_credentials";

/// [`McpCredentialStore`] backed by one owner-only JSON file per server.
///
/// Writes go through a unique temp file + rename so a crash mid-save never
/// leaves a torn credential file, and the OAuth token rotation (each refresh
/// replaces the refresh token) cannot lose the only working copy.
pub struct FileMcpCredentialStore {
    dir: PathBuf,
    /// One lock per server, held by the engine across a whole refresh, code
    /// exchange, or sign-out. Without it a refresh that loaded the old grant
    /// could save it back over a newer one, or over a sign-out.
    refresh_locks: parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl FileMcpCredentialStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            refresh_locks: Default::default(),
        }
    }

    fn path_for(&self, server_id: &str) -> Result<PathBuf, BoxError> {
        Ok(self
            .dir
            .join(format!("{}.json", credential_file_stem(server_id)?)))
    }
}

/// Maps a server id to a safe file stem. Server ids are already validated as
/// tool-name parts by the engine, so this is defense in depth against path
/// separators or dot-only names reaching the filesystem layer.
fn credential_file_stem(server_id: &str) -> Result<String, BoxError> {
    let stem: String = server_id
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() || stem.chars().all(|ch| ch == '.') {
        return Err(format!("MCP server id {server_id:?} cannot name a credential file").into());
    }
    Ok(stem)
}

#[async_trait]
impl McpCredentialStore for FileMcpCredentialStore {
    async fn load(&self, server_id: &str) -> Result<Option<StoredCredentials>, BoxError> {
        let path = self.path_for(server_id)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    async fn save(&self, server_id: &str, credentials: StoredCredentials) -> Result<(), BoxError> {
        let path = self.path_for(server_id)?;
        tokio::fs::create_dir_all(&self.dir).await?;
        restrict_secret_dir_permissions(&self.dir)?;
        let json = serde_json::to_vec_pretty(&credentials)?;
        write_daemon_config_atomically(&path, &json).await
    }

    async fn clear(&self, server_id: &str) -> Result<(), BoxError> {
        let path = self.path_for(server_id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    async fn acquire_refresh_guard(
        &self,
        server_id: &str,
    ) -> Result<Option<CredentialRefreshGuard>, BoxError> {
        let lock = self
            .refresh_locks
            .lock()
            .entry(server_id.to_string())
            .or_default()
            .clone();
        Ok(Some(CredentialRefreshGuard::new(lock.lock_owned().await)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials(client_id: &str) -> StoredCredentials {
        StoredCredentials::new(
            client_id.to_string(),
            None,
            vec!["events:read".to_string()],
            Some(1_234_567),
        )
    }

    #[tokio::test]
    async fn load_returns_none_before_any_save() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileMcpCredentialStore::new(dir.path().join("creds"));
        assert!(store.load("alink").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_load_clear_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileMcpCredentialStore::new(dir.path().join("creds"));

        store.save("alink", credentials("oac_1")).await.unwrap();
        let loaded = store.load("alink").await.unwrap().expect("saved");
        assert_eq!(loaded.client_id, "oac_1");
        assert_eq!(loaded.granted_scopes, vec!["events:read".to_string()]);

        // Save replaces the previous value (token rotation).
        store.save("alink", credentials("oac_2")).await.unwrap();
        let loaded = store.load("alink").await.unwrap().expect("saved");
        assert_eq!(loaded.client_id, "oac_2");

        store.clear("alink").await.unwrap();
        assert!(store.load("alink").await.unwrap().is_none());
        // Clearing again is a no-op, not an error.
        store.clear("alink").await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn saved_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let creds_dir = dir.path().join("creds");
        let store = FileMcpCredentialStore::new(creds_dir.clone());
        store.save("alink", credentials("oac_1")).await.unwrap();

        let dir_mode = std::fs::metadata(&creds_dir).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o077, 0, "dir must have no group/other bits");
        let file_mode = std::fs::metadata(creds_dir.join("alink.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(file_mode & 0o077, 0, "file must have no group/other bits");
    }

    #[tokio::test]
    async fn server_ids_are_sanitized_into_file_stems() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileMcpCredentialStore::new(dir.path().join("creds"));
        store
            .save("api.al.ink/mcp", credentials("oac_1"))
            .await
            .unwrap();
        assert!(store.load("api.al.ink/mcp").await.unwrap().is_some());
        assert!(
            dir.path()
                .join("creds")
                .join("api.al.ink_mcp.json")
                .exists()
        );
        assert!(store.load("..").await.is_err());
    }

    #[tokio::test]
    async fn refresh_guard_serializes_writers_per_server() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileMcpCredentialStore::new(dir.path().join("creds"));

        let guard = store.acquire_refresh_guard("alink").await.unwrap();
        assert!(guard.is_some());
        // Another server is not blocked by it.
        let _other = store.acquire_refresh_guard("github").await.unwrap();
        // The same server is, until the first guard is released.
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(50),
                store.acquire_refresh_guard("alink")
            )
            .await
            .is_err()
        );
        drop(guard);
        assert!(
            store
                .acquire_refresh_guard("alink")
                .await
                .unwrap()
                .is_some()
        );
    }
}
