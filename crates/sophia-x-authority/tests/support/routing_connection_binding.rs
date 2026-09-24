// A connection established and serving: the records that bind it, its queue,
// and the retained continuation that outlives the instance that made it.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_retained_continuation_outlives_its_instance_and_goes_with_its_store() {
    // AN INSTANCE THAT ENDS DOES NOT TAKE THE STORE WITH IT. The constructor
    // takes it by reference and clones a holder, so a caller that keeps its
    // own holder still has the store -- and the work retained into it -- after
    // the instance is gone. That is what durable means.
    //
    // WHAT THIS DOES NOT ESTABLISH: anything about a ring. The connection here
    // disposes of its place at teardown, so no reservation is live when the
    // last holder goes, and a store that held a place-reference of its own
    // would not be caught by this. That is a separate audit, for a holder that
    // does not exist yet.
    let durable = PrivateSettlementOwner::default();
    let capability = durable.settlement_ref();
    let client = XServerFrontendClientId(8321);
    let (cell, survived, wire_weak, output_weak, pending_weak) = {
        let service_keeper = service_owner(&durable, 2);
        let private = private_over(&service_keeper, 2);
        let (registration, channels) = private
            .broker
            .registry
            .register_client_with_admission(client, Some(admitted(client)))
            .expect("a place and a row");
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
        let sender = capture_gated_sender(&private, client);
        let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(83210);
        let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
        gated_send(&sender, capsule).expect("an open endpoint");
        drop(sender);
        // The binding's own halves are weakly observed from here on, so what
        // the retained record holds can be asked about without holding it.
        let weaks = (
            Arc::downgrade(&wire),
            Arc::downgrade(&output),
            Arc::downgrade(&pending),
        );
        drop((wire, output, pending));

        // Teardown hands the queue into the place reserved for it.
        drop(registration);
        assert_eq!(durable.continuations_retained(), Some(1));

        // AND THEN THE INSTANCE ENDS, with the retained work still owed.
        drop(private);
        (cell, weaks.0.upgrade().is_some(), weaks.0, weaks.1, weaks.2)
    };
    assert!(
        survived,
        "an instance that ends does not take the retained binding with it"
    );
    assert_eq!(
        durable.continuations_retained(),
        Some(1),
        "the place is still this connection's after its instance has gone"
    );
    // THE PAYLOAD, not the counters: the exact capsule accepted before the
    // connection ended is still in the retained queue.
    let capsule = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Setup { accepted, .. } = continuation else {
                panic!("no owner is built yet")
            };
            let PrivateOrderedSetupCustody::Transport(transport) = accepted else {
                panic!("this connection bound")
            };
            transport.ordered.receiver.try_recv().ok()
        })
        .expect("the place this connection held")
        .expect("the capsule accepted before the connection ended");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(83210)
    );
    assert!(Arc::ptr_eq(
        &cell,
        &capsule.finalizer().expect("carried").completion
    ));
    drop((capsule, cell));

    // AND WHEN THE LAST HOLDER GOES, so does what it was holding. The place
    // was disposed of at teardown, so this is the store's own lifetime ending
    // and the binding being released with it -- not evidence about rings.
    assert!(capability.owner().is_some());
    drop(durable);
    assert!(capability.owner().is_none(), "no holder remains");
    assert!(
        wire_weak.upgrade().is_none()
            && output_weak.upgrade().is_none()
            && pending_weak.upgrade().is_none(),
        "a store that drops releases the binding it retained"
    );
}

#[test]
fn a_registration_that_ends_refuses_handovers_before_it_takes_its_queue_away() {
    // A CONNECTION THAT ENDS IS CLOSED TO HANDOVERS. Removing the row is not
    // what does it: the capture happens under the client table and the send
    // after it is released, so a sender already in a producer's hand outlives
    // the row.
    //
    // WHAT THIS ESTABLISHES is that teardown fences at all, and that what the
    // connection owed is in its place afterwards. It does NOT establish the
    // order of those two inside teardown: nothing is received during teardown,
    // so a capsule accepted between the take and the fence would land on the
    // same retained queue either way, and no outcome here separates them. The
    // order will become observable when a driver receives from that queue, and
    // it is written fence-first for that reason rather than for this one.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8311);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let sender = capture_gated_sender(&private, client);
    assert!(
        sender.admit().is_ok(),
        "before the registration ends, this captured sender admits"
    );
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

    drop(registration);
    assert_eq!(
        sender.admit().err(),
        Some(PrivateHandoverRefusal::Fenced),
        "a sender captured before the connection ended is refused after it"
    );
    assert!(
        retained_setup_kind(&durable, 0).is_some(),
        "and what it would have gone to is retained"
    );
}

#[test]
fn a_connection_whose_binding_refused_retains_its_queue_without_a_socket() {
    // RECEIVER ONLY. A transport carries an independent handle on the
    // connection, so retaining one keeps the wire open for whoever drives it.
    // A receiver alone carries no such handle: the accepted socket goes with
    // the connection that owned it, and the queue is retained with no way to
    // deliver what is in it. That is the worse outcome, and it is recorded as
    // what it is.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8321);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");

    // A receiver this registration did not mint: the binding refuses, and
    // refusing hands the receiver back rather than destroying it.
    let other = XServerFrontendClientId(8322);
    let (other_registration, other_channels) = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a second place and row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        registration
            .bind_ordered_output(other_channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        Some(X11OrderedServingRefusal::ForeignReceiver),
        "a receiver another registration minted is not this one's, and the \
         refusal does not destroy it"
    );
    drop(channels);

    drop(registration);
    let (kind, refusal, retained, drained, ended) =
        retained_setup_kind(&durable, 0).expect("the place this connection held");
    assert_eq!(kind, "receiver", "there is no transport to retain");
    assert_eq!(
        refusal,
        X11OrderedServingRefusal::ForeignReceiver,
        "THE CAUSE IS THE ONE THAT HAPPENED. Writing 'never served' over a \
         binding that actually refused would send whoever inherits this queue \
         looking for a worker that was never the problem"
    );
    assert_eq!((retained, drained, ended), (0, false, false));
    assert_eq!(
        durable.continuations_reserved(),
        Some(2),
        "both connections still hold their places"
    );
    drop(other_registration);
}

#[test]
fn a_second_binding_is_refused_rather_than_replacing_the_first() {
    // The first custody may already hold accepted capsules. Replacing it would
    // discard them with nothing recording that they existed, so the second is
    // handed back to whoever offered it.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8331);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("the first custody"));

    let second = XServerFrontendClientId(8332);
    let (second_registration, second_channels) = private
        .broker
        .registry
        .register_client_with_admission(second, Some(admitted(second)))
        .expect("a second place and row");
    let Err(returned) = registration.bind_ordered_output(
        second_channels.ordered,
        &output,
        &wire,
        &pending,
    ) else {
        panic!("a second binding is refused")
    };
    assert!(
        returned.minted_by(&second_registration),
        "and the receiver comes back whole, still the one its own \
         registration minted, rather than being dropped"
    );
    drop(returned);
    drop(registration);
    assert_eq!(
        retained_setup_kind(&durable, 0).map(|kind| kind.0),
        Some("transport"),
        "the first custody is what was retained"
    );
    drop(second_registration);
}


#[test]
fn a_receiver_only_connection_never_reports_an_ended_wire() {
    // HAVING NO HANDLE IS NOT HAVING ENDED SOMETHING. A binding that refused
    // leaves a queue and no way to reach the connection: the accepted socket
    // is still open and its peer still waiting. A visit that recorded `ended`
    // because there was nothing to end with let the record read as settled and
    // handed the place back over a live wire with accepted work still on it.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8341);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let other = XServerFrontendClientId(8342);
    let (other_registration, other_channels) = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a second place and row");

    // A real refusal: a receiver another registration minted.
    let (stream, peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        registration
            .bind_ordered_output(other_channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        Some(X11OrderedServingRefusal::ForeignReceiver)
    );
    drop(channels);
    drop(registration);

    // Driven as hard as anything drives it. The record can never report an
    // ending, so it never settles and its place never comes back.
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    let (kind, _refusal, _retained, _drained, ended) =
        retained_setup_kind(&durable, 0).expect("the place this connection held");
    assert_eq!(kind, "receiver");
    assert!(
        !ended,
        "nothing here has touched that socket, so nothing may say it was ended"
    );
    assert!(
        !durable
            .with_ordered_continuation(0, |continuation| continuation.settled())
            .expect("the place holds it"),
        "a record that cannot establish an ending is not a settled connection"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(2),
        "and its place is not handed back"
    );

    // The accepted socket is still open, which is the fact the record is
    // refusing to misreport.
    peer.set_nonblocking(true).expect("a readable peer");
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "the peer is still connected and still waiting"
    );
    drop(other_registration);
}

#[test]
fn a_second_binding_hands_back_the_receiver_it_was_offered() {
    // REACHABLE, and it took two steps to reach: offer this registration a
    // receiver it did not mint -- refused, and retained as custody -- then
    // offer it its own. The second binding SUCCEEDS and retention refuses it,
    // which is the arm that hands a bound transport's receiver back out.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let holder = XServerFrontendClientId(8351);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(holder, Some(admitted(holder)))
        .expect("a place and a row");
    let other = XServerFrontendClientId(8352);
    let (other_registration, other_channels) = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a second place and row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));

    // First: a foreign receiver. Refused, and retained with that cause.
    assert_eq!(
        registration
            .bind_ordered_output(other_channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        Some(X11OrderedServingRefusal::ForeignReceiver)
    );

    // Second: its own receiver. The binding is fine; the custody is not free.
    let Err(returned) =
        registration.bind_ordered_output(channels.ordered, &output, &wire, &pending)
    else {
        panic!("this registration already holds custody")
    };
    assert!(
        returned.minted_by(&registration),
        "the receiver handed back is the one that was offered, whole"
    );

    // The first custody is untouched: it is the foreign receiver, with the
    // cause that stopped it.
    drop(returned);
    drop(registration);
    let (kind, refusal, ..) = retained_setup_kind(&durable, 0).expect("the retained place");
    assert_eq!(kind, "receiver");
    assert_eq!(
        refusal,
        X11OrderedServingRefusal::ForeignReceiver,
        "the first custody stayed, with the reason it was first refused"
    );
    drop(other_registration);
}

#[test]
fn a_bound_connection_keeps_its_queue_through_a_later_setup_refusal() {
    // THE BINDING COMES BEFORE THE FIRST THING THAT CAN REFUSE. Connection
    // setup publishes the row, then attaches lifecycle, connection state and
    // recovery -- each of which can fail. A binding placed after them left
    // every one of those refusals dropping the receiver, and the reserved
    // place survived holding nothing. Here the binding has happened, and the
    // connection then ends the way a refusal ends it.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8361);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));

    // Accepted after the row was published, which is the window that matters.
    let sender = capture_gated_sender(&private, client);
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(83610);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    gated_send(&sender, capsule).expect("an open endpoint");

    // A refusal after this point unwinds the connection: the registration
    // drops, and so does everything the caller still held.
    drop(registration);
    drop(private);

    let survived = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Setup { accepted, .. } = continuation else {
                panic!("no owner is built yet")
            };
            let PrivateOrderedSetupCustody::Transport(transport) = accepted else {
                panic!("this connection bound")
            };
            transport.ordered.receiver.try_recv().ok()
        })
        .expect("the place this connection held")
        .expect("the capsule accepted before the refusal");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83610)
    );
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(
        cell.answer().is_none(),
        "nobody answered for it, which is why losing it was losing something"
    );
}


/// The fence a retained record carries, read from the place it went into.
fn retained_fence(
    durable: &PrivateSettlementOwner,
    index: usize,
) -> Option<Option<PrivateHandoverFence>> {
    durable.with_ordered_continuation(index, |continuation| match continuation {
        PrivateOrderedContinuation::Setup { evidence, .. }
        | PrivateOrderedContinuation::Serving { evidence, .. } => evidence.fence,
    })
}

#[test]
fn a_retained_connection_carries_what_closing_it_established() {
    // THE OUTCOME TRAVELS WITH THE WORK. The gate belongs to the registration
    // that minted it and the registration is gone by the time anything drives
    // this record, so the one moment the closure could be established is the
    // moment it was. Asking later is not available; carrying it is.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8391);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));

    drop(registration);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Established)),
        "teardown closed this endpoint, and the record says so"
    );
}

#[test]
fn a_connection_closed_over_a_panicking_handover_never_reads_as_settled() {
    // A FINISHED CHANNEL IS NOT A RESOLVED HANDOVER. Drained is set from
    // Disconnected, so it already establishes that no later send is possible;
    // that is not what the fence is for. The fence carries evidence about the
    // other side: someone panicked inside the gate, so the closure could not
    // be established over resolved custody, and a record carrying that must
    // not read as finished however quiet its queue goes.
    //
    // Exclusion is not what failed. A poisoned lock is acquired and handed
    // back inside the error, so nothing was running beside the close.
    //
    // WHAT THIS CONTROL ESTABLISHES: that such a close is classified as
    // unreadable, carried onto the record, and keeps the place. It does NOT
    // exercise an interrupted capsule transfer -- it holds the gate's own lock
    // and sends nothing -- so it says nothing about recovering a half-made
    // handover, which is open work.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8401);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));

    // A producer panics inside the gate, the way one would if a handover
    // failed mid-flight.
    let gate = registration.ordered_gate.clone();
    let holder = std::thread::spawn(move || {
        let _inside = gate.fenced.lock().expect("an open gate");
        panic!("a handover panicked inside this gate");
    });
    assert!(holder.join().is_err(), "the holder panicked");

    drop(registration);
    drop(private);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Unreadable)),
        "the close could not be established, and the record carries that"
    );

    // Everything else about this record says finished: the producers are gone
    // with the registry, the wire ends on the first visit, and nothing was
    // kept back.
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    let (_kind, _refusal, retained, drained, ended) =
        retained_setup_kind(&durable, 0).expect("the place this connection held");
    assert_eq!(
        (retained, drained, ended),
        (0, true, true),
        "drained, ended, and nothing retained"
    );
    assert!(
        !durable
            .with_ordered_continuation(0, |continuation| continuation.settled())
            .expect("the place holds it"),
        "and still not settled, because the closure was never established"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "so the place stays held"
    );
}

#[test]
fn a_connection_closed_cleanly_settles_once_its_work_is_gone() {
    // The same record with an established closure does finish, so the rule
    // above is the fence and not some other thing keeping the place.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8411);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));

    drop(registration);
    drop(private);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Established))
    );
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "an established closure over work that is gone returns the place"
    );
}


#[test]
fn a_serving_record_without_an_established_closure_never_settles_either() {
    // THE EVIDENCE MUST SURVIVE THE CONVERSION. A serving owner answers for
    // what it is holding, and its own termination is a different fact from the
    // endpoint's closure: one says this writer finished, the other says
    // nothing was left half-handed-over on the way to it. A conversion that
    // dropped the closure would let the owner's termination speak for both.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8421));
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    // Its producers go, and then it closes: finished by its own account --
    // nothing held, nothing in flight, an established termination over a
    // drained queue.
    drop(f);
    close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        owner.closing().is_some_and(|closing| {
            closing.termination == X11OrderedTermination::Established && closing.drained
        }),
        "this owner terminated and its queue finished"
    );
    assert!(owner.retained_unanswered().is_empty() && owner.in_flight().is_none());

    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let mut source = Some(PrivateOrderedContinuation::Serving {
        owner: home_holding(owner),
        // STAGED, NOT OBSERVED. No holder panicked here and no teardown ran:
        // this is the value such a record would arrive carrying, set directly
        // so the guard below is about what a record with it does. What writes
        // this value for real is teardown, and the controls named
        // teardown_records_* are where that is established.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Unreadable),
            ..PrivateOrderedEvidence::unstarted()
        },
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    assert!(
        !durable
            .with_ordered_continuation(0, |continuation| continuation.settled())
            .expect("the place holds it"),
        "an owner that finished is not a connection whose handovers resolved"
    );
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "so the place stays held"
    );
}

#[test]
fn a_closure_someone_else_made_is_carried_as_already_established() {
    // None IS NOT "THE ENDPOINT IS OPEN". Closing is a public act: a caller
    // that closed this endpoint directly leaves the record saying None until
    // teardown writes what ITS close established -- which is then
    // AlreadyEstablished, because the closure was already made.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8431);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));

    // Closed by a caller, through the real API, while the registration lives.
    assert_eq!(
        registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    assert_eq!(
        registration
            .ordered_home
            .borrow(|continuation| match continuation {
                PrivateOrderedContinuation::Setup { evidence, .. } => evidence.fence,
                PrivateOrderedContinuation::Serving { evidence, .. } => evidence.fence,
            }),
        Some(None),
        "the record still says None: no teardown outcome has been recorded here"
    );

    drop(registration);
    drop(private);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::AlreadyEstablished)),
        "teardown's own close found the closure already made, and says so"
    );
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "an already-established closure is a closure, so this place returns"
    );
}


/// A serving owner over this connection's OWN receiver and socket.
///
/// No spare registration is borrowed for a receiver, so the places in the
/// store belong to this connection alone and what a teardown does to them is
/// readable. The conversion that would build one of these in production is not
/// wired; this stands in for its hand-in, and for nothing after it -- the
/// teardown that follows is the real one, and is the subject.
// The service keeper comes back too, for the reason `bound_connection` gives.
fn serving_custody_for(
    f: PreparedOrderedFixture,
) -> (
    X11OrderedServingOwner,
    XServerFrontendClientRouteRegistration,
    PrivatePreparedRunner,
    PrivateSettlementOwner,
    Arc<Mutex<X11ClientOutput>>,
    crate::PrivateServiceOwner,
) {
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(socket, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let PreparedOrderedFixture {
        registration,
        runner,
        channels,
        durable,
        keeper,
        ..
    } = f;
    let transport = XAuthorityOrderedTransport::bind(
        &registration,
        channels.ordered,
        &output,
        &wire,
        &pending,
        None,
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let owner = X11OrderedServingOwner::for_registration(
        runner.frontend.as_ref().expect("a live runner"),
        &registration,
        transport,
    )
    .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));
    (owner, registration, runner, durable, output, keeper)
}

#[test]
fn teardown_records_its_actual_close_on_a_serving_record_too() {
    // A RECORD THAT REACHES A PLACE WITHOUT THIS CARRIES None FOR EVER.
    // Nothing after installation can establish a closure -- the gate goes with
    // the registration -- so a serving record left unwritten could never
    // settle however finished it was, and its place would never come back.
    let f = prepared_ordered_fixture(XServerFrontendClientId(8441));
    let (owner, registration, runner, durable, _output, _keeper) = serving_custody_for(f);
    registration
        .retain_ordered_setup(PrivateOrderedContinuation::Serving {
            owner: home_holding(owner),
            // A STAGED PRECONDITION, not an observation: no close has happened
            // yet. What this control is about is what teardown writes over it.
            evidence: PrivateOrderedEvidence::unstarted(),
        })
        .unwrap_or_else(|_| panic!("this registration holds no custody yet"));

    // Its producers go, and then the real teardown runs.
    drop(runner);
    drop(registration);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Established)),
        "the actual teardown wrote what its own close established"
    );

    // And because it did, a record with nothing left can finish.
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "a finished serving record with an established closure returns its place"
    );
}

#[test]
fn teardown_records_an_unreadable_close_on_a_serving_record() {
    // The other outcome through the same seam. A holder panicked inside the
    // gate, so teardown's close establishes nothing, and the record says so
    // rather than defaulting to success.
    let f = prepared_ordered_fixture(XServerFrontendClientId(8451));
    let (owner, registration, runner, durable, _output, _keeper) = serving_custody_for(f);
    registration
        .retain_ordered_setup(PrivateOrderedContinuation::Serving {
            owner: home_holding(owner),
            evidence: PrivateOrderedEvidence::unstarted(),
        })
        .unwrap_or_else(|_| panic!("this registration holds no custody yet"));

    let gate = registration.ordered_gate.clone();
    let holder = std::thread::spawn(move || {
        let _inside = gate.fenced.lock().expect("an open gate");
        panic!("a handover panicked inside this gate");
    });
    assert!(holder.join().is_err(), "the holder panicked");

    drop(runner);
    drop(registration);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Unreadable)),
        "teardown established nothing, and wrote that rather than a success"
    );
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "so this record keeps its place"
    );
}

#[test]
fn teardown_records_an_already_established_close_on_a_serving_record() {
    // The third outcome through the same seam: the endpoint was closed by a
    // caller before teardown reached it, and teardown says what it found.
    let f = prepared_ordered_fixture(XServerFrontendClientId(8461));
    let (owner, registration, runner, durable, _output, _keeper) = serving_custody_for(f);
    registration
        .retain_ordered_setup(PrivateOrderedContinuation::Serving {
            owner: home_holding(owner),
            evidence: PrivateOrderedEvidence::unstarted(),
        })
        .unwrap_or_else(|_| panic!("this registration holds no custody yet"));
    assert_eq!(
        registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established,
        "closed through the real API, before teardown"
    );

    drop(runner);
    drop(registration);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::AlreadyEstablished))
    );
}


#[test]
fn a_connections_place_coming_back_answers_nothing_for_an_interrupted_handover() {
    // THE TWO ACCOUNTINGS ARE SEPARATE, and this is where that matters most.
    // A handover interrupted between taking the capsule out of custody and
    // handing it over leaves a delivery nobody can resolve from here: the slot
    // is empty, the phase says a handover may have begun, and whether the
    // capsule reached the queue is not knowable from either side. Meanwhile
    // the CONNECTION can finish perfectly well -- its producers go, its wire
    // ends, its closure is established -- and its place comes back.
    //
    // WHAT THIS ESTABLISHES: the delivery is answered by the client going, and
    // answered as that; the place returning neither produces that answer nor
    // revises it; and nothing anywhere rebuilds or re-offers the capsule.
    //
    // WHAT IT DOES NOT ESTABLISH, and the precondition is the reason: the
    // capsule is taken and dropped here OUTSIDE the gate, which leaves the
    // gate healthy, so teardown establishes a closure and the place comes
    // back. A real unwind is not like that -- a producer is INSIDE the gate
    // from before the take until after the owned report, so unwinding there
    // poisons it, teardown then establishes nothing, and the place is kept.
    // The control below carries that state instead. This one is about the
    // separation of accountings under a supplied clean fence, and nothing it
    // says applies to a handover that actually unwound.
    //
    // WHAT IS STAGED IS THE CUSTODY HALF, under a supplied healthy gate. It is
    // not the state an unwind leaves: an unwind is inside the gate and poisons
    // it, and the control below carries that composite. This one sets the
    // custody side directly -- an empty slot under a phase that says a
    // handover may have begun -- and leaves the gate untouched on purpose, so
    // the separation it is about can be read without the closure question
    // mixed into it. What is real is everything after: the producer's own
    // refusal to re-offer, the teardown, the drive, and the answer.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8491));
    attempt_run(&mut f, 84910, 272, true);
    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 84910);
    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    {
        let record = &mut private.terminal.holds[0];
        let emission = record
            .native
            .as_mut()
            .unwrap()
            .take_press_emission()
            .expect("its own press emission");
        PrivateXServerFrontend::stow_press_capsule(
            &mut record.custody,
            emission,
            &recovery,
            f.client,
        );
        // The write-ahead, then the take -- and then nothing, which is the
        // interruption.
        record.custody.dispatch = PrivateDispatchPhase::Indeterminate;
        let taken = record.custody.pending.take();
        assert!(
            matches!(taken, Some(PrivatePendingDelivery::Capsule(_))),
            "there was a capsule, and this is where it is lost"
        );
        drop(taken);
    }

    // THE PRODUCER NEVER OFFERS IT AGAIN. The phase authorises, not the slot,
    // and an empty slot under Indeterminate is exactly what an interrupted
    // handover looks like from here.
    for _ in 0..8 {
        f.runner
            .frontend
            .as_mut()
            .unwrap()
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the executor keeps running");
    }
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "nothing is rebuilt and nothing is re-offered"
    );
    let private = f.runner.frontend.as_ref().unwrap();
    assert_eq!(
        handover_phase(private, &cell),
        Some(PrivateDispatchPhase::Indeterminate),
        "and the phase still says a handover may have begun"
    );
    assert!(cell.answer().is_none(), "nobody has answered for it");

    // Now the connection ends and finishes: bound, torn down, closed,
    // producers gone, wire ended.
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let PreparedOrderedFixture {
        registration,
        runner,
        channels,
        durable,
        ..
    } = f;
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a registration that holds no custody yet"));
    drop(runner);
    drop(registration);
    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Established))
    );

    // WHAT ANSWERED IT IS THE DISCONNECT, AND WHAT IT SAYS IS THE DISCONNECT.
    // The client is gone, so this delivery can never be confirmed; recording
    // that is an honest terminal outcome and not a receipt. Nothing claims it
    // was written, flushed or seen.
    let answer = cell
        .answer()
        .expect("the client going is itself an outcome for what it was owed");
    assert_eq!(answer.delivery, XAuthorityInputDeliveryId::from_raw(84910));
    assert_eq!(
        answer.outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        "the outcome is that the client went, not that anything reached it"
    );

    // AND THE PLACE COMING BACK CHANGES NOTHING ABOUT IT. A place is about a
    // connection's ordered output; it is not an answer, and it does not revise
    // one. This is the separation: two accountings, each finishing on its own
    // evidence, neither standing in for the other.
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "the connection's own accounting completes: its place comes back"
    );
    let after = cell.answer().expect("still answered, once");
    assert_eq!(after.outcome, answer.outcome, "and answered the same way");
    assert_eq!(after.delivery, answer.delivery);
}


#[test]
fn a_handover_that_unwound_inside_the_gate_keeps_its_place() {
    // WHAT AN ACTUAL UNWIND LEAVES. A producer is inside the gate from before
    // it takes the capsule until after it writes down what came back, so an
    // unwind anywhere in there poisons the gate as it goes. Teardown then
    // establishes no closure, and the place is KEPT -- the opposite of the
    // clean-fence case beside this one, and the reason that case cannot be
    // read as covering this one.
    //
    // BOTH HALVES ARE STAGED HERE, and deliberately together, because that is
    // what makes the state the one an unwind produces: the interrupted custody
    // AND the poisoned gate. This crate cannot unwind a producer mid-call
    // without a hook; what it can do is refuse to pretend that half the state
    // is the whole of it.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8501));
    attempt_run(&mut f, 85010, 272, true);
    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 85010);
    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    {
        let record = &mut private.terminal.holds[0];
        let emission = record
            .native
            .as_mut()
            .unwrap()
            .take_press_emission()
            .expect("its own press emission");
        PrivateXServerFrontend::stow_press_capsule(
            &mut record.custody,
            emission,
            &recovery,
            f.client,
        );
        record.custody.dispatch = PrivateDispatchPhase::Indeterminate;
        drop(record.custody.pending.take());
    }
    // The other half: a producer that unwound inside the gate leaves it
    // poisoned.
    let gate = f.registration.ordered_gate.clone();
    let holder = std::thread::spawn(move || {
        let _inside = gate.fenced.lock().expect("an open gate");
        panic!("a handover unwound inside this gate");
    });
    assert!(holder.join().is_err(), "the holder unwound");

    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let PreparedOrderedFixture {
        registration,
        runner,
        channels,
        durable,
        ..
    } = f;
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a registration that holds no custody yet"));
    drop(runner);
    drop(registration);

    assert_eq!(
        retained_fence(&durable, 0),
        Some(Some(PrivateHandoverFence::Unreadable)),
        "teardown established nothing, because the gate had been unwound through"
    );
    let answer = cell.answer().expect("the client going is still an outcome");
    assert_eq!(
        answer.outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        "answered as the client going, exactly as in the clean case"
    );
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    let (_kind, _refusal, retained, drained, ended) =
        retained_setup_kind(&durable, 0).expect("the place this connection held");
    assert_eq!(
        (retained, drained, ended),
        (0, true, true),
        "its queue finished and its wire ended, as far as those go"
    );
    assert!(
        !durable
            .with_ordered_continuation(0, |continuation| continuation.settled())
            .expect("the place holds it")
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "AND THE PLACE STAYS HELD. An unwound handover is not a finished one, \
         however quiet everything around it goes"
    );
}


#[test]
fn a_release_meeting_a_gone_receiver_keeps_its_capsule_and_gives_the_attempt_back() {
    // KNOWN NOT ENQUEUED, the other way. A full queue and a gone receiver are
    // both exact answers from the channel: neither took the capsule, so both
    // leave it offerable and both give the attempt back. The existing control
    // for an unplaceable attempt drops the whole registration, so it refuses
    // at the row lookup before the gate; this one keeps the row and the gate
    // and lets the SEND be what refuses.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8511));
    attempt_release(&mut f, 85110, 272);
    {
        let private = f.runner.frontend.as_mut().unwrap();
        assert_eq!(private.dispatch_one_press(), Some(true));
        assert_eq!(private.record_one_native(), Some(true));
    }
    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 85111);

    // Its receiver goes while its row stays. A spare registration supplies a
    // receiver to put in its place so the fixture stays whole; the spare's own
    // row goes with it and is nothing to do with this claim.
    let spare = XServerFrontendClientId(f.client.raw() + 900_000);
    let spare_ordered = f
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .register_client_with_admission(spare, Some(admitted(spare)))
        .expect("a spare registration to borrow a receiver from")
        .1
        .ordered;
    drop(std::mem::replace(&mut f.channels.ordered, spare_ordered));

    let private = f.runner.frontend.as_mut().unwrap();
    let index = private
        .terminal
        .settling
        .iter()
        .position(|release| release.completion().is_some_and(|held| Arc::ptr_eq(held, &cell)))
        .expect("the release this control is about");
    assert_eq!(
        private.attempt_one_delivery(),
        Some(false),
        "the send itself refused: there is no receiver left"
    );

    // EXACTLY WHAT A FULL QUEUE LEAVES. The capsule is the one the release
    // decided -- not rebuilt, not reselected -- the phase says it is offerable
    // again, and the attempt went back rather than being spent on a delivery
    // nobody will make.
    assert_eq!(
        handover_phase(private, &cell),
        Some(PrivateDispatchPhase::Pending),
        "known not enqueued, so it may be offered again"
    );
    match private.terminal.settling[index].custody.pending.as_ref() {
        Some(PrivatePendingDelivery::Capsule(capsule)) => {
            assert_eq!(
                capsule.delivery(),
                XAuthorityInputDeliveryId::from_raw(85111)
            );
            assert!(Arc::ptr_eq(&cell, &capsule.finalizer().unwrap().completion));
        }
        _ => panic!("the refusal handed the exact capsule back"),
    }
    assert!(
        private.terminal.settling[index].custody.attempt.is_none(),
        "the record stops naming an attempt once the ledger confirmed it back"
    );
    assert!(
        private.terminal.attempt_custody.is_none(),
        "and nothing holds the ledger's slot for a delivery nobody will make"
    );
    assert!(
        cell.answer().is_none(),
        "a refused send answers nothing: the debt is exactly as owed as before"
    );
}
