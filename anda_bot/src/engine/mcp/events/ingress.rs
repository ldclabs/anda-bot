//! Webhook delivery through dMsg.
//!
//! Anda listens on no public port. An event type delivered only by webhook is
//! received by dMsg instead: the MCP server marked `events.webhook_ingress` in
//! mcp.json (normally `dmsg mcp`). For each trigger Anda creates an endpoint
//! there (`dmsg_events_endpoint_create` gives its URL and signing secret),
//! subscribes the upstream server to that URL itself, tells dMsg about the
//! subscription (`dmsg_events_bind`), and reads what dMsg relays from its
//! `dmsg.relay.delivery` event, acknowledging it as it is stored
//! (`dmsg_events_ack`). Refreshing and ending the upstream subscription stay
//! with Anda. These calls are the runtime's own: they reach dMsg without the
//! model and without the call gate, on the owner's consent to the trigger.

use anda_core::BoxError;
use anda_engine::extension::mcp::{
    McpEvent, McpEventDeliveryMode, McpEventError, McpEventErrorKind, McpEventSink,
    McpEventSubscribeRequest, McpEventSubscription, McpWebhookSubscribeRequest,
    McpWebhookSubscription,
};
use rmcp::model::CallToolResult;
use serde_json::{Map, Value, json};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

use super::store::{EventTrigger, NewEvent, WebhookState};
use crate::engine::mcp::McpManager;

/// The event dMsg relays webhook deliveries on.
pub(crate) const RELAY_EVENT: &str = "dmsg.relay.delivery";
const ENDPOINT_CREATE: &str = "dmsg_events_endpoint_create";
const ENDPOINT_DELETE: &str = "dmsg_events_endpoint_delete";
const BIND: &str = "dmsg_events_bind";
const ACK: &str = "dmsg_events_ack";
/// Bound on the runtime's own calls to dMsg and the upstream server.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// The ingress server and how it relays.
pub(crate) struct Ingress {
    pub id: String,
    pub mode: McpEventDeliveryMode,
}

/// What a batch of relayed deliveries amounts to.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct RelayOutcome {
    /// Verified upstream events.
    pub events: Vec<NewEvent>,
    /// The latest upstream cursor.
    pub cursor: Option<String>,
    /// Events may have been lost upstream or in the relay.
    pub missed: bool,
    /// The upstream subscription should be renewed now.
    pub refresh: bool,
    /// The upstream server or dMsg ended the subscription.
    pub terminated: Option<String>,
    /// Deliveries dropped, with why.
    pub rejected: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct DmsgIngress {
    manager: McpManager,
}

impl DmsgIngress {
    pub(crate) fn new(manager: McpManager) -> Self {
        Self { manager }
    }

    /// The configured ingress, when it is enabled and relays webhooks.
    pub(crate) async fn server(&self) -> Result<Ingress, String> {
        let id = self.manager.webhook_ingress().ok_or_else(|| {
            "only a webhook delivers this event, and no MCP server is set to receive webhooks; \
             add dMsg to mcp.json with \"events\": {\"webhook_ingress\": true}"
                .to_string()
        })?;
        let events = tokio::time::timeout(
            CALL_TIMEOUT,
            self.manager
                .provider()
                .list_events(&id, CancellationToken::new()),
        )
        .await
        .map_err(|_| format!("MCP server {id} did not list its events in time"))?
        .map_err(|err| format!("MCP server {id} could not be reached: {err}"))?
        .ok_or_else(|| format!("MCP server {id} does not support MCP Events"))?;
        let relay = events
            .iter()
            .find(|event| event.name == RELAY_EVENT)
            .ok_or_else(|| {
                format!(
                    "MCP server {id} does not relay webhooks ({RELAY_EVENT} is missing); check \
                     that Anda may use event endpoints there"
                )
            })?;
        let mode = relay
            .local_mode()
            .ok_or_else(|| format!("MCP server {id} relays by neither push nor poll"))?;
        Ok(Ingress { id, mode })
    }

    /// Creates an endpoint for `trigger` and keeps its secret in the MCP
    /// secret store.
    pub(crate) async fn create_endpoint(
        &self,
        ingress: &Ingress,
        trigger: &EventTrigger,
    ) -> Result<WebhookState, BoxError> {
        let endpoint = self
            .call(
                &ingress.id,
                ENDPOINT_CREATE,
                json!({"context": {"session": trigger._id.to_string(), "label": trigger.name}}),
            )
            .await?;
        let field = |name: &str| {
            endpoint
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 2048)
                .map(str::to_string)
                .ok_or_else(|| format!("{ENDPOINT_CREATE} returned no {name}"))
        };
        let (endpoint_id, url, secret) = (field("endpoint_id")?, field("url")?, field("secret")?);
        let secret_name = format!("MCP_EVENT_{}_WEBHOOK_SECRET", trigger._id);
        self.manager
            .store_secret(&secret_name, Some(&secret))
            .await?;
        Ok(WebhookState {
            ingress: ingress.id.clone(),
            endpoint_id,
            url,
            secret_name,
            ..Default::default()
        })
    }

    /// Subscribes, or subscribes again, the upstream server to the endpoint,
    /// resuming at the trigger's cursor.
    pub(crate) async fn subscribe_upstream(
        &self,
        trigger: &EventTrigger,
        webhook: &WebhookState,
    ) -> Result<McpWebhookSubscription, McpEventError> {
        let secret = self
            .manager
            .secret_value(&webhook.secret_name)
            .ok_or_else(|| McpEventError {
                kind: McpEventErrorKind::Other,
                code: None,
                message: format!("the endpoint secret {} is missing", webhook.secret_name),
                data: None,
            })?;
        let request = McpWebhookSubscribeRequest {
            name: trigger.event.clone(),
            arguments: trigger.arguments(),
            url: webhook.url.clone(),
            secret,
            cursor: trigger.cursor.clone(),
            max_age_ms: None,
            ttl_ms: None,
        };
        let cancel = CancellationToken::new();
        let subscribe =
            self.manager
                .provider()
                .subscribe_webhook(&trigger.server_id, request, cancel.clone());
        match tokio::time::timeout(CALL_TIMEOUT, subscribe).await {
            Ok(Ok(subscription)) => Ok(subscription),
            Ok(Err(err)) => Err(match err.downcast::<McpEventError>() {
                Ok(err) => *err,
                Err(err) => McpEventError {
                    kind: McpEventErrorKind::Other,
                    code: None,
                    message: err.to_string(),
                    data: None,
                },
            }),
            Err(_) => {
                cancel.cancel();
                Err(McpEventError {
                    kind: McpEventErrorKind::Other,
                    code: None,
                    message: "events/subscribe timed out".to_string(),
                    data: None,
                })
            }
        }
    }

    /// Tells dMsg which upstream subscription delivers to the endpoint and
    /// when it expires, for its renewal reminders.
    pub(crate) async fn bind(
        &self,
        trigger: &EventTrigger,
        webhook: &WebhookState,
    ) -> Result<(), BoxError> {
        let refresh_before = webhook.refresh_before.and_then(|ms| {
            chrono::DateTime::from_timestamp_millis(ms as i64).map(|time| time.to_rfc3339())
        });
        self.call(
            &webhook.ingress,
            BIND,
            json!({
                "endpoint_id": webhook.endpoint_id,
                "server": trigger.server_id,
                "subscription_id": webhook.subscription_id,
                "refreshBefore": refresh_before,
            }),
        )
        .await?;
        Ok(())
    }

    /// Reads the endpoint's deliveries from dMsg, from the last position.
    pub(crate) fn open(
        &self,
        ingress: &Ingress,
        webhook: &WebhookState,
        sink: Arc<dyn McpEventSink>,
    ) -> Result<McpEventSubscription, BoxError> {
        self.manager.provider().subscribe_events(
            &webhook.ingress,
            McpEventSubscribeRequest {
                name: RELAY_EVENT.to_string(),
                arguments: Map::from_iter([(
                    "endpoint_id".to_string(),
                    Value::String(webhook.endpoint_id.clone()),
                )]),
                mode: ingress.mode,
                cursor: webhook.relay_cursor.clone(),
                max_age_ms: None,
            },
            sink,
        )
    }

    /// Lets dMsg clean up what has been stored. Best effort: the relay
    /// cursor, not the acknowledgement, decides where reading resumes.
    pub(crate) async fn ack(&self, webhook: &WebhookState, cursor: Option<String>) {
        let Some(cursor) = cursor else {
            return;
        };
        if let Err(err) = self
            .call(
                &webhook.ingress,
                ACK,
                json!({"endpoint_id": webhook.endpoint_id, "cursor": cursor}),
            )
            .await
        {
            log::debug!("{ACK} for endpoint {} failed: {err}", webhook.endpoint_id);
        }
    }

    /// Ends the upstream subscription and deletes the endpoint and its secret.
    /// Best effort: a server that is gone cannot be told.
    pub(crate) async fn remove(&self, trigger: &EventTrigger, webhook: &WebhookState) {
        if webhook.subscription_id.is_some() {
            let unsubscribe = self.manager.provider().unsubscribe_webhook(
                &trigger.server_id,
                &trigger.event,
                trigger.arguments(),
                &webhook.url,
                CancellationToken::new(),
            );
            match tokio::time::timeout(CALL_TIMEOUT, unsubscribe).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => log::warn!(
                    "MCP event automation {}: events/unsubscribe failed: {err}",
                    trigger._id
                ),
                Err(_) => log::warn!(
                    "MCP event automation {}: events/unsubscribe timed out",
                    trigger._id
                ),
            }
        }
        if let Err(err) = self
            .call(
                &webhook.ingress,
                ENDPOINT_DELETE,
                json!({"endpoint_id": webhook.endpoint_id}),
            )
            .await
        {
            log::warn!(
                "MCP event automation {}: endpoint {} not deleted: {err}",
                trigger._id,
                webhook.endpoint_id
            );
        }
        if let Err(err) = self.manager.store_secret(&webhook.secret_name, None).await {
            log::warn!("{} not removed: {err}", webhook.secret_name);
        }
    }

    async fn call(&self, server_id: &str, tool: &str, arguments: Value) -> Result<Value, BoxError> {
        let Value::Object(arguments) = arguments else {
            return Err("tool arguments must be an object".into());
        };
        let cancel = CancellationToken::new();
        let call =
            self.manager
                .provider()
                .call_server_tool(server_id, tool, arguments, cancel.clone());
        let result = tokio::time::timeout(CALL_TIMEOUT, call)
            .await
            .map_err(|_| {
                cancel.cancel();
                format!("{tool} timed out")
            })??;
        tool_json(tool, result)
    }
}

/// A tool result's JSON: its structured content, or its text parsed as JSON.
fn tool_json(tool: &str, result: CallToolResult) -> Result<Value, BoxError> {
    let text: Vec<String> = result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect();
    if result.is_error == Some(true) {
        return Err(format!("{tool} failed: {}", text.join(" ")).into());
    }
    if let Some(value) = result.structured_content {
        return Ok(value);
    }
    text.iter()
        .find_map(|text| serde_json::from_str::<Value>(text).ok())
        .ok_or_else(|| format!("{tool} returned no JSON").into())
}

/// Turns relayed deliveries into upstream events and notices. Only events
/// the relay verified are kept; dMsg's `verified.v1a` means the signature was
/// checked end to end, `v1` that the relay checked it.
pub(crate) fn relay_events(deliveries: Vec<McpEvent>) -> RelayOutcome {
    let mut outcome = RelayOutcome::default();
    for delivery in deliveries {
        let payload = delivery.data;
        let upstream = payload.get("upstream").cloned().unwrap_or(Value::Null);
        let upstream_cursor = upstream
            .get("cursor")
            .or_else(|| payload.get("cursor"))
            .and_then(Value::as_str)
            .map(str::to_string);
        match payload.get("kind").and_then(Value::as_str).unwrap_or("") {
            "event" => {
                let verified = payload.get("verified");
                let v1 = verified.and_then(|v| v.get("v1")).and_then(Value::as_bool);
                let v1a = verified.and_then(|v| v.get("v1a")).and_then(Value::as_bool);
                if v1 != Some(true) {
                    outcome.rejected.push(format!(
                        "dropped relayed delivery {} that the relay did not verify",
                        delivery.event_id
                    ));
                    continue;
                }
                let Some(event_id) = upstream
                    .get("eventId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                else {
                    outcome.rejected.push(format!(
                        "dropped relayed delivery {} without an upstream eventId",
                        delivery.event_id
                    ));
                    continue;
                };
                outcome.events.push(NewEvent {
                    event_id: event_id.to_string(),
                    name: upstream
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    timestamp: upstream
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    data: upstream.get("data").cloned().unwrap_or(Value::Null),
                    verified: Some(if v1a == Some(true) { "v1a" } else { "v1" }.to_string()),
                });
                if upstream_cursor.is_some() {
                    outcome.cursor = upstream_cursor;
                }
            }
            "gap" | "relay_gap" => {
                outcome.missed = true;
                if upstream_cursor.is_some() {
                    outcome.cursor = upstream_cursor;
                    outcome.refresh = true;
                }
            }
            "terminated" => {
                let reason = upstream
                    .pointer("/error/message")
                    .or_else(|| payload.get("reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("the subscription or its dMsg endpoint ended");
                outcome.terminated = Some(format!("webhook delivery ended: {reason}"));
            }
            "refresh_due" => outcome.refresh = true,
            // `verification` only says the upstream server checked the
            // endpoint; it starts no run.
            _ => {}
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delivery(id: &str, payload: Value) -> McpEvent {
        McpEvent {
            event_id: id.to_string(),
            name: RELAY_EVENT.to_string(),
            timestamp: "2026-10-10T00:00:00Z".to_string(),
            data: payload,
            cursor: Some(format!("r-{id}")),
            meta: None,
        }
    }

    #[test]
    fn relayed_deliveries_keep_verified_upstream_events() {
        let outcome = relay_events(vec![
            delivery(
                "d1",
                json!({"endpoint_id": "ep", "kind": "verification", "verified": {"v1": true}}),
            ),
            delivery(
                "d2",
                json!({"endpoint_id": "ep", "kind": "event", "verified": {"v1": true, "v1a": true},
                       "upstream": {"eventId": "u1", "name": "issue.opened", "timestamp": "t", "data": {"n": 1}, "cursor": "c1"}}),
            ),
            delivery(
                "d3",
                json!({"endpoint_id": "ep", "kind": "event", "verified": {"v1": false, "v1a": null},
                       "upstream": {"eventId": "forged", "name": "issue.opened", "data": {}}}),
            ),
            delivery(
                "d4",
                json!({"endpoint_id": "ep", "kind": "event", "verified": {"v1": true, "v1a": null},
                       "upstream": {"eventId": "u2", "name": "issue.opened", "data": {"n": 2}, "cursor": "c2"}}),
            ),
        ]);
        assert_eq!(
            outcome
                .events
                .iter()
                .map(|e| (e.event_id.as_str(), e.verified.as_deref()))
                .collect::<Vec<_>>(),
            [("u1", Some("v1a")), ("u2", Some("v1"))]
        );
        assert_eq!(outcome.cursor.as_deref(), Some("c2"));
        assert_eq!(outcome.rejected.len(), 1);
        assert!(!outcome.missed && !outcome.refresh && outcome.terminated.is_none());
    }

    #[test]
    fn gaps_resume_upstream_and_terminations_end_the_trigger() {
        let gap = relay_events(vec![delivery(
            "g",
            json!({"kind": "relay_gap", "upstream": {"cursor": "c7"}}),
        )]);
        assert!(gap.missed && gap.refresh);
        assert_eq!(gap.cursor.as_deref(), Some("c7"));

        let due = relay_events(vec![delivery("r", json!({"kind": "refresh_due"}))]);
        assert!(due.refresh && !due.missed);

        let ended = relay_events(vec![delivery(
            "t",
            json!({"kind": "terminated", "upstream": {"error": {"code": -32024, "message": "Access revoked"}}}),
        )]);
        assert_eq!(
            ended.terminated.as_deref(),
            Some("webhook delivery ended: Access revoked")
        );
    }
}
