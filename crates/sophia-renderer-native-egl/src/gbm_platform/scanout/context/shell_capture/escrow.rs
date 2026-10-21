//! Process-lifetime quarantine for the experimental shell-copy owner.
//!
//! A failed observation is not a source-release event. Drop therefore keeps
//! submitted custody alive and poisons admission, including after context
//! replacement. No destructor attempts EGL work or manufactures completion.
use std::sync::{Arc, Mutex, OnceLock};

const MAX_COPIES: usize = 4;

#[derive(Default)]
struct State {
    reserved: usize,
    quarantined: bool,
}

#[derive(Default)]
pub(super) struct Capacity(Mutex<State>);

impl Capacity {
    pub(super) fn process() -> Arc<Self> {
        static CAPACITY: OnceLock<Arc<Capacity>> = OnceLock::new();
        CAPACITY.get_or_init(|| Arc::new(Self::default())).clone()
    }

    pub(super) fn reserve(self: &Arc<Self>) -> Option<Slot> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.quarantined || state.reserved >= MAX_COPIES {
            return None;
        }
        state.reserved += 1;
        Some(Slot {
            owner: self.clone(),
            returned: false,
        })
    }
}

pub(super) struct Slot {
    owner: Arc<Capacity>,
    returned: bool,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if !self.returned {
            self.owner
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reserved -= 1;
        }
    }
}

/// Construct immediately before the first potentially source-using operation.
/// Only the native adapter may extract it after its exact fence proof. A failed
/// start, unwind, abandoned pending handle or uncertain poll retains everything.
pub(super) struct Escrow<P> {
    payload: Option<P>,
    slot: Slot,
}

impl<P> Escrow<P> {
    pub(super) fn new(payload: P, slot: Slot) -> Self {
        Self {
            payload: Some(payload),
            slot,
        }
    }

    pub(super) fn payload(&self) -> &P {
        self.payload.as_ref().expect("submitted custody")
    }

    pub(super) fn payload_mut(&mut self) -> &mut P {
        self.payload.as_mut().expect("submitted custody")
    }

    // Private to the native adapter, never an exported completion mechanism.
    pub(super) fn finish(mut self) -> P {
        self.payload.take().expect("completed custody")
    }
}

impl<P> Drop for Escrow<P> {
    fn drop(&mut self) {
        if let Some(payload) = self.payload.take() {
            let mut state = self.slot.owner.0.lock().unwrap_or_else(|e| e.into_inner());
            state.quarantined = true;
            // Keep the slot charged too. The process cap cannot reset with a
            // renderer, channel, request or epoch. This intentionally retains
            // the FDs and reservation until process exit, without release credit.
            self.slot.returned = true;
            std::mem::forget(payload);
        }
    }
}

#[cfg(test)]
#[path = "../../../../../tests/support/shell_capture_escrow.rs"]
mod tests;
