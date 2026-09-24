#[cfg(unix)]
#[derive(Clone)]
struct XServerFrontendRouteRegistry {
    input_recovery: InputRecovery,
    runtime: Arc<std::sync::OnceLock<std::sync::Weak<Mutex<XAuthorityRuntime>>>>,
    private_applied: Arc<std::sync::OnceLock<PrivateAppliedRegistryOwner>>,
    /// Where a connection's ordered continuation will go if it ever needs one.
    ///
    /// Set for a private instance, so registering can take a connection's place
    /// BEFORE it publishes that connection's sender. Unset elsewhere, where
    /// there is no ordered output to hand over.
    continuation_owner: Arc<std::sync::OnceLock<PrivateSettlementRef>>,
    /// Who keeps this instance's connections' evidence custodies.
    ///
    /// Set for a private instance built over a service owner, so registering
    /// can reserve a connection's external keeper BEFORE its row is
    /// published. Held weakly, like the store above and for the same reason.
    custody_keeper: Arc<std::sync::OnceLock<PrivateCustodyKeeper>>,
    /// Who holds which client number in this registry's namespace.
    ///
    /// A NUMBER IS AN INDEX, NOT AN IDENTITY. This is what keeps one
    /// connection's ending from acting by number on the connection that took
    /// the number next.
    occupancy: PrivateNumberOccupancy,
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
    /// Controls a full per-client channel would not take, kept per client in
    /// the order they were routed; the private path's, see
    /// `registry/control_backlog.rs` for its bound and its lock order.
    control_backlog: Arc<Mutex<BTreeMap<XServerFrontendClientId, VecDeque<XDeferredRoutedControl>>>>,
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
    /// Where this route's internal-processing outcome is reported, when a
    /// submitter asked to be told. Armed by the submitter, carried with the
    /// work, answered by the receiver once the registry has taken the effect
    /// -- after, never on acceptance, which is the ordering FakeInput owes its
    /// next request. `None` for every route the session's own input phase
    /// sends, which is answered to nobody.
    completion: Option<PrivateBarrierTicket>,
}


#[cfg(unix)]
#[derive(Clone)]
struct XServerFrontendClientRouteSenders {
    connection_state: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    input: SyncSender<XAuthorityClientInputEvent>,
    control: SyncSender<X11RoutedControl>,
    protocol: X11ProtocolSender,
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
    ordered: PrivateGatedOrderedSender,
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
    protocol: X11ProtocolReceiver,
    #[allow(dead_code)]
    ordered: XAuthorityOrderedReceiver,
}

#[cfg(unix)]
struct XServerFrontendClientRouteRegistration {
    /// This connection's way back to the evidence custody reserved for it.
    ///
    /// RESERVED BEFORE THIS ROW WAS PUBLISHED and kept by the service owner,
    /// not here: this is a capability that names one custody, and asking it
    /// twice names the same home. `None` where no service owner was installed,
    /// which is not a claim that evidence is kept somewhere else.
    ///
    /// NOT RELEASED BY THIS REGISTRATION GOING. A connection ending is not its
    /// evidence being disposed of, and an entry that vanished with the
    /// registration would make a service exit look like a settlement.
    ordered_custody: Option<PrivateRegisteredCustody>,
    /// What this connection's destruction is responsible for.
    ///
    /// SHARED WITH ITS KEEPER, and reached by everything that used to read
    /// these as fields of this handle. The responsibility outlives the handle;
    /// what still triggers it is this handle's own `Drop`, unchanged.
    cleanup: Arc<PrivateCleanupRecord>,
}

/// A registration reads as the connection it is a handle to.
///
/// ITS STATE MOVED, NOT ITS MEANING. Everything that asked this handle for its
/// home, its gate, its client or its routing tables is asking the connection,
/// and that is where those now live.
#[cfg(unix)]
impl std::ops::Deref for XServerFrontendClientRouteRegistration {
    type Target = PrivateCleanupRecord;

    fn deref(&self) -> &Self::Target {
        &self.cleanup
    }
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

    /// Give this registry the store its connections take their places from,
    /// against the declared client limit.
    ///
    /// BEFORE ANY ROW CAN BE PUBLISHED. A registry that admitted connections
    /// first and was given a store afterwards would have exposed queues whose
    /// accepted work has nowhere to go, and no later installation can go back
    /// and reserve for them. Once installed it does not change: the places
    /// held under it belong to it.
    ///
    /// The bound is declared, not imposed -- a durable store that already
    /// carries places from an earlier instance keeps the bound those were
    /// taken against. Reports the bound in force, or nothing if the store
    /// cannot be read or an owner is already installed.
    pub(crate) fn install_continuation_owner(
        &self,
        owner: &PrivateSettlementOwner,
        connections: NonZeroUsize,
    ) -> Option<usize> {
        let bound = owner.declare_connection_bound(connections)?;
        // Held weakly. The store retains inventories that hold this registry,
        // so owning it back would close a ring neither end could leave.
        self.continuation_owner.set(owner.settlement_ref()).ok()?;
        Some(bound)
    }

    /// Give this registry the owner that keeps its connections' evidence.
    ///
    /// BEFORE ANY ROW CAN BE PUBLISHED, for the same reason as the store: a
    /// connection exposed first would be one whose external keeper was decided
    /// after it was already admitted.
    ///
    /// ONCE, AND NOT AGAIN. A registry that could be given a second keeper
    /// could put one connection's evidence in one owner's inventory and the
    /// next connection's in another, and nothing afterwards could say which
    /// owner was responsible for what.
    pub(crate) fn install_custody_keeper(&self, keeper: PrivateCustodyKeeper) -> bool {
        self.custody_keeper.set(keeper).is_ok()
    }

    /// Whether this registry's connections' evidence is kept by that owner.
    ///
    /// BY INVENTORY IDENTITY, not by store or by bound. Two owners over one
    /// store are two separate inventories, and a service told they were
    /// interchangeable would reserve into one and look in the other.
    /// Whether this lease is on the owner that keeps this registry's
    /// connections' evidence.
    pub(crate) fn leased_by(&self, service: &PrivateServiceLease<'_>) -> bool {
        self.custody_keeper
            .get()
            .is_some_and(|keeper| service.keeps_for(keeper))
    }

    pub(crate) fn custody_keeper_is(&self, owner: &PrivateServiceOwner) -> bool {
        self.custody_keeper
            .get()
            .is_some_and(|keeper| keeper.kept_by(owner))
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
        // MINTED WITH THE QUEUE, so there is no moment at which this
        // registration's queue is reachable through a sender that no close can
        // serialize with. Bound to this registration: a replacement for the
        // same client mints its own, and closing this endpoint cannot reach it.
        let gate = Arc::new(PrivateHandoverGate::open());
        // Minted with the queue as well, and for the same reason: there is no
        // moment at which this connection has a sender that nothing counts.
        let wake = Arc::new(PrivateOrderedWake::for_first_sender());
        let ordered_sender = PrivateGatedOrderedSender {
            sender: Some(ordered_sender),
            gate: gate.clone(),
            wake: wake.clone(),
        };
        // THE PLACE IS TAKEN BEFORE THE ROW IS PUBLISHED, and before the
        // client table is held. The senders above already exist; what
        // publication does is make one reachable, and from that moment a
        // capsule can be accepted into that queue -- so a connection whose
        // accepted work would have nowhere to go must not be exposed at all.
        // Taking the settlement
        // store beneath the client table would reverse the order the retained
        // drive already uses -- it holds settlement and then takes clients to
        // release a lease. Two orders, one deadlock.
        let mut continuation = match self.continuation_owner.get() {
            // A store that has gone is not a store with room. This registry
            // does not own it, so the connection is refused rather than
            // exposed with nowhere to hand over to.
            Some(store) => {
                let owner = store
                    .owner()
                    .ok_or(XServerFrontendRouteError::ContinuationUnavailable { client })?;
                Some(
                    owner
                        .reserve_ordered_continuation()
                        .map_err(|_| XServerFrontendRouteError::ContinuationUnavailable {
                            client,
                        })?,
                )
            }
            None => None,
        };
        // Both halves are minted from the one cell, which is what makes the
        // question "did this registration make this receiver" answerable.
        let connection_state: Arc<std::sync::OnceLock<PrivateAppliedClientState>> =
            Arc::new(std::sync::OnceLock::new());
        let senders = XServerFrontendClientRouteSenders {
            connection_state: connection_state.clone(),
            input: input_sender,
            control: control_sender,
            protocol: X11ProtocolSender(protocol_sender),
            admission,
            ordered: ordered_sender,
            control_writer_gone: Arc::new(AtomicBool::new(false)),
        };
        // THE HOME THE RESERVATION MADE, when there is one. A reservation
        // allocates the home along with the place, so the registration and the
        // place hold the same one and nothing has to be moved between them
        // later. Without a store there is no place, so this connection gets a
        // home of its own that goes when it does.
        let home = match continuation.as_ref().and_then(PrivateOrderedContinuationSlot::home) {
            Some(home) => home,
            None => Arc::new(PrivateOrderedHome::empty()),
        };
        home.origin.set(Arc::downgrade(&self.clients))
            .map_err(|_| XServerFrontendRouteError::ContinuationUnavailable { client })?;
        // AND THIS CONNECTION'S EXTERNAL KEEPER, on the same reservation and
        // before the same boundary. The place and the maintenance destination
        // above are storage inside the store; this is the custody outside it
        // that will hold whatever this connection's worker leaves. All three
        // are set aside before the row goes in, because after that this
        // connection has work that can be accepted and nowhere honest to put
        // the evidence of how it ended.
        //
        // NO KEEPER INSTALLED MEANS NO CUSTODY, which is the public frontend's
        // shape: there is no private service owner, so there is nothing to
        // reserve from and nothing is claimed about one.
        // THIS CONNECTION'S TEARDOWN RESPONSIBILITY, prepared before its row
        // is published and before any accepted work can depend on it. The
        // place moves into it here: one home for the reservation, so a later
        // conversion or a late lifecycle attachment reaches the same state
        // this connection's destruction will act on.
        let mut cleanup = Some(Arc::new(PrivateCleanupRecord::prepared_for(
            self,
            client,
            continuation.take(),
            home,
            Arc::clone(&gate),
            connection_state.clone(),
        )));
        let mut custody = match (self.custody_keeper.get(), cleanup.as_ref()) {
            (Some(keeper), Some(record)) if record.holds_a_place() => {
                // THE GATE THIS CONNECTION'S QUEUE WAS MINTED WITH, handed to
                // its custody here -- before the row goes in, on the same
                // reservation. The sender above already has it, and this is
                // what makes the source's gate the same gate rather than one
                // that merely matches.
                match keeper.reserve_for(
                    &record.maintenance_identity().expect("it holds a place"),
                    Arc::clone(&gate),
                    Arc::clone(record),
                ) {
                    PrivateCustodyReserved::Reserved(registered) => Some(registered),
                    // REFUSED BEFORE EXPOSURE, and the place above goes back
                    // with it: this connection is not admitted at all rather
                    // than admitted without a keeper. A saturated inventory is
                    // a limitation to report, and nothing here retires another
                    // connection's evidence to make room.
                    PrivateCustodyReserved::AlreadyKept
                    | PrivateCustodyReserved::Saturated
                    | PrivateCustodyReserved::Foreign
                    | PrivateCustodyReserved::Unreadable => {
                        // THIS ATTEMPT'S OWN RESERVATIONS, AND ONLY THOSE. The
                        // record was never published, so nothing outside this
                        // call has it and disposing of the place is all it
                        // owes.
                        if let Some(unexposed) = cleanup.take() {
                            unexposed.relinquish_unexposed();
                        }
                        return Err(XServerFrontendRouteError::EvidenceCustodyUnavailable {
                            client,
                        });
                    }
                }
            }
            _ => None,
        };
        let published = self.publish_registered_client(
            client,
            senders,
            &mut cleanup,
            &mut custody,
        );
        // The client table is released here, before the place is disposed of.
        //
        // A RESERVATION THAT PUBLISHED NOTHING IS NOT RETAINED WORK. Every
        // refusal above happened with no row and no reachable queue, so no
        // capsule could have been accepted for this connection and the place
        // owes nothing. Publication is what takes it: on success the
        // registration holds it, and this is None.
        if let Some(unexposed) = cleanup.take() {
            unexposed.relinquish_unexposed();
        }
        // THE SAME FOR THE KEEPER'S ENTRY, and only for an attempt that was
        // never exposed. Publication takes it exactly as it takes the place,
        // so this is None on success. A refusal here -- a duplicate client
        // arriving after preparation, say -- gives back the entry THIS attempt
        // reserved and nothing else: the live sibling this attempt collided
        // with keeps its own custody, its own home and its own accounting.
        if let Some(unexposed) = custody.take() {
            unexposed.release_unexposed();
        }
        let registration = published?;
        Ok((
            registration,
            XServerFrontendClientRouteChannels {
                input,
                control,
                protocol: X11ProtocolReceiver::Tracked { receiver: protocol, registration: connection_state.clone() },
                ordered: XAuthorityOrderedReceiver {
                    receiver: ordered,
                    registration: connection_state,
                    capacity: self.per_client_input_capacity.get(),
                    wake,
                },
            },
        ))
    }

    /// Insert the row and mint the registration that owns it.
    ///
    /// Separated so the client table is held for exactly this, and released
    /// before the caller disposes of anything held elsewhere.
    ///
    /// The place is taken out of `continuation` only once the row is in. A
    /// caller that gets an error back still owns it, and one that gets a
    /// registration back does not: taken means published.
    fn publish_registered_client(
        &self,
        client: XServerFrontendClientId,
        senders: XServerFrontendClientRouteSenders,
        cleanup: &mut Option<Arc<PrivateCleanupRecord>>,
        custody: &mut Option<PrivateRegisteredCustody>,
    ) -> Result<XServerFrontendClientRouteRegistration, XServerFrontendRouteError> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if clients.contains_key(&client) {
            return Err(XServerFrontendRouteError::DuplicateClient { client });
        }
        // THE NUMBER ITSELF, TAKEN BEFORE ANYTHING IS ESTABLISHED UNDER IT.
        // The recovery ledger below and the expected writer after it are both
        // keyed by this number, so a claim placed only before the row would
        // let a predecessor's unfinished ending reach state a successor had
        // already reset.
        //
        // NO ROW IS NOT NO OCCUPANT. A row is removed when a send finds its
        // endpoint gone and when a client stops draining its queue, so the
        // check above says nothing about whether the connection that had this
        // number has finished with it.
        //
        // LOCK ORDER: the client table, then this. Nothing takes the client
        // table while holding the occupancy record.
        let record = cleanup.as_ref().expect("a publication has its record");
        let number = match self.occupancy.claim(client, &record.connection_state) {
            Ok(right) => right,
            Err(PrivateNumberRefusal::Excluded) => {
                return Err(XServerFrontendRouteError::ClientNumberExcluded { client });
            }
            // NOT THE SAME REFUSAL. Excluded says an incumbent owns this
            // number; an unreadable record has established no such thing, and
            // saying it had would be reporting a fact nobody checked. Startup
            // is refused either way.
            Err(PrivateNumberRefusal::Unreadable) => {
                return Err(XServerFrontendRouteError::RegistryPoisoned);
            }
        };
        if let Err(refusal) = self
            .input_recovery
            .register(client, Some(&record.connection_state))
        {
            // Nothing was established under it, so it goes straight back.
            number.relinquish_unpublished();
            return Err(refusal);
        }
        // KEPT WITH THE RESPONSIBILITY, which is what ends it. A right held by
        // the frame that published would be one a lost row or an ended view
        // could hand to somebody else.
        record
            .number
            .set(number)
            .unwrap_or_else(|_| panic!("a record is published once"));
        // A writer for this client exists or is about to: registration comes
        // before the spawn, and control accepted in that window is not control
        // with nowhere to go. The writer stopping is what clears it.
        if let Some(completion) = self.control_completion.get() {
            completion.expect_writer(client);
        }
        clients.insert(client, senders);
        Ok(XServerFrontendClientRouteRegistration {
            // TAKEN WITH THE ROW, like the place. What this registration gets
            // is a capability naming the one custody reserved for it -- not a
            // licence to make a publication home later, and not a handle that
            // keeps one alive. Asking twice names the same home.
            ordered_custody: custody.take(),
            // AND THE RESPONSIBILITY THIS HANDLE CARRIES, which its keeper
            // also reaches. Taken with the row for the same reason as the
            // place: from publication onwards this connection's destruction
            // owes what is in here.
            cleanup: cleanup.take().expect("a published row has its record"),
        })
    }

    fn route_input(
        &self,
        route: XAuthorityClientInputEvent,
    ) -> Result<(), XServerFrontendRouteError> {
        let senders = self.client_senders(route.client)?;
        let incarnation = senders.connection_state.clone();
        if !self.input_recovery.bind(route.delivery, route.client)? {
            return Ok(());
        }
        match self.route_to_client(route.client, &incarnation, senders.input, route) {
            Err(error @ XServerFrontendRouteError::ClientQueueFull { client }) => {
                // A client that stops draining its private input queue has
                // failed as an endpoint. Remove every sender for that client
                // so later routes cannot repeatedly pressure the shared
                // broker and its worker observes channel disconnection.
                // BOTH BY IDENTITY. The recovery disconnect is as keyed by
                // the number as the removal is, so a successor would be
                // disconnected as readily as it would be removed. Neither
                // happens unless the row under this number is still the
                // connection whose sender failed.
                if self.remove_row_of(client, &incarnation)? {
                    // AND THE DISCONNECT COMPARES AGAIN, under its own
                    // acquisition. The row check above released the client
                    // table before this line; a successor can publish in
                    // between, and its recovery entry would then be the one
                    // under this number. The identity travels to the act.
                    self.input_recovery.disconnect_exact(
                        client,
                        &incarnation,
                        XAuthorityInputDeliveryOutcome::ClientDisconnected,
                        route.delivery,
                    )?;
                }
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

    /// A window given a new parent by a departing client's save-set: every
    /// creator's entry for it follows, since the keys name the creator and
    /// the window outlived the client that reparented it.
    fn update_window_parent(
        &self,
        window: XResourceId,
        parent: XResourceId,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut parents = self
            .window_parents
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        for (key, value) in parents.iter_mut() {
            if key.1 == window {
                *value = parent;
            }
        }
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
include!("registry/ordered.rs");
include!("registry/delivery.rs");
include!("registry/control_backlog.rs");
include!("registry/present_msc.rs");

include!("registry/present_layout.rs");
