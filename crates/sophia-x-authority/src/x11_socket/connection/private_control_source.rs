/// Original connection resources and source-produced teardown facts. The
/// endpoint points to its registration, whose reverse link is weak.
#[cfg(unix)]
struct PrivateControlClientSource {
    endpoint: PrivateEndpointIdentity,
    completion: std::sync::Weak<Mutex<ControlCompletions>>,
    state: X11CoreSocketServerState,
    resource_range: crate::XWireClientResourceRange,
    teardown: Mutex<PrivateControlTeardown>,
    #[cfg(all(test, unix))]
    fail_after_runtime: AtomicBool,
}

#[cfg(unix)]
#[derive(Default)]
struct PrivateControlTeardown {
    /// Produced by the actual native resource release, before any fallible
    /// publication. Kept on failure, including its selection/renderer duties.
    removed: Option<PrivateControlRemovalReceipt>,
    properties_removed: bool,
    pending_publication: Option<XAuthorityObservedTransactionBatch>,
    publication_started: bool,
    finished: bool,
}

#[cfg(unix)]
struct PrivateControlRemovalReceipt {
    endpoint: PrivateEndpointIdentity,
    resources: crate::XAuthorityClientResourceRelease,
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn prepare_control_source(
        &self,
        registration: &XServerFrontendClientRouteRegistration,
        state: &X11CoreSocketServerState,
        resource_range: crate::XWireClientResourceRange,
    ) -> Result<Option<Arc<PrivateControlClientSource>>, X11SetupSocketError> {
        let Some(owner) = self.private_applied.get() else {
            return Ok(None);
        };
        let endpoint = owner
            .controller
            .under_common(|_| {
                let bindings = owner
                    .bindings
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::AdmissionUnavailable)?;
                let bound = bindings
                    .bound
                    .get(&registration.client)
                    .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
                let clients = self
                    .clients
                    .lock()
                    .map_err(|_| PrivateAppliedRegistryRefusal::RegistryUnavailable)?;
                let endpoint = self
                    .applied_client(&clients, registration.client, bound)?
                    .endpoint;
                if !endpoint.is_registration(&registration.connection_state) {
                    return Err(PrivateAppliedRegistryRefusal::DifferentConnectionState);
                }
                Ok(endpoint)
            })
            .map_err(|cause| {
                X11SetupSocketError::new(format!("control source authority unavailable: {cause:?}"))
            })?
            .map_err(|cause| {
                X11SetupSocketError::new(format!("control source unavailable: {cause:?}"))
            })?;
        let source = Arc::new(PrivateControlClientSource {
            endpoint,
            completion: Arc::downgrade(
                &self
                    .control_completion()
                    .ok_or_else(|| X11SetupSocketError::new("private control registry absent"))?
                    .inner,
            ),
            state: state.clone(),
            resource_range,
            teardown: Mutex::new(PrivateControlTeardown::default()),
            #[cfg(all(test, unix))]
            fail_after_runtime: AtomicBool::new(false),
        });
        registration
            .connection_state
            .get()
            .ok_or_else(|| X11SetupSocketError::new("control source lost its original connection"))?
            .control_source
            .set(Arc::downgrade(&source))
            .map_err(|_| X11SetupSocketError::new("control source already installed"))?;
        Ok(Some(source))
    }
}

#[cfg(unix)]
impl PrivateControlClientSource {
    fn record_removal(
        &self,
        state: &X11CoreSocketServerState,
        lease: &XServerFrontendClientLease,
        release: &crate::XAuthorityClientResourceRelease,
    ) -> Result<(), X11SetupSocketError> {
        if lease.client != self.endpoint.client
            || lease.resource_id_range != self.resource_range
            || !Arc::ptr_eq(&self.state.runtime, &state.runtime)
            || !Arc::ptr_eq(&self.state.properties, &state.properties)
        {
            return Err(X11SetupSocketError::new("foreign control resource removal"));
        }
        let mut held = self
            .teardown
            .lock()
            .map_err(|_| X11SetupSocketError::new("control teardown unavailable"))?;
        if held.removed.is_some() {
            return Err(X11SetupSocketError::new("control removal already recorded"));
        }
        held.removed = Some(PrivateControlRemovalReceipt {
            endpoint: self.endpoint.clone(),
            resources: release.clone(),
        });
        Ok(())
    }

    fn retain_teardown_publication(
        &self,
        observation: &X11DispatchObservation,
    ) -> Result<(), X11SetupSocketError> {
        let mut held = self
            .teardown
            .lock()
            .map_err(|_| X11SetupSocketError::new("control teardown unavailable"))?;
        if held.publication_started || held.finished {
            return Err(X11SetupSocketError::new(
                "control teardown publication already attempted",
            ));
        }
        held.pending_publication =
            XAuthorityObservedTransactionBatch::from_dispatch_observation(observation);
        held.publication_started = true;
        Ok(())
    }

    /// Called only after the actual connection cleanup and its observer have
    /// returned successfully. Unknown output retains the original payload.
    fn finish_teardown(&self) -> Result<(), X11SetupSocketError> {
        let mut held = self
            .teardown
            .lock()
            .map_err(|_| X11SetupSocketError::new("control teardown unavailable"))?;
        if held.removed.is_none() || !held.properties_removed {
            return Err(X11SetupSocketError::new(
                "control teardown has no native receipt",
            ));
        }
        held.pending_publication = None;
        held.finished = true;
        Ok(())
    }
}
