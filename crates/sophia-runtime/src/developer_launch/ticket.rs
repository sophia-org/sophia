use super::*;
use sophia_protocol::developer_launch::*;
use std::sync::atomic::{AtomicU8, AtomicU32};
use std::time::Instant;

struct State {
    // queued -> recheck requested -> verified -> dispatched; cancellation
    // wins against any state before dispatch. Only the worker inspects peers.
    phase: AtomicU8,
    result: AtomicU8,
    process: AtomicU32,
}
#[derive(Clone)]
pub struct DeveloperLaunchTicket {
    pub request: Arc<DeveloperLaunchRequest>,
    pub received: Instant,
    state: Arc<State>,
    shared: Arc<Shared>,
}
impl DeveloperLaunchTicket {
    pub(super) fn new(request: DeveloperLaunchRequest, shared: Arc<Shared>) -> Self {
        Self {
            request: Arc::new(request),
            received: Instant::now(),
            state: Arc::new(State {
                phase: AtomicU8::new(0),
                result: AtomicU8::new(0),
                process: AtomicU32::new(0),
            }),
            shared,
        }
    }
    /// Poll immediately before handing the request to the launch owner. A
    /// first call asks the worker to recheck admission; it never blocks Session.
    pub fn claim(&self) -> bool {
        if self.shared.stopped.load(Ordering::Acquire) {
            self.cancel();
            return false;
        }
        if self
            .state
            .phase
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.shared.worker_wake.notify();
        }
        self.state
            .phase
            .compare_exchange(4, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn cancelled(&self) -> bool {
        self.state.phase.load(Ordering::Acquire) == 2
    }
    pub fn dispatched(&self) -> bool {
        self.state.phase.load(Ordering::Acquire) == 1
    }
    pub(super) fn check_requested(&self) -> bool {
        self.state.phase.load(Ordering::Acquire) == 3
    }
    pub(super) fn verified(&self) {
        let _ = self
            .state
            .phase
            .compare_exchange(3, 4, Ordering::AcqRel, Ordering::Acquire);
        self.shared.owner_wake.notify();
    }
    pub(super) fn cancel(&self) {
        let _ = self
            .state
            .phase
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |p| {
                matches!(p, 0 | 3 | 4).then_some(2)
            });
        self.shared.owner_wake.notify();
    }
    /// Exactly one terminal observation wins. A timeout after dispatch cannot
    /// later turn into an apparently safe retry or overwrite a completed result.
    pub fn finish(&self, status: DeveloperLaunchStatus, process: Option<u32>) -> bool {
        let result = DeveloperLaunchResult {
            version: 1,
            request: self.request.request,
            status,
            process,
        };
        if result.validate().is_err()
            || (status == DeveloperLaunchStatus::Started && !self.dispatched())
        {
            return false;
        }
        // End a still-queued request before exposing any terminal observation.
        // A claim cannot slip between result publication and cancellation.
        self.cancel();
        if self
            .state
            .result
            .compare_exchange(0, 255, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.state
            .process
            .store(process.unwrap_or(0), Ordering::Relaxed);
        self.state.result.store(
            match status {
                DeveloperLaunchStatus::Started => 1,
                DeveloperLaunchStatus::Denied => 2,
                DeveloperLaunchStatus::Unavailable => 3,
                DeveloperLaunchStatus::Failed => 4,
                DeveloperLaunchStatus::Indeterminate => 5,
            },
            Ordering::Release,
        );
        self.shared.worker_wake.notify();
        true
    }
    pub(super) fn result(&self) -> Option<DeveloperLaunchResult> {
        let status = match self.state.result.load(Ordering::Acquire) {
            1 => DeveloperLaunchStatus::Started,
            2 => DeveloperLaunchStatus::Denied,
            3 => DeveloperLaunchStatus::Unavailable,
            4 => DeveloperLaunchStatus::Failed,
            5 => DeveloperLaunchStatus::Indeterminate,
            _ => return None,
        };
        Some(DeveloperLaunchResult {
            version: 1,
            request: self.request.request,
            status,
            process: (status == DeveloperLaunchStatus::Started)
                .then(|| self.state.process.load(Ordering::Relaxed)),
        })
    }
}
