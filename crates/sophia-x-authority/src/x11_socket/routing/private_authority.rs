// The one authority an instance executes against, and the roles that reach it.
//
// Split by subject: who may reserve, who may execute, and who may take a
// reservation back are three different rights over one instance, and the
// point of this file is that they are not the same handle.

/// The single authoritative instance, reachable only through role-limited
/// methods.
///
/// Shared rather than owned outright. The reservation API needs
/// `&mut AuthorityInstance`, and reserving has to happen at submission, which
/// is a detached producer -- so an instance held exclusively by the executor
/// could never be reserved against. Sharing it is not a rank inversion: a rank
/// orders acquisitions made while another guard is held, and a producer taking
/// this mutex holds nothing else when it does.
///
/// What is withheld is not the instance but the rights over it. Issuer rights
/// stay here, because a caller holding those could revoke and reissue the
/// grant its own request was validated against, and nothing downstream would
/// see that the request it is executing answers to a grant that no longer
/// exists.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateAuthorityController {
    common: Arc<Mutex<sophia_input_authority::AuthorityInstance>>,
    /// Held, never handed out.
    issuer: Arc<sophia_input_authority::IssuerHandle>,
    /// Cells owed disposal that could not be disposed when they were dropped.
    ///
    /// Its own lock, beneath nothing and above nothing: it is never held while
    /// common is taken, and taking it never waits on common. A reservation
    /// dropped while this thread already holds common cannot take common again
    /// to dispose itself -- that is a deadlock, not a rank question -- so it
    /// records the debt here and the next caller that does hold common pays
    /// it. Disposal is deferred, never skipped.
    owed_disposal: Arc<Mutex<Vec<sophia_input_authority::RequestToken>>>,
}

/// Why a role-limited authority call could not be made.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateAuthorityRefusal {
    /// The authority itself refused, before any effect.
    Authority(sophia_input_authority::RegistrationError),
    /// The instance could not be reached at all. Not an outcome: nothing was
    /// attempted, so nothing can be concluded about the work.
    Unreachable,
}

#[cfg(unix)]
impl PrivateAuthorityController {
    /// Build a controller over one authority and the issuer rights for it.
    ///
    /// Fallible, and checked rather than assumed. An instance paired with
    /// another instance's issuer accepts reservations -- the submit handle
    /// only establishes which authority is being addressed -- and then cannot
    /// dispose them, because disposal is an issuer act and that issuer answers
    /// for something else. The cell is stranded with nothing able to publish,
    /// consume or reissue it. The parts come back intact on refusal, since
    /// they may be the caller's only handles.
    #[allow(clippy::result_large_err)]
    pub fn new(
        authority: sophia_input_authority::AuthorityInstance,
        issuer: sophia_input_authority::IssuerHandle,
    ) -> Result<
        Self,
        (
            PrivateAuthorityRefusal,
            sophia_input_authority::AuthorityInstance,
            sophia_input_authority::IssuerHandle,
        ),
    > {
        if let Err(error) = authority.authority_identity(&issuer) {
            return Err((PrivateAuthorityRefusal::Authority(error), authority, issuer));
        }
        Ok(Self {
            common: Arc::new(Mutex::new(authority)),
            issuer: Arc::new(issuer),
            owed_disposal: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Act on the instance under common, paying any deferred disposal first.
    ///
    /// The caller takes whatever later-ranked guards it needs inside, which is
    /// the only order that works: common first, then X. Handing back a guard
    /// instead would let a caller hold common across something this file
    /// cannot see.
    fn under_common<R>(
        &self,
        act: impl FnOnce(&mut sophia_input_authority::AuthorityInstance) -> R,
    ) -> Result<R, PrivateAuthorityRefusal> {
        let mut held = self
            .common
            .lock()
            .map_err(|_| PrivateAuthorityRefusal::Unreachable)?;
        self.pay_owed_disposal(&mut held);
        Ok(act(&mut held))
    }

    /// Dispose everything recorded as owed, while common is already held.
    fn pay_owed_disposal(&self, authority: &mut sophia_input_authority::AuthorityInstance) {
        let Ok(mut owed) = self.owed_disposal.lock() else {
            return;
        };
        while let Some(token) = owed.pop() {
            let _ = authority.abandon_request(&self.issuer, token);
        }
    }

    /// Which authority this is. Read-only, so no role is implied by asking.
    pub fn identity(
        &self,
    ) -> Result<sophia_input_authority::AuthorityIdentity, PrivateAuthorityRefusal> {
        self.under_common(|authority| {
            authority
                .authority_identity(&self.issuer)
                .map_err(PrivateAuthorityRefusal::Authority)
        })?
    }

    /// Issue a capability for one admitted connection. Origin-only.
    fn issue_capability(
        &self,
        connection: sophia_input_authority::ConnectionIdentity,
        device: sophia_protocol::DeviceId,
    ) -> Result<
        (
            sophia_input_authority::DeviceCapability,
            sophia_input_authority::GrantGeneration,
        ),
        PrivateAuthorityRefusal,
    > {
        self.under_common(|authority| {
            let (grant, generation) = authority
                .issue_grant(&self.issuer, connection)
                .map_err(PrivateAuthorityRefusal::Authority)?;
            let capability = authority
                .allocate_device(&self.issuer, grant, generation, device)
                .map_err(PrivateAuthorityRefusal::Authority)?;
            Ok((capability, generation))
        })?
    }

    /// Take back a reservation that was never published.
    ///
    /// Issuer-owned and exact: this disposes the one cell named, never a
    /// sweep. It never waits for common. If common cannot be taken without
    /// waiting -- including because this very thread holds it -- the debt is
    /// recorded and paid by the next caller that holds it, so a drop inside an
    /// execution callback records rather than deadlocks.
    fn dispose_unpublished(&self, token: sophia_input_authority::RequestToken) {
        match self.common.try_lock() {
            Ok(mut authority) => {
                self.pay_owed_disposal(&mut authority);
                let _ = authority.abandon_request(&self.issuer, token);
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                let mut authority = poisoned.into_inner();
                self.pay_owed_disposal(&mut authority);
                let _ = authority.abandon_request(&self.issuer, token);
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if let Ok(mut owed) = self.owed_disposal.lock() {
                    owed.push(token);
                }
            }
        }
    }

    /// Run final validation and application for one request this producer
    /// holds custody of.
    ///
    /// Scoped rather than a general authority callback: the closure receives
    /// the execution permit and nothing that owns a reservation, so nothing it
    /// drops can try to take common again. The token and the connection come
    /// from the custody value rather than from the caller, so this cannot be
    /// pointed at another producer's request.
    pub fn execute(
        &self,
        outstanding: &PrivateOutstandingRequest,
        act: impl FnOnce(
            &mut sophia_input_authority::ExecutionPermit<'_>,
        ) -> Result<(), sophia_input_authority::RegistrationError>,
    ) -> Result<sophia_input_authority::RequestCompletion, PrivateAuthorityRefusal> {
        let token = outstanding.token;
        let connection = outstanding.connection;
        self.under_common(|authority| {
            authority
                .execute_reserved(&self.issuer, token, connection, act)
                .map_err(PrivateAuthorityRefusal::Authority)
        })?
    }

    /// Drive a coordinator transition against this authority. Origin-only.
    ///
    /// The caller already holds the coordinator and common is taken here,
    /// which is the documented order. Nothing that owns a reservation may be
    /// dropped inside, and nothing here hands one in.
    pub fn under_transition<R>(
        &self,
        act: impl FnOnce(
            &mut sophia_input_authority::AuthorityInstance,
            &sophia_input_authority::IssuerHandle,
        ) -> R,
    ) -> Result<R, PrivateAuthorityRefusal> {
        self.under_common(|authority| act(authority, &self.issuer))
    }
}

/// A reservation that has not reached the order yet.
///
/// Held rather than returned as a bare token, because the window between
/// reserving and publishing has exactly two honest ends: the work is published
/// and the reservation belongs to it, or it is not and the reservation goes
/// back. A bare token makes the second end something a caller has to remember
/// on every refusal path, including the ones that unwind.
///
/// There is deliberately no public way to extract the token. Publication is
/// internal and happens only where the work is genuinely accepted, so a
/// producer cannot disable disposal by asserting that it published.
#[cfg(unix)]
pub struct PrivateReservation {
    controller: PrivateAuthorityController,
    /// Taken when the work reaches the order. `None` afterwards, so the drop
    /// below knows an unpublished reservation from one that has an owner.
    token: Option<sophia_input_authority::RequestToken>,
    connection: sophia_input_authority::ConnectionIdentity,
    capability: sophia_input_authority::DeviceCapability,
    submit: sophia_input_authority::SubmitHandle,
}

#[cfg(unix)]
impl PrivateReservation {
    pub fn capability(&self) -> sophia_input_authority::DeviceCapability {
        self.capability
    }

    /// The work reached the order, so custody of the request travels with it.
    ///
    /// Crate-internal and consuming: the only caller is the point where
    /// acceptance actually happened, and what comes back is custody rather
    /// than a bare token.
    #[cfg_attr(not(test), allow(dead_code))]
    fn accepted(mut self) -> PrivateOutstandingRequest {
        let token = self
            .token
            .take()
            .expect("a reservation reaches the order at most once");
        PrivateOutstandingRequest {
            controller: self.controller.clone(),
            submit: self.submit,
            token,
            connection: self.connection,
        }
    }
}

#[cfg(unix)]
impl Drop for PrivateReservation {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        // Never published, so the cell it holds answers to nothing. Disposed
        // through the origin's issuer rights rather than by the producer,
        // which has none, and by exact token rather than by clearing whatever
        // the grant currently holds -- that cell may already belong to the
        // next request.
        self.controller.dispose_unpublished(token);
    }
}

/// Custody of one request that reached the order.
///
/// This is what makes observing an outcome a right rather than a guess. The
/// token and the connection are carried here, not supplied by whoever calls,
/// so a producer cannot name another producer's request and consume its
/// completion -- which the authority alone cannot prevent, because a shared
/// submit handle establishes only which authority is being addressed and the
/// connection test compares against what the caller passed in.
#[cfg(unix)]
pub struct PrivateOutstandingRequest {
    controller: PrivateAuthorityController,
    #[cfg_attr(not(test), allow(dead_code))]
    submit: sophia_input_authority::SubmitHandle,
    token: sophia_input_authority::RequestToken,
    connection: sophia_input_authority::ConnectionIdentity,
}

#[cfg(unix)]
impl PrivateOutstandingRequest {
    /// Observe the outcome of this request, and only this one.
    pub fn observe(
        &self,
    ) -> Result<Option<sophia_input_authority::RequestCompletion>, PrivateAuthorityRefusal> {
        self.controller.under_common(|authority| {
            authority
                .take_completion(&self.submit, self.token, self.connection)
                .map_err(PrivateAuthorityRefusal::Authority)
        })?
    }
}

/// A producer's role over the authority: its own reservations, and nothing
/// else.
///
/// Cloneable and detached on purpose. Two producers contending for one order
/// is the thing being built, so this has to survive being handed to one.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateReservationRole {
    controller: PrivateAuthorityController,
    submit: sophia_input_authority::SubmitHandle,
    capability: sophia_input_authority::DeviceCapability,
    /// Bound at issue, not supplied per call. A producer asked to provide
    /// these would have to obtain them from somewhere, and the only places
    /// they exist are this role and another role's.
    generation: sophia_input_authority::GrantGeneration,
    connection: sophia_input_authority::ConnectionIdentity,
}

#[cfg(unix)]
impl PrivateReservationRole {
    fn new(
        controller: PrivateAuthorityController,
        submit: sophia_input_authority::SubmitHandle,
        capability: sophia_input_authority::DeviceCapability,
        generation: sophia_input_authority::GrantGeneration,
        connection: sophia_input_authority::ConnectionIdentity,
    ) -> Self {
        Self {
            controller,
            submit,
            capability,
            generation,
            connection,
        }
    }

    pub fn connection(&self) -> sophia_input_authority::ConnectionIdentity {
        self.connection
    }

    /// Reserve one request, before the work is published.
    ///
    /// The caller supplies the stamp it already captured through the
    /// coordinator and released, plus which request this is. The grant
    /// generation and the connection come from this role, so a producer never
    /// has to obtain an opaque value it has no way to know -- and never has a
    /// reason to reach for another role's.
    ///
    /// A transition landing between the stamp and this call is caught by the
    /// authority's own validation rather than by a check racing it, and
    /// nothing here reaches back for the coordinator from under common.
    pub fn reserve(
        &self,
        stamp: crate::ControlStamp,
        request: u64,
    ) -> Result<PrivateReservation, PrivateAuthorityRefusal> {
        let context = sophia_input_authority::ExecutionContext {
            generation: self.generation,
            connection: self.connection,
            epoch: stamp.control_epoch,
            publication: stamp.publication,
            request,
        };
        let token = self.controller.under_common(|authority| {
            authority
                .reserve_request(&self.submit, self.capability, context)
                .map_err(PrivateAuthorityRefusal::Authority)
        })??;
        Ok(PrivateReservation {
            controller: self.controller.clone(),
            token: Some(token),
            connection: self.connection,
            capability: self.capability,
            submit: self.submit,
        })
    }
}
