//! Publication of the current shared connection, independent of PAM verdicts.
use sophia_factotum::launch::queue::Ingress;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LaunchServiceConnection {
    pub generation: u64,
    pub ingress: Ingress,
}

#[derive(Default)]
struct State {
    closed: bool,
    generation: u64,
    current: Option<LaunchServiceConnection>,
}

#[derive(Clone, Default)]
pub struct LaunchService(Arc<Mutex<State>>);
impl LaunchService {
    /// Engine-side observation never waits for the supervisor's publication.
    pub fn current(&self) -> Option<LaunchServiceConnection> {
        self.0
            .try_lock()
            .ok()?
            .current
            .as_ref()
            .filter(|c| c.ingress.is_open())
            .cloned()
    }

    pub(super) fn publish(&self, ingress: Option<Ingress>) -> bool {
        let mut state = self.0.lock().expect("launch service publication");
        if state.closed {
            if let Some(ingress) = ingress {
                ingress.invalidate();
            }
            return false;
        }
        if let Some(old) = state.current.take() {
            old.ingress.invalidate();
        }
        let Some(generation) = state.generation.checked_add(1) else {
            return false;
        };
        state.generation = generation;
        state.current = ingress.map(|ingress| LaunchServiceConnection {
            generation,
            ingress,
        });
        true
    }

    pub(super) fn withdraw(&self) {
        let mut state = self.0.lock().expect("launch service withdrawal");
        if let Some(old) = state.current.take() {
            old.ingress.invalidate();
        }
    }

    pub(super) fn close(&self) {
        let mut state = self.0.lock().expect("launch service closure");
        state.closed = true;
        if let Some(old) = state.current.take() {
            old.ingress.invalidate();
        }
    }
}
