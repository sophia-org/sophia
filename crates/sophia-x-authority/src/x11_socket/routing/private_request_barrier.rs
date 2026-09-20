/// One submitter's view of one request's internal-processing completion.
///
/// NOT AN XTEST REPLY AND NOT A DELIVERY RECEIPT. What lands here is the value
/// the authority publishes inside the common guard after the effect. The
/// delivery receipt for the same work is strictly later and is drained
/// separately; a barrier built on that would be waiting for transport, and a
/// barrier built on acceptance would be waiting for a queue. FakeInput owes
/// neither: its next request waits until internal processing completes.
///
/// The slot is storage and the notifier is only a wakeup. That split is what
/// makes a lost or coalesced wake harmless: the outcome is already committed
/// where the waiter will look, so a wake that never arrives costs a waiter
/// nothing once anything else rouses it, and a wake that arrives twice finds
/// the same answer.
#[derive(Clone, Debug)]
pub struct PrivateRequestBarrier {
    slot: std::sync::Arc<PrivateBarrierSlot>,
}

/// The executor's half, carried by the reservation.
///
/// It travels with the work rather than being looked up, so publishing needs
/// no registry, no allocation and no search: whoever holds the request holds
/// the only way to answer it.
#[derive(Debug)]
pub struct PrivateBarrierTicket {
    slot: std::sync::Arc<PrivateBarrierSlot>,
    request: u64,
}

#[derive(Debug)]
struct PrivateBarrierSlot {
    /// A leaf. Taken while the common authority guard is held and never the
    /// other way round, and nothing at all is acquired while it is held.
    ///
    /// Reached through poison deliberately. Declining to read a slot because
    /// some unrelated thread panicked would strand a committed outcome behind
    /// a lock and leave a connection parked on it forever, which is a worse
    /// answer than reading a value that is, by construction, either fully
    /// written or absent.
    state: std::sync::Mutex<PrivateBarrierState>,
    /// Weak, so a runner holding this never pins a departed connection's
    /// descriptor.
    wake: crate::NotifierSubscription,
}

#[derive(Debug, Default)]
struct PrivateBarrierState {
    /// The request this slot is currently waiting on.
    ///
    /// Carried so a ticket from an abandoned request cannot answer the one
    /// that replaced it. Ids are per connection and only ever compared for
    /// equality against the slot's own.
    armed: Option<u64>,
    settled: Option<sophia_input_authority::RequestCompletion>,
}

impl PrivateRequestBarrier {
    /// Build a barrier that wakes this connection.
    pub fn over(notifier: &crate::ConnectionNotifier) -> Self {
        Self {
            slot: std::sync::Arc::new(PrivateBarrierSlot {
                state: std::sync::Mutex::new(PrivateBarrierState::default()),
                wake: notifier.subscription(),
            }),
        }
    }

    /// Claim the slot for one request and hand the executor its answer path.
    ///
    /// Arming replaces whatever the slot held. A previous outcome nobody took
    /// is discarded here rather than delivered to the wrong waiter, because a
    /// completion answers exactly one request and the request it answered is
    /// over.
    pub fn arm(&self, request: u64) -> PrivateBarrierTicket {
        let mut held = lock_barrier(&self.slot.state);
        held.armed = Some(request);
        held.settled = None;
        drop(held);
        PrivateBarrierTicket {
            slot: std::sync::Arc::clone(&self.slot),
            request,
        }
    }

    /// Take this request's outcome, disarming the slot.
    ///
    /// `None` means it has not been published yet, never that it was refused:
    /// a refusal is itself an outcome and arrives as one.
    pub fn take(&self) -> Option<sophia_input_authority::RequestCompletion> {
        let mut held = lock_barrier(&self.slot.state);
        let settled = held.settled.take();
        if settled.is_some() {
            held.armed = None;
        }
        settled
    }

    /// Whether a request is outstanding on this slot.
    pub fn armed(&self) -> bool {
        lock_barrier(&self.slot.state).armed.is_some()
    }
}

impl PrivateBarrierTicket {
    /// Store this request's outcome.
    ///
    /// Infallible and non-allocating on purpose: it is called under the common
    /// guard, immediately after the effect, where the authority permits no
    /// fallible publication. First writer wins, so a request answered from
    /// both its execution and its later observation keeps the earlier answer
    /// rather than overwriting a real outcome with a cancellation that only
    /// describes the cell being reclaimed.
    ///
    /// Reports whether the value was stored. False means the slot moved on to
    /// another request, which makes this outcome nobody's answer.
    pub fn report(&self, completion: sophia_input_authority::RequestCompletion) -> bool {
        let mut held = lock_barrier(&self.slot.state);
        if held.armed != Some(self.request) || held.settled.is_some() {
            return false;
        }
        held.settled = Some(completion);
        true
    }

    /// Raise the wake for whatever this slot already holds.
    ///
    /// Always outside the common guard, because notification routing stays
    /// outside the critical section. Idempotent, and its failure is not an
    /// error: a subscriber whose descriptor has gone is a connection that
    /// departed, and the stored outcome is what the runner still owes itself,
    /// not something the departed client is waiting for.
    pub fn flush(&self) {
        let _ = self.slot.wake.notify();
    }
}

/// Read a barrier slot whether or not some unrelated thread panicked holding
/// it. See the note on the mutex itself for why this never declines.
fn lock_barrier(
    state: &std::sync::Mutex<PrivateBarrierState>,
) -> std::sync::MutexGuard<'_, PrivateBarrierState> {
    state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
