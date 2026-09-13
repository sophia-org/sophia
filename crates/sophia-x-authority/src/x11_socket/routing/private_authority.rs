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
/// What is withheld is not the instance but the rights over it. A producer
/// reaches its own reservation and its own completion. Issuer rights stay
/// here, because a caller holding those could revoke and reissue the grant its
/// own request was validated against, and nothing downstream would see that
/// the request it is executing answers to a grant that no longer exists.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateAuthorityController {
    common: Arc<Mutex<sophia_input_authority::AuthorityInstance>>,
    /// Held, never handed out.
    issuer: Arc<sophia_input_authority::IssuerHandle>,
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
    pub fn new(
        authority: sophia_input_authority::AuthorityInstance,
        issuer: sophia_input_authority::IssuerHandle,
    ) -> Self {
        Self {
            common: Arc::new(Mutex::new(authority)),
            issuer: Arc::new(issuer),
        }
    }

    /// Act on the instance under common.
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
        Ok(act(&mut held))
    }

    /// The same, reaching through poison.
    ///
    /// For the paths that cannot refuse: taking a reservation back is a move
    /// into storage the reservation already holds, and declining it strands a
    /// cell nobody will ever publish or consume. A poisoned instance is not a
    /// reason to leak one.
    fn under_common_even_if_poisoned<R>(
        &self,
        act: impl FnOnce(&mut sophia_input_authority::AuthorityInstance) -> R,
    ) -> R {
        let mut held = self
            .common
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        act(&mut held)
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
    pub fn issue_capability(
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
    /// sweep. Reaching through poison because the alternative is a cell that
    /// can never be published, consumed or reissued.
    fn dispose_unpublished(&self, token: sophia_input_authority::RequestToken) -> bool {
        self.under_common_even_if_poisoned(|authority| {
            authority.abandon_request(&self.issuer, token).is_ok()
        })
    }

    /// Run final validation and application for one reserved request.
    ///
    /// Common is acquired here and held for the whole callback, so the caller
    /// takes its X guards inside and passes the guarded state it read into the
    /// execution closure rather than locking again.
    pub fn execute_reserved<R>(
        &self,
        act: impl FnOnce(
            &mut sophia_input_authority::AuthorityInstance,
            &sophia_input_authority::IssuerHandle,
        ) -> R,
    ) -> Result<R, PrivateAuthorityRefusal> {
        self.under_common(|authority| act(authority, &self.issuer))
    }
}

/// A reservation that has not been published to the order yet.
///
/// Held rather than returned as a bare token, because the window between
/// reserving and publishing has exactly two honest ends: the work is published
/// and the reservation belongs to it, or it is not and the reservation has to
/// go back. A bare token makes the second end something a caller has to
/// remember on every refusal path, including the ones that unwind.
#[cfg(unix)]
pub struct PrivateReservation {
    controller: PrivateAuthorityController,
    /// Taken when the work is published. `None` afterwards, so the drop below
    /// knows the difference between an unpublished reservation and one that
    /// has an owner.
    token: Option<sophia_input_authority::RequestToken>,
    capability: sophia_input_authority::DeviceCapability,
}

#[cfg(unix)]
impl PrivateReservation {
    /// The work reached the order, so the reservation travels with it.
    pub fn published(mut self) -> sophia_input_authority::RequestToken {
        self.token
            .take()
            .expect("a reservation is published at most once")
    }

    pub fn capability(&self) -> sophia_input_authority::DeviceCapability {
        self.capability
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
        let _disposed = self.controller.dispose_unpublished(token);
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
    submit: Arc<sophia_input_authority::SubmitHandle>,
    capability: sophia_input_authority::DeviceCapability,
}

#[cfg(unix)]
impl PrivateReservationRole {
    pub fn new(
        controller: PrivateAuthorityController,
        submit: sophia_input_authority::SubmitHandle,
        capability: sophia_input_authority::DeviceCapability,
    ) -> Self {
        Self {
            controller,
            submit: Arc::new(submit),
            capability,
        }
    }

    /// Reserve one request, before the work is published.
    ///
    /// The context carries the stamp the caller already captured through the
    /// coordinator, which it released before calling this. A transition
    /// landing in between is caught by the authority's own validation rather
    /// than by a check racing it, and this never reaches back for the
    /// coordinator from under common.
    pub fn reserve(
        &self,
        context: sophia_input_authority::ExecutionContext,
    ) -> Result<PrivateReservation, PrivateAuthorityRefusal> {
        let token = self.controller.under_common(|authority| {
            authority
                .reserve_request(&self.submit, self.capability, context)
                .map_err(PrivateAuthorityRefusal::Authority)
        })??;
        Ok(PrivateReservation {
            controller: self.controller.clone(),
            token: Some(token),
            capability: self.capability,
        })
    }

    /// Observe this producer's own completion. Not a second effect.
    pub fn observe(
        &self,
        token: sophia_input_authority::RequestToken,
        connection: sophia_input_authority::ConnectionIdentity,
    ) -> Result<Option<sophia_input_authority::RequestCompletion>, PrivateAuthorityRefusal> {
        self.controller.under_common(|authority| {
            authority
                .take_completion(&self.submit, token, connection)
                .map_err(PrivateAuthorityRefusal::Authority)
        })?
    }
}
