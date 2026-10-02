use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use sophia_protocol::{
    ApplicationRouteLeaseIdentity, ClientAdmissionContext, SurfaceId, TransactionId,
};

pub const X_EXPLICIT_POINTER_GRAB_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct XAuthorityExplicitPointerGrabRequestId(u64);

impl XAuthorityExplicitPointerGrabRequestId {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityExplicitPointerGrabAnchor {
    Surface(SurfaceId),
    AdmissionDefault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityExplicitPointerGrabRequestKind {
    Prepare {
        anchor: XAuthorityExplicitPointerGrabAnchor,
        replaces: Option<ApplicationRouteLeaseIdentity>,
        after_observation: Option<TransactionId>,
        control_epoch: u64,
    },
    Activate {
        identity: ApplicationRouteLeaseIdentity,
    },
    BeginRelease {
        identity: ApplicationRouteLeaseIdentity,
    },
    FinishRelease {
        identity: ApplicationRouteLeaseIdentity,
    },
    Abort {
        identity: ApplicationRouteLeaseIdentity,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityExplicitPointerGrabRequest {
    pub id: XAuthorityExplicitPointerGrabRequestId,
    pub admission: ClientAdmissionContext,
    pub kind: XAuthorityExplicitPointerGrabRequestKind,
    pub deadline: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityExplicitPointerGrabRejection {
    AlreadyOwned,
    NotViewable,
    Stale,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityExplicitPointerGrabResponse {
    Prepared(ApplicationRouteLeaseIdentity),
    Activated,
    ReleaseReady,
    Released,
    Aborted,
    Rejected(XAuthorityExplicitPointerGrabRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityExplicitPointerGrabBridgeError {
    Capacity,
    Disconnected,
    Timeout,
    IdExhausted,
    Poisoned,
}

impl core::fmt::Display for XAuthorityExplicitPointerGrabBridgeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "explicit pointer-grab bridge failed: {self:?}")
    }
}

impl std::error::Error for XAuthorityExplicitPointerGrabBridgeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityExplicitPointerGrabResponseDisposition {
    Delivered,
    Cancelled,
}

#[derive(Default)]
struct XAuthorityExplicitPointerGrabResponseState {
    responses:
        BTreeMap<XAuthorityExplicitPointerGrabRequestId, XAuthorityExplicitPointerGrabResponse>,
    cancelled: BTreeSet<XAuthorityExplicitPointerGrabRequestId>,
    outstanding: BTreeMap<XAuthorityExplicitPointerGrabRequestId, (Instant, bool)>,
    closed: bool,
}

struct XAuthorityExplicitPointerGrabShared {
    owner_wake: sophia_wake::WakeSlot,
    request_gate: Mutex<()>,
    state: Mutex<XAuthorityExplicitPointerGrabResponseState>,
    ready: Condvar,
    next_id: AtomicU64,
    pending: AtomicUsize,
    prepare_capacity: usize,
}

#[derive(Clone)]
pub struct XAuthorityExplicitPointerGrabClient {
    requests: SyncSender<XAuthorityExplicitPointerGrabRequest>,
    shared: Arc<XAuthorityExplicitPointerGrabShared>,
}

impl core::fmt::Debug for XAuthorityExplicitPointerGrabClient {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("XAuthorityExplicitPointerGrabClient")
            .field("pending", &self.shared.pending.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl XAuthorityExplicitPointerGrabClient {
    pub fn set_owner_wake(&self, wake: sophia_wake::Notifier) {
        self.shared.owner_wake.set(wake);
    }
    pub fn request(
        &self,
        admission: ClientAdmissionContext,
        kind: XAuthorityExplicitPointerGrabRequestKind,
    ) -> Result<XAuthorityExplicitPointerGrabResponse, XAuthorityExplicitPointerGrabBridgeError>
    {
        let deadline = Instant::now() + X_EXPLICIT_POINTER_GRAB_TIMEOUT;
        let raw = self
            .shared
            .next_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| XAuthorityExplicitPointerGrabBridgeError::IdExhausted)?;
        let id = XAuthorityExplicitPointerGrabRequestId(raw);
        let request = XAuthorityExplicitPointerGrabRequest {
            id,
            admission,
            kind,
            deadline,
        };
        let request_gate = self
            .shared
            .request_gate
            .lock()
            .map_err(|_| XAuthorityExplicitPointerGrabBridgeError::Poisoned)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| XAuthorityExplicitPointerGrabBridgeError::Poisoned)?;
        if state.closed {
            return Err(XAuthorityExplicitPointerGrabBridgeError::Disconnected);
        }
        let prepare = matches!(
            kind,
            XAuthorityExplicitPointerGrabRequestKind::Prepare { .. }
        );
        if prepare
            && state
                .outstanding
                .values()
                .filter(|(_, prepare)| *prepare)
                .count()
                >= self.shared.prepare_capacity
        {
            return Err(XAuthorityExplicitPointerGrabBridgeError::Capacity);
        }
        match self.requests.try_send(request) {
            Ok(()) => {
                state.outstanding.insert(id, (deadline, prepare));
                self.shared.pending.fetch_add(1, Ordering::AcqRel);
                self.shared.owner_wake.notify();
            }
            Err(TrySendError::Full(_)) => {
                return Err(XAuthorityExplicitPointerGrabBridgeError::Capacity);
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(XAuthorityExplicitPointerGrabBridgeError::Disconnected);
            }
        }
        drop(request_gate);
        loop {
            if let Some(response) = state.responses.remove(&id) {
                return Ok(response);
            }
            if state.closed {
                return Err(XAuthorityExplicitPointerGrabBridgeError::Disconnected);
            }
            if state.cancelled.remove(&id) {
                return Err(XAuthorityExplicitPointerGrabBridgeError::Timeout);
            }
            let now = Instant::now();
            if now >= deadline {
                if state.outstanding.contains_key(&id) {
                    state.cancelled.insert(id);
                } else {
                    state.cancelled.remove(&id);
                }
                return Err(XAuthorityExplicitPointerGrabBridgeError::Timeout);
            }
            let wait = deadline.saturating_duration_since(now);
            let (next, timeout) = self
                .shared
                .ready
                .wait_timeout(state, wait)
                .map_err(|_| XAuthorityExplicitPointerGrabBridgeError::Poisoned)?;
            state = next;
            if timeout.timed_out() && !state.responses.contains_key(&id) {
                if state.outstanding.contains_key(&id) {
                    state.cancelled.insert(id);
                } else {
                    state.cancelled.remove(&id);
                }
                return Err(XAuthorityExplicitPointerGrabBridgeError::Timeout);
            }
        }
    }
}

pub struct XAuthorityExplicitPointerGrabOwner {
    requests: Receiver<XAuthorityExplicitPointerGrabRequest>,
    shared: Arc<XAuthorityExplicitPointerGrabShared>,
}

impl core::fmt::Debug for XAuthorityExplicitPointerGrabOwner {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("XAuthorityExplicitPointerGrabOwner")
            .field("pending", &self.pending())
            .finish_non_exhaustive()
    }
}

impl XAuthorityExplicitPointerGrabOwner {
    pub fn try_recv(&self) -> Result<XAuthorityExplicitPointerGrabRequest, TryRecvError> {
        let Ok(_request_gate) = self.shared.request_gate.lock() else {
            return Err(TryRecvError::Disconnected);
        };
        self.requests.try_recv()
    }

    pub fn respond(
        &self,
        id: XAuthorityExplicitPointerGrabRequestId,
        response: XAuthorityExplicitPointerGrabResponse,
    ) -> Result<
        XAuthorityExplicitPointerGrabResponseDisposition,
        XAuthorityExplicitPointerGrabBridgeError,
    > {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| XAuthorityExplicitPointerGrabBridgeError::Poisoned)?;
        let Some((deadline, _)) = state.outstanding.remove(&id) else {
            return Ok(XAuthorityExplicitPointerGrabResponseDisposition::Cancelled);
        };
        self.shared.pending.fetch_sub(1, Ordering::AcqRel);
        let caller_cancelled = state.cancelled.remove(&id);
        if caller_cancelled || state.closed || Instant::now() >= deadline {
            if !caller_cancelled {
                state.cancelled.insert(id);
            }
            self.shared.ready.notify_all();
            return Ok(XAuthorityExplicitPointerGrabResponseDisposition::Cancelled);
        }
        state.responses.insert(id, response);
        self.shared.ready.notify_all();
        Ok(XAuthorityExplicitPointerGrabResponseDisposition::Delivered)
    }

    pub fn is_cancelled(
        &self,
        id: XAuthorityExplicitPointerGrabRequestId,
    ) -> Result<bool, XAuthorityExplicitPointerGrabBridgeError> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| XAuthorityExplicitPointerGrabBridgeError::Poisoned)?;
        Ok(state.closed
            || state.cancelled.contains(&id)
            || state
                .outstanding
                .get(&id)
                .is_none_or(|(deadline, _)| Instant::now() >= *deadline))
    }

    pub fn pending(&self) -> usize {
        self.shared.pending.load(Ordering::Acquire)
    }
}

impl Drop for XAuthorityExplicitPointerGrabOwner {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.closed = true;
            self.shared.ready.notify_all();
        }
    }
}

pub fn x_authority_explicit_pointer_grab_bridge(
    capacity: NonZeroUsize,
) -> (
    XAuthorityExplicitPointerGrabClient,
    XAuthorityExplicitPointerGrabOwner,
) {
    let (requests, receiver) = sync_channel(capacity.get().saturating_mul(2));
    let shared = Arc::new(XAuthorityExplicitPointerGrabShared {
        request_gate: Mutex::new(()),
        state: Mutex::new(XAuthorityExplicitPointerGrabResponseState::default()),
        ready: Condvar::new(),
        next_id: AtomicU64::new(1),
        pending: AtomicUsize::new(0),
        prepare_capacity: capacity.get(),
        owner_wake: sophia_wake::WakeSlot::default(),
    });
    (
        XAuthorityExplicitPointerGrabClient {
            requests,
            shared: shared.clone(),
        },
        XAuthorityExplicitPointerGrabOwner {
            requests: receiver,
            shared,
        },
    )
}
