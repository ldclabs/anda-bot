//! Desktop transport state. Notifications invalidate caller-scoped snapshots;
//! they are deliberately coalesced, not a log of individual model tokens.
use anda_core::BoxError;
use futures::FutureExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha3::{Digest, Sha3_384};
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{io::AsyncWriteExt, sync::watch};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SubmissionState {
    Accepted,
    Completed,
    Failed,
    Unknown,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppCapabilities {
    pub state_invalidation: bool,
    pub submission_receipts: bool,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppInitialize {
    pub protocol_version: u32,
    pub instance_id: String,
    pub capabilities: AppCapabilities,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StateChanged {
    pub instance_id: String,
    pub revision: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppSubmit {
    pub request_id: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: anda_core::AgentInput,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SubmissionRead {
    pub source: String,
    pub request_id: String,
}

pub struct AppEvents {
    pub instance: String,
    callers: Mutex<HashMap<String, watch::Sender<u64>>>,
}

impl Default for AppEvents {
    fn default() -> Self {
        Self {
            instance: ic_auth_types::Xid::new().to_string(),
            callers: Mutex::new(HashMap::new()),
        }
    }
}

impl AppEvents {
    pub fn subscribe(&self, caller: &str) -> watch::Receiver<u64> {
        let mut callers = self.callers.lock();
        callers.retain(|_, sender| sender.receiver_count() > 0);
        callers
            .entry(caller.to_owned())
            .or_insert_with(|| watch::channel(0).0)
            .subscribe()
    }

    pub fn changed(&self, caller: &str) {
        if let Some(sender) = self.callers.lock().get(caller) {
            sender.send_modify(|revision| *revision += 1);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SubmissionReceipt {
    pub request_id: String,
    pub source: String,
    pub state: SubmissionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "unknown"))]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    schema: u32,
    digest: String,
    receipt: SubmissionReceipt,
}

/// The durable intent precedes execution. A process crash leaves `unknown` on
/// restart; we never re-enqueue that intent. A live duplicate joins the result.
pub struct Submissions {
    directory: PathBuf,
    active: tokio::sync::Mutex<HashMap<String, watch::Receiver<SubmissionReceipt>>>,
}

fn hash(bytes: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(Sha3_384::digest(bytes))
}

fn key(caller: &str, source: &str, id: &str) -> Result<String, String> {
    if source.is_empty() || source.len() > 2048 || id.is_empty() || id.len() > 128 {
        return Err("Invalid submission identity".into());
    }
    Ok(hash(
        &serde_json::to_vec(&(caller, source, id)).map_err(|e| e.to_string())?,
    ))
}

impl Submissions {
    pub fn new(home: &Path) -> Arc<Self> {
        Arc::new(Self {
            directory: home.join("desktop-submissions"),
            active: Default::default(),
        })
    }

    async fn record(&self, key: &str) -> Result<Option<Record>, String> {
        match tokio::fs::read(self.directory.join(format!("{key}.json"))).await {
            Ok(bytes) => {
                let record: Record = serde_json::from_slice(&bytes)
                    .map_err(|_| "Unreadable submission receipt; automatic replay is disabled")?;
                if record.schema != 1 {
                    return Err("Unsupported submission receipt schema".into());
                }
                Ok(Some(record))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub async fn read(
        &self,
        caller: &str,
        source: &str,
        id: &str,
    ) -> Result<Option<SubmissionReceipt>, String> {
        let key = key(caller, source, id)?;
        let active = self.active.lock().await;
        if let Some(receiver) = active.get(&key) {
            return Ok(Some(receiver.borrow().clone()));
        }
        Ok(self.record(&key).await?.map(|mut r| {
            if r.receipt.state == SubmissionState::Accepted {
                r.receipt.state = SubmissionState::Unknown;
            }
            r.receipt
        }))
    }

    pub async fn submit<F>(
        self: &Arc<Self>,
        caller: &str,
        source: String,
        id: String,
        input: &Value,
        execute: F,
    ) -> Result<SubmissionReceipt, String>
    where
        F: Future<Output = Result<Value, String>> + Send + 'static,
    {
        let key = key(caller, &source, &id)?;
        let mut canonical = input.clone();
        canonical.sort_all_objects();
        let digest = hash(&serde_json::to_vec(&canonical).map_err(|e| e.to_string())?);
        let mut active = self.active.lock().await;
        let mut receiver = if let Some(record) = self.record(&key).await? {
            if record.digest != digest {
                return Err("Submission identity already used with different input".into());
            }
            if let Some(receiver) = active.get(&key) {
                receiver.clone()
            } else {
                let mut receipt = record.receipt;
                if receipt.state == SubmissionState::Accepted {
                    receipt.state = SubmissionState::Unknown;
                }
                return Ok(receipt);
            }
        } else {
            let receipt = SubmissionReceipt {
                request_id: id,
                source,
                state: SubmissionState::Accepted,
                result: None,
                error: None,
            };
            self.save(
                &key,
                &Record {
                    schema: 1,
                    digest: digest.clone(),
                    receipt: receipt.clone(),
                },
                true,
            )
            .await?;
            let (sender, receiver) = watch::channel(receipt.clone());
            active.insert(key.clone(), receiver.clone());
            let this = self.clone();
            tokio::spawn(async move {
                // Independent of the transport task: a closed socket never
                // cancels an accepted request or permits duplicate execution.
                let mut receipt = receipt;
                match std::panic::AssertUnwindSafe(execute).catch_unwind().await {
                    Ok(Ok(result)) => {
                        receipt.state = SubmissionState::Completed;
                        receipt.result = Some(result);
                    }
                    Ok(Err(error)) => {
                        receipt.state = SubmissionState::Failed;
                        receipt.error = Some(error);
                    }
                    Err(_) => {
                        receipt.state = SubmissionState::Unknown;
                        receipt.error = Some(
                            "Submission execution panicked; inspect state before proceeding".into(),
                        );
                    }
                }
                if this
                    .save(
                        &key,
                        &Record {
                            schema: 1,
                            digest,
                            receipt: receipt.clone(),
                        },
                        false,
                    )
                    .await
                    .is_err()
                {
                    receipt.state = SubmissionState::Unknown;
                    receipt.result = None;
                    receipt.error =
                        Some("Could not persist submission result; do not replay".into());
                }
                sender.send_replace(receipt);
                this.active.lock().await.remove(&key);
            });
            receiver
        };
        drop(active);
        loop {
            let receipt = receiver.borrow_and_update().clone();
            if receipt.state != SubmissionState::Accepted {
                return Ok(receipt);
            }
            if receiver.changed().await.is_err() {
                return Err("Submission outcome unknown".into());
            }
        }
    }

    async fn save(&self, key: &str, record: &Record, create: bool) -> Result<(), String> {
        async {
            tokio::fs::create_dir_all(&self.directory).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
                    .await?;
            }
            let target = self.directory.join(format!("{key}.json"));
            let temporary = self
                .directory
                .join(format!("{key}.{}.tmp", ic_auth_types::Xid::new()));
            let path = if create { &target } else { &temporary };
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(path).await?;
            file.write_all(&serde_json::to_vec(record)?).await?;
            file.sync_all().await?;
            drop(file);
            if !create {
                tokio::fs::rename(&temporary, &target).await?;
            }
            // Ensure the durable intent's directory entry precedes execution.
            #[cfg(unix)]
            tokio::fs::File::open(&self.directory)
                .await?
                .sync_all()
                .await?;
            Ok::<_, BoxError>(())
        }
        .await
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn desktop_protocol_types_match_rust() {
        use ts_rs::TS;
        let config = ts_rs::Config::default();
        let definitions = [
            SubmissionState::decl(&config),
            SubmissionReceipt::decl(&config),
            AppCapabilities::decl(&config),
            AppInitialize::decl(&config),
            StateChanged::decl(&config),
            AppSubmit::decl(&config),
            SubmissionRead::decl(&config),
        ];
        let generated = format!(
            "// Generated from engine/app_protocol.rs. Run ANDA_EXPORT_PROTOCOL=1 RUST_MIN_STACK=16777216 cargo test -p anda_bot desktop_protocol_types --bin anda.\n{}\n",
            definitions
                .map(|definition| format!("export {definition}"))
                .join("\n")
        );
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop/src/shared/app-protocol.ts");
        if std::env::var_os("ANDA_EXPORT_PROTOCOL").is_some() {
            std::fs::write(&path, &generated).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
            generated,
            "Regenerate desktop protocol types"
        );
    }

    #[tokio::test]
    async fn subscriptions_are_caller_scoped_and_coalesce() {
        let events = AppEvents::default();
        let mut a = events.subscribe("a");
        let b = events.subscribe("b");
        for _ in 0..10_000 {
            events.changed("a");
        }
        a.changed().await.unwrap();
        assert_eq!(*a.borrow_and_update(), 10_000);
        assert!(!b.has_changed().unwrap());
    }

    #[tokio::test]
    async fn receipt_survives_disconnect_and_restart_without_replay() {
        let home = tempfile::tempdir().unwrap();
        let store = Submissions::new(home.path());
        let count = Arc::new(AtomicUsize::new(0));
        let executions = count.clone();
        let (release, ready) = tokio::sync::oneshot::channel();
        let task_store = store.clone();
        let task = tokio::spawn(async move {
            task_store
                .submit(
                    "a",
                    "chat".into(),
                    "id".into(),
                    &json!({"prompt":"hi"}),
                    async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        ready.await.unwrap();
                        Ok(json!({"conversation":42}))
                    },
                )
                .await
        });
        while count.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        task.abort();
        assert_eq!(
            store.read("a", "chat", "id").await.unwrap().unwrap().state,
            SubmissionState::Accepted
        );
        assert!(store.read("b", "chat", "id").await.unwrap().is_none());
        release.send(()).unwrap();
        let receipt = store
            .submit(
                "a",
                "chat".into(),
                "id".into(),
                &json!({"prompt":"hi"}),
                async { panic!("duplicate executed") },
            )
            .await
            .unwrap();
        assert_eq!(receipt.result, Some(json!({"conversation":42})));
        let restarted = Submissions::new(home.path());
        assert_eq!(
            restarted
                .read("a", "chat", "id")
                .await
                .unwrap()
                .unwrap()
                .state,
            SubmissionState::Completed
        );
        assert!(
            restarted
                .submit(
                    "a",
                    "chat".into(),
                    "id".into(),
                    &json!({"prompt":"different"}),
                    async { panic!() }
                )
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unfinished_durable_intent_is_unknown_after_restart() {
        let home = tempfile::tempdir().unwrap();
        let store = Submissions::new(home.path());
        let k = key("a", "chat", "id").unwrap();
        store
            .save(
                &k,
                &Record {
                    schema: 1,
                    digest: hash(b"{}"),
                    receipt: SubmissionReceipt {
                        request_id: "id".into(),
                        source: "chat".into(),
                        state: SubmissionState::Accepted,
                        result: None,
                        error: None,
                    },
                },
                true,
            )
            .await
            .unwrap();
        let receipt = store
            .submit("a", "chat".into(), "id".into(), &json!({}), async {
                panic!("replayed uncertain intent")
            })
            .await
            .unwrap();
        assert_eq!(receipt.state, SubmissionState::Unknown);
    }

    #[tokio::test]
    async fn receipt_digest_ignores_json_object_key_order() {
        let home = tempfile::tempdir().unwrap();
        let store = Submissions::new(home.path());
        let a: Value =
            serde_json::from_str(r#"{"prompt":"hi","meta":{"source":"chat","language":"en"}}"#)
                .unwrap();
        let b: Value =
            serde_json::from_str(r#"{"meta":{"language":"en","source":"chat"},"prompt":"hi"}"#)
                .unwrap();
        let first = store
            .submit("owner", "chat".into(), "id".into(), &a, async {
                Ok(json!({"conversation":1}))
            })
            .await
            .unwrap();
        let repeated = store
            .submit("owner", "chat".into(), "id".into(), &b, async {
                panic!("equivalent input replayed")
            })
            .await
            .unwrap();
        assert_eq!(first.result, repeated.result);
        assert_eq!(repeated.state, SubmissionState::Completed);
    }
}
