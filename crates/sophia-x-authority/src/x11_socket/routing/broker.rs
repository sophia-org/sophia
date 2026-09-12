/// Engine-facing ingress and per-client queue registry for a routed X11
/// session.
///
/// Engine code sends client-addressed input through the bounded ingress queues,
/// then its session loop calls [`Self::route_pending`] to move it into the
/// registered worker's private queue. Latency-sensitive control can instead use
/// [`Self::control_router`] to reach the selected client's bounded queue
/// directly. The broker never broadcasts a route. Routes whose client
/// disappeared after Engine selection are retired with a negative
/// acknowledgement. A client that saturates its private input queue is
/// quarantined without terminating the shared frontend; corruption of shared
/// registry state remains service-fatal.
#[cfg(unix)]
pub struct XServerFrontendRouteBroker {
    registry: XServerFrontendRouteRegistry,
    input_sender: SyncSender<XAuthorityClientInputEvent>,
    input_receiver: Receiver<XAuthorityClientInputEvent>,
    routed_input_sender: SyncSender<XAuthorityEpochRoutedInput>,
    routed_input_receiver: Receiver<XAuthorityEpochRoutedInput>,
    input_control_epoch: Arc<AtomicU64>,
    routed_input_capacity: usize,
    applied_input_control_epoch: u64,
    /// Unset in ordinary mode, leaving every path below exactly as it was.
    control_gate: Arc<std::sync::OnceLock<crate::ControlEpochGate>>,
    route_lease_release_sender: SyncSender<XAuthorityRouteLeaseRelease>,
    route_lease_release_receiver: Receiver<XAuthorityRouteLeaseRelease>,
    control_sender: SyncSender<XAuthorityClientControlCommand>,
    control_receiver: Receiver<XAuthorityClientControlCommand>,
    acknowledgement_receiver: Option<Receiver<XAuthorityClientControlAck>>,
    metadata_candidate_receiver: Option<Receiver<XAuthorityClientMetadataCandidate>>,
    source_payload_receiver: Receiver<crate::ClipboardSourcePayload>,
    raster_sender: SyncSender<sophia_protocol::SurfaceRasterRequirements>,
    raster_receiver: Receiver<sophia_protocol::SurfaceRasterRequirements>,
}

#[cfg(unix)]
#[derive(Clone)]
pub struct XAuthorityRoutedInputSender {
    sender: SyncSender<XAuthorityEpochRoutedInput>,
    control_epoch: Arc<AtomicU64>,
    capacity: usize,
    recovery: InputRecovery,
    /// Shared with the broker rather than copied from it.
    ///
    /// A sender handed out before the gate was installed would otherwise keep
    /// its own `None` and go on stamping from the bare counter, which is an
    /// ungated route into a gated broker.
    control_gate: Arc<std::sync::OnceLock<crate::ControlEpochGate>>,
}

#[cfg(unix)]
impl XAuthorityRoutedInputSender {
    /// Stamp work once, at enqueue.
    ///
    /// Without a coordinator this is the counter, exactly as before. With one,
    /// a transition in flight yields no stamp at all, so the work is refused
    /// here rather than queued against a revision that is being replaced.
    fn stamp(&self) -> Result<crate::ControlStamp, ()> {
        match self.control_gate.get() {
            Some(gate) => gate.stamp().map_err(|_| ()),
            None => Ok(crate::ControlStamp {
                control_epoch: self.control_epoch.load(Ordering::Acquire),
                publication: 0,
            }),
        }
    }
}

#[cfg(unix)]
impl XAuthorityRoutedInputSender {
    pub fn send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::SendError<XAuthorityRoutedInput>> {
        let stamp = match self.stamp() {
            Ok(stamp) => stamp,
            Err(()) => return Err(std::sync::mpsc::SendError(route)),
        };
        let envelope = XAuthorityEpochRoutedInput {
            control_epoch: stamp.control_epoch,
            publication: stamp.publication,
            route,
        };
        if !self.recovery.admit(&envelope.route, envelope.control_epoch, Instant::now()) {
            return Err(std::sync::mpsc::SendError(envelope.route));
        }
        self.sender.send(envelope).map_err(|error| {
            self.recovery.abort_enqueue(error.0.route.delivery);
            std::sync::mpsc::SendError(error.0.route)
        })
    }

    pub fn try_send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityRoutedInput>> {
        let stamp = match self.stamp() {
            Ok(stamp) => stamp,
            Err(()) => return Err(std::sync::mpsc::TrySendError::Full(route)),
        };
        let envelope = XAuthorityEpochRoutedInput {
            control_epoch: stamp.control_epoch,
            publication: stamp.publication,
            route,
        };
        if !self.recovery.admit(&envelope.route, envelope.control_epoch, Instant::now()) {
            return Err(TrySendError::Full(envelope.route));
        }
        self.sender.try_send(envelope).map_err(|error| {
            let (envelope, full) = match error {
                TrySendError::Full(envelope) => (envelope, true),
                TrySendError::Disconnected(envelope) => (envelope, false),
            };
            self.recovery.abort_enqueue(envelope.route.delivery);
            if full { TrySendError::Full(envelope.route) }
            else { TrySendError::Disconnected(envelope.route) }
        })
    }

    /// The queue's bound, so a saturation report can say what was exhausted
    /// rather than only that something was.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn control_epoch(&self) -> u64 {
        self.control_epoch.load(Ordering::Acquire)
    }

    pub fn advance_control_epoch(&self, next: u64) -> bool {
        // A coordinator owns every transition it is installed for, so the
        // lockless path is refused rather than quietly racing it.
        if self.control_gate.get().is_some() {
            return false;
        }
        let mut current = self.control_epoch.load(Ordering::Acquire);
        loop {
            if next <= current {
                return next == current;
            }
            match self.control_epoch.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }
}

/// Cloneable protocol-feedback handle for Engine/backend presentation code.
///
/// This handle can outlive the broker value moved into the X11 service loop,
/// but it exposes only frontend protocol completion. It cannot route input,
/// mutate scene state, submit scanout, or access native renderer resources.
#[cfg(unix)]
#[derive(Clone)]
pub struct XServerFrontendProtocolRouter {
    registry: XServerFrontendRouteRegistry,
}

/// Cloneable control handle for Engine-owned focus and configure commands.
///
/// This handle routes directly to the selected client's bounded queue so
/// latency-sensitive control does not wait behind frontend input processing.
#[cfg(unix)]
#[derive(Clone)]
pub struct XServerFrontendControlRouter {
    registry: XServerFrontendRouteRegistry,
}

/// Protocol-neutral route for Engine-owned native-density raster demand.
/// The payload contains only `SurfaceId`, generation, logical extent, and
/// bounded density classes; X11 resource and physical-head identity stay in
/// the frontend.
#[cfg(unix)]
#[derive(Clone)]
pub struct XServerFrontendRasterRouter {
    sender: SyncSender<sophia_protocol::SurfaceRasterRequirements>,
}

#[cfg(unix)]
impl XServerFrontendRasterRouter {
    pub fn try_route(
        &self,
        requirements: sophia_protocol::SurfaceRasterRequirements,
    ) -> Result<(), TrySendError<sophia_protocol::SurfaceRasterRequirements>> {
        self.sender.try_send(requirements)
    }
}

/// Independent bounds for routes whose payloads have different expansion
/// factors and service rates at the X11 socket boundary.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XServerFrontendRouteCapacities {
    pub input: NonZeroUsize,
    pub control: NonZeroUsize,
    pub protocol: NonZeroUsize,
    pub presentations: NonZeroUsize,
}

#[cfg(unix)]
impl XServerFrontendRouteCapacities {
    pub const fn uniform(capacity: NonZeroUsize) -> Self {
        Self {
            input: capacity,
            control: capacity,
            protocol: capacity,
            presentations: capacity,
        }
    }

    pub const fn new(
        input: NonZeroUsize,
        control: NonZeroUsize,
        protocol: NonZeroUsize,
        presentations: NonZeroUsize,
    ) -> Self {
        Self {
            input,
            control,
            protocol,
            presentations,
        }
    }
}

#[cfg(unix)]
impl XServerFrontendControlRouter {
    pub fn route_control(
        &self,
        route: XAuthorityClientControlCommand,
    ) -> Result<(), XServerFrontendRouteError> {
        match self.registry.route_control(route) {
            Ok(()) => Ok(()),
            Err(
                XServerFrontendRouteError::UnknownClient { .. }
                | XServerFrontendRouteError::ClientQueueDisconnected { .. },
            ) => self.registry.acknowledge_stale_control(route),
            Err(error) => Err(error),
        }
    }
}

#[cfg(unix)]
impl XServerFrontendProtocolRouter {
    pub fn route_present_complete_with_layout(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
        comparison: Option<crate::XPresentLayoutComparison>,
    ) -> Result<crate::XPresentCompleteRouteOutcome, XServerFrontendRouteError> {
        self.registry
            .route_present_complete_with_layout(transaction, ust, msc, mode, comparison)
    }

    pub fn route_present_complete(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
    ) -> Result<bool, XServerFrontendRouteError> {
        self.registry
            .route_present_complete(transaction, ust, msc, mode)
    }

    pub fn route_present_idle(
        &self,
        transaction: TransactionId,
    ) -> Result<bool, XServerFrontendRouteError> {
        self.registry.route_present_idle(transaction)
    }
}

#[cfg(unix)]
impl XServerFrontendRouteBroker {
    pub fn with_explicit_pointer_grab_client(
        mut self,
        client: crate::XAuthorityExplicitPointerGrabClient,
    ) -> Self {
        self.registry.explicit_pointer_grabs = Some(client);
        self
    }

    pub fn new(queue_capacity: NonZeroUsize) -> Self {
        let capacity = queue_capacity.get();
        let (acknowledgement_sender, acknowledgement_receiver) = sync_channel(capacity);
        Self::with_transports(
            XServerFrontendRouteCapacities::uniform(queue_capacity),
            acknowledgement_sender,
            Some(acknowledgement_receiver),
            None,
            None,
        )
    }

    /// Creates a broker whose control acknowledgements return to the supplied
    /// Engine-owned bounded queue.
    pub fn with_control_ack_sender(
        queue_capacity: NonZeroUsize,
        acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
    ) -> Self {
        Self::with_transports(
            XServerFrontendRouteCapacities::uniform(queue_capacity),
            acknowledgement_sender,
            None,
            None,
            None,
        )
    }

    /// Creates a broker whose focus/configure and input-flush acknowledgements
    /// return through Engine-owned queues.
    pub fn with_control_and_input_delivery_senders(
        queue_capacity: NonZeroUsize,
        acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
        input_delivery_sender: Sender<XAuthorityClientInputDelivery>,
    ) -> Self {
        Self::with_transports(
            XServerFrontendRouteCapacities::uniform(queue_capacity),
            acknowledgement_sender,
            None,
            Some(input_delivery_sender),
            None,
        )
    }

    pub fn with_control_and_input_delivery_senders_and_xkb_config(
        queue_capacity: NonZeroUsize,
        acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
        input_delivery_sender: Sender<XAuthorityClientInputDelivery>,
        xkb_config: crate::XkbRmlvoConfig,
    ) -> Result<Self, crate::XkbKeyboardError> {
        crate::XkbKeyboardState::new(&xkb_config)?;
        let mut broker = Self::with_transports(
            XServerFrontendRouteCapacities::uniform(queue_capacity),
            acknowledgement_sender,
            None,
            Some(input_delivery_sender),
            None,
        );
        broker.registry.xkb_config = xkb_config.clone();
        broker.registry.xkb_worker = XkbKeyboardWorker::spawn(xkb_config);
        Ok(broker)
    }

    pub fn with_route_capacities_and_xkb_config(
        capacities: XServerFrontendRouteCapacities,
        acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
        input_delivery_sender: Sender<XAuthorityClientInputDelivery>,
        xkb_config: crate::XkbRmlvoConfig,
    ) -> Result<Self, crate::XkbKeyboardError> {
        crate::XkbKeyboardState::new(&xkb_config)?;
        let mut broker = Self::with_transports(
            capacities,
            acknowledgement_sender,
            None,
            Some(input_delivery_sender),
            None,
        );
        broker.registry.xkb_config = xkb_config.clone();
        broker.registry.xkb_worker = XkbKeyboardWorker::spawn(xkb_config);
        Ok(broker)
    }

    pub fn with_route_capacities_xkb_and_lease_updates(
        capacities: XServerFrontendRouteCapacities,
        acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
        input_delivery_sender: Sender<XAuthorityClientInputDelivery>,
        route_lease_update_sender: SyncSender<XAuthorityRouteLeaseUpdate>,
        xkb_config: crate::XkbRmlvoConfig,
    ) -> Result<Self, crate::XkbKeyboardError> {
        crate::XkbKeyboardState::new(&xkb_config)?;
        let mut broker = Self::with_transports(
            capacities,
            acknowledgement_sender,
            None,
            Some(input_delivery_sender),
            Some(route_lease_update_sender),
        );
        broker.registry.xkb_config = xkb_config.clone();
        broker.registry.xkb_worker = XkbKeyboardWorker::spawn(xkb_config);
        Ok(broker)
    }

    fn with_transports(
        capacities: XServerFrontendRouteCapacities,
        acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
        acknowledgement_receiver: Option<Receiver<XAuthorityClientControlAck>>,
        input_delivery_sender: Option<Sender<XAuthorityClientInputDelivery>>,
        route_lease_update_sender: Option<SyncSender<XAuthorityRouteLeaseUpdate>>,
    ) -> Self {
        let (input_sender, input_receiver) = sync_channel(capacities.input.get());
        let (routed_input_sender, routed_input_receiver) = sync_channel(capacities.input.get());
        let input_control_epoch = Arc::new(AtomicU64::new(1));
        let (route_lease_release_sender, route_lease_release_receiver) =
            sync_channel(capacities.control.get());
        let (control_sender, control_receiver) = sync_channel(capacities.control.get());
        let (metadata_candidate_sender, metadata_candidate_receiver) =
            sync_channel(capacities.control.get());
        let (source_payload_sender, source_payload_receiver) =
            sync_channel(capacities.input.get());
        let (raster_sender, raster_receiver) = sync_channel(capacities.control.get());
        let input_authority = Arc::new(Mutex::new(crate::XInputAuthorityState::default()));
        Self {
            control_gate: Arc::new(std::sync::OnceLock::new()),
            registry: XServerFrontendRouteRegistry {
                // Ingress + frozen + every possible client's private queue and
                // active writer. Terminal receipts retain their credit until
                // observed, so a slow owner cannot grow an unbounded ledger.
                input_recovery: InputRecovery::new(
                    capacities.input.get().saturating_mul(2).saturating_add(
                        usize::from(X11_MAX_CLIENT_RESOURCE_RANGES)
                            .saturating_mul(capacities.input.get().saturating_add(1))),
                    input_delivery_sender.clone(), input_authority.clone(),
                ),
                runtime: Arc::new(std::sync::OnceLock::new()),
                clients: Arc::new(Mutex::new(BTreeMap::new())),
                surfaces: Arc::new(Mutex::new(BTreeMap::new())),
                focused_surface: Arc::new(Mutex::new(None)),
                window_parents: Arc::new(Mutex::new(BTreeMap::new())),
                core_event_subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                randr_subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                xfixes_selection_subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                present_subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
                pending_presentations: Arc::new(XPendingPresentRegistry::default()),
                present_clock: Arc::new(Mutex::new(None)),
                pending_msc_notifies: Arc::new(Mutex::new(Vec::new())),
                pointer_state: Arc::new(Mutex::new(BTreeMap::new())),
                input_authority,
                frozen_input: Arc::new(Mutex::new(VecDeque::new())),
                xkb_config: crate::XkbRmlvoConfig::default(),
                xkb_worker: XkbKeyboardWorker::spawn(crate::XkbRmlvoConfig::default()),
                acknowledgement_sender,
                input_delivery_sender,
                metadata_candidate_sender,
                route_lease_update_sender,
                explicit_pointer_grabs: None,
                input_control_epoch: input_control_epoch.clone(),
                per_client_input_capacity: capacities.input,
                per_client_control_capacity: capacities.control,
                per_client_protocol_capacity: capacities.protocol,
                per_client_presentation_capacity: capacities.presentations,
                source_payload_sender,
            },
            input_sender,
            input_receiver,
            routed_input_sender,
            routed_input_receiver,
            input_control_epoch,
            routed_input_capacity: capacities.input.get(),
            applied_input_control_epoch: 1,
            route_lease_release_sender,
            route_lease_release_receiver,
            control_sender,
            control_receiver,
            acknowledgement_receiver,
            metadata_candidate_receiver: Some(metadata_candidate_receiver),
            source_payload_receiver,
            raster_sender,
            raster_receiver,
        }
    }

    pub fn input_sender(&self) -> SyncSender<XAuthorityClientInputEvent> {
        self.input_sender.clone()
    }

    pub fn routed_input_sender(&self) -> XAuthorityRoutedInputSender {
        XAuthorityRoutedInputSender {
            control_gate: Arc::clone(&self.control_gate),
            sender: self.routed_input_sender.clone(),
            control_epoch: self.input_control_epoch.clone(),
            capacity: self.routed_input_capacity,
            recovery: self.registry.input_recovery.clone(),
        }
    }

    pub fn route_lease_release_sender(&self) -> SyncSender<XAuthorityRouteLeaseRelease> {
        self.route_lease_release_sender.clone()
    }

    pub fn control_sender(&self) -> SyncSender<XAuthorityClientControlCommand> {
        self.control_sender.clone()
    }

    pub fn control_router(&self) -> XServerFrontendControlRouter {
        XServerFrontendControlRouter {
            registry: self.registry.clone(),
        }
    }

    pub fn take_metadata_candidate_receiver(
        &mut self,
    ) -> Option<Receiver<XAuthorityClientMetadataCandidate>> {
        self.metadata_candidate_receiver.take()
    }

    pub fn raster_router(&self) -> XServerFrontendRasterRouter {
        XServerFrontendRasterRouter {
            sender: self.raster_sender.clone(),
        }
    }

    pub(crate) fn try_recv_raster_requirements(
        &self,
    ) -> Result<
        sophia_protocol::SurfaceRasterRequirements,
        std::sync::mpsc::TryRecvError,
    > {
        self.raster_receiver.try_recv()
    }

    pub fn recv_control_ack_timeout(
        &self,
        timeout: Duration,
    ) -> Result<XAuthorityClientControlAck, RecvTimeoutError> {
        self.acknowledgement_receiver
            .as_ref()
            .ok_or(RecvTimeoutError::Disconnected)?
            .recv_timeout(timeout)
    }

    pub fn recv_clipboard_source_payload_timeout(
        &self,
        timeout: Duration,
    ) -> Result<crate::ClipboardSourcePayload, RecvTimeoutError> {
        self.source_payload_receiver.recv_timeout(timeout)
    }

    /// Put this broker under a coordinator.
    ///
    /// Absent one, every path here is what it was: the counter is the whole
    /// answer and publication plays no part. Present, stamping and admission
    /// both defer to it, and a transition in flight refuses new work at
    /// enqueue rather than queueing it against a revision being replaced.
    pub fn under_control_gate(self, gate: crate::ControlEpochGate) -> Self {
        // Set once, into a cell every sender already holds. Senders taken
        // before this call observe it too, so there is no ungated escape, and
        // it cannot later be swapped for a different coordinator.
        let _ = self.control_gate.set(gate);
        self
    }

    /// Whether a coordinator owns this broker's epoch transitions.
    fn is_gated(&self) -> bool {
        self.control_gate.get().is_some()
    }

    /// Apply a control transition's X-side clearing and report it.
    ///
    /// The privileged control path. The caller already holds the coordinator
    /// and, beneath it, the common guard it used to open this transition;
    /// this takes only the later-ranked X guards. That order matters: opening
    /// a transition takes the coordinator and then common, so acquiring them
    /// the other way round here would close a cycle between them.
    ///
    /// `snapshot_installed` is the caller's, because Session installs the
    /// publication snapshot and this path speaks only for what it cleared. The
    /// coordinator stamps its applied epoch only if the two together satisfy
    /// the kind of transition in flight.
    ///
    /// Receipts for revoked work are sent after the X guards are released, so
    /// a transition never waits on a queue while holding them.
    pub fn apply_control_transition(
        &self,
        coordinator: &mut crate::ControlEpochCoordinator,
        token: crate::TransitionToken,
        snapshot_installed: bool,
    ) -> Result<usize, XServerFrontendRouteError> {
        let (cleared, drained) = self.registry.clear_revoked_x_populations()?;
        coordinator
            .apply(
                token,
                crate::TransitionInstallation {
                    snapshot_installed,
                    ..cleared
                },
            )
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        self.registry.report_revoked_input(drained)
    }

    /// Routes every value currently available at the bounded ingress.
    pub fn route_pending(&mut self) -> Result<usize, XServerFrontendRouteError> {
        // Under a coordinator this application belongs to the ranked apply,
        // not here: running both would clear the same populations from two
        // places, through three sequentially taken locks that no transition
        // owns.
        //
        // Today this guard cannot be observed to matter, because the counter
        // it reads has exactly one writer -- the compare-and-swap below, which
        // a gate already refuses -- so under a gate the counter never moves.
        // It is kept against a second writer appearing, which would otherwise
        // reach this application silently.
        if !self.is_gated() {
            let input_control_epoch = self.input_control_epoch.load(Ordering::Acquire);
            if input_control_epoch != self.applied_input_control_epoch {
                self.registry.advance_input_control_epoch()?;
                self.applied_input_control_epoch = input_control_epoch;
            }
        }
        let mut routed = 0usize;
        loop {
            let mut progressed = false;
            match self.route_lease_release_receiver.try_recv() {
                Ok(release) => {
                    self.registry.release_route_lease(release)?;
                    routed = routed.saturating_add(1);
                    progressed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
            }
            match self.routed_input_receiver.try_recv() {
                Ok(route) => {
                    let admitted = match self.control_gate.get() {
                        Some(gate) => gate
                            .admits(crate::ControlStamp {
                                control_epoch: route.control_epoch,
                                publication: route.publication,
                            })
                            .is_ok(),
                        None => {
                            route.control_epoch
                                == self.input_control_epoch.load(Ordering::Acquire)
                        }
                    };
                    match self.registry.route_engine_input_admitted(
                        route.route,
                        crate::ControlStamp {
                            control_epoch: route.control_epoch,
                            publication: route.publication,
                        },
                        admitted,
                    ) {
                        Ok(()) => routed = routed.saturating_add(1),
                        Err(
                            XServerFrontendRouteError::UnknownSurface { .. }
                            | XServerFrontendRouteError::ClientQueueDisconnected { .. }
                            | XServerFrontendRouteError::UnknownClient { .. }
                            | XServerFrontendRouteError::ClientQueueFull { .. },
                        ) => {}
                        Err(error) => return Err(error),
                    }
                    progressed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
            }
            match self.input_receiver.try_recv() {
                Ok(route) => {
                    self.registry.observe_direct_query_input(&route)?;
                    if let Err(error) = self.registry.route_input(route) {
                        self.registry.send_input_delivery(
                            route.client,
                            route.delivery,
                            XAuthorityInputDeliveryOutcome::RouteRejected,
                        )?;
                        match error {
                            XServerFrontendRouteError::UnknownClient { .. }
                            | XServerFrontendRouteError::ClientQueueDisconnected { .. }
                            | XServerFrontendRouteError::ClientQueueFull { .. } => {}
                            error => return Err(error),
                        }
                    } else {
                        routed = routed.saturating_add(1);
                    }
                    progressed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
            }
            match self.control_receiver.try_recv() {
                Ok(route) => {
                    match self.registry.route_control(route) {
                        Ok(()) => {}
                        Err(
                            XServerFrontendRouteError::UnknownClient { .. }
                            | XServerFrontendRouteError::ClientQueueDisconnected { .. },
                        ) => {
                            self.registry.acknowledge_stale_control(route)?;
                        }
                        Err(error) => return Err(error),
                    }
                    routed = routed.saturating_add(1);
                    progressed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
            }
            let thawed = match self
                .registry
                .drain_thawed_input(
                    self.input_control_epoch.load(Ordering::Acquire),
                    self.control_gate.get(),
                )
            {
                Ok(thawed) => thawed,
                Err(
                    XServerFrontendRouteError::UnknownSurface { .. }
                    | XServerFrontendRouteError::ClientQueueDisconnected { .. }
                    | XServerFrontendRouteError::UnknownClient { .. }
                    | XServerFrontendRouteError::ClientQueueFull { .. },
                ) => {
                    progressed = true;
                    0
                }
                Err(error) => return Err(error),
            };
            if thawed != 0 {
                routed = routed.saturating_add(thawed);
                progressed = true;
            }
            if !progressed {
                return Ok(routed);
            }
        }
    }

    pub fn registered_client_count(&self) -> usize {
        self.registry.registered_client_count()
    }

    pub fn protocol_router(&self) -> XServerFrontendProtocolRouter {
        XServerFrontendProtocolRouter {
            registry: self.registry.clone(),
        }
    }

    pub fn route_present_complete(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
    ) -> Result<bool, XServerFrontendRouteError> {
        self.registry
            .route_present_complete(transaction, ust, msc, mode)
    }

    pub fn route_present_idle(
        &self,
        transaction: TransactionId,
    ) -> Result<bool, XServerFrontendRouteError> {
        self.registry.route_present_idle(transaction)
    }
}
