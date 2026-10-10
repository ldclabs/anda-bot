//! Creating and changing event triggers, and how they are shown. The API,
//! the CLI and the model tools all go through here.

use anda_core::BoxError;
use anda_engine::{extension::mcp::McpEventDefinition, unix_ms};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{runtime::McpEventRuntime, store::*};
use crate::{cron::CronJobOrigin, engine::mcp::McpError};

/// Longest event name accepted.
const MAX_EVENT_NAME_BYTES: usize = 256;
/// Event data shown per event in a trigger's detail.
const VIEW_EVENT_BYTES: usize = 2 * 1024;

/// A new trigger.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TriggerInput {
    pub server_id: String,
    pub event: String,
    #[serde(default)]
    pub arguments: Map<String, Value>,
    pub instructions: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub delivery: Option<TriggerDelivery>,
    #[serde(default)]
    pub batch_window_secs: Option<u64>,
    #[serde(default)]
    pub max_runs_per_hour: Option<u64>,
}

/// Changes to a trigger; what is left out stays.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TriggerPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<Map<String, Value>>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub delivery: Option<TriggerDelivery>,
    #[serde(default)]
    pub batch_window_secs: Option<u64>,
    #[serde(default)]
    pub max_runs_per_hour: Option<u64>,
}

impl TriggerPatch {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.arguments.is_none()
            && self.instructions.is_none()
            && self.delivery.is_none()
            && self.batch_window_secs.is_none()
            && self.max_runs_per_hour.is_none()
    }
}

fn name_of(name: &str) -> Result<String, BoxError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(McpError::invalid(format!(
            "an automation name must be 1 to {MAX_NAME_CHARS} characters"
        )));
    }
    Ok(name.to_string())
}

fn instructions_of(instructions: &str) -> Result<String, BoxError> {
    let instructions = instructions.trim();
    if instructions.is_empty() || instructions.len() > MAX_INSTRUCTIONS_BYTES {
        return Err(McpError::invalid(format!(
            "instructions must be 1 to {} KiB",
            MAX_INSTRUCTIONS_BYTES / 1024
        )));
    }
    Ok(instructions.to_string())
}

fn arguments_of(arguments: Map<String, Value>) -> Result<Value, BoxError> {
    let arguments = Value::Object(arguments);
    if serde_json::to_vec(&arguments)?.len() > MAX_ARGUMENTS_BYTES {
        return Err(McpError::invalid(format!(
            "the event arguments are longer than {} KiB",
            MAX_ARGUMENTS_BYTES / 1024
        )));
    }
    Ok(arguments)
}

fn window_of(secs: u64) -> Result<u64, BoxError> {
    if secs > MAX_BATCH_WINDOW_SECS {
        return Err(McpError::invalid(format!(
            "batch_window_secs must be at most {MAX_BATCH_WINDOW_SECS}"
        )));
    }
    Ok(secs)
}

fn runs_of(runs: u64) -> Result<u64, BoxError> {
    if !(1..=MAX_RUNS_PER_HOUR).contains(&runs) {
        return Err(McpError::invalid(format!(
            "max_runs_per_hour must be 1 to {MAX_RUNS_PER_HOUR}"
        )));
    }
    Ok(runs)
}

/// Parses event arguments written as a JSON object, or null for none.
pub(crate) fn arguments_from_json(text: Option<&str>) -> Result<Map<String, Value>, BoxError> {
    match text.map(str::trim).filter(|text| !text.is_empty()) {
        None => Ok(Map::new()),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(arguments)) => Ok(arguments),
            Ok(Value::Null) => Ok(Map::new()),
            _ => Err(McpError::invalid("arguments must be a JSON object")),
        },
    }
}

impl McpEventRuntime {
    /// Creates a trigger. The server must be configured; when it can be
    /// reached, it must offer the event too.
    pub(crate) async fn create(
        &self,
        input: TriggerInput,
        origin: Option<CronJobOrigin>,
        created_by: &str,
    ) -> Result<EventTrigger, BoxError> {
        if origin
            .as_ref()
            .is_some_and(|origin| origin.external_user == Some(true))
        {
            return Err(McpError::invalid(
                "external IM users cannot create MCP event automations",
            ));
        }
        let server_id = input.server_id.trim().to_string();
        self.manager().server(&server_id).await?;
        let event = input.event.trim().to_string();
        if event.is_empty() || event.len() > MAX_EVENT_NAME_BYTES {
            return Err(McpError::invalid("an event name must be 1 to 256 bytes"));
        }
        if let Ok(catalog) = self.catalog(&server_id, true).await {
            let Some(events) = catalog else {
                return Err(McpError::invalid(format!(
                    "MCP server {server_id} does not support MCP Events"
                )));
            };
            if !events.iter().any(|definition| definition.name == event) {
                return Err(McpError::invalid(format!(
                    "MCP server {server_id} offers no event {event:?}"
                )));
            }
        }
        let now = unix_ms();
        let trigger = EventTrigger {
            _id: 0,
            name: match input.name.as_deref() {
                Some(name) => name_of(name)?,
                None => name_of(&format!("{event} on {server_id}"))?,
            },
            server_id,
            event,
            arguments: arguments_of(input.arguments)?,
            instructions: instructions_of(&input.instructions)?,
            origin,
            delivery: input.delivery.unwrap_or_default(),
            batch_window_secs: window_of(
                input.batch_window_secs.unwrap_or(DEFAULT_BATCH_WINDOW_SECS),
            )?,
            max_runs_per_hour: runs_of(
                input.max_runs_per_hour.unwrap_or(DEFAULT_MAX_RUNS_PER_HOUR),
            )?,
            enabled: true,
            created_by: created_by.to_string(),
            created_at: now,
            updated_at: now,
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
        };
        let trigger = self.store().insert(trigger).await?;
        self.wake();
        Ok(trigger)
    }

    /// Changes a trigger. A different event, arguments or delivery starts a
    /// new subscription from now.
    pub(crate) async fn update(
        &self,
        id: u64,
        patch: TriggerPatch,
    ) -> Result<EventTrigger, BoxError> {
        if patch.is_empty() {
            return Err(McpError::invalid("nothing to change"));
        }
        let name = patch.name.as_deref().map(name_of).transpose()?;
        let instructions = patch
            .instructions
            .as_deref()
            .map(instructions_of)
            .transpose()?;
        let arguments = patch.arguments.map(arguments_of).transpose()?;
        let window = patch.batch_window_secs.map(window_of).transpose()?;
        let runs = patch.max_runs_per_hour.map(runs_of).transpose()?;
        let (trigger, before) = self
            .store()
            .modify(id, |trigger| {
                let before = trigger.clone();
                if let Some(name) = name {
                    trigger.name = name;
                }
                if let Some(instructions) = instructions {
                    trigger.instructions = instructions;
                }
                // Compared as values: the same arguments in another key order
                // (a form writes them in its schema's order) are no change,
                // while the subscription key would read them as one.
                if let Some(arguments) = arguments
                    && arguments != trigger.arguments
                {
                    trigger.arguments = arguments;
                }
                if let Some(delivery) = patch.delivery {
                    trigger.delivery = delivery;
                }
                if let Some(window) = window {
                    trigger.batch_window_secs = window;
                }
                if let Some(runs) = runs {
                    trigger.max_runs_per_hour = runs;
                }
                if trigger.subscription_key() != before.subscription_key() {
                    trigger.cursor = None;
                    trigger.webhook = None;
                    trigger.mode = None;
                }
                if trigger.state == TriggerState::Ended {
                    trigger.state = TriggerState::Starting;
                    trigger.last_error = None;
                }
                trigger.updated_at = unix_ms();
                Ok(before)
            })
            .await?
            .ok_or_else(|| McpError::missing(format!("MCP event automation {id} not found")))?;
        if trigger.subscription_key() != before.subscription_key() {
            self.stop(&before, true).await;
        }
        self.wake();
        Ok(trigger)
    }

    /// Turns a trigger on or off. Turning it on also clears what paused or
    /// ended it.
    pub(crate) async fn set_enabled(
        &self,
        id: u64,
        enabled: bool,
    ) -> Result<EventTrigger, BoxError> {
        let (trigger, _) = self
            .store()
            .modify(id, |trigger| {
                trigger.enabled = enabled;
                trigger.state = if enabled {
                    TriggerState::Starting
                } else {
                    TriggerState::Paused
                };
                if enabled {
                    trigger.last_error = None;
                }
                trigger.updated_at = unix_ms();
                Ok(())
            })
            .await?
            .ok_or_else(|| McpError::missing(format!("MCP event automation {id} not found")))?;
        if enabled {
            self.reset_failures(id);
        } else {
            self.stop(&trigger, false).await;
        }
        self.wake();
        Ok(trigger)
    }

    /// Deletes a trigger: its subscription ends, and its webhook endpoint and
    /// waiting events go with it. Its runs stay.
    pub(crate) async fn delete(&self, id: u64) -> Result<(), BoxError> {
        let trigger = self
            .store()
            .get(id)
            .await
            .map_err(|_| McpError::missing(format!("MCP event automation {id} not found")))?;
        self.stop(&trigger, true).await;
        self.store().remove(id).await?;
        self.wake();
        Ok(())
    }

    /// The triggers, as shown to the owner.
    pub(crate) async fn trigger_views(
        &self,
        server_id: Option<&str>,
    ) -> Result<Vec<Value>, BoxError> {
        let mut views = Vec::new();
        for trigger in self.store().list().await? {
            if server_id.is_some_and(|id| id != trigger.server_id) {
                continue;
            }
            let pending = self
                .store()
                .pending(trigger._id, MAX_PENDING_EVENTS)
                .await?
                .len();
            views.push(trigger_view(&trigger, pending));
        }
        Ok(views)
    }

    /// One trigger with its latest runs and events.
    pub(crate) async fn trigger_detail(&self, id: u64) -> Result<Value, BoxError> {
        let trigger = self
            .store()
            .get(id)
            .await
            .map_err(|_| McpError::missing(format!("MCP event automation {id} not found")))?;
        let pending = self.store().pending(id, MAX_PENDING_EVENTS).await?.len();
        let mut view = trigger_view(&trigger, pending);
        view["runs_recent"] = self
            .store()
            .list_runs(id, 10)
            .await?
            .iter()
            .map(run_view)
            .collect();
        view["events_recent"] = self
            .store()
            .recent_events(id, 10)
            .await?
            .iter()
            .map(event_view)
            .collect();
        Ok(view)
    }

    /// Adds each server's events to an MCP snapshot (`servers[].events`): its
    /// automations, how many are paused, and its event types when they were
    /// listed lately. Reads only what is stored and cached.
    pub(crate) async fn annotate_snapshot(&self, snapshot: &mut Value) {
        let triggers = self.store().list().await.unwrap_or_default();
        let types = self.cached_event_types().await;
        let Some(servers) = snapshot.get_mut("servers").and_then(Value::as_array_mut) else {
            return;
        };
        for server in servers {
            let id = server["id"].as_str().unwrap_or_default().to_string();
            let (automations, paused) = triggers
                .iter()
                .filter(|trigger| trigger.server_id == id)
                .fold((0, 0), |(all, paused), trigger| {
                    (all + 1, paused + usize::from(!trigger.enabled))
                });
            let mut events = json!({ "automations": automations, "paused": paused });
            match types.get(&id) {
                Some(Some(count)) => {
                    events["supported"] = true.into();
                    events["types"] = (*count).into();
                }
                Some(None) => events["supported"] = false.into(),
                None => {}
            }
            server["events"] = events;
        }
    }

    /// A server's event types and its triggers, for the MCP page. `supported`
    /// is false when the server does not implement MCP Events.
    pub(crate) async fn server_events(&self, server_id: &str) -> Result<Value, BoxError> {
        self.manager().server(server_id).await?;
        let catalog = self.catalog(server_id, true).await;
        let ingress = match self.ingress_status().await {
            Ok(id) => json!({"available": true, "server_id": id}),
            Err(reason) => json!({"available": false, "reason": reason}),
        };
        let triggers = self.trigger_views(Some(server_id)).await?;
        Ok(match catalog {
            Ok(Some(events)) => json!({
                "supported": true,
                "events": events.iter().map(definition_view).collect::<Vec<_>>(),
                "ingress": ingress,
                "triggers": triggers,
            }),
            Ok(None) => json!({
                "supported": false, "events": [], "ingress": ingress, "triggers": triggers,
            }),
            Err(error) => json!({
                "supported": false, "events": [], "error": error, "ingress": ingress,
                "triggers": triggers,
            }),
        })
    }
}

/// An event type for the owner and the model. Server text is marked untrusted.
pub(crate) fn definition_view(definition: &McpEventDefinition) -> Value {
    let webhook_only = definition.local_mode().is_none();
    json!({
        "name": definition.name,
        "description": definition.description.as_deref().map(|text| clip(text, 1_000)),
        "delivery": definition.delivery,
        "webhook_only": webhook_only,
        "input_schema": definition.input_schema,
        "payload_schema": definition.payload_schema,
    })
}

pub(crate) fn trigger_view(trigger: &EventTrigger, pending: usize) -> Value {
    let origin = trigger.origin.as_ref().map(|origin| {
        json!({
            "source": origin.source,
            "reply_target": origin.reply_target,
            "thread": origin.thread,
            "conversation_id": origin.conversation_id,
        })
    });
    json!({
        "id": trigger._id,
        "name": trigger.name,
        "server_id": trigger.server_id,
        "event": trigger.event,
        "arguments": trigger.arguments,
        "instructions": trigger.instructions,
        "delivery": trigger.delivery,
        "mode": trigger.mode,
        "batch_window_secs": trigger.batch_window_secs,
        "max_runs_per_hour": trigger.max_runs_per_hour,
        "enabled": trigger.enabled,
        "state": trigger.state,
        "last_error": trigger.last_error,
        "last_event_at": trigger.last_event_at,
        "last_run_at": trigger.last_run_at,
        "missed_events_at": trigger.missed_events_at,
        "events_received": trigger.events_received,
        "runs": trigger.runs,
        "pending": pending,
        "origin": origin,
        "last_conversation_id": trigger.last_conversation_id,
        "webhook": trigger.webhook.as_ref().map(|webhook| json!({
            "ingress": webhook.ingress,
            "endpoint_id": webhook.endpoint_id,
            "subscribed": webhook.subscription_id.is_some(),
            "refresh_before": webhook.refresh_before,
        })),
        "created_by": trigger.created_by,
        "created_at": trigger.created_at,
        "updated_at": trigger.updated_at,
    })
}

fn run_view(run: &TriggerRun) -> Value {
    json!({
        "id": run._id,
        "started_at": run.started_at,
        "finished_at": (run.finished_at > 0).then_some(run.finished_at),
        "events": run.events,
        "result": run.result.as_deref().map(|text| clip(text, 1_000)),
        "error": run.error,
        "conversation_id": run.conversation_id,
    })
}

fn event_view(record: &EventRecord) -> Value {
    let data = serde_json::to_string(&record.data).unwrap_or_default();
    json!({
        "event_id": record.event_id,
        "name": record.name,
        "timestamp": record.timestamp,
        "received_at": record.received_at,
        "handled": record.run_id != 0,
        "verified": record.verified,
        "data": clip(&data, VIEW_EVENT_BYTES),
    })
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
