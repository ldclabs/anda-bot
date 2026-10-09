//! A short owner-held maintenance lease. Existing work drains; a crashed UI
//! cannot leave the daemon permanently closed to new work.
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Admission {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    lease: Option<(String, Instant)>,
    active: usize,
}

pub struct Permit(Arc<Admission>);

#[derive(Clone)]
pub struct AdmittedCron;

#[derive(Serialize)]
pub struct Status {
    pub paused: bool,
    pub active: usize,
}

const LEASE_TIME: Duration = Duration::from_secs(90);

/// Approvals and chat reads let admitted work finish, so every transport keeps
/// these tools available while maintenance is pending.
pub const MAINTENANCE_TOOLS: [&str; 3] = ["actions_api", "conversations_api", "resources_api"];

impl Admission {
    /// Continue authenticated nested work or cancellation while draining.
    /// Never use this path for ordinary new-task admission.
    pub fn enter_existing(self: &Arc<Self>) -> Permit {
        self.inner.lock().active += 1;
        Permit(self.clone())
    }

    pub fn enter(self: &Arc<Self>) -> Result<Permit, &'static str> {
        let mut state = self.inner.lock();
        Self::expire(&mut state);
        if state.lease.is_some() {
            return Err("Anda is preparing an update. Retry when maintenance finishes.");
        }
        state.active += 1;
        Ok(Permit(self.clone()))
    }

    fn expire(state: &mut Inner) {
        if state
            .lease
            .as_ref()
            .is_some_and(|(_, until)| *until <= Instant::now())
        {
            state.lease = None;
        }
    }

    pub fn begin(&self) -> Result<String, &'static str> {
        let mut state = self.inner.lock();
        Self::expire(&mut state);
        if state.lease.is_some() {
            return Err("Maintenance is already owned by another request");
        }
        let token = ic_auth_types::Xid::new().to_string();
        state.lease = Some((token.clone(), Instant::now() + LEASE_TIME));
        Ok(token)
    }

    /// Extends the lease `token` owns.
    pub fn renew(&self, token: &str) -> Result<(), &'static str> {
        let mut state = self.inner.lock();
        *Self::owned_deadline(&mut state, token)? = Instant::now() + LEASE_TIME;
        Ok(())
    }

    /// Ends the lease `token` owns and admits new work again.
    pub fn release(&self, token: &str) -> Result<(), &'static str> {
        let mut state = self.inner.lock();
        Self::owned_deadline(&mut state, token)?;
        state.lease = None;
        Ok(())
    }

    fn owned_deadline<'a>(
        state: &'a mut Inner,
        token: &str,
    ) -> Result<&'a mut Instant, &'static str> {
        Self::expire(state);
        match state.lease.as_mut() {
            Some((key, until)) if key == token => Ok(until),
            _ => Err("Maintenance lease expired or does not belong to this request"),
        }
    }

    pub fn status(&self) -> Status {
        let mut state = self.inner.lock();
        Self::expire(&mut state);
        Status {
            paused: state.lease.is_some(),
            active: state.active,
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.inner.lock().active -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_stops_new_work_but_preserves_existing_permits() {
        let gate = Arc::new(Admission::default());
        let permit = gate.enter().unwrap();
        let lease = gate.begin().unwrap();
        assert!(gate.enter().is_err());
        assert_eq!(gate.status().active, 1);
        assert!(gate.release("other").is_err());
        drop(permit);
        assert_eq!(gate.status().active, 0);
        gate.renew(&lease).unwrap();
        assert!(gate.status().paused);
        gate.release(&lease).unwrap();
        assert!(gate.enter().is_ok());
        assert!(gate.renew(&lease).is_err());
    }

    #[test]
    fn abandoned_lease_expires_without_changing_active_count() {
        let gate = Arc::new(Admission::default());
        gate.inner.lock().lease = Some(("old".into(), Instant::now() - Duration::from_secs(1)));
        assert!(!gate.status().paused);
        assert!(gate.enter().is_ok());
    }
}
