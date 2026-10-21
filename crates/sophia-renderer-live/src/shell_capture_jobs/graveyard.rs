use super::{JobId, MAX_JOBS};
use std::sync::{Arc, Mutex, Weak};

struct State<P> {
    reserved: usize,
    quarantined: Vec<(JobId, P)>,
}

/// Renderer-owned storage, deliberately separate from completion receipts.
/// The real adapter must keep this owner until GPU use is proven ended.
/// EGL teardown alone is not that proof. There is no timeout, reset, or public
/// clear operation. The native adapter transfers submitted source custody into
/// its own process-bounded escrow before real GPU use.
pub(crate) struct CaptureGraveyard<P> {
    state: Mutex<State<P>>,
}

pub(super) struct Permit<P> {
    owner: Weak<CaptureGraveyard<P>>,
    armed: bool,
}

impl<P> CaptureGraveyard<P> {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                reserved: 0,
                quarantined: Vec::with_capacity(MAX_JOBS),
            }),
        })
    }

    pub(super) fn reserve(self: &Arc<Self>) -> Option<Permit<P>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.reserved == MAX_JOBS || !state.quarantined.is_empty() {
            return None;
        }
        state.reserved += 1;
        Some(Permit {
            owner: Arc::downgrade(self),
            armed: true,
        })
    }

    pub(super) fn quarantine(&self, id: JobId, payload: P, mut permit: Permit<P>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.quarantined.push((id, payload));
        // Keep this slot reserved until renderer teardown, not receipt disposal.
        permit.armed = false;
    }

    #[cfg(test)]
    pub(super) fn usage(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.reserved, state.quarantined.len())
    }
}

impl<P> Drop for Permit<P> {
    fn drop(&mut self) {
        if self.armed
            && let Some(owner) = self.owner.upgrade()
        {
            let mut state = owner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.reserved -= 1;
        }
    }
}
