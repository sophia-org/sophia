//! Bounded launch selection, separate from bounded process/negotiation polling.
use super::*;
use std::time::{Duration, Instant};

/// The consecutive failure count at which the retry first spaces out. One
/// failure keeps the base interval, because a single transient loss should
/// recover at once rather than be punished.
const BACKOFF_BEGINS_AFTER: u32 = 2;
/// The base retry interval, and the ceiling the spacing grows to.
const RETRY_BASE: Duration = Duration::from_secs(1);
const RETRY_CEILING: Duration = Duration::from_secs(60);
/// A process that served this long before failing was not looping: its
/// failure starts the count afresh instead of adding to one that ended
/// before it came up. The ceiling is the natural bound, since a loop paced
/// at the ceiling fails at least this often.
const HEALTHY_TENURE: Duration = RETRY_CEILING;

/// Space a slot's retry by its consecutive failures: one second, then
/// doubling to a minute, and staying there.
///
/// The spacing never becomes infinite. A component that cannot start because
/// of a condition that later clears -- a device that appears late, an endpoint
/// still held by a retiring peer -- must still be able to come up without the
/// session being restarted, so this bounds the rate rather than the attempts.
fn retry_delay(attempts: u32) -> Duration {
    if attempts <= 1 {
        return RETRY_BASE;
    }
    RETRY_BASE
        .checked_mul(1_u32 << (attempts - 1).min(6))
        .unwrap_or(RETRY_CEILING)
        .min(RETRY_CEILING)
}

impl ShellComponentSession {
    pub fn connected_roles(
        &self,
    ) -> [Option<(ComponentConnectionKey, ShellComponentRole)>; MAX_SHELL_COMPONENTS] {
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
        // Clear before selecting. A failure raised before any slot is chosen
        // must not be attributed to the previous visit's selection, and the
        // backoff transition belongs to the visit that caused it.
        self.last_start_slot = None;
        self.entered_backoff = None;
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
            // Record the selection before attempting it. `start` reports a
            // failure that does not name the slot, and the caller has no other
            // way to attribute the retained record.
            self.last_start_slot = Some(slot);
            let outcome = self.start(slot);
            // A start that succeeds does not clear the count: a component
            // that comes up and fails in service within its tenure is still
            // looping, and the next failure must be spaced as the one before.
            // Only serving through a healthy tenure clears it.
            let failures = match outcome {
                Ok(_) => {
                    self.started_at[slot] = Some(now);
                    0
                }
                Err(_) => {
                    let failures = self.count_failure(slot, now);
                    if failures == BACKOFF_BEGINS_AFTER {
                        self.entered_backoff = Some(slot);
                    }
                    failures
                }
            };
            self.retry_at[slot] = Some(
                now.checked_add(retry_delay(failures))
                    .ok_or("component retry deadline overflow")?,
            );
            return outcome.map(Some);
        }
        Ok(None)
    }

    /// Count a service failure against the slot's retry spacing, after the
    /// caller has stopped the component. Returns whether this failure is the
    /// one at which the spacing first widens, so the caller can record the
    /// transition once, as it does for a refused start.
    pub fn record_service_failure(&mut self, slot: usize, now: Instant) -> Result<bool> {
        if slot >= self.plans.len() {
            return Err("unknown component selection".into());
        }
        let failures = self.count_failure(slot, now);
        self.retry_at[slot] = Some(
            now.checked_add(retry_delay(failures))
                .ok_or("component retry deadline overflow")?,
        );
        Ok(failures == BACKOFF_BEGINS_AFTER)
    }

    /// One more consecutive failure for the slot, unless its process had
    /// served a healthy tenure, in which case this is the first of a new run.
    fn count_failure(&mut self, slot: usize, now: Instant) -> u32 {
        let healthy = self.started_at[slot]
            .is_some_and(|began| now.saturating_duration_since(began) >= HEALTHY_TENURE);
        let failures = if healthy {
            1
        } else {
            self.failures[slot].saturating_add(1)
        };
        self.failures[slot] = failures;
        self.started_at[slot] = None;
        failures
    }
}

#[path = "../../../../tests/support/metadata_shell_scheduling.rs"]
mod tests;
