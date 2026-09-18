use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLogoutDrainState {
    pub requested: bool,
    pub pending_input_deliveries: usize,
    pub pending_key_release_barriers: usize,
    pub pending_controls: usize,
    pub pending_wm_update: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLogoutDrainDecision {
    Running,
    Draining,
    Complete,
}

pub fn session_logout_drain_decision(state: SessionLogoutDrainState) -> SessionLogoutDrainDecision {
    if !state.requested {
        return SessionLogoutDrainDecision::Running;
    }
    if state.pending_input_deliveries != 0
        || state.pending_key_release_barriers != 0
        || state.pending_controls != 0
        || state.pending_wm_update
    {
        return SessionLogoutDrainDecision::Draining;
    }
    SessionLogoutDrainDecision::Complete
}

#[derive(Clone, Copy, Debug)]
pub struct SessionQuiescence {
    #[cfg_attr(not(feature = "native-session"), allow(dead_code))]
    pub(crate) reason: &'static str,
    started_at: Instant,
    pub(crate) deadline: Instant,
    pub(crate) frontend_authority_drained: bool,
}

impl SessionQuiescence {
    pub fn new(reason: &'static str, now: Instant, timeout: Duration) -> Self {
        Self {
            reason,
            started_at: now,
            deadline: now + timeout,
            frontend_authority_drained: false,
        }
    }

    pub fn mark_frontend_authority_drained(&mut self) {
        self.frontend_authority_drained = true;
    }

    /// Accepted controls can outlive frontend EOF and the final layout commit.
    /// Settlement wins at the deadline; outstanding work never extends it.
    pub fn decision(
        &self,
        now: Instant,
        snapshot: SessionQuiescenceSnapshot,
    ) -> SessionQuiescenceDecision {
        if self.frontend_authority_drained
            && snapshot.pending_authority_batches == 0
            && snapshot.pending_coordinator_work == 0
            && snapshot.pending_controls == 0
            && !snapshot.cpu_update_pending
            && !snapshot.native_work_pending
        {
            SessionQuiescenceDecision::Complete
        } else if now >= self.deadline {
            SessionQuiescenceDecision::TimedOut
        } else {
            SessionQuiescenceDecision::Pending
        }
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started_at)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionQuiescenceSnapshot {
    pub pending_authority_batches: usize,
    pub pending_coordinator_work: usize,
    pub pending_controls: usize,
    pub cpu_update_pending: bool,
    pub native_work_pending: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionQuiescenceDecision {
    Pending,
    Complete,
    TimedOut,
}
