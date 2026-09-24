// A worker's context: the evidence a service leaves with its owner, and the
// permit that cannot be published and stops the connection but keeps the worker.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_service_that_ends_leaves_its_connections_evidence_with_its_owner() {
    // SERVICE EXIT IS NOT SETTLEMENT. The frontend, the registration and every
    // frame that used this connection go here; the custody, its published
    // result and the payload it carries stay with the owner, which is the only
    // thing whose destruction ends them.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 2);
    let carried;
    let home;
    {
        let private = private_over(&keeper, 2);
        let client = XServerFrontendClientId(8510);
        let (registration, _channels) = private
            .broker
            .registry
            .register_client_with_admission(client, Some(admitted(client)))
            .expect("a place, a keeper and a row");
        let PrivateCustodyReach::Reached(pin) =
            registration.registered_custody(&keeper.lease()).expect("its own custody")
        else {
            panic!("its owner keeps it")
        };
        home = Arc::downgrade(pin.join());

        // ONE REAL WORKER FOR THIS ONE SOURCE, started into the slot that
        // source owns and joined here -- before the registration it belongs
        // to goes anywhere.
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(PrivateOrderedWake::for_first_sender());
        assert_eq!(
            start_connection_worker(pin.worker_slot(), &stop, &wake, || {
                std::thread::Builder::new().spawn(|| panic!("what this one carried"))
            }),
            PrivateStartupOutcome::Started
        );
        let record = PrivateReapingRecord::bound_to(&pin);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        carried = panic_payload_of(pin.join()).expect("its payload");
        assert_eq!(carried, "what this one carried");

        // And the whole service scope ends: the record first, because it
        // borrows the pin it publishes through and the compiler will not let
        // it outlive one, then the pin, the registration and the frontend.
        drop(record);
        drop((pin, registration, private));
    }

    // THE KEEPER STILL HAS IT, and it still says what happened.
    assert_eq!(keeper.custodies_kept(), 1);
    let kept = home.upgrade().expect("its owner keeps this home");
    assert_eq!(kept.phase(), PrivateReapingPhase::Joined);
    assert_eq!(
        panic_payload_of(&kept).as_deref(),
        Some("what this one carried"),
        "the exact result, after the service that produced it has gone"
    );

    // AND ONLY DESTROYING THE OWNER ENDS IT, which is a separate event.
    drop(kept);
    drop(keeper);
    assert!(
        home.upgrade().is_none(),
        "the ultimate owner letting go is what releases the graph"
    );
    drop(durable);
}

#[test]
fn a_service_cannot_be_prepared_over_a_keeper_that_is_not_its_own() {
    // TWO OWNERS OVER ONE STORE ARE TWO INVENTORIES. A service built with one
    // and prepared with the other would reserve evidence in one place and look
    // for it in another, so the association is refused rather than carried.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 2);
    let stranger = service_owner(&durable, 2);
    let private = private_over(&keeper, 2);
    let (cause, returned) = match private.prepare_runner(NamespaceId::from_raw(8511), &stranger) {
        Ok(_) => panic!("a foreign owner prepared an execution scope"),
        Err(refused) => refused,
    };
    assert_eq!(cause, PrivateRunnerRefusal::ForeignServiceOwner);

    // REFUSED WITH NOTHING INSTALLED AND NOTHING CONSUMED: the caller still
    // has its frontend, and its own owner still prepares.
    let prepared = returned
        .prepare_runner(NamespaceId::from_raw(8511), &keeper)
        .map_err(|(cause, _)| cause);
    assert!(prepared.is_ok(), "its own owner prepares it");

    // AND A LEASE ON THE WRONG OWNER UNLOCKS NOTHING. A lease proves that SOME
    // keeper is alive; every act asks whether it is THIS service's, because a
    // stranger's liveness is not this service's liveness. The stranger here is
    // an owner over the very same store, which is the closest a wrong one can
    // be.
    let mut runner = prepared.expect("its own owner prepared it");
    let client = XServerFrontendClientId(8514);
    let (registration, _channels) = runner
        .frontend
        .as_ref()
        .expect("a live runner")
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper and a row");
    assert_eq!(
        runner
            .ingress_for(&stranger.lease(), client, DeviceId::from_raw(1))
            .err(),
        Some(PrivateServiceRefusal::ForeignServiceOwner),
        "a producer is not handed out on somebody else's keeper"
    );
    assert!(
        matches!(
            runner.service_turn(&stranger.lease()),
            Err(XServerFrontendRouteError::ForeignServiceOwner)
        ),
        "and no turn is served on one"
    );
    // AND A MISMATCH IS NOT A DESTRUCTION. Both owners are alive; what is
    // wrong is the association, and a caller told the keeper had gone would go
    // looking for something that never happened.
    assert!(
        matches!(
            registration.registered_custody(&stranger.lease()),
            Some(PrivateCustodyReach::ForeignKeeper)
        ),
        "and no custody is reached through one"
    );
    assert!(
        matches!(
            registration.registered_custody(&keeper.lease()),
            Some(PrivateCustodyReach::Reached(_))
        ),
        "its own lease reaches it immediately, so nothing was destroyed"
    );
    // ITS OWN OWNER GETS PAST ALL THREE. What the participant then says about
    // this connection is its own business -- this control registered a route
    // and did not admit one -- and an admission refusal is not a refusal about
    // the service.
    assert!(
        !matches!(
            runner.ingress_for(&keeper.lease(), client, DeviceId::from_raw(1)),
            Err(PrivateServiceRefusal::ForeignServiceOwner)
        ),
        "its own owner is this service's keeper"
    );
    assert!(runner.service_turn(&keeper.lease()).is_ok());
    assert!(matches!(
        registration.registered_custody(&keeper.lease()),
        Some(PrivateCustodyReach::Reached(_))
    ));
    drop((registration, runner));

    // AND AN OWNER CANNOT BE ESTABLISHED OVER A STORE NOBODY CAN READ. The
    // bound it must size itself to is the store's, so a store that cannot say
    // what its bound is cannot have an owner made over it. (Establishing also
    // refuses when the storage for that many places cannot be allocated, which
    // this control does not arrange.)
    let unreadable = PrivateSettlementOwner::default();
    let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = unreadable.records_even_if_poisoned();
        panic!("poisoning this store's aggregate, and nothing else");
    }));
    assert!(broken.is_err(), "the store's lock is poisoned");
    assert!(
        crate::PrivateServiceOwner::established_over(
            &unreadable,
            NonZeroUsize::new(2).expect("a real bound"),
        )
        .is_none(),
        "an owner is not established over a bound nobody could read"
    );
    drop((keeper, stranger, durable, unreadable));
}

/// A connection registered AND BOUND on this instance, the way a real one is,
/// so that its place can afterwards be finished and returned.
fn bound_on(
    private: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
) -> XServerFrontendClientRouteRegistration {
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        registration
            .bind_ordered_output(channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        None
    );
    registration
}


#[test]
fn an_inventory_refuses_a_second_home_and_a_foreign_name() {
    // ASKED OF THE KEEPER DIRECTLY, which is the same capability the registry
    // holds and reserves through. Registration reserves once per connection,
    // so neither of these answers is reachable by registering: they are what
    // the inventory says to a caller that asks twice, or asks about somebody
    // else's store.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 3);
    let private = private_over(&keeper, 3);
    let client = XServerFrontendClientId(8511);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper and a row");
    let named = registration.maintenance_identity().expect("a name");
    let PrivateCustodyReach::Reached(pin) =
        registration.registered_custody(&keeper.lease()).expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let home = Arc::clone(pin.join());
    assert_eq!(keeper.custodies_kept(), 1);

    // A SECOND PREPARATION FOR ONE LIVE RESERVATION. The first home is
    // preserved and the state it is in -- here, a join that has not happened
    // -- is left exactly as it was.
    let capability = keeper.keeper();
    assert!(matches!(
        capability.reserve_for(
            &named,
            registration.handover_gate(),
            Arc::clone(&registration.cleanup),
        ),
        PrivateCustodyReserved::AlreadyKept
    ));
    assert_eq!(
        keeper.custodies_kept(),
        1,
        "nothing was added and nothing was replaced"
    );
    let PrivateCustodyReach::Reached(again) =
        registration.registered_custody(&keeper.lease()).expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert!(
        Arc::ptr_eq(again.join(), &home),
        "the home this connection had is the home it has"
    );
    assert_eq!(again.join().phase(), PrivateReapingPhase::NotBegun);

    // A NAME FROM ANOTHER STORE IS NOT THIS OWNER'S TO KEEP, however well
    // formed it is.
    let elsewhere = PrivateSettlementOwner::default();
    let stranger_keeper = service_owner(&elsewhere, 2);
    let stranger = private_over(&stranger_keeper, 2);
    let outsider = XServerFrontendClientId(8512);
    let (foreign, _foreign_channels) = stranger
        .broker
        .registry
        .register_client_with_admission(outsider, Some(admitted(outsider)))
        .expect("a place, a keeper and a row");
    assert!(matches!(
        capability.reserve_for(
            &foreign.maintenance_identity().expect("a name"),
            foreign.handover_gate(),
            Arc::clone(&foreign.cleanup),
        ),
        PrivateCustodyReserved::Foreign
    ));
    assert_eq!(
        keeper.custodies_kept(),
        1,
        "a refused name took no place in this inventory"
    );
    assert_eq!(stranger_keeper.custodies_kept(), 1, "and none in that one");
    drop((pin, again));
    drop((registration, private, keeper));
    drop((foreign, stranger, stranger_keeper, elsewhere, durable));
}

#[test]
fn a_registry_cannot_be_given_a_second_keeper() {
    // ONE SERVICE, ONE INVENTORY. A registry that accepted a second keeper
    // could put this connection's evidence in one owner's inventory and the
    // next connection's in another, and afterwards nothing could say which
    // owner was answerable for what.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 3);
    let substitute = service_owner(&durable, 3);
    let private = private_over(&keeper, 3);
    assert!(
        !private
            .broker
            .registry
            .install_custody_keeper(substitute.keeper()),
        "construction installed this registry's keeper already"
    );

    // AND CONNECTIONS STILL GO TO THE OWNER IT WAS BUILT WITH.
    let client = XServerFrontendClientId(8513);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper and a row");
    assert_eq!(keeper.custodies_kept(), 1);
    assert_eq!(
        substitute.custodies_kept(),
        0,
        "the substitute keeps nothing, which is what being refused means"
    );
    let named = registration.maintenance_identity().expect("a name");
    assert!(keeper.custody_named(&named).is_some());
    assert!(
        substitute.custody_named(&named).is_none(),
        "and it answers for nothing either"
    );
    drop((registration, private, keeper, substitute, durable));
}

#[test]
fn a_producer_asks_again_at_every_acceptance() {
    // ISSUANCE IS NOT A STANDING PERMISSION. A producer handed out while the
    // keeper was alive goes on existing; what it must not do is go on
    // ACCEPTING work once the lease it is offered is not this service's. Both
    // producer classes are asked, because both take work into the service.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 2);
    let stranger = service_owner(&durable, 2);
    let mut private = private_over(&keeper, 2);
    let client = XServerFrontendClientId(8515);
    let surface = SurfaceId::new(8515, 1);
    let namespace = NamespaceId::from_raw(8515);
    let admission = namespaced(client, namespace);
    private.participant.admit(client, admission).expect("admitted");
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admission))
        .expect("a place, a keeper and a row");
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, XResourceId::new(0x8515, 1))
        .expect("the surface to register");
    // The plain stamped ingress, not a reserving producer: taking one of
    // those engages the ordered consumer, and this control drives the
    // unprepared instance's own routing path below.
    let ingress = private.ingress();
    let control = private.control_producer();

    // ITS OWN OWNER, AND THE WORK IS ACCEPTED.
    assert!(
        ingress
            .submit(
                &keeper.lease(),
                button_to(surface, XAuthorityInputDeliveryId::from_raw(85150), 272, true),
            )
            .is_ok(),
        "a live keeper accepts"
    );

    // A STRANGER'S LEASE, AND NOTHING IS ACCEPTED -- by either class. The work
    // comes back in hand, unaccepted, which is what a refusal owes a producer.
    let refused = ingress.submit(
        &stranger.lease(),
        button_to(surface, XAuthorityInputDeliveryId::from_raw(85151), 272, false),
    );
    assert!(
        matches!(refused, Err(PrivateSendError::ForeignServiceOwner(_))),
        "the input producer asks at acceptance: {refused:?}"
    );
    let control_refused = control.submit(
        &stranger.lease(),
        XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(8515),
                surface,
            },
        },
    );
    assert!(
        matches!(
            control_refused,
            Err((AdmissionRefusal::ForeignServiceOwner, _))
        ),
        "and so does the control producer"
    );

    // AND THE UNPREPARED INSTANCE'S OWN EXECUTION PATH ASKS TOO. This frontend
    // was never prepared into a runner, so it carries no borrow of its owner;
    // without this it would go on applying accepted work with no keeper.
    assert!(
        matches!(
            private.route_pending(&stranger.lease()),
            Err(XServerFrontendRouteError::ForeignServiceOwner)
        ),
        "an unprepared instance is not an unguarded one"
    );
    let ran = private
        .route_pending(&keeper.lease())
        .expect("its own owner runs it");
    assert_eq!(ran.len(), 1, "the one accepted entry, and only it");
    drop((ingress, control, registration, private, keeper, stranger));
}

#[test]
fn a_connections_worker_source_is_inert_and_names_one_slot_sink_and_home() {
    // WHAT THIS ESTABLISHES, AND WHAT IT DOES NOT. It looks at the source the
    // moment a registration returns: the slot is empty, the exit record says
    // nothing, the home says nothing, and asking again names the same three.
    //
    // IT IS NOT AN OBSERVATION OF THE INTERVAL. That the source was allocated
    // BEFORE the row went in is established by the order of the source --
    // reserve_for runs before publish_registered_client, and a refused
    // publication has an entry of its own to give back, which
    // a_connections_evidence_keeper_is_reserved_before_its_row_is_published
    // observes from inside the client table. Nothing here watches that
    // interval, and a control that inspected afterwards and called it proof
    // would be reading a state and claiming a schedule.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 3);
    let private = private_over(&keeper, 3);
    let client = XServerFrontendClientId(8601);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a source and a row");

    // EMPTY AND UNSTARTED, which is what reserving storage means.
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    {
        let slot = pin.worker_slot().lock().expect("a readable slot");
        assert!(slot.handle.is_none(), "no thread was started by registering");
        assert_eq!(slot.life, PrivateWorkerLife::NeverStarted);
        assert!(!slot.departing);
    }
    assert_eq!(
        pin.exit_sink().reading(),
        PrivateExitReading::NotLeft,
        "the exit record is this connection's own, and says nothing yet"
    );
    assert!(!pin.exit_sink().left(), "nothing has left, because nothing ran");
    assert_eq!(pin.join().phase(), PrivateReapingPhase::NotBegun);

    // AND ASKING AGAIN NAMES THE SAME THREE THINGS. A registration that could
    // make a second source would be deciding where its worker lives after the
    // connection was already exposed.
    let slot_address = std::ptr::from_ref(pin.worker_slot()) as usize;
    let home = Arc::clone(pin.join());
    let sink = Arc::clone(pin.exit_sink());
    drop(pin);
    for _ in 0..3 {
        let PrivateCustodyReach::Reached(again) = registration
            .registered_custody(&keeper.lease())
            .expect("its own custody")
        else {
            panic!("its owner keeps it")
        };
        assert_eq!(std::ptr::from_ref(again.worker_slot()) as usize, slot_address);
        assert!(Arc::ptr_eq(again.exit_sink(), &sink));
        assert!(Arc::ptr_eq(again.join(), &home));
    }
    assert_eq!(keeper.custodies_kept(), 1, "asking is not reserving");
    drop((sink, home));
    drop((registration, private, keeper, durable));
}

#[test]
fn a_started_worker_stays_with_its_source_after_the_view_that_started_it_ends() {
    // THE POINT OF THE WHOLE COMPONENT. A handle installed in the registered
    // slot belongs to the connection's external owner, so the frame that
    // started it can end -- ordinarily -- and a completely fresh view finds
    // that exact thread and joins it, publishing the original result into the
    // original home.
    //
    // OBSERVED FIRST, COLLECTED, AND ONLY THEN COMPARED. The worker is held on
    // a channel, so the final comparisons are arranged to follow its release
    // and its join on this path.
    //
    // THAT IS ALL THIS ORDER ESTABLISHES. The helper calls and lock reads
    // above it can still fail first, and no cleanup guard is armed, so this is
    // not a claim that every failure path collects the worker.
    let f = worker_fixture(XServerFrontendClientId(8602));
    f.permit();
    let (release, held) = std::sync::mpsc::channel::<()>();
    let home;
    let started_into_the_slot;
    {
        // ONE STARTUP VIEW, which ends here and takes nothing with it.
        let starting = custody_for(&f, &f.fixture.keeper);
        home = Arc::clone(starting.join());
        started_worker(&starting, &f, move || {
            let _ = held.recv();
            panic!("what this connection's worker carried out with it");
        });
        started_into_the_slot = starting
            .worker_slot()
            .lock()
            .expect("a readable slot")
            .handle
            .is_some();
    }

    // WHAT A FRESH VIEW FINDS, taken while the worker is still blocked.
    let later = custody_for(&f, &f.fixture.keeper);
    let found = {
        let slot = later.worker_slot().lock().expect("a readable slot");
        (slot.handle.is_some(), slot.life)
    };
    let same_home = Arc::ptr_eq(later.join(), &home);

    // RELEASED AND COLLECTED THROUGH THAT VIEW, before the comparisons.
    drop(release);
    let record = PrivateReapingRecord::bound_to(&later);
    let reaped = record.reap().reaped;
    let payload = panic_payload_of(&home);

    assert!(
        started_into_the_slot,
        "the handle went into the registered slot, not into the starting frame"
    );
    assert_eq!(
        found,
        (true, PrivateWorkerLife::Running),
        "its owner still held the thread after the starting view ended"
    );
    assert!(same_home, "and the same home for it");
    assert_eq!(reaped, PrivateReaped::Joined);
    assert_eq!(
        payload.as_deref(),
        Some("what this connection's worker carried out with it"),
        "the original result, in the home the source has owned throughout"
    );
    drop(record);
    drop(later);
    drop(f.fixture);
}

#[test]
fn a_caller_that_unwinds_after_startup_leaves_the_handle_with_its_owner() {
    // THE BOUNDARY IS AFTER STARTUP RETURNED, and this control says so rather
    // than claiming anything about an interruption inside a spawn. What it
    // establishes is that losing the frame that started a worker does not lose
    // the worker.
    //
    // THE COMPARISONS FOLLOW RELEASE AND COLLECTION ON THIS PATH. Every
    // observation below is taken into a local first; the thread is released
    // and joined; and only then does anything compare.
    //
    // NOT A GENERAL CLAIM ABOUT FAILURE PATHS. The helper calls and lock reads
    // before that point can fail on their own, and nothing here arms a
    // release-and-join guard that would run if they did.
    let f = worker_fixture(XServerFrontendClientId(8603));
    f.permit();
    let (release, held) = std::sync::mpsc::channel::<()>();
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let starting = custody_for(&f, &f.fixture.keeper);
        started_worker(&starting, &f, move || {
            let _ = held.recv();
        });
        panic!("the frame that started it goes here");
    }));

    let after = custody_for(&f, &f.fixture.keeper);
    let observed = {
        let slot = after.worker_slot().lock().expect("a readable slot");
        (slot.handle.is_some(), slot.life)
    };

    // RELEASED AND COLLECTED, before the comparisons.
    drop(release);
    let record = PrivateReapingRecord::bound_to(&after);
    let reaped = record.reap().reaped;
    let published = matches!(after.join().result(), Some(PrivateJoinResult::Returned));

    assert!(unwound.is_err(), "the caller really did unwind");
    assert_eq!(
        observed,
        (true, PrivateWorkerLife::Running),
        "an unwound starter does not take the handle with it"
    );
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(published, "and its result went into its own home");
    drop(record);
    drop(after);
    drop(f.fixture);
}

#[test]
fn a_control_context_takes_its_credentials_from_its_own_serving_owner() {
    // NOT FROM WHOEVER PREPARED IT. The stop and the notice are the ones this
    // connection's own serving owner published, resolved under the one
    // acquisition of the home that establishes that owner is there and live.
    let f = worker_fixture(XServerFrontendClientId(8701));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody
        .prepare_control()
        .expect("a live serving owner with a stop of its own");

    // THE EXACT ORIGINAL HANDLES, not copies of what they held. The fixture's
    // own clones came from the same binding, so pointer identity is the whole
    // assertion.
    assert!(
        Arc::ptr_eq(context.stop(), &f.stop),
        "this connection's authoritative stop"
    );
    assert!(
        Arc::ptr_eq(context.notice(), &f.wake),
        "and its own notice"
    );
    assert!(
        Arc::ptr_eq(context.home(), &f.fixture.registration.ordered_home),
        "resolved to the home its own reservation made"
    );
    assert!(Arc::ptr_eq(context.exit_sink(), custody.exit_sink()));

    // AND A FRESH VIEW RECOVERS THE PUBLISHED ASSOCIATION rather than binding
    // a second one.
    //
    // ASKED WHERE A SECOND BINDING WOULD ANSWER DIFFERENTLY. Preparing again
    // over a live owner would derive the same handles either way, which proves
    // nothing; this connection's home is RETAINED first, so a preparation that
    // rebound would refuse -- and the association that is already published is
    // what a later view must still find.
    // The first context ends here; it is a borrow, so there is nothing to
    // release.
    assert!(f.fixture.registration.ordered_home.retain());
    let again = custody_for(&f, &f.fixture.keeper);
    let recovered = again
        .prepare_control()
        .expect("what was published is still this connection's");
    assert!(Arc::ptr_eq(recovered.stop(), &f.stop));
    assert!(Arc::ptr_eq(recovered.notice(), &f.wake));
    assert!(Arc::ptr_eq(
        recovered.home(),
        &f.fixture.registration.ordered_home
    ));
    drop(again);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn one_connections_context_cannot_stop_its_sibling() {
    // TWO REAL CONNECTIONS OF ONE INSTANCE, over ONE store and ONE service
    // owner -- which this control asserts rather than assumes, because two
    // fixtures would be two stores and separation between those is not the
    // question. Each derives its own credentials from its own serving owner.
    let f = worker_fixture(XServerFrontendClientId(8702));
    let (sibling, sibling_stop, sibling_notice, _sibling_peer) =
        serving_sibling(&f, XServerFrontendClientId(8703), true);
    let keeper = &f.fixture.keeper;
    assert!(
        keeper
            .custody_named(&f.fixture.registration.maintenance_identity().expect("a name"))
            .is_some()
            && keeper
                .custody_named(&sibling.maintenance_identity().expect("a name"))
                .is_some(),
        "one owner's inventory answers for both, so this is one store"
    );

    let first_custody = custody_for(&f, keeper);
    let PrivateCustodyReach::Reached(second_custody) = sibling
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("the same owner keeps it")
    };
    let first_context = first_custody.prepare_control().expect("its own owner");
    let second_context = second_custody.prepare_control().expect("its own owner");

    assert!(
        !Arc::ptr_eq(first_context.stop(), second_context.stop()),
        "two connections, two stops"
    );
    assert!(!Arc::ptr_eq(first_context.notice(), second_context.notice()));
    assert!(!Arc::ptr_eq(first_context.home(), second_context.home()));
    assert!(Arc::ptr_eq(second_context.stop(), &sibling_stop));
    assert!(Arc::ptr_eq(second_context.notice(), &sibling_notice));

    // ONE EXACT CAPSULE ON THE SIBLING'S QUEUE, so a cancellation that reached
    // across would have something of its neighbour's to disturb.
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(87030);
    let delivery = capsule.delivery();
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let frames = order_pass_frames(&capsule);
    let sibling_sender = capture_gated_sender(
        f.fixture.runner.frontend.as_ref().expect("a live runner"),
        XServerFrontendClientId(8703),
    );
    produced_send(&sibling_sender, capsule);

    // ONE IS CANCELLED, AND ONLY ONE IS TOLD TO STOP.
    first_context.cancel();
    assert!(
        f.stop.load(std::sync::atomic::Ordering::Acquire),
        "the one this context is about"
    );
    assert!(
        !sibling_stop.load(std::sync::atomic::Ordering::Acquire),
        "and its sibling was not told anything"
    );

    // AND THE SIBLING'S EXACT QUEUED WORK IS WHERE IT WAS.
    let queued = second_context
        .home()
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("a readable live home")
        .expect("the capsule this control accepted for it");
    assert_eq!(queued.delivery(), delivery);
    assert!(Arc::ptr_eq(
        &cell,
        &queued.finalizer().expect("carried").completion
    ));
    assert_eq!(order_pass_frames(&queued), frames);
    assert!(cell.answer().is_none());
    drop(queued);
    drop(sibling_sender);
    drop((first_custody, second_custody));
    drop(sibling);
    drop(f.fixture);
}

#[test]
fn cancelling_through_a_context_does_not_wait_on_the_home_it_came_from() {
    // THE CASE THE BINDING EXISTS FOR. A worker blocked while borrowing its
    // own home is exactly when cancellation must work, so a cancellation that
    // went back to the home to find its own stop would queue behind the thing
    // it is trying to stop.
    //
    // THE HOME IS HELD BY THIS CONTROL for the whole cancellation, which is
    // the same lock a borrowing worker would hold. This says nothing about how
    // long anything takes.
    let f = worker_fixture(XServerFrontendClientId(8704));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");

    let blocked = context.home().borrow(|_| {
        // Inside the home's own lock, and this is where cancellation runs.
        context.cancel();
        (
            f.stop.load(std::sync::atomic::Ordering::Acquire),
            f.wake.state.lock().expect("a readable notice").pending,
        )
    });
    assert_eq!(
        blocked,
        Some((true, true)),
        "the stop was set and the recheck published while its home was held"
    );

    // AND THE STORE'S AGGREGATE WAS NOT NEEDED EITHER: this one runs with the
    // settlement store's own guard held by this control. That is the store,
    // not the owner's custody inventory -- naming the wrong lock would be
    // claiming a separation this does not test.
    let other = worker_fixture(XServerFrontendClientId(8705));
    let other_custody = custody_for(&other, &other.fixture.keeper);
    let other_context = other_custody.prepare_control().expect("its own owner");
    let held = other.fixture.durable.records_even_if_poisoned();
    other_context.cancel();
    drop(held);
    assert!(other.stop.load(std::sync::atomic::Ordering::Acquire));
    drop((custody, other_custody));
    drop((f.fixture, other.fixture));
}

#[test]
fn a_departure_that_wins_leaves_the_spawner_uncalled() {
    // NO MORE STARTS MEANS NO MORE STARTS. Departure and startup use the same
    // registered slot, so a start that arrives afterwards never reaches its
    // spawner -- which is the only way to be sure no thread was made.
    let f = worker_fixture(XServerFrontendClientId(8706));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");

    assert_eq!(
        context.depart(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
    );
    assert!(
        f.stop.load(std::sync::atomic::Ordering::Acquire),
        "departure set this connection's own stop first"
    );
    let called = std::sync::atomic::AtomicBool::new(false);
    let outcome = context.start(|| {
        called.store(true, std::sync::atomic::Ordering::Release);
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(outcome, PrivateStartupOutcome::NoLongerStartable);
    assert!(
        !called.load(std::sync::atomic::Ordering::Acquire),
        "the spawner was never called, so no thread exists to be lost"
    );
    assert_eq!(
        context.depart(),
        PrivateDeparted::AlreadyDecided(PrivateDeparture::NothingStarted),
        "and saying it again reports the decision rather than making another"
    );
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_stop_reaches_a_worker_whose_destination_is_still_held() {
    // THE OTHER HALF OF THE SAME PROPERTY. A spawn holds this connection's
    // slot for the whole transaction; a cancellation arriving in that window
    // must still be able to set the stop and publish the recheck, because it
    // needs neither the slot nor the home to find them.
    let f = worker_fixture(XServerFrontendClientId(8707));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let (enter, entered) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();

    let witnessed = std::thread::scope(|scope| {
        let context = &context;
        let started = scope.spawn(move || {
            context.start(move || {
                // The destination is held for as long as this spawner runs.
                enter.send(()).expect("its caller is waiting");
                let _ = released.recv();
                std::thread::Builder::new().spawn(|| {})
            })
        });
        entered
            .recv_timeout(Duration::from_secs(3))
            .expect("the spawn began and is holding the slot");
        // WITNESSED WHILE THE SLOT IS HELD: this is the interval, not a guess
        // about one.
        context.cancel();
        let witnessed = (
            f.stop.load(std::sync::atomic::Ordering::Acquire),
            f.wake.state.lock().expect("a readable notice").pending,
        );
        drop(release);
        assert_eq!(started.join().expect("the startup returned"), PrivateStartupOutcome::Started);
        witnessed
    });
    assert_eq!(
        witnessed,
        (true, true),
        "the stop was set while this connection's own destination was held"
    );

    // AND THE WORKER IT DID MAKE IS THIS CONNECTION'S, AND IS COLLECTED HERE.
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    drop(record);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_body_driven_by_its_own_context_stops_when_that_context_says_so() {
    // END TO END THROUGH THE REGISTERED SEAM. The approved body runs on the
    // handles this context derived and the exit sink this source owns; the
    // same context cancels it; and its custody joins it.
    let f = worker_fixture(XServerFrontendClientId(8708));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let home = Arc::clone(context.home());
    let notice = Arc::clone(context.notice());
    let stop = Arc::clone(context.stop());
    let sink = Arc::clone(context.exit_sink());
    let sequence = Arc::clone(&f.sequence);
    assert_eq!(
        context.start(|| {
            std::thread::Builder::new().spawn(move || {
                PrivateWorkerBody {
                    home: &home,
                    wake: &notice,
                    stop: &stop,
                    byte_order: XByteOrder::LittleEndian,
                    sequence: &sequence,
                    exit: &sink,
                    steps: 16,
                }
                .run();
            })
        }),
        PrivateStartupOutcome::Started
    );

    // TOLD TO STOP THROUGH THE CONTEXT, and collected through the custody.
    context.cancel();
    let record = PrivateReapingRecord::bound_to(&custody);
    let reaping = record.reap();
    let reaped = reaping.reaped;
    let exit = reaping.exit;

    assert_eq!(reaped, PrivateReaped::Joined);
    let Some(PrivateExitReading::Classified(outcome)) = exit else {
        panic!("the body left a classification: {exit:?}")
    };
    assert!(
        stopped_by_cancellation(&outcome),
        "the owner's own terminal answer, which is not the same fact as the \
         cancellation that triggered it: {outcome:?}"
    );
    drop(record);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn preparing_a_control_context_refuses_each_shape_as_itself() {
    // SEVEN DIFFERENT ANSWERS, and a caller told the wrong one looks in the
    // wrong place. None of them starts anything, permits anything, sets a
    // stop, takes queued work or changes what a home holds.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 6);
    let private = private_over(&keeper, 6);

    // NOTHING BOUND: a registered connection whose setup never arrived. Its
    // home is live and empty, so there is no owner to take credentials from.
    let bare = XServerFrontendClientId(8711);
    let (bare_registration, _bare_channels) = private
        .broker
        .registry
        .register_client_with_admission(bare, Some(admitted(bare)))
        .expect("a place, a keeper, a source and a row");
    let PrivateCustodyReach::Reached(bare_custody) = bare_registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert_eq!(
        bare_custody.prepare_control().err(),
        Some(PrivateControlRefusal::NothingBound)
    );
    assert!(
        bare_custody.prepare_control().is_err(),
        "a refusal leaves it unprepared, so asking again asks again"
    );
    assert_eq!(
        bare_custody
            .worker_slot()
            .lock()
            .expect("a readable slot")
            .life,
        PrivateWorkerLife::NeverStarted,
        "and starts nothing"
    );

    // UNSTOPPABLE: a real serving owner bound without a stop of its own. Its
    // binding said so, and nothing here mints a replacement.
    let unstoppable = worker_fixture_bound(XServerFrontendClientId(8712), false);
    let unstoppable_custody = custody_for(&unstoppable, &unstoppable.fixture.keeper);
    assert_eq!(
        unstoppable_custody.prepare_control().err(),
        Some(PrivateControlRefusal::Unstoppable),
        "a stop this context invented would be one its worker never reads"
    );

    // RETAINED: its connection has ended. Driving is not how retained work is
    // finished, and this is not that borrower's home.
    let ended = worker_fixture(XServerFrontendClientId(8713));
    let ended_custody = custody_for(&ended, &ended.fixture.keeper);
    assert!(ended_custody.prepare_control().is_ok(), "live to begin with");
    let later = worker_fixture(XServerFrontendClientId(8714));
    let later_custody = custody_for(&later, &later.fixture.keeper);

    // ONE EXACT CAPSULE, ACCEPTED BEFORE ANY OF THIS, so the refusal below has
    // something it could have taken.
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(87140);
    let delivery = capsule.delivery();
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let frames = order_pass_frames(&capsule);
    produced_send(&later.sender, capsule);
    assert!(later.fixture.registration.ordered_home.retain());
    assert_eq!(
        later_custody.prepare_control().err(),
        Some(PrivateControlRefusal::Retained)
    );

    // AND THE EXACT QUEUED WORK IS STILL THERE, unanswered and unchanged: the
    // refusal did not take it, answer it or rebind it.
    let queued = later
        .fixture
        .registration
        .ordered_home
        .peek_retained(|continuation| continuation.queue().try_recv().ok())
        .expect("a readable retained home")
        .flatten()
        .expect("the capsule this control accepted is still queued");
    assert_eq!(queued.delivery(), delivery);
    assert!(Arc::ptr_eq(
        &cell,
        &queued.finalizer().expect("carried").completion
    ));
    assert_eq!(order_pass_frames(&queued), frames);
    assert!(cell.answer().is_none(), "and nothing answered it");
    drop(queued);

    // NOT SERVING: a real connection bound but never promoted. Its home holds
    // its setup, which is a payload and is not an owner to take credentials
    // from. Nothing is staged -- this is the shape a connection has between
    // binding and promotion.
    let host = worker_fixture(XServerFrontendClientId(8718));
    let (unpromoted, _unpromoted_stop, _unpromoted_notice, _unpromoted_peer) =
        serving_sibling(&host, XServerFrontendClientId(8719), false);
    let PrivateCustodyReach::Reached(setup_custody) = unpromoted
        .registered_custody(&host.fixture.keeper.lease())
        .expect("its own custody")
    else {
        panic!("the same owner keeps it")
    };
    assert_eq!(
        setup_custody.prepare_control().err(),
        Some(PrivateControlRefusal::NotServing)
    );

    // UNREADABLE: a holder panicked inside the home. Not recovered into
    // eligibility -- the lock WAS acquired, and what is unknown is what is in
    // there, which is the thing preparation would be asserting.
    let broken = worker_fixture(XServerFrontendClientId(8720));
    let broken_custody = custody_for(&broken, &broken.fixture.keeper);
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = broken
            .fixture
            .registration
            .ordered_home
            .state
            .lock()
            .expect("a readable home");
        panic!("poisoning this connection's home, and nothing else");
    }));
    assert!(poisoning.is_err());
    assert_eq!(
        broken_custody.prepare_control().err(),
        Some(PrivateControlRefusal::Unreadable)
    );

    // STORE GONE IS DEFENSIVE HERE, and this control says so rather than
    // staging a path to it. A registered custody owns the store its name is
    // in, so while that custody exists the store does; the answer is kept
    // apart because it is a different fact, not because this reaches it.

    drop((
        bare_custody,
        unstoppable_custody,
        ended_custody,
        later_custody,
        setup_custody,
        broken_custody,
    ));
    drop(unpromoted);
    drop((host.fixture, broken.fixture));
    drop(bare_registration);
    drop((unstoppable.fixture, ended.fixture, later.fixture));
    drop((private, keeper, durable));
}

#[test]
fn a_stale_name_prepares_nothing_for_the_connection_that_took_its_place() {
    // A NAME OUTLIVES ITS CONNECTION, and a place is taken again. A context
    // prepared from the old name would be driving the successor's worker with
    // the predecessor's credentials.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 3);
    let private = private_over(&keeper, 3);
    let first = bound_on(&private, XServerFrontendClientId(8715));
    let PrivateCustodyReach::Reached(first_custody) = first
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let place = first.maintenance_identity().expect("a name").place();
    drop(first);
    settle_and_return(&durable, place);

    // A REAL SUCCESSOR TAKES THAT NUMBER.
    let successor = bound_on(&private, XServerFrontendClientId(8716));
    assert_eq!(
        successor.maintenance_identity().expect("a name").place(),
        place
    );
    assert_eq!(
        first_custody.prepare_control().err(),
        Some(PrivateControlRefusal::StaleName),
        "the old name reaches nothing at a number its connection gave back"
    );
    drop(first_custody);
    drop((successor, private, keeper, durable));
}

#[test]
fn a_permit_that_cannot_be_published_stops_this_connection_and_keeps_its_worker() {
    // THE PATH THE STOP IN A STARTUP IS FOR. A permit that cannot be published
    // leaves a thread already running, so the transaction tells THIS
    // connection to stop, wakes it, and keeps the handle where it is to be
    // joined. A startup holding somebody else's stop would set a flag its own
    // worker never looks at.
    let f = worker_fixture(XServerFrontendClientId(8717));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");

    // The notice is poisoned by an ordinary panic under its own lock. Nothing
    // is rewritten: this is what a holder that unwound would leave.
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = context.notice().state.lock().expect("a readable notice");
        panic!("poisoning this connection's notice, and nothing else");
    }));
    assert!(poisoning.is_err());
    assert!(context.notice().state.is_poisoned());

    let outcome = context.start(|| std::thread::Builder::new().spawn(|| {}));
    let stopped = f.stop.load(std::sync::atomic::Ordering::Acquire);
    let kept = context
        .custody
        .worker_slot()
        .lock()
        .expect("a readable slot")
        .handle
        .is_some();

    // COLLECTED BEFORE ANYTHING COMPARES, so a failure cannot leave a thread
    // this control started with nobody to join it.
    let record = PrivateReapingRecord::bound_to(&custody);
    let reaped = record.reap().reaped;

    assert_eq!(outcome, PrivateStartupOutcome::PermitRefused);
    assert!(
        stopped,
        "the transaction set THIS connection's own stop, which is the one its \
         worker reads"
    );
    assert!(kept, "and kept the handle where its owner can join it");
    assert_eq!(reaped, PrivateReaped::Joined);
    drop(record);
    drop(custody);
    drop(f.fixture);
}
