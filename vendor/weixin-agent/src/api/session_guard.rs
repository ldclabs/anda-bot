//! In-memory session pause/cooldown guard.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::types::SESSION_PAUSE_DURATION_MS;

/// Guards against API calls during a session-expired cooldown period.
pub(crate) struct SessionGuard {
    pause_until_ms: AtomicU64,
}

impl SessionGuard {
    /// Create a new (unpaused) guard.
    pub fn new() -> Self {
        Self {
            pause_until_ms: AtomicU64::new(0),
        }
    }

    /// Pause all API calls for one hour from now.
    pub fn pause(&self) {
        let until = now_ms() + SESSION_PAUSE_DURATION_MS;
        self.pause_until_ms.store(until, Ordering::Relaxed);
        tracing::info!(until_ms = until, "session paused");
    }

    /// Returns `true` if currently within the cooldown window.
    pub fn is_paused(&self) -> bool {
        let until = self.pause_until_ms.load(Ordering::Relaxed);
        until > 0 && now_ms() < until
    }

    /// Returns remaining pause time in milliseconds (0 if not paused).
    pub fn remaining_ms(&self) -> u64 {
        let until = self.pause_until_ms.load(Ordering::Relaxed);
        if until == 0 {
            return 0;
        }
        until.saturating_sub(now_ms())
    }

    /// Returns `Ok(())` if active, or `Err(SessionExpired)` if paused.
    #[allow(dead_code)] // Public API for consumers via WeixinClient
    pub fn assert_active(&self) -> Result<()> {
        if self.is_paused() {
            Err(Error::SessionExpired)
        } else {
            Ok(())
        }
    }
}

impl Default for SessionGuard {
    fn default() -> Self {
        Self::new()
    }
}

use crate::util::now_ms;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_not_paused() {
        let guard = SessionGuard::new();
        assert!(!guard.is_paused());
        assert_eq!(guard.remaining_ms(), 0);
        assert!(guard.assert_active().is_ok());
    }

    #[test]
    fn pause_and_check() {
        let guard = SessionGuard::new();
        guard.pause();
        assert!(guard.is_paused());
        assert!(guard.remaining_ms() > 0);
    }

    #[test]
    fn assert_active_when_paused() {
        let guard = SessionGuard::new();
        guard.pause();
        assert!(guard.assert_active().is_err());
    }
}
