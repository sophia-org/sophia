// Delivery recovery is independent of both the socket writer lock and the
// frontend command queues. No event payload is retained here.
#[cfg(unix)]
pub const X_AUTHORITY_INPUT_DELIVERY_DEADLINE: Duration = Duration::from_secs(6);

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
pub struct XAuthorityInputDeliveryTicket {
    pub delivery: XAuthorityInputDeliveryId,
    pub surface: SurfaceId,
    pub seat: SeatId,
    pub control_epoch: u64,
    pub admitted_at: Instant,
    pub client: Option<XServerFrontendClientId>,
}

#[cfg(unix)]
struct TrackedInputDelivery {
    ticket: XAuthorityInputDeliveryTicket,
    terminal: Option<XAuthorityClientInputDelivery>,
    observed: bool,
    routing_finished: bool,
    /// An execution holds this delivery and its effect may already be under
    /// way.
    ///
    /// Not a lock: the ledger's own guard is released while this is set, so
    /// that the execution can take the guards it needs in their own rank. It
    /// is an arbitration marker. A cancellation arriving while it is set has
    /// lost the race, and publishing a terminal outcome for it anyway would
    /// be a claim that the effect then contradicts.
    claimed: bool,
    /// Whether any execution of this delivery may have applied an effect.
    ///
    /// Cumulative and never cleared. One claim resolving as having applied
    /// nothing says what THAT execution did, not what the delivery has been
    /// through: an earlier one may already have moved the ledger and cleared a
    /// projection. Letting a later per-claim answer erase the earlier fact
    /// puts a cancellation back in reach of a delivery whose effect already
    /// happened, one claim cycle later than the arbitration prevented it.
    may_have_applied: bool,
    /// What a cancellation wanted to record while this was claimed.
    ///
    /// Held rather than dropped, because a cancellation that lost to an
    /// execution which then applied nothing has not lost at all. The first is
    /// kept: later ones describe the same delivery already being cancelled.
    deferred: Option<DeferredCancellation>,
}

/// Whether an outcome asserts something about the delivery before its effect.
///
/// A cancellation says the delivery was withdrawn: it never happened, and
/// whoever was waiting may stop. Once an effect may have happened, that is a
/// claim the effect contradicts, and it must not be published however it
/// arrives -- held over from a claim, or fresh from a later sweep.
///
/// What an established recipient fact says is different in kind. A client that
/// disconnected, a write that failed, a flush that reached it: those describe
/// what became of the delivery, not a denial that it occurred, and they stay
/// publishable. Conflating the two would leave a delivery whose effect
/// happened with no way to ever be answered.
#[cfg(unix)]
fn cancels_before_the_effect(outcome: XAuthorityInputDeliveryOutcome) -> bool {
    match outcome {
        // The epoch withdrew the request, or the route was refused before it
        // was carried. Both say it did not happen.
        XAuthorityInputDeliveryOutcome::EpochRevoked
        | XAuthorityInputDeliveryOutcome::RouteRejected
        | XAuthorityInputDeliveryOutcome::TargetGone => true,
        // What became of a delivery that did happen. A deadline passing with
        // no receipt is a statement about the receipt, not a denial of the
        // effect, so it stays available -- otherwise a delivery whose writer
        // never reported could never be answered at all.
        XAuthorityInputDeliveryOutcome::Flushed
        | XAuthorityInputDeliveryOutcome::WriteFailed
        | XAuthorityInputDeliveryOutcome::ClientDisconnected
        | XAuthorityInputDeliveryOutcome::TimedOut => false,
    }
}

/// A cancellation that lost to a claim, with the identity it was recorded
/// under.
///
/// The identity is kept beside the receipt because binding can happen between
/// the two: a delivery with no recipient yet is cancelled naming none, and by
/// the time the claim is given back it may have one. Publishing the original
/// receipt then fails the ledger's own identity check and the cancellation is
/// lost, which is the silent loss deferring it was meant to prevent.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct DeferredCancellation {
    receipt: XAuthorityClientInputDelivery,
    /// What the ticket was bound to when this was deferred. `None` means the
    /// delivery had no recipient, so the receipt names none either.
    bound: Option<XServerFrontendClientId>,
}

#[cfg(unix)]
struct InputRecoveryConnection {
    lifecycle: Option<PrivateLifecycleGate>,
    // A distinct descriptor for the SAME socket. shutdown interrupts every
    // writer without acquiring the mutex protecting output serialization.
    socket: Option<UnixStream>,
    revoked: bool,
}

#[cfg(unix)]
#[derive(Default)]
struct InputRecoveryState {
    tickets: BTreeMap<XAuthorityInputDeliveryId, TrackedInputDelivery>,
    connections: BTreeMap<XServerFrontendClientId, InputRecoveryConnection>,
}

#[cfg(unix)]
#[derive(Clone)]
struct InputRecovery {
    lifecycle: Arc<std::sync::OnceLock<PrivateLifecycleOwner>>,
    /// Setup-only role: independent transport registration, never execution
    /// or cleanup authority. Installed before a private frontend is returned.
    watchdog: Arc<std::sync::OnceLock<private_watchdog::PrivateWatchdogRegistrar>>,
    state: Arc<Mutex<InputRecoveryState>>,
    sender: Option<Sender<XAuthorityClientInputDelivery>>,
    capacity: usize,
    // The ordinary compatibility path keeps its admission-age policy. Private
    // execution requires writer-produced transport deadlines instead.
    admission_deadline: Arc<std::sync::atomic::AtomicBool>,
    authority: Arc<Mutex<crate::XInputAuthorityState>>,
}

#[cfg(unix)]
/// What the recovery ledger knows about a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    /// Still tracked, so still owed an outcome.
    Live,
    /// Tracked and finished, both recorded and observed.
    Ended,
    /// The ledger cannot be read, so nothing about this delivery is known.
    /// Not the same as ended.
    Unavailable,
}

/// The answer to an execution asking to hold a delivery while it applies it.
///
/// Four answers, because a caller that has to record why it did not run needs
/// a decision, the absence of one, and a contention apart. A ledger nobody can
/// read has not said this delivery ended, and something else already executing
/// it is not the same as it having finished.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionClaim {
    /// Held. Cancellation can no longer publish an outcome for it until the
    /// claim resolves, and the claim must be resolved however this execution
    /// ends.
    Claimed,
    /// A terminal outcome is already recorded for it. Cancellation won.
    Ended,
    /// Another execution holds it.
    Contended,
    /// The ledger could not be read, so nothing is known about it.
    Unavailable,
}

/// Why the recovery ledger would not track a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAdmissionRefusal {
    /// The ledger has no room. Retrying later is sensible.
    LedgerFull,
    /// This delivery id is already live. Retrying cannot help, and cancelling
    /// the live one would answer a different request.
    DeliveryAlreadyTracked(XAuthorityInputDeliveryId),
    /// The ledger cannot be read. Nothing can be tracked until that is over.
    LedgerUnavailable,
}

impl InputRecovery {
    fn new(
        capacity: usize,
        sender: Option<Sender<XAuthorityClientInputDelivery>>,
        authority: Arc<Mutex<crate::XInputAuthorityState>>,
    ) -> Self {
        Self {
            lifecycle: Arc::new(std::sync::OnceLock::new()),
            watchdog: Arc::new(std::sync::OnceLock::new()),
            state: Arc::default(),
            sender,
            capacity,
            admission_deadline: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authority,
        }
    }

    /// Installed before private producers or registrations are exposed.
    /// No writer has blocked merely because a ticket waited in a queue.
    /// This disables the legacy age producer; it creates no transport receipt.
    fn require_writer_deadline(&self) {
        self.admission_deadline.store(false, Ordering::Release);
    }

    fn admit(&self, route: &XAuthorityRoutedInput, epoch: u64, now: Instant) -> bool {
        self.admit_typed(route, epoch, now).is_ok()
    }

    /// Admit, saying which refusal this is.
    ///
    /// The boolean above answers three different questions with one word: the
    /// ledger is full, this delivery id is already live, or the ledger cannot
    /// be read at all. A caller told only `false` reports all three as
    /// saturation, which invites a retry that will never succeed for two of
    /// them.
    fn admit_typed(
        &self,
        route: &XAuthorityRoutedInput,
        epoch: u64,
        now: Instant,
    ) -> Result<(), RecoveryAdmissionRefusal> {
        let Some(delivery) = route.delivery else {
            return Ok(());
        };
        let Ok(mut state) = self.state.lock() else {
            return Err(RecoveryAdmissionRefusal::LedgerUnavailable);
        };
        if state.tickets.contains_key(&delivery) {
            return Err(RecoveryAdmissionRefusal::DeliveryAlreadyTracked(delivery));
        }
        if state.tickets.len() >= self.capacity {
            return Err(RecoveryAdmissionRefusal::LedgerFull);
        }
        state.tickets.insert(
            delivery,
            TrackedInputDelivery {
                claimed: false,
                may_have_applied: false,
                deferred: None,
                ticket: XAuthorityInputDeliveryTicket {
                    delivery,
                    surface: route.request.target_surface,
                    seat: route.request.seat,
                    control_epoch: epoch,
                    admitted_at: now,
                    client: None,
                },
                terminal: None,
                observed: false,
                routing_finished: false,
            },
        );
        Ok(())
    }

    fn abort_enqueue(&self, delivery: Option<XAuthorityInputDeliveryId>) {
        if let Some(id) = delivery
            && let Ok(mut state) = self.state.lock()
        {
            state.tickets.remove(&id);
        }
    }

    /// Whether a delivery is still live, unreachable, or done.
    ///
    /// Three answers, because an unreadable ledger is not an ended delivery.
    /// A caller told only "absent" would treat a poisoned ledger as every
    /// delivery having finished, which is the most dangerous reading
    /// available: it frees whatever those deliveries were holding.
    fn delivery_state(&self, id: XAuthorityInputDeliveryId) -> DeliveryState {
        let Ok(state) = self.state.lock() else {
            return DeliveryState::Unavailable;
        };
        if state.tickets.contains_key(&id) {
            DeliveryState::Live
        } else {
            DeliveryState::Ended
        }
    }

    fn ticket(&self, id: XAuthorityInputDeliveryId) -> Option<XAuthorityInputDeliveryTicket> {
        self.state
            .lock()
            .ok()?
            .tickets
            .get(&id)
            .map(|entry| entry.ticket)
    }

    // Cancellation before resolution leaves a bounded tombstone until the
    // frontend consumes the ingress/frozen entry. It cannot resurrect later.
    fn begin_routing(&self, id: Option<XAuthorityInputDeliveryId>) -> bool {
        let Some(id) = id else { return true };
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(entry) = state.tickets.get_mut(&id) else {
            return true;
        };
        if entry.terminal.is_none() {
            return true;
        }
        entry.routing_finished = true;
        if entry.observed {
            state.tickets.remove(&id);
        }
        false
    }

    /// Hold this delivery for the duration of an execution.
    ///
    /// Arbitration, not a look. Asking whether a delivery is current and then
    /// applying it leaves a gap in which a cancellation can publish a terminal
    /// outcome that the effect goes on to contradict, and no guard spans that
    /// gap: the ledger's own is released before the execution takes the guards
    /// it needs, in their rank. What spans it is this claim.
    ///
    /// Whoever claims must resolve, however the execution ends, or the
    /// delivery can never be cancelled again.
    fn claim_execution(&self, id: Option<XAuthorityInputDeliveryId>) -> ExecutionClaim {
        let Some(id) = id else {
            return ExecutionClaim::Claimed;
        };
        let Ok(mut state) = self.state.lock() else {
            return ExecutionClaim::Unavailable;
        };
        let Some(entry) = state.tickets.get_mut(&id) else {
            // Untracked, so there is nothing to arbitrate over and nothing to
            // resolve. Resolving an absent claim is a no-op.
            return ExecutionClaim::Claimed;
        };
        if entry.terminal.is_some() {
            entry.routing_finished = true;
            if entry.observed {
                state.tickets.remove(&id);
            }
            return ExecutionClaim::Ended;
        }
        if entry.claimed {
            return ExecutionClaim::Contended;
        }
        entry.claimed = true;
        ExecutionClaim::Claimed
    }

    /// Give up a claim, saying whether an effect may have happened under it.
    ///
    /// `may_have_applied` is what decides a cancellation that arrived while
    /// the claim was held. If nothing was applied, that cancellation had
    /// nothing to contradict and it stands. If something may have been, it
    /// cannot be published as this delivery's outcome -- the delivery stays
    /// owed one, which its writer result or its deadline answers.
    ///
    /// The unknown case is counted as applied. A cancellation published over
    /// an effect that did happen is the failure this exists to prevent; a
    /// delivery left owed an outcome is answered by the deadline.
    fn resolve_claim(&self, id: Option<XAuthorityInputDeliveryId>, may_have_applied: bool) {
        let Some(id) = id else { return };
        // Reached through poison. This gives back something only this caller
        // holds, and declining leaves a delivery permanently claimed: nothing
        // could ever cancel it again, and no owner would know it was owed.
        // What it does here is bounded -- clear a flag this caller set, and
        // resolve a cancellation already decided elsewhere -- and neither
        // reading depends on the rest of the ledger being consistent.
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = state.tickets.get_mut(&id) else {
            return;
        };
        entry.claimed = false;
        // Accumulated, not assigned. What this execution did is added to what
        // the delivery has been through.
        entry.may_have_applied |= may_have_applied;
        if entry.may_have_applied {
            // An effect may have happened, so a cancellation cannot become
            // this delivery's outcome -- not now and not after a later claim
            // that happens to apply nothing. It is kept rather than dropped
            // because it remains a record of what was attempted, and the
            // publication path above refuses it on the same fact.
            return;
        }
        let Some(deferred) = entry.deferred.take() else {
            return;
        };
        let receipt = match (deferred.bound, entry.ticket.client) {
            // Unchanged, so the receipt still names what the ledger does.
            (was, now) if was == now => deferred.receipt,
            // The delivery had no recipient when it was cancelled and has one
            // now. Resolving an identity the cancellation left open is not
            // inventing one: the outcome was always this delivery's.
            (None, Some(client)) => XAuthorityClientInputDelivery {
                client,
                ..deferred.receipt
            },
            // Bound to one recipient when cancelled and to another now.
            // Nothing here can say which the cancellation meant, so the
            // obligation goes back rather than being published against a
            // client it never named or dropped for not fitting.
            _ => {
                entry.deferred = Some(deferred);
                return;
            }
        };
        self.terminal_locked(&mut state, receipt);
    }

    fn bind(
        &self,
        id: Option<XAuthorityInputDeliveryId>,
        client: XServerFrontendClientId,
    ) -> Result<bool, XServerFrontendRouteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let revoked = state
            .connections
            .get(&client)
            .is_some_and(|connection| connection.revoked);
        let Some(entry) = id.and_then(|id| state.tickets.get_mut(&id)) else {
            // Legacy, already client-addressed test/proof ingress.
            return Ok(!revoked);
        };
        entry.routing_finished = true;
        if entry.terminal.is_some() {
            if entry.observed {
                state.tickets.remove(&id.expect("tracked ID"));
            }
            return Ok(false);
        }
        entry.ticket.client = Some(client);
        if revoked {
            self.terminal_locked(
                &mut state,
                XAuthorityClientInputDelivery {
                    client,
                    delivery: id.expect("tracked ID"),
                    outcome: XAuthorityInputDeliveryOutcome::ClientDisconnected,
                },
            );
        }
        Ok(!revoked)
    }

    fn active(
        &self,
        id: Option<XAuthorityInputDeliveryId>,
        client: XServerFrontendClientId,
    ) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        if state
            .connections
            .get(&client)
            .is_some_and(|connection| connection.revoked)
        {
            return false;
        }
        id.and_then(|id| state.tickets.get(&id))
            .is_none_or(|entry| entry.terminal.is_none() && entry.ticket.client == Some(client))
    }

    fn terminal_locked(
        &self,
        state: &mut InputRecoveryState,
        receipt: XAuthorityClientInputDelivery,
    ) {
        if let Some(entry) = state.tickets.get_mut(&receipt.delivery) {
            if entry.terminal.is_some()
                || entry
                    .ticket
                    .client
                    .is_some_and(|client| client != receipt.client)
            {
                return;
            }
            if entry.may_have_applied && cancels_before_the_effect(receipt.outcome) {
                // An effect may already have happened for this delivery.
                // Saying now that it was withdrawn would be the contradiction
                // the claim exists to prevent, arriving after the claim rather
                // than during it. The delivery stays owed an outcome, which
                // its writer result or an established recipient fact answers.
                return;
            }
            if entry.claimed {
                // An execution holds this delivery and its effect may already
                // have happened. Publishing now would tell everyone waiting
                // that it ended, and the effect would then contradict that.
                // Held until the claim resolves, which is where it is decided
                // whether this cancellation had anything to contradict.
                let bound = entry.ticket.client;
                entry
                    .deferred
                    .get_or_insert(DeferredCancellation { receipt, bound });
                return;
            }
            entry.terminal = Some(receipt);
        }
        if let Some(sender) = &self.sender {
            let _ = sender.send(receipt);
        }
    }

    fn finish(
        &self,
        client: XServerFrontendClientId,
        id: Option<XAuthorityInputDeliveryId>,
        outcome: XAuthorityInputDeliveryOutcome,
    ) -> Result<(), XServerFrontendRouteError> {
        let Some(delivery) = id else { return Ok(()) };
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let revoked = state
            .connections
            .get(&client)
            .is_some_and(|connection| connection.revoked);
        // A route rejected before queue publication keeps its precise failure
        // even if an earlier expanded event caused the connection to close.
        // Revocation can never turn a late writer result into a successful flush.
        let outcome = if revoked && outcome == XAuthorityInputDeliveryOutcome::Flushed {
            XAuthorityInputDeliveryOutcome::ClientDisconnected
        } else {
            outcome
        };
        if let Some(entry) = state.tickets.get_mut(&delivery) {
            entry.routing_finished = true;
            if entry.terminal.is_some() && entry.observed {
                state.tickets.remove(&delivery);
                return Ok(());
            }
        } else if revoked && outcome != XAuthorityInputDeliveryOutcome::RouteRejected {
            return Ok(());
        }
        self.terminal_locked(
            &mut state,
            XAuthorityClientInputDelivery {
                client,
                delivery,
                outcome,
            },
        );
        Ok(())
    }

    fn observe(&self, receipt: XAuthorityClientInputDelivery) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(entry) = state.tickets.get_mut(&receipt.delivery) else {
            return false;
        };
        if entry.observed || entry.terminal != Some(receipt) {
            return false;
        }
        entry.observed = true;
        if entry.routing_finished {
            state.tickets.remove(&receipt.delivery);
        }
        true
    }

    fn register(&self, client: XServerFrontendClientId) -> Result<(), XServerFrontendRouteError> {
        self.state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .connections
            .insert(
                client,
                InputRecoveryConnection {
                    lifecycle: None,
                    socket: None,
                    revoked: false,
                },
            );
        Ok(())
    }

    fn attach_lifecycle(
        &self,
        client: XServerFrontendClientId,
        gate: PrivateLifecycleGate,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut held = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let entry = held
            .connections
            .get_mut(&client)
            .ok_or(XServerFrontendRouteError::UnknownClient { client })?;
        if entry.revoked {
            gate.close();
        }
        entry.lifecycle = Some(gate);
        Ok(())
    }

    fn attach(
        &self,
        client: XServerFrontendClientId,
        socket: UnixStream,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let connection = state
            .connections
            .get_mut(&client)
            .ok_or(XServerFrontendRouteError::UnknownClient { client })?;
        if connection.revoked {
            socket
                .shutdown(Shutdown::Both)
                .or_else(|error| {
                    if error.kind() == ErrorKind::NotConnected {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .map_err(|_| XServerFrontendRouteError::RecoveryShutdownFailed { client })?;
            return Ok(());
        }
        connection.socket = Some(socket);
        Ok(())
    }

    fn disconnect_locked(
        &self,
        state: &mut InputRecoveryState,
        client: XServerFrontendClientId,
        outcome: XAuthorityInputDeliveryOutcome,
        rejected: Option<XAuthorityInputDeliveryId>,
    ) -> Result<(), XServerFrontendRouteError> {
        if let Some(connection) = state.connections.get_mut(&client) {
            // Revocation and shutdown precede terminal settlement. The ledger
            // lock arbitrates this transition against a successful writer.
            if let Some(gate) = &connection.lifecycle {
                gate.close();
            }
            connection.revoked = true;
            if let Some(socket) = &connection.socket
                && let Err(error) = socket.shutdown(Shutdown::Both)
                && error.kind() != ErrorKind::NotConnected
            {
                return Err(XServerFrontendRouteError::RecoveryShutdownFailed { client });
            }
            // Keep only the revoked identity tombstone, never a dead socket FD.
            connection.socket.take();
        }
        let pending: Vec<_> = state
            .tickets
            .values()
            .filter(|entry| entry.ticket.client == Some(client) && entry.terminal.is_none())
            .map(|entry| entry.ticket.delivery)
            .collect();
        for delivery in pending {
            let outcome = if Some(delivery) == rejected {
                XAuthorityInputDeliveryOutcome::RouteRejected
            } else {
                outcome
            };
            self.terminal_locked(
                state,
                XAuthorityClientInputDelivery {
                    client,
                    delivery,
                    outcome,
                },
            );
        }
        Ok(())
    }

    fn disconnect(
        &self,
        client: XServerFrontendClientId,
        outcome: XAuthorityInputDeliveryOutcome,
    ) -> Result<(), XServerFrontendRouteError> {
        self.disconnect_rejecting(client, outcome, None)
    }

    fn disconnect_rejecting(
        &self,
        client: XServerFrontendClientId,
        outcome: XAuthorityInputDeliveryOutcome,
        rejected: Option<XAuthorityInputDeliveryId>,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let disconnected = self.disconnect_locked(&mut state, client, outcome, rejected);
        drop(state);
        if self.lifecycle.get().is_some() {
            // Also reached by registration Drop. The exact gate was closed
            // under recovery state; origin drive performs cleanup separately.
            return disconnected;
        }
        disconnected?;
        self.authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .cleanup_owner(client.raw());
        Ok(())
    }

    fn recover(
        &self,
        now: Instant,
        force: bool,
    ) -> Result<Vec<XAuthorityInputDeliveryTicket>, XServerFrontendRouteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let expired: Vec<_> = state
            .tickets
            .values()
            .filter(|entry| {
                entry.terminal.is_none()
                    && (force
                        || (self.admission_deadline.load(Ordering::Acquire)
                            && now.saturating_duration_since(entry.ticket.admitted_at)
                                >= X_AUTHORITY_INPUT_DELIVERY_DEADLINE))
            })
            .map(|entry| entry.ticket)
            .collect();
        // Whose connection this sweep actually revoked. Collected as it
        // happens, because what has to be cleaned up follows from the
        // connection having been taken down, not from which receipts managed
        // to publish. A delivery whose cancellation was suppressed publishes
        // nothing and still leaves a revoked connection behind it, and reading
        // cleanup off the published list would skip exactly that case --
        // leaving this client's grabs and selections installed after its
        // socket is gone.
        let mut revoked: Vec<XServerFrontendClientId> = Vec::new();
        for ticket in &expired {
            if let Some(client) = ticket.client {
                if !revoked.contains(&client) {
                    revoked.push(client);
                }
                // What the sweep is, not what the path is called. A forced
                // sweep revokes the epoch; a deadline sweep reports that no
                // outcome arrived in time. Both reach a bound ticket through
                // the disconnect machinery, and publishing a deadline for a
                // revocation says a delivery ran out of time when it was
                // withdrawn -- and hides, from anything that classifies by
                // outcome, that this one denies the delivery happened.
                let outcome = if force {
                    XAuthorityInputDeliveryOutcome::EpochRevoked
                } else {
                    XAuthorityInputDeliveryOutcome::TimedOut
                };
                self.disconnect_locked(&mut state, client, outcome, None)?;
            } else {
                self.terminal_locked(
                    &mut state,
                    XAuthorityClientInputDelivery {
                        client: XServerFrontendClientId(0),
                        delivery: ticket.delivery,
                        outcome: XAuthorityInputDeliveryOutcome::EpochRevoked,
                    },
                );
            }
        }
        // Only the ones that actually ended. A delivery an execution holds had
        // its cancellation deferred rather than applied, and reporting it as
        // revoked would be exactly the claim an effect could go on to
        // contradict -- the caller would free what it was holding.
        let expired: Vec<_> = expired
            .into_iter()
            .filter(|ticket| {
                state
                    .tickets
                    .get(&ticket.delivery)
                    .is_none_or(|entry| entry.terminal.is_some())
            })
            .collect();
        drop(state);
        if let Some(owner) = self.lifecycle.get() {
            owner
                .drive(NonZeroUsize::new(1).unwrap())
                .map_err(|_| XServerFrontendRouteError::LifecycleUnavailable)?;
            return Ok(expired);
        }
        let mut authority = self
            .authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        for client in revoked {
            authority.cleanup_owner(client.raw());
        }
        Ok(expired)
    }
}

#[cfg(unix)]
impl XAuthorityRoutedInputSender {
    pub fn delivery_ticket(
        &self,
        id: XAuthorityInputDeliveryId,
    ) -> Option<XAuthorityInputDeliveryTicket> {
        self.recovery.ticket(id)
    }
    pub fn observe_delivery(&self, receipt: XAuthorityClientInputDelivery) -> bool {
        self.recovery.observe(receipt)
    }
    pub fn recover_input_deliveries(
        &self,
        now: Instant,
        revoke_all: bool,
    ) -> Result<Vec<XAuthorityInputDeliveryTicket>, XServerFrontendRouteError> {
        self.recovery.recover(now, revoke_all)
    }
    pub fn disconnect_input_client(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.recovery
            .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
    }
}

#[cfg(unix)]
impl X11InputEventReceiver {
    fn delivery_active(
        &self,
        client: XServerFrontendClientId,
        id: Option<XAuthorityInputDeliveryId>,
    ) -> bool {
        match self {
            Self::Routed {
                recovery: Some(recovery),
                ..
            } => recovery.active(id, client),
            _ => true,
        }
    }
}

#[cfg(unix)]
struct X11InputWriterRecoveryGuard<'a> {
    receiver: &'a X11InputEventReceiver,
    client: XServerFrontendClientId,
}

#[cfg(unix)]
impl Drop for X11InputWriterRecoveryGuard<'_> {
    fn drop(&mut self) {
        if let X11InputEventReceiver::Routed {
            recovery: Some(recovery),
            ..
        } = self.receiver
        {
            let _ = recovery.disconnect(
                self.client,
                XAuthorityInputDeliveryOutcome::ClientDisconnected,
            );
        }
    }
}

#[cfg(unix)]
struct X11InputDeliveryGuard<'a> {
    receiver: &'a X11InputEventReceiver,
    client: XServerFrontendClientId,
    delivery: Option<XAuthorityInputDeliveryId>,
    settled: std::cell::Cell<bool>,
}
#[cfg(unix)]
impl X11InputDeliveryGuard<'_> {
    fn finish(&self, outcome: XAuthorityInputDeliveryOutcome) -> Result<(), X11SetupSocketError> {
        if !self.settled.replace(true) {
            self.receiver
                .send_delivery(self.client, self.delivery, outcome)?;
        }
        Ok(())
    }
}
#[cfg(unix)]
impl Drop for X11InputDeliveryGuard<'_> {
    fn drop(&mut self) {
        let _ = self.finish(XAuthorityInputDeliveryOutcome::WriteFailed);
    }
}
