/// Authority-owned state shared by every client accepted by one X11 socket
/// listener. Client sequence numbers remain connection-local.
#[cfg(unix)]
#[derive(Clone)]
pub struct X11CoreSocketServerState {
    runtime: Arc<Mutex<XAuthorityRuntime>>,
    atoms: Arc<Mutex<XAtomTable>>,
    properties: Arc<Mutex<XPropertyTable>>,
    control_runtime_pending: Arc<AtomicUsize>,
    pixmap_progress: Arc<(Mutex<()>, Condvar)>,
    clients: Arc<Mutex<X11CoreClientLeaseState>>,
    next_transaction_id: Arc<AtomicU64>,
    render_device_provider: Arc<std::sync::OnceLock<Arc<dyn XServerFrontendRenderDeviceProvider>>>,
    pixmap_allocator: Option<Arc<dyn XServerFrontendPixmapAllocator>>,
    legacy_device_formats: Arc<std::sync::OnceLock<Vec<crate::XServerFrontendDmaBufImportFormat>>>,
    devices: Arc<Mutex<X11DeviceBundles>>,
    connection_device: Option<Option<Arc<crate::XServerFrontendDeviceBundle>>>,
}

#[cfg(unix)]
impl core::fmt::Debug for X11CoreSocketServerState {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("X11CoreSocketServerState")
            .field("runtime", &self.runtime)
            .field("atoms", &self.atoms)
            .field("properties", &self.properties)
            .field(
                "control_runtime_pending",
                &self.control_runtime_pending.load(Ordering::Relaxed),
            )
            .field("clients", &self.clients)
            .field(
                "next_transaction_id",
                &self.next_transaction_id.load(Ordering::Relaxed),
            )
            .field(
                "has_render_device_provider",
                &self.has_render_device_provider(),
            )
            .field("has_pixmap_allocator", &self.pixmap_allocator.is_some())
            .finish()
    }
}

/// The small part of socket state that must be serialized across connection
/// setup and teardown. Protocol dispatch itself uses the independent runtime,
/// atom, and property locks above.
#[cfg(unix)]
#[derive(Debug)]
struct X11CoreClientLeaseState {
    next_client_resource_range: u16,
    next_client_id: u64,
    client_leases: BTreeMap<XServerFrontendClientId, XServerFrontendClientLease>,
    /// Windows a client asked to have saved when it departs (ChangeSaveSet):
    /// reparented to the nearest surviving ancestor and re-mapped if they were
    /// mapped, rather than destroyed with the client's own subtree.
    save_sets: BTreeMap<XServerFrontendClientId, BTreeSet<XResourceId>>,
    retained_ranges: Vec<XRetainedClientRange>,
}

#[cfg(unix)]
impl Default for X11CoreSocketServerState {
    fn default() -> Self {
        Self {
            runtime: Default::default(),
            atoms: Default::default(),
            properties: Default::default(),
            control_runtime_pending: Default::default(),
            pixmap_progress: Default::default(),
            clients: Arc::new(Mutex::new(X11CoreClientLeaseState {
                next_client_resource_range: 1,
                next_client_id: 1,
                client_leases: Default::default(),
                save_sets: Default::default(),
                retained_ranges: Vec::new(),
            })),
            next_transaction_id: Arc::new(AtomicU64::new(1)),
            render_device_provider: Default::default(),
            pixmap_allocator: None,
            legacy_device_formats: Default::default(),
            devices: Default::default(),
            connection_device: None,
        }
    }
}

#[cfg(unix)]
impl X11CoreSocketServerState {
    pub fn new() -> Self {
        Self::default()
    }

    fn allocate_transaction(&self) -> Result<TransactionId, X11SetupSocketError> {
        let raw = self
            .next_transaction_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| X11SetupSocketError::new("X11 transaction identity space exhausted"))?;
        Ok(TransactionId::from_raw(raw))
    }

    pub fn with_render_device_provider(
        self,
        provider: Arc<dyn XServerFrontendRenderDeviceProvider>,
    ) -> Self {
        let formats = provider.dma_buf_import_formats();
        // Every clone shares the first provider and its inventory.
        if let Ok(mut runtime) = self.runtime.lock()
            && self.render_device_provider.set(provider).is_ok()
        {
            let _ = self.legacy_device_formats.set(formats.clone());
            runtime.set_dma_buf_import_formats(formats);
        }
        self
    }

    fn with_optional_render_device_provider(
        self,
        provider: Option<Arc<dyn XServerFrontendRenderDeviceProvider>>,
    ) -> Self {
        match provider {
            Some(provider) => self.with_render_device_provider(provider),
            None => self,
        }
    }

    /// Grants a client one more block of resource identifiers.
    ///
    /// Drawn from the same counter that hands every client its range at
    /// connection setup, so a granted block cannot overlap one already given
    /// out. `None` when the counter is exhausted, which `GetXIDRange` reports
    /// as a count of zero rather than by inventing a block that would collide
    /// with another client's resources.
    fn grant_client_resource_range(&self) -> Option<(u32, u32)> {
        let mut clients = self.clients.lock().ok()?;
        if clients.next_client_resource_range > X11_MAX_CLIENT_RESOURCE_RANGES {
            return None;
        }
        let base = u32::from(clients.next_client_resource_range) * X11_CLIENT_RESOURCE_RANGE_SIZE;
        clients.next_client_resource_range = clients.next_client_resource_range.saturating_add(1);
        Some((base, X11_CLIENT_RESOURCE_RANGE_SIZE))
    }

    fn open_render_device_fd(&self) -> Result<OwnedFd, XServerFrontendRenderDeviceError> {
        if let Some(pinned) = self.connection_device.as_ref() {
            let bundle = pinned.as_ref().filter(|bundle| bundle.available())
                .ok_or(XServerFrontendRenderDeviceError::Unavailable)?;
            let fd = bundle.provider.open_render_device_fd()?;
            if !bundle.available() { return Err(XServerFrontendRenderDeviceError::Unavailable); }
            return Ok(fd);
        }
        self.render_device_provider.get()
            .ok_or(XServerFrontendRenderDeviceError::Unavailable)?.open_render_device_fd()
    }

    fn has_render_device_provider(&self) -> bool {
        self.connection_device.as_ref().map_or_else(
            || self.render_device_provider.get().is_some(), Option::is_some)
    }

    pub fn with_pixmap_allocator(
        mut self,
        allocator: Arc<dyn XServerFrontendPixmapAllocator>,
    ) -> Self {
        self.pixmap_allocator = Some(allocator);
        self
    }

    fn with_optional_pixmap_allocator(
        mut self,
        allocator: Option<Arc<dyn XServerFrontendPixmapAllocator>>,
    ) -> Self {
        self.pixmap_allocator = allocator;
        self
    }

    fn pixmap_allocator(&self) -> Option<&Arc<dyn XServerFrontendPixmapAllocator>> {
        match self.connection_device.as_ref() {
            Some(Some(bundle)) => bundle.allocator.as_ref(),
            _ => self.pixmap_allocator.as_ref(),
        }
    }

    /// Latches the provider's pixmap-texture capability into the runtime.
    ///
    /// Read once here rather than per request, so the advertisement a client
    /// received cannot disagree with what a later request is answered by.
    fn latch_pixmap_texture_support(&self) -> Result<(), X11SetupSocketError> {
        self.initialize_legacy_device_bundle()?;
        let current = self.devices.lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?.current();
        let supported = current.as_ref().map_or_else(
            || self.pixmap_allocator.as_ref().is_some_and(|allocator| allocator.supports_pixmap_textures()),
            |bundle| bundle.supports_pixmap_textures());
        self.devices.lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?.pixmap_textures = Some(supported);
        self.runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .set_pixmap_textures_supported(supported);
        Ok(())
    }

    fn set_policy_map_deferred(&self, deferred: bool) -> Result<(), X11SetupSocketError> {
        self.runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .set_policy_map_deferred(deferred);
        Ok(())
    }

    pub fn with_output_topology(
        output_topology: sophia_protocol::OutputTopologySnapshot,
    ) -> Result<Self, X11SetupSocketError> {
        let runtime =
            XAuthorityRuntime::with_output_topology(output_topology).map_err(|error| {
                X11SetupSocketError::new(format!("invalid Engine output topology: {error:?}"))
            })?;
        Ok(Self {
            runtime: Arc::new(Mutex::new(runtime)),
            ..Self::default()
        })
    }

    pub fn with_output_topology_and_xkb_config(
        output_topology: sophia_protocol::OutputTopologySnapshot,
        xkb_config: &crate::XkbRmlvoConfig,
    ) -> Result<Self, X11SetupSocketError> {
        Self::with_output_topology_xkb_config_and_font_path(output_topology, xkb_config, &[])
    }

    pub fn with_output_topology_xkb_config_and_font_path(
        output_topology: sophia_protocol::OutputTopologySnapshot,
        xkb_config: &crate::XkbRmlvoConfig,
        font_path: &[std::path::PathBuf],
    ) -> Result<Self, X11SetupSocketError> {
        let mut runtime =
            XAuthorityRuntime::with_output_topology_and_xkb_config(output_topology, xkb_config)
                .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        runtime.index_font_path(font_path);
        Ok(Self {
            runtime: Arc::new(Mutex::new(runtime)),
            ..Self::default()
        })
    }

    fn next_client_setup_success(
        &self,
    ) -> Result<(XServerFrontendClientLease, XSetupSuccess), X11SetupSocketError> {
        let root_size = self
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .output_topology()
            .root_size()
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "invalid Engine output topology during setup: {error:?}"
                ))
            })?;
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?;
        if clients.next_client_resource_range > X11_MAX_CLIENT_RESOURCE_RANGES {
            return Err(X11SetupSocketError::new(
                "Sophia X Server Frontend exhausted X11 client resource ranges",
            ));
        }
        let resource_id_base =
            u32::from(clients.next_client_resource_range) * X11_CLIENT_RESOURCE_RANGE_SIZE;
        clients.next_client_resource_range = clients.next_client_resource_range.saturating_add(1);
        let client = XServerFrontendClientId(clients.next_client_id);
        clients.next_client_id = clients.next_client_id.checked_add(1).ok_or_else(|| {
            X11SetupSocketError::new("Sophia X Server Frontend exhausted client identities")
        })?;
        let resource_id_range = crate::XWireClientResourceRange {
            base: resource_id_base,
            mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
        };
        Ok((
            XServerFrontendClientLease {
                client,
                resource_id_range,
                close_down_mode: crate::XCloseDownMode::Destroy,
            },
            XSetupSuccess {
                resource_id_base,
                resource_id_mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
                root_size,
                ..XSetupSuccess::client_compatible()
            },
        ))
    }

    fn register_client(
        &self,
        lease: XServerFrontendClientLease,
    ) -> Result<(), X11SetupSocketError> {
        if self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?
            .client_leases
            .insert(lease.client, lease)
            .is_some()
        {
            return Err(X11SetupSocketError::new(
                "Sophia X Server Frontend assigned a duplicate client identity",
            ));
        }
        Ok(())
    }

    fn release_client(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<XServerFrontendClientLease, X11SetupSocketError> {
        self.clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?
            .client_leases
            .remove(&client)
            .ok_or_else(|| {
                X11SetupSocketError::new("Sophia X Server Frontend lost a client connection lease")
            })
    }

    fn set_close_down_mode(
        &self,
        client: XServerFrontendClientId,
        mode: crate::XCloseDownMode,
    ) -> Result<(), X11SetupSocketError> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?;
        if let Some(lease) = clients.client_leases.get_mut(&client) {
            lease.close_down_mode = mode;
        }
        Ok(())
    }

    fn change_save_set(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
        mode: crate::XSaveSetMode,
    ) -> Result<(), X11SetupSocketError> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?;
        let set = clients.save_sets.entry(client).or_default();
        match mode {
            crate::XSaveSetMode::Insert => {
                set.insert(window);
            }
            crate::XSaveSetMode::Delete => {
                set.remove(&window);
            }
        }
        Ok(())
    }

    fn take_save_set(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<BTreeSet<XResourceId>, X11SetupSocketError> {
        Ok(self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?
            .save_sets
            .remove(&client)
            .unwrap_or_default())
    }

    fn retain_client_range(&self, retained: XRetainedClientRange) -> Result<(), X11SetupSocketError> {
        self.clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?
            .retained_ranges
            .push(retained);
        Ok(())
    }

    /// The retained range holding `resource`, taken out of retention.
    fn take_retained_range_for_resource(
        &self,
        resource: XResourceId,
    ) -> Result<Option<XRetainedClientRange>, X11SetupSocketError> {
        let raw = u32::try_from(resource.local.raw()).ok();
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?;
        let index = raw.and_then(|raw| {
            clients
                .retained_ranges
                .iter()
                .position(|retained| retained.range.owns_new_resource(raw))
        });
        Ok(index.map(|index| clients.retained_ranges.remove(index)))
    }

    fn take_retained_temporary_ranges(&self) -> Result<Vec<XRetainedClientRange>, X11SetupSocketError> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?;
        let (temporary, kept): (Vec<_>, Vec<_>) = clients
            .retained_ranges
            .drain(..)
            .partition(|retained| retained.temporary);
        clients.retained_ranges = kept;
        Ok(temporary)
    }

    fn has_retained_ranges(&self) -> bool {
        self.clients
            .lock()
            .map(|clients| !clients.retained_ranges.is_empty())
            .unwrap_or(false)
    }

    fn active_client_count(&self) -> usize {
        self.clients
            .lock()
            .map(|clients| clients.client_leases.len())
            .unwrap_or(0)
    }

    fn client_for_resource(
        &self,
        resource: XResourceId,
    ) -> Result<Option<XServerFrontendClientId>, X11SetupSocketError> {
        let raw = u32::try_from(resource.local.raw()).ok();
        let clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 client lease lock poisoned"))?;
        Ok(raw.and_then(|raw| {
            clients.client_leases.iter().find_map(|(client, lease)| {
                lease
                    .resource_id_range
                    .owns_new_resource(raw)
                    .then_some(*client)
            })
        }))
    }
}

#[cfg(unix)]
fn release_x11_client_lease_with_control(
    state: &X11CoreSocketServerState,
    namespace: NamespaceId,
    lease: XServerFrontendClientLease,
    save_set: &[XResourceId],
    control: Option<&PrivateControlClientSource>,
) -> Result<crate::XAuthorityClientResourceRelease, X11SetupSocketError> {
    // Keep authority resource destruction and property removal together. X11
    // request dispatch acquires the runtime lock before the property lock, so
    // this prevents another client observing a destroyed window with stale
    // properties between the two cleanup steps.
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
    let release = runtime
        .release_client_resource_range_with_save_set(namespace, lease.resource_id_range, save_set)
        .map_err(|error| {
            X11SetupSocketError::new(format!("failed to release X11 client resources: {error:?}"))
        })?;
    if let Some(source) = control {
        source.record_removal(state, &lease, &release)?;
    }
    // Atoms before properties, matching the order request dispatch takes
    // them in. The last client leaving is the one moment the protocol says
    // client-interned atoms become undefined, and the authority outlives its
    // connections, so nothing else would ever say so.
    let forgotten = if state.active_client_count() == 0 && !state.has_retained_ranges() {
        state
            .atoms
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?
            .forget_client_interned()
    } else {
        Vec::new()
    };
    let mut properties = state
        .properties
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 property table lock poisoned"))?;
    for window in &release.destroyed_windows {
        properties.remove_window(namespace, *window);
    }
    // The records keyed by those atoms go with them. A row under a name
    // nothing can intern again is unreachable, and the authority's own
    // advertisement is rewritten from scratch by the next connection.
    if !forgotten.is_empty() {
        properties.remove_atoms(&forgotten);
    }
    if let Some(source) = control {
        source.teardown.lock().map_err(|_| X11SetupSocketError::new("control teardown unavailable"))?.properties_removed = true;
    }
    Ok(release)
}
