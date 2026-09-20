// Destinations and their evidence: what a fencing leaves readable, what a name
// establishes, and the connections each belongs to.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_fencing_that_waits_on_a_handover_leaves_its_evidence_readable() {
    // THE GATE MAY BLOCK, and the point is what stays readable while it does.
    // A handover admitted before this asks holds the gate until it is done;
    // nothing of this connection's is held behind that wait, so a caller can
    // still read the join result and this attempt's standing.
    let g = fence_fixture(XServerFrontendClientId(8428));
    cancel_connection_worker(&g.f.stop, &g.f.wake);
    let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    let fence = PrivateFenceRecord::bound_to(&custody);

    std::thread::scope(|scope| {
        let (admitted, wait) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let sender = g.f.sender.clone();
        let producer = scope.spawn(move || {
            let _inside = sender.admit().expect("an open endpoint");
            admitted.send(()).expect("inside the gate");
            let _ = held.recv();
        });
        wait.recv().expect("a handover is inside the gate");

        let fence = &fence;
        let fencing = scope.spawn(move || fence.record_fence());
        // WHAT IS OBSERVED, exactly: while that handover holds the gate, no
        // fence is published, and the join result and this attempt's standing
        // are both readable. It does NOT establish that the fencing thread has
        // reached the gate's lock -- nothing here can say that.
        assert!(
            waited_for(|| fence.phase() == PrivateFencePhase::InProgress),
            "the attempt claimed this record"
        );
        assert_eq!(fence.fence(), None, "and has published nothing");
        assert!(
            matches!(record.result(), Some(PrivateJoinResult::Returned)),
            "the join result is readable throughout"
        );
        assert_eq!(record.phase(), PrivateReapingPhase::Joined);

        drop(release);
        producer.join().expect("the handover finished");
        assert_eq!(
            fencing.join().expect("the fencing finished"),
            PrivateFenced::Recorded
        );
    });
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));
    drop(g.f.fixture);
}

#[test]
fn fencing_one_connection_leaves_another_connections_gate_open() {
    // A GATE IS ONE CONNECTION'S. This record holds the one it was bound to
    // and reaches nothing by lookup, so closing it says nothing about anybody
    // else's endpoint -- and nothing about anybody's queue, custody or wire,
    // its own included.
    //
    // THE WORK IS QUEUED BEFORE THE FENCE, on both connections, so what is
    // compared afterwards is the same capsule that was there: a capsule
    // inserted after a closure and then found could have been anything.
    // Fixture capsules, carrying their own completions; no claim is made that
    // either recipient would have admitted them.
    let g = fence_fixture(XServerFrontendClientId(8429));
    let other = worker_fixture(XServerFrontendClientId(8430));
    // The target's own worker is stopped and joined first, so what is queued
    // for it below stays queued: a running body would serve it, which is that
    // component's business and not this one's.
    cancel_connection_worker(&g.f.stop, &g.f.wake);
    let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);

    let queued = [(&g.f, 84290u64), (&other, 84300u64)].map(|(f, delivery)| {
        let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(delivery);
        let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
        let frames = order_pass_frames(&capsule);
        produced_send(&f.sender, capsule);
        (cell, frames)
    });
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));

    assert_eq!(
        g.f.fixture.registration.ordered_handovers_fenced(),
        Some(true)
    );
    assert_eq!(
        other.fixture.registration.ordered_handovers_fenced(),
        Some(false),
        "the other connection still admits handovers"
    );

    // AND NEITHER QUEUE WAS TOUCHED. The exact capsules, their frames and
    // their completion cells, on the fenced connection and on its neighbour
    // alike: closing a gate receives nothing and answers nothing.
    for (f, (cell, frames), delivery) in [
        (&g.f, &queued[0], 84290u64),
        (&other, &queued[1], 84300u64),
    ] {
        let survived = f
            .home
            .borrow_live(|payload| {
                let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                    panic!("promoted")
                };
                owner.queue.try_recv().ok()
            })
            .acted()
            .expect("its own home")
            .expect("what was accepted for it before the fencing");
        assert_eq!(
            survived.delivery(),
            XAuthorityInputDeliveryId::from_raw(delivery)
        );
        assert_eq!(order_pass_frames(&survived), *frames);
        assert!(Arc::ptr_eq(
            cell,
            &survived.finalizer().expect("carried").completion
        ));
        assert!(cell.answer().is_none(), "and nothing answered for it");
        // AND NOTHING REACHED EITHER PEER. A fence writes no bytes.
        let mut byte = [0u8; 1];
        assert_eq!(
            std::io::Read::read(&mut (&f.peer), &mut byte)
                .expect_err("nothing was written")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(
            f.home.standing(),
            PrivateHomeStanding::Live,
            "and no standing was changed"
        );
    }
    drop((g.f.fixture, other.fixture));
}

#[test]
fn a_fence_is_not_delayed_by_a_diagnostic_somebody_is_holding() {
    // A PUBLISHED JOIN IS ENOUGH, AND NEITHER OPTIONAL LOCK IS ON THE WAY TO
    // THE GATE. WHAT THIS ESTABLISHES, exactly: a fencing completes while both
    // the panic payload and the exit diagnostic are held by somebody else. It
    // does NOT establish that a fencing can run before the reaping call has
    // returned from its own diagnostic read -- the reap here finishes first --
    // and that is a separate claim needing a separate control.
    let f = worker_fixture(XServerFrontendClientId(8431));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || panic!("held while the gate is closed"));
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    let fence = PrivateFenceRecord::bound_to(&custody);

    let PrivateJoinResult::Panicked(payload) = record.result().expect("a completed join") else {
        panic!("this worker panicked")
    };
    let payload_held = payload.lock().expect("a readable payload");
    let diagnostic_held = custody.exit_sink().outcome.lock().expect("a readable exit record");
    assert_eq!(
        fence.record_fence(),
        PrivateFenced::Recorded,
        "neither lock is on the way to the gate"
    );
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));
    drop((payload_held, diagnostic_held));
    drop(f.fixture);
}

/// Which holder entry, if any, this store has set aside for a place.
fn maintenance_destination(
    durable: &PrivateSettlementOwner,
    place: usize,
) -> Option<&'static str> {
    let held = durable.records_even_if_poisoned();
    held.holders.iter().find_map(|entry| match entry {
        PrivateHolderPlace::Reserved(reserved) if *reserved == place => Some("reserved"),
        PrivateHolderPlace::Promised(promised) if *promised == place => Some("promised"),
        PrivateHolderPlace::Taken(holder) if holder.credit.index == place => Some("taken"),
        _ => None,
    })
}

#[test]
fn a_connections_obligation_is_named_and_housed_before_it_is_published() {
    // BEFORE THE ROW, WHICH IS THE WHOLE POINT. What will eventually fill this
    // destination -- a joined worker, a closed gate, an obligation somebody
    // commits -- all happens long after this connection is reachable. Finding
    // storage for it then would mean a connection whose sender producers can
    // already reach could be left unable to be finished for want of room.
    //
    // SOURCE ORDERING IS WHAT ESTABLISHES "BEFORE" HERE. The reservation runs
    // inside register_client_with_admission ahead of publish_registered_client
    // and ahead of the client table, and the destination is set aside in the
    // same acquisition as the place. What this control observes is the state
    // that ordering leaves: a registration that exists at all already has
    // both, on one credit.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8441);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");

    let identity = registration
        .maintenance_identity()
        .expect("this connection has a place, so it has a name");
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert_eq!(
        maintenance_destination(&durable, identity.place()),
        Some("reserved"),
        "its destination was set aside with its place"
    );
    assert_eq!(
        durable.holders_taken(),
        Some(0),
        "and nothing holds one: a reserved destination is inert"
    );

    // THE SAME NAME EVERY TIME, and it names this connection's own home.
    let again = registration
        .maintenance_identity()
        .expect("the same connection");
    assert!(identity.same_as(&again), "one connection, one name");
    assert!(
        identity
            .with_home(|home| Arc::ptr_eq(home, &registration.ordered_home))
            .reached()
            .expect("its place is its own"),
        "and the home it names is the one this registration binds into"
    );
    drop((registration, private));
}

#[test]
fn every_connection_at_the_bound_already_has_its_destination() {
    // NO SECOND POOL AND NO SECOND CHARGE. One connection place carries one
    // maintenance destination, so a store at its bound has a destination for
    // every connection it admitted and the conversion needs no extra credit.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let held: Vec<_> = [8442u64, 8443]
        .iter()
        .map(|raw| {
            let client = XServerFrontendClientId(*raw);
            private
                .broker
                .registry
                .register_client_with_admission(client, Some(admitted(client)))
                .expect("a place and a row")
                .0
        })
        .collect();
    assert_eq!(durable.continuations_reserved(), Some(2), "at the bound");
    for registration in &held {
        let place = registration
            .maintenance_identity()
            .expect("a place, so a name")
            .place();
        assert_eq!(maintenance_destination(&durable, place), Some("reserved"));
    }

    // AND THE APPROVED CONVERSION FINDS ITS DESTINATION ALREADY THERE, taking
    // no further credit for it.
    let lease = lease_of(&held[0]);
    let before = durable.continuations_reserved();
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("the destination reserved with this place");
    assert_eq!(durable.continuations_reserved(), before);
    assert_eq!(
        maintenance_destination(&durable, lease.index),
        Some("promised")
    );
    let outer = durable.clone();
    assert!(matches!(
        lease.convert_to_internal(&outer, destination),
        PrivateInternalConversion::Held
    ));
    assert_eq!(durable.continuations_reserved(), before, "and still none");
    assert_eq!(durable.continuations_abandoned(), Some(0));
    drop((held, private, outer));
}

#[test]
fn a_preparation_that_is_never_committed_leaves_the_destination_reserved() {
    // BACK TO RESERVED, NOT FREE. Nothing was put in it and the place it was
    // set aside for is still this connection's; handing it to somebody else
    // would leave a published connection with nowhere for its obligation.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8444);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let lease = lease_of(&registration);
    {
        let destination = durable
            .prepare_internal_holder(&lease)
            .expect("its own destination");
        assert_eq!(
            maintenance_destination(&durable, destination.for_place),
            Some("promised")
        );
    }
    assert_eq!(
        maintenance_destination(&durable, lease.index),
        Some("reserved"),
        "the preparation went and the reservation stayed"
    );
    // And it can be prepared again, because it is still this connection's.
    assert!(durable.prepare_internal_holder(&lease).is_ok());
    drop((registration, private, lease));
}

#[test]
fn an_unexposed_refusal_leaves_no_destination_behind() {
    // A RESERVATION THAT PUBLISHED NOTHING GIVES BACK BOTH. The existing
    // unexposed release returns the place; the destination set aside with it
    // goes the same way, or the next connection to reserve would find one of
    // its entries already spoken for by a connection that never existed.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let lease = durable
        .reserve_ordered_continuation()
        .expect("a declared bound leaves a place");
    let place = lease.index;
    assert_eq!(maintenance_destination(&durable, place), Some("reserved"));

    lease.relinquish_unexposed();
    assert_eq!(durable.continuations_reserved(), Some(0));
    assert_eq!(
        maintenance_destination(&durable, place),
        None,
        "and its destination went with it"
    );
    drop(private);
}

#[test]
fn a_name_outlives_its_connection_and_its_conversion() {
    // THE NAME IS WHAT THE OBLIGATION IS CALLED, so it has to survive the
    // things that end a connection without ending what it owed: its
    // registration being destroyed, and its place becoming the store's own
    // responsibility.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8445);
    let (private, registration, cell, frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 84450);
    let identity = registration
        .maintenance_identity()
        .expect("a place, so a name");

    let outer = hand_place_to_store(&durable, &registration);
    // A CONVERSION DOES NOT RENAME IT: same store, same place, same home.
    let named = durable.take_internal_holder(0).expect("a holder");
    assert!(
        identity.same_as(&named.credit.maintenance_identity()),
        "the store's own holder names the same obligation"
    );

    drop(registration);
    // AND THE CONNECTION ENDING DOES NOT EITHER. The name still resolves, and
    // the home it reaches still holds what was accepted for it.
    let survived = identity
        .with_home(|home| home.borrow(|continuation| continuation.queue().try_recv().ok()))
        .reached()
        .expect("its place is still its own")
        .expect("its home")
        .expect("the capsule accepted before it ended");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(84450)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    drop((private, survived, cell, named, outer));
}

#[test]
fn a_name_stops_resolving_when_its_place_goes_back() {
    // A RETURNED PLACE IS NOBODY'S, INCLUDING ITS OWN. The name must stop
    // resolving the moment the place goes back -- not when a successor
    // arrives -- because between those two a caller acting on "still current"
    // would be acting on a place that is free.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8446);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 84460);
    let identity = registration
        .maintenance_identity()
        .expect("a place, so a name");
    let place = identity.place();
    let outer = hand_place_to_store(&durable, &registration);
    drop(registration);
    let mut named = durable.take_internal_holder(0).expect("a holder");
    assert!(matches!(
        credit_receives(&named.credit),
        PrivateCreditReach::Reached(Some(_))
    ));
    drop(private);

    // The place is returned through the credit's own release.
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
    assert!(settled);
    assert!(matches!(
        named.credit.release(),
        PrivateCreditRelease::Released
    ));
    assert_eq!(durable.continuations_reserved(), Some(0));

    // NOTHING HAS TAKEN THE NUMBER YET, and the name is already stale.
    assert!(matches!(
        identity.with_home(|_| ()),
        PrivateMaintenanceReach::Stale
    ));
    assert_eq!(
        maintenance_destination(&durable, place),
        None,
        "and its destination is free for whoever comes next"
    );

    // A SUCCESSOR TAKES THE SAME NUMBER AND GETS A DIFFERENT NAME.
    let second = XServerFrontendClientId(8447);
    let (successor, successor_registration, successor_cell, successor_frames, _successor_wire, _successor_keeper) =
        converted_fixture(&durable, second, 84470);
    let successor_identity = successor_registration
        .maintenance_identity()
        .expect("a place, so a name");
    assert_eq!(successor_identity.place(), place, "the same number");
    assert!(
        !identity.same_as(&successor_identity),
        "and a different obligation"
    );
    assert!(matches!(
        identity.with_home(|_| ()),
        PrivateMaintenanceReach::Stale
    ));
    assert_eq!(
        maintenance_destination(&durable, place),
        Some("reserved"),
        "the successor's own destination, set aside with its own place"
    );

    // AND THE OLD NAME REACHED NOTHING OF THE SUCCESSOR'S.
    let capsule = successor_registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("its own home")
        .expect("the successor's own capsule");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(84470)
    );
    assert_eq!(order_pass_frames(&capsule), successor_frames);
    assert!(Arc::ptr_eq(
        &successor_cell,
        &capsule.finalizer().expect("carried").completion
    ));
    assert!(successor_cell.answer().is_none());
    drop((successor_registration, successor, capsule, named, outer));
}

#[test]
fn a_name_from_another_store_resolves_to_nothing_here() {
    // A NAME CARRIES ITS STORE. Two connections can hold the same number in
    // different stores, and neither is the other's.
    let one = PrivateSettlementOwner::default();
    let two = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&one, 2);
    let first = private_over(&service_keeper, 2);
    let service_keeper = service_owner(&two, 2);
    let second = private_over(&service_keeper, 2);
    let a = XServerFrontendClientId(8448);
    let b = XServerFrontendClientId(8449);
    let (one_registration, _one_channels) = first
        .broker
        .registry
        .register_client_with_admission(a, Some(admitted(a)))
        .expect("a place and a row");
    let (two_registration, _two_channels) = second
        .broker
        .registry
        .register_client_with_admission(b, Some(admitted(b)))
        .expect("a place and a row");
    let here = one_registration.maintenance_identity().expect("a name");
    let there = two_registration.maintenance_identity().expect("a name");

    // WHAT THIS ESTABLISHES is structural: each name carries its own store
    // weakly and resolves in that one, so two connections holding the same
    // number in different stores are different obligations and neither
    // resolves into the other. There is no receiving store to refuse a
    // foreign name -- with_home has nowhere to be asked from -- and this does
    // not claim such a refusal exists.
    assert_eq!(here.place(), there.place(), "the same number");
    assert!(!here.same_as(&there), "and different obligations");
    assert!(
        here.with_home(|home| Arc::ptr_eq(home, &one_registration.ordered_home))
            .reached()
            .expect("its own store"),
        "each resolves in its own store"
    );
    assert!(
        there
            .with_home(|home| Arc::ptr_eq(home, &two_registration.ordered_home))
            .reached()
            .expect("its own store")
    );
    drop((one_registration, two_registration, first, second));
}

#[test]
fn a_lookup_keeps_the_store_it_found_until_its_act_is_over() {
    // A NAME HOLDS ITS STORE WEAKLY, so every lookup begins by upgrading. An
    // operation that let that upgrade go before acting would be acting on a
    // home whose store could disappear underneath it -- and the home survives
    // in the handle the lookup pinned, so the act would finish and look like
    // success.
    //
    // THE LAST HOLDER IS DROPPED FROM INSIDE THE ACT, which is precisely the
    // interval. Nothing is injected and no thread is raced.
    let durable = PrivateSettlementOwner::default();
    let capability = durable.settlement_ref();
    let client = XServerFrontendClientId(8451);
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let identity = registration.maintenance_identity().expect("a name");
    let home = Arc::clone(&registration.ordered_home);
    // Every holder of the store that is not the lookup's own goes in here --
    // the service owner with them, because keeping the store is exactly what
    // an owner is for.
    drop(registration);
    let last = std::cell::Cell::new(Some((private, durable, service_keeper)));
    let watch = capability.clone();

    let reached = identity
        .with_home(|found| {
            assert!(Arc::ptr_eq(found, &home));
            drop(last.take());
            // THE ASSERTION THAT SEPARATES THEM. A lookup that let its upgrade
            // go would leave the store gone from here on, and the rest of this
            // act would run against a home nobody could reach -- which reads
            // exactly like success, because the home itself survives in the
            // handle the lookup pinned.
            watch.owner().is_some()
        })
        .reached()
        .expect("its place is its own");
    assert!(
        reached,
        "the lookup holds the store it found for as long as its act runs"
    );

    // AND ONLY UNTIL IT IS OVER.
    assert!(capability.owner().is_none());
    assert!(matches!(
        identity.with_home(|_| ()),
        PrivateMaintenanceReach::StoreGone
    ));
    drop(home);
}

#[test]
fn a_name_whose_store_has_gone_says_so_and_holds_nothing_up() {
    // A NAME IS A NAME, NOT CUSTODY. It holds its store weakly, so a store
    // whose legitimate holders have all gone drops with everything in it --
    // and the name says the store has gone rather than that the place moved.
    let capability;
    let identity;
    {
        let durable = PrivateSettlementOwner::default();
        capability = durable.settlement_ref();
        let client = XServerFrontendClientId(8450);
        let service_keeper = service_owner(&durable, 2);
        let private = private_over(&service_keeper, 2);
        let (registration, _channels) = private
            .broker
            .registry
            .register_client_with_admission(client, Some(admitted(client)))
            .expect("a place and a row");
        identity = registration.maintenance_identity().expect("a name");
        assert!(identity.with_home(|_| ()).reached().is_some());
        drop((registration, private, durable));
    }
    assert!(
        capability.owner().is_none(),
        "a name does not keep a store alive"
    );
    assert!(matches!(
        identity.with_home(|_| ()),
        PrivateMaintenanceReach::StoreGone
    ));
}

/// Settle a connection's retained place through the real drive, so it goes
/// back and its number becomes available to a successor.
///
/// Every step is production's: the connection ends, its own teardown retains
/// what it owed, and the store's drive finishes and returns it.
fn settle_and_return(durable: &PrivateSettlementOwner, place: usize) {
    for _ in 0..16 {
        durable.drive_ordered_continuations(4);
        if matches!(
            durable.records_even_if_poisoned().continuations.get(place),
            Some(PrivateOrderedContinuationPlace::Free)
        ) {
            return;
        }
    }
    panic!("the drive did not return this place");
}

#[test]
fn a_stale_preparations_drop_leaves_its_successors_promise_alone() {
    // A DESTINATION ENTRY IS REUSABLE NOW, which is what makes this possible:
    // returning a place frees its destination at once, so a successor can be
    // reserved and prepared while an old preparation is still in somebody's
    // hand. A drop that recognised only "promised" would put that successor's
    // promise back to reserved and give its count away, and the successor
    // could then be prepared twice over.
    //
    // Both generations use the same number deliberately; what separates them
    // is which home occupies the place.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let lease = durable
        .reserve_ordered_continuation()
        .expect("a declared bound leaves a place");
    let place = lease.index;
    let stale = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    lease.relinquish_unexposed();
    assert_eq!(durable.continuations_reserved(), Some(0));

    // A REAL SUCCESSOR AT THE SAME NUMBER, with a preparation of its own.
    let client = XServerFrontendClientId(8452);
    let (successor, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let successor_lease = lease_of(&successor);
    assert_eq!(successor_lease.index, place, "the number was handed on");
    let kept = durable
        .prepare_internal_holder(&successor_lease)
        .expect("the successor's own destination");
    assert_eq!(durable.holders_taken(), Some(1));

    // THE OLD PREPARATION GOES, and takes nothing of the successor's with it.
    drop(stale);
    assert_eq!(
        maintenance_destination(&durable, place),
        Some("promised"),
        "the successor's promise is still its own"
    );
    assert_eq!(durable.holders_taken(), Some(1), "and still counted once");
    assert!(
        matches!(
            durable.prepare_internal_holder(&successor_lease),
            Err(PrivateHolderRefusal::AlreadyHeld)
        ),
        "so it cannot be prepared a second time"
    );
    drop((kept, successor, channels, successor_lease, private));
}

#[test]
fn a_stale_lease_cannot_prepare_its_successors_destination() {
    // A LEASE CAN OUTLIVE ITS PLACE. The drive returns a place whose work is
    // settled and does not consult the lease that reserved it, so a caller
    // holding one after that is holding a number somebody else now has.
    // Preparing on the strength of it takes the successor's destination and
    // leaves the successor unable to prepare its own.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8453);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 84530);
    let stale_lease = lease_of(&registration);
    let place = stale_lease.index;
    // Its capsule is taken out, so the drive can finish the record.
    assert!(registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().is_ok())
        .expect("its own home"));
    drop(registration);
    settle_and_return(&durable, place);
    assert_eq!(durable.continuations_reserved(), Some(0));

    // A REAL SUCCESSOR TAKES THE NUMBER.
    let second = XServerFrontendClientId(8454);
    let (successor, successor_registration, successor_cell, successor_frames, _wire, _successor_keeper) =
        converted_fixture(&durable, second, 84540);
    let successor_lease = lease_of(&successor_registration);
    assert_eq!(successor_lease.index, place);

    // THE STALE LEASE IS REFUSED, and the successor is untouched.
    assert!(
        matches!(
            durable.prepare_internal_holder(&stale_lease),
            Err(PrivateHolderRefusal::Stale)
        ),
        "a lease whose place has gone back prepares nothing"
    );
    assert_eq!(
        maintenance_destination(&durable, place),
        Some("reserved"),
        "the successor's destination is still reserved for it"
    );
    assert!(
        durable.prepare_internal_holder(&successor_lease).is_ok(),
        "and the successor can still prepare its own"
    );
    let capsule = successor_registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("its own home")
        .expect("the successor's own capsule");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(84540)
    );
    assert_eq!(order_pass_frames(&capsule), successor_frames);
    assert!(Arc::ptr_eq(
        &successor_cell,
        &capsule.finalizer().expect("carried").completion
    ));
    assert!(successor_cell.answer().is_none());
    drop((
        private,
        successor,
        successor_registration,
        successor_lease,
        stale_lease,
        capsule,
    ));
}

#[test]
fn a_conversion_whose_place_moved_on_disturbs_nothing() {
    // THE CHECK AND THE PUBLICATION ARE TWO ACQUISITIONS, with the store
    // released in between. A place whose work settles in that gap goes back,
    // and the next connection takes the number and prepares a destination of
    // its own -- so publishing on the strength of the earlier check would
    // replace that connection's promise with a holder naming a home it never
    // had.
    //
    // WHAT THIS CONTROL ACTUALLY REACHES, said plainly: the place goes back
    // before the conversion is asked at all, so the FIRST of the two checks
    // catches it and the answer is NoPlace. The revalidation in the second
    // acquisition -- the one that matters when the place moves in the gap --
    // is not reached from here, and cannot be: the gap is inside one call and
    // closing it needs a schedule this control cannot impose. That the
    // successor is left untouched is established either way, and it is the
    // part a caller depends on.
    let durable = PrivateSettlementOwner::default();
    let client = XServerFrontendClientId(8455);
    let (private, registration, _cell, _frames, _wire, _private_keeper) =
        converted_fixture(&durable, client, 84550);
    let lease = lease_of(&registration);
    let place = lease.index;
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    assert!(registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().is_ok())
        .expect("its own home"));
    drop(registration);
    settle_and_return(&durable, place);

    // A REAL SUCCESSOR AT THE SAME NUMBER, with its own destination reserved.
    let second = XServerFrontendClientId(8456);
    let (successor, successor_registration, successor_cell, successor_frames, _wire, _successor_keeper) =
        converted_fixture(&durable, second, 84560);
    assert_eq!(
        maintenance_destination(&durable, place),
        Some("reserved"),
        "the successor's own"
    );

    // THE OLD CONVERSION IS ASKED AND REFUSES.
    let outer = durable.clone();
    let PrivateInternalConversion::NoPlace(lease) = lease.convert_to_internal(&outer, destination)
    else {
        panic!("a place that moved on is not this lease's to commit")
    };
    assert_eq!(
        maintenance_destination(&durable, place),
        Some("reserved"),
        "the successor's destination is untouched"
    );
    assert!(durable.take_internal_holder(0).is_none(), "no holder was made");
    assert_eq!(
        durable.continuations_abandoned(),
        Some(0),
        "and nothing was marked against the successor"
    );
    let capsule = successor_registration
        .ordered_home
        .borrow(|continuation| continuation.queue().try_recv().ok())
        .expect("its own home")
        .expect("the successor's own capsule");
    assert_eq!(order_pass_frames(&capsule), successor_frames);
    assert!(Arc::ptr_eq(
        &successor_cell,
        &capsule.finalizer().expect("carried").completion
    ));
    assert!(successor_cell.answer().is_none());
    drop((private, successor, successor_registration, lease, outer, capsule));
}

#[test]
fn a_publication_that_is_refused_leaves_exactly_the_first_connections_reservation() {
    // THE REFUSAL PRODUCTION ACTUALLY HAS. A second registration for a client
    // that already has one is refused at publication, after its own place and
    // destination were reserved -- and the existing unexposed release is what
    // gives both back. The first connection keeps exactly what it had.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8457);
    let (first, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let identity = first.maintenance_identity().expect("a name");
    assert_eq!(durable.continuations_reserved(), Some(1));

    assert!(
        matches!(
            private
                .broker
                .registry
                .register_client_with_admission(client, Some(admitted(client))),
            Err(XServerFrontendRouteError::DuplicateClient { .. })
        ),
        "a client that already has a row is refused"
    );

    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the refused registration gave its place back"
    );
    assert_eq!(
        maintenance_destination(&durable, identity.place()),
        Some("reserved"),
        "and exactly the first connection's destination is left"
    );
    assert_eq!(
        durable
            .records_even_if_poisoned()
            .holders
            .iter()
            .filter(|place| !matches!(place, PrivateHolderPlace::Free))
            .count(),
        1,
        "one destination, not two"
    );
    assert!(
        identity
            .with_home(|home| Arc::ptr_eq(home, &first.ordered_home))
            .reached()
            .expect("still its own"),
        "and the first connection's name still resolves to its own home"
    );
    drop((first, private));
}

/// A registration of its own, to stand in a fixture's field while the real one
/// is dropped.
///
/// The fixture owns its registration and other records borrow its slot and
/// gate, so a control that needs the connection to END cannot simply move the
/// registration out. This puts an unrelated one in its place; nothing in the
/// control touches it.
fn spare_registration(
    f: &PreparedOrderedFixture,
) -> XServerFrontendClientRouteRegistration {
    let client = XServerFrontendClientId(f.client.raw() + 900_000);
    f.runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row")
        .0
}

/// A connection carried all the way to committable evidence: real startup,
/// real body, real join, real fencing.
///
/// THE ASSOCIATION IS THIS FIXTURE'S, built from one registration and that
/// connection's own startup, join and gate. Nothing in the types establishes
/// which connection a worker served, and this does not claim otherwise.
struct PrivateCommitFixture {
    g: PrivateFenceFixture,
    place: usize,
}

fn commit_fixture(client: XServerFrontendClientId, panicking: bool) -> PrivateCommitFixture {
    let f = worker_fixture(client);
    f.permit();
    let gate = f.fixture.registration.handover_gate();
    let place = f
        .fixture
        .registration
        .maintenance_identity()
        .expect("a place, so a name")
        .place();
    let (home, wake, stop, sequence) = f.handles();
    {
        let custody = custody_for(&f, &f.fixture.keeper);
        let running = Arc::clone(custody.exit_sink());
        started_worker(&custody, &f, move || {
            if panicking {
                panic!("what this connection's worker carried out with it");
            }
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit: &running,
                steps: 16,
            }
            .run();
        });
    }
    if !panicking {
        cancel_connection_worker(&f.stop, &f.wake);
    }
    PrivateCommitFixture {
        g: PrivateFenceFixture { f, gate },
        place,
    }
}

#[test]
fn a_commitment_waits_for_the_evidence_it_rests_on() {
    // COMPLETED, PUBLISHED EVIDENCE AND NOTHING WEAKER. A fencing that has not
    // recorded what the gate said, and a join that has published no result,
    // are both reasons this connection's obligation cannot yet be stated --
    // and neither is a reason to go and produce one.
    let c = commit_fixture(XServerFrontendClientId(8461), false);
    let durable = c.g.f.fixture.durable.clone();
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("the destination reserved with this place");
    let outer = durable.clone();
    // The fixture's own spare registration has already ended, so this store's
    // abandonment account is not zero. What matters is that nothing below
    // moves it.
    let abandoned_before = durable.continuations_abandoned();

    // NEITHER JOINED NOR FENCED.
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(matches!(context.commit(), PrivateCommitted::NotYetEvidenced));
    // NOTHING WAS CONSUMED AND NOTHING WAS CHANGED. The lease and the prepared
    // destination both stay in this context -- the destination still promised
    // to this place -- so the same context simply asks again.
    assert_eq!(
        maintenance_destination(&durable, c.place),
        Some("promised"),
        "the prepared destination stayed in this context, not spent and not \
         handed back"
    );
    assert_eq!(durable.continuations_abandoned(), abandoned_before);

    // JOINED BUT NOT FENCED: still not enough, and still the same context.
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert!(matches!(context.commit(), PrivateCommitted::NotYetEvidenced));
    assert_eq!(
        maintenance_destination(&durable, c.place),
        Some("promised"),
        "the prepared destination stayed in this context across both refusals"
    );

    // AND WHEN THE FENCE HAS RECORDED, THE SAME CONTEXT COMMITS.
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    assert!(matches!(context.commit(), PrivateCommitted::Committed));
    // A SECOND VISIT REPLACES NOTHING.
    assert!(matches!(context.commit(), PrivateCommitted::AlreadyCommitted));
    assert!(
        durable
            .committed_obligation(c.place)
            .is_some(),
        "the obligation is in this connection's own destination"
    );
    drop((c.g.f.fixture, outer));
}

#[test]
fn a_commitment_keeps_the_exact_evidence_after_the_frames_that_made_it_go() {
    // WHAT KEEPS THE EVIDENCE IS THE CUSTODY, TAKEN BEFORE ANY OF THIS BEGAN.
    // A worker that panicked was carrying something; an obligation that
    // recorded only "it panicked" while the payload went with the caller's
    // record would have kept the wrong thing, and a commitment that HANDED the
    // only handle back would have offered a keeper rather than made one.
    //
    // NOTHING IS RETURNED HERE. Every frame that produced this evidence --
    // the reaping record, the fencing record, the commitment context, the
    // borrowed view -- goes away below, and the payload is read afterwards out
    // of the home the custody has owned throughout.
    let c = commit_fixture(XServerFrontendClientId(8462), true);
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let durable = c.g.f.fixture.durable.clone();
    let place = c.place;
    {
        let view = custody.view();
        // A view reaches everything the custody has, and owns none of it.
        assert!(view.identity().same_as(custody.identity()));
        assert!(Arc::ptr_eq(view.join(), custody.join()));
        let lease = lease_of(&c.g.f.fixture.registration);
        let record = PrivateReapingRecord::bound_to(&custody);
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let destination = view
            .store()
            .prepare_internal_holder(&lease)
            .expect("its own destination");
        let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
        assert!(matches!(context.commit(), PrivateCommitted::Committed));
        // And the borrowed view ends here, which is the end of a borrow.
    }

    let obligation = durable
        .committed_obligation(place)
        .expect("the store names this connection's obligation");
    assert_eq!(obligation.closed(), PrivateHandoverFence::Established);
    assert_eq!(obligation.identity().place(), place);
    let evidence = obligation
        .join()
        .expect("its custodian still owns the evidence");
    assert!(
        Arc::ptr_eq(&evidence, custody.join()),
        "the very home the custody has owned since before the join"
    );
    let PrivateJoinResult::Panicked(payload) = evidence.result().expect("a completed join") else {
        panic!("this worker panicked")
    };
    assert_eq!(
        payload
            .lock()
            .expect("a readable payload")
            .downcast_ref::<&str>()
            .copied(),
        Some("what this connection's worker carried out with it"),
        "the exact payload, after every frame that made it has gone"
    );
    drop(evidence);

    // A READER LETTING GO IS NOT THE END OF IT. This pin was one handle among
    // others; what keeps this home is the service owner's inventory, and it is
    // still there.
    drop(custody);
    assert!(
        durable
            .committed_obligation(place)
            .expect("the obligation is still here")
            .join()
            .is_some(),
        "an operation's handle going is not the keeper letting go"
    );

    // AND ONLY THE ULTIMATE OWNER LETTING GO ENDS IT. The fixture holds the
    // service owner and drops it last, after the instance it kept for. The
    // obligation then says the evidence has gone -- which is not a
    // disposition and not a fresh fact about the join.
    drop(c.g.f.fixture);
    assert!(
        durable
            .committed_obligation(place)
            .expect("the obligation is still here")
            .join()
            .is_none(),
        "the store named it and did not own it"
    );
}

#[test]
fn a_commitment_records_what_the_gate_said_whichever_it_was() {
    // ALL THREE STAY DISTINCT. An unreadable gate is not a closed one, and
    // recording it as established would claim something nobody established --
    // but it is also not a reason to refuse the responsibility, because
    // something is still owed and that case is what this retention is for.
    for (client, arrange, expected) in [
        (
            8463u64,
            None::<fn(&PrivateCommitFixture)>,
            PrivateHandoverFence::Established,
        ),
        (
            8464,
            Some((|c: &PrivateCommitFixture| {
                assert_eq!(
                    c.g.f.fixture.registration.fence_ordered_handovers(),
                    PrivateHandoverFence::Established,
                    "somebody else closed it first, through the real API"
                );
            }) as fn(&PrivateCommitFixture)),
            PrivateHandoverFence::AlreadyEstablished,
        ),
        (
            8465,
            Some((|c: &PrivateCommitFixture| {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _inside = c.g.gate.fenced.lock().expect("a readable gate");
                        panic!("a holder unwound inside this connection's gate");
                    }))
                    .is_err(),
                    "the holder unwound"
                );
            }) as fn(&PrivateCommitFixture)),
            PrivateHandoverFence::Unreadable,
        ),
    ] {
        let c = commit_fixture(XServerFrontendClientId(client), false);
        let durable = c.g.f.fixture.durable.clone();
        let outer = durable.clone();
        let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
        let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        if let Some(arrange) = arrange {
            arrange(&c);
        }
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let destination = durable
            .prepare_internal_holder(&lease)
            .expect("its own destination");
        let context =
            PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
        assert!(matches!(context.commit(), PrivateCommitted::Committed));
        assert_eq!(
            durable
                .committed_obligation(c.place)
                .expect("an obligation")
                .closed(),
            expected,
            "exactly what the gate said"
        );
        drop((c.g.f.fixture, outer));
    }
}
