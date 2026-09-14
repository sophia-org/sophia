/// The registry keeps the real per-connection projections; it never rebuilds
/// selections from its coarser subscription map or a route's admission copy.
/// This immutable slot is shared by the exact route entry and its registration
/// guard. Removing the route removes its discoverability, with no history map.
#[cfg(unix)]
struct PrivateAppliedClientState {
    registry: std::sync::Weak<
        Mutex<BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>,
    >,
    namespace: NamespaceId,
    selections: Arc<Mutex<XCoreEventSelectionState>>,
    focused_projection: Arc<AtomicU64>,
    queued_focus: Mutex<Option<PrivateFocusIssued>>,
    applied_focus_generation: AtomicU64,
}

#[cfg(unix)]
struct PrivateAppliedRegistryOwner {
    authority: sophia_input_authority::AuthorityIdentity,
    controller: PrivateAuthorityController,
    bindings: Arc<Mutex<PrivateAdmissionBindings>>,
    namespace: NamespaceId,
    publication: Arc<Mutex<PrivateAppliedRoutingState>>,
    next_focus_claim: AtomicU64,
    /// A failed installation may have bound only some existing connections.
    /// That is retained preparation, never permission to use a partial view.
    ready: AtomicBool,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrivateAppliedRegistryRefusal {
    AuthorityUnavailable,
    RegistryUnavailable,
    SelectionUnavailable,
    PublicationUnavailable,
    NoPrivateOwner,
    PreparationIncomplete,
    MissingClient,
    MissingConnectionState,
    MissingAdmission,
    AdmissionUnavailable,
    AdmissionClosed,
    ForeignOrigin,
    DifferentConnectionState,
    MissingFocusClaim,
    FocusIdentityExhausted,
    Selection(PrivateAppliedRefusal),
}

/// Borrowed from a still-held client-table guard and the currently held
/// admission binding. This is not an unlocked currentness result. It cannot
/// outlive those borrows or be re-used against a replacement registration.
#[cfg(unix)]
struct PrivateAppliedClientRef<'a> {
    owner: &'a PrivateAppliedRegistryOwner,
    connection: &'a PrivateAppliedClientState,
    client: XServerFrontendClientId,
    _admission: &'a PrivateAdmissionBinding,
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    /// Setup attachment, before worker exposure. The existing client-table
    /// lock serializes this with installation. No common, X authority or
    /// publication lock is acquired here, and ordinary connections simply
    /// retain the state they already own.
    fn attach_connection_state(
        &self,
        registration: &XServerFrontendClientRouteRegistration,
        namespace: NamespaceId,
        selections: Arc<Mutex<XCoreEventSelectionState>>,
        focused_projection: Arc<AtomicU64>,
    ) -> Result<(), PrivateAppliedRegistryRefusal> {
        let clients = self
            .clients
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
        let entry = clients
            .get(&registration.client)
            .ok_or(PrivateAppliedRegistryRefusal::MissingClient)?;
        if !Arc::ptr_eq(&self.clients, &registration.clients)
            || !Arc::ptr_eq(&entry.connection_state, &registration.connection_state)
        {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        if entry
            .admission
            .is_some_and(|admission| admission.namespace.id != namespace)
        {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        if let Some(existing) = entry.connection_state.get() {
            return if existing.namespace == namespace
                && Arc::ptr_eq(&existing.selections, &selections)
                && Arc::ptr_eq(&existing.focused_projection, &focused_projection)
            {
                Ok(())
            } else {
                Err(PrivateAppliedRegistryRefusal::DifferentConnectionState)
            };
        }
        if let Some(owner) = self.private_applied.get() {
            Self::bind_connection_selections(owner, registration.client, namespace, &selections)?;
        }
        entry
            .connection_state
            .set(PrivateAppliedClientState {
                registry: Arc::downgrade(&self.clients),
                namespace,
                selections,
                focused_projection,
                queued_focus: Mutex::new(None),
                applied_focus_generation: AtomicU64::new(0),
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::DifferentConnectionState)
    }

    /// Called by preparation before exposing an ingress. Common is acquired
    /// here, so the caller must not already hold it or an adapter guard.
    /// Existing setup rows and future setup rows bind to the same immutable
    /// authority. A failed attempt leaves preparation unavailable and a retry
    /// of that same origin may finish it; another origin cannot replace it.
    fn install_private_applied(
        &self,
        participant: &PrivateAdmissionParticipant,
        namespace: NamespaceId,
    ) -> Result<Arc<Mutex<PrivateAppliedRoutingState>>, PrivateAppliedRegistryRefusal> {
        let controller = &participant.controller;
        controller
            .under_common_as_origin(|authority, issuer| {
                let identity = authority
                    .authority_identity(issuer)
                    .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?;
                let clients = self
                    .clients
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
                let owner = self
                    .private_applied
                    .get_or_init(|| PrivateAppliedRegistryOwner {
                        authority: identity,
                        controller: controller.clone(),
                        bindings: participant.bindings.clone(),
                        namespace,
                        publication: Arc::new(Mutex::new(PrivateAppliedRoutingState::new(
                            identity, namespace,
                        ))),
                        ready: AtomicBool::new(false),
                        next_focus_claim: AtomicU64::new(1),
                    });
                if owner.authority != identity
                    || owner.namespace != namespace
                    || !Arc::ptr_eq(&owner.bindings, &participant.bindings)
                {
                    return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
                }
                owner.ready.store(false, Ordering::Release);
                for (client, entry) in clients.iter() {
                    if let Some(connection) = entry.connection_state.get() {
                        Self::bind_connection_selections(
                            owner,
                            *client,
                            connection.namespace,
                            &connection.selections,
                        )?;
                    }
                }
                owner.ready.store(true, Ordering::Release);
                Ok(owner.publication.clone())
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?
    }

    fn bind_connection_selections(
        owner: &PrivateAppliedRegistryOwner,
        client: XServerFrontendClientId,
        namespace: NamespaceId,
        selections: &Mutex<XCoreEventSelectionState>,
    ) -> Result<(), PrivateAppliedRegistryRefusal> {
        if namespace != owner.namespace {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        selections
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::SelectionUnavailable)?
            .bind_private_origin(PrivateAppliedSelectionOrigin {
                authority: owner.authority,
                namespace,
                client,
            })
            .map_err(PrivateAppliedRegistryRefusal::Selection)
    }

    /// The caller takes clients before the X guards, beneath common and the
    /// admission boundary, and keeps that same guard through application. This
    /// method only borrows it; it never acquires clients from beneath X.
    /// `admission` is lent by the participant while its boundary guard is held.
    #[cfg_attr(not(test), allow(dead_code))] // Consumed by the guarded executor integration.
    fn applied_client<'a>(
        &'a self,
        clients: &'a std::sync::MutexGuard<
            '_,
            BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>,
        >,
        client: XServerFrontendClientId,
        admission: &'a PrivateAdmissionBinding,
    ) -> Result<PrivateAppliedClientRef<'a>, PrivateAppliedRegistryRefusal> {
        if self.clients.is_poisoned() {
            return Err(PrivateAppliedRegistryRefusal::RegistryUnavailable);
        }
        let owner = self
            .private_applied
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::NoPrivateOwner)?;
        if !owner.ready.load(Ordering::Acquire) {
            return Err(PrivateAppliedRegistryRefusal::PreparationIncomplete);
        }
        if admission.closed
            || admission
                .lifecycle
                .as_ref()
                .is_some_and(|gate| !gate.is_open())
        {
            return Err(PrivateAppliedRegistryRefusal::AdmissionClosed);
        }
        let entry = clients
            .get(&client)
            .ok_or(PrivateAppliedRegistryRefusal::MissingClient)?;
        let connection = entry
            .connection_state
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::MissingConnectionState)?;
        let registered = entry
            .admission
            .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
        if !std::sync::Weak::ptr_eq(&connection.registry, &Arc::downgrade(&self.clients))
            || admission.namespace != owner.namespace
            || connection.namespace != owner.namespace
            || registered.namespace.id != admission.namespace
            || registered.client_id != admission.admission
            || registered.auth_provenance.session_generation != admission.generation
        {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        // Weak registry identity above rejects a foreign client-table guard
        // even when every numeric name and admission tuple collides.
        Ok(PrivateAppliedClientRef {
            owner,
            connection,
            client,
            _admission: admission,
        })
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Actual guarded execution joins these borrowed parts next.
impl PrivateAppliedClientRef<'_> {
    /// Take selections after X authority; no X/common/client lock is hidden
    /// inside this method. Keep this guard through resolution and application.
    fn lock_selections(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, XCoreEventSelectionState>, PrivateAppliedRegistryRefusal>
    {
        let selections = self
            .connection
            .selections
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::SelectionUnavailable)?;
        if selections.private_origin
            != Some(PrivateAppliedSelectionOrigin {
                authority: self.owner.authority,
                namespace: self.owner.namespace,
                client: self.client,
            })
        {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        Ok(selections)
    }

    /// Publication is separate from selections so the caller chooses and
    /// retains the documented guard interval, rather than a callback relocking
    /// state it already holds. An unreadable publication is never a default.
    fn lock_publication(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, PrivateAppliedRoutingState>, PrivateAppliedRegistryRefusal>
    {
        self.owner
            .publication
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::PublicationUnavailable)
    }

    fn focused_projection(&self) -> &AtomicU64 {
        &self.connection.focused_projection
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct PrivateFocusIssued {
    generation: u64,
    window: XResourceId,
}

/// An origin-issued intent token, not an applied focus receipt. Keeping the
/// exact registration slot prevents a replaced connection with the same
/// numeric client/window names from consuming it. The connection retains only
/// `PrivateFocusIssued`, so retaining this token creates no reference cycle.
#[cfg(unix)]
#[derive(Clone)]
struct PrivateFocusClaim {
    authority: sophia_input_authority::AuthorityIdentity,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    issued: PrivateFocusIssued,
    admission: sophia_protocol::ClientAdmissionId,
    connection_generation: u64,
    connection: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
}

#[cfg(unix)]
impl std::fmt::Debug for PrivateFocusClaim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivateFocusClaim")
            .field("authority", &self.authority)
            .field("namespace", &self.namespace)
            .field("client", &self.client)
            .field("issued", &self.issued)
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Control/core producer call sites arrive in the integration patch.
impl XServerFrontendRouteRegistry {
    /// Mint before later X/queue guards. This only reserves an identity, not
    /// execution permission or a position in the shared operation order.
    /// Gaps from refused queue admission do not publish or invalidate focus.
    fn reserve_private_focus(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<Option<PrivateFocusClaim>, PrivateAppliedRegistryRefusal> {
        let Some(owner) = self.private_applied.get() else {
            return Ok(None);
        };
        if !owner.ready.load(Ordering::Acquire) {
            return Err(PrivateAppliedRegistryRefusal::PreparationIncomplete);
        }
        let clients = self
            .clients
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
        let entry = clients
            .get(&client)
            .ok_or(PrivateAppliedRegistryRefusal::MissingClient)?;
        let connection = entry
            .connection_state
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::MissingConnectionState)?;
        if connection.namespace != owner.namespace {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        let generation = owner
            .next_focus_claim
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| {
                next.checked_add(1)
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::FocusIdentityExhausted)?;
        Ok(Some(PrivateFocusClaim {
            authority: owner.authority,
            namespace: owner.namespace,
            client,
            issued: PrivateFocusIssued { generation, window },
            admission: entry
                .admission
                .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?
                .client_id,
            connection_generation: entry
                .admission
                .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?
                .auth_provenance
                .session_generation,
            connection: entry.connection_state.clone(),
        }))
    }

    /// Called only after queue acceptance, while the router still owns its
    /// existing focused-intent guard. Recording owns no effect or allocation.
    /// A poisoned scalar slot cannot excuse losing accepted provenance.
    fn record_private_focus_queued(claim: &PrivateFocusClaim) {
        let connection = claim.connection.get().expect("origin constructed claim");
        let mut queued = connection
            .queued_focus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queued.is_none_or(|old| old.generation < claim.issued.generation) {
            *queued = Some(claim.issued);
        }
    }

    /// Copy the queued intent's generation, including when its writer has not
    /// yet applied it. This cannot make pending focus readable to execution.
    fn private_focus_dependency(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<Option<PrivateFocusClaim>, PrivateAppliedRegistryRefusal> {
        let Some(owner) = self.private_applied.get() else {
            return Ok(None);
        };
        let clients = self
            .clients
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
        let entry = clients
            .get(&client)
            .ok_or(PrivateAppliedRegistryRefusal::MissingClient)?;
        let connection = entry
            .connection_state
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::MissingConnectionState)?;
        let queued = connection
            .queued_focus
            .lock()
            .map_err(|_| PrivateAppliedRegistryRefusal::PublicationUnavailable)?;
        let admission = entry
            .admission
            .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
        Ok(queued
            .filter(|issued| issued.window == window)
            .map(|issued| PrivateFocusClaim {
                authority: owner.authority,
                namespace: owner.namespace,
                client,
                issued,
                admission: admission.client_id,
                connection_generation: admission.auth_provenance.session_generation,
                connection: entry.connection_state.clone(),
            }))
    }

    /// Read only under common and the exact retained participant boundary.
    /// A registration copy supplies expected identity, never its currentness.
    fn focus_claim_admission(
        owner: &PrivateAppliedRegistryOwner,
        bindings: &PrivateAdmissionBindings,
        claim: &PrivateFocusClaim,
    ) -> Result<(), PrivateAppliedRegistryRefusal> {
        let bound = bindings
            .bound
            .get(&claim.client)
            .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
        if bound.closed || bound.lifecycle.as_ref().is_some_and(|gate| !gate.is_open()) {
            return Err(PrivateAppliedRegistryRefusal::AdmissionClosed);
        }
        if owner.authority != claim.authority
            || owner.namespace != claim.namespace
            || bound.namespace != claim.namespace
            || bound.admission != claim.admission
            || bound.generation != claim.connection_generation
        {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        Ok(())
    }

    fn focus_claim_connection<'a>(
        &'a self,
        owner: &PrivateAppliedRegistryOwner,
        clients: &'a BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>,
        namespace: NamespaceId,
        client: XServerFrontendClientId,
        projection: &AtomicU64,
        claim: &PrivateFocusClaim,
    ) -> Result<&'a PrivateAppliedClientState, PrivateAppliedRegistryRefusal> {
        let entry = clients
            .get(&client)
            .ok_or(PrivateAppliedRegistryRefusal::MissingClient)?;
        let connection = entry
            .connection_state
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::MissingConnectionState)?;
        if owner.authority != claim.authority
            || owner.namespace != namespace
            || claim.namespace != namespace
            || claim.client != client
            || connection.namespace != namespace
            || !Arc::ptr_eq(&entry.connection_state, &claim.connection)
            || !std::ptr::eq(connection.focused_projection.as_ref(), projection)
        {
            return Err(PrivateAppliedRegistryRefusal::ForeignOrigin);
        }
        Ok(connection)
    }

    #[allow(clippy::too_many_arguments)] // All fields name the exact producer/connection being checked.
    fn apply_private_focus(
        &self,
        runtime: &mut XAuthorityRuntime,
        namespace: NamespaceId,
        client: XServerFrontendClientId,
        focused_projection: &AtomicU64,
        claim: &PrivateFocusClaim,
        change: X11FocusChange,
    ) -> Result<X11AppliedFocus, X11FocusApplyError> {
        let owner = self.private_applied.get().ok_or(X11FocusApplyError::State(
            PrivateAppliedRegistryRefusal::NoPrivateOwner,
        ))?;
        owner
            .controller
            .under_common(|_| {
                let bindings = owner.bindings.lock().map_err(|_| {
                    X11FocusApplyError::State(PrivateAppliedRegistryRefusal::AdmissionUnavailable)
                })?;
                Self::focus_claim_admission(owner, &bindings, claim)
                    .map_err(X11FocusApplyError::State)?;
                let check = || -> Result<_, PrivateAppliedRegistryRefusal> {
                    if !owner.ready.load(Ordering::Acquire) {
                        return Err(PrivateAppliedRegistryRefusal::PreparationIncomplete);
                    }
                    let clients = self
                        .clients
                        .lock()
                        .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
                    Ok(clients)
                };
                let clients = check().map_err(X11FocusApplyError::State)?;
                let connection = self
                    .focus_claim_connection(
                        owner,
                        &clients,
                        namespace,
                        client,
                        focused_projection,
                        claim,
                    )
                    .map_err(X11FocusApplyError::State)?;
                if change.window() != claim.issued.window {
                    return Err(X11FocusApplyError::State(
                        PrivateAppliedRegistryRefusal::ForeignOrigin,
                    ));
                }
                let selections = connection.selections.lock().map_err(|_| {
                    X11FocusApplyError::State(PrivateAppliedRegistryRefusal::SelectionUnavailable)
                })?;
                if selections.private_origin
                    != Some(PrivateAppliedSelectionOrigin {
                        authority: owner.authority,
                        namespace,
                        client,
                    })
                {
                    return Err(X11FocusApplyError::State(
                        PrivateAppliedRegistryRefusal::ForeignOrigin,
                    ));
                }
                let mut state = owner.publication.lock().map_err(|_| {
                    X11FocusApplyError::State(PrivateAppliedRegistryRefusal::PublicationUnavailable)
                })?;
                if claim.issued.generation <= state.focus_generation {
                    return Err(X11FocusApplyError::Superseded);
                }
                // Write ahead in both stores. Interrupted effects cannot look like
                // the older applied generation to a queued dependent FocusOut.
                state.focus_generation = claim.issued.generation;
                connection
                    .applied_focus_generation
                    .store(claim.issued.generation, Ordering::Release);
                let route = change
                    .has_key_target()
                    .then_some(XServerFrontendSurfaceRoute {
                        client,
                        namespace,
                        admission: clients[&client].admission,
                        window: change.window(),
                    });
                state
                    .begin_focus_change()
                    .map_err(|cause| {
                        X11FocusApplyError::State(PrivateAppliedRegistryRefusal::Selection(cause))
                    })?
                    .apply_exact(runtime, focused_projection, route, change)
                    .map_err(X11FocusApplyError::Runtime)
            })
            .map_err(|_| {
                X11FocusApplyError::State(PrivateAppliedRegistryRefusal::AuthorityUnavailable)
            })?
    }

    fn apply_private_focus_out(
        &self,
        namespace: NamespaceId,
        client: XServerFrontendClientId,
        window: XResourceId,
        focused_projection: &AtomicU64,
        claim: &PrivateFocusClaim,
    ) -> Result<X11DependentFocusEffect, PrivateAppliedRegistryRefusal> {
        let owner = self
            .private_applied
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::NoPrivateOwner)?;
        owner
            .controller
            .under_common(|_| {
                let clients = self
                    .clients
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
                let connection = self.focus_claim_connection(
                    owner,
                    &clients,
                    namespace,
                    client,
                    focused_projection,
                    claim,
                )?;
                if window != claim.issued.window
                    || connection.applied_focus_generation.load(Ordering::Acquire)
                        != claim.issued.generation
                    || focused_projection.load(Ordering::Acquire) != window.local.raw()
                {
                    return Ok(X11DependentFocusEffect::Superseded);
                }
                let mut state = owner
                    .publication
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::PublicationUnavailable)?;
                // Revoke this exact projection claim before its clear. Even
                // root -> root must defeat a pending core publication; equal
                // window numbers do not preserve the old claim's authority.
                connection
                    .applied_focus_generation
                    .store(0, Ordering::Release);
                if state.focus_generation == claim.issued.generation {
                    // This operation changes only the connection projection. The
                    // old runtime focus remains until the receiving writer runs.
                    // Invalidate rather than falsely publish runtime agreement.
                    let _unpublished = state
                        .begin_focus_change()
                        .map_err(PrivateAppliedRegistryRefusal::Selection)?;
                }
                focused_projection.store(u64::from(X_SETUP_DEFAULT_ROOT), Ordering::Release);
                Ok(X11DependentFocusEffect::ProjectionCleared)
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?
    }

    /// Called only by the source-owned flush operation, with outer runtime
    /// still held and the socket already released. A stale completion can
    /// never republish a replaced, cleared or interrupted generation.
    fn publish_private_focus_after_output(
        &self,
        runtime: &XAuthorityRuntime,
        claim: &PrivateFocusClaim,
        revert_to: u8,
    ) -> Result<bool, PrivateAppliedRegistryRefusal> {
        let owner = self
            .private_applied
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::NoPrivateOwner)?;
        owner
            .controller
            .under_common(|_| {
                let bindings = owner
                    .bindings
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::AdmissionUnavailable)?;
                match Self::focus_claim_admission(owner, &bindings, claim) {
                    Ok(()) => {}
                    Err(
                        PrivateAppliedRegistryRefusal::MissingAdmission
                        | PrivateAppliedRegistryRefusal::AdmissionClosed
                        | PrivateAppliedRegistryRefusal::ForeignOrigin,
                    ) => return Ok(false),
                    Err(cause) => return Err(cause),
                }
                let clients = self
                    .clients
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
                let source = claim
                    .connection
                    .get()
                    .ok_or(PrivateAppliedRegistryRefusal::MissingConnectionState)?;
                let connection = self.focus_claim_connection(
                    owner,
                    &clients,
                    claim.namespace,
                    claim.client,
                    &source.focused_projection,
                    claim,
                )?;
                let selections = connection
                    .selections
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::SelectionUnavailable)?;
                if selections.private_origin
                    != Some(PrivateAppliedSelectionOrigin {
                        authority: owner.authority,
                        namespace: claim.namespace,
                        client: claim.client,
                    })
                    || selections.applied_revision.is_none()
                {
                    return Err(PrivateAppliedRegistryRefusal::Selection(
                        PrivateAppliedRefusal::Interrupted,
                    ));
                }
                let mut state = owner
                    .publication
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::PublicationUnavailable)?;
                if state.focus_generation != claim.issued.generation
                    || connection.applied_focus_generation.load(Ordering::Acquire)
                        != claim.issued.generation
                    || connection.focused_projection.load(Ordering::Acquire)
                        != claim.issued.window.local.raw()
                    || (state.focus_window, state.focus_revert_to)
                        != (claim.issued.window, revert_to)
                    || runtime.input_focus(claim.namespace) != (claim.issued.window, revert_to)
                {
                    return Ok(false);
                }
                state.published = true;
                Ok(true)
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?
    }

    /// Recheck only source-owned effect identity after the source has the
    /// socket. This never takes outer runtime or performs IO under common.
    fn private_focus_output_current(
        &self,
        claim: &PrivateFocusClaim,
        revert_to: u8,
    ) -> Result<bool, PrivateAppliedRegistryRefusal> {
        let owner = self
            .private_applied
            .get()
            .ok_or(PrivateAppliedRegistryRefusal::NoPrivateOwner)?;
        owner
            .controller
            .under_common(|_| {
                let bindings = owner
                    .bindings
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::AdmissionUnavailable)?;
                match Self::focus_claim_admission(owner, &bindings, claim) {
                    Ok(()) => {}
                    Err(
                        PrivateAppliedRegistryRefusal::MissingAdmission
                        | PrivateAppliedRegistryRefusal::AdmissionClosed
                        | PrivateAppliedRegistryRefusal::ForeignOrigin,
                    ) => return Ok(false),
                    Err(cause) => return Err(cause),
                }
                let clients = self
                    .clients
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
                let source = claim
                    .connection
                    .get()
                    .ok_or(PrivateAppliedRegistryRefusal::MissingConnectionState)?;
                let connection = self.focus_claim_connection(
                    owner,
                    &clients,
                    claim.namespace,
                    claim.client,
                    &source.focused_projection,
                    claim,
                )?;
                let state = owner
                    .publication
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::PublicationUnavailable)?;
                Ok(state.focus_generation == claim.issued.generation
                    && connection.applied_focus_generation.load(Ordering::Acquire)
                        == claim.issued.generation
                    && connection.focused_projection.load(Ordering::Acquire)
                        == claim.issued.window.local.raw()
                    && (state.focus_window, state.focus_revert_to)
                        == (claim.issued.window, revert_to))
            })
            .map_err(|_| PrivateAppliedRegistryRefusal::AuthorityUnavailable)?
    }
}
