// The per-connection side of input recovery: which exact connection an entry
// is for, what it holds for that connection, and how that connection is
// disconnected.
//
// SEPARATED FROM THE TICKET LEDGER BY SUBJECT. Tickets are about deliveries;
// this is about the connection a number names, which is reissued, so every
// act here that was decided earlier than it runs has to be able to ask
// whether the entry it reached is still the one it meant.

#[cfg(unix)]
struct InputRecoveryConnection {
    /// Which exact connection this entry belongs to, when publication said.
    ///
    /// THE ENTRY IS KEYED BY NUMBER AND THE NUMBER IS REISSUED. A disconnect
    /// that reaches this entry long after it captured the failing endpoint --
    /// a saturated queue noticed after the row went, a delivery that failed
    /// after a successor published -- has to be able to ask whether this is
    /// still the connection it meant. `None` is an entry nobody published a
    /// connection for, which no exact disconnect can act on.
    occupant: Option<std::sync::Weak<std::sync::OnceLock<PrivateAppliedClientState>>>,
    lifecycle: Option<PrivateLifecycleGate>,
    // A distinct descriptor for the SAME socket. shutdown interrupts every
    // writer without acquiring the mutex protecting output serialization.
    socket: Option<UnixStream>,
    revoked: bool,
}

#[cfg(unix)]
impl InputRecoveryConnection {
    /// Whether this entry is the one published for `occupant`.
    fn belongs_to(&self, occupant: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>) -> bool {
        self.occupant
            .as_ref()
            .is_some_and(|held| std::ptr::eq(held.as_ptr(), Arc::as_ptr(occupant)))
    }
}

#[cfg(unix)]
impl InputRecovery {
    /// Start a fresh entry for this number.
    ///
    /// THIS REPLACES WHATEVER ENTRY THE NUMBER HAD: its gate, its socket and
    /// its revocation all go. Publication takes the number's claim before
    /// calling this, so the entry replaced is never a live connection's; a
    /// caller that has not established that is disarming whoever holds the
    /// number. `occupant` names the connection this entry is for, and is what
    /// an exact disconnect compares against.
    fn register(
        &self,
        client: XServerFrontendClientId,
        occupant: Option<&Arc<std::sync::OnceLock<PrivateAppliedClientState>>>,
    ) -> Result<(), XServerFrontendRouteError> {
        self.state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .connections
            .insert(
                client,
                InputRecoveryConnection {
                    occupant: occupant.map(Arc::downgrade),
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
        self.finish_disconnect(client, disconnected)
    }

    /// Disconnect this number only if its entry is still `occupant`'s.
    ///
    /// FOR AN ACT THAT WAS DECIDED EARLIER THAN IT RUNS. A route captured its
    /// sender while the connection was live and found it saturated or gone
    /// later; a cleanup began under one claim and acts under it. In between,
    /// the number can have been given back and taken by a successor whose
    /// entry now sits here. This compares under the same acquisition it acts
    /// under, so there is no interval between deciding the entry is the right
    /// one and revoking it.
    ///
    /// `Ok(false)` is not a failure: the entry is another connection's, or
    /// nobody's, and nothing was done. `Ok(true)` is the disconnect that was
    /// asked for.
    fn disconnect_exact(
        &self,
        client: XServerFrontendClientId,
        occupant: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
        outcome: XAuthorityInputDeliveryOutcome,
        rejected: Option<XAuthorityInputDeliveryId>,
    ) -> Result<bool, XServerFrontendRouteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if !state
            .connections
            .get(&client)
            .is_some_and(|connection| connection.belongs_to(occupant))
        {
            return Ok(false);
        }
        let disconnected = self.disconnect_locked(&mut state, client, outcome, rejected);
        drop(state);
        self.finish_disconnect(client, disconnected).map(|()| true)
    }

    fn finish_disconnect(
        &self,
        client: XServerFrontendClientId,
        disconnected: Result<(), XServerFrontendRouteError>,
    ) -> Result<(), XServerFrontendRouteError> {
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
}
