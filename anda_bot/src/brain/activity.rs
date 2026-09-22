//! Rebuildable index over the journal; never an execution or retry queue.
use super::{
    Client, FormationFailure, FormationState, FormationSubmission, Journal, SourceMessageRef,
};
use crate::engine::ConversationsTool;
use anda_core::{BoxError, Principal};
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    query::{Filter, Query, RangeQuery},
    schema::{AndaDBSchema, FieldTyped, Fv},
};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default, Serialize, Deserialize, FieldTyped, AndaDBSchema)]
struct ActivityIndex {
    _id: u64,
    user: String,
    conversation: u64,
    journal_key: String,
    submitted_at: u64,
    #[serde(default)]
    native_id: String,
}

#[derive(Clone)]
pub struct ActivityStore {
    index: Arc<Collection>,
    journal: Journal,
    conversations: Arc<ConversationsTool>,
    client: Client,
    write_lock: Arc<tokio::sync::Mutex<()>>,
    initialized: Arc<AtomicBool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityQuery {
    pub conversation: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActivityPage {
    pub schema_version: u32,
    pub items: Vec<MemoryActivity>,
    pub next_cursor: Option<String>,
    pub complete: bool,
    pub partial_reason: Option<String>,
}

impl ActivityPage {
    pub fn render(&self) -> String {
        let mut text = "Memory processing / 记忆整理记录\n".to_string();
        for item in &self.items {
            text.push_str(&format!(
                "\n#{} · {} · {}{}",
                item.conversation,
                item.state_label(),
                item.id,
                if item.stale {
                    " (stale / 待刷新)"
                } else {
                    ""
                }
            ));
        }
        if self.items.is_empty() {
            text.push_str("\nNo visible processing records / 暂无可见的整理记录");
        }
        if let Some(reason) = &self.partial_reason {
            text.push_str(&format!("\nPartial inventory / 记录尚不完整: {reason}"));
        }
        if let Some(cursor) = &self.next_cursor {
            text.push_str(&format!("\nNext cursor / 后续游标: {cursor}"));
        }
        text.push_str("\n\nProcessing completion does not prove that every fact was saved. / 整理完成不代表每条事实已保存。");
        text
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryActivity {
    pub id: String,
    pub conversation: String,
    pub state: String,
    pub submitted_at: u64,
    pub last_checked_at: Option<u64>,
    pub stale: bool,
    pub source_messages: Vec<SourceMessageRef>,
    pub provenance_complete: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemorySource {
    pub kind: String,
    pub conversation: Option<String>,
    pub index: Option<String>,
    pub role: String,
    pub text: Option<String>,
    #[serde(default)]
    pub text_truncated: bool,
    pub source: String,
}

impl MemoryActivity {
    pub fn state_label(&self) -> &str {
        match self.state.as_str() {
            "recalled" => "Memory search returned / 记忆检索已返回",
            "recall_failed" => "Memory search incomplete / 记忆检索未完成",
            "submitting" => "Submitting / 正在提交",
            "suppressed" => "Source excluded / 来源已停止自动整理",
            "accepted" => "Queued / 等待整理",
            "processing" => "Processing / 正在整理",
            "completed" => "Conversation processed / 对话整理完成",
            "rejected" => "Submission rejected / 提交失败",
            "failed" => "Processing failed / 整理未完成",
            "legacy_unattributed" => "Historical record / 历史处理记录",
            _ => "Acceptance unknown / 接受结果待确认",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ActivityCursor {
    user: String,
    conversation: Option<String>,
    upper: u64,
    before: u64,
}

impl ActivityStore {
    pub async fn resolve_source(
        &self,
        caller: Principal,
        source: &anda_brain::product::RecordSource,
    ) -> Result<Option<MemorySource>, BoxError> {
        if source.product_operation.is_some() {
            if let Some(host) = self.client.embedded_host()
                && let Some(text) = host
                    .state
                    .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
                    .await?
                    .product_correction_source(caller, source)
                    .await?
            {
                return Ok(Some(MemorySource {
                    kind: "correction".into(),
                    conversation: None,
                    index: None,
                    role: "user".into(),
                    text_truncated: false,
                    text: Some(text),
                    source: "memory_change".into(),
                }));
            }
            return Ok(None);
        }
        let (Some(native), Some(index), Some(digest)) = (
            source.formation_conversation,
            source.message_index,
            source.payload_digest.as_ref(),
        ) else {
            return Ok(None);
        };
        let rows: Vec<ActivityIndex> = self
            .index
            .search_as(Query {
                search: None,
                filter: Some(Filter::And(vec![
                    Box::new(eq("native_id", Fv::Text(native.to_string()))),
                    Box::new(eq("user", Fv::Text(caller.to_string()))),
                ])),
                limit: Some(2),
            })
            .await?;
        if rows.len() != 1 {
            return Ok(None);
        }
        let Some(submission) = self
            .journal
            .read::<FormationSubmission>(&rows[0].journal_key)
            .await?
        else {
            return Ok(None);
        };
        if submission.brain_conversation != Some(native) {
            return Ok(None);
        }
        let Some(provenance) = submission.provenance else {
            return Ok(None);
        };
        if provenance.caller != caller.to_string() {
            return Ok(None);
        }
        let Some(reference) = provenance.source_messages.get(index) else {
            return Ok(None);
        };
        if reference.submitted_digest.as_ref() != Some(digest) {
            return Ok(None);
        }
        let conversation = self
            .conversations
            .conversations
            .get_conversation(submission.bot_conversation)
            .await?;
        if conversation.user != caller {
            return Ok(None);
        }
        let original = reference
            .index
            .parse::<usize>()
            .ok()
            .and_then(|i| conversation.messages.get(i));
        let Some(original) = original else {
            return Ok(None);
        };
        if anda_cognitive_nexus::content_digest(original)? != reference.content_digest {
            return Ok(None);
        }
        let text = serde_json::from_value::<anda_core::Message>(original.clone())
            .ok()
            .and_then(|m| m.text());
        let text_truncated = text.as_ref().is_some_and(|s| s.chars().count() > 4096);
        let text = text.map(|s| s.chars().take(4096).collect());
        Ok(Some(MemorySource {
            kind: "conversation".into(),
            conversation: Some(conversation._id.to_string()),
            index: Some(reference.index.clone()),
            role: reference.role.clone(),
            text_truncated,
            text,
            source: provenance.source,
        }))
    }

    pub async fn connect(
        db: Arc<AndaDB>,
        conversations: Arc<ConversationsTool>,
        journal: Journal,
        client: Client,
    ) -> Result<Self, BoxError> {
        let index = db
            .open_or_create_collection(
                ActivityIndex::schema()?,
                CollectionConfig {
                    name: "bot_memory_activity_v1".into(),
                    description: "Rebuildable memory processing index".into(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["user"]).await?;
                    collection.create_btree_index_nx(&["conversation"]).await?;
                    collection.create_btree_index_nx(&["journal_key"]).await?;
                    collection.create_btree_index_nx(&["native_id"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;
        Ok(Self {
            index,
            conversations,
            journal,
            client,
            write_lock: Default::default(),
            initialized: Default::default(),
        })
    }

    async fn index_submission(&self, key: &str, row: &FormationSubmission) -> Result<(), BoxError> {
        let conversation = self
            .conversations
            .conversations
            .get_conversation(row.bot_conversation)
            .await?;
        if row
            .provenance
            .as_ref()
            .is_some_and(|p| p.caller != conversation.user.to_string())
        {
            return Err("Formation ownership mismatch".into());
        }
        self.index_activity(ActivityIndex {
            _id: 0,
            user: conversation.user.to_string(),
            conversation: row.bot_conversation,
            journal_key: key.into(),
            submitted_at: row.submitted_at,
            native_id: row
                .brain_conversation
                .map(|id| id.to_string())
                .unwrap_or_default(),
        })
        .await
    }

    async fn index_activity(&self, row: ActivityIndex) -> Result<(), BoxError> {
        let _guard = self.write_lock.lock().await;
        let existing: Vec<ActivityIndex> = self
            .index
            .search_as(Query {
                search: None,
                filter: Some(eq("journal_key", Fv::Text(row.journal_key.clone()))),
                limit: Some(1),
            })
            .await?;
        if existing.is_empty() {
            self.index.add_from(&row).await?;
            self.index.flush(anda_engine::unix_ms()).await?;
        } else if !row.native_id.is_empty() && row.native_id != existing[0].native_id {
            self.index
                .update(
                    existing[0]._id,
                    std::collections::BTreeMap::from([(
                        "native_id".into(),
                        Fv::Text(row.native_id),
                    )]),
                )
                .await?;
            self.index.flush(anda_engine::unix_ms()).await?;
        }
        Ok(())
    }

    /// Scan incrementally in bounded memory. Listing order is not guaranteed by
    /// ObjectStore: a restart deliberately rescans, never skips by guessed key.
    pub async fn run(&self, cancel: CancellationToken) {
        loop {
            let result = tokio::select! { _ = cancel.cancelled() => break, result = self.reconcile() => result };
            if let Err(error) = result {
                log::warn!("Memory activity reconciliation incomplete: {error}");
            }
            tokio::select! { _ = cancel.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(5)) => {} }
        }
        if let Err(error) = self.index.flush(anda_engine::unix_ms()).await {
            log::warn!("Memory activity flush failed: {error}");
        }
    }

    pub(super) async fn reconcile(&self) -> Result<(), BoxError> {
        let store = self.journal.object_store();
        let prefix = object_store::path::Path::from("bot-brain/v1/formation/");
        let mut objects = store.list(Some(&prefix));
        let mut processed = 0usize;
        let mut complete = true;
        while let Some(object) = objects.try_next().await? {
            let Some(key) = object.location.as_ref().strip_prefix("bot-brain/v1/") else {
                continue;
            };
            let Some(mut row) = self.journal.read::<FormationSubmission>(key).await? else {
                continue;
            };
            if row.brain_conversation.is_some()
                && !matches!(
                    row.state,
                    FormationState::Completed | FormationState::Failed | FormationState::Suppressed
                )
            {
                // GET only. The native id, never a global high-water mark, is
                // the proof. Unknown submissions without an id are not retried.
                if !matches!(
                    tokio::time::timeout(
                        Duration::from_secs(10),
                        self.journal.refresh_formation(&self.client, &mut row)
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    complete = false;
                }
            }
            if let Err(error) = self.index_submission(key, &row).await {
                complete = false;
                log::debug!("Memory activity projection skipped: {error}");
            }
            processed += 1;
            if processed.is_multiple_of(20) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        let recall_prefix = object_store::path::Path::from("bot-brain/v1/recall/");
        let mut recalls = store.list(Some(&recall_prefix));
        while let Some(object) = recalls.try_next().await? {
            let Some(key) = object.location.as_ref().strip_prefix("bot-brain/v1/") else {
                continue;
            };
            let Some(row) = self.journal.read::<super::RecallDelivery>(key).await? else {
                continue;
            };
            let Some(id) = row.bot_conversation else {
                continue;
            };
            let Ok(conv) = self.conversations.conversations.get_conversation(id).await else {
                complete = false;
                continue;
            };
            if conv.user.to_string() != row.caller {
                continue;
            }
            if self
                .index_activity(ActivityIndex {
                    _id: 0,
                    user: row.caller,
                    conversation: id,
                    journal_key: key.into(),
                    submitted_at: row.delivered_at,
                    native_id: String::new(),
                })
                .await
                .is_err()
            {
                complete = false
            }
            processed += 1;
            if processed.is_multiple_of(20) {
                tokio::task::yield_now().await;
            }
        }
        self.journal.write("activity-checkpoint/v1", &serde_json::json!({"version":1,"completed_at":anda_engine::unix_ms(),"complete":complete})).await?;
        self.initialized.store(complete, Ordering::SeqCst);
        Ok(())
    }

    pub async fn page(
        &self,
        caller: Principal,
        query: ActivityQuery,
    ) -> Result<ActivityPage, BoxError> {
        let limit = query.limit.unwrap_or(20);
        if !(1..=50).contains(&limit) {
            return Err("invalid_request".into());
        }
        let conversation = query
            .conversation
            .as_ref()
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|_| "invalid_request")?;
        if let Some(id) = conversation {
            let conv = self
                .conversations
                .conversations
                .get_conversation(id)
                .await
                .map_err(|_| "not_found")?;
            if conv.user != caller {
                return Err("not_found".into());
            }
        }
        let user = caller.to_string();
        let cursor = match &query.cursor {
            Some(text) if text.len() <= 2048 => {
                let cursor: ActivityCursor =
                    serde_json::from_str(text).map_err(|_| "invalid_cursor")?;
                if cursor.user != user
                    || cursor.conversation != query.conversation
                    || cursor.before > cursor.upper.saturating_add(1)
                {
                    return Err("invalid_cursor".into());
                }
                cursor
            }
            Some(_) => return Err("invalid_cursor".into()),
            None => {
                let upper = self.index.max_document_id();
                ActivityCursor {
                    user: user.clone(),
                    conversation: query.conversation.clone(),
                    upper,
                    before: upper.saturating_add(1),
                }
            }
        };
        let mut filters = vec![
            Box::new(eq("user", Fv::Text(user))),
            Box::new(Filter::Field((
                "_id".into(),
                RangeQuery::Lt(Fv::U64(cursor.before)),
            ))),
        ];
        if let Some(id) = conversation {
            filters.push(Box::new(eq("conversation", Fv::U64(id))));
        }
        let mut ids = self
            .index
            .query_last_ids(Filter::And(filters), Some(limit))
            .await?;
        ids.sort_unstable_by(|a, b| b.cmp(a));
        let next_cursor = if ids.len() == limit {
            ids.last()
                .map(|id| {
                    serde_json::to_string(&ActivityCursor {
                        before: *id,
                        ..cursor
                    })
                })
                .transpose()?
        } else {
            None
        };
        let mut items = Vec::new();
        let mut partial = !self.initialized.load(Ordering::SeqCst);
        for id in ids {
            let row: ActivityIndex = self.index.get_as(id).await?;
            // Neither index ownership nor its cached state is authoritative.
            let conv = match self
                .conversations
                .conversations
                .get_conversation(row.conversation)
                .await
            {
                Ok(c) if c.user == caller => c,
                _ => {
                    partial = true;
                    continue;
                }
            };
            if row.journal_key.starts_with("recall/") {
                let Some(recall) = self
                    .journal
                    .read::<super::RecallDelivery>(&row.journal_key)
                    .await?
                else {
                    partial = true;
                    continue;
                };
                if recall.caller == caller.to_string() && recall.bot_conversation == Some(conv._id)
                {
                    items.push(MemoryActivity {
                        id: row.journal_key,
                        conversation: conv._id.to_string(),
                        state: if recall.failed {
                            "recall_failed"
                        } else {
                            "recalled"
                        }
                        .into(),
                        submitted_at: recall.delivered_at,
                        last_checked_at: Some(recall.delivered_at),
                        stale: false,
                        source_messages: vec![],
                        provenance_complete: false,
                    });
                }
                continue;
            }
            let Some(submission) = self
                .journal
                .read::<FormationSubmission>(&row.journal_key)
                .await?
            else {
                partial = true;
                continue;
            };
            if submission.bot_conversation != conv._id
                || submission
                    .provenance
                    .as_ref()
                    .is_some_and(|p| p.caller != caller.to_string())
            {
                partial = true;
                continue;
            }
            let mut view = project(&row.journal_key, &submission);
            if submission.state == FormationState::Pending
                && submission.provenance.is_some()
                && self.journal.submission_in_flight(&row.journal_key)
            {
                view.state = "submitting".into();
            }
            // A changed source is no longer a valid message-level receipt.
            view.source_messages.retain(|source| {
                source
                    .index
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| conv.messages.get(i))
                    .and_then(|v| anda_cognitive_nexus::content_digest(v).ok())
                    .is_some_and(|digest| digest == source.content_digest)
            });
            if view.source_messages.len()
                != submission
                    .provenance
                    .as_ref()
                    .map_or(0, |p| p.source_messages.len())
            {
                view.provenance_complete = false;
            }
            items.push(view);
        }
        Ok(ActivityPage {
            schema_version: 1,
            items,
            complete: next_cursor.is_none() && !partial,
            next_cursor,
            partial_reason: partial.then(|| "projection_incomplete".into()),
        })
    }
}

fn eq(field: &str, value: Fv) -> Filter {
    Filter::Field((field.into(), RangeQuery::Eq(value)))
}

fn project(key: &str, row: &FormationSubmission) -> MemoryActivity {
    let state = if row.provenance.is_none() {
        "legacy_unattributed"
    } else {
        match row.state {
            FormationState::Pending | FormationState::Unknown => "unknown",
            FormationState::Accepted => "accepted",
            FormationState::Suppressed => "suppressed",
            FormationState::Processing => "processing",
            FormationState::Completed => "completed",
            FormationState::Failed
                if row.brain_conversation.is_none()
                    && matches!(
                        row.failure_stage,
                        Some(FormationFailure::SubmissionRejected)
                    ) =>
            {
                "rejected"
            }
            FormationState::Failed => "failed",
        }
    };
    MemoryActivity {
        id: key.into(),
        conversation: row.bot_conversation.to_string(),
        state: state.into(),
        submitted_at: row.submitted_at,
        last_checked_at: row.updated_at,
        stale: !matches!(
            row.state,
            FormationState::Completed | FormationState::Failed | FormationState::Suppressed
        ) && row
            .updated_at
            .is_none_or(|t| anda_engine::unix_ms().saturating_sub(t) > 30_000),
        source_messages: row
            .provenance
            .as_ref()
            .map(|p| p.source_messages.clone())
            .unwrap_or_default(),
        provenance_complete: row.provenance.as_ref().is_some_and(|p| {
            p.version == 1 && p.input_digest.is_some() && !p.source_messages.is_empty()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::{FormationProvenance, SourceMessageRef};
    use anda_engine::memory::{Conversation, ConversationRef};
    use serde_json::json;

    async fn fixture() -> (ActivityStore, Arc<AndaDB>, Principal, u64) {
        let db = crate::test_support::memory_db("memory_activity").await;
        let conversations = Arc::new(
            ConversationsTool::connect(db.clone(), "bot".into(), "/tmp".into())
                .await
                .unwrap(),
        );
        let owner = crate::identity::Ed25519Key::new([75; 32]).id();
        let mut conv = Conversation {
            user: owner,
            ..Default::default()
        };
        conv.append_messages(vec![anda_core::Message {
            role: "user".into(),
            content: vec!["source preference".to_string().into()],
            ..Default::default()
        }]);
        let id = conversations
            .conversations
            .add_conversation(ConversationRef::from(&conv))
            .await
            .unwrap();
        let journal = Journal::new(db.object_store());
        let store = ActivityStore::connect(
            db.clone(),
            conversations,
            journal,
            Client::new("http://127.0.0.1:0".into(), None),
        )
        .await
        .unwrap();
        (store, db, owner, id)
    }

    fn submission(id: u64) -> FormationSubmission {
        FormationSubmission {
            bot_conversation: id,
            window_start: 0,
            window_end: 1,
            submitted_at: 1,
            brain_conversation: None,
            state: FormationState::Unknown,
            error: None,
            provenance: None,
            updated_at: None,
            failure_stage: None,
        }
    }

    #[tokio::test]
    async fn memory_activity_rebuilds_legacy_rows_but_never_assigns_fact_receipts() {
        let (store, db, owner, id) = fixture().await;
        let key = format!("formation/{id}/0");
        // Original JSON lacks all new optional fields.
        store.journal.write(&key,&json!({"bot_conversation":id,"window_start":0,"window_end":1,"submitted_at":1,"brain_conversation":null,"state":"completed","error":null})).await.unwrap();
        store.reconcile().await.unwrap();
        let reconnected = ActivityStore::connect(
            db,
            store.conversations.clone(),
            store.journal.clone(),
            store.client.clone(),
        )
        .await
        .unwrap();
        let page = reconnected
            .page(owner, ActivityQuery::default())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].state, "legacy_unattributed");
        assert!(!page.items[0].provenance_complete);
        assert!(page.items[0].source_messages.is_empty());
        assert!(
            !page.complete,
            "a new process has not completed its reconciliation scan"
        );
    }

    #[tokio::test]
    async fn memory_activity_enforces_caller_cursor_and_live_source_ownership() {
        let (store, _, owner, id) = fixture().await;
        let other = crate::identity::Ed25519Key::new([76; 32]).id();
        store
            .journal
            .write(&format!("formation/{id}/0"), &submission(id))
            .await
            .unwrap();
        store.reconcile().await.unwrap();
        let page = store
            .page(
                owner,
                ActivityQuery {
                    limit: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(page.next_cursor.is_some());
        assert!(
            store
                .page(
                    other,
                    ActivityQuery {
                        conversation: Some(id.to_string()),
                        ..Default::default()
                    }
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("not_found")
        );
        assert!(
            store
                .page(
                    other,
                    ActivityQuery {
                        cursor: page.next_cursor,
                        ..Default::default()
                    }
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("invalid_cursor")
        );
        store
            .conversations
            .conversations
            .delete_conversation(id)
            .await
            .unwrap();
        assert!(
            store
                .page(owner, ActivityQuery::default())
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[tokio::test]
    async fn memory_activity_checks_source_digest_and_keeps_unknown_submission_unsent() {
        let (store, _, owner, id) = fixture().await;
        let mut row = submission(id);
        row.provenance = Some(FormationProvenance {
            policy_revision: None,
            version: 1,
            caller: owner.to_string(),
            session: None,
            source_identity: None,
            source: "cli".into(),
            reply_target: None,
            thread: None,
            external_user: false,
            counterparty: Some(owner.to_string()),
            source_messages: vec![SourceMessageRef {
                conversation: id.to_string(),
                index: "0".into(),
                role: "user".into(),
                content_digest: "changed".into(),
                submitted_digest: None,
            }],
            input_digest: Some("input".into()),
        });
        store
            .journal
            .write(&format!("formation/{id}/0"), &row)
            .await
            .unwrap();
        // No native ID: reconciliation succeeds even though its HTTP client
        // cannot connect. There must be no submission or status request.
        store.reconcile().await.unwrap();
        let page = store.page(owner, ActivityQuery::default()).await.unwrap();
        assert_eq!(page.items[0].state, "unknown");
        assert!(page.items[0].source_messages.is_empty());
        assert!(!page.items[0].provenance_complete);
    }
}
