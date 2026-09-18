//! Bounded launch selection, separate from bounded process/negotiation polling.
use super::*;
use std::time::{Duration, Instant};

impl ShellComponentSession {
    pub fn connected_roles(&self) -> [Option<(ComponentConnectionKey, ShellComponentRole)>; 2] {
        std::array::from_fn(|slot| {
            if !self.available || self.stopping {
                return None;
            }
            let ready = self.ready[slot].as_ref()?;
            (self.processes.phase(ready.key).ok() == Some(ComponentConnectionPhase::Connected))
                .then(|| (ready.key, self.plans[slot].selection().role))
        })
    }

    /// At most one launch attempt per visit, alternating the first selection.
    /// The caller polls/reaps and settles exact runtime claims before this call.
    /// A denied or not-yet-prepared role does not block its neighbor.
    pub fn start_next(
        &mut self,
        now: Instant,
        mut role_ready: impl FnMut(ShellComponentRole) -> bool,
    ) -> Result<Option<ComponentConnectionKey>> {
        if self.last_schedule.is_some_and(|last| now < last) {
            return Err("component scheduler clock regressed".into());
        }
        self.last_schedule = Some(now);
        if !self.available || self.stopping || self.revoked.len() != 0 {
            return Ok(None);
        }
        let count = self.plans.len();
        for offset in 0..count {
            let slot = (self.start_cursor + offset) % count;
            if !role_ready(self.plans[slot].selection().role)
                || self.retry_at[slot].is_some_and(|deadline| now < deadline)
            {
                continue;
            }
            if let Some(key) = self.processes.attempt(slot)
                && (self.processes.phase(key)? != ComponentConnectionPhase::Revoked
                    || self.processes.process_retained(key))
            {
                continue;
            }
            self.start_cursor = (slot + 1) % count;
            self.retry_at[slot] = Some(
                now.checked_add(Duration::from_secs(1))
                    .ok_or("component retry deadline overflow")?,
            );
            return self.start(slot).map(Some);
        }
        Ok(None)
    }
}
