//! mcp_secrets.json: the values that `${secret:NAME}` references in mcp.json
//! expand to. Keeping them apart lets mcp.json be read, shared and backed up
//! without the tokens in it. The file is owner-only like the credential
//! store, and its values never leave the daemon: the API reports only which
//! names are set and when.

use anda_core::BoxError;
use anda_engine::unix_ms;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use super::{McpError, config_store};
use crate::config::{McpSecretValues, McpServerSettings, is_secret_name, secret_references};
use crate::engine::write_daemon_config_atomically;

pub(crate) const MCP_SECRETS_FILE_NAME: &str = "mcp_secrets.json";
const SECRETS_VERSION: u32 = 1;
/// Tokens and keys are short; this bounds a mistaken paste.
const MAX_SECRET_BYTES: usize = 16 * 1024;

/// A secret as the API shows it: never its value.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpSecretView {
    pub name: String,
    pub is_set: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    /// The servers whose mcp.json entries reference it.
    pub used_by: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredSecret {
    value: String,
    updated_at: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct SecretsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    secrets: BTreeMap<String, StoredSecret>,
}

pub(crate) struct McpSecretStore {
    path: PathBuf,
    secrets: parking_lot::RwLock<BTreeMap<String, StoredSecret>>,
    /// Why the file could not be read. Writing then would lose the secrets
    /// in it, so changes are refused until it is fixed or removed.
    unreadable: Option<String>,
    /// Serializes saves, so an older snapshot never lands after a newer one.
    save_lock: tokio::sync::Mutex<()>,
}

impl McpSecretStore {
    /// Loads the secrets at `path`; a missing file holds none.
    pub async fn open(path: PathBuf) -> Self {
        let (secrets, unreadable) = match tokio::fs::read(&path).await {
            Ok(bytes) => match serde_json::from_slice::<SecretsFile>(&bytes) {
                Ok(file) => (file.secrets, None),
                Err(err) => (BTreeMap::new(), Some(err.to_string())),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (BTreeMap::new(), None),
            Err(err) => (BTreeMap::new(), Some(err.to_string())),
        };
        if let Some(err) = &unreadable {
            log::warn!("{} was not loaded: {err}", path.display());
        }
        Self {
            path,
            secrets: parking_lot::RwLock::new(secrets),
            unreadable,
            save_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// The values references expand to.
    pub fn values(&self) -> McpSecretValues {
        self.secrets
            .read()
            .iter()
            .map(|(name, secret)| (name.clone(), secret.value.clone()))
            .collect()
    }

    /// The names that are set, with when each was.
    pub fn names(&self) -> BTreeMap<String, u64> {
        self.secrets
            .read()
            .iter()
            .map(|(name, secret)| (name.clone(), secret.updated_at))
            .collect()
    }

    /// Sets `name` to `value`, or removes it when `value` is `None`, and
    /// returns whether anything changed.
    pub async fn set(&self, name: &str, value: Option<&str>) -> Result<bool, BoxError> {
        if !is_secret_name(name) {
            return Err(McpError::invalid(format!(
                "{name:?} cannot name a secret: use letters, digits and _, not starting with a digit"
            )));
        }
        if let Some(err) = &self.unreadable {
            return Err(McpError::invalid(format!(
                "{} could not be read ({err}); fix or remove it first",
                self.path.display()
            )));
        }
        let value = match value.map(str::trim) {
            Some("") => return Err(McpError::invalid("the secret value is empty")),
            Some(value) if value.len() > MAX_SECRET_BYTES => {
                return Err(McpError::invalid("the secret value is longer than 16 KiB"));
            }
            value => value,
        };
        let _guard = self.save_lock.lock().await;
        let file = {
            let mut secrets = self.secrets.write();
            let changed = match value {
                Some(value) if secrets.get(name).is_some_and(|old| old.value == value) => false,
                Some(value) => {
                    secrets.insert(
                        name.to_string(),
                        StoredSecret {
                            value: value.to_string(),
                            updated_at: unix_ms(),
                        },
                    );
                    true
                }
                None => secrets.remove(name).is_some(),
            };
            if !changed {
                return Ok(false);
            }
            SecretsFile {
                version: SECRETS_VERSION,
                secrets: secrets.clone(),
            }
        };
        let mut bytes = serde_json::to_vec_pretty(&file)?;
        bytes.push(b'\n');
        write_daemon_config_atomically(&self.path, &bytes).await?;
        Ok(true)
    }
}

/// The secrets referenced by the mcp.json entries in `root` (an invalid or
/// disabled one included) and by `servers`, with the ids that reference each.
pub(crate) fn secrets_in_use<'a>(
    root: &Value,
    servers: impl IntoIterator<Item = &'a McpServerSettings>,
) -> BTreeMap<String, BTreeSet<String>> {
    let entries = config_store::raw_entries(root)
        .into_iter()
        .map(|(id, raw)| {
            let mut names = BTreeSet::new();
            secret_references(&raw.to_string(), &mut names);
            (id, names)
        });
    let servers = servers
        .into_iter()
        .map(|server| (server.id.clone(), server.secret_names()));
    let mut in_use: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (id, names) in entries.chain(servers) {
        for name in names {
            in_use.entry(name).or_default().insert(id.clone());
        }
    }
    in_use
}

/// Every secret that is set (`set`, with when) or referenced (`in_use`).
pub(crate) fn secret_views(
    in_use: BTreeMap<String, BTreeSet<String>>,
    set: &BTreeMap<String, u64>,
) -> Vec<McpSecretView> {
    let mut all = in_use;
    for name in set.keys() {
        all.entry(name.clone()).or_default();
    }
    all.into_iter()
        .map(|(name, used_by)| McpSecretView {
            is_set: set.contains_key(&name),
            updated_at: set.get(&name).copied(),
            used_by: used_by.into_iter().collect(),
            name,
        })
        .collect()
}

/// The set secrets that a server removed from `before` to `after` used and
/// no other server does: they go with it.
pub(crate) fn orphaned_secrets(
    id: &str,
    before: &BTreeMap<String, BTreeSet<String>>,
    after: &BTreeMap<String, BTreeSet<String>>,
    set: &BTreeMap<String, u64>,
) -> Vec<String> {
    before
        .iter()
        .filter(|(name, users)| {
            users.contains(id) && !after.contains_key(*name) && set.contains_key(*name)
        })
        .map(|(name, _)| name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn secrets_persist_owner_only_and_refuse_bad_names_and_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MCP_SECRETS_FILE_NAME);
        let store = McpSecretStore::open(path.clone()).await;
        assert!(store.values().is_empty());

        assert!(store.set("GITHUB_PAT", Some(" ghp_x\n")).await.unwrap());
        assert!(!store.set("GITHUB_PAT", Some("ghp_x")).await.unwrap());
        assert!(store.set("1BAD", Some("x")).await.is_err());
        assert!(store.set("EMPTY", Some("  ")).await.is_err());
        assert!(
            store
                .set("BIG", Some(&"x".repeat(MAX_SECRET_BYTES + 1)))
                .await
                .is_err()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let reopened = McpSecretStore::open(path.clone()).await;
        assert_eq!(reopened.values()["GITHUB_PAT"], "ghp_x");
        assert!(reopened.names().contains_key("GITHUB_PAT"));

        assert!(reopened.set("GITHUB_PAT", None).await.unwrap());
        assert!(!reopened.set("GITHUB_PAT", None).await.unwrap());
        assert!(McpSecretStore::open(path.clone()).await.values().is_empty());

        // A file that cannot be read is never overwritten.
        std::fs::write(&path, "{ not json").unwrap();
        let broken = McpSecretStore::open(path.clone()).await;
        assert!(broken.set("NEW", Some("x")).await.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }
}
