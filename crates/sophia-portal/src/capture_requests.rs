//! One bounded request slot for an authenticated recipient. Session consumes
//! requests and decides output policy; this queue cannot approve a transfer.
use crate::execution_fence::PortalExecutionFence;
use sophia_protocol::output_capture_request::{
    OutputCaptureRequest, OutputCaptureState, OutputCaptureStatus,
};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

const REQUEST_TIMEOUT_MSEC: u64 = 10_000;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureRequestError {
    Invalid,
    Busy,
    Denied,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingCaptureRequest {
    owner: u64,
    request: OutputCaptureRequest,
    revision: u64,
    deadline: u64,
}
impl PendingCaptureRequest {
    pub fn execution_revision(self) -> u64 {
        self.revision
    }
    pub fn request(self) -> OutputCaptureRequest {
        self.request
    }
}

struct State {
    pending: Option<PendingCaptureRequest>,
    phase: OutputCaptureState,
}
pub struct CaptureRequestQueue {
    owner: u64,
    fence: PortalExecutionFence,
    state: Mutex<State>,
}
impl CaptureRequestQueue {
    pub fn new(fence: PortalExecutionFence) -> Self {
        Self {
            owner: NEXT_OWNER
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("capture request owner identities exhausted"),
            fence,
            state: Mutex::new(State {
                pending: None,
                phase: OutputCaptureState::Idle,
            }),
        }
    }

    /// Wire serials correlate replies and must increase within this owner.
    /// They do not select a namespace, source, grant or native presentation.
    pub fn submit(&self, request: OutputCaptureRequest) -> Result<(), CaptureRequestError> {
        request.encode().ok_or(CaptureRequestError::Invalid)?;
        self.status();
        let revision = self.fence.revision();
        let deadline = self
            .fence
            .now_msec()
            .checked_add(REQUEST_TIMEOUT_MSEC)
            .ok_or(CaptureRequestError::Denied)?;
        self.fence
            .execute(revision, deadline, || {
                let mut state = self.state.lock().map_err(|_| CaptureRequestError::Denied)?;
                if state
                    .pending
                    .is_some_and(|old| request.serial <= old.request.serial)
                {
                    return Err(CaptureRequestError::Stale);
                }
                if matches!(
                    state.phase,
                    OutputCaptureState::Queued | OutputCaptureState::Capturing
                ) {
                    return Err(CaptureRequestError::Busy);
                }
                state.pending = Some(PendingCaptureRequest {
                    owner: self.owner,
                    request,
                    revision,
                    deadline,
                });
                state.phase = OutputCaptureState::Queued;
                Ok(())
            })
            .unwrap_or(Err(CaptureRequestError::Denied))
    }

    pub fn take(&self) -> Option<PendingCaptureRequest> {
        self.status();
        let pending = self.state.lock().ok()?.pending?;
        self.fence
            .execute(pending.revision, pending.deadline, || {
                let mut state = self.state.lock().ok()?;
                if state.pending != Some(pending) || state.phase != OutputCaptureState::Queued {
                    return None;
                }
                state.phase = OutputCaptureState::Capturing;
                Some(pending)
            })
            .flatten()
    }

    /// Check again immediately before native scheduling. Lock/unlock never
    /// revives a request from the earlier interval.
    pub fn current(&self, pending: PendingCaptureRequest) -> bool {
        self.fence
            .execute(pending.revision, pending.deadline, || {
                self.state.lock().is_ok_and(|state| {
                    state.pending == Some(pending) && state.phase == OutputCaptureState::Capturing
                })
            })
            .unwrap_or(false)
    }

    pub fn finish(&self, pending: PendingCaptureRequest, result: OutputCaptureState) -> bool {
        if !result.terminal() {
            return false;
        }
        let finished = self
            .fence
            .execute(pending.revision, pending.deadline, || {
                let Ok(mut state) = self.state.lock() else {
                    return false;
                };
                if state.pending != Some(pending) || state.phase != OutputCaptureState::Capturing {
                    return false;
                }
                state.phase = result;
                true
            })
            .unwrap_or(false);
        if !finished {
            self.status();
        }
        finished
    }

    /// Historical status, not a lease. No image bytes are held by this queue.
    pub fn status(&self) -> OutputCaptureStatus {
        let pending = self.state.lock().ok().and_then(|state| state.pending);
        if let Some(pending) = pending
            && self
                .fence
                .execute(pending.revision, pending.deadline, || ())
                .is_none()
            && let Ok(mut state) = self.state.lock()
            && state.pending == Some(pending)
        {
            state.phase = OutputCaptureState::Cancelled;
        }
        self.state
            .lock()
            .map(|state| OutputCaptureStatus {
                request: state.pending.map(|pending| pending.request),
                state: state.phase,
            })
            .unwrap_or(OutputCaptureStatus {
                request: None,
                state: OutputCaptureState::Idle,
            })
    }
}
