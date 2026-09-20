/// A local X11 listener owned by the Sophia X Server Frontend.
///
/// The frontend owns only X11 protocol state. It has no DRM/KMS, physical-input,
/// scene-graph, or layout ownership. Its established APIs serve one client at
/// a time. The explicit concurrent APIs use bounded workers that share the
/// independently synchronized frontend state.
#[cfg(unix)]
#[derive(Debug)]
pub struct XServerFrontend {
    config: XServerFrontendConfig,
    listener: UnixListener,
    state: X11CoreSocketServerState,
    workers: BTreeMap<u64, X11CoreClientWorker>,
    worker_completions: Receiver<X11CoreClientWorkerCompletion>,
    worker_completion_sender: Sender<X11CoreClientWorkerCompletion>,
    worker_admissions: BTreeMap<ClientAdmissionId, u64>,
    pending_admission_revocations: BTreeSet<ClientAdmissionId>,
    worker_admission_events: Receiver<X11CoreClientWorkerAdmission>,
    worker_admission_event_sender: Sender<X11CoreClientWorkerAdmission>,
    next_worker_id: u64,
}

#[cfg(unix)]
impl XServerFrontend {
    /// Binds the frontend, reclaiming a pre-existing socket at the path.
    ///
    /// The established behaviour, for callers that restart over their own
    /// leftover socket. It cannot tell a leftover from a live listener; a
    /// service that must not displace one uses [`Self::bind_exclusive`].
    pub fn bind(config: XServerFrontendConfig) -> Result<Self, X11SetupSocketError> {
        let listener = bind_x11_core_socket_server(config.socket_path())?;
        Self::over_listener(config, listener)
    }

    /// Binds the frontend, refusing a path that already exists.
    ///
    /// The kernel refuses atomically, so two services racing this at one path
    /// leave exactly one owner and the loser learns it lost. Nothing is
    /// unlinked, so a live listener is never displaced.
    pub fn bind_exclusive(config: XServerFrontendConfig) -> Result<Self, X11SetupSocketError> {
        let listener = bind_x11_core_socket_server_exclusive(config.socket_path())?;
        Self::over_listener(config, listener)
    }

    /// Everything after the listener exists, shared by both bind paths so the
    /// two cannot drift into configuring the same frontend differently.
    fn over_listener(
        config: XServerFrontendConfig,
        listener: UnixListener,
    ) -> Result<Self, X11SetupSocketError> {
        let state = X11CoreSocketServerState::with_output_topology_xkb_config_and_font_path(
            config.output_topology().clone(),
            config.xkb_config(),
            config.font_path(),
        )?
        .with_optional_render_device_provider(config.render_device_provider())
        .with_optional_pixmap_allocator(config.pixmap_allocator());
        if let Some(bundle) = config.device_bundle() {
            state.install_device_bundle(bundle).map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        }
        state.set_policy_map_deferred(config.policy_map_deferred())?;
        state.latch_pixmap_texture_support()?;
        let (worker_completion_sender, worker_completions) = std::sync::mpsc::channel();
        let (worker_admission_event_sender, worker_admission_events) = std::sync::mpsc::channel();
        Ok(Self {
            config,
            listener,
            state,
            workers: BTreeMap::new(),
            worker_completions,
            worker_completion_sender,
            worker_admissions: BTreeMap::new(),
            pending_admission_revocations: BTreeSet::new(),
            worker_admission_events,
            worker_admission_event_sender,
            next_worker_id: 1,
        })
    }

    pub fn install_device_bundle(&self, bundle: Arc<crate::XServerFrontendDeviceBundle>)
        -> Result<(), crate::XServerFrontendDeviceBundleError>
    {
        self.state.install_device_bundle(bundle)
    }

    pub fn mark_device_generation_unavailable(&self, generation: u64)
        -> Result<(), crate::XServerFrontendDeviceBundleError>
    {
        self.state.mark_device_generation_unavailable(generation)
    }

    pub fn update_window_allocation_preferences(
        &self, snapshot: crate::XWindowAllocationPreferences,
    ) -> Result<crate::XWindowAllocationUpdate, X11SetupSocketError> {
        Ok(self.state.runtime.lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .update_window_allocation_preferences(snapshot))
    }

    pub fn config(&self) -> &XServerFrontendConfig {
        &self.config
    }

    pub fn update_output_topology(
        &mut self,
        snapshot: sophia_protocol::OutputTopologySnapshot,
    ) -> Result<XAuthorityOutputUpdateOutcome, X11SetupSocketError> {
        let generation = snapshot.generation;
        let mut runtime = self
            .state
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
        match runtime.update_output_topology(snapshot) {
            Ok(true) => Ok(XAuthorityOutputUpdateOutcome::Applied {
                generation,
                notifications: 0,
            }),
            Ok(false) => Ok(XAuthorityOutputUpdateOutcome::RejectedStale { generation }),
            Err(error) => Ok(XAuthorityOutputUpdateOutcome::RejectedInvalid { generation, error }),
        }
    }

    /// Number of X11 clients currently holding a frontend connection lease.
    ///
    /// With the present sequential dispatcher this is normally zero between
    /// `serve_next` calls. Concurrent workers retain their lease until stream
    /// teardown finishes, so the value is also useful for supervision.
    pub fn active_client_count(&self) -> usize {
        self.state.active_client_count()
    }

    /// Number of worker threads currently supervised by the concurrent APIs.
    ///
    /// This includes a worker while it is completing X11 setup, before that
    /// connection receives its client lease.
    pub fn active_client_worker_count(&self) -> usize {
        self.workers.len()
    }

    pub fn clipboard_executor(
        &self,
        broker: &XServerFrontendRouteBroker,
    ) -> XServerFrontendClipboardExecutor {
        XServerFrontendClipboardExecutor {
            state: self.state.clone(),
            routing: broker.registry.clone(),
        }
    }

    /// Reaps every concurrent worker that has already completed without
    /// waiting for an active client.
    pub fn poll_client_workers(&mut self) -> Result<(), X11SetupSocketError> {
        self.reap_finished_client_workers()?;
        self.state.release_exported_pixmaps()
    }

    /// Shuts down every accepted client stream while leaving worker threads
    /// responsible for their normal route and resource cleanup.
    pub fn shutdown_all_client_workers(&self) -> Result<(), X11SetupSocketError> {
        let mut failures = Vec::new();
        for worker_id in self.workers.keys().copied() {
            if let Err(error) = self.shutdown_worker(worker_id) {
                failures.push(error.to_string());
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(X11SetupSocketError::new(failures.join("; ")))
        }
    }

    /// Disconnects the worker holding one session-issued admission.
    ///
    /// The worker retains teardown ownership: it stops its private writers,
    /// releases routes and X resources, emits surface removal, and only then
    /// revokes the admission lease. An admission that is not attached yet is
    /// retained as a pending revocation so a setup race cannot lose the
    /// supervisor command; `Ok(false)` reports that deferred outcome.
    pub fn revoke_admission(
        &mut self,
        admission: ClientAdmissionId,
    ) -> Result<bool, X11SetupSocketError> {
        self.reap_finished_client_workers()?;
        self.observe_worker_admissions()?;
        let Some(worker_id) = self.worker_admissions.remove(&admission) else {
            self.pending_admission_revocations.insert(admission);
            return Ok(false);
        };
        if let Err(error) = self.shutdown_worker(worker_id) {
            self.worker_admissions.insert(admission, worker_id);
            return Err(error);
        }
        Ok(true)
    }

    /// Starts one client worker, if the configured concurrency limit permits
    /// it, and returns as soon as that connection is accepted.
    ///
    /// Call [`Self::wait_for_clients`] before releasing a manually supervised
    /// frontend so every accepted connection is reaped. The observer must be
    /// thread-safe because worker callbacks may run concurrently.
    pub fn serve_next_concurrently(&mut self) -> Result<(), X11SetupSocketError> {
        let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
        self.serve_next_concurrently_traced(observer)
    }

    /// Like [`Self::serve_next_concurrently`], with an observer for each
    /// completed X11 dispatch.
    pub fn serve_next_concurrently_traced(
        &mut self,
        observer: Arc<X11CoreTraceObserver>,
    ) -> Result<(), X11SetupSocketError> {
        self.serve_next_concurrently_with_routing(observer, None)
    }

    /// Starts one concurrent client worker with the Engine-facing route broker
    /// attached to its private input and control queues.
    pub fn serve_next_concurrently_routed(
        &mut self,
        broker: &XServerFrontendRouteBroker,
    ) -> Result<(), X11SetupSocketError> {
        let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
        self.serve_next_concurrently_routed_traced(broker, observer)
    }

    /// Like [`Self::serve_next_concurrently_routed`], with a thread-safe
    /// observer for each completed X11 dispatch.
    pub fn serve_next_concurrently_routed_traced(
        &mut self,
        broker: &XServerFrontendRouteBroker,
        observer: Arc<X11CoreTraceObserver>,
    ) -> Result<(), X11SetupSocketError> {
        self.serve_next_concurrently_with_routing(observer, Some(broker.registry.clone()))
    }

    /// Attempts to accept one routed concurrent client without blocking.
    ///
    /// `Ok(false)` means no connection is ready. The configured worker limit
    /// remains enforced as an error, so a service must reap completed workers
    /// before attempting another accept.
    pub fn try_serve_next_concurrently_routed_traced(
        &mut self,
        broker: &XServerFrontendRouteBroker,
        observer: Arc<X11CoreTraceObserver>,
    ) -> Result<bool, X11SetupSocketError> {
        self.try_serve_next_concurrently_with_routing(observer, Some(broker.registry.clone()))
    }

    fn serve_next_concurrently_with_routing(
        &mut self,
        observer: Arc<X11CoreTraceObserver>,
        routing: Option<XServerFrontendRouteRegistry>,
    ) -> Result<(), X11SetupSocketError> {
        self.accept_next_concurrently_with_routing(observer, routing, false)
            .map(|_| ())
    }

    fn try_serve_next_concurrently_with_routing(
        &mut self,
        observer: Arc<X11CoreTraceObserver>,
        routing: Option<XServerFrontendRouteRegistry>,
    ) -> Result<bool, X11SetupSocketError> {
        self.accept_next_concurrently_with_routing(observer, routing, true)
    }

    fn accept_next_concurrently_with_routing(
        &mut self,
        observer: Arc<X11CoreTraceObserver>,
        routing: Option<XServerFrontendRouteRegistry>,
        nonblocking: bool,
    ) -> Result<bool, X11SetupSocketError> {
        self.reap_finished_client_workers()?;
        let limit = self.config.max_concurrent_clients().get();
        if self.workers.len() >= limit {
            return Err(X11SetupSocketError::new(format!(
                "Sophia X Server Frontend concurrent-client limit ({limit}) reached"
            )));
        }
        let accepted = if nonblocking {
            self.listener.set_nonblocking(true).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to make X11 core listener nonblocking: {error}"
                ))
            })?;
            let accepted = self.listener.accept();
            self.listener.set_nonblocking(false).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to restore blocking X11 core listener: {error}"
                ))
            })?;
            accepted
        } else {
            self.listener.accept()
        };
        match accepted {
            Ok((stream, _)) => {
                self.spawn_client_worker(stream, observer, routing)?;
                Ok(true)
            }
            Err(error) if nonblocking && error.kind() == ErrorKind::WouldBlock => Ok(false),
            Err(error) => Err(X11SetupSocketError::new(format!(
                "failed to accept X11 core client: {error}"
            ))),
        }
    }

    /// Reaps every connection worker started by the concurrent APIs.
    ///
    /// This is the explicit supervision boundary for a caller that accepts a
    /// bounded batch of local clients. It waits only for already accepted
    /// clients; it does not accept another connection.
    pub fn wait_for_clients(&mut self) -> Result<(), X11SetupSocketError> {
        let mut first_error = self.reap_finished_client_workers().err();
        while !self.workers.is_empty() {
            if let Err(error) = self.observe_worker_admissions()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
            let completion = if self.pending_admission_revocations.is_empty() {
                match self.worker_completions.recv() {
                    Ok(completion) => completion,
                    Err(_) => {
                        return Err(first_error.unwrap_or_else(|| {
                            X11SetupSocketError::new(
                                "Sophia X Server Frontend concurrent worker supervisor disconnected",
                            )
                        }));
                    }
                }
            } else {
                match self
                    .worker_completions
                    .recv_timeout(Duration::from_millis(1))
                {
                    Ok(completion) => completion,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(first_error.unwrap_or_else(|| {
                            X11SetupSocketError::new(
                                "Sophia X Server Frontend concurrent worker supervisor disconnected",
                            )
                        }));
                    }
                }
            };
            if let Err(error) = self.reap_client_worker(completion)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn serve_next(&mut self) -> Result<(), X11SetupSocketError> {
        serve_x11_core_socket_listener_once_with_setup_authorization(
            &self.listener,
            self.config.namespace(),
            &self.state,
            self.config.setup_authorization(),
            self.config.admission_policy(),
            self.config.injection_policy(),
            None,
            |_| Ok(()),
        )
    }

    pub fn serve_next_traced(
        &mut self,
        observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
    ) -> Result<(), X11SetupSocketError> {
        serve_x11_core_socket_listener_once_with_setup_authorization(
            &self.listener,
            self.config.namespace(),
            &self.state,
            self.config.setup_authorization(),
            self.config.admission_policy(),
            self.config.injection_policy(),
            None,
            observer,
        )
    }

    pub fn serve_forever(&mut self) -> Result<(), X11SetupSocketError> {
        self.serve_forever_traced(|_| Ok(()))
    }

    pub fn serve_forever_traced(
        &mut self,
        observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
    ) -> Result<(), X11SetupSocketError> {
        serve_x11_core_socket_listener_with_setup_authorization(
            &self.listener,
            self.config.namespace(),
            &self.state,
            self.config.setup_authorization(),
            self.config.admission_policy(),
            self.config.injection_policy(),
            observer,
        )
    }

    fn spawn_client_worker(
        &mut self,
        mut stream: UnixStream,
        observer: Arc<X11CoreTraceObserver>,
        routing: Option<XServerFrontendRouteRegistry>,
    ) -> Result<(), X11SetupSocketError> {
        let worker_id = self.next_worker_id;
        self.next_worker_id = self.next_worker_id.checked_add(1).ok_or_else(|| {
            X11SetupSocketError::new("Sophia X Server Frontend exhausted worker identities")
        })?;
        let state = self.state.clone();
        let namespace = self.config.namespace();
        let authorization = self.config.setup_authorization().clone();
        let admission_policy = self.config.admission_policy();
        let injection_policy = self.config.injection_policy();
        let completion_sender = self.worker_completion_sender.clone();
        let admission_event_sender = self.worker_admission_event_sender.clone();
        #[cfg(all(test, unix))]
        let acceptance_origin = routing.clone();
        let shutdown = stream.try_clone().map_err(|error| {
            X11SetupSocketError::new(format!(
                "failed to clone X11 client socket for supervision: {error}"
            ))
        })?;
        let worker = std::thread::Builder::new()
            .name(format!("sophia-x11-client-{worker_id}"))
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    serve_x11_core_socket_client_with_trace_observer_and_input(
                        &mut stream,
                        namespace,
                        &state,
                        X11ClientConnectionInputs {
                            input_receiver: None,
                            control_channels: None,
                            client_routing: routing,
                        },
                        X11ClientAdmissionContext {
                            authorization: &authorization,
                            admission_policy,
                            injection_policy,
                            worker_admission: Some((worker_id, admission_event_sender)),
                        },
                        move |trace| observer(trace),
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(X11SetupSocketError::new(
                        "Sophia X Server Frontend client worker panicked",
                    ))
                });
                let _ = completion_sender.send(X11CoreClientWorkerCompletion { worker_id, result });
            })
            .map_err(|error| {
                X11SetupSocketError::new(format!("failed to start X11 client worker: {error}"))
            })?;
        #[cfg(all(test, unix))]
        if let Some(registry) = acceptance_origin.as_ref() {
            routing_tests::m3_acceptance::actor_started(registry, worker.thread().id(), "connection");
        }
        self.workers.insert(
            worker_id,
            X11CoreClientWorker {
                thread: worker,
                shutdown,
            },
        );
        Ok(())
    }

    fn reap_finished_client_workers(&mut self) -> Result<(), X11SetupSocketError> {
        self.observe_worker_admissions()?;
        loop {
            match self.worker_completions.try_recv() {
                Ok(completion) => self.reap_client_worker(completion)?,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) if self.workers.is_empty() => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(X11SetupSocketError::new(
                        "Sophia X Server Frontend concurrent worker supervisor disconnected",
                    ));
                }
            }
        }
    }

    fn reap_client_worker(
        &mut self,
        completion: X11CoreClientWorkerCompletion,
    ) -> Result<(), X11SetupSocketError> {
        let worker = self.workers.remove(&completion.worker_id).ok_or_else(|| {
            X11SetupSocketError::new("Sophia X Server Frontend lost a concurrent client worker")
        })?;
        self.worker_admissions
            .retain(|_, worker_id| *worker_id != completion.worker_id);
        #[cfg(all(test, unix))]
        let thread = worker.thread.thread().id();
        let joined = worker.thread.join();
        #[cfg(all(test, unix))]
        routing_tests::m3_acceptance::actor_joined(thread);
        joined.map_err(|_| {
            X11SetupSocketError::new("Sophia X Server Frontend client worker panicked")
        })?;
        match completion.result {
            Err(error)
                if error.client_failure || error.client_disconnect || error.service_shutdown =>
            {
                tracing::debug!(
                    client_failure = error.client_failure,
                    client_disconnect = error.client_disconnect,
                    service_shutdown = error.service_shutdown,
                    reason = %error,
                    "Sophia X Server Frontend disconnected one client"
                );
                Ok(())
            }
            result => result,
        }
    }

    fn observe_worker_admissions(&mut self) -> Result<(), X11SetupSocketError> {
        loop {
            match self.worker_admission_events.try_recv() {
                Ok(event) if self.workers.contains_key(&event.worker_id) => {
                    match self.worker_admissions.get(&event.admission).copied() {
                        Some(existing) if existing != event.worker_id => {
                            return Err(X11SetupSocketError::new(
                                "Sophia X Server Frontend admission is attached to multiple workers",
                            ));
                        }
                        Some(_) => {}
                        None => {
                            self.worker_admissions
                                .insert(event.admission, event.worker_id);
                        }
                    }
                    if self.pending_admission_revocations.remove(&event.admission) {
                        self.shutdown_worker(event.worker_id)?;
                        self.worker_admissions.remove(&event.admission);
                    }
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) if self.workers.is_empty() => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(X11SetupSocketError::new(
                        "Sophia X Server Frontend admission observer disconnected",
                    ));
                }
            }
        }
    }

    fn shutdown_worker(&self, worker_id: u64) -> Result<(), X11SetupSocketError> {
        let Some(worker) = self.workers.get(&worker_id) else {
            return Ok(());
        };
        match worker.shutdown.shutdown(Shutdown::Both) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotConnected => Ok(()),
            Err(error) => Err(X11SetupSocketError::new(format!(
                "failed to revoke X11 client admission: {error}"
            ))),
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityOutputUpdateOutcome {
    Applied {
        generation: u64,
        /// RandR records queued to subscribed live clients by the routed
        /// service. Direct frontend updates retain zero.
        notifications: usize,
    },
    RejectedStale {
        generation: u64,
    },
    RejectedInvalid {
        generation: u64,
        error: sophia_protocol::OutputTopologyError,
    },
}
