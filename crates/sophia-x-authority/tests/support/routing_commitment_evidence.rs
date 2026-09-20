// Commitments and what they record: the gate's answer whichever it was, and a
// successor at one number that does not take its predecessor's evidence.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_commitment_is_not_gated_by_a_diagnostic_somebody_is_holding() {
    // THE PUBLISHED EVIDENCE IS READ DIRECTLY. Whether anybody is holding the
    // exit diagnostic or the panic payload has nothing to do with whether this
    // connection's worker finished or its gate was asked.
    let c = commit_fixture(XServerFrontendClientId(8466), true);
    let durable = c.g.f.fixture.durable.clone();
    let outer = durable.clone();
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");

    let PrivateJoinResult::Panicked(payload) = record.result().expect("a completed join") else {
        panic!("this worker panicked")
    };
    let payload_held = payload.lock().expect("a readable payload");
    let diagnostic_held = custody
        .exit_sink()
        .outcome
        .lock()
        .expect("a readable exit record");
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(
        matches!(context.commit(), PrivateCommitted::Committed),
        "neither lock is on the way to the obligation"
    );
    // The pin goes before the keeper it borrows, which is the order the
    // borrow checker now insists on and the order the thing itself has.
    drop((payload_held, diagnostic_held, outer));
    drop(custody);
    drop(c.g.f.fixture);
}

#[test]
fn a_commitment_takes_no_further_credit_and_moves_the_duty_once() {
    // THE SAME PLACE, THE SAME NAME, THE SAME COUNT. Committing states what a
    // connection is owed; it does not charge for a second place, and it leaves
    // exactly one holder owing this place's disposal.
    let c = commit_fixture(XServerFrontendClientId(8467), false);
    let durable = c.g.f.fixture.durable.clone();
    let outer = durable.clone();
    let identity = c
        .g
        .f
        .fixture
        .registration
        .maintenance_identity()
        .expect("a name");
    let reserved_before = durable.continuations_reserved();
    let abandoned_before = durable.continuations_abandoned();
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(matches!(context.commit(), PrivateCommitted::Committed));

    assert_eq!(durable.continuations_reserved(), reserved_before);
    assert_eq!(durable.continuations_abandoned(), abandoned_before);
    assert!(
        identity.same_as(
            durable
                .committed_obligation(c.place)
                .expect("an obligation")
                .identity()
        ),
        "the same name it was reserved under"
    );
    // ONE HOLDER OWES THIS PLACE. Taking it out and dropping it marks the
    // place once, which it could not do if the lease still owed it too.
    let named = durable
        .take_internal_holder(c.place)
        .expect("the store's holder");
    drop(named);
    assert_eq!(
        durable.continuations_abandoned(),
        abandoned_before.map(|before| before + 1),
        "once, which it could not be if the lease still owed it too"
    );
    drop((c.g.f.fixture, outer));
}

#[test]
fn a_second_commitment_replaces_nothing() {
    // A REPEATED REQUEST MUST NOT RESTATE THE OBLIGATION. The first evidence
    // stands, and a second attempt finds its destination already holding one.
    let c = commit_fixture(XServerFrontendClientId(8468), false);
    let durable = c.g.f.fixture.durable.clone();
    let outer = durable.clone();
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(matches!(context.commit(), PrivateCommitted::Committed));

    // THERE IS NO SECOND LEASE TO ASK WITH: the first commitment consumed it,
    // and the registration has none to give.
    assert!(
        c.g.f
            .fixture
            .registration
            .maintenance_identity()
            .is_none(),
        "this registration is not holding a lease any more"
    );
    assert_eq!(
        maintenance_destination(&durable, c.place),
        Some("taken"),
        "and its destination holds the first obligation"
    );
    // THE OBLIGATION IS THE FIRST ONE, with its own evidence.
    assert_eq!(
        durable
            .committed_obligation(c.place)
            .expect("an obligation")
            .closed(),
        PrivateHandoverFence::Established
    );
    drop((c.g.f.fixture, outer));
}

#[test]
fn a_commitment_leaves_no_store_self_cycle() {
    // AN OBLIGATION IS KEPT BY THE STORE, so nothing it names may keep the
    // store. WHAT ACTUALLY MAKES THAT TRUE is the weak edge: the obligation
    // names its evidence rather than owning it, and its identity names the
    // store weakly too.
    //
    // AND THIS CONTROL COVERS THE RETURNED CASE ONLY. A join that panicked
    // carries whatever the frame was carrying, which may be anything at all
    // including a handle to this store; that the graph still releases then
    // rests on the custodian being outside it. The control that shows THAT is
    // a_payload_holding_the_store_is_a_chain_from_its_custodian, where the
    // payload really is a store handle -- not the neighbouring panic controls,
    // whose payloads are strings and establish nothing about this graph.
    let capability;
    let outer;
    {
        let c = commit_fixture(XServerFrontendClientId(8469), false);
        let durable = c.g.f.fixture.durable.clone();
        capability = durable.settlement_ref();
        outer = durable.clone();
        let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
        let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let destination = durable
            .prepare_internal_holder(&lease)
            .expect("its own destination");
        let context =
            PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
        assert!(matches!(context.commit(), PrivateCommitted::Committed));
        drop((c.g.f.fixture, durable));
    }
    assert!(
        capability.owner().is_some(),
        "the caller's own holder keeps it"
    );
    drop(outer);
    assert!(
        capability.owner().is_none(),
        "and a committed obligation is on no ring"
    );
}

#[test]
fn a_commitment_whose_place_moved_on_leaves_the_successor_alone() {
    // THE OCCUPANT IS CHECKED IN THE ACQUISITION THAT INSTALLS. A place whose
    // work settles goes back and the next connection takes the number, so a
    // commitment resting on a lease that has outlived its place would state an
    // obligation into the successor's destination on the strength of evidence
    // about somebody else's connection.
    //
    // NO HOOK: the place goes back before the commitment is asked. The
    // preparation happens first, because a stale lease can no longer prepare.
    let mut c = commit_fixture(XServerFrontendClientId(8470), false);
    let durable = c.g.f.fixture.durable.clone();
    let outer = durable.clone();
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination, while it is still its own");

    // The connection ends and the real drive returns its place. The reaping
    // and fencing records borrow the slot and gate, which the fixture still
    // owns, so only the registration goes here.
    let place = c.place;
    // Its row and this control's own captured sender both go: the drive can
    // only finish a queue whose producers have all disappeared, and a sender
    // this fixture is still holding is a producer.
    let stand_in = spare_registration(&c.g.f.fixture);
    let stand_in_client = stand_in.client;
    let registration = std::mem::replace(&mut c.g.f.fixture.registration, stand_in);
    let spare_sender = capture_gated_sender(
        c.g.f.fixture.runner.frontend.as_ref().unwrap(),
        stand_in_client,
    );
    let sender = std::mem::replace(&mut c.g.f.sender, spare_sender);
    // STAGE-ONLY: THE DEFERRED DUTY, EXECUTED BY THE CONTROL. This
    // registration's worker was joined above, so its destruction defers
    // rather than tearing down, and the place this control needs to move on
    // would stay held. The executor a later boundary attaches is stood in
    // for here by running the record's synchronous body directly; the drop
    // below then records a deferral and runs nothing.
    registration.cleanup.run_synchronous_cleanup();
    drop((registration, sender));
    // This connection's place is back; the stand-in above holds one of its
    // own, so the store's total is not zero and nothing here claims it is.
    settle_and_return(&durable, place);

    // A REAL SUCCESSOR TAKES THE NUMBER.
    let second = XServerFrontendClientId(8471);
    let (successor, successor_registration, successor_cell, successor_frames, _wire, _successor_keeper) =
        converted_fixture(&durable, second, 84710);
    let abandoned_before = durable.continuations_abandoned();
    // THE SUCCESSOR PREPARES ITS OWN DESTINATION FIRST, which is what
    // separates the two halves of the check: the entry is Promised for this
    // number again, so a commitment that asked only about the promise would
    // find it satisfied. What differs is the home.
    let successor_lease = lease_of(&successor_registration);
    let successors_own = durable
        .prepare_internal_holder(&successor_lease)
        .expect("the successor's own destination");
    assert_eq!(maintenance_destination(&durable, place), Some("promised"));

    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(matches!(context.commit(), PrivateCommitted::Stale));
    assert_eq!(
        maintenance_destination(&durable, place),
        Some("promised"),
        "the successor's own promise is untouched"
    );
    assert!(
        durable.committed_obligation(place).is_none(),
        "and nothing was committed into it"
    );
    assert_eq!(
        durable.continuations_abandoned(),
        abandoned_before,
        "nothing was marked on the successor's account"
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
    drop((
        successor,
        successor_registration,
        successor_lease,
        successors_own,
        capsule,
        outer,
    ));
}

#[test]
fn a_commitment_with_a_destination_from_elsewhere_is_refused() {
    // A DESTINATION BELONGS TO ONE PLACE IN ONE STORE. Committing through one
    // from another store, or one prepared for another place, would state this
    // connection's obligation into storage that was never its own.
    let c = commit_fixture(XServerFrontendClientId(8472), false);
    let durable = c.g.f.fixture.durable.clone();
    let outer = durable.clone();
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);

    // Another store, with a connection and a destination of its own.
    let elsewhere = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&elsewhere, 2);
    let other_private = private_over(&service_keeper, 2);
    let other = XServerFrontendClientId(8473);
    let (other_registration, _other_channels) = other_private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a place and a row");
    let other_lease = lease_of(&other_registration);
    let foreign = elsewhere
        .prepare_internal_holder(&other_lease)
        .expect("the other store's own destination");

    let foreign_context =
        PrivateCommitmentContext::bound_to(&custody, &fence, lease, foreign);
    assert!(matches!(foreign_context.commit(), PrivateCommitted::Foreign));
    assert!(durable.committed_obligation(c.place).is_none());
    assert_eq!(
        maintenance_destination(&durable, c.place),
        Some("reserved"),
        "this connection's own destination was never touched"
    );

    // And a context over its own destination still commits.
    let lease = foreign_context
        .into_parts()
        .expect("nothing was consumed")
        .0;
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(matches!(context.commit(), PrivateCommitted::Committed));
    drop((
        c.g.f.fixture,
        other_registration,
        other_private,
        other_lease,
        outer,
    ));
}

#[test]
fn an_operation_that_unwinds_loses_the_operation_and_not_the_result() {
    // THE WHOLE POINT OF TAKING CUSTODY FIRST. A reaping publishes into a home
    // its custodian already owned, so a frame that returns -- or one that
    // unwinds -- takes itself away and leaves the result where it was.
    //
    // THE UNWIND IS THE CALLER'S, AFTER THE REAL OPERATION RETURNED. It
    // establishes that boundary and no other: nothing here witnesses an
    // arbitrary instruction inside a reaping.
    let f = worker_fixture(XServerFrontendClientId(8481));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || panic!("what the custodian keeps"));

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let record = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        panic!("the frame that did the reaping goes here");
    }));
    assert!(unwound.is_err(), "the operation's frame unwound");

    // AND THE RESULT IS EXACTLY WHERE IT WAS PUBLISHED.
    let PrivateJoinResult::Panicked(payload) =
        custody.join().result().expect("a completed join")
    else {
        panic!("this worker panicked")
    };
    assert_eq!(
        payload
            .lock()
            .expect("a readable payload")
            .downcast_ref::<&str>()
            .copied(),
        Some("what the custodian keeps"),
        "the exact payload, after the frame that joined it unwound"
    );
    drop(f.fixture);
}

#[test]
fn losing_an_operation_before_a_result_exists_invents_none() {
    // AN UNWIND BEFORE PUBLICATION MUST NOT LEAVE A RESULT. What the home says
    // then is what it said before: not begun, or an attempt that may have
    // consumed a handle -- readable states of this same home, and neither a
    // join outcome.
    let f = worker_fixture(XServerFrontendClientId(8482));
    let custody = custody_for(&f, &f.fixture.keeper);
    let (release, held) = std::sync::mpsc::channel::<()>();
    started_worker(&custody, &f, move || {
        let _ = held.recv();
    });

    assert_eq!(custody.join().phase(), PrivateReapingPhase::NotBegun);
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _record = PrivateReapingRecord::bound_to(&custody);
        panic!("the frame goes before it ever asks");
    }));
    assert!(unwound.is_err());
    assert_eq!(
        custody.join().phase(),
        PrivateReapingPhase::NotBegun,
        "nothing was begun, so nothing says it was"
    );
    assert!(custody.join().result().is_none(), "and no result was invented");

    // The worker is released and joined before the registration goes, because
    // teardown is not worker-aware.
    drop(release);
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    drop(f.fixture);
}

#[test]
fn a_commitment_needs_no_returned_handle_to_keep_its_evidence() {
    // COMMITTING RETURNS NOTHING TO KEEP. The custody owned this home before
    // the reaping began and still does; there is nothing a caller could drop
    // or unwind holding that would cost the result.
    let c = commit_fixture(XServerFrontendClientId(8483), true);
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let durable = c.g.f.fixture.durable.clone();
    let place = c.place;
    let lease = lease_of(&c.g.f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    // The outcome is a word, not a handle: there is nothing here to discard.
    assert!(matches!(context.commit(), PrivateCommitted::Committed));

    // AND THE COMMITTED NAME RESOLVES THE EXACT ORIGINAL HOME.
    let named = durable
        .committed_obligation(place)
        .expect("an obligation")
        .join()
        .expect("its custodian owns it");
    assert!(Arc::ptr_eq(&named, custody.join()));
    drop(custody);
    drop(c.g.f.fixture);
}

#[test]
fn a_commitment_refuses_another_connections_evidence() {
    // A COMMITMENT CANNOT BE GIVEN ANY LIVE KEEPER. Its custody must be about
    // the connection whose place it is committing, by name AND by home.
    //
    // TWO SEPARATE MISTAKES, and each is made here with everything else right,
    // so neither is caught by a check that was already there. Both custodies
    // belong to the same store, so the store comparison cannot refuse either.
    let c = commit_fixture(XServerFrontendClientId(8484), false);
    let durable = c.g.f.fixture.durable.clone();
    // A SIBLING OF THE SAME STORE, AND ITS OWN REGISTERED CUSTODY -- the one
    // its registration reserved, not one made beside it. Its place, its home,
    // its slot and its exit record are all that connection's.
    let sibling = spare_registration(&c.g.f.fixture);
    let PrivateCustodyReach::Reached(sibling_custody) = sibling
        .registered_custody(&c.g.f.fixture.keeper.lease())
        .expect("a sibling registration reserves a custody of its own")
    else {
        panic!("the same owner keeps it")
    };
    // BOTH CUSTODIES BEFORE THE LEASE IS TAKEN: a name is handed out from the
    // registration's lease, so a custody asked for after the lease has gone
    // has nothing to be about.
    let twin = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let lease = lease_of(&c.g.f.fixture.registration);

    // (1) THE WRONG NAME. The whole operation runs against the sibling's
    // custody -- ITS slot, ITS exit record and ITS home, because a reaping
    // view can no longer be handed parts from two connections. So the
    // fencing's evidence IS the home this context names, and the sibling's
    // store is this store. Only the name says this reservation is not the one
    // that custody is about.
    //
    // The sibling's own worker, in the sibling's own slot, so there is a real
    // join for that custody to have.
    started_worker(&sibling_custody, &c.g.f, || {});
    let misnamed = PrivateReapingRecord::bound_to(&sibling_custody);
    let misfence = PrivateFenceRecord::bound_to(&sibling_custody);
    assert_eq!(misnamed.reap().reaped, PrivateReaped::Joined);
    assert_eq!(misfence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    let wrong_name =
        PrivateCommitmentContext::bound_to(&sibling_custody, &misfence, lease, destination);
    assert!(
        matches!(wrong_name.commit(), PrivateCommitted::Foreign),
        "the store and the home matched, and the name did not"
    );
    assert!(
        durable.committed_obligation(c.place).is_none(),
        "nothing was stated about this connection"
    );

    // (2) THE WRONG HOME. This custody has the RIGHT name -- it is about this
    // very connection -- but it is not the keeper the fencing published into,
    // so an obligation naming its home would name a home holding nothing.
    let (lease, destination) = wrong_name.into_parts().expect("nothing was consumed");
    assert!(twin.identity().same_as(&lease.maintenance_identity()));
    let wrong_home = PrivateCommitmentContext::bound_to(&twin, &misfence, lease, destination);
    assert!(
        matches!(wrong_home.commit(), PrivateCommitted::Foreign),
        "the store and the name matched, and the home did not"
    );
    assert!(durable.committed_obligation(c.place).is_none());

    // AND THE REFUSED CONTEXT KEPT WHAT IT WAS GIVEN, so the correct
    // commitment still happens -- which is what makes the two refusals above
    // refusals rather than breakage.
    let (lease, destination) = wrong_home.into_parts().expect("nothing was consumed");

    // THIS CONNECTION'S OWN WORKER, joined through its own custody. That is
    // also this control's obligation to the thread it started.
    let own = PrivateReapingRecord::bound_to(&twin);
    assert_eq!(
        own.reap().reaped,
        PrivateReaped::Joined,
        "its own handle was never the sibling's to take"
    );
    let own_fence = PrivateFenceRecord::bound_to(&twin);
    assert_eq!(own_fence.record_fence(), PrivateFenced::Recorded);
    let context = PrivateCommitmentContext::bound_to(&twin, &own_fence, lease, destination);
    assert!(
        matches!(context.commit(), PrivateCommitted::Committed),
        "its own name over its own home commits"
    );
    let stated = durable
        .committed_obligation(c.place)
        .expect("the obligation is stated now");
    assert!(
        Arc::ptr_eq(
            &stated.join().expect("its custodian owns it"),
            twin.join()
        ),
        "over the home its own custody has owned throughout"
    );
    drop(stated);
    drop(context);
    drop(own);
    drop((sibling_custody, twin));
    drop((c.g.f.fixture, sibling));
}

#[test]
fn a_completed_join_cannot_be_withdrawn_by_a_later_view() {
    // WHAT SHARING THE HOME COST, AND WHAT PAYS FOR IT. When each record kept
    // its own claim, a second view over the same source and the same custody
    // arrived with a claim of its own, made an empty attempt and withdrew the
    // phase of a join that had already completed: the result sat in storage
    // and stopped being readable, and the obligation naming it read None.
    //
    // NO HOOK, NO SECOND WORKER, NO SCHEDULE. The first view is simply gone by
    // the time the second one is made.
    let c = commit_fixture(XServerFrontendClientId(8487), true);
    let custody = custody_for(&c.g.f, &c.g.f.fixture.keeper);
    let durable = c.g.f.fixture.durable.clone();
    let lease = lease_of(&c.g.f.fixture.registration);
    {
        let first = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(first.reap().reaped, PrivateReaped::Joined);
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let destination = durable
            .prepare_internal_holder(&lease)
            .expect("its own destination");
        let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
        assert!(matches!(context.commit(), PrivateCommitted::Committed));
    }
    let stated = durable
        .committed_obligation(c.place)
        .expect("the obligation is stated");
    assert!(
        stated
            .join()
            .expect("its evidence is still kept")
            .result()
            .is_some(),
        "the join it rests on is readable before the second view exists"
    );

    // A SECOND VIEW OF THE SAME SOURCE AND THE SAME CUSTODY. It is refused,
    // because the one right to publish into that home went with the attempt
    // that consumed the handle and is not something a new view can mint.
    let second = PrivateReapingRecord::bound_to(&custody);
    let repeated = second.reap();
    assert_eq!(repeated.reaped, PrivateReaped::NotThePublisher);
    assert_eq!(repeated.exit, None, "it read nothing, having done nothing");
    assert_eq!(
        custody.join().phase(),
        PrivateReapingPhase::Joined,
        "a view that is not the producer cannot withdraw a completed phase"
    );
    assert!(
        stated
            .join()
            .expect("its evidence is still kept")
            .result()
            .is_some(),
        "and the obligation still reads the join it rests on"
    );
    assert_eq!(
        panic_payload_of(custody.join()).as_deref(),
        Some("what this connection's worker carried out with it"),
        "the original result, and not a fresh one"
    );
    drop(custody);
    drop(c.g.f.fixture);
}

#[test]
fn two_views_of_one_home_do_not_both_publish() {
    // TWO SEPARATE RECORDS OVER ONE HOME, ASKING TOGETHER. This is the case a
    // record-local claim never covered: neither view has been asked before, so
    // each would have claimed itself and gone on to write the other's phase.
    //
    // THE OVERLAP IS WITNESSED, NOT HOPED FOR. The worker is held until the
    // slot itself says its handle has gone to a joiner and the refused view
    // has come back.
    let f = worker_fixture(XServerFrontendClientId(8488));
    let custody = custody_for(&f, &f.fixture.keeper);
    let (release, held) = std::sync::mpsc::channel::<()>();
    started_worker(&custody, &f, move || {
        let _ = held.recv();
        panic!("what exactly one view keeps");
    });

    // BOTH VIEWS REACH FOR THE RIGHT AT THE SAME MOMENT. Each is built before
    // the rendezvous, so what the two threads do after it is the acquisition
    // itself: a right that was looked at and then taken, rather than taken in
    // one exchange, has its window here.
    let together = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let (report, asked) = std::sync::mpsc::channel();
        for _ in 0..2 {
            let custody = &custody;
            let together = &together;
            let report = report.clone();
            scope.spawn(move || {
                let view = PrivateReapingRecord::bound_to(custody);
                together.wait();
                report.send(view.reap().reaped)
            });
        }
        drop(report);

        assert!(
            waited_for(|| {
                custody.worker_slot().lock().expect("a readable slot").life == PrivateWorkerLife::HandedToJoiner
            }),
            "one view took the handle while its worker is still running"
        );
        // AND THE OTHER IS ALREADY BACK, while that worker is still held: the
        // only view that can return now is the one refused the right, because
        // the one that took it is waiting on the thread.
        assert_eq!(
            asked
                .recv_timeout(Duration::from_secs(3))
                .expect("the refused view returned"),
            PrivateReaped::NotThePublisher
        );
        assert_eq!(
            custody.join().phase(),
            PrivateReapingPhase::InProgress,
            "a handle is consumed and no result is confirmed"
        );
        assert!(custody.join().result().is_none());

        drop(release);
        assert_eq!(
            asked
                .recv_timeout(Duration::from_secs(3))
                .expect("the publishing view returned"),
            PrivateReaped::Joined
        );
    });

    assert_eq!(custody.join().phase(), PrivateReapingPhase::Joined);
    assert_eq!(
        panic_payload_of(custody.join()).as_deref(),
        Some("what exactly one view keeps"),
        "one join, one result"
    );
    let state = custody.worker_slot().lock().expect("a readable slot");
    assert!(state.handle.is_none());
    assert_eq!(state.life, PrivateWorkerLife::HandedToJoiner);
    drop(state);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_view_that_consumed_nothing_leaves_the_right_for_the_next() {
    // THE RIGHT IS NOT SPENT BY LOOKING. An attempt that found no handle
    // consumed nothing, so it gives the right back and a later view of the
    // same home still does the real join. Keeping it would have made one early
    // look enough to leave a connection's evidence unpublishable for good.
    let f = worker_fixture(XServerFrontendClientId(8489));
    let custody = custody_for(&f, &f.fixture.keeper);

    // A VIEW THAT ARRIVES BEFORE THE WORKER DOES, and is then gone.
    {
        let early = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(early.reap().reaped, PrivateReaped::NothingStarted);
        assert_eq!(custody.join().phase(), PrivateReapingPhase::NotBegun);
    }
    assert_eq!(
        start_connection_worker(custody.worker_slot(), &f.stop, &f.wake, || {
            std::thread::Builder::new().spawn(|| {})
        }),
        PrivateStartupOutcome::Started
    );

    // A LATER VIEW, WHICH IS A DIFFERENT RECORD, still publishes.
    let later = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(later.reap().reaped, PrivateReaped::Joined);
    assert!(matches!(
        custody.join().result(),
        Some(PrivateJoinResult::Returned)
    ));
    assert_eq!(custody.join().phase(), PrivateReapingPhase::Joined);

    // AND NOW IT IS SPENT, because that one consumed a handle.
    let third = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(third.reap().reaped, PrivateReaped::NotThePublisher);
    assert_eq!(custody.join().phase(), PrivateReapingPhase::Joined);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_payload_holding_the_store_is_a_chain_from_its_custodian() {
    // THE CASE THE EXTERNAL KEEPER EXISTS FOR. A panic payload is whatever the
    // frame was carrying, and it may be a handle to this very store. If the
    // store owned the evidence, that would be a ring with itself: store,
    // holder, obligation, evidence, payload, store.
    //
    // It does not own it. The custodian does, so the chain runs custodian ->
    // evidence -> payload -> store, and letting the custodian go releases all
    // of it -- with nothing broken by hand in cleanup.
    // EXACTLY ONE WORKER IS STARTED HERE, and this control joins it. A fixture
    // that starts a worker of its own and then starts a second would be
    // dropping its connection with a thread it never collected -- cancelling
    // one is not joining it -- so this assembles the pieces from a worker
    // fixture rather than taking a fixture that has already started one.
    let f = worker_fixture(XServerFrontendClientId(8486));
    f.permit();
    let durable = f.fixture.durable.clone();
    let capability = durable.settlement_ref();
    let custody = custody_for(&f, &f.fixture.keeper);
    let place = f
        .fixture
        .registration
        .maintenance_identity()
        .expect("a place, so a name")
        .place();
    let gate = f.fixture.registration.handover_gate();

    // A real worker panicking with a real handle to this store.
    let carried = durable.clone();
    started_worker(&custody, &f, move || {
        std::panic::panic_any(carried);
    });
    let lease = lease_of(&f.fixture.registration);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    let destination = durable
        .prepare_internal_holder(&lease)
        .expect("its own destination");
    let context = PrivateCommitmentContext::bound_to(&custody, &fence, lease, destination);
    assert!(matches!(context.commit(), PrivateCommitted::Committed));

    // The payload really is a store handle.
    let evidence = Arc::clone(custody.join());
    let PrivateJoinResult::Panicked(payload) = evidence.result().expect("a completed join") else {
        panic!("this worker panicked")
    };
    assert!(
        payload
            .lock()
            .expect("a readable payload")
            .downcast_ref::<PrivateSettlementOwner>()
            .is_some(),
        "it is carrying a handle to this very store"
    );

    // THE OPERATIONS AND THIS CONTROL'S OWN HANDLES GO HERE: the commitment,
    // the fencing, the reaping, the pin, this control's store handle, the
    // worker's slot and exit and the connection's gate.
    //
    // THE FIXTURE IS STILL HOLDING ITS INSTANCE AND ITS OWN STORE HANDLE at
    // this point, and the custodian with them. What the assertion below says
    // is only that the chain is not yet broken -- the release comes after the
    // fixture goes, which is the last drop in this control.
    drop(context);
    drop(record);
    drop(custody);
    drop((durable, gate));
    assert!(
        capability.owner().is_some(),
        "the custodian's chain keeps the store"
    );
    let still_here = f
        .fixture
        .keeper
        .store()
        .committed_obligation(place)
        .expect("the obligation is still here");
    assert!(
        still_here.join().is_some(),
        "and the obligation still reaches its evidence"
    );
    drop(still_here);

    // AND THE CUSTODIAN LETTING GO RELEASES ALL OF IT -- once this reader's
    // own owning handle on the evidence goes too, which is allowed to outlive
    // it and is not what keeps the graph.
    drop(evidence);
    drop(f.fixture);
    assert!(
        capability.owner().is_none(),
        "a payload holding the store is a chain from outside, not a ring"
    );
}

/// An owner established over this store for this many connections.
///
/// THE OWNER IS THE CONTROL'S OWN LOCAL, declared before whatever it is about
/// to build and dropped after it. That is the shape the construction path
/// requires, and a control that let it go first would be testing a service
/// whose keeper had already gone.
fn service_owner(
    store: &crate::PrivateSettlementOwner,
    connections: usize,
) -> crate::PrivateServiceOwner {
    crate::PrivateServiceOwner::established_over(
        store,
        NonZeroUsize::new(connections).expect("a control declares a real bound"),
    )
    .expect("a readable store declares its bound")
}

#[test]
fn a_connections_evidence_keeper_is_reserved_before_its_row_is_published() {
    // THE INTERVAL, OBSERVED FROM INSIDE IT. Seeing a custody and a row both
    // present afterwards says nothing about which came first, and counting one
    // before and one after says only that the total did not change. This holds
    // the client table itself -- the lock publication must take -- so a second
    // registration is stopped exactly between preparing and publishing, and
    // its OWN entry is counted while it waits there.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(8501);
    let (first, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper and a row");
    assert_eq!(keeper.custodies_kept(), 1);
    assert_eq!(keeper.custody_capacity_remaining(), 3);
    let home = {
        let PrivateCustodyReach::Reached(kept) = first
            .registered_custody(&keeper.lease())
            .expect("its own custody")
        else {
            panic!("its owner keeps it")
        };
        Arc::clone(kept.join())
    };

    let registry = private.broker.registry.clone();
    let refused = std::thread::scope(|scope| {
        // THE REAL CLIENT TABLE, held by this control. Nothing is hooked: the
        // registration below runs its ordinary path and stops where it would
        // stop against any other publisher.
        let table = registry.clients.lock().expect("a readable client table");
        let attempt = scope.spawn(|| {
            registry.register_client_with_admission(client, Some(admitted(client)))
        });
        // ITS OWN ENTRY IS HERE WHILE PUBLICATION CANNOT HAVE HAPPENED.
        assert!(
            waited_for(|| keeper.custodies_kept() == 2),
            "the attempt reserved its evidence before reaching publication"
        );
        assert_eq!(keeper.custody_capacity_remaining(), 2);
        assert_eq!(
            durable.continuations_reserved(),
            Some(2),
            "and its place, on the same reservation"
        );
        drop(table);
        attempt.join().expect("the attempt returned")
    });

    // AND PUBLICATION REFUSED IT, so both of that attempt's reservations went
    // back and the live sibling it collided with kept everything of its own.
    assert!(matches!(
        refused,
        Err(XServerFrontendRouteError::DuplicateClient { client: same }) if same == client
    ));
    assert_eq!(
        keeper.custodies_kept(),
        1,
        "the refused attempt gave back exactly its own"
    );
    assert_eq!(keeper.custody_capacity_remaining(), 3);
    assert_eq!(durable.continuations_reserved(), Some(1));
    let PrivateCustodyReach::Reached(again) = first
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner still keeps it")
    };
    assert!(
        Arc::ptr_eq(again.join(), &home),
        "the live connection's home is the one it always had"
    );
    drop(again);
    drop((first, private, registry, keeper));
}

#[test]
fn asking_a_registration_for_its_custody_twice_names_one_home() {
    // A CAPABILITY, NOT A FACTORY. A registration that could make a
    // publication home on demand would be deciding its connection's keeper
    // after that connection was already exposed, and two operations could be
    // publishing into two different homes for one join.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 2);
    let private = private_over(&keeper, 2);
    let client = XServerFrontendClientId(8502);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper and a row");
    let mut homes = Vec::new();
    for _ in 0..3 {
        let PrivateCustodyReach::Reached(pin) =
            registration.registered_custody(&keeper.lease()).expect("its own custody")
        else {
            panic!("its owner keeps it")
        };
        homes.push(Arc::clone(pin.join()));
    }
    assert!(Arc::ptr_eq(&homes[0], &homes[1]) && Arc::ptr_eq(&homes[1], &homes[2]));
    assert_eq!(
        keeper.custodies_kept(),
        1,
        "asking is not reserving: three asks made no second entry"
    );
    // AND IT IS ABOUT THIS CONNECTION'S PLACE.
    let PrivateCustodyReach::Reached(pin) =
        registration.registered_custody(&keeper.lease()).expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert!(
        pin.identity()
            .same_as(&registration.maintenance_identity().expect("a name")),
        "the custody names the place this registration holds"
    );
    drop(pin);
    drop((registration, private, keeper));
}

#[test]
fn one_connections_keeper_is_not_another_connections() {
    // BY NAME AND BY HOME, not by number. Two live connections of one service
    // have two entries, and neither name reaches the other's evidence.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let mut kept = Vec::new();
    for raw in [8503, 8504] {
        let client = XServerFrontendClientId(raw);
        let (registration, _channels) = private
            .broker
            .registry
            .register_client_with_admission(client, Some(admitted(client)))
            .expect("a place, a keeper and a row");
        kept.push(registration);
    }
    assert_eq!(keeper.custodies_kept(), 2);
    let pins: Vec<_> = kept
        .iter()
        .map(|registration| {
            let PrivateCustodyReach::Reached(pin) =
                registration.registered_custody(&keeper.lease()).expect("its own custody")
            else {
                panic!("its owner keeps it")
            };
            pin
        })
        .collect();
    assert!(
        !Arc::ptr_eq(pins[0].join(), pins[1].join()),
        "two connections, two homes"
    );
    assert!(!pins[0].identity().same_as(pins[1].identity()));
    assert_ne!(pins[0].identity().place(), pins[1].identity().place());

    // AND THE OWNER RESOLVES EACH NAME TO ITS OWN HOME, never to the other's.
    for (registration, pin) in kept.iter().zip(pins.iter()) {
        let named = registration.maintenance_identity().expect("a name");
        let found = keeper.custody_named(&named).expect("its own entry");
        assert!(Arc::ptr_eq(found.join(), pin.join()));
    }
    // A NAME FROM ANOTHER STORE ENTIRELY RESOLVES TO NOTHING HERE.
    let elsewhere = PrivateSettlementOwner::default();
    let stranger_keeper = service_owner(&elsewhere, 2);
    let stranger = private_over(&stranger_keeper, 2);
    let outsider = XServerFrontendClientId(8599);
    let (foreign, _foreign_channels) = stranger
        .broker
        .registry
        .register_client_with_admission(outsider, Some(admitted(outsider)))
        .expect("a place, a keeper and a row");
    assert!(
        keeper
            .custody_named(&foreign.maintenance_identity().expect("a name"))
            .is_none(),
        "one owner's inventory does not answer for another store's connection"
    );
    drop(pins);
    drop((kept, private, keeper));
    drop((foreign, stranger, stranger_keeper, elsewhere));
}

#[test]
fn a_connection_whose_evidence_cannot_be_kept_is_not_exposed() {
    // THE BOUND IS THE STORE'S, AND OUTSTANDING ENTRIES CONSUME IT. A place
    // that goes back can be taken again; the evidence of what happened in it
    // cannot be written over, so a service whose inventory is full refuses the
    // next connection BEFORE its row goes in rather than admitting it and
    // discarding somebody's result.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 2);
    let private = private_over(&keeper, 2);
    let mut live = Vec::new();
    for raw in [8505, 8506] {
        live.push(bound_on(&private, XServerFrontendClientId(raw)));
    }
    assert_eq!(keeper.custodies_kept(), 2);
    assert_eq!(keeper.custody_capacity_remaining(), 0);

    // BOTH PLACES GO BACK. The store has room again; the keeper does not.
    let places: Vec<usize> = live
        .iter()
        .map(|registration| {
            registration
                .maintenance_identity()
                .expect("a name")
                .place()
        })
        .collect();
    drop(live);
    for place in places {
        settle_and_return(&durable, place);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "the store has its places back"
    );
    assert_eq!(
        keeper.custodies_kept(),
        2,
        "and the evidence of those connections is still kept"
    );

    // SO THE NEXT CONNECTION IS REFUSED, AND NOTHING IS EXPOSED.
    let later = XServerFrontendClientId(8507);
    let refused = private
        .broker
        .registry
        .register_client_with_admission(later, Some(admitted(later)));
    assert!(
        matches!(
            refused,
            Err(XServerFrontendRouteError::EvidenceCustodyUnavailable { client })
                if client == later
        ),
        "refused for the keeper, and told so: {refused:?}",
        refused = refused.as_ref().err()
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "the place the refused attempt took went back with it"
    );
    assert_eq!(keeper.custodies_kept(), 2, "and nothing was retired to fit");
    assert_eq!(keeper.custody_capacity_remaining(), 0);
    drop((private, keeper));
}

#[test]
fn a_successor_at_one_number_does_not_take_its_predecessors_evidence() {
    // A PLACE IS A NUMBER AND EVIDENCE IS NOT. The first connection's place
    // goes back and the next connection takes the same index; its custody is
    // its own, and the entry the first one left is untouched.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 3);
    let private = private_over(&keeper, 3);
    let first = bound_on(&private, XServerFrontendClientId(8508));
    let PrivateCustodyReach::Reached(first_pin) =
        first.registered_custody(&keeper.lease()).expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let first_home = Arc::clone(first_pin.join());
    let first_sink = Arc::clone(first_pin.exit_sink());
    let first_slot = std::ptr::from_ref(first_pin.worker_slot()) as usize;
    let place = first.maintenance_identity().expect("a name").place();
    drop((first_pin, first));
    settle_and_return(&durable, place);

    let next_client = XServerFrontendClientId(8509);
    let (next, _next_channels) = private
        .broker
        .registry
        .register_client_with_admission(next_client, Some(admitted(next_client)))
        .expect("a place, a keeper and a row");
    assert_eq!(
        next.maintenance_identity().expect("a name").place(),
        place,
        "the successor really did take that number"
    );
    let PrivateCustodyReach::Reached(next_pin) =
        next.registered_custody(&keeper.lease()).expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert!(
        !Arc::ptr_eq(next_pin.join(), &first_home),
        "a reused number is not a reused home"
    );
    assert!(
        !Arc::ptr_eq(next_pin.exit_sink(), &first_sink),
        "nor a reused exit record"
    );
    assert_ne!(
        std::ptr::from_ref(next_pin.worker_slot()) as usize,
        first_slot,
        "nor a reused worker slot: the successor's handle has nowhere to go \
         that its predecessor's evidence is in"
    );
    assert_eq!(
        keeper.custodies_kept(),
        2,
        "the predecessor's evidence is still kept beside the successor's"
    );
    assert_eq!(keeper.custody_capacity_remaining(), 1);
    assert!(
        Arc::strong_count(&first_home) >= 1 && first_home.result().is_none(),
        "its home is still here, and still says nothing happened in it"
    );
    assert!(
        !first_sink.left(),
        "and its exit record still says nothing left"
    );
    drop(next_pin);
    drop((next, private, keeper));
}
