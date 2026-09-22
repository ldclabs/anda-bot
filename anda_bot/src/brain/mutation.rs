//! Owner-confirmed changes. Native writes and Bot Notes cleanup outlive HTTP waiters.
use super::{Journal, MemoryAccess, catalog::MemoryRecordView};
use anda_core::{BoxError, Principal};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRequest {
    pub operation_id: String,
    pub record_id: String,
    pub expected_revision: String,
    pub kind: anda_brain::product::ChangeKind,
    pub new_value: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRequest {
    pub preview_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangeView {
    pub schema_version: u32,
    pub operation_id: String,
    pub state: String,
    pub preview_digest: String,
    pub expires_at: u64,
    pub kind: anda_brain::product::ChangeKind,
    pub before: Option<MemoryRecordView>,
    pub new_value: Option<String>,
    pub targets: Vec<String>,
    #[serde(default)]
    pub affected_records: Vec<ChangeRecordSummary>,
    pub excluded_source_count: usize,
    pub resets_notes: bool,
    pub replacement_record: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangeRecordSummary {
    pub id: String,
    pub text: String,
    pub state: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct StoredChange {
    caller: String,
    input: ChangeRequest,
    input_digest: String,
    view: ChangeView,
    native: anda_brain::product::ChangeReceipt,
}

fn confirmed_and_clean(record: &StoredChange) -> bool {
    record.view.state == "confirmed"
        && record.view.error.is_none()
        && (record.input.kind != anda_brain::product::ChangeKind::Delete
            || record.view.before.is_none())
}

pub struct MutationService {
    access: Arc<MemoryAccess>,
    journal: Journal,
    sources: super::activity::ActivityStore,
    tasks: TaskTracker,
    admitted: Arc<parking_lot::Mutex<BTreeSet<String>>>,
    closing: std::sync::atomic::AtomicBool,
}

impl MutationService {
    pub fn new(
        access: Arc<MemoryAccess>,
        journal: Journal,
        sources: super::activity::ActivityStore,
    ) -> Arc<Self> {
        Arc::new(Self {
            access,
            journal,
            sources,
            tasks: TaskTracker::new(),
            admitted: Default::default(),
            closing: Default::default(),
        })
    }

    pub async fn existing(
        &self,
        caller: Principal,
        input: &ChangeRequest,
    ) -> Result<Option<ChangeView>, BoxError> {
        let key = key(caller, &input.operation_id)?;
        let Some(record) = self
            .journal
            .read::<StoredChange>(&format!("changes/{key}"))
            .await?
        else {
            return Ok(None);
        };
        if record.caller != caller.to_string()
            || record.input_digest
                != anda_cognitive_nexus::content_digest(&serde_json::to_value(input)?)?
        {
            return Err("idempotency_conflict".into());
        }
        Ok(Some(record.view))
    }
    async fn validate_sources(
        &self,
        caller: Principal,
        receipt: &anda_brain::product::ChangeReceipt,
    ) -> Result<(), BoxError> {
        let space = self
            .access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        let mut sources = receipt.preview.record.sources.clone();
        for target in &receipt.preview.targets {
            if target.kind == "evidence" {
                sources.push(space.product_source(&target.id).await?);
            }
        }
        for source in sources {
            if self
                .sources
                .resolve_source(caller, &source)
                .await?
                .is_none()
            {
                return Err("unsupported_scope".into());
            }
        }
        Ok(())
    }

    pub async fn prepare(
        &self,
        caller: Principal,
        input: ChangeRequest,
        before: MemoryRecordView,
    ) -> Result<ChangeView, BoxError> {
        let key = key(caller, &input.operation_id)?;
        let digest = anda_cognitive_nexus::content_digest(&serde_json::to_value(&input)?)?;
        let _gate = self.access.gate.lock().await;
        if let Some(old) = self
            .journal
            .read::<StoredChange>(&format!("changes/{key}"))
            .await?
        {
            if old.caller != caller.to_string() || old.input_digest != digest {
                return Err("idempotency_conflict".into());
            }
            return Ok(old.view);
        }
        if !before.sources_complete {
            return Err("unsupported_scope".into());
        }
        if before.revision != input.expected_revision {
            return Err("revision_conflict".into());
        }
        let native_input = anda_brain::product::ChangeInput {
            operation_id: input.operation_id.clone(),
            record_id: input.record_id.clone(),
            expected_revision: input
                .expected_revision
                .parse()
                .map_err(|_| "invalid_request")?,
            kind: input.kind.clone(),
            new_value: input.new_value.clone(),
        };
        let native = self
            .access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?
            .product_prepare(caller, native_input)
            .await?;
        if let Err(error) = self.validate_sources(caller, &native).await {
            self.access
                .host
                .state
                .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
                .await?
                .product_discard(caller, input.operation_id.clone())
                .await?;
            return Err(error);
        }
        let mut affected_records = Vec::new();
        for target in &native.preview.targets {
            if target.kind == "assertion" {
                let record =
                    super::catalog::get(&self.access.host, &self.sources, caller, &target.id)
                        .await?;
                affected_records.push(ChangeRecordSummary {
                    id: record.id,
                    text: record.text,
                    state: record.state,
                });
            }
        }
        let view = ChangeView {
            schema_version: 1,
            operation_id: input.operation_id.clone(),
            state: "prepared".into(),
            preview_digest: anda_cognitive_nexus::content_digest(
                &json!({"native":native.preview_digest,"bot_scope":"memory-change-v1-with-notes-reset","before":before,"affected_records":affected_records}),
            )?,
            expires_at: native.expires_at,
            kind: input.kind.clone(),
            before: Some(before),
            new_value: input.new_value.clone(),
            targets: native
                .preview
                .targets
                .iter()
                .map(|target| target.id.clone())
                .collect(),
            excluded_source_count: native.preview.excluded_sources.len(),
            affected_records,
            resets_notes: true,
            replacement_record: None,
            error: None,
        };
        let record = StoredChange {
            caller: caller.to_string(),
            input,
            input_digest: digest,
            view: view.clone(),
            native,
        };
        if !self
            .journal
            .create(&format!("changes/{key}"), &record)
            .await?
        {
            return Err("idempotency_conflict".into());
        }
        Ok(view)
    }

    pub async fn get(&self, caller: Principal, id: &str) -> Result<ChangeView, BoxError> {
        let record = self.read(caller, id).await?;
        Ok(record.view)
    }

    pub async fn discard(&self, caller: Principal, id: &str) -> Result<(), BoxError> {
        let key = key(caller, id)?;
        let _guard = self.access.gate.lock().await;
        self.access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?
            .product_discard(caller, id.into())
            .await?;
        if let Some(mut record) = self
            .journal
            .read::<StoredChange>(&format!("changes/{key}"))
            .await?
        {
            record.view.state = "discarded".into();
            record.view.before = None;
            record.view.affected_records.clear();
            record.view.new_value = None;
            record.input.new_value = None;
            record.native = self
                .access
                .host
                .state
                .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
                .await?
                .product_change(caller, id)
                .await?;
            self.journal
                .write(&format!("changes/{key}"), &record)
                .await?;
        }
        Ok(())
    }

    async fn read(&self, caller: Principal, id: &str) -> Result<StoredChange, BoxError> {
        let key = key(caller, id)?;
        let record = self
            .journal
            .read::<StoredChange>(&format!("changes/{key}"))
            .await?
            .ok_or("not_found")?;
        if record.caller != caller.to_string() {
            return Err("not_found".into());
        }
        Ok(record)
    }

    pub async fn commit(
        self: &Arc<Self>,
        caller: Principal,
        id: String,
        preview_digest: String,
    ) -> Result<ChangeView, BoxError> {
        use std::sync::atomic::Ordering;
        let record = self.read(caller, &id).await?;
        if record.view.preview_digest != preview_digest {
            return Err("revision_conflict".into());
        }
        if confirmed_and_clean(&record) {
            return Ok(record.view);
        }
        let key = key(caller, &id)?;
        let task = {
            let mut admitted = self.admitted.lock();
            if self.closing.load(Ordering::SeqCst) {
                return Err("service_unavailable".into());
            }
            if admitted.contains(&key) {
                return Ok(record.view);
            }
            if admitted.len() >= 8 {
                return Err("capacity".into());
            }
            admitted.insert(key.clone());
            let this = self.clone();
            let admitted = self.admitted.clone();
            self.tasks.spawn(async move {
                let _admission = Admission {
                    admitted,
                    key: key.clone(),
                };
                this.commit_inner(caller, &id, &key).await
            })
        };
        task.await?
    }

    async fn commit_inner(
        &self,
        caller: Principal,
        id: &str,
        key: &str,
    ) -> Result<ChangeView, BoxError> {
        let _gate = self.access.gate.lock().await;
        let _engine = self.access.keep_engine_alive()?;
        let mut record = self.read(caller, id).await?;
        if confirmed_and_clean(&record) {
            return Ok(record.view);
        }
        let space = self
            .access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        let current = space.product_change(caller, id).await?;
        // Recheck live sources only before native admission. They may already
        // be erased when reconciling a lost acknowledgement.
        if current.state == "prepared" && space.product_available() {
            self.validate_sources(caller, &current).await?;
        }
        record.view.state = "committing".into();
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        let space = self
            .access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        let (native, admission_uncertain) = match space
            .product_commit(caller, id.into(), record.native.preview_digest.clone())
            .await
        {
            Ok(native) => (native, false),
            Err(_) => (space.product_change(caller, id).await?, true),
        };
        record.view.state = native.state.clone();
        record.view.replacement_record = native.replacement_record.clone();
        record.native = native;
        if record.view.state == "confirmed" {
            // A native commit may persist its receipt and then fail while
            // returning it. Confirmation is only complete after Bot Notes are
            // coherent and a deleted preview has been cleared.
            if self.access.synchronize_locked().await.is_err() {
                record.view.state = "cleanup_pending".into();
                record.view.error = Some("notes_reset_pending".into());
            } else {
                record.view.error = None;
                if record.input.kind == anda_brain::product::ChangeKind::Delete {
                    record.view.before = None;
                    record.view.affected_records.clear();
                }
            }
        } else if admission_uncertain {
            record.view.error = Some(
                if record.view.state == "prepared" {
                    "preview_needs_review"
                } else {
                    "acceptance_unknown"
                }
                .into(),
            );
        } else {
            record.view.error = None;
        }
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        Ok(record.view)
    }

    pub async fn run(self: &Arc<Self>, cancel: CancellationToken) {
        loop {
            tokio::select! {_=cancel.cancelled()=>break,_=self.recover()=>{}}
            tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(5))=>{}}
        }
        {
            let _guard = self.admitted.lock();
            self.closing
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.tasks.close();
        }
        self.tasks.wait().await;
    }

    async fn recover(self: &Arc<Self>) {
        let store = self.journal.object_store();
        let prefix = object_store::path::Path::from("bot-brain/v1/changes/");
        let mut entries = store.list(Some(&prefix));
        let mut count = 0usize;
        while let Ok(Some(entry)) = entries.try_next().await {
            let Some(key) = entry.location.as_ref().strip_prefix("bot-brain/v1/") else {
                continue;
            };
            let Ok(Some(record)) = self.journal.read::<StoredChange>(key).await else {
                continue;
            };
            if matches!(
                record.view.state.as_str(),
                "committing" | "reconciling" | "cleanup_pending"
            ) || (record.view.state == "confirmed" && !confirmed_and_clean(&record))
            {
                if let Ok(caller) = record.caller.parse() {
                    let _ = self
                        .commit(
                            caller,
                            record.input.operation_id,
                            record.view.preview_digest,
                        )
                        .await;
                }
            } else if record.view.state == "prepared"
                && anda_engine::unix_ms() > record.view.expires_at
                && let Ok(caller) = record.caller.parse()
            {
                let _ = self.discard(caller, &record.input.operation_id).await;
            }
            count += 1;
            if count.is_multiple_of(20) {
                tokio::task::yield_now().await;
            }
        }
    }
}

fn key(caller: Principal, id: &str) -> Result<String, BoxError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("invalid_request".into());
    }
    Ok(anda_cognitive_nexus::content_digest(
        &json!({"caller":caller.to_string(),"operation_id":id}),
    )?[7..]
        .into())
}

struct Admission {
    admitted: Arc<parking_lot::Mutex<BTreeSet<String>>>,
    key: String,
}
impl Drop for Admission {
    fn drop(&mut self) {
        self.admitted.lock().remove(&self.key);
    }
}
