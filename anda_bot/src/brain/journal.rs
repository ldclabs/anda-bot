//! Small host associations, separate from model context and native authority.
//! Versioned object keys use the existing MetaStore's conditional writes.
use anda_brain::recall_receipt::RecallReceiptRef;
use anda_core::{AgentOutput, BoxError, Usage};
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, path::Path};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Journal {
    store: Arc<dyn ObjectStore>,
    formation_locks: Arc<parking_lot::Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>>,
    changed: Arc<parking_lot::Mutex<BTreeSet<String>>>,
    #[cfg(test)]
    reads: Arc<std::sync::atomic::AtomicUsize>,
}

/// An in-process read optimization, not an execution queue. The durable journal
/// is rescanned on startup and periodically to repair missed notifications.
#[derive(Default)]
pub(super) struct JournalPoll {
    next_scan: Option<Instant>,
    pending: BTreeSet<String>,
}

impl JournalPoll {
    pub async fn keys(
        &mut self,
        journal: &Journal,
        prefixes: &[&str],
    ) -> Result<Vec<String>, BoxError> {
        {
            let mut changed = journal.changed.lock();
            changed.retain(|key| {
                if prefixes.iter().any(|prefix| key.starts_with(prefix)) {
                    self.pending.insert(key.clone());
                    false
                } else {
                    true
                }
            });
        }
        if self.next_scan.is_none_or(|next| Instant::now() >= next) {
            use futures::TryStreamExt;
            for prefix in prefixes {
                let path = Path::from(format!("bot-brain/v1/{prefix}"));
                let mut entries = journal.store.list(Some(&path));
                while let Some(entry) = entries.try_next().await? {
                    if let Some(key) = entry.location.as_ref().strip_prefix("bot-brain/v1/") {
                        self.pending.insert(key.into());
                    }
                }
            }
            self.next_scan = Some(Instant::now() + Duration::from_secs(300));
        }
        Ok(self.pending.iter().cloned().collect())
    }

    pub fn finish(&mut self, key: &str, refresh: bool) {
        if !refresh {
            self.pending.remove(key);
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecallDelivery {
    pub invocation: String,
    pub caller: String,
    pub bot_conversation: Option<u64>,
    pub bot_turn: Option<String>,
    pub tool_call: Option<String>,
    pub brain_conversation: Option<u64>,
    pub receipt: Option<RecallReceiptRef>,
    pub delivered_at: u64,
    pub failed: bool,
    pub usage: Usage,
    pub tools_usage: HashMap<String, Usage>,
    /// Provider prices and omitted measurements are unknown, never zero.
    pub accounting_complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormationSubmission {
    pub bot_conversation: u64,
    pub window_start: usize,
    pub window_end: usize,
    pub submitted_at: u64,
    /// The source time captured for this observe attempt, reused on replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub brain_conversation: Option<u64>,
    pub state: FormationState,
    pub error: Option<String>,
    #[serde(default)]
    pub provenance: Option<FormationProvenance>,
    #[serde(default)]
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub failure_stage: Option<FormationFailure>,
    /// The Memory Interface receipt of this window's `observe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_ref: Option<String>,
    /// Which submission of the window the observe key names; it moves on
    /// only after a definite rejection, so a retry replays, never re-forms.
    #[serde(default)]
    pub attempt: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormationProvenance {
    #[serde(default)]
    pub policy_revision: Option<String>,
    pub version: u32,
    pub caller: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub source_identity: Option<anda_brain::product::SourceIdentity>,
    pub source: String,
    pub reply_target: Option<String>,
    pub thread: Option<String>,
    pub external_user: bool,
    pub counterparty: Option<String>,
    pub source_messages: Vec<SourceMessageRef>,
    pub input_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceMessageRef {
    // Public IDs/indices are strings to avoid loss of precision in JS.
    pub conversation: String,
    pub index: String,
    pub role: String,
    pub content_digest: String,
    #[serde(default)]
    pub submitted_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormationFailure {
    SubmissionRejected,
    NativeFailed,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormationState {
    Pending,
    Accepted,
    Processing,
    Completed,
    Failed,
    Unknown,
    Suppressed,
}

impl Journal {
    pub fn new(store: Arc<dyn ObjectStore>) -> Self {
        Self {
            store,
            formation_locks: Default::default(),
            changed: Default::default(),
            #[cfg(test)]
            reads: Default::default(),
        }
    }

    pub(super) fn formation_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.formation_locks.lock();
        locks.retain(|_, lock| lock.strong_count() > 0);
        let lock = locks
            .get(key)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| Arc::new(tokio::sync::Mutex::new(())));
        locks.insert(key.into(), Arc::downgrade(&lock));
        lock
    }

    pub(super) fn object_store(&self) -> Arc<dyn ObjectStore> {
        self.store.clone()
    }

    pub(super) fn submission_in_flight(&self, key: &str) -> bool {
        self.formation_locks
            .lock()
            .get(key)
            .is_some_and(|lock| lock.strong_count() > 0)
    }
    pub async fn read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, BoxError> {
        #[cfg(test)]
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match self
            .store
            .get(&Path::from(format!("bot-brain/v1/{key}")))
            .await
        {
            Ok(value) => Ok(Some(serde_json::from_slice(&value.bytes().await?)?)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
    #[cfg(test)]
    pub(super) fn read_count(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub async fn write<T: Serialize>(&self, key: &str, value: &T) -> Result<(), BoxError> {
        self.store
            .put(
                &Path::from(format!("bot-brain/v1/{key}")),
                serde_json::to_vec(value)?.into(),
            )
            .await?;
        self.mark_changed(key);
        Ok(())
    }
    pub async fn create<T: Serialize>(&self, key: &str, value: &T) -> Result<bool, BoxError> {
        match self
            .store
            .put_opts(
                &Path::from(format!("bot-brain/v1/{key}")),
                serde_json::to_vec(value)?.into(),
                PutOptions {
                    mode: PutMode::Create,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => {
                self.mark_changed(key);
                Ok(true)
            }
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
    fn mark_changed(&self, key: &str) {
        if ["formation/", "recall/", "changes/"]
            .iter()
            .any(|prefix| key.starts_with(prefix))
        {
            self.changed.lock().insert(key.into());
        }
    }
    pub async fn record_recall(&self, delivery: &RecallDelivery) -> Result<(), BoxError> {
        self.write(&format!("recall/{}", delivery.invocation), delivery)
            .await
    }
    pub async fn submit_formation(
        &self,
        client: &super::Client,
        mut submission: FormationSubmission,
        input: anda_brain::types::FormationInputRef<'_>,
    ) -> Result<FormationSubmission, BoxError> {
        let key = format!(
            "formation/{}/{}",
            submission.bot_conversation, submission.window_start
        );
        let lock = self.formation_lock(&key);
        let _guard = lock.lock().await;
        if let (Some(host), Some(_)) = (client.embedded_host(), &submission.provenance) {
            return self
                .observe_formation(client, &host, &key, submission, input)
                .await;
        }
        let requested_window = submission.clone();
        if !self.create(&key, &submission).await? {
            submission = self
                .read(&key)
                .await?
                .ok_or("formation journal disappeared")?;
            if submission.state == FormationState::Suppressed {
                return Ok(submission);
            }
            // A crash or transport failure may have happened after acceptance.
            // No server idempotency key exists; do not blindly resend this window.
            if submission.brain_conversation.is_some() {
                self.refresh_formation_unlocked(client, &mut submission)
                    .await?;
                return Ok(submission);
            }
            if submission.state == FormationState::Failed {
                submission = requested_window;
                submission.state = FormationState::Pending;
                self.write(&key, &submission).await?;
            } else {
                return Err("formation acceptance is unknown; retained the original window for reconciliation".into());
            }
        }
        let result = client.formation_submission(input, &submission).await;
        match result {
            Ok(AgentOutput {
                conversation,
                failed_reason,
                ..
            }) => {
                submission.brain_conversation = conversation;
                submission.state = if failed_reason.is_some() {
                    FormationState::Failed
                } else if conversation.is_some() {
                    FormationState::Accepted
                } else {
                    FormationState::Unknown
                };
                submission.error = failed_reason.map(|s| s.chars().take(512).collect());
                submission.failure_stage = match submission.state {
                    FormationState::Failed => Some(FormationFailure::SubmissionRejected),
                    FormationState::Unknown => Some(FormationFailure::Unknown),
                    _ => None,
                };
            }
            Err(err) => {
                if matches!(
                    err.downcast_ref::<anda_brain::product::SourceAdmissionError>(),
                    Some(anda_brain::product::SourceAdmissionError::Suppressed)
                ) {
                    submission.state = FormationState::Suppressed;
                    submission.updated_at = Some(anda_engine::unix_ms());
                    submission.error = None;
                    self.write(&key, &submission).await?;
                    return Ok(submission);
                }
                // Any completed HTTP/RPC response is a definite rejection and
                // can be retried with backoff. Only transport failures may have
                // lost an acceptance response and must remain unresolved.
                let confirmed_http_status = err
                    .downcast_ref::<super::HttpError>()
                    .map(|response| response.status);
                submission.state = if confirmed_http_status.is_some()
                    || err.downcast_ref::<super::RpcFailure>().is_some()
                {
                    FormationState::Failed
                } else {
                    FormationState::Unknown
                };
                submission.error = Some(err.to_string().chars().take(512).collect());
                submission.failure_stage = Some(if submission.state == FormationState::Failed {
                    FormationFailure::SubmissionRejected
                } else {
                    FormationFailure::Unknown
                });
            }
        }
        submission.updated_at = Some(anda_engine::unix_ms());
        self.write(&key, &submission).await?;
        if submission.state == FormationState::Accepted {
            Ok(submission)
        } else {
            Err(format!("formation {}: {:?}", key, submission.state).into())
        }
    }
    pub async fn refresh_formation(
        &self,
        client: &super::Client,
        submission: &mut FormationSubmission,
    ) -> Result<(), BoxError> {
        let key = format!(
            "formation/{}/{}",
            submission.bot_conversation, submission.window_start
        );
        let lock = self.formation_lock(&key);
        let _guard = lock.lock().await;
        if let Some(latest) = self.read(&key).await? {
            *submission = latest;
        }
        self.refresh_formation_unlocked(client, submission).await
    }

    /// Submits a window through the Memory Interface: the window is staged
    /// under the Bot's source identity and observed under a key naming the
    /// window and its attempt, so a retry after an interruption replays the
    /// same receipt instead of forming the window twice. Only a definite
    /// rejection moves to the next attempt. A native Formation failure after
    /// acceptance is Brain's to retry, as before.
    async fn observe_formation(
        &self,
        client: &super::Client,
        host: &super::Host,
        key: &str,
        requested: FormationSubmission,
        input: anda_brain::types::FormationInputRef<'_>,
    ) -> Result<FormationSubmission, BoxError> {
        use anda_kip::KipErrorCode;
        let requested_at = requested.submitted_at;
        let mut submission = requested.clone();
        submission.observed_at = input
            .timestamp
            .clone()
            .or_else(|| anda_engine::rfc3339_datetime(submission.submitted_at));
        let requested_time = submission.observed_at.clone();
        if !self.create(key, &submission).await? {
            let mut existing: FormationSubmission = self
                .read(key)
                .await?
                .ok_or("formation journal disappeared")?;
            if existing.state == FormationState::Suppressed {
                return Ok(existing);
            }
            if existing.brain_conversation.is_some() {
                self.refresh_formation_unlocked(client, &mut existing)
                    .await?;
                return Ok(existing);
            }
            let rejected =
                existing.state == FormationState::Failed || existing.receipt_ref.is_some();
            submission = FormationSubmission {
                attempt: existing.attempt + u32::from(rejected),
                state: FormationState::Pending,
                submitted_at: if rejected {
                    requested.submitted_at
                } else {
                    existing.submitted_at
                },
                observed_at: if rejected {
                    requested_time.clone()
                } else {
                    existing
                        .observed_at
                        .or_else(|| anda_engine::rfc3339_datetime(existing.submitted_at))
                },
                ..requested
            };
        }
        let mut replaced = false;
        let outcome = loop {
            let timestamp = submission.observed_at.clone();
            let replay_input = anda_brain::types::FormationInputRef {
                messages: input.messages,
                context: input.context,
                timestamp: &timestamp,
            };
            if let Some(provenance) = &mut submission.provenance {
                provenance.input_digest = Some(anda_cognitive_nexus::content_digest(
                    &serde_json::to_value(&replay_input)?,
                )?);
            }
            self.write(key, &submission).await?;
            match super::memory::observe_window(host, &submission, &replay_input).await {
                // The key was bound to other bytes (an interrupted, shorter
                // window): this window is a new submission.
                Err(error)
                    if !replaced
                        && error
                            .downcast_ref::<anda_kip::KipError>()
                            .is_some_and(|e| e.code == KipErrorCode::IdempotencyConflict) =>
                {
                    replaced = true;
                    submission.attempt += 1;
                    submission.submitted_at = requested_at;
                    submission.observed_at = requested_time.clone();
                }
                outcome => break outcome,
            }
        };
        match outcome {
            Ok((receipt_ref, state)) => {
                submission.receipt_ref = Some(receipt_ref.clone());
                submission.brain_conversation = state.brain_conversation;
                apply_progress(&mut submission, &state.progress);
                if state.progress.phase == anda_kip::memory::binding::Phase::Recorded
                    && let Some(provenance) = &submission.provenance
                    && let Err(error) = self
                        .update_memory_session(
                            &provenance.caller,
                            &submission.bot_conversation.to_string(),
                            |session| {
                                Ok(session.record_receipt(
                                    crate::config::ANDA_BOT_SPACE_ID,
                                    &receipt_ref,
                                )?)
                            },
                        )
                        .await
                {
                    // The window row keeps the receipt; only recall's wait
                    // on it is lost.
                    log::warn!("formation {key}: receipt not kept for recall: {error}");
                }
            }
            Err(error) => {
                if matches!(
                    error.downcast_ref::<anda_brain::product::SourceAdmissionError>(),
                    Some(anda_brain::product::SourceAdmissionError::Suppressed)
                ) {
                    submission.state = FormationState::Suppressed;
                    submission.error = None;
                } else {
                    submission.state = FormationState::Failed;
                    submission.error = Some(error.to_string().chars().take(512).collect());
                    submission.failure_stage = Some(FormationFailure::SubmissionRejected);
                }
            }
        }
        submission.updated_at = Some(anda_engine::unix_ms());
        self.write(key, &submission).await?;
        match submission.state {
            FormationState::Suppressed => Ok(submission),
            _ if submission.brain_conversation.is_some() => Ok(submission),
            state => Err(format!("formation {key}: {state:?}").into()),
        }
    }

    async fn refresh_formation_unlocked(
        &self,
        client: &super::Client,
        submission: &mut FormationSubmission,
    ) -> Result<(), BoxError> {
        if matches!(
            submission.state,
            FormationState::Completed | FormationState::Failed | FormationState::Suppressed
        ) {
            return Ok(());
        }
        if let (Some(receipt), Some(host), Some(provenance)) = (
            &submission.receipt_ref,
            client.embedded_host(),
            &submission.provenance,
        ) {
            let state = host.memory_receipt(&provenance.caller, receipt).await?;
            submission.brain_conversation =
                state.brain_conversation.or(submission.brain_conversation);
            apply_progress(submission, &state.progress);
            submission.updated_at = Some(anda_engine::unix_ms());
            return self
                .write(
                    &format!(
                        "formation/{}/{}",
                        submission.bot_conversation, submission.window_start
                    ),
                    submission,
                )
                .await;
        }
        use anda_engine::memory::ConversationStatus;
        let id = submission
            .brain_conversation
            .ok_or("formation acceptance is unknown")?;
        let conversation = client.formation_conversation(id).await?;
        submission.state = match conversation.status {
            ConversationStatus::Submitted => FormationState::Accepted,
            ConversationStatus::Working | ConversationStatus::Idle => FormationState::Processing,
            ConversationStatus::Completed => FormationState::Completed,
            ConversationStatus::Failed | ConversationStatus::Cancelled => FormationState::Failed,
        };
        submission.error = conversation
            .failed_reason
            .map(|s| s.chars().take(512).collect());
        submission.updated_at = Some(anda_engine::unix_ms());
        submission.failure_stage =
            (submission.state == FormationState::Failed).then_some(FormationFailure::NativeFailed);
        self.write(
            &format!(
                "formation/{}/{}",
                submission.bot_conversation, submission.window_start
            ),
            submission,
        )
        .await
    }
}

/// A receipt's phase as the window's state: recorded is accepted, processed
/// or available is completed, failed stays failed with its reason.
fn apply_progress(
    submission: &mut FormationSubmission,
    progress: &anda_kip::memory::binding::Progress,
) {
    use anda_kip::memory::binding::Phase;
    submission.state = match progress.phase {
        Phase::Recorded => FormationState::Accepted,
        Phase::Processed | Phase::Available => FormationState::Completed,
        Phase::Failed => FormationState::Failed,
    };
    submission.error = (progress.phase == Phase::Failed).then(|| {
        progress
            .reason
            .as_deref()
            .unwrap_or("memory processing failed")
            .chars()
            .take(512)
            .collect()
    });
    submission.failure_stage = (progress.phase == Phase::Failed).then(|| {
        if submission.brain_conversation.is_some() {
            FormationFailure::NativeFailed
        } else {
            FormationFailure::SubmissionRejected
        }
    });
}

/// Prepared by the host when the model emits tool calls. Only a unique match
/// receives the model's call id; ambiguous parallel duplicates remain unassigned.
#[derive(Clone, Default)]
pub struct RecallTurn(pub Arc<parking_lot::Mutex<RecallTurnState>>);
#[derive(Default)]
pub struct RecallTurnState {
    pub conversation: u64,
    pub turn: String,
    pub calls: Vec<(serde_json::Value, Option<String>)>,
}
impl RecallTurn {
    pub fn prepare(&self, conversation: u64, calls: &[anda_core::ToolCall]) {
        let mut state = self.0.lock();
        state.conversation = conversation;
        state.turn = ic_auth_types::Xid::new().to_string();
        state.calls = calls
            .iter()
            .filter(|call| call.name == super::Client::NAME && call.result.is_none())
            .filter_map(|call| {
                serde_json::from_value::<anda_brain::types::RecallInput>(call.args.clone())
                    .ok()
                    .and_then(|args| serde_json::to_value(args).ok())
                    .map(|args| (args, call.call_id.clone()))
            })
            .collect();
    }
    pub fn identify(
        &self,
        args: &anda_brain::types::RecallInput,
    ) -> (Option<u64>, Option<String>, Option<String>) {
        let state = self.0.lock();
        let args = serde_json::to_value(args).unwrap_or_default();
        let matches: Vec<_> = state
            .calls
            .iter()
            .filter(|(input, _)| *input == args)
            .collect();
        (
            Some(state.conversation),
            Some(state.turn.clone()),
            if matches.len() == 1 {
                matches[0].1.clone()
            } else {
                None
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, response::IntoResponse, routing};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn submission() -> FormationSubmission {
        FormationSubmission {
            bot_conversation: 42,
            window_start: 0,
            window_end: 2,
            submitted_at: 1,
            observed_at: None,
            brain_conversation: None,
            state: FormationState::Pending,
            error: None,
            provenance: None,
            updated_at: None,
            failure_stage: None,
            receipt_ref: None,
            attempt: 0,
        }
    }
    #[tokio::test]
    async fn formation_acceptance_survives_restart_and_exact_status_is_read() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route("/formation", routing::post(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { axum::Json(json!({"result": AgentOutput { conversation: Some(7), ..Default::default() }})) }
        })).route("/conversations/7", routing::get(|| async {
            axum::Json(json!({"result": anda_engine::memory::Conversation { _id:7, status: anda_engine::memory::ConversationStatus::Completed, ..Default::default() }}))
        }));
        let url = crate::test_support::spawn_http_mock(app).await;
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let client = super::super::Client::new(url, None);
        let input = || anda_brain::types::FormationInputRef {
            messages: &[],
            context: &None,
            timestamp: &None,
        };
        let journal = Journal::new(store.clone());
        let accepted = journal
            .submit_formation(&client, submission(), input())
            .await
            .unwrap();
        assert_eq!(accepted.brain_conversation, Some(7));
        assert_eq!(accepted.state, FormationState::Accepted);
        drop(journal);
        let recovered = Journal::new(store)
            .submit_formation(&client, submission(), input())
            .await
            .unwrap();
        assert_eq!(recovered.state, FormationState::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn lost_acceptance_is_durable_and_never_blindly_resent() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route(
            "/formation",
            routing::post(move || {
                count.fetch_add(1, Ordering::SeqCst);
                async { "truncated response after acceptance" }
            }),
        );
        let client =
            super::super::Client::new(crate::test_support::spawn_http_mock(app).await, None);
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        for end in [2, 4] {
            let mut window = submission();
            window.window_end = end;
            let journal = Journal::new(store.clone());
            assert!(
                journal
                    .submit_formation(
                        &client,
                        window,
                        anda_brain::types::FormationInputRef {
                            messages: &[],
                            context: &None,
                            timestamp: &None
                        }
                    )
                    .await
                    .is_err()
            );
            let row: FormationSubmission = journal.read("formation/42/0").await.unwrap().unwrap();
            assert_eq!(row.state, FormationState::Unknown);
            assert!(row.brain_conversation.is_none());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    async fn confirmed_failure_retries(status: Option<http::StatusCode>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route(
            "/formation",
            routing::post(move || {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        let body = axum::Json(json!({"error": {"message": "queue full"}}));
                        return match status {
                            Some(status) => (status, body).into_response(),
                            None => body.into_response(),
                        };
                    }
                    axum::Json(json!({
                        "result": AgentOutput {
                            conversation: Some(8),
                            ..Default::default()
                        }
                    }))
                    .into_response()
                }
            }),
        );
        let client =
            super::super::Client::new(crate::test_support::spawn_http_mock(app).await, None);
        let journal = Journal::new(Arc::new(object_store::memory::InMemory::new()));
        let input = || anda_brain::types::FormationInputRef {
            messages: &[],
            context: &None,
            timestamp: &None,
        };

        assert!(
            journal
                .submit_formation(&client, submission(), input())
                .await
                .is_err()
        );
        let failed: FormationSubmission = journal.read("formation/42/0").await.unwrap().unwrap();
        assert_eq!(failed.state, FormationState::Failed);

        let mut expanded = submission();
        expanded.window_end = 4;
        let accepted = journal
            .submit_formation(&client, expanded, input())
            .await
            .unwrap();
        assert_eq!(accepted.state, FormationState::Accepted);
        assert_eq!(accepted.window_end, 4);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn confirmed_http_and_rpc_failures_remain_retryable() {
        confirmed_failure_retries(Some(http::StatusCode::SERVICE_UNAVAILABLE)).await;
        confirmed_failure_retries(None).await;
    }

    #[test]
    fn recall_turn_preserves_unique_call_identity_without_guessing_duplicates() {
        let trace = RecallTurn::default();
        let call = anda_core::ToolCall {
            name: super::super::Client::NAME.into(),
            args: json!({"query":"prior task","context":null,"budget":null}),
            call_id: Some("call-1".into()),
            ..Default::default()
        };
        let args = serde_json::from_value(call.args.clone()).unwrap();
        trace.prepare(9, std::slice::from_ref(&call));
        assert_eq!(trace.identify(&args).2.as_deref(), Some("call-1"));
        trace.prepare(9, &[call.clone(), call]);
        assert_eq!(trace.identify(&args).2, None);
    }
}
