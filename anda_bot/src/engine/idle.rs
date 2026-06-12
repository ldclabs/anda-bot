use anda_core::BoxError;
use anda_engine::{rfc3339_datetime, unix_ms};
use async_trait::async_trait;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::brain::{self, MaintenanceInput, MaintenanceScope};

/// How often the idle monitor samples the bot's activity.
pub const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);
/// The bot must be continuously idle for this long before idle hooks fire.
/// This debounces short gaps between turns: live sessions flip between busy
/// and idle as messages arrive, and hooks must not fire in those gaps.
pub const IDLE_HOOK_THRESHOLD_MS: u64 = 10 * 60 * 1000;
/// Trigger a brain sleep when the last maintenance cycle started longer ago
/// than this.
const BRAIN_SLEEP_INTERVAL_MS: u64 = 12 * 60 * 60 * 1000; // 12 hours
/// Wait this long before re-checking the brain after triggering a sleep,
/// finding the brain busy, or hitting an error. The idle monitor calls hooks
/// about once a minute while idle and must not hammer the brain endpoint.
const BRAIN_SLEEP_RECHECK_MS: u64 = 10 * 60 * 1000;

/// A hook invoked while the bot is fully idle: every live session is idle
/// (its completion runner has no pending work) and no background tasks are
/// running.
#[async_trait]
pub trait IdleHook: Send + Sync {
    /// Called roughly once per [`IDLE_CHECK_INTERVAL`] for as long as the bot
    /// stays idle, with the continuous idle duration so far. Implementations
    /// must rate-limit their own work.
    async fn on_idle(&self, idle_ms: u64);
}

/// Tracks busy/idle observations and reports when the continuous idle time
/// has reached the threshold.
pub struct IdleTracker {
    threshold_ms: u64,
    idle_since: Option<u64>,
}

impl IdleTracker {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            threshold_ms,
            idle_since: None,
        }
    }

    /// Records one observation. Returns the continuous idle duration once it
    /// has reached the threshold; any busy observation restarts the clock.
    pub fn observe(&mut self, busy: bool, now_ms: u64) -> Option<u64> {
        if busy {
            self.idle_since = None;
            return None;
        }

        let since = *self.idle_since.get_or_insert(now_ms);
        let idle_ms = now_ms.saturating_sub(since);
        (idle_ms >= self.threshold_ms).then_some(idle_ms)
    }
}

/// Puts the brain to sleep (a full maintenance cycle) when the bot is idle
/// and no maintenance cycle has started for 12 hours.
pub struct BrainSleepIdleHook {
    brain: brain::Client,
    sleep_interval_ms: u64,
    recheck_backoff_ms: u64,
    // Unix ms before which idle calls are ignored; avoids querying the brain
    // every monitor tick during long idle stretches.
    next_check_at: AtomicU64,
}

impl BrainSleepIdleHook {
    pub fn new(brain: brain::Client) -> Self {
        Self::with_intervals(brain, BRAIN_SLEEP_INTERVAL_MS, BRAIN_SLEEP_RECHECK_MS)
    }

    fn with_intervals(
        brain: brain::Client,
        sleep_interval_ms: u64,
        recheck_backoff_ms: u64,
    ) -> Self {
        Self {
            brain,
            sleep_interval_ms,
            recheck_backoff_ms,
            next_check_at: AtomicU64::new(0),
        }
    }

    // Returns the unix ms before which no further check is needed.
    async fn check_and_sleep(&self, now_ms: u64) -> Result<u64, BoxError> {
        let status = self.brain.brain_status().await?;
        // start_at is when the latest maintenance cycle (sleep) started;
        // 0 means the brain has never slept and is due immediately.
        let due_at = status
            .maintenance_at
            .start_at
            .saturating_add(self.sleep_interval_ms);
        if now_ms < due_at {
            return Ok(due_at);
        }

        if status.formation_processing || status.maintenance_processing {
            // The brain itself is still busy; sleeping now would be rejected.
            return Ok(now_ms.saturating_add(self.recheck_backoff_ms));
        }

        let output = self
            .brain
            .maintenance(&MaintenanceInput {
                trigger: "scheduled".to_string(),
                scope: MaintenanceScope::Full,
                timestamp: rfc3339_datetime(now_ms),
                ..Default::default()
            })
            .await?;
        log::info!(
            conversation = output.conversation.unwrap_or_default();
            "triggered brain sleep (full maintenance cycle) while idle"
        );
        // The cycle runs asynchronously and records its start time right
        // away, so the next check lands on the new start_at + interval.
        Ok(now_ms.saturating_add(self.recheck_backoff_ms))
    }
}

#[async_trait]
impl IdleHook for BrainSleepIdleHook {
    async fn on_idle(&self, _idle_ms: u64) {
        let now_ms = unix_ms();
        if now_ms < self.next_check_at.load(Ordering::SeqCst) {
            return;
        }

        let next_check_at = match self.check_and_sleep(now_ms).await {
            Ok(next_check_at) => next_check_at,
            Err(err) => {
                log::warn!("brain sleep check failed: {err}");
                now_ms.saturating_add(self.recheck_backoff_ms)
            }
        };
        self.next_check_at.store(next_check_at, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anda_core::AgentOutput;
    use axum::{Json as AxumJson, Router, extract::State, routing};
    use parking_lot::RwLock;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const HOUR_MS: u64 = 60 * 60 * 1000;

    #[test]
    fn idle_tracker_requires_continuous_idle_threshold() {
        let mut tracker = IdleTracker::new(1000);

        assert_eq!(tracker.observe(true, 0), None);
        assert_eq!(tracker.observe(false, 100), None);
        assert_eq!(tracker.observe(false, 600), None);
        assert_eq!(tracker.observe(false, 1100), Some(1000));
        assert_eq!(tracker.observe(false, 2100), Some(2000));

        // Any busy observation restarts the idle clock.
        assert_eq!(tracker.observe(true, 2200), None);
        assert_eq!(tracker.observe(false, 2300), None);
        assert_eq!(tracker.observe(false, 3200), None);
        assert_eq!(tracker.observe(false, 3300), Some(1000));
    }

    #[derive(Clone, Default)]
    struct BrainMockState {
        // maintenance_at.start_at reported by the formation_status route.
        start_at: Arc<RwLock<u64>>,
        processing: Arc<RwLock<bool>>,
        status_calls: Arc<RwLock<u64>>,
        maintenance_calls: Arc<RwLock<Vec<Value>>>,
    }

    async fn spawn_brain_mock(state: BrainMockState) -> String {
        let app = Router::new()
            .route(
                "/v1/anda_bot/formation_status",
                routing::get(|State(state): State<BrainMockState>| async move {
                    *state.status_calls.write() += 1;
                    AxumJson(json!({
                        "result": {
                            "id": "anda_bot",
                            "concepts": 0,
                            "propositions": 0,
                            "conversations": 0,
                            "formation_processing": *state.processing.read(),
                            "maintenance_processing": false,
                            "formation_processed_id": 0,
                            "maintenance_processed_id": 0,
                            "maintenance_at": {
                                "daydream": 0,
                                "full": 0,
                                "quick": 0,
                                "start_at": *state.start_at.read(),
                            },
                        }
                    }))
                }),
            )
            .route(
                "/v1/anda_bot/maintenance",
                routing::post(
                    |State(state): State<BrainMockState>, AxumJson(body): AxumJson<Value>| async move {
                        state.maintenance_calls.write().push(body);
                        AxumJson(json!({
                            "result": serde_json::to_value(AgentOutput {
                                conversation: Some(42),
                                ..Default::default()
                            })
                            .unwrap()
                        }))
                    },
                ),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/v1/anda_bot")
    }

    #[tokio::test]
    async fn brain_sleep_hook_triggers_full_sleep_when_stale() {
        let now_ms = unix_ms();
        let state = BrainMockState::default();
        *state.start_at.write() = now_ms.saturating_sub(13 * HOUR_MS);
        let base_url = spawn_brain_mock(state.clone()).await;
        let hook = BrainSleepIdleHook::new(brain::Client::new(base_url, None));

        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;

        let calls = state.maintenance_calls.read().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["scope"], "full");
        assert_eq!(calls[0]["trigger"], "scheduled");
        assert!(calls[0]["timestamp"].is_string());

        // The triggered cycle runs asynchronously; further idle ticks are
        // ignored until the recheck backoff expires.
        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;
        assert_eq!(state.maintenance_calls.read().len(), 1);
    }

    #[tokio::test]
    async fn brain_sleep_hook_sleeps_when_brain_never_slept() {
        // start_at stays 0: the brain has never run a maintenance cycle.
        let state = BrainMockState::default();
        let base_url = spawn_brain_mock(state.clone()).await;
        let hook = BrainSleepIdleHook::new(brain::Client::new(base_url, None));

        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;

        assert_eq!(state.maintenance_calls.read().len(), 1);
    }

    #[tokio::test]
    async fn brain_sleep_hook_skips_recent_sleep() {
        let now_ms = unix_ms();
        let state = BrainMockState::default();
        *state.start_at.write() = now_ms.saturating_sub(HOUR_MS);
        let base_url = spawn_brain_mock(state.clone()).await;
        let hook = BrainSleepIdleHook::new(brain::Client::new(base_url, None));

        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;

        assert!(state.maintenance_calls.read().is_empty());
        assert_eq!(*state.status_calls.read(), 1);

        // The next check is deferred until the 12-hour interval elapses, so
        // an immediate second tick does not query the brain at all.
        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;
        assert_eq!(*state.status_calls.read(), 1);
        assert!(state.maintenance_calls.read().is_empty());
    }

    #[tokio::test]
    async fn brain_sleep_hook_waits_for_processing_brain() {
        let state = BrainMockState::default();
        *state.processing.write() = true;
        let base_url = spawn_brain_mock(state.clone()).await;
        let hook = BrainSleepIdleHook::new(brain::Client::new(base_url, None));

        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;

        assert!(state.maintenance_calls.read().is_empty());
    }

    #[tokio::test]
    async fn brain_sleep_hook_backs_off_after_errors() {
        // No mock server: every brain call fails.
        let hook = BrainSleepIdleHook::new(brain::Client::new(
            "http://127.0.0.1:1/v1/anda_bot".to_string(),
            None,
        ));

        hook.on_idle(IDLE_HOOK_THRESHOLD_MS).await;

        let next_check_at = hook.next_check_at.load(Ordering::SeqCst);
        assert!(next_check_at > unix_ms());
        assert!(next_check_at <= unix_ms().saturating_add(BRAIN_SLEEP_RECHECK_MS));
    }
}
