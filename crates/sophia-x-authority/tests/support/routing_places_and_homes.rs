// Places, stores and homes: where a connection's output lives, which credit
// accounts for a place, and what an operation keeps until it ends.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


/// A registration bound to a real socket, with one capsule accepted for it.
///
/// The connection's own reservation, its published queue and its binding, all
/// through production construction -- the shape every control below starts
/// from. The payload is in the home the reservation made from the moment the
/// binding lands; nothing in these controls puts it there.
/// A converted connection, its instance, and the owner that keeps both.
type ConvertedFixture = (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteRegistration,
    Arc<PrivateDeliveryCompletion>,
    Vec<Vec<u8>>,
    std::sync::Weak<X11WirePermission>,
    crate::PrivateServiceOwner,
);

fn converted_fixture(
    durable: &PrivateSettlementOwner,
    client: XServerFrontendClientId,
    delivery: u64,
) -> ConvertedFixture {
    // The owner is handed back rather than kept here: one that died as this
    // returned would leave the caller holding a service with no keeper.
    let service_keeper = service_owner(durable, 2);
    let private = private_over(&service_keeper, 2);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = Arc::new(Mutex::new(stream));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        registration
            .bind_ordered_output(channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        None
    );
    let sender = capture_gated_sender(&private, client);
    // A fixture capsule carrying its own completion: work on this queue, with
    // no claim that this recipient would have admitted it.
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(delivery);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let frames = order_pass_frames(&capsule);
    gated_send(&sender, capsule).expect("an open endpoint");
    let wire_weak = Arc::downgrade(&wire);
    (private, registration, cell, frames, wire_weak, service_keeper)
}

/// The place a registration reserved, taken without ending the connection.
fn lease_of(
    registration: &XServerFrontendClientRouteRegistration,
) -> PrivateOrderedContinuationSlot {
    registration
        .ordered_continuation
        .lock()
        .expect("a readable registration")
        .take()
        .expect("the place reserved before this connection was exposed")
}

/// Wait for a delivery's completion to be published on its own cell.
///
/// A FINISHED WRITE IS NOT A RECEIPT. The step that puts the last byte of an
/// event on the wire can return Advanced; finalising it is the step after
/// that. A control that read the bytes and then stopped the body would be
/// asking for a receipt nobody had written yet -- and bytes, a stop and a join
/// cannot supply one between them.
///
/// Bounded, because a control that never finishes reports nothing.
fn published(cell: &Arc<PrivateDeliveryCompletion>) -> bool {
    waited_for(|| cell.answer().is_some())
}

/// Poll for an observation, bounded.
///
/// THE BOUND IS NOT THE EVIDENCE. What a caller establishes is whatever it
/// asked about, and this only decides how long to keep asking before giving
/// up; a control that never finished would report nothing at all. Yielding
/// alone is not enough on a machine running the rest of the suite beside it --
/// a spin can exhaust itself while the thread it is waiting on has not been
/// scheduled.
fn waited_for(mut observed: impl FnMut() -> bool) -> bool {
    for _ in 0..3_000 {
        if observed() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

/// Whether a body left because this control cancelled it.
///
/// TWO LEGAL ANSWERS, and which one a run gives is the scheduler's. A body
/// told to stop before its own check departs itself; one told between that
/// check and its visit has the OWNER observe the stop, and the trigger is then
/// the owner's step. Both carry the owner's own word, which is the fact worth
/// asserting -- the trigger alone is a race.
fn stopped_by_cancellation(outcome: &PrivateWorkerOutcome) -> bool {
    matches!(
        outcome.trigger,
        PrivateWorkerTrigger::Stopped | PrivateWorkerTrigger::OwnerStep
    ) && outcome.last == Some(PrivateWorkerAsk::Said(X11OrderedServeStep::Stopped))
}

/// Read one capsule out of the place a credit names, through the credit.
fn credit_receives(
    credit: &PrivateInternalCredit,
) -> PrivateCreditReach<Option<XAuthorityOrderedDelivery>> {
    credit.with_place(|continuation| continuation.queue().try_recv().ok())
}

/// Give this connection's place to a holder the store keeps, through the real
/// two-step conversion.
fn hand_place_to_store(
    durable: &PrivateSettlementOwner,
    registration: &XServerFrontendClientRouteRegistration,
) -> PrivateSettlementOwner {
    let lease = lease_of(registration);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("a declared bound leaves a holder place");
    let outer = durable.clone();
    assert!(matches!(
        lease.convert_to_internal(&outer, destination),
        PrivateInternalConversion::Held
    ));
    outer
}

#[test]
fn a_connections_output_lives_in_a_home_its_registration_does_not_own() {
    // THIS IS WHAT THE RELOCATION IS FOR. The payload used to live in the
    // registration, and the only thing that could reach it while the
    // connection ran was the registration itself; teardown then moved it into
    // the place. Anything meaning to borrow it later had to be handed the
    // payload rather than a way to reach it, and the hand-over invalidated
    // whatever it was holding.
    //
    // Here the home is made with the place, before the connection is exposed,
    // and the registration and the place hold the same one. So the exact
    // capsule is readable from the place WHILE the connection runs, and still
    // readable from the same home after the registration is destroyed --
    // without anything having been moved in between.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8381);
    let (private, registration, cell, frames, wire_weak, _private_keeper) =
        converted_fixture(&durable, client, 83810);

    // THE SAME HOME, not a copy: the registration's handle and the place's are
    // one object, which is what makes a borrow from either a borrow of the
    // same work.
    let from_place = {
        let held = durable.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(home) = &held.continuations[0] else {
            panic!("its place holds its home")
        };
        Arc::clone(home)
    };
    assert!(Arc::ptr_eq(&from_place, &registration.ordered_home));
    assert!(from_place.occupied(), "bound, and in its home from binding");
    assert_eq!(from_place.standing(), PrivateHomeStanding::Live);
    assert_eq!(
        durable.continuations_retained(),
        Some(0),
        "a live connection's home is not retained work"
    );

    // AND THE CONNECTION ENDS. Nothing moves; what changes is its standing.
    drop(registration);
    assert_eq!(from_place.standing(), PrivateHomeStanding::Retained);
    assert_eq!(durable.continuations_retained(), Some(1));
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert_eq!(durable.continuations_abandoned(), Some(0));

    // The exact capsule is in the same home it was accepted into, and the
    // binding is still there.
    let survived = from_place
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("the home its connection bound")
        .expect("the capsule accepted before it ended");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83810)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none());
    assert!(wire_weak.upgrade().is_some());

    // Teardown's own evidence is in the home, written where the payload lives.
    assert!(
        from_place
            .borrow(|continuation| matches!(
                continuation,
                PrivateOrderedContinuation::Setup { evidence, .. }
                    if evidence.fence.is_some()
                        && evidence.worker == PrivateOrderedWorkerExit::NeverStarted
            ))
            .expect("the home its connection bound"),
        "the fence it closed and the worker it never started"
    );
    drop((private, survived, cell, from_place));
}

#[test]
fn a_live_connections_home_is_not_driven_by_the_retained_drive() {
    // A DRIVE THAT FINISHES WHAT CONNECTIONS LEFT BEHIND MUST NOT TOUCH ONE
    // THAT HAS NOT LEFT. Now that a connection's output is in its place from
    // binding rather than from teardown, the drive can see it -- and visiting
    // it would receive from a queue a producer may still be using, end a wire
    // the connection is still serving on, and hand the result to whoever is
    // borrowing it. Standing is what separates them.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8382);
    let (private, registration, cell, frames, wire_weak, _private_keeper) =
        converted_fixture(&durable, client, 83820);
    assert_eq!(durable.continuations_reserved(), Some(1));

    for _ in 0..8 {
        assert_eq!(
            durable.drive_ordered_continuations(4),
            0,
            "there is nothing retained to drive"
        );
    }

    // UNTOUCHED, AND THE PAYLOAD SAYS SO, not only the flags: the exact
    // capsule is still on the queue with its frames and its unanswered
    // completion. Flags alone would hold equally over a queue the drive had
    // received from and thrown away.
    assert!(
        registration
            .ordered_home
            .borrow(|continuation| matches!(
                continuation,
                PrivateOrderedContinuation::Setup { ended, drained, .. }
                    if !*ended && !*drained
            ))
            .expect("its own home"),
        "the drive neither ended its wire nor drained its queue"
    );
    let survived = registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("its own home")
        .expect("the capsule the drive did not take");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83820)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none());
    assert!(wire_weak.upgrade().is_some());
    assert_eq!(durable.continuations_reserved(), Some(1));

    // AND ONCE IT HAS ENDED, the same drive does take it.
    drop(registration);
    let mut driven = 0;
    for _ in 0..8 {
        driven += durable.drive_ordered_continuations(4);
    }
    assert!(driven > 0, "a retained home is this drive's business");
    drop((private, survived, cell));
}

#[test]
fn the_drive_does_not_hold_the_store_while_it_waits_on_one_connections_home() {
    // TWO LOCKS, ONE ORDER. A home may reach the store while it is held -- a
    // close running under a home's lock can reserve a place, which the helper
    // controls here already do -- so home-then-store is a permitted edge.
    // Asking a home anything while holding the store is that edge reversed,
    // and a scan that did it put every other connection behind whichever home
    // happened to be contended.
    //
    // WHAT THIS ESTABLISHES: that with one connection's home held, the store
    // itself stays acquirable while the drive is waiting on it. It does NOT
    // establish where the driving thread has got to -- nothing here can say
    // that -- and it is not a deadlock observation: the drive waits on that
    // home either way, and what is asked is whether it waits holding the store.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8384);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 83840);
    let home = Arc::clone(&registration.ordered_home);
    drop((registration, private));
    assert_eq!(durable.continuations_retained(), Some(1));

    let blocker = home.state.lock().expect("hold this connection's home");
    let driving = durable.clone();
    let (started, wait) = std::sync::mpsc::channel();
    let driver = std::thread::spawn(move || {
        started.send(()).expect("started");
        driving.drive_ordered_continuations(4)
    });
    wait.recv().expect("the driving thread started");
    std::thread::sleep(Duration::from_millis(50));

    // The store is taken and released here many times over while the drive is
    // waiting on a home it cannot have.
    let mut acquired = 0usize;
    for _ in 0..2_000 {
        if durable.inner.try_lock().is_ok() {
            acquired += 1;
            if acquired == 8 {
                break;
            }
        }
        std::thread::yield_now();
    }
    assert_eq!(
        acquired, 8,
        "a contended home must not hold the whole store behind it"
    );

    drop(blocker);
    assert!(
        driver.join().expect("the driving thread finished") > 0,
        "and the drive takes it once the home is free"
    );
}

#[test]
fn a_running_connection_is_not_reported_as_retained_work() {
    // A BINDING POPULATES A LIVE HOME NOW. Before the relocation, anything in
    // a place had been put there by a teardown, so occupancy alone meant
    // retention. A reader that still asked only whether something was there
    // would describe every running connection as work owed to whoever drives
    // retained records -- and a caller acting on that would be acting on a
    // connection that has not ended, through a record its own registration is
    // still using.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8385);
    let (private, registration, cell, frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 83850);

    assert!(
        registration.ordered_home.occupied(),
        "bound, and in its home"
    );
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert_eq!(durable.continuations_retained(), Some(0));
    assert!(
        durable
            .retained_dispositions()
            .expect("a readable store")
            .is_empty(),
        "a live connection is not a retained row"
    );

    // AND THE READING TOUCHED NOTHING. Asking a channel whether it is finished
    // means receiving from it, so a reader that reached into a live queue
    // would answer the question by emptying it.
    let survived = registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("its own home")
        .expect("its capsule is still there");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83850)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none());

    // ONCE IT HAS ENDED, the same reader has a row for it.
    drop(registration);
    assert_eq!(durable.continuations_retained(), Some(1));
    let readings = durable.retained_dispositions().expect("a readable store");
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].0, 0);
    assert!(
        readings[0].1.is_some(),
        "and it is a reading, not an unreadable row"
    );
    drop((private, survived, cell));
}

#[test]
fn a_retained_home_refuses_a_binding_that_arrives_after_its_connection_ended() {
    // NOTHING IS BOUND INTO A CONNECTION THAT HAS GONE. The home outlives the
    // registration now, so a binding arriving late has somewhere to land that
    // it did not have before -- and landing there would give a queue to a
    // connection nobody will serve, behind whoever is already responsible for
    // finishing what is in it. The offer comes back instead.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8383);
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    // A capsule is accepted into this connection's queue before it ends, so
    // what the refused binding carries is real work rather than an empty
    // shape.
    let sender = capture_gated_sender(&private, client);
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(83830);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let frames = order_pass_frames(&capsule);
    gated_send(&sender, capsule).expect("an open endpoint");
    drop(sender);
    let home = Arc::clone(&registration.ordered_home);
    drop(registration);
    assert_eq!(home.standing(), PrivateHomeStanding::Retained);

    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = Arc::new(Mutex::new(stream));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let late = PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        evidence: PrivateOrderedEvidence::unstarted(),
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    };
    let PrivateHomeBinding::Ended(returned) = home.bind(late) else {
        panic!("a retained home takes no binding")
    };
    // AND IT COMES BACK WHOLE -- the exact capsule, its frames and its
    // unanswered completion, not merely something of the right shape.
    // Reporting a refusal by dropping the offer would answer a question about
    // a queue by destroying the queue.
    assert!(matches!(
        returned,
        PrivateOrderedContinuation::Setup {
            accepted: PrivateOrderedSetupCustody::Receiver(_),
            ..
        }
    ));
    let carried = returned
        .queue()
        .try_recv()
        .expect("the capsule accepted before it ended");
    assert_eq!(
        carried.delivery(),
        XAuthorityInputDeliveryId::from_raw(83830)
    );
    assert_eq!(order_pass_frames(&carried), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &carried.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none());
    assert!(!home.occupied(), "and nothing was put in it");
    drop((private, output, wire, pending, returned, carried, cell));
}

#[test]
fn a_conversion_leaves_the_store_holding_the_place_its_connection_held() {
    // THE HOLDER IS THE STORE'S OWN. A place could only be named from outside,
    // by the connection that reserved it; after teardown nothing named it at
    // all. A store that is to finish its own retained work needs an entry of
    // its own for each place, and that entry cannot be an owner of the store
    // it is in.
    let durable = PrivateSettlementOwner::default();
    let capability = durable.settlement_ref();
    let client = XServerFrontendClientId(8351);
    let (private, registration, cell, frames, wire_weak, _private_keeper) =
        converted_fixture(&durable, client, 83510);
    let place = registration
        .ordered_continuation
        .lock()
        .expect("a readable registration")
        .as_ref()
        .expect("its own reservation")
        .index;
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert_eq!(durable.holders_taken(), Some(0));

    let outer = hand_place_to_store(&durable, &registration);
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the same place, neither recharged nor released"
    );
    // AND THE CONNECTION IS STILL RUNNING. Handing the place to the store says
    // who is responsible for it, not that the connection has ended: a
    // conversion that also retired it would give a live connection's home to a
    // drive that closes wires, with the registration still holding it.
    assert_eq!(
        registration.ordered_home.standing(),
        PrivateHomeStanding::Live
    );
    assert_eq!(durable.continuations_retained(), Some(0));
    assert_eq!(
        durable.continuations_abandoned(),
        Some(0),
        "a place that crossed was continuously somebody's"
    );

    // THE CONNECTION THEN ENDS, with its place already the store's. Teardown
    // has no lease left to account for and does not invent one; what it does
    // is say the connection has gone, in the home the holder names.
    drop(registration);
    assert_eq!(durable.continuations_retained(), Some(1));
    assert_eq!(durable.continuations_abandoned(), Some(0));

    let named = durable
        .take_internal_holder(0)
        .expect("the store keeps a holder");
    assert_eq!(named.credit.place(), place);
    assert_eq!(durable.holders_taken(), Some(0));

    // AND THE WORK IS READ THROUGH THE CREDIT, which is the only thing that
    // names the place now -- and which never owned the registration.
    let PrivateCreditReach::Reached(Some(survived)) = credit_receives(&named.credit) else {
        panic!("the capsule accepted before the connection ended")
    };
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83510)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    drop((private, survived, cell, durable, _private_keeper));

    // THE OUTER HOLDER IS THE CONVERSION'S CALLER, and nothing else: the
    // connection, its instance, the service owner and the caller's other
    // binding are gone.
    assert!(capability.owner().is_some());
    assert!(wire_weak.upgrade().is_some(), "with the work still in it");

    // AND WHEN THE LEGITIMATE HOLDERS GO, so does the graph.
    drop(outer);
    assert!(
        capability.owner().is_none(),
        "a store's own holder must not be the reason the store exists"
    );
    assert!(wire_weak.upgrade().is_none());
    drop(named);
}

#[test]
fn a_store_owned_holder_left_in_its_store_does_not_keep_it_alive() {
    // THE RING, BUILT AND LEFT STANDING. The holder above was taken out before
    // the last holder went. This one is not: the credit stays inside the store
    // it names a place in, which is the shape -- store, holder, credit, store
    // -- that an owner in the credit would close.
    let durable = PrivateSettlementOwner::default();
    let capability = durable.settlement_ref();
    let client = XServerFrontendClientId(8352);
    let (private, registration, cell, _frames, wire_weak, _private_keeper) =
        converted_fixture(&durable, client, 83520);
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    assert_eq!(durable.holders_taken(), Some(1), "and it is still in there");
    drop((private, cell, durable, _private_keeper));

    assert!(
        capability.owner().is_some(),
        "held by the conversion's caller"
    );
    drop(outer);
    assert!(
        capability.owner().is_none(),
        "the store's own holder is on no ring"
    );
    assert!(
        wire_weak.upgrade().is_none(),
        "and the work it named went with the place"
    );
}

#[test]
fn a_refused_holder_preparation_leaves_the_lease_and_the_work_where_they_were() {
    // PREPARATION IS THE FALLIBLE HALF, so this is where a place that already
    // has a holder has to be felt. A conversion that prepared as it went would
    // have taken the place out of the connection's hands before finding out it
    // had nowhere to put it.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8353);
    let (private, registration, cell, frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 83530);
    let lease = lease_of(&registration);
    let place = lease.index;

    let other = XServerFrontendClientId(8363);
    let (other_registration, _other_channels) = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a place and a row");
    let other_lease = lease_of(&other_registration);
    let held_places = [
        durable
            .prepare_internal_holder(&lease)
            .expect("within the declared bound"),
        durable
            .prepare_internal_holder(&other_lease)
            .expect("within the declared bound"),
    ];
    assert_eq!(durable.holders_taken(), Some(2));
    assert!(
        matches!(
            durable.prepare_internal_holder(&lease),
            Err(PrivateHolderRefusal::AlreadyHeld)
        ),
        "one place, one holder"
    );

    // NOTHING MOVED. The lease is still armed and still names the same place,
    // the work is still in its home, and the counters are untouched.
    assert!(lease.armed);
    assert_eq!(lease.index, place);
    assert!(registration.ordered_home.occupied());
    assert_eq!(durable.continuations_reserved(), Some(2));
    assert_eq!(durable.continuations_abandoned(), Some(0));

    // AND A RELEASED PREPARATION GIVES ITS PLACE BACK.
    drop(held_places);
    assert_eq!(durable.holders_taken(), Some(0));
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("the released places are free again");
    let outer = durable.clone();
    assert!(matches!(
        lease.convert_to_internal(&outer, destination),
        PrivateInternalConversion::Held
    ));
    drop(registration);
    let named = durable.take_internal_holder(0).expect("a holder");
    assert_eq!(named.credit.place(), place, "the same place, after a refusal");
    let PrivateCreditReach::Reached(Some(survived)) = credit_receives(&named.credit) else {
        panic!("the same capsule")
    };
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    drop((other_registration, private, outer, named));
}

#[test]
fn a_conversion_with_a_foreign_destination_is_refused_without_touching_anything() {
    // NOT A DEBUG ASSERTION. A destination prepared against another store
    // names an index in THAT store's holders. Committing it here writes into
    // whatever this store has at that index -- taking over another
    // connection's promise -- and leaves the other store's promise held for
    // ever. A check that only exists in builds with debug assertions makes the
    // provenance of a place a testing-only property.
    let one = PrivateSettlementOwner::default();
    let two = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8360);
    let (private_one, registration_one, cell, frames, _wire, _private_one_keeper) =
        converted_fixture(&one, client, 83600);
    let other = XServerFrontendClientId(8361);
    let (private_two, registration_two, _cell_two, _frames_two, _wire_two, _private_two_keeper) =
        converted_fixture(&two, other, 83610);
    let lease = lease_of(&registration_one);
    let foreign_lease = lease_of(&registration_two);

    // Both are ordinary results of the real API, from their own stores.
    let foreign = two
        .prepare_internal_holder(&foreign_lease)
        .expect("the other store has a holder place");
    let outer = one.clone();
    let PrivateInternalConversion::Foreign(lease) = lease.convert_to_internal(&outer, foreign)
    else {
        panic!("a destination from another store is not this store's to commit")
    };

    // NOTHING TOUCHED, on either side.
    assert!(lease.armed);
    assert!(registration_one.ordered_home.occupied());
    assert_eq!(one.holders_taken(), Some(0));
    assert_eq!(
        two.holders_taken(),
        Some(0),
        "the destination was consumed by the refusal and gave its place back, \
         rather than being left held for ever"
    );
    assert_eq!(one.continuations_abandoned(), Some(0));
    assert_eq!(two.continuations_abandoned(), Some(0));

    // AND A PROMISE MADE FOR ANOTHER PLACE IN THIS STORE IS REFUSED TOO.
    let neighbour = XServerFrontendClientId(8368);
    let (neighbour_registration, _neighbour_channels) = private_one
        .broker
        .registry
        .register_client_with_admission(neighbour, Some(admitted(neighbour)))
        .expect("a place and a row");
    let neighbour_lease = lease_of(&neighbour_registration);
    let neighbours = one
        .prepare_internal_holder(&neighbour_lease)
        .expect("its own place");
    assert_ne!(neighbours.for_place, lease.index);
    let PrivateInternalConversion::Foreign(lease) = lease.convert_to_internal(&outer, neighbours)
    else {
        panic!("a promise made for another place is not this lease's to commit")
    };
    assert!(lease.armed);
    assert_eq!(one.holders_taken(), Some(0));

    // And the lease that came back still converts, over its own store.
    let destination = one
        .prepare_internal_holder(&lease)
        .expect("its own store has a place");
    assert!(matches!(
        lease.convert_to_internal(&outer, destination),
        PrivateInternalConversion::Held
    ));
    drop(registration_one);
    let named = one.take_internal_holder(0).expect("a holder");
    let PrivateCreditReach::Reached(Some(survived)) = credit_receives(&named.credit) else {
        panic!("the work it was refused over")
    };
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    drop((
        registration_two,
        neighbour_registration,
        private_one,
        private_two,
        outer,
        named,
        foreign_lease,
        neighbour_lease,
    ));
}

#[test]
fn a_promise_made_for_one_reservation_is_not_committed_against_its_successor() {
    // THE PROMISE NEEDS AN IDENTITY FOR THE SAME REASON THE CREDIT DOES. A
    // place goes back when its reservation is given up, and the next
    // connection takes the same number. A destination that named only the
    // number would then be committed against that connection's lease -- a
    // holder over a connection nobody prepared one for, made from a promise
    // that was for somebody else.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    // RESERVED WITHOUT BEING PUBLISHED, which is what relinquish_unexposed is
    // for: registering a client publishes its row, and a place given back
    // after that is not an unexposed one.
    let lease = durable
        .reserve_ordered_continuation()
        .expect("a declared bound leaves a place");
    let index = lease.index;
    let stale = durable
        .prepare_internal_holder(&lease)
        .expect("a holder place for this reservation");
    lease.relinquish_unexposed();
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "a reservation that published nothing gives its place back"
    );

    // THE SAME NUMBER, A DIFFERENT CONNECTION.
    let successor_id = XServerFrontendClientId(8372);
    let (successor, successor_registration, cell, frames, wire_weak, _successor_keeper) =
        converted_fixture(&durable, successor_id, 83720);
    let successor_lease = lease_of(&successor_registration);
    assert_eq!(successor_lease.index, index, "the place was handed on");

    let PrivateInternalConversion::Foreign(successor_lease) =
        successor_lease.convert_to_internal(&durable.clone(), stale)
    else {
        panic!("a promise made for another reservation is not this one's to commit")
    };

    // NOTHING TOUCHED: the successor still holds its place and its work.
    assert!(successor_lease.armed);
    assert!(successor_registration.ordered_home.occupied());
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert_eq!(durable.continuations_abandoned(), Some(0));
    assert_eq!(durable.holders_taken(), Some(0));
    assert!(wire_weak.upgrade().is_some());

    // And its own promise commits, over its own record.
    let destination = durable
        .prepare_internal_holder(&successor_lease)
        .expect("its own reservation");
    let outer = durable.clone();
    assert!(matches!(
        successor_lease.convert_to_internal(&outer, destination),
        PrivateInternalConversion::Held
    ));
    drop(successor_registration);
    let named = durable.take_internal_holder(0).expect("a holder");
    let PrivateCreditReach::Reached(Some(landed)) = credit_receives(&named.credit) else {
        panic!("the successor's own work, in the successor's own record")
    };
    assert_eq!(
        landed.delivery(),
        XAuthorityInputDeliveryId::from_raw(83720)
    );
    assert_eq!(order_pass_frames(&landed), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &landed.finalizer().expect("carried").completion
    ));
    drop((private, successor, named, outer));
}

#[test]
fn a_credit_over_a_place_whose_connection_never_bound_will_not_free_it() {
    // AN EMPTY HOME IS NOT A FINISHED ONE. A place whose home holds nothing is
    // a connection that has not bound -- and one of the moments it has not
    // bound yet is while it is still running. Reading that as "nothing owed"
    // frees the place out from under a binding that is still coming, and the
    // next connection takes it.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8369);
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    assert!(
        !registration.ordered_home.occupied(),
        "exposed, and not bound yet"
    );
    let outer = hand_place_to_store(&durable, &registration);
    let mut named = durable.take_internal_holder(0).expect("a holder");

    assert!(matches!(
        named.credit.with_place(|_| ()),
        PrivateCreditReach::Empty
    ));
    assert!(matches!(
        named.credit.release(),
        PrivateCreditRelease::NotHandedOver
    ));
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the place is kept, so a binding still coming keeps its home"
    );
    assert_eq!(durable.continuations_abandoned(), Some(0));
    drop((registration, private, named, outer));
}

#[test]
fn a_credit_accounts_for_its_place_exactly_once() {
    // ONE PLACE, ONE DISPOSAL. A conversion disarms the lease rather than
    // disposing of it, so the place is not counted abandoned on the way
    // through; the credit takes that duty on, and must not discharge it twice
    // or discharge a duty the lease already discharged.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8354);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 83540);
    let outer = hand_place_to_store(&durable, &registration);
    let named = durable.take_internal_holder(0).expect("a holder");

    // THE CONNECTION ENDS WITH ITS PLACE ALREADY THE STORE'S. Teardown has no
    // lease to account for and must not act as though it had one.
    drop(registration);
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert_eq!(durable.continuations_abandoned(), Some(0));

    // A holder that goes without disposing of its place leaves work nobody
    // accounted for. It is marked once, and the capacity is not handed out
    // again.
    drop(named);
    assert_eq!(durable.continuations_abandoned(), Some(1));
    assert_eq!(durable.continuations_reserved(), Some(1));
    drop((private, outer));
}

#[test]
fn a_credit_refuses_to_free_a_place_that_still_owes_work() {
    // THE PRECONDITION IS CHECKED, NOT ASSUMED. Freeing a place over a queue
    // that still holds capsules destroys them unanswered, which is the loss
    // the precondition existed to prevent.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8355);
    let (private, registration, cell, frames, wire_weak, _private_keeper) =
        converted_fixture(&durable, client, 83550);
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    let mut named = durable.take_internal_holder(0).expect("a holder");

    // THE NEGATIVE: a capsule is on that queue and nothing established an
    // ending, so this record is not settled and the release is refused.
    assert!(
        named
            .credit
            .with_place(|continuation| continuation.settled())
            .reached()
            .is_some_and(|settled| !settled),
        "a bound connection with a queued capsule owes work"
    );
    assert!(matches!(
        named.credit.release(),
        PrivateCreditRelease::StillOwed
    ));
    assert_eq!(durable.continuations_reserved(), Some(1), "the place is kept");
    assert_eq!(durable.continuations_abandoned(), Some(0));

    // AND THE PAYLOAD IS STILL THERE.
    let PrivateCreditReach::Reached(Some(survived)) = credit_receives(&named.credit) else {
        panic!("a refused release destroys nothing")
    };
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83550)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none());
    assert!(wire_weak.upgrade().is_some());
    drop((private, survived, cell, named, outer));
}

#[test]
fn a_credit_frees_a_place_whose_record_finished_owing_nothing() {
    // THE POSITIVE, over a record that is genuinely settled: its producers are
    // gone, its queue reported Disconnected as it was drained, its wire was
    // ended and its closure established.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8356);
    let (private, registration, _cell, _frames, wire_weak, _private_keeper) =
        converted_fixture(&durable, client, 83560);
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    let mut named = durable.take_internal_holder(0).expect("a holder");

    // DRIVEN THROUGH THE CREDIT, which is what a holder taken out of the store
    // is for. The store's own drive is deliberately not used: it returns the
    // places it settles itself, so a control that let it run would be watching
    // that return rather than this release.
    assert!(matches!(
        credit_receives(&named.credit),
        PrivateCreditReach::Reached(Some(_))
    ));
    drop(private);
    let mut settled = false;
    for _ in 0..8 {
        let Some(finished) = named
            .credit
            .with_place(|continuation| {
                continuation.visit();
                continuation.settled()
            })
            .reached()
        else {
            panic!("the place is this credit's until it releases it")
        };
        settled = finished;
        if settled {
            break;
        }
    }
    assert!(settled, "the real visit finished this record");

    assert!(matches!(
        named.credit.release(),
        PrivateCreditRelease::Released
    ));
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "and the place is free again"
    );
    assert_eq!(durable.continuations_abandoned(), Some(0));
    drop(named);
    assert_eq!(
        durable.continuations_abandoned(),
        Some(0),
        "a released credit is not also an abandoned one"
    );
    // AND THE SERVICE OWNER, which keeps this connection's teardown record and
    // so keeps what that record owns. Its going is a separate event from the
    // registration's, and this control makes it happen before asking.
    drop(_private_keeper);
    assert!(wire_weak.upgrade().is_none());
    drop(outer);
}

#[test]
fn an_old_credit_cannot_reach_or_free_the_place_its_successor_took() {
    // A NUMBER IS NOT AN IDENTITY. A place is returned when the work in it is
    // settled, and the next connection to reserve one takes that same index.
    let durable = PrivateSettlementOwner::default();
    let first = XServerFrontendClientId(8364);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, first, 83640);
    let index = registration
        .ordered_continuation
        .lock()
        .expect("a readable registration")
        .as_ref()
        .expect("its own reservation")
        .index;
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    let mut stale = durable.take_internal_holder(0).expect("a holder");

    // THE REAL DRIVE SETTLES IT AND RETURNS THE PLACE.
    assert!(matches!(
        credit_receives(&stale.credit),
        PrivateCreditReach::Reached(Some(_))
    ));
    drop(private);
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "the drive returned the place it settled"
    );

    // A SUCCESSOR TAKES THE SAME INDEX, through ordinary registration.
    let second = XServerFrontendClientId(8365);
    let (successor, successor_registration, successor_cell, successor_frames, successor_wire, _successor_keeper) =
        converted_fixture(&durable, second, 83650);
    let successor_place = successor_registration
        .ordered_continuation
        .lock()
        .expect("a readable registration")
        .as_ref()
        .expect("its own reservation")
        .index;
    assert_eq!(
        successor_place, index,
        "the same place, a different connection"
    );

    // THE OLD CREDIT REACHES NOTHING.
    assert!(matches!(
        credit_receives(&stale.credit),
        PrivateCreditReach::Moved
    ));
    assert!(matches!(
        stale.credit.release(),
        PrivateCreditRelease::NotOurs
    ));
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the successor still has its place"
    );

    // AND ITS CAPSULE AND BINDING ARE UNTOUCHED.
    let capsule = successor_registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("its own home")
        .expect("the successor's own capsule");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(83650)
    );
    assert_eq!(order_pass_frames(&capsule), successor_frames);
    assert!(Arc::ptr_eq(
        &successor_cell,
        &capsule.finalizer().expect("carried").completion
    ));
    assert!(successor_cell.answer().is_none());
    assert!(successor_wire.upgrade().is_some());

    // AND DROPPING IT ACCOUNTS FOR NOTHING.
    let before = durable.continuations_abandoned();
    drop(stale);
    assert_eq!(durable.continuations_abandoned(), before);
    drop((successor_registration, successor, capsule, outer));
}

#[test]
fn a_returned_place_leaves_no_holder_behind_naming_it() {
    // THE OTHER HALF OF THE SAME PROBLEM. The credit checking its place keeps
    // a stale holder harmless; this keeps one from being left there at all.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8366);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 83660);
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    assert_eq!(durable.holders_taken(), Some(1));

    {
        let held = durable.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(home) = &held.continuations[0] else {
            panic!("its place holds its home")
        };
        let home = Arc::clone(home);
        drop(held);
        assert!(home
            .borrow(|continuation| continuation.queue().try_recv().is_ok())
            .expect("its own home"));
    }
    drop(private);
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert_eq!(durable.continuations_reserved(), Some(0));
    assert_eq!(
        durable.holders_taken(),
        Some(0),
        "the holder went with the place it named"
    );
    assert_eq!(
        durable.continuations_abandoned(),
        Some(0),
        "and a holder retired over a settled place accounts for nothing"
    );
    drop(outer);
}

#[test]
fn a_credits_operation_keeps_the_store_it_found_until_the_operation_ends() {
    // THE INTERVAL, DRIVEN RATHER THAN DESCRIBED. A credit holds no owner, so
    // every operation begins by upgrading. A lookup that clones the home
    // handle and lets that upgrade go before acting leaves a window where the
    // last outside holder can drop: the home survives in the clone, so the
    // operation finishes and reports success into storage nobody can reach.
    let durable = PrivateSettlementOwner::default();
    let capability = durable.settlement_ref();
    let client = XServerFrontendClientId(8367);
    let (private, registration, cell, frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 83670);
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    let named = durable.take_internal_holder(0).expect("a holder");
    drop((private, durable, _private_keeper));

    let last = std::cell::Cell::new(Some(outer));
    let watch = capability.clone();
    let PrivateCreditReach::Reached(Some(survived)) = named.credit.with_place(|continuation| {
        drop(last.take());
        // THE ASSERTION THAT SEPARATES THEM. A lookup that let its upgrade go
        // would leave the store gone from here on, and the rest of this
        // callback would run against a home nobody could reach -- which reads
        // exactly like success, because the home itself survives in the handle
        // the lookup cloned. The store is what is asked about, not the home.
        assert!(
            watch.owner().is_some(),
            "the operation holds the store it found for as long as it runs"
        );
        continuation.queue().try_recv().ok()
    }) else {
        panic!("and the work was still readable in it")
    };
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83670)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));

    // AND ONLY UNTIL IT ENDS.
    assert!(capability.owner().is_none());
    assert!(matches!(
        named.credit.with_place(|_| ()),
        PrivateCreditReach::StoreGone
    ));
    drop((survived, cell, named));
}
