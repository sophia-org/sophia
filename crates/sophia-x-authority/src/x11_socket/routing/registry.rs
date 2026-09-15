#[cfg(unix)]
#[derive(Clone)]
struct XServerFrontendRouteRegistry {
    input_recovery: InputRecovery,
    runtime: Arc<std::sync::OnceLock<std::sync::Weak<Mutex<XAuthorityRuntime>>>>,
    private_applied: Arc<std::sync::OnceLock<PrivateAppliedRegistryOwner>>,
    clients: Arc<Mutex<BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>>,
    surfaces: Arc<Mutex<BTreeMap<SurfaceId, XServerFrontendSurfaceRoute>>>,
    focused_surface: Arc<Mutex<Option<XServerFrontendSurfaceRoute>>>,
    window_parents:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), XResourceId>>>,
    core_event_subscriptions:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), u32>>>,
    randr_subscriptions: Arc<Mutex<BTreeMap<XServerFrontendClientId, (XResourceId, u16)>>>,
    /// Selections a client watches, keyed by the window it named when it
    /// subscribed. One client may watch several selections, and the same
    /// selection through different windows, so the window is part of the key
    /// rather than a value that the next subscription overwrites.
    xfixes_selection_subscriptions: XFixesSelectionSubscriptions,
    present_subscriptions:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), XPresentSubscription>>>,
    pending_presentations: Arc<XPendingPresentRegistry>,
    /// The last (ust, msc) any completion carried: the presentation clock a
    /// NotifyMSC answer reads. `None` until the first frame completes.
    present_clock: Arc<Mutex<Option<(u64, u64)>>>,
    /// MSC notifications whose target is still ahead of the clock, flushed as
    /// completions advance it. (window, serial, target_msc)
    pending_msc_notifies: Arc<Mutex<Vec<(XResourceId, u32, u64)>>>,
    pointer_state: Arc<Mutex<BTreeMap<(NamespaceId, SeatId), crate::XCorePointerMapper>>>,
    input_authority: Arc<Mutex<crate::XInputAuthorityState>>,
    frozen_input: Arc<Mutex<VecDeque<XDeferredRoutedInput>>>,
    xkb_config: crate::XkbRmlvoConfig,
    xkb_worker: XkbKeyboardWorker,
    /// The completion registry of the private instance that owns this
    /// registry, installed once at construction.
    ///
    /// Absent on the public path, which has no private instance to answer to.
    /// A client writer reads it here because this is what both routing sites
    /// and every client registration already reach.
    control_completion: Arc<std::sync::OnceLock<ControlCompletionRegistry>>,
    acknowledgement_sender: SyncSender<XAuthorityClientControlAck>,
    input_delivery_sender: Option<Sender<XAuthorityClientInputDelivery>>,
    metadata_candidate_sender: SyncSender<XAuthorityClientMetadataCandidate>,
    route_lease_update_sender: Option<SyncSender<XAuthorityRouteLeaseUpdate>>,
    explicit_pointer_grabs: Option<crate::XAuthorityExplicitPointerGrabClient>,
    input_control_epoch: Arc<AtomicU64>,
    per_client_input_capacity: NonZeroUsize,
    per_client_control_capacity: NonZeroUsize,
    per_client_protocol_capacity: NonZeroUsize,
    per_client_presentation_capacity: NonZeroUsize,
    source_payload_sender: SyncSender<crate::ClipboardSourcePayload>,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XServerFrontendSurfaceRoute {
    client: XServerFrontendClientId,
    namespace: NamespaceId,
    admission: Option<ClientAdmissionContext>,
    window: XResourceId,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct XPresentSubscription {
    event_id: XResourceId,
    window: XResourceId,
    mask: u32,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct XPendingPresent {
    client: XServerFrontendClientId,
    window: XResourceId,
    pixmap: XResourceId,
    serial: u32,
    idle_fence: Option<XResourceId>,
    // Advice is permitted only when opted in without forced copying.
    suboptimal: bool,
    phases: crate::XPresentFeedbackPhases,
    allocation_subject: Option<crate::runtime::XPresentAllocationSubject>,
}

#[cfg(unix)]
#[derive(Default)]
struct XPendingPresentRegistry {
    entries: Mutex<BTreeMap<TransactionId, XPendingPresent>>,
    capacity_changed: Condvar,
}

#[cfg(unix)]
#[derive(Clone, Debug)]
struct XDeferredRoutedInput {
    client: XServerFrontendClientId,
    control_epoch: u64,
    /// Kept alongside the epoch so a thaw validates the stamp the work was
    /// given, rather than half of it. Zero where no coordinator is present.
    publication: u64,
    route: XAuthorityRoutedInput,
}

#[cfg(unix)]
#[derive(Debug)]
struct XAuthorityEpochRoutedInput {
    control_epoch: u64,
    /// Zero when no coordinator is present, where publication plays no part.
    publication: u64,
    route: XAuthorityRoutedInput,
    /// The request reserved for this work, when it was reserved before being
    /// published.
    ///
    /// Owned rather than named. Travelling as a value is what makes the two
    /// ends of the window the only reachable ones: the work is accepted and
    /// the reservation goes on with it, or it is refused and dropping what
    /// comes back disposes the cell. `None` on the ordinary path, which
    /// reserves nothing.
    ///
    /// Not `Clone` for the same reason -- two copies of custody would let one
    /// request be executed twice, or disposed while the other still expects to
    /// publish for it.
    reservation: Option<PrivateReservation>,
}


#[cfg(unix)]
#[derive(Clone)]
struct XServerFrontendClientRouteSenders {
    connection_state: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    input: SyncSender<XAuthorityClientInputEvent>,
    control: SyncSender<X11RoutedControl>,
    protocol: SyncSender<XClientEvent>,
    admission: Option<ClientAdmissionContext>,
    /// Where ordered deliveries go, kept apart from the ordinary input queue.
    ///
    /// A separate queue because the two carry different things: an ordinary
    /// route is resolved by the writer as it writes, and an ordered delivery
    /// is resolved once and written as it stands. Sharing one queue would put
    /// them behind each other and give the writer two shapes to tell apart on
    /// a path where it must not be deciding anything.
    ///
    /// Bounded by the same per-client input capacity, as an explicit queue
    /// policy for this client. That bound is about this queue's length and
    /// says nothing about retained output or holds across turns, which are
    /// reserved before acceptance and not by anything the queue does.
    #[allow(dead_code)]
    ordered: SyncSender<XAuthorityOrderedDelivery>,
    /// Set when this client's control writer stops, however it stopped.
    ///
    /// Lives with the route senders rather than in a ledger of its own, so it
    /// is bounded by the clients that exist and goes when the registration
    /// goes. A separate ledger grew with every client an instance ever served
    /// and had to evict, and an evicted entry silently stopped protecting a
    /// client whose writer was gone.
    control_writer_gone: Arc<AtomicBool>,
}

#[cfg(unix)]
struct XServerFrontendClientRouteChannels {
    input: Receiver<XAuthorityClientInputEvent>,
    control: Receiver<X11RoutedControl>,
    protocol: Receiver<XClientEvent>,
    #[allow(dead_code)]
    ordered: XAuthorityOrderedReceiver,
}

/// One connection's ordered receiver, minted with the registration that owns
/// it.
///
/// MINTED HERE AND NOWHERE ELSE, in the same expression that makes the channel
/// and beside the registration that gets the other end. A serving owner that
/// accepted a bare receiver could be handed one connection's registration and
/// another's queue, and nothing about either value would say so; a receiver
/// that carries the registration cell it was made with can be asked.
///
/// There is deliberately no constructor taking a receiver and a witness: one
/// would let a caller assert exactly the association this exists to establish.
#[cfg(unix)]
struct XAuthorityOrderedReceiver {
    receiver: Receiver<XAuthorityOrderedDelivery>,
    /// The connection-state cell this registration is, by pointer.
    registration: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
impl XAuthorityOrderedReceiver {
    /// Whether this receiver was minted by exactly this registration.
    fn minted_by(&self, registration: &XServerFrontendClientRouteRegistration) -> bool {
        Arc::ptr_eq(&self.registration, &registration.connection_state)
    }

    /// Give up the receiver itself, once its provenance has been established.
    fn into_receiver(self) -> Receiver<XAuthorityOrderedDelivery> {
        self.receiver
    }
}

/// Reading a connection's queued output does not need its provenance, so the
/// ordinary receiver operations are available directly. Taking ownership of
/// the receiver does need it, and that goes through `into_receiver`.
#[cfg(unix)]
impl std::ops::Deref for XAuthorityOrderedReceiver {
    type Target = Receiver<XAuthorityOrderedDelivery>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

#[cfg(unix)]
struct XServerFrontendClientRouteRegistration {
    lifecycle: Mutex<Option<PrivateConnectionLifecycle>>,
    connection_state: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    input_recovery: InputRecovery,
    client: XServerFrontendClientId,
    /// The completion registry this client's control is answered through,
    /// when the instance is private. Held so that losing the registration is
    /// an edge this client's control records are told about, rather than one
    /// that quietly leaves them waiting for a writer that has gone.
    control_completion: Arc<std::sync::OnceLock<ControlCompletionRegistry>>,
    clients: Arc<Mutex<BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>>,
    surfaces: Arc<Mutex<BTreeMap<SurfaceId, XServerFrontendSurfaceRoute>>>,
    focused_surface: Arc<Mutex<Option<XServerFrontendSurfaceRoute>>>,
    window_parents:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), XResourceId>>>,
    core_event_subscriptions:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), u32>>>,
    randr_subscriptions: Arc<Mutex<BTreeMap<XServerFrontendClientId, (XResourceId, u16)>>>,
    /// Selections a client watches, keyed by the window it named when it
    /// subscribed. One client may watch several selections, and the same
    /// selection through different windows, so the window is part of the key
    /// rather than a value that the next subscription overwrites.
    xfixes_selection_subscriptions: XFixesSelectionSubscriptions,
    present_subscriptions:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), XPresentSubscription>>>,
    pending_presentations: Arc<XPendingPresentRegistry>,
    frozen_input: Arc<Mutex<VecDeque<XDeferredRoutedInput>>>,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum XkbWorkerCommand {
    Key {
        seat: SeatId,
        keycode: u32,
        pressed: bool,
    },
    Modifiers {
        seat: SeatId,
    },
}

#[cfg(unix)]
type XkbKeyboardReply = Option<(u8, u16, u16)>;

#[cfg(unix)]
type SharedXkbKeyboardReplies = Arc<Mutex<Receiver<XkbKeyboardReply>>>;

#[cfg(unix)]
#[derive(Clone)]
struct XkbKeyboardWorker {
    commands: SyncSender<XkbWorkerCommand>,
    replies: SharedXkbKeyboardReplies,
}

#[cfg(unix)]
/// How long the routing thread waits for one keyboard translation.
///
/// Generous relative to the work, which is a table lookup, and short relative
/// to a human noticing: the point is only that the wait ends.
const XKB_WORKER_REPLY_DEADLINE: std::time::Duration = std::time::Duration::from_millis(250);

impl XkbKeyboardWorker {
    fn spawn(config: crate::XkbRmlvoConfig) -> Self {
        let (commands, command_receiver) = sync_channel(64);
        let (reply_sender, replies) = sync_channel(64);
        std::thread::Builder::new()
            .name("sophia-xkb-authority".to_owned())
            .spawn(move || {
                let mut seats = BTreeMap::<SeatId, crate::XkbKeyboardState>::new();
                while let Ok(command) = command_receiver.recv() {
                    let seat_id = match command {
                        XkbWorkerCommand::Key { seat, .. }
                        | XkbWorkerCommand::Modifiers { seat } => seat,
                    };
                    // A keymap that no longer compiles is a real fault, but
                    // panicking here would take the thread down and leave every
                    // later request looking like a poisoned lock. Answering
                    // `None` reports the failure through the same channel as
                    // any other unmappable key.
                    let state = match seats.entry(seat_id) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            match crate::XkbKeyboardState::new(&config) {
                                Ok(state) => entry.insert(state),
                                Err(_) => {
                                    if reply_sender.send(None).is_err() {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }
                    };
                    let reply = match command {
                        XkbWorkerCommand::Key {
                            keycode, pressed, ..
                        } => state.map_evdev_key(keycode, pressed).map(|(keycode, before)| {
                            (keycode, before, state.modifier_mask())
                        }),
                        XkbWorkerCommand::Modifiers { .. } => {
                            let modifiers = state.modifier_mask();
                            Some((0, modifiers, modifiers))
                        }
                    };
                    if reply_sender.send(reply).is_err() {
                        break;
                    }
                }
            })
            .expect("Sophia XKB authority worker must start");
        Self {
            commands,
            replies: Arc::new(Mutex::new(replies)),
        }
    }

    fn request(
        &self,
        command: XkbWorkerCommand,
    ) -> Result<Option<(u8, u16, u16)>, XServerFrontendRouteError> {
        self.commands.try_send(command).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(_) => {
                XServerFrontendRouteError::XkbWorkerSaturated
            }
            std::sync::mpsc::TrySendError::Disconnected(_) => {
                XServerFrontendRouteError::XkbWorkerUnavailable
            }
        })?;
        // Bounded, because this runs on the routing thread: a worker that never
        // answers would otherwise stall every client's input, and a stalled
        // keyboard is worse than an unmapped key.
        self.replies
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .recv_timeout(XKB_WORKER_REPLY_DEADLINE)
            .map_err(|_| XServerFrontendRouteError::XkbWorkerUnavailable)
    }
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    #[cfg_attr(not(test), allow(dead_code))]
    fn register_client(
        &self,
        client: XServerFrontendClientId,
    ) -> Result<
        (
            XServerFrontendClientRouteRegistration,
            XServerFrontendClientRouteChannels,
        ),
        XServerFrontendRouteError,
    > {
        self.register_client_with_admission(client, None)
    }

    fn register_client_with_admission(
        &self,
        client: XServerFrontendClientId,
        admission: Option<ClientAdmissionContext>,
    ) -> Result<
        (
            XServerFrontendClientRouteRegistration,
            XServerFrontendClientRouteChannels,
        ),
        XServerFrontendRouteError,
    > {
        let (input_sender, input) = sync_channel(self.per_client_input_capacity.get());
        let (control_sender, control) = sync_channel(self.per_client_control_capacity.get());
        let (protocol_sender, protocol) =
            sync_channel(self.per_client_protocol_capacity.get());
        let (ordered_sender, ordered) = sync_channel(self.per_client_input_capacity.get());
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if clients.contains_key(&client) {
            return Err(XServerFrontendRouteError::DuplicateClient { client });
        }
        self.input_recovery.register(client)?;
        let connection_state = Arc::new(std::sync::OnceLock::new());
        // A writer for this client exists or is about to: registration comes
        // before the spawn, and control accepted in that window is not control
        // with nowhere to go. The writer stopping is what clears it.
        if let Some(completion) = self.control_completion.get() {
            completion.expect_writer(client);
        }
        clients.insert(
            client,
            XServerFrontendClientRouteSenders {
                connection_state: connection_state.clone(),
                input: input_sender,
                control: control_sender,
                protocol: protocol_sender,
                admission,
                ordered: ordered_sender,
                control_writer_gone: Arc::new(AtomicBool::new(false)),
            },
        );
        // Taken before the registration consumes it: the receiver and the
        // registration are minted from the one cell, which is what makes the
        // question "did this registration make this receiver" answerable.
        let ordered_witness = connection_state.clone();
        Ok((
            XServerFrontendClientRouteRegistration {
                lifecycle: Mutex::new(None),
                connection_state,
                input_recovery: self.input_recovery.clone(),
                client,
                control_completion: self.control_completion.clone(),
                clients: self.clients.clone(),
                surfaces: self.surfaces.clone(),
                focused_surface: self.focused_surface.clone(),
                window_parents: self.window_parents.clone(),
                core_event_subscriptions: self.core_event_subscriptions.clone(),
                randr_subscriptions: self.randr_subscriptions.clone(),
                xfixes_selection_subscriptions: self.xfixes_selection_subscriptions.clone(),
                present_subscriptions: self.present_subscriptions.clone(),
                pending_presentations: self.pending_presentations.clone(),
                frozen_input: self.frozen_input.clone(),
            },
            XServerFrontendClientRouteChannels {
                input,
                control,
                protocol,
                // Minted with the cell this registration and its client-table
                // entry both hold, so what it came from can be asked later.
                ordered: XAuthorityOrderedReceiver {
                    receiver: ordered,
                    registration: ordered_witness,
                },
            },
        ))
    }

    fn route_input(
        &self,
        route: XAuthorityClientInputEvent,
    ) -> Result<(), XServerFrontendRouteError> {
        let sender = self.client_senders(route.client)?.input;
        if !self.input_recovery.bind(route.delivery, route.client)? {
            return Ok(());
        }
        match self.route_to_client(route.client, sender, route) {
            Err(error @ XServerFrontendRouteError::ClientQueueFull { client }) => {
                // A client that stops draining its private input queue has
                // failed as an endpoint. Remove every sender for that client
                // so later routes cannot repeatedly pressure the shared
                // broker and its worker observes channel disconnection.
                self.input_recovery.disconnect_rejecting(client, XAuthorityInputDeliveryOutcome::ClientDisconnected, route.delivery)?;
                self.clients.lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?.remove(&client);
                Err(error)
            }
            result => result,
        }
    }

    fn register_surface(
        &self,
        client: XServerFrontendClientId,
        namespace: NamespaceId,
        surface: SurfaceId,
        window: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        let admission = self.client_senders(client)?.admission;
        let mut surfaces = self
            .surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if surfaces.contains_key(&surface) {
            return Err(XServerFrontendRouteError::DuplicateSurface { surface });
        }
        surfaces.insert(
            surface,
            XServerFrontendSurfaceRoute {
                client,
                namespace,
                admission,
                window,
            },
        );
        Ok(())
    }

    fn remove_surface(
        &self,
        surface: SurfaceId,
    ) -> Result<bool, XServerFrontendRouteError> {
        Ok(self
            .surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .remove(&surface)
            .is_some())
    }

    fn surface_route_observation(
        &self,
        surface: SurfaceId,
    ) -> Result<Option<XAuthoritySurfaceRouteObservation>, XServerFrontendRouteError> {
        Ok(self
            .surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&surface)
            .map(|route| XAuthoritySurfaceRouteObservation {
                surface,
                client: route.client,
                admission: route.admission,
            }))
    }

    fn emit_metadata_candidate(
        &self,
        candidate: sophia_protocol::ReducedMetadataCandidate,
    ) -> Result<(), XServerFrontendRouteError> {
        let client = self
            .surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&candidate.surface)
            .map(|route| route.client)
            .ok_or(XServerFrontendRouteError::UnknownSurface {
                surface: candidate.surface,
            })?;
        match self
            .metadata_candidate_sender
            .try_send(XAuthorityClientMetadataCandidate { client, candidate })
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(XServerFrontendRouteError::MetadataQueueFull),
            Err(TrySendError::Disconnected(_)) => {
                Err(XServerFrontendRouteError::MetadataQueueDisconnected)
            }
        }
    }

    fn register_window_parent(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
        parent: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .insert((client, window), parent);
        Ok(())
    }

    fn remove_window_parent(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        self.window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .remove(&(client, window));
        Ok(())
    }

    fn window_ancestry(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
    ) -> Result<Vec<XResourceId>, XServerFrontendRouteError> {
        let parents = self
            .window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mut ancestry = vec![window];
        let mut candidate = window;
        for _ in 0..64 {
            let Some(parent) = parents.get(&(client, candidate)).copied() else {
                break;
            };
            if ancestry.contains(&parent) {
                break;
            }
            ancestry.push(parent);
            candidate = parent;
        }
        Ok(ancestry)
    }

    fn window_parent(
        &self,
        window: XResourceId,
    ) -> Result<Option<XResourceId>, XServerFrontendRouteError> {
        Ok(self
            .window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .find_map(|((_, candidate), parent)| (*candidate == window).then_some(*parent)))
    }

    fn select_randr_input(
        &self,
        client: XServerFrontendClientId,
        window: XResourceId,
        mask: u16,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut subscriptions = self
            .randr_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if mask == 0 {
            subscriptions.remove(&client);
        } else {
            subscriptions.insert(client, (window, mask));
        }
        Ok(())
    }

    fn broadcast_randr_update(
        &self,
        snapshot: &sophia_protocol::OutputTopologySnapshot,
    ) -> Result<usize, XServerFrontendRouteError> {
        let size = snapshot
            .root_size()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let width =
            u16::try_from(size.width).map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let height =
            u16::try_from(size.height).map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let mm_width = u16::try_from((i64::from(size.width) * 254 + 480) / 960)
            .unwrap_or(u16::MAX)
            .max(1);
        let mm_height = u16::try_from((i64::from(size.height) * 254 + 480) / 960)
            .unwrap_or(u16::MAX)
            .max(1);
        let timestamp = u32::try_from(snapshot.generation)
            .unwrap_or(u32::MAX)
            .max(1);
        let subscriptions = self
            .randr_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .clone();
        let mut delivered = 0usize;
        for (client, (window, mask)) in subscriptions {
            if mask & 1 != 0 {
                self.route_protocol(
                    client,
                    XClientEvent::RandrScreenChange {
                        sequence: 0,
                        timestamp,
                        config_timestamp: timestamp,
                        root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                        request_window: window,
                        width,
                        height,
                        mm_width,
                        mm_height,
                    },
                )?;
                delivered = delivered.saturating_add(1);
            }
            for output in &snapshot.outputs {
                let identity = crate::dispatch::stable_randr_identity(output.output.raw());
                let crtc = 0x1000_0000 | identity;
                let output_id = 0x2000_0000 | identity;
                let mode = crate::dispatch::stable_randr_mode_id(
                    output.logical.width,
                    output.logical.height,
                    output.refresh_millihz,
                );
                if mask & (1 << 1) != 0 {
                    self.route_protocol(
                        client,
                        XClientEvent::RandrCrtcChange {
                            sequence: 0,
                            timestamp,
                            window,
                            crtc,
                            mode,
                            x: i16::try_from(output.logical.x).unwrap_or(i16::MAX),
                            y: i16::try_from(output.logical.y).unwrap_or(i16::MAX),
                            width: u16::try_from(output.logical.width).unwrap_or(u16::MAX),
                            height: u16::try_from(output.logical.height).unwrap_or(u16::MAX),
                        },
                    )?;
                    delivered = delivered.saturating_add(1);
                }
                if mask & (1 << 2) != 0 {
                    self.route_protocol(
                        client,
                        XClientEvent::RandrOutputChange {
                            sequence: 0,
                            timestamp,
                            window,
                            output: output_id,
                            crtc,
                            mode,
                        },
                    )?;
                    delivered = delivered.saturating_add(1);
                }
            }
            if mask & (1 << 6) != 0 {
                self.route_protocol(
                    client,
                    XClientEvent::RandrResourceChange {
                        sequence: 0,
                        timestamp,
                        window,
                    },
                )?;
                delivered = delivered.saturating_add(1);
            }
        }
        Ok(delivered)
    }

}
include!("registry/present.rs");
include!("registry/delivery.rs");
include!("registry/present_msc.rs");

include!("registry/present_layout.rs");
