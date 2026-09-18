// Exactly which endpoint an emission was minted for, and which one a writer
// serves.
//
// Split by subject from both the registry that makes registrations and the
// source that mints emissions: this answers only "are these the same
// endpoint", and both of those have to ask it.

/// Exactly which endpoint, by the registration itself and not by its numbers.
///
/// THE NUMBERS ALONE ARE NOT AN IDENTITY, and the boundary already says so. A
/// connection generation is the SESSION's generation, so a replacement
/// admission for one client inside a session carries the same number. An
/// admission id is allocator-scoped, so it names an admission within one
/// origin and nothing across origins. Two registrations can therefore agree on
/// every number and still be different endpoints -- which is exactly the case
/// a delayed revoke must not close and a stale capsule must not be written to.
///
/// What distinguishes them is the registration: the connection-state cell the
/// registry creates once per registration and shares with exactly that
/// client-table entry and that registration guard. Its pointer is the witness,
/// and it is already there -- no counter is added and no parallel table is
/// kept.
///
/// OPAQUE. There is no constructor taking the parts. One that did would let a
/// caller assemble the identity it is about to be checked against, which is
/// the whole of what this prevents.
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct PrivateEndpointIdentity {
    lifecycle: Option<PrivateLifecycleGate>,
    client: XServerFrontendClientId,
    admission: sophia_protocol::ClientAdmissionId,
    namespace: NamespaceId,
    /// The session's generation, kept because it is part of what the registry
    /// and the binding agreed on. It is not what makes this unique.
    generation: u64,
    registration: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
impl PrivateEndpointIdentity {
    fn record_ordered_termination(&self) {
        #[cfg(all(test, unix))]
        routing_tests::m3_acceptance::before_recipient_termination(self);
        if let Some(original) = self.registration.get() {
            let _ = original.ordered_termination.set(());
        }
    }

    fn ordered_termination(&self) -> bool {
        self.registration.get().is_some_and(|original| original.ordered_termination.get().is_some())
    }

    /// Captured from the recipient's own client-table entry and the admission
    /// binding held over it.
    ///
    /// Both are required, so there is no way to name an endpoint without the
    /// two records that have to agree about what it is. The caller holds the
    /// client-table guard the entry was borrowed from; this copies nothing it
    /// was not handed.
    fn captured(
        client: XServerFrontendClientId,
        entry: &XServerFrontendClientRouteSenders,
        admission: &PrivateAdmissionBinding,
    ) -> Self {
        Self {
            lifecycle: admission.lifecycle.clone(),
            client,
            admission: admission.admission,
            namespace: admission.namespace,
            generation: admission.generation,
            registration: entry.connection_state.clone(),
        }
    }

    /// Whether these name the same endpoint.
    ///
    /// The registration is compared by pointer and first, because it is the
    /// part that is an identity; the numbers are compared too, so a mismatch
    /// that the pointer alone would let through cannot arise from some later
    /// change to what a registration is allowed to carry.
    pub(crate) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.registration, &other.registration)
            && self.client == other.client
            && self.admission == other.admission
            && self.namespace == other.namespace
            && self.generation == other.generation
    }

    /// Whether this endpoint is exactly the given registration.
    ///
    /// The registration guard holds the same cell the client-table entry does,
    /// so a holder of the guard can say "this is mine" without a lookup.
    fn is_registration(
        &self,
        witness: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    ) -> bool {
        Arc::ptr_eq(&self.registration, witness)
    }

    /// Whether this endpoint is exactly the given client-table entry.
    ///
    /// Asked by a producer against the entry it is about to send through, so
    /// the row that is checked is the row the capsule goes to.
    fn is_entry(
        &self,
        client: XServerFrontendClientId,
        entry: &XServerFrontendClientRouteSenders,
    ) -> bool {
        Arc::ptr_eq(&self.registration, &entry.connection_state)
            && self.client == client
            && entry
                .admission
                .is_some_and(|registered| {
                    registered.client_id == self.admission
                        && registered.namespace.id == self.namespace
                        && registered.auth_provenance.session_generation == self.generation
                })
    }

}

#[cfg(unix)]
impl std::fmt::Debug for PrivateEndpointIdentity {
    /// The registration is named by its address, because that is what
    /// distinguishes it and there is nothing else to print about a cell.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateEndpointIdentity")
            .field("client", &self.client)
            .field("admission", &self.admission)
            .field("namespace", &self.namespace)
            .field("generation", &self.generation)
            .field("registration", &Arc::as_ptr(&self.registration))
            .finish()
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
impl PrivateXServerFrontend {
    /// The exact endpoint of a registration this instance admitted.
    ///
    /// FOR THE SERVING SIDE TO RETAIN AT WORKER ADMISSION, captured under the
    /// same boundary and client-table guards the source captures it under, and
    /// from the same two records. A writer's expectation and a capsule's
    /// identity are then the same kind of thing, taken the same way -- which is
    /// what makes comparing them mean anything.
    ///
    /// Not derived from a capsule and not from a bare client-id lookup: the
    /// first would let the thing being checked supply the expectation, and the
    /// second would answer with whatever registration currently holds the
    /// number.
    ///
    /// THE REGISTRATION IS THE ARGUMENT, not the client. A client id names
    /// whatever row exists now, so asking by id would hand a replacement's
    /// identity to a writer that was started for the one it replaced -- which
    /// is exactly the confusion this identity exists to prevent. The
    /// registration held here is the capability, and a current row that is not
    /// it is refused rather than described.
    fn endpoint_for(
        &self,
        registration: &PrivateCleanupRecord,
    ) -> Result<PrivateEndpointIdentity, PrivateAdmissionRefusal> {
        let client = registration.client;
        self.participant.under_boundary(|_authority, _issuer, bindings| {
            let bound = bindings
                .bound
                .get(&client)
                .ok_or(PrivateAdmissionRefusal::NotAdmitted)?;
            let clients = self
                .broker
                .registry
                .clients
                .lock()
                .map_err(|_| PrivateAdmissionRefusal::Unreachable)?;
            let witness = self
                .broker
                .registry
                .applied_client(&clients, client, bound)
                .map_err(|_| PrivateAdmissionRefusal::NotAdmitted)?;
            let endpoint = witness.endpoint.clone();
            drop(clients);
            // The current row has to BE this registration. A replacement that
            // passed every check above is still not the registration this
            // caller holds, and describing it as such is the whole mistake.
            if !endpoint.is_registration(&registration.connection_state) {
                return Err(PrivateAdmissionRefusal::NotAdmitted);
            }
            Ok(endpoint)
        })?
    }
}
