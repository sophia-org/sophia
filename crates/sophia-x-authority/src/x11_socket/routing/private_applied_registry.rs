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
}

#[cfg(unix)]
struct PrivateAppliedRegistryOwner {
    authority: sophia_input_authority::AuthorityIdentity,
    namespace: NamespaceId,
    publication: Arc<Mutex<PrivateAppliedRoutingState>>,
    /// A failed installation may have bound only some existing connections.
    /// That is retained preparation, never permission to use a partial view.
    ready: AtomicBool,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrivateAppliedRegistryRefusal {
    AuthorityUnavailable,
    RegistryUnavailable,
    SelectionUnavailable,
    PublicationUnavailable,
    NoPrivateOwner,
    PreparationIncomplete,
    MissingClient,
    MissingConnectionState,
    MissingAdmission,
    AdmissionClosed,
    ForeignOrigin,
    DifferentConnectionState,
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
        controller: &PrivateAuthorityController,
        namespace: NamespaceId,
    ) -> Result<Arc<Mutex<PrivateAppliedRoutingState>>, PrivateAppliedRegistryRefusal> {
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
                        namespace,
                        publication: Arc::new(Mutex::new(PrivateAppliedRoutingState::new(
                            identity, namespace,
                        ))),
                        ready: AtomicBool::new(false),
                    });
                if owner.authority != identity || owner.namespace != namespace {
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
        if admission.closed || admission.lifecycle.as_ref().is_some_and(|gate| !gate.is_open()) {
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
