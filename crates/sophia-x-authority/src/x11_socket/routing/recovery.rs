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
}

#[cfg(unix)]
struct InputRecoveryConnection {
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
    state: Arc<Mutex<InputRecoveryState>>,
    sender: Option<Sender<XAuthorityClientInputDelivery>>,
    capacity: usize,
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
            state: Arc::default(),
            sender,
            capacity,
            authority,
        }
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
                    socket: None,
                    revoked: false,
                },
            );
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
        self.disconnect_locked(&mut state, client, outcome, rejected)?;
        drop(state);
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
                        || now.saturating_duration_since(entry.ticket.admitted_at)
                            >= X_AUTHORITY_INPUT_DELIVERY_DEADLINE)
            })
            .map(|entry| entry.ticket)
            .collect();
        for ticket in &expired {
            if let Some(client) = ticket.client {
                self.disconnect_locked(
                    &mut state,
                    client,
                    XAuthorityInputDeliveryOutcome::TimedOut,
                    None,
                )?;
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
        drop(state);
        let mut authority = self
            .authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        for ticket in &expired {
            if let Some(client) = ticket.client {
                authority.cleanup_owner(client.raw());
            }
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
