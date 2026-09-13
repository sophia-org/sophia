#[cfg(unix)]
#[derive(Clone)]
struct XServerFrontendRouteRegistry {
    input_recovery: InputRecovery,
    runtime: Arc<std::sync::OnceLock<std::sync::Weak<Mutex<XAuthorityRuntime>>>>,
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
#[derive(Clone, Debug)]
struct XAuthorityEpochRoutedInput {
    control_epoch: u64,
    /// Zero when no coordinator is present, where publication plays no part.
    publication: u64,
    route: XAuthorityRoutedInput,
}


#[cfg(unix)]
#[derive(Clone)]
struct XServerFrontendClientRouteSenders {
    input: SyncSender<XAuthorityClientInputEvent>,
    control: SyncSender<X11RoutedControl>,
    protocol: SyncSender<XClientEvent>,
    admission: Option<ClientAdmissionContext>,
}

#[cfg(unix)]
struct XServerFrontendClientRouteChannels {
    input: Receiver<XAuthorityClientInputEvent>,
    control: Receiver<X11RoutedControl>,
    protocol: Receiver<XClientEvent>,
}

#[cfg(unix)]
struct XServerFrontendClientRouteRegistration {
    input_recovery: InputRecovery,
    client: XServerFrontendClientId,
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
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if clients.contains_key(&client) {
            return Err(XServerFrontendRouteError::DuplicateClient { client });
        }
        self.input_recovery.register(client)?;
        clients.insert(
            client,
            XServerFrontendClientRouteSenders {
                input: input_sender,
                control: control_sender,
                protocol: protocol_sender,
                admission,
            },
        );
        Ok((
            XServerFrontendClientRouteRegistration {
                input_recovery: self.input_recovery.clone(),
                client,
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

    fn select_present_input(
        &self,
        client: XServerFrontendClientId,
        event_id: XResourceId,
        window: XResourceId,
        mask: u32,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut subscriptions = self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let key = (client, event_id);
        if mask == 0 {
            subscriptions.remove(&key);
        } else {
            subscriptions.insert(
                key,
                XPresentSubscription {
                    event_id,
                    window,
                    mask,
                },
            );
        }
        Ok(())
    }

    fn present_configure_subscribers(
        &self,
        window: XResourceId,
    ) -> Result<Vec<(XServerFrontendClientId, XResourceId)>, XServerFrontendRouteError> {
        Ok(self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((subscription_client, _), subscription)| {
                (subscription.window == window && subscription.mask & 1 != 0)
                    .then_some((*subscription_client, subscription.event_id))
            })
            .collect())
    }

    fn queue_present(
        &self,
        transaction: TransactionId,
        client: XServerFrontendClientId,
        window: XResourceId,
        pixmap: XResourceId,
        serial: u32,
        idle_fence: Option<XResourceId>,
        suboptimal: bool,
    ) -> Result<(), XServerFrontendRouteError> {
        // The window decides which surface a present reaches; the presenting
        // client does not have to be the one that created it. A browser's GPU
        // process presents to a window its browser process owns, which X
        // permits -- requiring creator == presenter here silently locked out
        // every client that splits the two across connections.
        self.surfaces
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .find_map(|(surface, route)| (route.window == window).then_some(*surface))
            .ok_or(XServerFrontendRouteError::UnknownPresentWindow { window })?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut pending = self
            .pending_presentations
            .entries
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if pending.contains_key(&transaction) {
            return Err(XServerFrontendRouteError::DuplicatePresentation { transaction });
        }
        while pending
            .values()
            .filter(|presentation| presentation.client == client)
            .count()
            >= self.per_client_presentation_capacity.get()
        {
            let now = Instant::now();
            if now >= deadline {
                return Err(XServerFrontendRouteError::ClientQueueFull { client });
            }
            let (next, wait) = self
                .pending_presentations
                .capacity_changed
                .wait_timeout(pending, deadline.saturating_duration_since(now))
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            pending = next;
            if wait.timed_out()
                && pending
                    .values()
                    .filter(|presentation| presentation.client == client)
                    .count()
                    >= self.per_client_presentation_capacity.get()
            {
                return Err(XServerFrontendRouteError::ClientQueueFull { client });
            }
        }
        pending.insert(
            transaction,
            XPendingPresent {
                client,
                window,
                pixmap,
                serial,
                idle_fence,
                suboptimal,
                phases: crate::XPresentFeedbackPhases::default(),
                allocation_subject: None,
            },
        );
        Ok(())
    }

    fn route_present_complete(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
    ) -> Result<bool, XServerFrontendRouteError> {
        self.route_present_complete_with_layout(transaction, ust, msc, mode, None)
            .map(|outcome| outcome.routed)
    }

    fn route_present_complete_with_layout(
        &self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        mode: XPresentCompletionMode,
        comparison: Option<crate::XPresentLayoutComparison>,
    ) -> Result<crate::XPresentCompleteRouteOutcome, XServerFrontendRouteError> {
        // Reallocation advice is decided here from the current transaction and
        // preference state. A caller-supplied mode cannot replace that decision.
        let mode = match mode {
            XPresentCompletionMode::SuboptimalCopy => XPresentCompletionMode::Copy,
            mode => mode,
        };
        let (presentation, layout_comparison, mode) = {
            let authority = comparison
                .and_then(|_| self.runtime.get())
                .and_then(std::sync::Weak::upgrade);
            let mut runtime = authority.as_ref().and_then(|authority| authority.try_lock().ok());
            let mut pending = self
                .pending_presentations
                .entries
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some(presentation) = pending.get_mut(&transaction) else {
                return Ok(crate::XPresentCompleteRouteOutcome {
                    routed: false,
                    mode,
                    layout_comparison: None,
                });
            };
            let layout_comparison = comparison.map(|comparison| {
                let matched = mode == XPresentCompletionMode::Copy
                    && runtime.as_ref().is_some_and(|runtime| {
                        presentation.allocation_subject.is_some_and(|subject| {
                            runtime.compare_present_layout(subject, comparison)
                        })
                    });
                if matched {
                    crate::XPresentLayoutComparisonResult::Matched
                } else {
                    crate::XPresentLayoutComparisonResult::Rejected
                }
            });
            if !presentation.phases.observe_complete() {
                return Ok(crate::XPresentCompleteRouteOutcome {
                    routed: false,
                    mode,
                    layout_comparison: None,
                });
            }
            let advise = presentation.suboptimal
                && layout_comparison == Some(crate::XPresentLayoutComparisonResult::Matched)
                && comparison.zip(presentation.allocation_subject).is_some_and(
                    |(comparison, subject)| {
                        runtime.as_mut().is_some_and(|runtime| {
                            runtime.claim_present_reallocation(subject, comparison)
                        })
                    },
                );
            let mode = if advise {
                XPresentCompletionMode::SuboptimalCopy
            } else {
                mode
            };
            let presentation = *presentation;
            if presentation.phases.finished() {
                pending.remove(&transaction);
                self.pending_presentations.capacity_changed.notify_all();
            }
            (presentation, layout_comparison, mode)
        };
        // Every completion advances the presentation clock, and the clock is
        // what answers a NotifyMSC. Ripened deferrals flush here because this
        // is the only place the clock moves.
        *self
            .present_clock
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)? = Some((ust, msc));
        let ripe = {
            let mut pending = self
                .pending_msc_notifies
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let (ripe, waiting) = pending
                .drain(..)
                .partition::<Vec<_>, _>(|(_, _, target)| *target <= msc);
            *pending = waiting;
            ripe
        };
        for (window, serial, _) in ripe {
            self.route_present_msc_notify(window, serial, ust, msc)?;
        }
        let subscriptions = self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, _), subscription)| {
                (subscription.window == presentation.window
                    && subscription.mask & (1 << 1) != 0)
                    .then_some((*client, *subscription))
            })
            .collect::<Vec<_>>();
        if subscriptions.is_empty() {
            return Ok(crate::XPresentCompleteRouteOutcome {
                routed: false,
                mode,
                layout_comparison,
            });
        }
            // A Present subscription belongs to whoever took it, not to
            // whoever presents. A browser subscribes from its GPU process for a
            // window its browser process created, which X permits and Mesa
            // relies on: it blocks in xcb_wait_for_special_event until an idle
            // notify arrives, so an event withheld here is not an error the
            // client can see -- it is a client that never draws again.
        for (target, subscription) in subscriptions {
            self.route_protocol(
                target,
                XClientEvent::PresentCompleteNotify {
                    sequence: 0,
                    event_id: subscription.event_id,
                    window: presentation.window,
                    serial: presentation.serial,
                    ust,
                    msc,
                    kind: 0,
                    mode: mode as u8,
                },
            )?;
        }
        Ok(crate::XPresentCompleteRouteOutcome {
            routed: true,
            mode,
            layout_comparison,
        })
    }

    fn cancel_present(&self, transaction: TransactionId) -> Result<(), XServerFrontendRouteError> {
        self.pending_presentations
            .entries
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .remove(&transaction);
        self.pending_presentations.capacity_changed.notify_all();
        Ok(())
    }

    fn route_present_idle(
        &self,
        transaction: TransactionId,
    ) -> Result<bool, XServerFrontendRouteError> {
        let presentation = {
            let mut pending = self
                .pending_presentations
                .entries
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some(presentation) = pending.get_mut(&transaction) else {
                return Ok(false);
            };
            if !presentation.phases.observe_idle() {
                return Ok(false);
            }
            let presentation = *presentation;
            // Copy may release its source before display completion, while
            // Flip completes before its retained source becomes idle. Keep
            // the route until both independently owned phases arrive.
            if presentation.phases.finished() {
                pending.remove(&transaction);
                self.pending_presentations.capacity_changed.notify_all();
            }
            presentation
        };
        let subscriptions = self
            .present_subscriptions
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .iter()
            .filter_map(|((client, _), subscription)| {
                (subscription.window == presentation.window
                    && subscription.mask & (1 << 2) != 0)
                    .then_some((*client, *subscription))
            })
            .collect::<Vec<_>>();
        if subscriptions.is_empty() {
            return Ok(false);
        }
        for (target, subscription) in subscriptions {
            self.route_protocol(
                target,
                XClientEvent::PresentIdleNotify {
                    sequence: 0,
                    event_id: subscription.event_id,
                    window: presentation.window,
                    serial: presentation.serial,
                    pixmap: presentation.pixmap,
                    idle_fence: presentation.idle_fence,
                },
            )?;
        }
        Ok(true)
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
include!("registry/delivery.rs");
include!("registry/present_msc.rs");

include!("registry/present_layout.rs");
