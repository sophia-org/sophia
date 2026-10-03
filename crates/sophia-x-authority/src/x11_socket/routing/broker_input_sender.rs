// Enqueue-time input stamps and recovery reservations.
#[cfg(unix)]
#[derive(Clone)]
pub struct XAuthorityRoutedInputSender {
    sender: sophia_wake::SignalSender<XAuthorityEpochRoutedInput>,
    control_epoch: Arc<AtomicU64>,
    applied_control_epoch: Arc<AtomicU64>,
    capacity: usize,
    recovery: InputRecovery,
    /// Shared with the broker rather than copied from it.
    ///
    /// A sender handed out before the gate was installed would otherwise keep
    /// its own `None` and go on stamping from the bare counter, which is an
    /// ungated route into a gated broker.
    control_gate: Arc<std::sync::OnceLock<crate::ControlEpochGate>>,
}

#[cfg(unix)]
impl XAuthorityRoutedInputSender {
    /// Stamp work once, at enqueue.
    ///
    /// Without a coordinator this is the counter, exactly as before. With one,
    /// a transition in flight yields no stamp at all, so the work is refused
    /// here rather than queued against a revision that is being replaced.
    fn stamp(&self) -> Result<crate::ControlStamp, ()> {
        match self.control_gate.get() {
            Some(gate) => gate.stamp().map_err(|_| ()),
            None => Ok(crate::ControlStamp {
                control_epoch: self.control_epoch.load(Ordering::Acquire),
                publication: 0,
            }),
        }
    }
}

#[cfg(unix)]
impl XAuthorityRoutedInputSender {
    pub fn send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::SendError<XAuthorityRoutedInput>> {
        let stamp = match self.stamp() {
            Ok(stamp) => stamp,
            Err(()) => return Err(std::sync::mpsc::SendError(route)),
        };
        let envelope = XAuthorityEpochRoutedInput {
            control_epoch: stamp.control_epoch,
            publication: stamp.publication,
            route,
            reservation: None,
            completion: None,
        };
        if !self
            .recovery
            .admit(&envelope.route, envelope.control_epoch, Instant::now())
        {
            return Err(std::sync::mpsc::SendError(envelope.route));
        }
        self.sender.send(envelope).map_err(|error| {
            self.recovery.abort_enqueue(error.0.route.delivery);
            std::sync::mpsc::SendError(error.0.route)
        })
    }

    /// Stamp work and reserve its place in the recovery ledger.
    ///
    /// Reservation before acceptance, so a caller that is later refused has
    /// something exact to roll back rather than a guess. Every refusal here is
    /// typed at its source: the ledger being full, this delivery already being
    /// live, and the ledger being unreadable are three different answers, and
    /// only the first is worth retrying.
    fn stamp_and_reserve(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<
        (
            XAuthorityEpochRoutedInput,
            Option<PrivateAcceptedInputCompletion>,
        ),
        PrivateSendError,
    > {
        let stamp = match self.stamp() {
            Ok(stamp) => stamp,
            Err(()) => return Err(PrivateSendError::Denied(route)),
        };
        let envelope = XAuthorityEpochRoutedInput {
            control_epoch: stamp.control_epoch,
            publication: stamp.publication,
            route,
            reservation: None,
            completion: None,
        };
        match self.recovery.admit_with_completion(
            &envelope.route,
            envelope.control_epoch,
            Instant::now(),
        ) {
            Ok(completion) => Ok((envelope, completion)),
            Err(RecoveryAdmissionRefusal::LedgerFull) => {
                Err(PrivateSendError::Saturated(envelope.route))
            }
            Err(RecoveryAdmissionRefusal::DeliveryAlreadyTracked(_)) => {
                Err(PrivateSendError::DeliveryAlreadyTracked(envelope.route))
            }
            Err(RecoveryAdmissionRefusal::LedgerUnavailable) => {
                Err(PrivateSendError::Unavailable(envelope.route))
            }
        }
    }

    /// Release exactly one reservation, by its own delivery.
    ///
    /// Never another request's: a refusal rolls back what it reserved and
    /// leaves every live delivery alone.
    fn abort_reservation(&self, delivery: Option<XAuthorityInputDeliveryId>) {
        self.recovery.abort_enqueue(delivery);
    }

    pub fn try_send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityRoutedInput>> {
        let stamp = match self.stamp() {
            Ok(stamp) => stamp,
            Err(()) => return Err(std::sync::mpsc::TrySendError::Full(route)),
        };
        let envelope = XAuthorityEpochRoutedInput {
            control_epoch: stamp.control_epoch,
            publication: stamp.publication,
            route,
            reservation: None,
            completion: None,
        };
        if !self
            .recovery
            .admit(&envelope.route, envelope.control_epoch, Instant::now())
        {
            return Err(TrySendError::Full(envelope.route));
        }
        self.sender.try_send(envelope).map_err(|error| {
            let (envelope, full) = match error {
                TrySendError::Full(envelope) => (envelope, true),
                TrySendError::Disconnected(envelope) => (envelope, false),
            };
            self.recovery.abort_enqueue(envelope.route.delivery);
            if full {
                TrySendError::Full(envelope.route)
            } else {
                TrySendError::Disconnected(envelope.route)
            }
        })
    }

    /// The queue's bound, so a saturation report can say what was exhausted
    /// rather than only that something was.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn control_epoch(&self) -> u64 {
        self.control_epoch.load(Ordering::Acquire)
    }

    /// The epoch the frontend has finished applying: its grabs, frozen input
    /// and server grab cleared. A security transition is complete only when
    /// this reaches the epoch it requested; the request alone proves nothing.
    pub fn applied_control_epoch(&self) -> u64 {
        match self.control_gate.get() {
            Some(gate) => gate.applied_control_epoch(),
            None => self.applied_control_epoch.load(Ordering::Acquire),
        }
    }

    pub fn advance_control_epoch(&self, next: u64) -> bool {
        // A coordinator owns every transition it is installed for, so the
        // lockless path is refused rather than quietly racing it.
        if self.control_gate.get().is_some() {
            return false;
        }
        let mut current = self.control_epoch.load(Ordering::Acquire);
        loop {
            if next <= current {
                return next == current;
            }
            match self.control_epoch.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.sender.notify();
                    return true;
                }
                Err(observed) => current = observed,
            }
        }
    }
}
