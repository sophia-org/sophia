#[cfg(unix)]
#[derive(Default)]
struct X11DeviceBundles {
    generations: BTreeMap<u64, Arc<crate::XServerFrontendDeviceBundle>>,
    latest: u64,
    pixmap_textures: Option<bool>,
    backing_owners: BTreeMap<sophia_protocol::BufferHandle, X11PixmapProviderOwner>,
}

#[cfg(unix)]
#[derive(Clone)]
struct X11PixmapProviderOwner {
    bundle: Option<Arc<crate::XServerFrontendDeviceBundle>>,
    allocator: Arc<dyn XServerFrontendPixmapAllocator>,
    retains_storage: bool,
}

#[cfg(unix)]
impl X11DeviceBundles {
    fn current(&self) -> Option<Arc<crate::XServerFrontendDeviceBundle>> {
        self.generations.get(&self.latest).cloned()
    }

    fn install(
        &mut self,
        bundle: Arc<crate::XServerFrontendDeviceBundle>,
    ) -> Result<(), crate::XServerFrontendDeviceBundleError> {
        use crate::XServerFrontendDeviceBundleError as E;
        if bundle.generation() <= self.latest {
            return Err(E::StaleGeneration);
        }
        if !bundle.available() {
            return Err(E::Unavailable);
        }
        if self
            .pixmap_textures
            .is_some_and(|supported| supported != bundle.supports_pixmap_textures())
        {
            return Err(E::CapabilityMismatch);
        }
        // The registry's reference alone does not keep a superseded generation alive.
        self.generations.retain(|generation, bundle| {
            *generation == self.latest || Arc::strong_count(bundle) > 1
        });
        let retiring_current = self
            .generations
            .get(&self.latest)
            .is_some_and(|old| Arc::strong_count(old) == 1);
        let retained = self.generations.len() - usize::from(retiring_current);
        if retained >= crate::X_SERVER_FRONTEND_DEVICE_BUNDLE_CAPACITY {
            return Err(E::Capacity);
        }
        if retiring_current {
            self.generations.remove(&self.latest);
        }
        self.latest = bundle.generation();
        self.generations.insert(self.latest, bundle);
        Ok(())
    }
}

#[cfg(unix)]
impl X11CoreSocketServerState {
    pub fn install_device_bundle(
        &self,
        bundle: Arc<crate::XServerFrontendDeviceBundle>,
    ) -> Result<(), crate::XServerFrontendDeviceBundleError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| crate::XServerFrontendDeviceBundleError::Unavailable)?;
        let mut devices = self
            .devices
            .lock()
            .map_err(|_| crate::XServerFrontendDeviceBundleError::Unavailable)?;
        let supported = bundle.supports_pixmap_textures();
        devices.install(bundle)?;
        if devices.pixmap_textures.is_none() {
            devices.pixmap_textures = Some(supported);
            runtime.set_pixmap_textures_supported(supported);
        }
        Ok(())
    }

    /// Install a configuration's first generation and release the
    /// configuration's reference, so that both construction paths leave only
    /// the registry and real leases holding it.
    pub(crate) fn install_configured_device_bundle(
        &self,
        config: &mut crate::XServerFrontendConfig,
    ) -> Result<(), X11SetupSocketError> {
        match config.take_device_bundle() {
            Some(bundle) => self
                .install_device_bundle(bundle)
                .map_err(|error| X11SetupSocketError::new(error.to_string())),
            None => Ok(()),
        }
    }

    pub fn mark_device_generation_unavailable(
        &self,
        generation: u64,
    ) -> Result<(), crate::XServerFrontendDeviceBundleError> {
        let _runtime = self
            .runtime
            .lock()
            .map_err(|_| crate::XServerFrontendDeviceBundleError::Unavailable)?;
        let devices = self
            .devices
            .lock()
            .map_err(|_| crate::XServerFrontendDeviceBundleError::Unavailable)?;
        devices
            .generations
            .get(&generation)
            .ok_or(crate::XServerFrontendDeviceBundleError::UnknownGeneration)?
            .mark_unavailable();
        Ok(())
    }

    fn initialize_legacy_device_bundle(&self) -> Result<(), X11SetupSocketError> {
        if self
            .devices
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?
            .latest
            != 0
        {
            return Ok(());
        }
        let Some(provider) = self.render_device_provider.get() else {
            return Ok(());
        };
        let formats = self
            .legacy_device_formats
            .get()
            .cloned()
            .unwrap_or_default();
        let bundle = crate::XServerFrontendDeviceBundle::from_snapshot(
            1,
            provider.clone(),
            self.pixmap_allocator.clone(),
            formats,
        )
        .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        let mut devices = self
            .devices
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?;
        if devices.latest == 0 {
            devices
                .install(Arc::new(bundle))
                .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        }
        Ok(())
    }

    fn pin_connection_device(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<(Self, X11ClientDevicePin), X11SetupSocketError> {
        self.initialize_legacy_device_bundle()?;
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
        let bundle = {
            let mut devices = self
                .devices
                .lock()
                .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?;
            devices
                .pixmap_textures
                .get_or_insert(runtime.pixmap_textures_supported());
            devices.current()
        };
        runtime.pin_client_device_bundle(client.raw(), bundle.clone());
        drop(runtime);
        let mut connection = self.clone();
        connection.connection_device = Some(bundle);
        Ok((
            connection,
            X11ClientDevicePin {
                runtime: self.runtime.clone(),
                client,
            },
        ))
    }

    fn allocation_provider_owner(&self) -> Option<X11PixmapProviderOwner> {
        let allocator = self.pixmap_allocator()?.clone();
        let bundle = self
            .connection_device
            .as_ref()
            .and_then(|bundle| bundle.clone());
        let retains_storage = bundle.as_ref().map_or_else(
            || allocator.supports_pixmap_textures(),
            |bundle| bundle.supports_pixmap_textures(),
        );
        Some(X11PixmapProviderOwner {
            bundle,
            allocator,
            retains_storage,
        })
    }

    fn reserve_pixmap_provider(
        &self,
        handle: sophia_protocol::BufferHandle,
        owner: X11PixmapProviderOwner,
    ) -> Result<bool, X11SetupSocketError> {
        let mut devices = self
            .devices
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?;
        if owner
            .bundle
            .as_ref()
            .is_some_and(|bundle| !bundle.available())
        {
            return Ok(false);
        }
        if devices.backing_owners.contains_key(&handle) {
            return Err(X11SetupSocketError::new(
                "X11 pixmap provider identity already reserved",
            ));
        }
        if owner.retains_storage {
            devices.backing_owners.insert(handle, owner);
        }
        Ok(true)
    }

    fn pixmap_provider_owner(
        &self,
        handle: sophia_protocol::BufferHandle,
    ) -> Result<Option<X11PixmapProviderOwner>, X11SetupSocketError> {
        Ok(self
            .devices
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?
            .backing_owners
            .get(&handle)
            .cloned())
    }

    fn finish_pixmap_provider_owner(
        &self,
        handle: sophia_protocol::BufferHandle,
    ) -> Result<(), X11SetupSocketError> {
        self.devices
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 device bundle lock poisoned"))?
            .backing_owners
            .remove(&handle);
        Ok(())
    }
}

#[cfg(unix)]
struct X11ClientDevicePin {
    runtime: Arc<Mutex<XAuthorityRuntime>>,
    client: XServerFrontendClientId,
}

#[cfg(unix)]
impl Drop for X11ClientDevicePin {
    fn drop(&mut self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.release_client_device_bundle(self.client.raw());
        }
    }
}
