// Departure and cleanup: what a preparation recovers after losing a race, the
// records a departure leaves, and the boundary that admits nothing.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_preparation_that_loses_a_race_recovers_what_the_winner_published() {
    // OVERLAPPING, NOT SEQUENTIAL. The sequential case -- ask, then ask again
    // -- is covered where a retained home makes a rebinding preparation
    // answer differently. THIS one is about a preparation that began when
    // nothing was published and finishes after another has committed: its own
    // resolution fails, and the connection nevertheless has an association.
    //
    // A caller refused here would have to ask again for what this call should
    // have recovered, and nothing about that refusal is still true.
    //
    // STAGED WITHOUT A THREAD. The order is produced by doing the two halves
    // of the losing preparation around the winner: this control observes that
    // nothing is published, lets the winner publish, poisons the home by an
    // ordinary caught panic under its own mutex, and only then runs the
    // preparation whose resolution must fail.
    let f = worker_fixture(XServerFrontendClientId(8721));
    let losing = custody_for(&f, &f.fixture.keeper);
    let winning = custody_for(&f, &f.fixture.keeper);

    // THE LOSER'S STARTING CONDITION, observed rather than assumed.
    assert!(
        losing.bound().is_none(),
        "nothing is published when the losing preparation begins"
    );

    // THE WINNER COMMITS.
    let published = winning.prepare_control().expect("a live serving owner");
    let stop = Arc::clone(published.stop());
    let notice = Arc::clone(published.notice());
    let home = Arc::clone(published.home());

    // AND THE HOME BECOMES UNREADABLE, which is what the loser's own
    // resolution will find.
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = f
            .fixture
            .registration
            .ordered_home
            .state
            .lock()
            .expect("a readable home");
        panic!("poisoning this connection's home, and nothing else");
    }));
    assert!(poisoning.is_err());

    // THE LOSING PREPARATION FINISHES. Its entry check already found nothing
    // -- that is what was observed above -- so what runs now is the half that
    // resolves, which is where the interval is. Its resolution cannot read the
    // home; the connection's association exists; the association is the
    // answer.
    let recovered = losing
        .resolve_control()
        .expect("the binding another preparation committed");
    assert!(Arc::ptr_eq(recovered.stop(), &stop));
    assert!(Arc::ptr_eq(recovered.notice(), &notice));
    assert!(Arc::ptr_eq(recovered.home(), &home));

    // AND A SOURCE NOBODY PREPARED STILL GETS THE REFUSAL. Recovering a
    // published binding is not recovering a poisoned home into eligibility.
    let unprepared = worker_fixture(XServerFrontendClientId(8722));
    let never = custody_for(&unprepared, &unprepared.fixture.keeper);
    let breaking = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = unprepared
            .fixture
            .registration
            .ordered_home
            .state
            .lock()
            .expect("a readable home");
        panic!("poisoning a home nobody has prepared over");
    }));
    assert!(breaking.is_err());
    assert_eq!(
        never.resolve_control().err(),
        Some(PrivateControlRefusal::Unreadable),
        "an unprepared source has no association to recover"
    );
    assert_eq!(
        never.prepare_control().err(),
        Some(PrivateControlRefusal::Unreadable),
        "by either entry point"
    );
    drop((losing, winning, never));
    drop((f.fixture, unprepared.fixture));
}

/// A SECOND connection of THIS instance, served by a real owner of its own.
///
/// The same registry, the same store and the same service owner as the fixture
/// it is given: a sibling built on its own frontend would be a different
/// store, and separation between two stores is not the question.
fn serving_sibling(
    f: &PrivateWorkerFixture,
    client: XServerFrontendClientId,
    promote: bool,
) -> (
    XServerFrontendClientRouteRegistration,
    Arc<AtomicBool>,
    Arc<PrivateOrderedWake>,
    UnixStream,
) {
    let private = f
        .fixture
        .runner
        .frontend
        .as_ref()
        .expect("a live runner");
    // IN THIS INSTANCE'S OWN NAMESPACE, which is what makes it this
    // instance's sibling rather than a stranger it would refuse.
    let admission = namespaced(client, f.fixture.namespace);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admission))
        .expect("a place, a keeper, a source and a row");
    // Admitted at the boundary, because promotion asks it for this
    // registration's endpoint and a connection assembled beside it is refused
    // as unadmitted -- rightly.
    private
        .participant
        .admit(client, admission)
        .expect("the boundary admits this sibling");
    // And its applied state, because the endpoint promotion asks for is the
    // one attached to this registration's own row.
    private
        .broker
        .registry
        .attach_connection_state(
            &registration,
            f.fixture.namespace,
            Arc::new(Mutex::new(XCoreEventSelectionState::default())),
            Arc::new(AtomicU64::new(0)),
        )
        .expect("its own applied state attaches");
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(3)))
        .expect("a bounded read");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::clone(&channels.ordered.wake);
    let transport = XAuthorityOrderedTransport::bind(
        &registration,
        channels.ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    assert!(matches!(
        registration.retain_ordered_setup(PrivateOrderedContinuation::Setup {
            accepted: PrivateOrderedSetupCustody::Transport(Box::new(transport)),
            refusal: X11OrderedServingRefusal::Unserved,
            evidence: PrivateOrderedEvidence::unstarted(),
            retained: Vec::new(),
            drained: false,
            ended: false,
            ending_refused: None,
        }),
        Ok(())
    ));
    if promote {
        assert_eq!(
            registration.promote_ordered_serving(private),
            PrivateOrderedPromotion::Ready
        );
    }
    (registration, stop, wake, peer)
}

#[test]
fn a_connections_fence_source_names_the_gate_its_queue_was_minted_with() {
    // THE EXACT GATE, not one that matches. The sender, the row, the
    // registration and this source were all given the same Arc, which is what
    // makes a fencing through this custody a fencing of this connection.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8801);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a source and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert!(
        Arc::ptr_eq(pin.gate(), &registration.handover_gate()),
        "the registration's own gate"
    );

    // AND ITS FENCE STORAGE IS RESERVED, INERT AND EMPTY.
    assert_eq!(
        pin.fence_evidence().phase(),
        PrivateFencePhase::NotAttempted
    );
    assert!(pin.fence_evidence().fence().is_none());
    assert!(
        !registration
            .ordered_handovers_fenced()
            .expect("a readable gate"),
        "and reserving it closed nothing"
    );

    // ASKING AGAIN NAMES THE SAME GATE AND THE SAME STORAGE. This is an
    // observation after registration returned; that the gate was bound BEFORE
    // publication is established by source order -- reserve_for runs before
    // publish_registered_client -- and by the client-table control named
    // below, not by anything this control watches.
    let address = std::ptr::from_ref(pin.fence_evidence()) as usize;
    drop(pin);
    let PrivateCustodyReach::Reached(again) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert!(Arc::ptr_eq(again.gate(), &registration.handover_gate()));
    assert_eq!(
        std::ptr::from_ref(again.fence_evidence()) as usize,
        address
    );

    // A REFUSED DUPLICATE PUBLICATION RETURNS ONLY ITS OWN, AND THAT IS ALL
    // THIS SHOWS. A count that is one before and one after does not witness
    // the interval: what establishes that the gate and this storage are bound
    // BEFORE the row goes in is source order -- reserve_for runs before
    // publish_registered_client -- and the client-table rendezvous in
    // a_connections_evidence_keeper_is_reserved_before_its_row_is_published,
    // which observes a waiting attempt's own entry while publication is
    // excluded.
    assert_eq!(keeper.custodies_kept(), 1);
    assert!(matches!(
        private
            .broker
            .registry
            .register_client_with_admission(client, Some(admitted(client))),
        Err(XServerFrontendRouteError::DuplicateClient { .. })
    ));
    assert_eq!(keeper.custodies_kept(), 1);
    assert!(Arc::ptr_eq(again.gate(), &registration.handover_gate()));
    assert!(
        !registration
            .ordered_handovers_fenced()
            .expect("a readable gate"),
        "and the live connection's gate is untouched"
    );
    drop(again);
    drop((registration, private, keeper, durable));
}

#[test]
fn fencing_one_connection_through_its_custody_leaves_its_sibling_alone() {
    // TWO REAL CONNECTIONS OF ONE INSTANCE. A fencing takes its join, its gate
    // and its result home from one custody, so there is nothing to give it
    // that belongs to the other.
    let f = worker_fixture(XServerFrontendClientId(8802));
    let (sibling, _sibling_stop, _sibling_notice, _sibling_peer) =
        serving_sibling(&f, XServerFrontendClientId(8803), true);
    let keeper = &f.fixture.keeper;
    let custody = custody_for(&f, keeper);
    let PrivateCustodyReach::Reached(sibling_custody) = sibling
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("the same owner keeps it")
    };

    // ONE EXACT CAPSULE ON THE SIBLING'S QUEUE.
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(88030);
    let delivery = capsule.delivery();
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let frames = order_pass_frames(&capsule);
    let sibling_sender = capture_gated_sender(
        f.fixture.runner.frontend.as_ref().expect("a live runner"),
        XServerFrontendClientId(8803),
    );
    produced_send(&sibling_sender, capsule);

    // ITS OWN WORKER, JOINED, AND ITS OWN GATE CLOSED.
    started_worker(&custody, &f, || {});
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));

    // AND THE SIBLING IS EXACTLY AS IT WAS.
    assert!(
        !sibling
            .ordered_handovers_fenced()
            .expect("a readable gate"),
        "its gate is open"
    );
    assert_eq!(
        sibling_custody.fence_evidence().phase(),
        PrivateFencePhase::NotAttempted,
        "nothing was attempted for it"
    );
    assert_eq!(
        sibling.ordered_home.standing(),
        PrivateHomeStanding::Live,
        "and its home is live"
    );
    let queued = sibling
        .ordered_home
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
    drop(record);
    drop(sibling_sender);
    drop((custody, sibling_custody));
    drop(sibling);
    drop(f.fixture);
}

#[test]
fn a_fencing_survives_the_view_that_recorded_it() {
    // THE RESULT BELONGS TO THE KEEPER, NOT THE VIEW. An early view refuses
    // and costs nothing; a later one records; and what it recorded is still
    // there after that view ends ordinarily AND after another unwinds holding
    // one.
    let f = worker_fixture(XServerFrontendClientId(8804));
    let custody = custody_for(&f, &f.fixture.keeper);

    // TOO EARLY: no join has published, so no gate is touched and no attempt
    // is spent.
    {
        let early = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(early.record_fence(), PrivateFenced::JoinIncomplete);
        assert_eq!(
            custody.fence_evidence().phase(),
            PrivateFencePhase::NotAttempted
        );
        assert!(
            !f.fixture
                .registration
                .ordered_handovers_fenced()
                .expect("a readable gate"),
            "an ask made too early asks no gate"
        );
    }

    started_worker(&custody, &f, || {});
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);

    // A LATER VIEW RECORDS, AND THEN ENDS.
    {
        let recording = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(recording.record_fence(), PrivateFenced::Recorded);
    }
    assert_eq!(
        custody.fence_evidence().fence(),
        Some(PrivateHandoverFence::Established),
        "the answer is the keeper's, and the view that got it has gone"
    );

    // AND AN UNWIND AFTER RECORDING TAKES NOTHING WITH IT.
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let holding = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(holding.record_fence(), PrivateFenced::AlreadyAttempted);
        panic!("the frame holding a view goes here");
    }));
    assert!(unwound.is_err());
    assert_eq!(
        custody.fence_evidence().fence(),
        Some(PrivateHandoverFence::Established),
        "still the original answer"
    );

    // A FRESH VIEW OBSERVES THAT ORIGINAL RESULT rather than asking the gate
    // again: a second close call would answer AlreadyEstablished, and this
    // connection's record of its own closure would become the wrong one.
    let fresh = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(fresh.record_fence(), PrivateFenced::AlreadyAttempted);
    assert_eq!(fresh.fence(), Some(PrivateHandoverFence::Established));
    drop((fresh, record));
    drop(custody);
    drop(f.fixture);
}

#[test]
fn two_eligible_views_make_one_attempt() {
    // TWO FRESHLY BUILT VIEWS, NEITHER PREVIOUSLY ASKED. A claim kept on the
    // view would let each mint its own right and both reach the gate; the
    // second close call would answer AlreadyEstablished and overwrite nothing
    // -- but this connection would have two records of one closure and the
    // published one would be whichever won a race.
    //
    // THE OVERLAP IS WITNESSED BY HOLDING THE GATE'S OWN ADMISSION, not by
    // sleeping. What that establishes is exactly this: one view has CLAIMED
    // the attempt and has not finished it, and the loser's answer is read
    // while that is true. Where the winner has got to is not established --
    // it may not have reached the gate's mutex at all -- and this control
    // does not say it has.
    let f = worker_fixture(XServerFrontendClientId(8805));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || {});
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);

    let own_gate = f.fixture.registration.handover_gate();
    let admitted_handover = own_gate
        .entered()
        .expect("an open gate admits this control");
    let (report, answered) = std::sync::mpsc::channel();
    let observed = std::thread::scope(|scope| {
        let custody = &custody;
        let report = report.clone();
        let winner = scope.spawn(move || {
            let view = PrivateFenceRecord::bound_to(custody);
            report.send(view.record_fence()).expect("its caller waits");
        });
        // THE CLAIM IS TAKEN WHILE THIS CONTROL HOLDS THE GATE.
        assert!(
            waited_for(|| custody.fence_evidence().phase() == PrivateFencePhase::InProgress),
            "one view claimed the attempt and has not finished it"
        );
        let losing = PrivateFenceRecord::bound_to(custody);
        let loser = losing.record_fence();
        let phase_while_held = custody.fence_evidence().phase();
        let result_while_held = custody.fence_evidence().fence();
        drop(admitted_handover);
        winner.join().expect("the fencing view returned");
        (loser, phase_while_held, result_while_held)
    });
    let winner = answered
        .recv_timeout(Duration::from_secs(3))
        .expect("the winning view reported");

    assert_eq!(observed.0, PrivateFenced::AlreadyAttempted, "one attempt");
    assert_eq!(
        observed.1,
        PrivateFencePhase::InProgress,
        "the losing ask did not withdraw the winner's intent"
    );
    assert_eq!(
        observed.2, None,
        "and an unfinished attempt publishes no result"
    );
    assert_eq!(winner, PrivateFenced::Recorded);
    assert_eq!(
        custody.fence_evidence().fence(),
        Some(PrivateHandoverFence::Established),
        "the one attempt's own answer"
    );
    drop(record);
    drop(custody);
    drop(f.fixture);
}

/// The cleanup record this connection's registration and keeper share.
fn cleanup_of(registration: &XServerFrontendClientRouteRegistration) -> Arc<PrivateCleanupRecord> {
    Arc::clone(&registration.cleanup)
}

#[test]
fn a_registration_and_its_keeper_reach_one_cleanup_record() {
    // ONE RESPONSIBILITY, ONE HOME. The handle a caller drops and the keeper
    // that outlives it must reach the same state, or deferring that
    // destruction later would be deferring a copy of it.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8901);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let shared = cleanup_of(&registration);
    assert!(
        Arc::ptr_eq(pin.cleanup_record(), &shared),
        "the keeper reaches the registration's own record"
    );

    // THE SAME MUTABLE STATE, not a snapshot of it. A late lifecycle
    // attachment is written through the registration and read through the
    // keeper's handle.
    assert!(
        shared.lifecycle.lock().expect("readable").is_none(),
        "nothing is attached yet"
    );
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary admits and the lifecycle attaches");
    assert!(
        shared.lifecycle.lock().expect("readable").is_some(),
        "and the keeper's record sees what was attached afterwards"
    );

    // AND THE PLACE IS IN THAT ONE RECORD TOO.
    assert!(
        shared.maintenance_identity().is_some(),
        "its reservation lives here, not beside it"
    );
    assert!(Arc::ptr_eq(&shared.ordered_gate, &registration.handover_gate()));

    // A FRESH PIN AFTER THE FIRST VIEW ENDS FINDS THE SAME RECORD.
    drop(pin);
    let PrivateCustodyReach::Reached(again) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert!(Arc::ptr_eq(again.cleanup_record(), &shared));
    drop(again);
    drop((registration, private, keeper, durable, shared));
}

#[test]
fn a_refused_duplicate_leaves_no_cleanup_record_behind() {
    // THIS ATTEMPT'S OWN RESERVATIONS, AND ONLY THOSE. A duplicate is refused
    // while publication is excluded, so the attempt has a record of its own to
    // give back -- and the live connection it collided with keeps everything.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8902);
    let (first, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let live = cleanup_of(&first);
    let place = first.maintenance_identity().expect("a name").place();
    assert_eq!(keeper.custodies_kept(), 1);
    assert_eq!(durable.continuations_reserved(), Some(1));

    let registry = private.broker.registry.clone();
    let refused = std::thread::scope(|scope| {
        // THE REAL CLIENT TABLE, held by this control: the attempt below stops
        // exactly between preparing its record and publishing it.
        let table = registry.clients.lock().expect("a readable client table");
        let attempt = scope.spawn(|| {
            registry.register_client_with_admission(client, Some(admitted(client)))
        });
        assert!(
            waited_for(|| keeper.custodies_kept() == 2),
            "the attempt prepared its own custody and record before publication"
        );
        assert_eq!(
            durable.continuations_reserved(),
            Some(2),
            "and its own place, on the same reservation"
        );
        drop(table);
        attempt.join().expect("the attempt returned")
    });
    assert!(matches!(
        refused,
        Err(XServerFrontendRouteError::DuplicateClient { .. })
    ));

    // BOTH OF THAT ATTEMPT'S RESERVATIONS WENT BACK, AND THE LIVE SIBLING IS
    // UNTOUCHED -- including the record it is still holding.
    assert_eq!(keeper.custodies_kept(), 1);
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert!(Arc::ptr_eq(&cleanup_of(&first), &live));
    assert_eq!(
        live.maintenance_identity().expect("a name").place(),
        place,
        "the live connection's own place"
    );
    assert!(
        !first
            .ordered_handovers_fenced()
            .expect("a readable gate"),
        "and nothing ran its cleanup"
    );
    drop((first, live, private, registry, keeper, durable));
}

#[test]
fn keeping_a_cleanup_record_neither_runs_it_nor_repeats_it() {
    // KEEPING IS NOT RUNNING. The registration's own Drop performs this
    // connection's cleanup at exactly the point it always did; the keeper goes
    // on holding the record afterwards, and holding it does nothing.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8903);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let shared = cleanup_of(&registration);
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable client table")
            .contains_key(&client),
        "its row is in"
    );
    assert!(!shared.ordered_gate.fenced().expect("a readable gate"));

    // THE HANDLE GOES, AND THE CLEANUP RAN.
    drop(registration);
    assert!(
        shared
            .ordered_gate
            .fenced()
            .expect("a readable gate"),
        "its gate was closed by the cleanup its Drop performed"
    );
    assert!(
        !private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable client table")
            .contains_key(&client),
        "and its row is out"
    );
    assert_eq!(
        shared.ordered_home.standing(),
        PrivateHomeStanding::Retained,
        "its queue went to the place reserved for it"
    );

    // AND THE KEEPER STILL HAS THE RECORD, which changes nothing.
    assert_eq!(keeper.custodies_kept(), 1);
    let fenced_before = shared.ordered_gate.fenced();
    drop(shared);
    assert_eq!(
        keeper.custodies_kept(),
        1,
        "a record going out of one holder's hands is not a disposition"
    );
    assert_eq!(fenced_before, Some(true));
    drop((private, keeper, durable));
}

#[test]
fn a_connection_with_a_lifecycle_lease_closes_it_when_its_registration_goes() {
    // THE LEASE IS PRESENT THROUGH TEARDOWN, which is the case that moving its
    // home could have changed. It used to be a field of the handle, so its own
    // destructor ran at exactly that point; it now lives in a record that
    // outlives the handle, and the cleanup takes it rather than leaving it.
    //
    // WHAT THIS CONTROL SEPARATES. It shows the gate is closed when the
    // registration goes AND that the lease was taken rather than closed in
    // place: a cleanup that closed through a borrow would leave the lease in a
    // record its keeper still holds, and the assertion below is what refuses
    // that.
    //
    // WHAT IT DOES NOT SEPARATE is the explicit close from the disposal. This
    // lease's own `Drop` closes the same gate and closing is idempotent, so
    // dropping it without closing it first leaves the same observable state.
    // The explicit close is written for the reason the original had it, not
    // because this control can tell.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8904);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary admits and the lifecycle attaches");
    let shared = cleanup_of(&registration);
    let gate = shared
        .lifecycle
        .lock()
        .expect("a readable lifecycle")
        .as_ref()
        .expect("a lease is attached")
        .gate();
    assert!(gate.is_open(), "its lifecycle is open");

    drop(registration);
    assert!(
        !gate.is_open(),
        "the cleanup closed this connection's lifecycle"
    );
    assert!(
        shared
            .lifecycle
            .lock()
            .expect("a readable lifecycle")
            .is_none(),
        "and took the lease rather than leaving it for whenever the record goes"
    );
    drop((shared, private, keeper, durable));
}

#[test]
fn a_poisoned_lifecycle_cell_still_closes_this_connections_gate() {
    // WHAT THE HANDLE USED TO GET FOR FREE. The lease was a field of the
    // registration, so its destructor ran when the handle was destroyed and
    // closed the gate even on the path where the explicit close was skipped.
    // Once its home outlived the handle, leaving it in place on that path
    // meant the gate stayed open for as long as the keeper held the record.
    //
    // NO WORKER IS INVOLVED. The cell is poisoned by an ordinary caught panic
    // under its own lock, and nothing in it is rewritten.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8905);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary admits and the lifecycle attaches");
    let shared = cleanup_of(&registration);
    let gate = shared
        .lifecycle
        .lock()
        .expect("a readable lifecycle")
        .as_ref()
        .expect("a lease is attached")
        .gate();
    assert!(gate.is_open());

    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = shared.lifecycle.lock().expect("a readable lifecycle");
        panic!("poisoning this connection's lifecycle cell, and nothing else");
    }));
    assert!(poisoning.is_err());
    assert!(shared.lifecycle.is_poisoned());

    // ONLY THE REGISTRATION GOES. The keeper still holds the record, which is
    // exactly the condition that used to hide the missing disposal.
    drop(registration);
    assert!(
        !gate.is_open(),
        "its lifecycle was closed, which is what dropping the lease always did"
    );
    let still_held = match shared.lifecycle.lock() {
        Ok(held) => held.is_some(),
        Err(poisoned) => poisoned.into_inner().is_some(),
    };
    assert!(
        !still_held,
        "and the lease was taken, not left for whenever the record goes"
    );
    drop((shared, private, keeper, durable));
}

#[test]
fn a_connection_can_depart_before_anything_prepares_it() {
    // NO PREPARED ASSOCIATION IS REQUIRED. Forbidding a future start must not
    // mean entering this connection's home, inventing a stop, or making it
    // serve first -- a connection that never became serving is exactly one
    // that may need forbidding.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9001);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a source and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert_eq!(pin.startup_admitted(), Some(true));
    assert!(
        pin.prepare_control().is_err(),
        "nothing is bound: this connection never became serving"
    );

    assert_eq!(
        pin.depart_registered(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
    );
    assert_eq!(pin.startup_admitted(), Some(false));
    assert_eq!(
        pin.departure_observation(),
        Some(PrivateDeparture::NothingStarted),
        "and the connection keeps what was found, not the frame that asked"
    );

    // AND NOTHING WAS ENTERED TO DO IT. Its home is live and empty, its gate
    // is open, and its place is exactly where it was.
    assert_eq!(registration.ordered_home.standing(), PrivateHomeStanding::Live);
    assert!(!registration.ordered_handovers_fenced().expect("a readable gate"));
    assert_eq!(durable.continuations_reserved(), Some(1));
    drop(pin);
    drop((registration, private, keeper, durable));
}

#[test]
fn a_context_obtained_before_departure_cannot_start_afterwards() {
    // A CONTEXT IS STILL A CONTEXT; what it must not be is a way in. This one
    // is prepared first, departs through the source, and then tries to start.
    let f = worker_fixture(XServerFrontendClientId(9002));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");

    assert_eq!(
        custody.depart_registered(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
    );

    let called = std::sync::atomic::AtomicBool::new(false);
    let outcome = context.start(|| {
        called.store(true, std::sync::atomic::Ordering::Release);
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(outcome, PrivateStartupOutcome::NoLongerStartable);
    assert!(
        !called.load(std::sync::atomic::Ordering::Acquire),
        "a refused start calls no spawner, so no thread exists to be lost"
    );
    assert_eq!(
        custody.worker_slot().lock().expect("a readable slot").life,
        PrivateWorkerLife::NeverStarted
    );

    // AND ASKING FOR A CONTEXT AFTERWARDS DOES NOT REOPEN ANYTHING.
    //
    // WHAT THIS IS NOT. The association was already published by the context
    // above, so this ask returns at the entry check -- it is a second view,
    // not a resolution that began before the departure and finished after it.
    //
    // AND NO CONTROL HERE ARRANGES THAT SCHEDULE.
    // a_preparation_that_loses_a_race_recovers_what_the_winner_published
    // separates the two halves of a preparation, but it stages one preparation
    // against another and then poisons the home; it never departs and never
    // closes admission. A preparation resolving across a departure is not
    // covered by anything landed here.
    let later = custody.prepare_control().expect("its association stands");
    assert!(Arc::ptr_eq(later.stop(), context.stop()));
    assert_eq!(custody.startup_admitted(), Some(false));
    let outcome = later.start(|| std::thread::Builder::new().spawn(|| {}));
    assert_eq!(outcome, PrivateStartupOutcome::NoLongerStartable);
    drop(f.fixture);
}

#[test]
fn a_departure_reaching_an_admitted_start_stops_it_before_waiting() {
    // THE RACE THIS COMPONENT EXISTS FOR. A start is admitted and is inside
    // its transaction holding the destination; a departure arrives, finds the
    // pair that start published, stops the worker through it, and only then
    // waits for the slot.
    //
    // THE OVERLAP IS WITNESSED BY HOLDING THE SPAWNER, not by sleeping: the
    // start cannot leave its transaction while this control holds it, and the
    // stop is observed while that is true.
    let f = worker_fixture(XServerFrontendClientId(9003));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let (enter, entered) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();

    let (started, departed, witnessed) = std::thread::scope(|scope| {
        let context = &context;
        let custody = &custody;
        let starting = scope.spawn(move || {
            context.start(move || {
                enter.send(()).expect("its caller is waiting");
                let _ = released.recv();
                std::thread::Builder::new().spawn(|| {})
            })
        });
        entered
            .recv_timeout(Duration::from_secs(3))
            .expect("the spawn began and holds the destination");

        // THE PAIR IS ALREADY PUBLISHED, which is what makes the stop below
        // reachable without the home.
        let published = custody.published_stop().is_some();

        let departing = scope.spawn(move || custody.depart_registered());
        // OBSERVED INTO A LOCAL, NOT COMPARED HERE. This control has started a
        // worker whose handle is in the custody; a comparison that failed at
        // this point would leave that worker uncollected, and joining these
        // two helper threads is not collecting it.
        let stopped_while_held = waited_for(|| f.stop.load(std::sync::atomic::Ordering::Acquire));
        let witnessed = (
            published,
            stopped_while_held,
            custody.startup_admitted(),
        );
        drop(release);
        let started = starting.join().expect("the startup returned");
        let departed = departing.join().expect("the departure returned");
        (started, departed, witnessed)
    });

    // COLLECTED BEFORE THE COMPARISONS THIS CONTROL IS ABOUT. The worker it
    // started is joined here, through its own custody, and the stop and
    // outcome observations are compared after that.
    //
    // NOT A GENERAL FAILURE-PATH CLAIM. The setup, the channel receives and
    // the helper joins above can still fail on their own, and no cleanup guard
    // is armed for those.
    let record = PrivateReapingRecord::bound_to(&custody);
    let reaped = record.reap().reaped;

    assert_eq!(started, PrivateStartupOutcome::Started);
    assert_eq!(
        witnessed,
        (true, true, Some(false)),
        "the pair was published, the stop was set while the destination was \
         held, and admission was closed"
    );
    // EXACTLY WHAT THIS ARRANGEMENT ESTABLISHES. Nothing here hands the handle
    // on before the departure returns, and this control's own reaping is the
    // first joiner -- so WorkerHandedOn and HandedElsewhere are not outcomes
    // this fixture can reach, and accepting them would accept a schedule in
    // which it never collected its worker.
    assert_eq!(
        departed,
        PrivateDeparted::Decided(PrivateDeparture::WorkerRunning),
        "its handle was in the slot when the departure reached it"
    );
    assert_eq!(reaped, PrivateReaped::Joined);
    drop(record);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn one_departure_decision_is_recorded_however_many_ask() {
    // ONE DEPARTURE, ONE HISTORY. A second ask reports what the first
    // established rather than deciding again, and a view ending takes nothing
    // with it.
    let f = worker_fixture(XServerFrontendClientId(9004));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    // STARTED THROUGH THE REGISTERED SEAM, so this connection's pair is
    // published where a departure reaches it. The low-level transaction would
    // have made a worker without going through admission at all, which is not
    // what this protocol control is about.
    let starting = custody.prepare_control().expect("its own owner");
    assert_eq!(
        starting.start(|| std::thread::Builder::new().spawn(|| {})),
        PrivateStartupOutcome::Started
    );

    let first = custody.depart_registered();
    assert_eq!(
        first,
        PrivateDeparted::Decided(PrivateDeparture::WorkerRunning),
        "its handle was still in the slot when this looked"
    );
    for _ in 0..3 {
        assert_eq!(
            custody.depart_registered(),
            PrivateDeparted::AlreadyDecided(PrivateDeparture::WorkerRunning),
            "reported, not decided again"
        );
    }

    // AND THROUGH A CONTEXT PREPARED AFTERWARDS, which is another view of the
    // same connection and not another departure.
    let context = custody.prepare_control().expect("its own owner");
    assert_eq!(
        context.depart(),
        PrivateDeparted::AlreadyDecided(PrivateDeparture::WorkerRunning)
    );
    assert_eq!(
        custody.departure_observation(),
        Some(PrivateDeparture::WorkerRunning),
        "the first fact, which is the connection's"
    );

    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    drop(record);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_departure_leaves_a_same_store_sibling_exactly_as_it_was() {
    // ONE CONNECTION'S DECISION IS ONE CONNECTION'S. A sibling of the same
    // instance keeps its admission, its stop, its gate, its standing and the
    // exact work already accepted for it.
    let f = worker_fixture(XServerFrontendClientId(9005));
    let (sibling, sibling_stop, _sibling_notice, _peer) =
        serving_sibling(&f, XServerFrontendClientId(9006), true);
    let keeper = &f.fixture.keeper;
    let custody = custody_for(&f, keeper);
    let PrivateCustodyReach::Reached(sibling_custody) = sibling
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("the same owner keeps it")
    };

    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(90060);
    let delivery = capsule.delivery();
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let frames = order_pass_frames(&capsule);
    let sender = capture_gated_sender(
        f.fixture.runner.frontend.as_ref().expect("a live runner"),
        XServerFrontendClientId(9006),
    );
    produced_send(&sender, capsule);

    // COUNTED BEFORE, because this fixture holds more than this connection --
    // it keeps a spare registration of its own -- and what this control means
    // is that departing changes nothing, not that the total is any number.
    let reserved_before = f.fixture.durable.continuations_reserved();
    assert_eq!(
        custody.depart_registered(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
    );

    assert_eq!(
        sibling_custody.startup_admitted(),
        Some(true),
        "its sibling still admits a start"
    );
    assert_eq!(sibling_custody.departure_observation(), None);
    assert!(!sibling_stop.load(std::sync::atomic::Ordering::Acquire));
    assert!(!sibling.ordered_handovers_fenced().expect("a readable gate"));
    assert_eq!(sibling.ordered_home.standing(), PrivateHomeStanding::Live);
    let queued = sibling
        .ordered_home
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

    // AND THE DECISION CHANGED NO STANDING AND NO ACCOUNTING for either.
    assert_eq!(
        f.fixture.registration.ordered_home.standing(),
        PrivateHomeStanding::Live
    );
    assert_eq!(
        f.fixture.durable.continuations_reserved(),
        reserved_before,
        "no place was returned or taken by deciding"
    );
    drop(queued);
    drop(sender);
    drop((custody, sibling_custody));
    drop(sibling);
    drop(f.fixture);
}

#[test]
fn an_unreadable_admission_boundary_admits_nothing() {
    // FAIL CLOSED. A boundary nobody can read is exactly when letting a start
    // through would be worst: it would be restoring an eligibility nobody
    // established, on the one path where nothing can be established at all.
    //
    // The boundary is poisoned by an ordinary caught panic under its own lock,
    // and nothing in it is rewritten.
    let f = worker_fixture(XServerFrontendClientId(9007));
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    assert_eq!(custody.startup_admitted(), Some(true));

    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = custody
            .source
            .departure
            .lock()
            .expect("a readable boundary");
        panic!("poisoning this connection's admission boundary, and nothing else");
    }));
    assert!(poisoning.is_err());
    assert!(custody.source.departure.is_poisoned());

    let called = std::sync::atomic::AtomicBool::new(false);
    let outcome = context.start(|| {
        called.store(true, std::sync::atomic::Ordering::Release);
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(outcome, PrivateStartupOutcome::NoLongerStartable);
    assert!(
        !called.load(std::sync::atomic::Ordering::Acquire),
        "no spawner ran, so no thread exists that nothing admitted"
    );
    assert_eq!(
        custody.worker_slot().lock().expect("a readable slot").life,
        PrivateWorkerLife::NeverStarted
    );

    // AND A DEPARTURE SAYS IT ESTABLISHED NOTHING, rather than reporting an
    // absence it could not read.
    assert_eq!(custody.depart_registered(), PrivateDeparted::Unreadable);
    assert_eq!(custody.startup_admitted(), None);
    assert_eq!(custody.departure_observation(), None);
    drop(f.fixture);
}
