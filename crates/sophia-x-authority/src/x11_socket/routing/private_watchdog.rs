// Independent execution supervision. This module takes no authority, route,
// output, or recovery lock and establishes no input or recipient completion.

use sophia_input_authority::ExecutionPhase;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const PRIVATE_EXECUTION_DEADLINE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrivateWatchdogCause {
    Deadline,
    ExecutionAbandoned,
    OwnerDropped,
    SupervisorPanicked,
    StateUnavailable,
    InvalidDequeueTime,
    IdentityExhausted,
}

/// A lifecycle failure and the last recorded phase, never an effect or
/// transport receipt. A commit marker does not prove a recipient arrival.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PrivateWatchdogFailure {
    pub(crate) cause: PrivateWatchdogCause,
    pub(crate) execution: Option<u64>,
    pub(crate) phase: Option<ExecutionPhase>,
    pub(crate) elapsed: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrivateWatchdogRefusal {
    NotSealed,
    Sealed,
    Closed,
    Busy,
    TransportCapacity,
    InvalidTransition,
    Failed(PrivateWatchdogFailure),
}

#[derive(Clone, Copy)]
struct Active {
    identity: u64,
    dequeued: Instant,
    phase: ExecutionPhase,
}

struct Transport {
    identity: u64,
    socket: Arc<UnixStream>,
}

struct Inventory {
    active: Option<Active>,
    next_identity: Option<u64>,
    failure: Option<PrivateWatchdogFailure>,
    stop: bool,
    sealed: bool,
    transports: Vec<Option<Transport>>,
}

struct Shared {
    inventory: Mutex<Inventory>,
    changed: Condvar,
    closed: AtomicBool,
    supervisor_finished: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inventory> {
        match self.inventory.lock() {
            Ok(inventory) => inventory,
            Err(poisoned) => {
                let mut inventory = poisoned.into_inner();
                self.fail(&mut inventory, PrivateWatchdogCause::StateUnavailable);
                inventory
            }
        }
    }

    fn fail(&self, inventory: &mut Inventory, cause: PrivateWatchdogCause) {
        inventory.failure.get_or_insert_with(|| {
            let now = Instant::now();
            PrivateWatchdogFailure {
                cause,
                execution: inventory.active.map(|active| active.identity),
                phase: inventory.active.map(|active| active.phase),
                elapsed: inventory
                    .active
                    .map(|active| now.saturating_duration_since(active.dequeued)),
            }
        });
        self.closed.store(true, Ordering::Release);
        self.changed.notify_all();
    }

    fn check_live(&self, inventory: &mut Inventory) -> Result<(), PrivateWatchdogRefusal> {
        if inventory.failure.is_none()
            && inventory
                .active
                .is_some_and(|active| active.dequeued.elapsed() >= PRIVATE_EXECUTION_DEADLINE)
        {
            // A delayed supervisor cannot let a late finish or phase
            // change erase an elapsed deadline.
            self.fail(inventory, PrivateWatchdogCause::Deadline);
        }
        if let Some(failure) = inventory.failure {
            Err(PrivateWatchdogRefusal::Failed(failure))
        } else if inventory.stop {
            Err(PrivateWatchdogRefusal::Closed)
        } else {
            Ok(())
        }
    }

    fn take_identity(&self, inventory: &mut Inventory) -> Result<u64, PrivateWatchdogRefusal> {
        let Some(identity) = inventory.next_identity else {
            self.fail(inventory, PrivateWatchdogCause::IdentityExhausted);
            return Err(PrivateWatchdogRefusal::Failed(
                inventory.failure.expect("failure just recorded"),
            ));
        };
        inventory.next_identity = identity.checked_add(1);
        Ok(identity)
    }
}

/// Constructed before producer exposure. Its supervisor owns a thread
/// independent of the worker and all execution guards. The owner neither
/// owns nor moves accepted input; failure leaves that inventory with the
/// worker and its existing terminal owner.
pub(crate) struct PrivateWatchdogOwner {
    shared: Arc<Shared>,
    supervisor: Option<JoinHandle<()>>,
}

impl PrivateWatchdogOwner {
    pub(crate) fn prepare(transport_capacity: usize) -> io::Result<Self> {
        let mut transports = Vec::new();
        transports
            .try_reserve_exact(transport_capacity)
            .map_err(|_| {
                io::Error::new(io::ErrorKind::OutOfMemory, "watchdog transport capacity")
            })?;
        transports.resize_with(transport_capacity, || None);
        let shared = Arc::new(Shared {
            inventory: Mutex::new(Inventory {
                active: None,
                next_identity: Some(1),
                failure: None,
                stop: false,
                sealed: false,
                transports,
            }),
            changed: Condvar::new(),
            closed: AtomicBool::new(false),
            supervisor_finished: AtomicBool::new(false),
        });
        let watching = shared.clone();
        let supervisor = thread::Builder::new()
            .name("private-input-watchdog".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    supervise(&watching);
                }));
                if result.is_err() {
                    let mut inventory = watching.lock();
                    watching.fail(&mut inventory, PrivateWatchdogCause::SupervisorPanicked);
                    drop(inventory);
                    shutdown_transports(&watching);
                }
                watching.supervisor_finished.store(true, Ordering::Release);
            })?;
        Ok(Self {
            shared,
            supervisor: Some(supervisor),
        })
    }

    /// Transfer an already independent descriptor before sealing. Refusal
    /// hands it back; attachment never tries to acquire an output mutex.
    /// The returned registration must outlive the client workers it covers.
    pub(crate) fn attach_transport(
        &mut self,
        socket: UnixStream,
    ) -> Result<PrivateWatchdogTransport, (PrivateWatchdogRefusal, UnixStream)> {
        let mut inventory = self.shared.lock();
        if let Err(refusal) = self.shared.check_live(&mut inventory) {
            return Err((refusal, socket));
        }
        if inventory.sealed {
            return Err((PrivateWatchdogRefusal::Sealed, socket));
        }
        let Some(slot) = inventory.transports.iter().position(Option::is_none) else {
            return Err((PrivateWatchdogRefusal::TransportCapacity, socket));
        };
        let identity = match self.shared.take_identity(&mut inventory) {
            Ok(identity) => identity,
            Err(refusal) => return Err((refusal, socket)),
        };
        inventory.transports[slot] = Some(Transport {
            identity,
            socket: Arc::new(socket),
        });
        Ok(PrivateWatchdogTransport {
            shared: self.shared.clone(),
            slot,
            identity,
        })
    }

    /// Finish descriptor preparation before exposing producers. Clones of
    /// this gate share one permanent failure latch and cannot begin work.
    pub(crate) fn seal(&mut self) -> Result<PrivateWatchdogGate, PrivateWatchdogRefusal> {
        let mut inventory = self.shared.lock();
        self.shared.check_live(&mut inventory)?;
        inventory.sealed = true;
        Ok(PrivateWatchdogGate {
            shared: self.shared.clone(),
        })
    }

    /// The caller records this Instant at actual execution dequeue, before
    /// acquiring common or X guards. Deliberate budget/frozen/delay waits
    /// happen before dequeue; they cannot restart an active deadline.
    pub(crate) fn begin_dequeued(
        &self,
        dequeued: Instant,
    ) -> Result<PrivateWatchedExecution, PrivateWatchdogRefusal> {
        let mut inventory = self.shared.lock();
        self.shared.check_live(&mut inventory)?;
        if !inventory.sealed {
            return Err(PrivateWatchdogRefusal::NotSealed);
        }
        if inventory.active.is_some() {
            return Err(PrivateWatchdogRefusal::Busy);
        }
        if dequeued > Instant::now() {
            self.shared
                .fail(&mut inventory, PrivateWatchdogCause::InvalidDequeueTime);
            return Err(PrivateWatchdogRefusal::Failed(
                inventory.failure.expect("failure just recorded"),
            ));
        }
        let identity = self.shared.take_identity(&mut inventory)?;
        inventory.active = Some(Active {
            identity,
            dequeued,
            phase: ExecutionPhase::BeforeGuards,
        });
        self.shared.changed.notify_all();
        self.shared.check_live(&mut inventory)?;
        Ok(PrivateWatchedExecution {
            shared: self.shared.clone(),
            identity,
            finished: false,
        })
    }

    /// Reap only a thread already known to have returned. No worker join is
    /// performed here, and Drop never joins even this supervisor.
    pub(crate) fn reap_finished(&mut self) -> Option<thread::Result<()>> {
        if !self.supervisor.as_ref()?.is_finished() {
            return None;
        }
        Some(self.supervisor.take().expect("finished supervisor").join())
    }
}

impl Drop for PrivateWatchdogOwner {
    fn drop(&mut self) {
        let mut inventory = self.shared.lock();
        if inventory.active.is_some() {
            self.shared
                .fail(&mut inventory, PrivateWatchdogCause::OwnerDropped);
        }
        inventory.stop = true;
        self.shared.closed.store(true, Ordering::Release);
        self.shared.changed.notify_all();
        // The JoinHandle detaches on field destruction. The thread owns
        // Shared until it exits and takes no execution lock to do so.
    }
}

/// Admission/consumer failure gate, with no issuer or execution rights.
#[derive(Clone)]
pub(crate) struct PrivateWatchdogGate {
    shared: Arc<Shared>,
}

impl PrivateWatchdogGate {
    /// One-way failure check, usable without any mutex. Final execution
    /// arbitration still happens at begin_dequeued, not through this read.
    pub(crate) fn allows_execution(&self) -> bool {
        !self.shared.closed.load(Ordering::Acquire)
    }

    pub(crate) fn failure(&self) -> Option<PrivateWatchdogFailure> {
        self.shared.lock().failure
    }

    pub(crate) fn supervisor_finished(&self) -> bool {
        self.shared.supervisor_finished.load(Ordering::Acquire)
    }
}

/// A descriptor registration held until the corresponding workers stop.
/// Concurrent removal cannot destroy a descriptor already selected by the
/// supervisor: shutdown retains a strong socket reference outside the lock.
pub(crate) struct PrivateWatchdogTransport {
    shared: Arc<Shared>,
    slot: usize,
    identity: u64,
}

impl Drop for PrivateWatchdogTransport {
    fn drop(&mut self) {
        let mut inventory = self.shared.lock();
        // Once failure selected this cohort, dropping a registration
        // cannot race its descriptor out from under the pending shutdown.
        // The failed inventory retains that bounded slot through cleanup.
        if inventory.failure.is_some() {
            return;
        }
        if inventory.transports[self.slot]
            .as_ref()
            .is_some_and(|transport| transport.identity == self.identity)
        {
            inventory.transports[self.slot] = None;
        }
    }
}

/// One owned, counted execution. No watchdog guard is kept across any call
/// to the authority or adapter; these methods take only the independent
/// supervisor mutex and release it before returning.
pub(crate) struct PrivateWatchedExecution {
    shared: Arc<Shared>,
    identity: u64,
    finished: bool,
}

impl PrivateWatchedExecution {
    pub(crate) fn applying(&mut self) -> Result<(), PrivateWatchdogRefusal> {
        self.advance(ExecutionPhase::BeforeGuards, ExecutionPhase::Applying)
    }

    pub(crate) fn committed(&mut self) -> Result<(), PrivateWatchdogRefusal> {
        self.advance(ExecutionPhase::Applying, ExecutionPhase::Committed)
    }

    fn advance(
        &self,
        from: ExecutionPhase,
        to: ExecutionPhase,
    ) -> Result<(), PrivateWatchdogRefusal> {
        let mut inventory = self.shared.lock();
        self.shared.check_live(&mut inventory)?;
        let Some(active) = inventory.active.as_mut() else {
            return Err(PrivateWatchdogRefusal::InvalidTransition);
        };
        if active.identity != self.identity || active.phase != from {
            return Err(PrivateWatchdogRefusal::InvalidTransition);
        }
        active.phase = to;
        Ok(())
    }

    /// Return without claiming what the operation did. A deadline already
    /// elapsed or latched cannot be overwritten by a late return.
    pub(crate) fn finish(mut self) -> Result<(), PrivateWatchdogRefusal> {
        let mut inventory = self.shared.lock();
        self.shared.check_live(&mut inventory)?;
        if inventory
            .active
            .is_none_or(|active| active.identity != self.identity)
        {
            return Err(PrivateWatchdogRefusal::InvalidTransition);
        }
        inventory.active = None;
        self.finished = true;
        self.shared.changed.notify_all();
        Ok(())
    }
}

impl Drop for PrivateWatchedExecution {
    fn drop(&mut self) {
        if !self.finished {
            let mut inventory = self.shared.lock();
            if inventory
                .active
                .is_some_and(|active| active.identity == self.identity)
            {
                self.shared
                    .fail(&mut inventory, PrivateWatchdogCause::ExecutionAbandoned);
            }
        }
    }
}

fn supervise(shared: &Shared) {
    let mut inventory = shared.lock();
    loop {
        let _ = shared.check_live(&mut inventory);
        if inventory.failure.is_some() {
            drop(inventory);
            shutdown_transports(shared);
            return;
        }
        if inventory.stop {
            return;
        }
        let result = if let Some(active) = inventory.active {
            let remaining = PRIVATE_EXECUTION_DEADLINE.saturating_sub(active.dequeued.elapsed());
            shared
                .changed
                .wait_timeout(inventory, remaining)
                .map(|(guard, _)| guard)
                .map_err(|poisoned| poisoned.into_inner().0)
        } else {
            shared
                .changed
                .wait(inventory)
                .map_err(|poisoned| poisoned.into_inner())
        };
        inventory = match result {
            Ok(inventory) => inventory,
            Err(mut inventory) => {
                shared.fail(&mut inventory, PrivateWatchdogCause::StateUnavailable);
                inventory
            }
        };
    }
}

fn shutdown_transports(shared: &Shared) {
    let count = shared.lock().transports.len();
    for slot in 0..count {
        let socket = shared.lock().transports[slot]
            .as_ref()
            .map(|transport| transport.socket.clone());
        if let Some(socket) = socket {
            // shutdown does not acquire the writer's output mutex, does
            // not wait for it to drain, and sends no fabricated receipt.
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}
