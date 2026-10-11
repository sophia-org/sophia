//! Serializes a security transition with a recipient's actual effect. No
//! network operation or source-payload wait belongs inside the guarded call.
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct PortalExecutionFence {
    state: Arc<Mutex<State>>,
    origin: Instant,
}
struct State {
    revision: u64,
    blocked: bool,
    closed: bool,
}
impl Default for PortalExecutionFence {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                revision: 1,
                blocked: false,
                closed: false,
            })),
            origin: Instant::now(),
        }
    }
}
impl PortalExecutionFence {
    pub fn now_msec(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    pub fn revision(&self) -> u64 {
        self.state.lock().map_or(0, |s| s.revision)
    }
    pub fn live(&self) -> bool {
        self.state.lock().is_ok_and(|s| !s.blocked && !s.closed)
    }
    /// Returns only after any earlier guarded recipient effect has finished.
    pub fn block(&self) {
        self.transition(true, false);
    }
    pub fn unblock(&self) {
        self.transition(false, false);
    }
    pub fn close(&self) {
        self.transition(true, true);
    }
    fn transition(&self, blocked: bool, closed: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.closed |= closed;
            state.blocked = blocked;
            if let Some(revision) = state.revision.checked_add(1) {
                state.revision = revision;
            } else {
                state.closed = true;
            }
        }
    }
    /// The caller still checks namespace custody and source identity within
    /// `effect`. A failed mutex, old interval or expired deadline runs nothing.
    pub fn execute<T>(
        &self,
        revision: u64,
        deadline_msec: u64,
        effect: impl FnOnce() -> T,
    ) -> Option<T> {
        let state = self.state.lock().ok()?;
        if state.closed
            || state.blocked
            || state.revision != revision
            || self.now_msec() >= deadline_msec
        {
            return None;
        }
        let result = effect();
        drop(state);
        Some(result)
    }
}
