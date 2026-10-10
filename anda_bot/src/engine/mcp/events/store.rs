//! Event triggers, the events they received and their runs, in AndaDB.

use anda_core::BoxError;
use anda_db::{
    collection::{Collection, CollectionConfig},
    database::AndaDB,
    error::DBError,
    query::{Filter, RangeQuery},
    schema::{AndaDBSchema, Fv},
    unix_ms,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc};

use crate::cron::{CronJobOrigin, CronJobResult};

/// Triggers one daemon keeps.
pub(crate) const MAX_TRIGGERS: usize = 64;
pub(crate) const MAX_NAME_CHARS: usize = 100;
pub(crate) const MAX_INSTRUCTIONS_BYTES: usize = 8 * 1024;
pub(crate) const MAX_ARGUMENTS_BYTES: usize = 16 * 1024;
/// Event data stored past this is replaced by a truncated preview.
pub(crate) const MAX_STORED_EVENT_BYTES: usize = 64 * 1024;
/// Window in which events are collected into one run.
pub(crate) const DEFAULT_BATCH_WINDOW_SECS: u64 = 30;
pub(crate) const MAX_BATCH_WINDOW_SECS: u64 = 3_600;
/// Runs within an hour after which a trigger pauses itself.
pub(crate) const DEFAULT_MAX_RUNS_PER_HOUR: u64 = 12;
pub(crate) const MAX_RUNS_PER_HOUR: u64 = 120;
/// Processed events and runs kept per trigger; older ones are pruned.
const MAX_RECORDS_PER_TRIGGER: usize = 500;
const MAX_RUNS_PER_TRIGGER: usize = 100;
/// Events waiting for a run, per trigger; past it the oldest are dropped.
pub(crate) const MAX_PENDING_EVENTS: usize = 500;

/// How a trigger receives its events.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TriggerDelivery {
    /// Push, then poll, then a webhook through the dMsg ingress.
    #[default]
    Auto,
    Poll,
    Push,
    Webhook,
}

impl TriggerDelivery {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Poll => "poll",
            Self::Push => "push",
            Self::Webhook => "webhook",
        }
    }
}

/// Where a trigger stands; recomputed by the runtime, stored for display.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TriggerState {
    /// Being subscribed.
    #[default]
    Starting,
    /// Subscribed and receiving events.
    Active,
    /// Subscribed, but the last attempt failed; it is retried.
    Retrying,
    /// Turned off by the user, or paused by its run limits.
    Paused,
    /// The server is removed, disabled or failing; resumes when it is back.
    Waiting,
    /// The server needs the user to sign in.
    NeedsAuth,
    /// Only a webhook delivers the event, and no dMsg ingress is available.
    NeedsIngress,
    /// The server ended the subscription; it needs a change or a resume.
    Ended,
}

/// A webhook subscription received through the dMsg ingress.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WebhookState {
    /// The MCP server that relays the deliveries (dMsg).
    pub ingress: String,
    pub endpoint_id: String,
    pub url: String,
    /// Name of the endpoint's signing secret in the MCP secret store.
    pub secret_name: String,
    #[serde(default)]
    pub subscription_id: Option<String>,
    /// Unix ms to subscribe again by; `None` when it does not expire.
    #[serde(default)]
    pub refresh_before: Option<u64>,
    /// Unix ms of the last subscribe.
    #[serde(default)]
    pub refreshed_at: Option<u64>,
    /// Position in the ingress's relay stream.
    #[serde(default)]
    pub relay_cursor: Option<String>,
}

/// An automation: when `server_id` reports `event` with `arguments`, the
/// agent runs `instructions` on the events, on the `origin` route.
#[derive(Clone, Debug, Serialize, Deserialize, AndaDBSchema)]
pub(crate) struct EventTrigger {
    pub _id: u64,
    pub name: String,
    pub server_id: String,
    pub event: String,
    /// Subscription arguments, a JSON object.
    #[field_type = "Json"]
    pub arguments: Value,
    pub instructions: String,
    #[field_type = "Option<Map<Text, Json>>"]
    pub origin: Option<CronJobOrigin>,
    #[field_type = "Text"]
    pub delivery: TriggerDelivery,
    pub batch_window_secs: u64,
    pub max_runs_per_hour: u64,
    pub enabled: bool,
    /// `owner` (the apps and the CLI) or `model`.
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Where the event subscription resumes on the server.
    pub cursor: Option<String>,
    #[field_type = "Option<Map<Text, Json>>"]
    pub webhook: Option<WebhookState>,
    #[field_type = "Text"]
    pub state: TriggerState,
    /// The delivery mode in use.
    pub mode: Option<String>,
    pub last_error: Option<String>,
    pub last_event_at: Option<u64>,
    pub last_run_at: Option<u64>,
    /// When the server last said events were lost.
    pub missed_events_at: Option<u64>,
    pub events_received: u64,
    pub runs: u64,
    pub last_conversation_id: Option<u64>,
}

impl EventTrigger {
    pub(crate) fn arguments(&self) -> Map<String, Value> {
        self.arguments.as_object().cloned().unwrap_or_default()
    }

    /// What subscribing depends on; a change resubscribes.
    pub(crate) fn subscription_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.server_id,
            self.event,
            self.arguments,
            self.delivery.as_str()
        )
    }
}

/// One event a trigger received.
#[derive(Clone, Debug, Serialize, Deserialize, AndaDBSchema)]
pub(crate) struct EventRecord {
    pub _id: u64,
    /// `{trigger_id}:{event_id}`: an event is stored once per trigger.
    #[unique]
    pub key: String,
    pub trigger_id: u64,
    pub event_id: String,
    pub name: String,
    pub timestamp: String,
    /// The event's data, untrusted; a truncated preview when it was large.
    #[field_type = "Json"]
    pub data: Value,
    pub received_at: u64,
    /// The run that handled it; 0 while it waits.
    pub run_id: u64,
    /// How the event was verified on its way in: `v1a` (end to end), `v1`
    /// (by the relay), or none for poll and push.
    pub verified: Option<String>,
}

/// One run of a trigger.
#[derive(Clone, Debug, Default, Serialize, Deserialize, AndaDBSchema)]
pub(crate) struct TriggerRun {
    pub _id: u64,
    pub trigger_id: u64,
    pub started_at: u64,
    pub finished_at: u64,
    pub events: u64,
    pub result: Option<String>,
    pub error: Option<String>,
    pub conversation_id: Option<u64>,
}

/// An event to store.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NewEvent {
    pub event_id: String,
    pub name: String,
    pub timestamp: String,
    pub data: Value,
    pub verified: Option<String>,
}

#[derive(Clone)]
pub(crate) struct TriggerStore {
    triggers: Arc<Collection>,
    events: Arc<Collection>,
    runs: Arc<Collection>,
    /// Serializes read-modify-write of triggers.
    mutations: Arc<tokio::sync::Mutex<()>>,
}

impl TriggerStore {
    pub(crate) async fn connect(db: Arc<AndaDB>) -> Result<Self, BoxError> {
        let triggers = db
            .open_or_create_collection(
                EventTrigger::schema()?,
                CollectionConfig {
                    name: "mcp_event_triggers".to_string(),
                    description: "Automations run by MCP server events".to_string(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["server_id"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;
        let events = db
            .open_or_create_collection(
                EventRecord::schema()?,
                CollectionConfig {
                    name: "mcp_event_inbox".to_string(),
                    description: "Events received by MCP event automations".to_string(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["key"]).await?;
                    collection.create_btree_index_nx(&["trigger_id"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;
        let runs = db
            .open_or_create_collection(
                TriggerRun::schema()?,
                CollectionConfig {
                    name: "mcp_event_runs".to_string(),
                    description: "Runs of MCP event automations".to_string(),
                },
                async |collection| {
                    collection.create_btree_index_nx(&["trigger_id"]).await?;
                    Ok::<(), DBError>(())
                },
            )
            .await?;
        Ok(Self {
            triggers,
            events,
            runs,
            mutations: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub(crate) async fn insert(&self, mut trigger: EventTrigger) -> Result<EventTrigger, BoxError> {
        let _guard = self.mutations.lock().await;
        if self.list_unlocked().await?.len() >= MAX_TRIGGERS {
            return Err(format!("at most {MAX_TRIGGERS} MCP event automations can be kept").into());
        }
        trigger._id = self.triggers.add_from(&trigger).await?;
        self.triggers.flush(unix_ms()).await?;
        Ok(trigger)
    }

    pub(crate) async fn get(&self, id: u64) -> Result<EventTrigger, BoxError> {
        get_existing(&self.triggers, id)
            .await?
            .ok_or_else(|| format!("MCP event automation {id} not found").into())
    }

    /// Every trigger, oldest first.
    pub(crate) async fn list(&self) -> Result<Vec<EventTrigger>, BoxError> {
        self.list_unlocked().await
    }

    async fn list_unlocked(&self) -> Result<Vec<EventTrigger>, BoxError> {
        let ids = self
            .triggers
            .query_ids(
                Filter::Field(("_id".to_string(), RangeQuery::Ge(Fv::U64(0)))),
                None,
            )
            .await?;
        let mut triggers = Vec::with_capacity(ids.len());
        for id in ids {
            triggers.extend(get_existing(&self.triggers, id).await?);
        }
        Ok(triggers)
    }

    /// Changes a trigger under the store's lock and saves the fields that
    /// changed. `Ok(None)` when it no longer exists.
    pub(crate) async fn modify<R>(
        &self,
        id: u64,
        change: impl FnOnce(&mut EventTrigger) -> Result<R, BoxError>,
    ) -> Result<Option<(EventTrigger, R)>, BoxError> {
        let _guard = self.mutations.lock().await;
        let Some(before) = get_existing::<EventTrigger>(&self.triggers, id).await? else {
            return Ok(None);
        };
        let mut trigger = before.clone();
        let result = change(&mut trigger)?;
        let mut patch = patch_of(&self.triggers, &trigger)?;
        let previous = patch_of(&self.triggers, &before)?;
        patch.retain(|key, value| previous.get(key) != Some(value));
        if !patch.is_empty() {
            self.triggers.update(id, patch).await?;
            self.triggers.flush(unix_ms()).await?;
        }
        Ok(Some((trigger, result)))
    }

    /// Removes a trigger and the events it has not handled; its runs stay.
    pub(crate) async fn remove(&self, id: u64) -> Result<bool, BoxError> {
        let _guard = self.mutations.lock().await;
        if self.triggers.remove(id).await?.is_none() {
            return Ok(false);
        }
        for record in self.records_of(id).await? {
            self.events.remove(record._id).await?;
        }
        let now_ms = unix_ms();
        self.triggers.flush(now_ms).await?;
        self.events.flush(now_ms).await?;
        Ok(true)
    }

    /// Stores the events a trigger has not seen; returns how many were new.
    pub(crate) async fn record_events(
        &self,
        trigger_id: u64,
        events: Vec<NewEvent>,
    ) -> Result<usize, BoxError> {
        let now_ms = unix_ms();
        let mut added = 0;
        for event in events {
            let record = EventRecord {
                _id: 0,
                key: format!("{trigger_id}:{}", event.event_id),
                trigger_id,
                event_id: event.event_id,
                name: event.name,
                timestamp: event.timestamp,
                data: bounded_data(event.data),
                received_at: now_ms,
                run_id: 0,
                verified: event.verified,
            };
            match self.events.add_from(&record).await {
                Ok(_) => added += 1,
                // Delivered before: at least once means this happens.
                Err(err) if err.unique_index_conflict().is_some() => {}
                Err(err) => return Err(err.into()),
            }
        }
        if added > 0 {
            self.drop_overflow(trigger_id).await?;
            self.events.flush(now_ms).await?;
        }
        Ok(added)
    }

    /// Drops the oldest waiting events past [`MAX_PENDING_EVENTS`]; returns
    /// whether any were dropped.
    async fn drop_overflow(&self, trigger_id: u64) -> Result<bool, BoxError> {
        let pending = self.pending(trigger_id, usize::MAX).await?;
        let excess = pending.len().saturating_sub(MAX_PENDING_EVENTS);
        for record in &pending[..excess] {
            self.events.remove(record._id).await?;
        }
        Ok(excess > 0)
    }

    async fn records_of(&self, trigger_id: u64) -> Result<Vec<EventRecord>, BoxError> {
        let ids = self.events.query_ids(of_trigger(trigger_id), None).await?;
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            records.extend(get_existing(&self.events, id).await?);
        }
        Ok(records)
    }

    /// The oldest events waiting for a run, up to `limit`.
    pub(crate) async fn pending(
        &self,
        trigger_id: u64,
        limit: usize,
    ) -> Result<Vec<EventRecord>, BoxError> {
        let mut pending: Vec<_> = self
            .records_of(trigger_id)
            .await?
            .into_iter()
            .filter(|record| record.run_id == 0)
            .collect();
        pending.sort_by_key(|record| record._id);
        pending.truncate(limit);
        Ok(pending)
    }

    /// The latest events a trigger received, newest first.
    pub(crate) async fn recent_events(
        &self,
        trigger_id: u64,
        limit: usize,
    ) -> Result<Vec<EventRecord>, BoxError> {
        let ids = self
            .events
            .query_last_ids(of_trigger(trigger_id), Some(limit))
            .await?;
        let mut records = Vec::with_capacity(ids.len());
        for id in ids.into_iter().rev() {
            records.extend(get_existing(&self.events, id).await?);
        }
        Ok(records)
    }

    pub(crate) async fn start_run(
        &self,
        trigger_id: u64,
        events: usize,
    ) -> Result<TriggerRun, BoxError> {
        let mut run = TriggerRun {
            trigger_id,
            started_at: unix_ms(),
            events: events as u64,
            ..Default::default()
        };
        run._id = self.runs.add_from(&run).await?;
        self.runs.flush(unix_ms()).await?;
        Ok(run)
    }

    /// Records a finished run and marks its events handled.
    pub(crate) async fn finish_run(
        &self,
        mut run: TriggerRun,
        events: &[EventRecord],
        result: &CronJobResult,
    ) -> Result<TriggerRun, BoxError> {
        let now_ms = unix_ms();
        run.finished_at = now_ms;
        run.result = result.result.as_deref().map(preview);
        run.error = result.error.as_deref().map(preview);
        run.conversation_id = result.conversation_id;
        let patch = patch_of(&self.runs, &run)?;
        self.runs.update(run._id, patch).await?;
        for record in events {
            match self
                .events
                .update(
                    record._id,
                    BTreeMap::from([("run_id".to_string(), Fv::U64(run._id))]),
                )
                .await
            {
                Ok(_) | Err(DBError::NotFound { .. }) => {}
                Err(err) => return Err(err.into()),
            }
        }
        self.prune(run.trigger_id).await?;
        self.runs.flush(now_ms).await?;
        self.events.flush(now_ms).await?;
        Ok(run)
    }

    /// Runs of a trigger, newest first.
    pub(crate) async fn list_runs(
        &self,
        trigger_id: u64,
        limit: usize,
    ) -> Result<Vec<TriggerRun>, BoxError> {
        let ids = self
            .runs
            .query_last_ids(of_trigger(trigger_id), Some(limit))
            .await?;
        let mut runs = Vec::with_capacity(ids.len());
        for id in ids.into_iter().rev() {
            runs.extend(get_existing(&self.runs, id).await?);
        }
        Ok(runs)
    }

    /// Start times of the trigger's runs since `since_ms`.
    pub(crate) async fn runs_since(
        &self,
        trigger_id: u64,
        since_ms: u64,
    ) -> Result<usize, BoxError> {
        Ok(self
            .list_runs(trigger_id, MAX_RUNS_PER_HOUR as usize + 1)
            .await?
            .iter()
            .filter(|run| run.started_at >= since_ms)
            .count())
    }

    async fn prune(&self, trigger_id: u64) -> Result<(), BoxError> {
        let handled: Vec<_> = self
            .records_of(trigger_id)
            .await?
            .into_iter()
            .filter(|record| record.run_id != 0)
            .collect();
        let excess = handled.len().saturating_sub(MAX_RECORDS_PER_TRIGGER);
        for record in &handled[..excess] {
            self.events.remove(record._id).await?;
        }
        let ids = self.runs.query_ids(of_trigger(trigger_id), None).await?;
        let excess = ids.len().saturating_sub(MAX_RUNS_PER_TRIGGER);
        for id in &ids[..excess] {
            self.runs.remove(*id).await?;
        }
        Ok(())
    }
}

fn of_trigger(trigger_id: u64) -> Filter {
    Filter::Field((
        "trigger_id".to_string(),
        RangeQuery::Eq(Fv::U64(trigger_id)),
    ))
}

/// Reads a document that may have been removed concurrently.
async fn get_existing<T: DeserializeOwned>(
    collection: &Collection,
    id: u64,
) -> Result<Option<T>, BoxError> {
    match collection.get_as(id).await {
        Ok(doc) => Ok(Some(doc)),
        Err(DBError::NotFound { .. }) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Every field of `doc` but `_id`, typed by the collection's schema.
fn patch_of<T: Serialize>(
    collection: &Collection,
    doc: &T,
) -> Result<BTreeMap<String, Fv>, BoxError> {
    let schema = collection.schema();
    let Value::Object(fields) = serde_json::to_value(doc)? else {
        return Err("document must serialize to an object".into());
    };
    let mut patch = BTreeMap::new();
    for (name, value) in fields {
        if name == "_id" {
            continue;
        }
        let field_type = schema.get_field(&name).map(|field| field.r#type());
        patch.insert(name, Fv::serialized(&value, field_type)?);
    }
    Ok(patch)
}

/// Large event data is kept as a preview of its JSON text.
fn bounded_data(data: Value) -> Value {
    let text = serde_json::to_string(&data).unwrap_or_default();
    if text.len() <= MAX_STORED_EVENT_BYTES {
        return data;
    }
    let mut end = MAX_STORED_EVENT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    serde_json::json!({
        "truncated": true,
        "bytes": text.len(),
        "preview": &text[..end],
    })
}

/// Bounded text for a run's result and error.
fn preview(text: &str) -> String {
    match text.char_indices().nth(4_000) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn trigger(server_id: &str, event: &str) -> EventTrigger {
        EventTrigger {
            _id: 0,
            name: format!("{event} on {server_id}"),
            server_id: server_id.to_string(),
            event: event.to_string(),
            arguments: serde_json::json!({"repo": "ldclabs/anda"}),
            instructions: "Label it.".to_string(),
            origin: None,
            delivery: TriggerDelivery::Auto,
            batch_window_secs: 0,
            max_runs_per_hour: DEFAULT_MAX_RUNS_PER_HOUR,
            enabled: true,
            created_by: "owner".to_string(),
            created_at: 1,
            updated_at: 1,
            cursor: None,
            webhook: None,
            state: TriggerState::Starting,
            mode: None,
            last_error: None,
            last_event_at: None,
            last_run_at: None,
            missed_events_at: None,
            events_received: 0,
            runs: 0,
            last_conversation_id: None,
        }
    }

    fn event(id: &str) -> NewEvent {
        NewEvent {
            event_id: id.to_string(),
            name: "issue.opened".to_string(),
            timestamp: "2026-10-10T00:00:00Z".to_string(),
            data: serde_json::json!({"id": id}),
            verified: None,
        }
    }

    async fn store() -> TriggerStore {
        let db = crate::test_support::memory_db("mcp_events").await;
        TriggerStore::connect(db).await.unwrap()
    }

    #[tokio::test]
    async fn events_are_stored_once_and_handled_by_runs() {
        let store = store().await;
        let first = store
            .insert(trigger("github", "issue.opened"))
            .await
            .unwrap();
        let second = store.insert(trigger("github", "pr.opened")).await.unwrap();
        assert_eq!(
            store
                .record_events(first._id, vec![event("e1"), event("e2"), event("e1")])
                .await
                .unwrap(),
            2
        );
        // A redelivery is skipped; another trigger stores the same id anew.
        assert_eq!(
            store
                .record_events(first._id, vec![event("e2")])
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .record_events(second._id, vec![event("e1")])
                .await
                .unwrap(),
            1
        );

        let pending = store.pending(first._id, 10).await.unwrap();
        assert_eq!(
            pending
                .iter()
                .map(|r| r.event_id.as_str())
                .collect::<Vec<_>>(),
            ["e1", "e2"]
        );
        let run = store.start_run(first._id, pending.len()).await.unwrap();
        let run = store
            .finish_run(
                run,
                &pending,
                &CronJobResult {
                    result: Some("labelled".into()),
                    conversation_id: Some(9),
                    error: None,
                },
            )
            .await
            .unwrap();
        assert!(store.pending(first._id, 10).await.unwrap().is_empty());
        assert_eq!(store.pending(second._id, 10).await.unwrap().len(), 1);
        let runs = store.list_runs(first._id, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]._id, run._id);
        assert_eq!(runs[0].events, 2);
        assert_eq!(runs[0].conversation_id, Some(9));
        assert_eq!(
            store.recent_events(first._id, 1).await.unwrap()[0].event_id,
            "e2"
        );

        // Removing a trigger drops its events.
        assert!(store.remove(first._id).await.unwrap());
        assert!(store.recent_events(first._id, 10).await.unwrap().is_empty());
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn modify_saves_only_what_changed() {
        let store = store().await;
        let trigger = store
            .insert(trigger("github", "issue.opened"))
            .await
            .unwrap();
        let (updated, _) = store
            .modify(trigger._id, |trigger| {
                trigger.cursor = Some("c9".into());
                trigger.state = TriggerState::Active;
                trigger.webhook = Some(WebhookState {
                    ingress: "dmsg".into(),
                    endpoint_id: "ep1".into(),
                    url: "https://hooks.example/1".into(),
                    secret_name: "S".into(),
                    ..Default::default()
                });
                trigger.arguments = serde_json::json!({"repo": "ldclabs/anda-bot"});
                Ok(())
            })
            .await
            .unwrap()
            .unwrap();
        let stored = store.get(trigger._id).await.unwrap();
        assert_eq!(stored.cursor.as_deref(), Some("c9"));
        assert_eq!(stored.state, TriggerState::Active);
        assert_eq!(stored.webhook, updated.webhook);
        assert_eq!(stored.arguments["repo"], "ldclabs/anda-bot");
        assert!(store.modify(999, |_| Ok(())).await.unwrap().is_none());
    }

    #[test]
    fn large_event_data_is_kept_as_a_preview() {
        let data = bounded_data(serde_json::json!({"body": "x".repeat(MAX_STORED_EVENT_BYTES)}));
        assert_eq!(data["truncated"], true);
        assert!(data["preview"].as_str().unwrap().len() <= MAX_STORED_EVENT_BYTES);
        assert_eq!(
            bounded_data(serde_json::json!({"a": 1})),
            serde_json::json!({"a": 1})
        );
    }
}
