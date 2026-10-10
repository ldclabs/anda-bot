//! mcp_state.json: what Anda records about each server that mcp.json does not
//! say: where it came from, how its connection last went, the tool
//! definitions the owner reviewed, and how often its tools were called. Only
//! the daemon writes it, so it never competes with edits to mcp.json.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::engine::write_daemon_config_atomically;

pub(crate) const MCP_STATE_FILE_NAME: &str = "mcp_state.json";
const STATE_VERSION: u32 = 1;

/// Where a server came from.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpSource {
    /// Found in mcp.json: written by hand, or before Anda recorded sources.
    #[default]
    File,
    /// Added by the owner through the app or the CLI.
    Manual,
    /// Added by the agent, with the owner's approval.
    Model,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct McpErrorRecord {
    pub at: u64,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct McpServerState {
    #[serde(default)]
    pub source: McpSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ready_at: Option<u64>,
    /// The last failure, cleared when the server connects again. The engine
    /// reports only a status, so this is where the reason is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<McpErrorRecord>,
    /// When the server's tools were first pinned. Until then the first
    /// catalog it serves is taken as reviewed (trust on first use).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<u64>,
    /// The reviewed definition of each tool, by remote name. A tool whose
    /// definition differs, or that has none, needs review.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, McpToolPin>,
    #[serde(default, skip_serializing_if = "McpUsage::is_empty")]
    pub usage: McpUsage,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct McpToolPin {
    pub digest: String,
    /// What the digest covers, kept to show what changed.
    pub definition: Value,
    pub reviewed_at: u64,
}

/// Calls the agent made to a server's tools.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct McpUsage {
    #[serde(default)]
    pub calls: u64,
    /// Calls that failed or returned an error result.
    #[serde(default)]
    pub errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

impl McpUsage {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Default, Deserialize, Serialize)]
struct McpStateFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    servers: BTreeMap<String, McpServerState>,
}

pub(crate) struct McpStateStore {
    path: PathBuf,
    servers: parking_lot::Mutex<BTreeMap<String, McpServerState>>,
    /// Serializes saves, so an older snapshot never lands after a newer one.
    save_lock: tokio::sync::Mutex<()>,
    /// Set by changes left for the next [`McpStateStore::flush`].
    unsaved: AtomicBool,
}

impl McpStateStore {
    /// Loads the state at `path`. A missing or unreadable file starts empty:
    /// nothing in it is needed to run a server.
    pub async fn open(path: PathBuf) -> Self {
        let servers = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<McpStateFile>(&bytes)
                .map(|file| file.servers)
                .unwrap_or_else(|err| {
                    log::warn!("{} was not loaded: {err}", path.display());
                    BTreeMap::new()
                }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(err) => {
                log::warn!("{} was not loaded: {err}", path.display());
                BTreeMap::new()
            }
        };
        Self {
            path,
            servers: parking_lot::Mutex::new(servers),
            save_lock: tokio::sync::Mutex::new(()),
            unsaved: AtomicBool::new(false),
        }
    }

    pub fn get(&self, id: &str) -> McpServerState {
        self.servers.lock().get(id).cloned().unwrap_or_default()
    }

    /// Reads the record of `id` in place, for callers that need only part of
    /// it: a record holds every reviewed tool definition.
    pub fn read<T>(&self, id: &str, read: impl FnOnce(Option<&McpServerState>) -> T) -> T {
        read(self.servers.lock().get(id))
    }

    /// Changes the record of `id`, and returns whether anything changed.
    pub fn update(&self, id: &str, change: impl FnOnce(&mut McpServerState)) -> bool {
        let mut servers = self.servers.lock();
        let before = servers.get(id).cloned().unwrap_or_default();
        let mut state = before.clone();
        change(&mut state);
        if state == before {
            return false;
        }
        if state == McpServerState::default() {
            servers.remove(id);
        } else {
            servers.insert(id.to_string(), state);
        }
        true
    }

    /// Changes the record of `id` in place without saving it: for frequent
    /// changes that always change something, such as call counts.
    /// [`Self::flush`] saves them.
    pub fn update_later(&self, id: &str, change: impl FnOnce(&mut McpServerState)) {
        change(self.servers.lock().entry(id.to_string()).or_default());
        self.unsaved.store(true, Ordering::Relaxed);
    }

    /// Saves the changes made with [`Self::update_later`], if any.
    pub async fn flush(&self) {
        if self.unsaved.load(Ordering::Relaxed) {
            self.save().await;
        }
    }

    /// Forgets the servers `keep` rejects, and returns whether any were.
    pub fn retain(&self, keep: impl Fn(&str) -> bool) -> bool {
        let mut servers = self.servers.lock();
        let before = servers.len();
        servers.retain(|id, _| keep(id));
        servers.len() != before
    }

    /// Writes the state. Best effort: losing it costs only history.
    pub async fn save(&self) {
        let _guard = self.save_lock.lock().await;
        self.unsaved.store(false, Ordering::Relaxed);
        let file = McpStateFile {
            version: STATE_VERSION,
            servers: self.servers.lock().clone(),
        };
        let result = match serde_json::to_vec_pretty(&file) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                write_daemon_config_atomically(&self.path, &bytes).await
            }
            Err(err) => Err(err.into()),
        };
        if let Err(err) = result {
            log::warn!("failed to write {}: {err}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn state_round_trips_and_drops_empty_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MCP_STATE_FILE_NAME);
        let store = McpStateStore::open(path.clone()).await;
        assert!(store.update("github", |state| {
            state.source = McpSource::Model;
            state.added_at = Some(1);
        }));
        assert!(!store.update("github", |state| state.added_at = Some(1)));
        assert!(store.update("docs", |state| {
            state.last_error = Some(McpErrorRecord {
                at: 2,
                message: "tools/list timed out".into(),
            })
        }));
        store.save().await;

        // Counted in memory, written by the next flush.
        store.update_later("github", |state| state.usage.calls += 1);
        store.flush().await;

        let reopened = McpStateStore::open(path.clone()).await;
        assert_eq!(reopened.get("github").source, McpSource::Model);
        assert_eq!(reopened.get("github").usage.calls, 1);
        assert_eq!(
            reopened.get("docs").last_error.unwrap().message,
            "tools/list timed out"
        );
        // A record back at its defaults is not kept.
        assert!(reopened.update("docs", |state| state.last_error = None));
        assert!(reopened.retain(|id| id != "github"));
        assert_eq!(reopened.get("github"), McpServerState::default());

        let file: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
        assert_eq!(file["version"], 1);
        assert_eq!(file["servers"]["github"]["source"], "model");
    }

    #[tokio::test]
    async fn an_unreadable_state_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MCP_STATE_FILE_NAME);
        tokio::fs::write(&path, "not json").await.unwrap();
        let store = McpStateStore::open(path).await;
        assert_eq!(store.get("any"), McpServerState::default());
    }
}
