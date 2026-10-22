//! Contract and recovery regressions, plus an opt-in test of the real dMsg
//! stdio executable against an isolated daemon fixture (no user credentials).

use super::tests::{input, origin, runtime_with, until};
use super::*;
use crate::engine::mcp::test_server::{EventsMock, serve_events, validate_dmsg_tool};
use serde_json::Value;

async fn webhook_runtime(
    bind_fails: bool,
) -> (
    McpEventRuntime,
    Arc<anda_engine::engine::Engine>,
    EventsMock,
    EventsMock,
    tempfile::TempDir,
) {
    let upstream = EventsMock::default();
    *upstream.events.write() = vec![json!({"name": "issue.opened", "delivery": ["webhook"]})];
    *upstream.subscription.write() =
        json!({"id": "sub1", "refreshBefore": "2099-01-01T00:00:00Z", "truncated": false});
    let dmsg = EventsMock::default();
    *dmsg.events.write() = vec![json!({"name": "dmsg.relay.delivery", "delivery": ["poll"]})];
    dmsg.tools.write().extend([
        (
            "dmsg_events_endpoint_create".into(),
            json!({"endpoint_id": "ep", "url": "https://hooks.example/ep", "secret": "whsec_test"}),
        ),
        (
            "dmsg_events_bind".into(),
            if bind_fails {
                json!({"error": "temporarily locked"})
            } else {
                json!({"endpoint_id": "ep"})
            },
        ),
        (
            "dmsg_events_ack".into(),
            json!({"acked": 1, "through": "42"}),
        ),
        (
            "dmsg_events_endpoint_delete".into(),
            json!({"state": "deleted"}),
        ),
    ]);
    let upstream_url = serve_events(upstream.clone()).await;
    let relay_url = serve_events(dmsg.clone()).await;
    let (runtime, engine, _, home) = runtime_with(
        json!({
            "docs": {"type": "http", "url": upstream_url},
            "dmsg": {"type": "http", "url": relay_url, "events": {"webhook_ingress": true}}
        }),
        false,
    )
    .await;
    (runtime, engine, upstream, dmsg, home)
}

#[test]
fn contract_rejects_the_previous_bind_and_ack_payloads() {
    assert!(
        validate_dmsg_tool(
            "dmsg_events_bind",
            &json!({"endpoint_id": "ep", "server": "docs"})
        )
        .is_err()
    );
    assert!(
        validate_dmsg_tool(
            "dmsg_events_bind",
            &json!({"endpoint_id": "ep", "refreshBefore": "2026-01-01T00:00:00Z"})
        )
        .is_err()
    );
    assert!(
        validate_dmsg_tool(
            "dmsg_events_bind",
            &json!({"endpoint_id": "ep", "refresh_before": "2026-01-01T00:00:00+00:00"})
        )
        .is_err()
    );
    assert!(
        validate_dmsg_tool(
            "dmsg_events_ack",
            &json!({"endpoint_id": "ep", "cursor": "42"})
        )
        .is_err()
    );
    assert!(
        validate_dmsg_tool(
            "dmsg_events_ack",
            &json!({"endpoint_id": "ep", "through": "42"})
        )
        .is_ok()
    );
}

#[tokio::test]
async fn only_https_upstreams_supply_a_key_discovery_url() {
    let (runtime, _engine, _prompts, _home) = runtime_with(
        json!({
            "secure": {"type": "http", "url": "https://mcp.example.com/mcp"},
            "plain": {"type": "http", "url": "http://127.0.0.1:9/mcp"},
        }),
        false,
    )
    .await;
    assert_eq!(
        runtime.manager().server_url("secure").unwrap().as_deref(),
        Some("https://mcp.example.com/mcp")
    );
    // dMsg refuses a plain-HTTP origin and with it the whole bind.
    assert_eq!(runtime.manager().server_url("plain").unwrap(), None);
}

#[tokio::test]
async fn binding_failure_retries_the_same_endpoint_and_subscription() {
    let (runtime, _engine, upstream, dmsg, _home) = webhook_runtime(true).await;
    let trigger = runtime
        .create(input("docs", "issue.opened"), Some(origin()), "owner")
        .await
        .unwrap();
    runtime.tick().await;
    let stored = runtime.store().get(trigger._id).await.unwrap();
    assert_eq!(stored.state, TriggerState::Retrying);
    assert!(
        stored
            .last_error
            .as_deref()
            .unwrap()
            .contains("binding failed")
    );
    assert!(!stored.webhook.unwrap().bound);
    assert!(dmsg.calls("events/poll").is_empty());
    let bind = &dmsg.tool_calls("dmsg_events_bind")[0];
    // The mock upstream is plain HTTP: dMsg would refuse that origin for key
    // discovery, so the bind carries no URL and verifies with `v1` only.
    assert!(bind.get("server").is_none());
    assert!(bind["refresh_before"].is_u64());
    assert!(bind.get("refreshBefore").is_none());
    dmsg.tools
        .write()
        .insert("dmsg_events_bind".into(), json!({"endpoint_id": "ep"}));
    until(&runtime, "successful bind", || async {
        runtime.store().get(trigger._id).await.unwrap().state == TriggerState::Active
    })
    .await;
    assert!(
        runtime
            .store()
            .get(trigger._id)
            .await
            .unwrap()
            .webhook
            .unwrap()
            .bound
    );
    assert_eq!(upstream.calls("events/subscribe").len(), 1);
    assert_eq!(dmsg.tool_calls("dmsg_events_endpoint_create").len(), 1);
}

#[tokio::test]
async fn acknowledgement_receipts_are_checked_and_retried_after_restart() {
    let (runtime, _engine, _, dmsg, _home) = webhook_runtime(false).await;
    let trigger = runtime
        .create(input("docs", "issue.opened"), None, "owner")
        .await
        .unwrap();
    runtime.tick().await;
    runtime
        .store()
        .modify(trigger._id, |stored| {
            stored.webhook.as_mut().unwrap().relay_cursor = Some("42".into());
            Ok(())
        })
        .await
        .unwrap();
    dmsg.tools.write().insert(
        "dmsg_events_ack".into(),
        json!({"acked": 0, "through": "0"}),
    );
    runtime
        .ack_pending(&runtime.store().get(trigger._id).await.unwrap())
        .await;
    let stored = runtime.store().get(trigger._id).await.unwrap();
    let webhook = stored.webhook.as_ref().unwrap();
    assert!(webhook.acked_cursor.is_none());
    assert!(webhook.ack_error.is_some());
    let restarted = McpEventRuntime::new(McpEventRuntimeConfig {
        store: runtime.store().clone(),
        manager: runtime.manager().clone(),
        engine: runtime.inner.engine.clone(),
        admission: runtime.inner.admission.clone(),
    });
    dmsg.tools.write().insert(
        "dmsg_events_ack".into(),
        json!({"acked": 1, "through": "42"}),
    );
    restarted.ack_pending(&stored).await;
    let webhook = restarted
        .store()
        .get(trigger._id)
        .await
        .unwrap()
        .webhook
        .unwrap();
    assert_eq!(webhook.acked_cursor.as_deref(), Some("42"));
    assert!(webhook.ack_error.is_none());
    restarted
        .ack_pending(&restarted.store().get(trigger._id).await.unwrap())
        .await;
    assert_eq!(dmsg.tool_calls("dmsg_events_ack").len(), 2);
}

#[tokio::test]
async fn renewal_preserves_delivery_progress_and_rebinds_the_new_expiry() {
    let (runtime, _engine, upstream, dmsg, _home) = webhook_runtime(false).await;
    let trigger = runtime
        .create(input("docs", "issue.opened"), None, "owner")
        .await
        .unwrap();
    runtime.tick().await;
    let stale = runtime.store().get(trigger._id).await.unwrap();
    let webhook = stale.webhook.clone().unwrap();
    runtime
        .store()
        .modify(trigger._id, |stored| {
            stored.cursor = Some("newer-event".into());
            let webhook = stored.webhook.as_mut().unwrap();
            webhook.relay_cursor = Some("42".into());
            webhook.acked_cursor = Some("41".into());
            Ok(())
        })
        .await
        .unwrap();
    *upstream.subscription.write() = json!({"id": "sub2", "refreshBefore": "2099-02-01T00:00:00Z", "cursor": "old-position", "truncated": false});
    runtime.refresh_upstream(&stale, webhook).await.unwrap();
    let stored = runtime.store().get(trigger._id).await.unwrap();
    assert_eq!(stored.cursor.as_deref(), Some("newer-event"));
    let webhook = stored.webhook.unwrap();
    assert_eq!(webhook.relay_cursor.as_deref(), Some("42"));
    assert_eq!(webhook.acked_cursor.as_deref(), Some("41"));
    assert!(webhook.bound);
    assert_eq!(webhook.subscription_id.as_deref(), Some("sub2"));
    assert_eq!(
        dmsg.tool_calls("dmsg_events_bind").last().unwrap()["refresh_before"],
        webhook.refresh_before.unwrap()
    );
}

#[tokio::test]
async fn retry_backoff_and_failure_budget_survive_runtime_restart() {
    let (runtime, _engine, prompts, _home) = runtime_with(json!({}), true).await;
    let trigger = runtime
        .store()
        .insert(super::super::store::tests::trigger("gh", "issue.opened"))
        .await
        .unwrap();
    runtime
        .receive(
            trigger._id,
            vec![NewEvent {
                event_id: "e1".into(),
                name: "issue.opened".into(),
                timestamp: String::new(),
                data: json!({}),
                verified: None,
            }],
            Some("1".into()),
            None,
        )
        .await
        .unwrap();
    runtime.run(trigger._id).await;
    assert_eq!(
        runtime
            .store()
            .pending(trigger._id, 20)
            .await
            .unwrap()
            .len(),
        1
    );
    let restarted = McpEventRuntime::new(McpEventRuntimeConfig {
        store: runtime.store().clone(),
        manager: runtime.manager().clone(),
        engine: runtime.inner.engine.clone(),
        admission: runtime.inner.admission.clone(),
    });
    restarted.load_pending().await;
    assert!(restarted.inner.schedule.lock()[&trigger._id].retry_at > unix_ms());
    restarted.dispatch(&[restarted.store().get(trigger._id).await.unwrap()]);
    tokio::task::yield_now().await;
    assert_eq!(prompts.lock().len(), 1);
    // Invoke attempts directly to avoid waiting through the scheduler's backoff.
    for _ in 1..MAX_CONSECUTIVE_FAILURES {
        restarted.run(trigger._id).await;
    }
    assert!(!restarted.store().get(trigger._id).await.unwrap().enabled);
    assert_eq!(
        restarted
            .store()
            .pending(trigger._id, 20)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(prompts.lock()[0].0.contains("idempotency_key"));
    tokio::time::sleep(Duration::from_millis(2)).await;
    restarted.set_enabled(trigger._id, true).await.unwrap();
    assert_eq!(restarted.inner.schedule.lock()[&trigger._id].retry_at, 0);
    until(
        &restarted,
        "retry on resume without a new event",
        || async {
            restarted.store().get(trigger._id).await.unwrap().runs
                == u64::from(MAX_CONSECUTIVE_FAILURES) + 1
        },
    )
    .await;
    assert!(restarted.store().get(trigger._id).await.unwrap().enabled);
}

#[tokio::test]
async fn highest_hourly_limit_pauses_before_attempt_121() {
    let (runtime, _engine, _, _home) = runtime_with(json!({}), false).await;
    let mut trigger = super::super::store::tests::trigger("gh", "issue.opened");
    trigger.max_runs_per_hour = MAX_RUNS_PER_HOUR;
    let trigger = runtime.store().insert(trigger).await.unwrap();
    for _ in 0..MAX_RUNS_PER_HOUR {
        let run = runtime.store().start_run(trigger._id, 0).await.unwrap();
        runtime
            .store()
            .finish_run(run, &[], &CronJobResult::default())
            .await
            .unwrap();
    }
    runtime.run(trigger._id).await;
    assert!(!runtime.store().get(trigger._id).await.unwrap().enabled);
    assert_eq!(
        runtime.store().runs_since(trigger._id, 0).await.unwrap(),
        MAX_RUNS_PER_HOUR as usize
    );
}

#[tokio::test]
async fn direct_relay_requires_stable_selectors_and_filters_verification() {
    use super::super::manage::validate_relay_arguments;
    for arguments in [
        json!({}),
        json!({"endpoint_ids": []}),
        json!({"session": " "}),
    ] {
        assert!(validate_relay_arguments("dmsg.relay.delivery", &arguments).is_err());
    }
    for arguments in [
        json!({"endpoint_id": "ep"}),
        json!({"endpoint_ids": ["ep"]}),
        json!({"session": "stable"}),
    ] {
        assert!(validate_relay_arguments("dmsg.relay.delivery", &arguments).is_ok());
    }
    let (runtime, _engine, _, _home) = runtime_with(json!({}), false).await;
    let mut trigger = super::super::store::tests::trigger("dmsg", "dmsg.relay.delivery");
    trigger.arguments = json!({"endpoint_id": "ep"});
    let trigger = runtime.store().insert(trigger).await.unwrap();
    let events = ["verification", "event", "event", "refresh_due", "relay_gap"]
        .iter()
        .enumerate()
        .map(|(i, kind)| McpEvent {
            event_id: i.to_string(),
            name: "dmsg.relay.delivery".into(),
            timestamp: String::new(),
            data: json!({"kind": kind, "verified": {"v1": i == 1}}),
            cursor: Some(i.to_string()),
            meta: None,
        })
        .collect();
    runtime
        .direct_relay(&trigger, events, Some("4".into()))
        .await
        .unwrap();
    let pending = runtime.store().pending(trigger._id, 20).await.unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|e| e.event_id.as_str())
            .collect::<Vec<_>>(),
        ["1", "3", "4"]
    );
    let stored = runtime.store().get(trigger._id).await.unwrap();
    assert_eq!(stored.cursor.as_deref(), Some("4"));
    assert!(stored.missed_events_at.is_some());
}

#[derive(Default)]
struct DaemonFixture {
    state: Mutex<DaemonState>,
    wake: Notify,
}

#[derive(Default)]
struct DaemonState {
    calls: Vec<(String, Value)>,
    sessions: HashSet<String>,
    records: Vec<Value>,
    locked: bool,
    revoked: bool,
    deleted: bool,
    created: usize,
}

impl DaemonFixture {
    fn push(&self, seq: u64, kind: &str) {
        self.state.lock().records.push(json!({
            "eventId": format!("ep:{seq}"), "name": "dmsg.relay.delivery", "timestamp": "2026-10-22T00:00:00Z", "cursor": seq.to_string(),
            "data": {"endpoint_id": "ep", "kind": kind, "received_at": 1, "verified": {"v1": true, "v1a": null},
                "upstream": {"eventId": format!("upstream-{seq}"), "name": "issue.opened", "timestamp": "2026-10-22T00:00:00Z", "cursor": format!("u{seq}"), "data": {"issue": seq}}}
        }));
        self.wake.notify_waiters();
    }

    async fn request(&self, request: Value) -> Value {
        let id = &request["id"];
        let error = |code: &str| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": code, "data": {"code": code}}});
        // Exercise the executable's built-in tool catalog, not a copy in this fixture.
        if request["method"] == "mcp/tools" {
            return error("DAEMON_STOPPED");
        }
        if request["method"] != "mcp/call" {
            return error("NOT_FOUND");
        }
        let params = &request["params"];
        let tool = params["tool"].as_str().unwrap_or_default();
        let input = &params["input"];
        {
            let mut state = self.state.lock();
            state.calls.push((tool.to_string(), input.clone()));
            if let Some(session) = params["mcp_session"].as_str() {
                state.sessions.insert(session.to_string());
            }
            if state.locked {
                return error("LOCKED");
            }
            if state.revoked {
                return json!({"jsonrpc": "2.0", "id": id, "result": {"error": {"code": "FORBIDDEN", "message": "events.endpoint revoked"}}});
            }
        }
        if let Err(message) = validate_dmsg_tool(tool, input) {
            return json!({"jsonrpc": "2.0", "id": id, "result": {"error": {"code": "INVALID_INPUT", "message": message}}});
        }
        if tool == "dmsg_events_next" {
            let arrived = self.wake.notified();
            tokio::pin!(arrived);
            arrived.as_mut().enable();
            let after = input["cursor"]
                .as_str()
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap();
            let empty = !self
                .state
                .lock()
                .records
                .iter()
                .any(|e| e["cursor"].as_str().unwrap().parse::<u64>().unwrap() > after);
            if empty {
                tokio::select! {
                    _ = arrived => {}
                    _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                }
            }
        }
        let mut state = self.state.lock();
        let result = match tool {
            "dmsg_events_endpoint_create" => {
                state.created += 1;
                json!({"endpoint_id": "ep", "url": "https://hooks.example/ep", "secret": "whsec_test"})
            }
            "dmsg_events_bind" => json!({"endpoint_id": "ep"}),
            "dmsg_events_ack" => {
                let through = input["through"].as_str().unwrap().parse::<u64>().unwrap();
                let before = state.records.len();
                state
                    .records
                    .retain(|e| e["cursor"].as_str().unwrap().parse::<u64>().unwrap() > through);
                json!({"acked": before - state.records.len(), "through": through.to_string()})
            }
            "dmsg_events_next" => {
                assert_eq!(input["endpoint_id"], "ep");
                let after = input["cursor"]
                    .as_str()
                    .unwrap_or("0")
                    .parse::<u64>()
                    .unwrap();
                let events: Vec<_> = state
                    .records
                    .iter()
                    .filter(|e| e["cursor"].as_str().unwrap().parse::<u64>().unwrap() > after)
                    .cloned()
                    .collect();
                let cursor = events.last().map_or_else(
                    || after.to_string(),
                    |e| e["cursor"].as_str().unwrap().to_string(),
                );
                json!({"events": events, "cursor": cursor, "hasMore": false, "truncated": false})
            }
            "dmsg_events_endpoint_delete" => {
                state.deleted = true;
                json!({"endpoint_id": "ep", "state": "deleted"})
            }
            _ => return error("NOT_FOUND"),
        };
        json!({"jsonrpc": "2.0", "id": id, "result": {"result": result}})
    }
}

/// Run with DMSG_BIN=/absolute/path/to/dmsg and --ignored. The real MCP
/// process handles JSON-RPC, push notifications, cancellation and rediscovery.
/// Its daemon here is a controllable fixture, never the owner's running daemon.
#[tokio::test]
#[ignore = "requires DMSG_BIN pointing to a dmsg executable with MCP Events"]
async fn real_dmsg_stdio_push_restart_lock_renew_revoke_and_cleanup() {
    let binary = std::env::var("DMSG_BIN").expect("set DMSG_BIN to the dmsg executable");
    assert!(std::path::Path::new(&binary).is_absolute());
    let dmsg_home = tempfile::tempdir().unwrap();
    let fixture = Arc::new(DaemonFixture::default());
    let serve_fixture = fixture.clone();
    let app = axum::Router::new().route(
        "/app/v1",
        axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
            let fixture = serve_fixture.clone();
            async move { axum::Json(fixture.request(request).await) }
        }),
    );
    let url = crate::test_support::spawn_http_mock(app).await;
    let port = reqwest::Url::parse(&url).unwrap().port().unwrap();
    std::fs::write(
        dmsg_home.path().join("daemon.json"),
        json!({"port": port, "pid": std::process::id(), "instance": "anda-test"}).to_string(),
    )
    .unwrap();
    let upstream = EventsMock::default();
    *upstream.events.write() = vec![json!({"name": "issue.opened", "delivery": ["webhook"]})];
    *upstream.subscription.write() = json!({"id": "sub1", "refreshBefore": "2099-01-01T00:00:00Z"});
    let upstream_url = serve_events(upstream.clone()).await;
    let (runtime, _engine, prompts, _home) = runtime_with(
        json!({
            "docs": {"type": "http", "url": upstream_url},
            "dmsg": {"command": binary, "args": ["--home", dmsg_home.path(), "mcp"],
                "inherit_env": false, "env": {"DMSG_AGENT_TOKEN": "dmsg-agent-test-only"},
                "events": {"webhook_ingress": true}}
        }),
        false,
    )
    .await;
    let trigger = runtime
        .create(input("docs", "issue.opened"), Some(origin()), "owner")
        .await
        .unwrap();
    let id = trigger._id;
    until(&runtime, "stdio push subscription", || async {
        runtime.store().get(id).await.unwrap().state == TriggerState::Active
    })
    .await;
    runtime
        .manager()
        .provider()
        .refresh_server("dmsg")
        .await
        .unwrap();
    let routes = runtime.manager().provider().routes();
    let ack = routes
        .iter()
        .find(|route| route.remote_name == "dmsg_events_ack")
        .unwrap();
    let definition = serde_json::to_value(&ack.tool).unwrap();
    assert!(
        definition["inputSchema"]["properties"]
            .get("through")
            .is_some()
    );
    fixture.push(1, "event");
    until(&runtime, "first event and ack", || async {
        let stored = runtime.store().get(id).await.unwrap();
        stored.runs == 1 && stored.webhook.unwrap().acked_cursor.as_deref() == Some("1")
    })
    .await;
    assert!(fixture.state.lock().records.is_empty());
    assert!(
        fixture
            .state
            .lock()
            .calls
            .iter()
            .any(|(name, input)| name == "dmsg_events_next" && input["wait_ms"] == 25_000)
    );

    // A new stdio process has a different session; the explicit endpoint and
    // persisted cursor resume without a duplicate endpoint or business run.
    runtime.manager().reconnect(Some("dmsg")).await.unwrap();
    fixture.push(2, "event");
    until(&runtime, "event after MCP restart", || async {
        runtime.store().get(id).await.unwrap().runs == 2
    })
    .await;
    assert!(fixture.state.lock().sessions.len() >= 2);
    assert_eq!(fixture.state.lock().created, 1);

    fixture.state.lock().locked = true;
    fixture.wake.notify_waiters();
    until(&runtime, "locked state", || async {
        runtime.store().get(id).await.unwrap().state == TriggerState::Retrying
    })
    .await;
    fixture.state.lock().locked = false;
    fixture.push(3, "event");
    // dMsg retries daemon failures after five seconds.
    tokio::time::sleep(Duration::from_secs(5)).await;
    until(&runtime, "unlock recovery", || async {
        runtime.store().get(id).await.unwrap().runs == 3
    })
    .await;

    *upstream.subscription.write() = json!({"id": "sub2", "refreshBefore": "2099-02-01T00:00:00Z"});
    fixture.push(4, "refresh_due");
    until(&runtime, "renewal and control ack", || async {
        let state = runtime.store().get(id).await.unwrap().webhook.unwrap();
        state.subscription_id.as_deref() == Some("sub2")
            && state.bound
            && state.acked_cursor.as_deref() == Some("4")
    })
    .await;
    assert_eq!(
        prompts.lock().len(),
        3,
        "control records must not start business runs"
    );

    fixture.state.lock().revoked = true;
    fixture.wake.notify_waiters();
    until(&runtime, "permission revoked", || async {
        runtime.store().get(id).await.unwrap().state == TriggerState::Ended
    })
    .await;
    fixture.state.lock().revoked = false;
    runtime.set_enabled(id, true).await.unwrap();
    until(&runtime, "permission restored", || async {
        runtime.store().get(id).await.unwrap().state == TriggerState::Active
    })
    .await;
    let secret = runtime
        .store()
        .get(id)
        .await
        .unwrap()
        .webhook
        .unwrap()
        .secret_name;
    runtime.delete(id).await.unwrap();
    assert!(fixture.state.lock().deleted);
    assert!(runtime.manager().secret_value(&secret).is_none());
    assert_eq!(upstream.calls("events/unsubscribe").len(), 1);
    assert!(fixture.state.lock().records.is_empty());
}
