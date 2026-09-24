//! Owner-confirmed changes. Native writes and Bot Notes cleanup outlive HTTP waiters.
use super::{Journal, MemoryAccess, catalog::MemoryRecordView};
use anda_core::{BoxError, Principal};
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
    /// The Memory Interface receipt a misrecording repair or a deletion's
    /// erasure runs under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryChange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryChange {
    pub receipt_ref: String,
    /// `recorded`, `processed`, `available` or `failed`.
    pub phase: String,
    /// A deletion's ErasurePlan report: `pending`, `partial`, `blocked` or
    /// `completed`, with its plan reference and summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erasure: Option<anda_kip::memory::binding::ForgetResult>,
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
    /// The native product change. A misrecording has none: it is recording
    /// repair through the Memory Interface `revise` intent.
    #[serde(default)]
    native: Option<anda_brain::product::ChangeReceipt>,
}

/// How long a misrecording preview stays confirmable.
const MISRECORDED_PREVIEW_MS: u64 = 15 * 60 * 1000;

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
    recovery: tokio::sync::Mutex<super::journal::JournalPoll>,
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
            recovery: Default::default(),
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
        space: &anda_brain::space::Space,
        receipt: &anda_brain::product::ChangeReceipt,
        resolver: &mut super::activity::SourceResolver<'_>,
    ) -> Result<(), BoxError> {
        let mut sources = receipt.preview.record.sources.clone();
        for target in &receipt.preview.targets {
            if target.kind == "evidence" {
                sources.push(space.product_source(&target.id).await?);
            }
        }
        for source in sources {
            if resolver.resolve(&source).await?.is_none() {
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
        if input.kind == anda_brain::product::ChangeKind::Misrecorded {
            // Recording repair re-reads the original source; the preview is
            // the extraction the owner reports as never said.
            if !before.allowed_actions.iter().any(|a| a == "misrecorded") {
                return Err("unsupported_scope".into());
            }
            let view = ChangeView {
                schema_version: 1,
                operation_id: input.operation_id.clone(),
                state: "prepared".into(),
                preview_digest: anda_cognitive_nexus::content_digest(
                    &json!({"kind":"misrecorded","bot_scope":"memory-change-v1-with-notes-reset","before":before,"report":input.new_value}),
                )?,
                expires_at: anda_engine::unix_ms() + MISRECORDED_PREVIEW_MS,
                kind: input.kind.clone(),
                targets: vec![before.id.clone()],
                before: Some(before),
                new_value: input.new_value.clone(),
                affected_records: vec![],
                excluded_source_count: 0,
                resets_notes: true,
                replacement_record: None,
                error: None,
                memory: None,
            };
            let record = StoredChange {
                caller: caller.to_string(),
                input,
                input_digest: digest,
                view: view.clone(),
                native: None,
            };
            if !self
                .journal
                .create(&format!("changes/{key}"), &record)
                .await?
            {
                return Err("idempotency_conflict".into());
            }
            return Ok(view);
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
        let space = self
            .access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        let native = space.product_prepare(caller, native_input).await?;
        let mut resolver = self.sources.source_resolver(caller);
        if let Err(error) = Self::validate_sources(&space, &native, &mut resolver).await {
            space
                .product_discard(caller, input.operation_id.clone())
                .await?;
            return Err(error);
        }
        let mut affected_records = Vec::new();
        for target in &native.preview.targets {
            if target.kind == "assertion" {
                let record =
                    super::catalog::get_with_resolver(&self.access.host, &mut resolver, &target.id)
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
            memory: None,
        };
        let record = StoredChange {
            caller: caller.to_string(),
            input,
            input_digest: digest,
            view: view.clone(),
            native: Some(native),
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
        let space = self
            .access
            .host
            .state
            .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
            .await?;
        let stored = self
            .journal
            .read::<StoredChange>(&format!("changes/{key}"))
            .await?;
        if let Some(record) = &stored
            && record.native.is_none()
        {
            // A misrecording has no native preview; one already sent is
            // Brain's to finish.
            if record.view.state != "prepared" {
                return Err("revision_conflict".into());
            }
        } else {
            space.product_discard(caller, id.into()).await?;
        }
        if let Some(mut record) = stored {
            record.view.state = "discarded".into();
            record.view.before = None;
            record.view.affected_records.clear();
            record.view.new_value = None;
            record.input.new_value = None;
            if record.native.is_some() {
                record.native = Some(space.product_change(caller, id).await?);
            }
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
        let Some(prepared) = record.native.clone() else {
            return self.commit_repair(caller, key, record).await;
        };
        let current = space.product_change(caller, id).await?;
        // Recheck live sources only before native admission. They may already
        // be erased when reconciling a lost acknowledgement.
        if current.state == "prepared" && space.product_available() {
            Self::validate_sources(&space, &current, &mut self.sources.source_resolver(caller))
                .await?;
        }
        // A deletion is erased through the Memory Interface `forget`: the
        // same native closure, plus Brain's transcripts, verified by its
        // ErasurePlan. One the native product path already confirmed (an
        // older receipt) is only reconciled.
        if record.input.kind == anda_brain::product::ChangeKind::Delete
            && current.state != "confirmed"
        {
            if current.state == "prepared" {
                space.product_discard(caller, id.into()).await?;
            }
            return self.commit_erasure(caller, key, record).await;
        }
        record.view.state = "committing".into();
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        let (native, admission_error) = match space
            .product_commit(caller, id.into(), prepared.preview_digest.clone())
            .await
        {
            Ok(native) => (native, None),
            Err(error) => (space.product_change(caller, id).await?, Some(error)),
        };
        record.view.state = native.state.clone();
        record.view.replacement_record = native.replacement_record.clone();
        record.native = Some(native);
        if record.view.state == "confirmed" {
            // A native commit may persist its receipt and then fail while
            // returning it. Confirmation is only complete after Bot Notes are
            // coherent and a deleted preview has been cleared.
            if let Err(error) = self.access.synchronize_locked().await {
                log::debug!("Memory change {id}: Notes cleanup pending: {error}");
                record.view.state = "cleanup_pending".into();
                record.view.error = Some("notes_reset_pending".into());
            } else {
                record.view.error = None;
                if record.input.kind == anda_brain::product::ChangeKind::Delete {
                    record.view.before = None;
                    record.view.affected_records.clear();
                }
            }
        } else if let Some(error) = &admission_error {
            record.view.error = Some(if record.view.state == "prepared" {
                match error.to_string().as_str() {
                    "preview_expired"
                    | "revision_conflict"
                    | "idempotency_conflict"
                    | "memory_change_pending"
                    | "unsupported_scope"
                    | "unsupported_correction" => error.to_string(),
                    _ => "preview_needs_review".into(),
                }
            } else {
                "acceptance_unknown".into()
            });
        } else {
            record.view.error = None;
        }
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        if record.view.state == "prepared"
            && let Some(error) = admission_error
        {
            return Err(error);
        }
        Ok(record.view)
    }

    /// A misrecording (§57.8): the owner's report is staged and sent as a
    /// `revise` with `change_kind: "misrecorded"`, under a key naming this
    /// operation, so a retry replays it. Brain re-reads the original source
    /// and repairs the extraction; it is confirmed once processed.
    async fn commit_repair(
        &self,
        caller: Principal,
        key: &str,
        mut record: StoredChange,
    ) -> Result<ChangeView, BoxError> {
        use anda_kip::memory::binding::Operation;
        let namespace = caller.to_string();
        let operation = format!("anda-bot/change/{}", record.input.operation_id);
        record.view.state = "committing".into();
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        let before = record.view.before.as_ref().ok_or("revision_conflict")?;
        let report = match &record.input.new_value {
            Some(said) => format!(
                "That memory is wrong: I never said \"{}\". What I said was: {said}",
                before.text
            ),
            None => format!("That memory is wrong: I never said \"{}\".", before.text),
        };
        let host = &self.access.host;
        let staged = host
            .stage_memory_source(
                &namespace,
                anda_brain::memory_interface::StageSourceInput {
                    messages: vec![anda_core::Message {
                        role: "user".into(),
                        content: vec![report.into()],
                        ..Default::default()
                    }],
                    // Reported when the owner reviewed it, so a retry
                    // stages the same bytes.
                    observed_at: anda_engine::rfc3339_datetime(
                        record
                            .view
                            .expires_at
                            .saturating_sub(MISRECORDED_PREVIEW_MS),
                    ),
                    kind: Default::default(),
                    order: None,
                    idempotency_key: operation.clone(),
                },
                anda_brain::memory_interface::HostSource {
                    identity: anda_brain::product::SourceIdentity {
                        key: format!("anda-bot/change/{namespace}/{}", record.input.operation_id),
                        parents: vec![],
                    },
                    context: None,
                },
            )
            .await?;
        let response = host
            .memory(
                &namespace,
                true,
                super::memory::request(
                    Operation::Revise,
                    Some(operation),
                    json!({
                        "source_ref": staged.source_ref,
                        "target_ref": record.input.record_id,
                        "change_kind": "misrecorded",
                    }),
                ),
            )
            .await?;
        if let Some(error) = super::memory::failure(&response) {
            record.view.state = "failed".into();
            record.view.error = Some(error.to_string().chars().take(512).collect());
        } else if let (Some(receipt), Some(progress)) = (&response.receipt, &response.progress) {
            record.view.memory = Some(MemoryChange {
                receipt_ref: receipt.receipt_ref.clone(),
                phase: phase_name(progress.phase),
                erasure: None,
            });
            record.view.state = match progress.phase {
                anda_kip::memory::binding::Phase::Recorded => "committing".into(),
                anda_kip::memory::binding::Phase::Failed => "failed".into(),
                _ => "confirmed".into(),
            };
            record.view.error = progress
                .reason
                .clone()
                .filter(|_| record.view.state == "failed");
        }
        self.finish_memory_change(key, record).await
    }

    /// A deletion through `forget` with `mode: "semantic"` under a key
    /// naming this operation. `completed` and `partial` are both final: the
    /// view keeps the ErasurePlan report, and `blocked` names a legal hold.
    async fn commit_erasure(
        &self,
        caller: Principal,
        key: &str,
        mut record: StoredChange,
    ) -> Result<ChangeView, BoxError> {
        use anda_kip::memory::binding::{ForgetResult, ForgetStatus, Operation};
        record.view.state = "committing".into();
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        let response = self
            .access
            .host
            .memory(
                &caller.to_string(),
                true,
                super::memory::request(
                    Operation::Forget,
                    Some(format!("anda-bot/change/{}", record.input.operation_id)),
                    json!({"target_ref": record.input.record_id, "mode": "semantic"}),
                ),
            )
            .await?;
        if let Some(error) = super::memory::failure(&response) {
            record.view.state = "failed".into();
            record.view.error = Some(error.to_string().chars().take(512).collect());
            return self.finish_memory_change(key, record).await;
        }
        let erasure: ForgetResult =
            serde_json::from_value(response.result.clone().ok_or("forget returned no result")?)?;
        record.view.state = match erasure.status {
            ForgetStatus::Completed | ForgetStatus::Partial => "confirmed",
            ForgetStatus::Blocked => "blocked",
            ForgetStatus::Pending => "committing",
        }
        .into();
        record.view.error = (erasure.status == ForgetStatus::Blocked).then(|| "legal_hold".into());
        record.view.memory = Some(MemoryChange {
            receipt_ref: response
                .receipt
                .as_ref()
                .map(|r| r.receipt_ref.clone())
                .unwrap_or_default(),
            phase: response
                .progress
                .as_ref()
                .map(|p| phase_name(p.phase))
                .unwrap_or_default(),
            erasure: Some(erasure),
        });
        self.finish_memory_change(key, record).await
    }

    /// Saves a Memory Interface change; a confirmed one then resets Notes
    /// and clears a deleted preview, as a native one does.
    async fn finish_memory_change(
        &self,
        key: &str,
        mut record: StoredChange,
    ) -> Result<ChangeView, BoxError> {
        if record.view.state == "confirmed" {
            if let Err(error) = self.access.synchronize_locked().await {
                log::debug!(
                    "Memory change {}: Notes cleanup pending: {error}",
                    record.input.operation_id
                );
                record.view.state = "cleanup_pending".into();
                record.view.error = Some("notes_reset_pending".into());
            } else {
                record.view.error = None;
                if record.input.kind == anda_brain::product::ChangeKind::Delete {
                    record.view.before = None;
                    record.view.affected_records.clear();
                }
            }
        }
        self.journal
            .write(&format!("changes/{key}"), &record)
            .await?;
        Ok(record.view)
    }

    pub async fn run(self: &Arc<Self>, cancel: CancellationToken) {
        loop {
            let result = tokio::select! {_=cancel.cancelled()=>break,result=self.recover()=>result};
            let delay = if let Err(error) = result {
                log::warn!("Memory change recovery incomplete: {error}");
                60
            } else {
                5
            };
            tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
        }
        {
            let _guard = self.admitted.lock();
            self.closing
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.tasks.close();
        }
        self.tasks.wait().await;
    }

    pub(super) async fn recover(self: &Arc<Self>) -> Result<(), BoxError> {
        let mut poll = self.recovery.lock().await;
        let mut first_error = None;
        for (index, key) in poll
            .keys(&self.journal, &["changes/"])
            .await?
            .iter()
            .enumerate()
        {
            match self.recover_one(key).await {
                Ok(refresh) => poll.finish(key, refresh),
                Err(error) => {
                    first_error.get_or_insert_with(|| format!("{key}: {error}"));
                }
            }
            if (index + 1).is_multiple_of(20) {
                tokio::task::yield_now().await;
            }
        }
        match first_error {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }

    async fn recover_one(self: &Arc<Self>, key: &str) -> Result<bool, BoxError> {
        let Some(record) = self.journal.read::<StoredChange>(key).await? else {
            return Ok(false);
        };
        let pending = matches!(
            record.view.state.as_str(),
            "committing" | "reconciling" | "cleanup_pending"
        ) || (record.view.state == "confirmed" && !confirmed_and_clean(&record));
        if pending {
            let caller = record.caller.parse()?;
            let id = record.input.operation_id;
            let view = self
                .commit(caller, id.clone(), record.view.preview_digest)
                .await
                .map_err(|error| format!("operation {id}, reconcile: {error}"))?;
            if view.state == "cleanup_pending" {
                return Err(format!("operation {id}, Notes cleanup: notes_reset_pending").into());
            }
            return Ok(matches!(view.state.as_str(), "committing" | "reconciling"));
        }
        if record.view.state == "prepared" {
            if anda_engine::unix_ms() > record.view.expires_at {
                let caller = record.caller.parse()?;
                let id = record.input.operation_id;
                self.discard(caller, &id)
                    .await
                    .map_err(|error| format!("operation {id}, expire preview: {error}"))?;
            } else {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn phase_name(phase: anda_kip::memory::binding::Phase) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
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
