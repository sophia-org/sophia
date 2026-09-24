// Continuations and the places they hold: the retained record that is not
// absent, and the permit whose wait leaves the producer's level alone.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn an_unreadable_retained_record_is_not_reported_as_absent() {
    // Counting a poisoned record as empty publishes a zero for an obligation
    // that is still owned and still unanswered, which is the one answer a
    // caller must not be given.
    let client = XServerFrontendClientId(8001);
    let f = prepared_ordered_fixture(client);
    let sender = f
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();
    let (emission, _endpoint) =
        private_native_tests::emission_and_endpoint_for_writer_fixture(80010);
    gated_send(
        &sender,
        XAuthorityOrderedDelivery::from_emission(emission).unwrap(),
    )
    .expect("accepted into its queue");

    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let PreparedOrderedFixture { channels, .. } = f;
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
    assert_eq!(durable.continuations_retained(), Some(1));

    // A real panic while a record is borrowed poisons only that record.
    let record = {
        let held = durable.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(record) = &held.continuations[0] else {
            panic!("its place holds the record")
        };
        record.clone()
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = record.state.lock().unwrap();
            panic!("intentional record poison for reporting fixture");
        }))
        .is_err()
    );
    assert!(record.unreadable());
    assert!(
        durable.inner.try_lock().is_ok(),
        "the store itself is still readable"
    );

    assert_eq!(
        durable.continuations_retained(),
        None,
        "an unreadable record is not an absent one"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "and its place is still taken"
    );
}

#[test]
fn a_quiet_continuation_keeps_its_place_and_does_not_starve_the_others() {
    // A place comes back when its work is gone, not when its queue falls
    // quiet: producers may still hold senders for it. And one record that
    // cannot progress must not take every visit, or the rest are never driven.
    let first = XServerFrontendClientId(8011);
    let second = XServerFrontendClientId(8012);
    let one = prepared_ordered_fixture(first);
    let two = prepared_ordered_fixture(second);
    // A sender for the first is kept alive, so its queue is quiet rather than
    // finished. The second's producers go.
    let live_sender = one
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&first)
        .expect("its row")
        .ordered
        .clone();

    let durable = PrivateSettlementOwner::with_capacities(4, 4);
    let mut places = Vec::new();
    for fixture in [one, two] {
        let slot = durable
            .reserve_ordered_continuation()
            .expect("a place, reserved before exposure");
        let PreparedOrderedFixture {
            channels,
            registration,
            ..
        } = fixture;
        // Bound, so a visit can actually end this wire. A receiver alone has
        // nothing to end with and could never reach a settled record, which
        // would make this control about the wrong thing.
        let mut source = Some(transport_continuation(&registration, channels.ordered));
        // And then gone, along with the row it published: a registration that
        // stayed would be a producer still holding a sender, which is the one
        // thing that keeps a queue from finishing.
        drop(registration);
        assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
        places.push(());
    }
    assert_eq!(durable.continuations_reserved(), Some(2));
    assert_eq!(durable.continuations_retained(), Some(2));

    // EVERY RECORD GETS A VISIT. The first cannot finish -- a sender for it is
    // still held -- and that must not stop the second being reached.
    let driven = durable.drive_ordered_continuations(4);
    assert_eq!(driven, 4, "each visit reached a record");
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the one whose producers are gone gave its place back"
    );
    assert_eq!(durable.continuations_retained(), Some(1));

    // The quiet one keeps its place for as long as anything can still send.
    for _ in 0..4 {
        durable.drive_ordered_continuations(2);
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "a quiet queue is not a finished one while a sender is held"
    );
    drop(live_sender);
    durable.drive_ordered_continuations(2);
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "and it comes back once nothing can send to it"
    );
    assert_eq!(durable.continuations_retained(), Some(0));
    let _ = places;
}

#[test]
fn asking_whether_a_continuation_is_settled_destroys_nothing() {
    // Receiving is the only way to question a channel, so a predicate that
    // questioned one consumed whatever was waiting and reported on work it had
    // just destroyed. The admission is real and its finalizer is watched by a
    // Weak, so a capsule thrown away by the question would be visible as gone.
    let client = XServerFrontendClientId(8021);
    let f = prepared_ordered_fixture(client);
    let sender = f
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();
    let delivery = XAuthorityInputDeliveryId::from_raw(80210);
    // The ledger that actually admitted this delivery is the one that can
    // answer for it.
    let (recovery, _receipts) = claim_fixture(delivery);
    let (emission, _endpoint) =
        private_native_tests::emission_and_endpoint_for_writer_fixture(80210);
    let mut capsule = XAuthorityOrderedDelivery::from_emission(emission).unwrap();
    let completion = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("its admission minted a cell");
    capsule.carry_finalizer(Arc::new(finalizer_from_held(
        &recovery,
        &completion,
        delivery,
        client,
    )));
    let finalizer = Arc::downgrade(capsule.finalizer().expect("carried"));
    gated_send(&sender, capsule).expect("accepted into its queue");

    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let PreparedOrderedFixture {
        channels,
        registration,
        ..
    } = f;
    let mut source = Some(transport_continuation(&registration, channels.ordered));
    drop(registration);
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    // ASKED REPEATEDLY, WITHOUT DRIVING. Nothing may be consumed by the
    // question, and the place may not come back while that admission exists.
    for _ in 0..8 {
        let settled = durable
            .with_ordered_continuation(0, |continuation| continuation.settled())
            .expect("the place holds it");
        assert!(!settled, "an accepted admission is not a settled connection");
    }
    assert!(
        finalizer.upgrade().is_some(),
        "the question did not destroy the capsule it was asked about"
    );
    assert_eq!(durable.continuations_reserved(), Some(1));
    assert!(completion.answer().is_none(), "and answered nobody");

    // A visit takes it into custody, where it is still owed an answer, so the
    // place still does not come back.
    durable.drive_ordered_continuations(4);
    assert!(
        finalizer.upgrade().is_some(),
        "a visit receives into custody rather than discarding"
    );
    let retained = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Setup { retained, .. } = continuation else {
                panic!("a setup case was installed")
            };
            retained
                .first()
                .map(XAuthorityOrderedDelivery::delivery)
        })
        .expect("the place holds it");
    assert_eq!(
        retained,
        Some(delivery),
        "the exact admission is held, not thrown away"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "and its place stays taken while it is owed an answer"
    );
    assert!(completion.answer().is_none());
}

#[test]
fn a_stale_return_cannot_take_the_place_its_successor_holds() {
    // A place returned and reserved again between a visit finding a record and
    // that visit finishing with it. The stale return must not free the place
    // its successor now holds.
    //
    // STAGED AT THE API, not observed as a concurrent race: the interleave is
    // constructed by holding the old record's handle across the return and the
    // new reservation.
    let first = XServerFrontendClientId(8031);
    let second = XServerFrontendClientId(8032);
    let one = prepared_ordered_fixture(first);
    let two = prepared_ordered_fixture(second);
    let durable = PrivateSettlementOwner::with_capacities(2, 2);

    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let PreparedOrderedFixture {
        channels,
        runner: one_runner,
        registration: one_registration,
        durable: _one_durable,
        ..
    } = one;
    // Bound, so a visit can establish an ending and this place can come back
    // at all. A receiver alone has nothing to end with.
    let mut source = Some(transport_continuation(&one_registration, channels.ordered));
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    // A visit's view of that record, captured before the place moves on.
    let stale = {
        let held = durable.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(record) = &held.continuations[0] else {
            panic!("its place holds the record")
        };
        record.clone()
    };

    // Its producers go, so a drive returns the place.
    drop(one_registration);
    drop(one_runner);
    let PreparedOrderedFixture {
        channels: second_channels,
        runner: two_runner,
        registration: two_registration,
        durable: _two_durable,
        ..
    } = two;
    durable.drive_ordered_continuations(8);
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "the first connection's place came back"
    );

    // Another connection takes that same place.
    let replacement = durable
        .reserve_ordered_continuation()
        .expect("the returned place is reusable");
    let mut successor = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(second_channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(replacement, &mut successor), PrivateContinuationCommit::Retained);
    assert_eq!(durable.continuations_reserved(), Some(1));

    // THE STALE RETURN ARRIVES. It names a record that is no longer there.
    durable.return_ordered_continuation(0, &stale);
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "a return for a record that moved on does not free its successor's place"
    );
    let still_there = durable
        .with_ordered_continuation(0, |continuation| {
            matches!(continuation, PrivateOrderedContinuation::Setup { .. })
        })
        .expect("the successor is still installed");
    assert!(still_there);
    drop(two_registration);
    drop(two_runner);
}

/// A private instance over a store the caller keeps, with a declared limit.
///
/// Built through the real constructor. Nothing here installs a continuation
/// owner or declares a bound: a fixture that did either would be certifying
/// its own wiring rather than production's.
fn private_over(
    keeper: &crate::PrivateServiceOwner,
    clients: usize,
) -> crate::PrivateXServerFrontend {
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(clients).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"))
}

#[test]
fn a_private_instance_takes_its_connection_bound_from_its_declared_client_limit() {
    // THROUGH REAL CONSTRUCTION. The store arrives with no bound declared, and
    // an instance that left it that way would admit connections reserving
    // nothing -- the public frontend's behaviour, silently applied to a
    // private one whose accepted work must have somewhere to go.
    let durable = PrivateSettlementOwner::default();
    assert_eq!(
        durable.continuations_reserved(),
        Some(0),
        "nothing is reserved before an instance exists"
    );
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);

    let first = XServerFrontendClientId(8051);
    let one = private
        .broker
        .registry
        .register_client_with_admission(first, Some(admitted(first)))
        .expect("the first connection has a place");
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "production installed the owner, so registration reserves"
    );
    let second = XServerFrontendClientId(8052);
    let two = private
        .broker
        .registry
        .register_client_with_admission(second, Some(admitted(second)))
        .expect("the declared limit is two");
    assert_eq!(durable.continuations_reserved(), Some(2));

    // The bound is the declared limit, not the abandoned-work capacity the
    // store was built with, and not unbounded.
    let third = XServerFrontendClientId(8053);
    let refused = private
        .broker
        .registry
        .register_client_with_admission(third, Some(admitted(third)))
        .err()
        .expect("the declared limit is two");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ContinuationUnavailable { client } if client == third
        ),
        "refused for the place, got {refused:?}"
    );
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .get(&third)
            .is_none(),
        "the refused connection has no row, so nothing can be accepted for it"
    );
    drop(one);
    drop(two);
}

#[test]
fn a_registry_does_not_keep_the_store_it_takes_places_from_alive() {
    // THE RING IS BUILT, not assumed: an instance that ends owing a hold hands
    // its terminal inventory to the store, and that inventory keeps the
    // registry it must act through. A registry that owned the store back would
    // close it -- store, retained inventory, registry, store -- and nothing in
    // it would ever drop. What that looks like from outside is an instance
    // whose obligations stay readable forever, so it reads as still settling
    // rather than as a leak.
    let durable = PrivateSettlementOwner::default();
    let watch = Arc::downgrade(&durable.inner);
    let owner_of_durable = service_owner(&durable, 16);
    let client = XServerFrontendClientId(8061);
    let common = instance_handing_over_a_retained_hold(
        &owner_of_durable,
        client,
        SurfaceId::new(0x8061, 1),
        NamespaceId::from_raw(client.raw()),
        80610,
        272,
    );
    assert_eq!(
        durable.terminal_inventories().expect("readable"),
        1,
        "the store retains an inventory, and that inventory holds a registry"
    );
    assert!(watch.upgrade().is_some(), "the store is alive and in use");
    let answered_to = Arc::downgrade(&common);

    // Everything that legitimately owns the store goes here -- the service
    // owner included, which keeps the store and the evidence it is about. The
    // registry inside the retained inventory does not own it, so it holds
    // nothing back.
    drop(common);
    drop(durable);
    drop(owner_of_durable);
    assert!(
        watch.upgrade().is_none(),
        "a registry that owned its store back would keep the store, the \
         inventory and every obligation in it reachable forever"
    );
    assert!(
        answered_to.upgrade().is_none(),
        "and the authority the retained hold answered to goes with it"
    );
}

#[test]
fn a_registry_whose_store_is_gone_refuses_rather_than_exposing_a_connection() {
    // A STORE THAT HAS GONE IS NOT AN EMPTY STORE. The registry holds it
    // weakly, so it can outlive it; carrying on without one would expose a
    // connection whose accepted work has nowhere to go, which is the thing
    // reserving was for.
    let registry = {
        let durable = PrivateSettlementOwner::default();
        let service_keeper = service_owner(&durable, 4);
        let private = private_over(&service_keeper, 4);
        private.broker.registry.clone()
    };
    let client = XServerFrontendClientId(8101);
    let refused = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("there is no store to take a place from");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ContinuationUnavailable { client: gone } if gone == client
        ),
        "refused for the place, got {refused:?}"
    );
    assert!(
        registry
            .clients
            .lock()
            .expect("a readable registry")
            .get(&client)
            .is_none(),
        "and published nothing"
    );
}

#[test]
fn a_later_instance_does_not_move_the_bound_its_places_were_taken_against() {
    // The store outlives the instance that declared its bound. A second
    // instance arriving with a different client limit finds places already
    // held against the first, and moving the number under them would hand out
    // places a departed instance already accounted for.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 1);
    let first_instance = private_over(&service_keeper, 1);
    let client = XServerFrontendClientId(8071);
    let registration = first_instance
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the one place");
    // Retained: the connection's place stays held after its instance goes.
    drop(registration);
    drop(first_instance);
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the place is retained across the instance that took it"
    );

    let service_keeper = service_owner(&durable, 8);
    let second_instance = private_over(&service_keeper, 8);
    let later = XServerFrontendClientId(8072);
    let refused = second_instance
        .broker
        .registry
        .register_client_with_admission(later, Some(admitted(later)))
        .err()
        .expect("the bound in force is the one the retained place was taken against");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ContinuationUnavailable { client } if client == later
        ),
        "refused for the place, got {refused:?}"
    );
}

#[test]
fn a_refusal_before_publication_gives_its_place_back() {
    // A RESERVATION THAT PUBLISHED NOTHING IS NOT RETAINED WORK. No row and no
    // reachable queue means no capsule could have been accepted for it, so
    // holding the place would spend the bound on a connection that never
    // existed -- and the store cannot tell later, because there is nothing to
    // ask.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8081);
    let registration = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place");
    assert_eq!(durable.continuations_reserved(), Some(1));

    // Refused after the place was taken and before anything was published.
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("the client is already registered");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::DuplicateClient { client: duplicate } if duplicate == client
        ),
        "refused for the duplicate, got {refused:?}"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the refusal gave its place back rather than retaining one for a \
         connection that was never exposed"
    );
    assert_eq!(
        durable.continuations_abandoned(),
        Some(0),
        "and did not abandon one either: there is nothing to account for"
    );

    // Proved against the bound, not just the counter: the second place is
    // still there to be taken.
    let other = XServerFrontendClientId(8082);
    let second = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("the place the refused duplicate gave back");
    drop(registration);
    drop(second);
}

#[test]
fn a_registration_never_takes_the_settlement_store_beneath_the_client_table() {
    // THE DRIVE ALREADY HOLDS SETTLEMENT AND THEN TAKES CLIENTS, to release a
    // route lease. A registration that reserved under the client table would
    // be the other order, and two orders is a deadlock.
    //
    // WHAT THIS ESTABLISHES, exactly: the store is held for a window in which
    // the registration provably cannot finish -- it reserves nothing and
    // returns nothing until the store is released -- and throughout that
    // window the client table is free. It does NOT establish that the worker
    // reached its reservation: sleeping for a while is not a rendezvous, and
    // an observation that found the table free because the thread had not yet
    // started would look the same from here. The inversion mutant is what
    // gives the assertion its teeth, and a labelled hook at the reservation
    // boundary is what would establish arrival.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 4);
    let private = private_over(&service_keeper, 4);
    let clients = private.broker.registry.clients.clone();
    let registry = private.broker.registry.clone();
    let client = XServerFrontendClientId(8091);
    let held = durable.records_even_if_poisoned();
    let joiner = std::thread::spawn(move || {
        registry
            .register_client_with_admission(client, Some(admitted(client)))
            .map(|(registration, _channels)| registration)
    });
    // The store is held for the whole window, so the registration cannot get
    // past its reservation during it -- whether or not it has reached it.
    let mut free = 0;
    for _ in 0..40 {
        assert!(
            clients.try_lock().is_ok(),
            "a registration that has not completed must not be holding the \
             client table while the store is held: that is the order the \
             retained drive takes them in, reversed"
        );
        free += 1;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        free, 40,
        "the client table was free at every observation taken during the \
         window in which the registration could not complete"
    );
    assert_eq!(
        durable_reserved(&held), 0,
        "and the registration had not completed: no place was taken while the \
         store was held"
    );
    drop(held);
    let registration = joiner
        .join()
        .expect("the registration thread")
        .expect("a place, once the store is free");
    assert_eq!(durable.continuations_reserved(), Some(1));
    drop(registration);
}

/// Reserved places, read from a guard the caller already holds.
fn durable_reserved(held: &AbandonedSettlements) -> usize {
    held.continuation_slots
}

/// Stops this connection's worker however the control leaves.
///
/// A body waiting on its notice outlives an assertion that failed above it,
/// and a scope joins what it spawned -- so without this a control that failed
/// would hang instead of reporting. It stops on the way out, whichever way
/// out that is.
struct PrivateWorkerStopper<'a>(&'a Arc<AtomicBool>, &'a Arc<PrivateOrderedWake>);

impl Drop for PrivateWorkerStopper<'_> {
    fn drop(&mut self) {
        // THROUGH THE PRODUCTION CANCELLATION. A bare store and a signal
        // beside the predicate mutex can land after a waiter has checked stop
        // and before it waits, and then the signal reaches nobody: the cleanup
        // hangs on a body that will never look again. The existing helper
        // publishes the recheck under that mutex, which is what makes the
        // stop visible to a waiter either side of its check.
        cancel_connection_worker(self.0, self.1);
    }
}

/// Wait until this connection's body has been through its idle wait.
///
/// A HANDSHAKE, NOT A SLEEP, and it establishes exactly one thing: that the
/// body reached the wait and consumed the level, because only a waiter
/// consumes it and only under that wait's own mutex. A body that had not got
/// there would leave it set.
///
/// WHAT IT DOES NOT ESTABLISH is where the body is when this returns.
/// Consuming the level is how the wait ENDS: the body goes on to another
/// ordinary visit and, if that is idle again, to another wait. So this says a
/// wait happened, never that one is happening now -- and a control that acts
/// on the second reading is racing a legal schedule.
///
/// Bounded, because a control that never finishes reports nothing.
fn waited_and_consumed(wake: &Arc<PrivateOrderedWake>) -> bool {
    wake.publish_recheck();
    waited_for(|| {
        !wake
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
    })
}

/// Hand a capsule over the way a production producer does: the recheck level
/// is armed first and published as the notice drops, whatever the send did.
///
/// `admit` alone publishes nothing -- it is the gate, not the notice -- so a
/// control that only sent would leave a waiting body with nothing to wake it.
fn produced_send(sender: &PrivateGatedOrderedSender, capsule: XAuthorityOrderedDelivery) {
    let notify = sender.arm_wake();
    gated_send(sender, capsule).expect("an open endpoint");
    drop(notify);
}

/// A connection served by a real owner, with an actual stop it can be told to
/// use.
///
/// THE PRODUCTION BINDING PASSES NO STOP. A worker over an owner that has none
/// cannot be told to stop, so these controls bind with one -- through the same
/// `bind` production uses -- and the body refuses an owner without one rather
/// than running a thread nothing can end. Wiring that binding is the
/// attachment's work and is not done here.
struct PrivateWorkerFixture {
    fixture: PreparedOrderedFixture,
    home: Arc<PrivateOrderedHome>,
    wake: Arc<PrivateOrderedWake>,
    stop: Arc<AtomicBool>,
    sequence: Arc<AtomicU16>,
    sender: PrivateGatedOrderedSender,
    peer: UnixStream,
    _output: Arc<Mutex<X11ClientOutput>>,
}

fn worker_fixture(client: XServerFrontendClientId) -> PrivateWorkerFixture {
    worker_fixture_bound(client, true)
}

/// The same, with the choice production makes today available: a transport
/// bound with no stop at all.
fn worker_fixture_bound(
    client: XServerFrontendClientId,
    stoppable: bool,
) -> PrivateWorkerFixture {
    // THE PREPARED CONNECTION, whole: admitted through its lifecycle, its
    // applied state attached, its durable store held by the fixture. Promotion
    // asks the boundary for this registration's endpoint, so a connection
    // assembled by hand would be refused as unadmitted -- and rightly.
    let mut f = prepared_ordered_fixture(client);
    let ordered = std::mem::replace(
        &mut f.channels.ordered,
        f.runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .register_client_with_admission(
                XServerFrontendClientId(client.raw() + 500_000),
                Some(admitted(XServerFrontendClientId(client.raw() + 500_000))),
            )
            .expect("a spare registration to borrow a receiver from")
            .1
            .ordered,
    );
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(3)))
        .expect("a bounded read");
    let output = X11ClientOutput::shared(socket, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::clone(&ordered.wake);
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        stoppable.then_some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    assert!(matches!(
        f.registration
            .retain_ordered_setup(PrivateOrderedContinuation::Setup {
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
    assert_eq!(
        f.registration
            .promote_ordered_serving(f.runner.frontend.as_ref().unwrap()),
        PrivateOrderedPromotion::Ready
    );
    let sender = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), client);
    let home = Arc::clone(&f.registration.ordered_home);
    PrivateWorkerFixture {
        fixture: f,
        home,
        wake,
        stop,
        sequence: Arc::new(AtomicU16::new(7)),
        sender,
        peer,
        _output: output,
    }
}

impl PrivateWorkerFixture {
    /// This connection's startup permit, published the way startup publishes
    /// it: under the notice, and woken.
    fn permit(&self) {
        self.wake
            .state
            .lock()
            .expect("a readable notice")
            .started = true;
        self.wake.ready.notify_all();
    }

    /// This connection's handles, cloned so a body can be built inside a
    /// thread without borrowing the fixture the test still needs.
    fn handles(
        &self,
    ) -> (
        Arc<PrivateOrderedHome>,
        Arc<PrivateOrderedWake>,
        Arc<AtomicBool>,
        Arc<AtomicU16>,
    ) {
        (
            Arc::clone(&self.home),
            Arc::clone(&self.wake),
            Arc::clone(&self.stop),
            Arc::clone(&self.sequence),
        )
    }

    fn body<'a>(&'a self, exit: &'a PrivateWorkerExit, steps: usize) -> PrivateWorkerBody<'a> {
        PrivateWorkerBody {
            home: &self.home,
            wake: &self.wake,
            stop: &self.stop,
            byte_order: XByteOrder::LittleEndian,
            sequence: &self.sequence,
            exit,
            steps,
        }
    }
}

#[test]
fn a_body_serves_an_admitted_delivery_to_real_bytes_and_answers_for_it() {
    // THE WHOLE POINT OF A WORKER, end to end through the real owner: an
    // admitted delivery -- reserved, executed, observed and dispatched by the
    // production path -- becomes bytes this connection's peer can read, and
    // its completion says so. The body encodes nothing, writes nothing and
    // answers nothing itself; it asks the owner for one step at a time.
    let mut f = worker_fixture(XServerFrontendClientId(8391));
    attempt_run(&mut f.fixture, 83910, 272, true);
    let private = f.fixture.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 83910);
    assert_eq!(
        private.dispatch_one_press(),
        Some(true),
        "the real producer handed it to this connection's queue"
    );
    assert!(cell.answer().is_none(), "and nothing has served it yet");

    f.permit();
    let exit = PrivateWorkerExit::unstarted();
    let (home, wake, stop, sequence) = f.handles();
    let outcome = std::thread::scope(|scope| {
        let exit = &exit;
        let worker = scope.spawn(move || {
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit,
                steps: 16,
            }
            .run()
        });
        let _stopper = PrivateWorkerStopper(&f.stop, &f.wake);
        // ITS BYTES ARE ON THE WIRE. A whole X event, read by the peer this
        // connection was bound to.
        let mut seen = [0u8; 32];
        std::io::Read::read_exact(&mut (&f.peer), &mut seen)
            .expect("its peer reads what was sent");
        assert_eq!(seen[0], 4, "a button press, which is what was run");
        // THE EXECUTION CONTEXT IS THIS CONNECTION'S, and the bytes say so.
        // The sequence field carries the counter this body was given, in the
        // byte order it was given -- not a constant, and not the other order,
        // either of which would put different bytes here.
        assert_eq!(
            &seen[2..4],
            &[7, 0],
            "its own sequence, little-endian as supplied"
        );

        // AND ITS RECEIPT IS PUBLISHED, observed before anything is cancelled.
        // The step that finishes the write can return Advanced; finalising it
        // is the step after, and a body stopped in between would leave a
        // control asking for a receipt nobody had written.
        assert!(published(&cell), "the delivery is answered for");

        // AND THEN IT IS STOPPED AND JOINED, before anything is dropped: a
        // registration going while a borrower is still in its home is the
        // integration boundary, not this body's to cross.
        cancel_connection_worker(&f.stop, &f.wake);
        worker.join().expect("the body finished")
    });
    let answer = cell.answer().expect("and the delivery is answered for");
    assert_eq!(answer.delivery, XAuthorityInputDeliveryId::from_raw(83910));
    assert_eq!(answer.outcome, XAuthorityInputDeliveryOutcome::Flushed);
    assert!(
        stopped_by_cancellation(&outcome),
        "the owner's own word, whichever of the two saw the stop: {outcome:?}"
    );
    assert_eq!(exit.outcome(), Some(outcome));
    assert!(exit.left(), "and its frame is gone");
    drop(f.fixture);
}

#[test]
fn a_body_will_not_serve_work_queued_before_its_permit() {
    // WORK IS NOT PERMISSION. A queue with something on it says a producer got
    // there first, not that the transaction which owns this worker's handle
    // has finished; serving on the strength of it would be a worker running
    // before anything permitted it.
    //
    // AND IT IS NOT A REASON TO GIVE UP EITHER. The pending level is left
    // exactly as it was found, so whatever put it there still gets its wake.
    let f = worker_fixture(XServerFrontendClientId(8392));
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(83920);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    produced_send(&f.sender, capsule);
    assert!(
        f.wake.state.lock().expect("a readable notice").pending,
        "the producer published a level"
    );

    // Stopped rather than permitted, so the body leaves startup without ever
    // serving. The trigger is not a race here and is asserted strictly: the
    // stop is set before the body runs at all, and the permit wait asks about
    // it first.
    //
    // SET DIRECTLY, NOT THROUGH THE CANCELLATION, and for this control only.
    // Nothing is running that could miss a signal, and the cancellation
    // publishes a recheck of its own -- which is the very level this control
    // goes on to assert the body left alone.
    f.stop.store(true, Ordering::SeqCst);
    let exit = PrivateWorkerExit::unstarted();
    let outcome = f.body(&exit, 8).run();

    assert_eq!(outcome.trigger, PrivateWorkerTrigger::Stopped);
    assert!(cell.answer().is_none(), "nothing was served");
    assert!(
        f.wake.state.lock().expect("a readable notice").pending,
        "and the level it never waited on is still there"
    );
    // THE CAPSULE IS STILL ON ITS OWN QUEUE, held by nobody else.
    assert!(
        f.home
            .borrow_live(|payload| {
                let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                    panic!("promoted")
                };
                owner.queue.try_recv().is_ok()
            })
            .acted()
            .expect("its own home"),
        "the work it refused to serve is where the producer put it"
    );
    drop(f.fixture);
}

#[test]
fn an_unpermitted_body_serves_nothing_until_it_is_permitted() {
    // THE PERMIT IS ASKED FOR, and waited on. A body that went straight to
    // serving would be writing this connection's events before the
    // transaction that owns its handle had finished -- and there is work
    // waiting here, so nothing but the permit is holding it back.
    let mut f = worker_fixture(XServerFrontendClientId(8402));
    attempt_run(&mut f.fixture, 84020, 272, true);
    let cell = admitted_cell(f.fixture.runner.frontend.as_ref().unwrap(), 84020);
    assert_eq!(
        f.fixture
            .runner
            .frontend
            .as_mut()
            .unwrap()
            .dispatch_one_press(),
        Some(true)
    );
    assert!(
        f.wake.state.lock().expect("a readable notice").pending,
        "work is queued and announced"
    );
    assert!(
        !f.wake.state.lock().expect("a readable notice").started,
        "and nothing has permitted this worker"
    );

    let exit = PrivateWorkerExit::unstarted();
    let (home, wake, stop, sequence) = f.handles();
    std::thread::scope(|scope| {
        let exit = &exit;
        let worker = scope.spawn(move || {
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit,
                steps: 16,
            }
            .run()
        });
        let _stopper = PrivateWorkerStopper(&f.stop, &f.wake);
        // A WINDOW, AND ONLY A WINDOW. There is work in front of this body and
        // a budget to serve it with, so one that never asked for a permit had
        // every opportunity here and took none. It does NOT establish that the
        // body has reached its startup wait -- that wait consumes no level a
        // handshake could use, and a sleep cannot say where a thread is.
        std::thread::sleep(Duration::from_millis(100));
        assert!(cell.answer().is_none(), "it served nothing");
        assert!(!exit.left(), "and is still waiting to be allowed to");

        // AND NOW IT MAY.
        f.permit();
        let mut seen = [0u8; 32];
        std::io::Read::read_exact(&mut (&f.peer), &mut seen)
            .expect("its peer reads what the permitted body sent");
        assert_eq!(seen[0], 4);
        assert!(
            published(&cell),
            "and answered for it, which is a step after the write"
        );
        cancel_connection_worker(&f.stop, &f.wake);
        let outcome = worker.join().expect("the body finished");
        assert!(
            stopped_by_cancellation(&outcome),
            "the owner's own word, whichever of the two saw the stop: {outcome:?}"
        );
    });
    assert!(cell.answer().is_some());
    drop(f.fixture);
}

#[test]
fn waiting_for_a_permit_leaves_the_producers_level_where_it_was() {
    // A PERMIT IS NOT AN IDLE WAIT, and must not consume what an idle wait
    // consumes. The level belongs to whoever published it: a startup that
    // swallowed it would leave a producer having announced work to a worker
    // that then went to sleep without looking.
    //
    // NO STEPS ARE TAKEN HERE, so what is observed is the startup wait alone.
    let f = worker_fixture(XServerFrontendClientId(8403));
    f.permit();
    // Published the way a producer publishes it, with nothing sent: the level
    // is the subject, not the capsule.
    drop(f.sender.arm_wake());
    assert!(f.wake.state.lock().expect("a readable notice").pending);

    let exit = PrivateWorkerExit::unstarted();
    let outcome = f.body(&exit, 0).run();
    assert_eq!(outcome.trigger, PrivateWorkerTrigger::Exhausted);
    assert!(
        f.wake.state.lock().expect("a readable notice").pending,
        "the level a producer published is still there for whoever waits next"
    );
    drop(f.fixture);
}

#[test]
fn a_full_store_of_departed_places_reclaims_them_rather_than_refusing() {
    // t138. Every place is taken by a connection that has gone, and the
    // reservation that finds none is not learning that the instance is busy --
    // it is learning that nobody has looked. Measured on a live private
    // instance as a fifth admission that completed setup and was reset, with
    // the invocation ending on "no retained place is available".
    //
    // No drive is called here. That is the whole point: before this, a caller
    // had to know to drive, and on the live path nothing did.
    let durable = PrivateSettlementOwner::with_capacities(4, 4);
    for raw in 0..4 {
        let client = XServerFrontendClientId(8110 + raw);
        let slot = durable
            .reserve_ordered_continuation()
            .expect("a place, while the store still has free ones");
        let PreparedOrderedFixture {
            channels,
            registration,
            ..
        } = prepared_ordered_fixture(client);
        let mut source = Some(transport_continuation(&registration, channels.ordered));
        // The connection departs: its row goes, and with it every sender that
        // could still reach the queue it left.
        drop(registration);
        assert_eq!(
            retain_into(slot, &mut source),
            PrivateContinuationCommit::Retained
        );
    }
    assert_eq!(
        durable.continuations_reserved(),
        Some(4),
        "four departures, four places still taken"
    );

    // The fifth admission. It must not be told the instance is full while
    // every place in it belongs to somebody who has left.
    let fifth = durable
        .reserve_ordered_continuation()
        .expect("a place reclaimed from a departed connection, not a refusal");
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the reclaimed places went back and only the new one is taken"
    );
    assert_eq!(
        durable.continuations_retained(),
        Some(0),
        "and none of them is still retained"
    );
    drop(fifth);
}

#[test]
fn a_store_full_of_live_places_still_refuses() {
    // The other half of the rule above, and the reason it is safe. Reclaiming
    // before refusing must not reclaim a place whose work is still there: a
    // full store of LIVE connections is genuinely full, and saying otherwise
    // would hand one connection's destination to another.
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let mut held = Vec::new();
    for raw in 0..2 {
        let _ = raw;
        held.push(
            durable
                .reserve_ordered_continuation()
                .expect("a place, while the store still has free ones"),
        );
    }

    assert!(
        matches!(
            durable.reserve_ordered_continuation(),
            Err(AdmissionRefusal::Saturated)
        ),
        "a place reserved and not yet disposed of is nobody else's to take"
    );
    assert_eq!(durable.continuations_reserved(), Some(2));
    drop(held);
}
