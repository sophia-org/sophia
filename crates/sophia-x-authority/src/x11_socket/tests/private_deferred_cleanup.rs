// Controls for the deferred connection cleanup: the visit that discharges a
// connection's deferred destruction after its frames are collected and its
// registered worker joined, and every refusal that keeps the duty owned.
//
// Two kinds of control. The service-level ones (in the attachment files and
// below) run the real private service. The ones here that call
// `visit_deferred_cleanup` directly run the same code the service calls, over
// a worker fixture's real registration, custody and store; they say so, and
// their collection token is STAGE-ONLY: the fixture has no service collection
// to mint it, so the control stands in for the collection owner's word.

/// STAGE-ONLY: the collection owner's word, minted by the control for a
/// fixture that has no service collection.
fn collected_for(registry: &XServerFrontendRouteRegistry) -> PrivateConnectionsCollected {
    PrivateConnectionsCollected {
        registry: Arc::clone(&registry.clients),
    }
}

/// Start a fixture connection's worker: through the real visit with the real
/// body, or through the registered startup with a body that panics with a
/// payload.
fn start_worker(f: &PrivateWorkerFixture, custody: &PrivateEvidenceCustody, panicking: Option<&'static str>) {
    match panicking {
        None => {
            visitable(f);
            let lease = f.fixture.keeper.lease();
            let frontend = f.fixture.runner.frontend.as_ref().expect("a live runner");
            assert_eq!(attach_ready_workers(frontend, &lease), 1, "the visit starts the worker");
        }
        Some(payload) => {
            visitable(f);
            let context = custody.prepare_control().expect("its own owner");
            assert_eq!(
                context.start(|| std::thread::Builder::new().spawn(move || panic!("{payload}"))),
                PrivateStartupOutcome::Started
            );
        }
    }
}

/// Stop and collect a fixture connection's worker through the real collection
/// functions, after its registration has gone.
fn stop_and_collect(
    lease: &PrivateServiceLease<'_>,
    registry: &XServerFrontendRouteRegistry,
    custody: &PrivateEvidenceCustody,
) -> Vec<PrivateWorkerCollection> {
    assert_eq!(
        custody.cleanup_record().destruction_standing(),
        PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::WorkerRunning
        ))
    );
    let failures = stop_attached_workers(lease, registry);
    assert!(failures.is_empty(), "{failures:?}");
    let (workers, uncollected) = collect_attached_workers(lease, registry);
    assert!(uncollected.is_empty(), "{uncollected:?}");
    workers
}


/// What the owner can read about one custody after a visit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CleanupSeen {
    standing: PrivateDeferredCleanupStanding,
    fence: Option<PrivateHandoverFence>,
    fence_phase: PrivateFencePhase,
    committed: bool,
    destination: Option<&'static str>,
    home: PrivateHomeStanding,
    home_worker: Option<PrivateOrderedWorkerExit>,
    number: Option<PrivateNumberStanding>,
    row: bool,
    lease_on_record: bool,
    abandoned: Option<usize>,
    destruction: PrivateDestructionStanding,
}

fn cleanup_seen(
    custody: &PrivateEvidenceCustody,
    registry: &XServerFrontendRouteRegistry,
    durable: &PrivateSettlementOwner,
) -> CleanupSeen {
    let record = custody.cleanup_record();
    let place = custody.identity().index;
    CleanupSeen {
        standing: custody.deferred_cleanup_standing(),
        fence: custody.fence_evidence().fence(),
        fence_phase: custody.fence_evidence().phase(),
        committed: durable.committed_obligation(place).is_some(),
        destination: maintenance_destination(durable, place),
        home: record.ordered_home.standing(),
        home_worker: record.ordered_home.borrow(|continuation| match continuation {
            PrivateOrderedContinuation::Setup { evidence, .. }
            | PrivateOrderedContinuation::Serving { evidence, .. } => evidence.worker.clone(),
        }),
        number: registry.occupancy.state_of(record.client),
        row: registry
            .clients
            .lock()
            .map(|clients| clients.contains_key(&record.client))
            .unwrap_or(false),
        lease_on_record: record
            .ordered_continuation
            .lock()
            .map(|held| held.is_some())
            .unwrap_or(false),
        abandoned: durable.continuations_abandoned(),
        destruction: record.destruction_standing(),
    }
}

fn joined_evidence(seen: &CleanupSeen) -> Option<Arc<PrivateJoinEvidence>> {
    match &seen.home_worker {
        Some(PrivateOrderedWorkerExit::Joined(weak)) => weak.upgrade(),
        _ => None,
    }
}

#[test]
fn a_collected_connections_deferred_cleanup_is_discharged_from_its_own_facts() {
    let f = worker_fixture(XServerFrontendClientId(9501));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let workers = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    assert_eq!(workers.len(), 1);
    assert!(workers[0].joined);
    let before = cleanup_seen(&custody, registry, durable);
    let token = collected_for(registry);
    let outcome = custody.visit_deferred_cleanup(Some(&token));
    let after = cleanup_seen(&custody, registry, durable);
    assert_eq!(before.number, Some(PrivateNumberStanding::Held), "held until the cleanup");
    assert!(before.row && before.lease_on_record && !before.committed);
    assert_eq!(before.home, PrivateHomeStanding::Live);
    let report = outcome.result.expect("discharged");
    assert_eq!(outcome.place, custody.identity().index);
    assert_eq!(report.closure, PrivateHandoverFence::Established);
    assert!(report.committed);
    assert_eq!(report.namespace, PrivateNamespaceClearance::Established);
    assert_eq!(after.standing, PrivateDeferredCleanupStanding::Done(report));
    assert_eq!(after.fence, Some(PrivateHandoverFence::Established));
    assert!(after.committed, "the maintenance obligation is in the store");
    assert_eq!(after.destination, Some("taken"));
    assert_eq!(after.home, PrivateHomeStanding::Retained, "the home is retained, not drained");
    let evidence = joined_evidence(&after).expect("the home names the joined worker's evidence");
    assert!(Arc::ptr_eq(&evidence, custody.join()), "exactly the custody's own");
    assert!(matches!(evidence.result(), Some(PrivateJoinResult::Returned)));
    assert_eq!(after.number, None, "the completed namespace cleanup released the number");
    assert!(!after.row, "and removed the row");
    assert!(!after.lease_on_record, "the lease was committed, not left or abandoned");
    assert_eq!(after.abandoned, before.abandoned, "nothing was counted abandoned");
    assert_eq!(after.destruction, before.destruction, "the destruction decision is not rewritten");
    drop(custody);
}

#[test]
fn a_panicked_worker_is_discharged_with_its_exact_payload_named_from_the_home() {
    let f = worker_fixture(XServerFrontendClientId(9502));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, Some("carried out of the worker frame"));
    drop(f.fixture.registration);
    let workers = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    assert_eq!(workers[0].join, Some(PrivateJoinKind::Panicked));
    let token = collected_for(registry);
    let report = custody
        .visit_deferred_cleanup(Some(&token))
        .result
        .expect("a panicked join is a collected worker too");
    assert_eq!(report.namespace, PrivateNamespaceClearance::Established);
    let after = cleanup_seen(&custody, registry, durable);
    let evidence = joined_evidence(&after).expect("named from the home");
    let Some(PrivateJoinResult::Panicked(payload)) = evidence.result() else {
        panic!("the join kept the panic: {:?}", evidence.phase())
    };
    let payload = payload.lock().expect("a readable payload");
    assert_eq!(
        payload.downcast_ref::<String>().map(String::as_str),
        Some("carried out of the worker frame"),
        "the exact payload, still in the custody the owner keeps"
    );
    drop(payload);
    assert_eq!(after.number, None);
    drop(custody);
}

#[test]
fn a_repeated_visit_answers_the_same_report_and_runs_nothing_again() {
    let f = worker_fixture(XServerFrontendClientId(9503));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let token = collected_for(registry);
    let first = custody.visit_deferred_cleanup(Some(&token));
    let seen_first = cleanup_seen(&custody, registry, durable);
    let second = custody.visit_deferred_cleanup(Some(&token));
    let seen_second = cleanup_seen(&custody, registry, durable);
    assert!(first.result.is_ok());
    assert_eq!(second, first, "the second visit answers from the record");
    assert_eq!(seen_second, seen_first, "and changes nothing: fence phase, store, number, row");
    drop(custody);
}

#[test]
fn a_stale_visit_after_a_successor_took_the_number_touches_nothing_of_the_successors() {
    // SCOPE: stale row/number cleanup exactness. The successor is registered
    // under the released number; whether the predecessor's lifecycle binding
    // permits re-admission is a separate boundary (the lifecycle owner's own
    // drive), exercised as a labelled fixture step below, not by the visit.
    let f = worker_fixture(XServerFrontendClientId(9504));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let client = f.fixture.client;
    let registry = worker_registry(&f.fixture.runner);
    let lifecycle = fixture_frontend(&f).terminal.lifecycle.clone();
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let token = collected_for(registry);
    assert!(custody.visit_deferred_cleanup(Some(&token)).result.is_ok());
    assert_eq!(registry.occupancy.state_of(client), None, "released");
    // A REAL SUCCESSOR UNDER THE RELEASED NUMBER.
    let (successor, _channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free for a successor");
    // FIXTURE STEP, SEPARATE FROM THE VISIT: the predecessor's lifecycle
    // binding is finished by the EXISTING lifecycle owner's authorized drive
    // (as earlier same-number controls do), after which the successor is
    // admitted through the lifecycle. Without that drive the participant
    // still holds the predecessor's binding and refuses `AlreadyAdmitted`:
    // the namespace clearance released the number, not that binding.
    let before_drive = registry.attach_private_lifecycle(&successor, admitted(client));
    lifecycle_drain(&lifecycle);
    registry
        .attach_private_lifecycle(&successor, admitted(client))
        .expect("after the lifecycle owner's drive the successor is admitted");
    assert!(
        matches!(before_drive, Err(ref refused) if refused.to_string().contains("AlreadyAdmitted")),
        "the visit did not retire the predecessor's binding: {before_drive:?}"
    );
    let successor_state = Arc::clone(&successor.connection_state);
    assert_eq!(registry.occupancy.state_of(client), Some(PrivateNumberStanding::Held));
    // THE STALE VISIT, repeated on the old custody.
    let repeated = custody.visit_deferred_cleanup(Some(&token));
    assert!(repeated.result.is_ok(), "answered from the old record");
    let row_state = registry
        .clients
        .lock()
        .expect("a readable registry")
        .get(&client)
        .map(|entry| Arc::clone(&entry.connection_state));
    assert!(
        row_state.is_some_and(|state| Arc::ptr_eq(&state, &successor_state)),
        "the successor's row is still the successor's"
    );
    assert_eq!(
        registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "and its number claim is untouched"
    );
    assert!(
        registry
            .input_recovery
            .disconnect_exact(client, &successor_state, XAuthorityInputDeliveryOutcome::ClientDisconnected, None)
            .is_ok_and(|ours| ours),
        "the successor's recovery entry is still its own to disconnect"
    );
    drop((successor, custody));
}

#[test]
fn a_visit_without_the_collections_token_refuses_and_runs_nothing() {
    let f = worker_fixture(XServerFrontendClientId(9505));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let outcome = custody.visit_deferred_cleanup(None);
    let seen = cleanup_seen(&custody, registry, durable);
    assert_eq!(outcome.result, Err(PrivateDeferredCleanupRefusal::ConnectionsUncollected));
    assert_eq!(
        seen.standing,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::ConnectionsUncollected,
            progress: PrivateDeferredCleanupProgress::default(),
        }
    );
    assert_eq!(seen.fence_phase, PrivateFencePhase::NotAttempted, "no fence was attempted");
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held));
    assert!(seen.row && seen.lease_on_record && !seen.committed);
    assert_eq!(seen.home, PrivateHomeStanding::Live);
    // Another registry's token is no better than none.
    let other = PrivateSettlementOwner::default();
    let other_keeper = service_owner(&other, 2);
    let other_private = private_over(&other_keeper, 2);
    let foreign = collected_for(&other_private.broker.registry);
    assert_eq!(
        custody.visit_deferred_cleanup(Some(&foreign)).result,
        Err(PrivateDeferredCleanupRefusal::ConnectionsUncollected)
    );
    // The real token then lets it proceed from the same facts.
    let token = collected_for(registry);
    assert!(custody.visit_deferred_cleanup(Some(&token)).result.is_ok());
    drop((custody, other_private, other_keeper, other));
}

#[test]
fn an_unrequested_undecided_or_unestablished_destruction_is_not_reinterpreted() {
    // Unrequested: the registration is alive.
    let f = worker_fixture(XServerFrontendClientId(9506));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    let token = collected_for(registry);
    assert_eq!(
        custody.visit_deferred_cleanup(Some(&token)).result,
        Err(PrivateDeferredCleanupRefusal::DestructionNotRequested)
    );
    // Undecided: STAGE-ONLY, the claim without its decision, as a lost frame
    // leaves it.
    assert!(custody.cleanup_record().claim_destruction());
    assert_eq!(
        custody.visit_deferred_cleanup(Some(&token)).result,
        Err(PrivateDeferredCleanupRefusal::DestructionUndecided)
    );
    // Unestablished: STAGE-ONLY, a decision that deferred over uncertainty.
    assert!(custody.cleanup_record().publish_destruction(PrivateDestructionDecision::Deferred(
        PrivateDestructionDeferral::Deciding
    )));
    assert_eq!(
        custody.visit_deferred_cleanup(Some(&token)).result,
        Err(PrivateDeferredCleanupRefusal::DestructionUnestablished(
            PrivateDestructionDeferral::Deciding
        ))
    );
    let seen = cleanup_seen(&custody, registry, durable);
    assert_eq!(seen.fence_phase, PrivateFencePhase::NotAttempted);
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held));
    assert!(!seen.committed && seen.lease_on_record);
    drop(custody);
}

#[test]
fn a_worker_held_or_handed_on_without_a_custody_join_is_refused_before_any_effect() {
    let f = worker_fixture(XServerFrontendClientId(9507));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let token = collected_for(registry);
    // Held live: no join yet.
    let held = custody.visit_deferred_cleanup(Some(&token));
    // Handed elsewhere: the slot gives its handle to a joiner outside the
    // custody; the custody's join is still unpublished.
    let handle = hand_worker_to_joiner(custody.worker_slot()).handle.expect("the handle");
    let handed = custody.visit_deferred_cleanup(Some(&token));
    let seen = cleanup_seen(&custody, registry, durable);
    cancel_connection_worker(&f.stop, &f.wake);
    handle.join().expect("the worker returned");
    let after_join = custody.visit_deferred_cleanup(Some(&token));
    assert_eq!(held.result, Err(PrivateDeferredCleanupRefusal::JoinUnpublished));
    assert_eq!(handed.result, Err(PrivateDeferredCleanupRefusal::JoinUnpublished));
    assert_eq!(seen.fence_phase, PrivateFencePhase::NotAttempted, "no effect ran");
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held));
    assert_eq!(
        after_join.result,
        Err(PrivateDeferredCleanupRefusal::JoinUnpublished),
        "a join elsewhere is not the custody's published join"
    );
    drop(custody);
}

#[test]
fn an_unreadable_fence_commits_the_obligation_but_withholds_the_namespace_cleanup() {
    let f = worker_fixture(XServerFrontendClientId(9508));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    // STAGE-ONLY: a holder unwinds inside the gate before the visit.
    let gate = Arc::clone(custody.gate());
    let poisoner = std::thread::spawn(move || {
        let _inside = gate.fenced.lock().expect("an open gate");
        panic!("a handover panicked inside this gate");
    });
    assert!(poisoner.join().is_err());
    let token = collected_for(registry);
    let first = custody.visit_deferred_cleanup(Some(&token));
    let seen = cleanup_seen(&custody, registry, durable);
    let second = custody.visit_deferred_cleanup(Some(&token));
    let seen_again = cleanup_seen(&custody, registry, durable);
    assert_eq!(first.result, Err(PrivateDeferredCleanupRefusal::ClosureUnestablished));
    assert_eq!(seen.fence, Some(PrivateHandoverFence::Unreadable), "recorded as it was");
    assert!(seen.committed, "responsibility is committed");
    assert_eq!(seen.home, PrivateHomeStanding::Retained);
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held), "closure unestablished: no cleanup");
    assert!(seen.row);
    assert_eq!(
        seen.standing,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::ClosureUnestablished,
            progress: PrivateDeferredCleanupProgress {
                closure: Some(PrivateHandoverFence::Unreadable),
                home_retained: true,
                home_occupied: true,
                committed: true,
                namespace: None,
            },
        },
        "the completed phases are kept with the refusal"
    );
    assert_eq!(second.result, first.result, "not retried into something else");
    assert_eq!(seen_again, seen, "and nothing moved: the fence stays Unreadable, the number Held");
    drop(custody);
}

#[test]
fn a_poisoned_number_keyed_table_leaves_the_number_unestablished_and_is_not_retried() {
    let f = worker_fixture(XServerFrontendClientId(9509));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    // STAGE-ONLY: a holder unwinds inside the client table before the visit.
    let clients = Arc::clone(&worker_registry(&f.fixture.runner).clients);
    let poisoner = std::thread::spawn(move || {
        let _inside = clients.lock().expect("a readable registry");
        panic!("a holder unwound inside the client table");
    });
    assert!(poisoner.join().is_err());
    let token = collected_for(registry);
    let first = custody.visit_deferred_cleanup(Some(&token));
    let seen = cleanup_seen(&custody, registry, durable);
    let second = custody.visit_deferred_cleanup(Some(&token));
    let report = first.result.expect("the visit ran to its end");
    assert_eq!(report.namespace, PrivateNamespaceClearance::Unestablished);
    assert!(report.committed);
    assert_eq!(
        seen.number,
        Some(PrivateNumberStanding::Unestablished),
        "an effect that could not run keeps the number excluded"
    );
    assert_eq!(second.result, Ok(report), "not retried: the same answer, no second interval");
    assert_eq!(
        cleanup_seen(&custody, registry, durable).number,
        Some(PrivateNumberStanding::Unestablished)
    );
    drop(custody);
}

#[test]
fn an_interruption_after_the_claim_stays_interrupted_and_owned() {
    let f = worker_fixture(XServerFrontendClientId(9510));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let token = collected_for(registry);
    stage_deferred_cleanup_at(PrivateDeferredCleanupPoint::AfterClaim, || {
        panic!("the visit is lost right after its claim")
    });
    let lost = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        custody.visit_deferred_cleanup(Some(&token))
    }));
    let seen = cleanup_seen(&custody, registry, durable);
    let again = custody.visit_deferred_cleanup(Some(&token));
    assert!(lost.is_err());
    assert_eq!(
        seen.standing,
        PrivateDeferredCleanupStanding::Claimed {
            progress: PrivateDeferredCleanupProgress::default()
        }
    );
    assert_eq!(seen.fence_phase, PrivateFencePhase::NotAttempted);
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held));
    assert!(seen.row && seen.lease_on_record);
    assert_eq!(again.result, Err(PrivateDeferredCleanupRefusal::Interrupted));
    assert_eq!(cleanup_seen(&custody, registry, durable), seen, "a repeat asks nothing and changes nothing");
    drop(custody);
}

fn interrupted_between_lease_and_commit(
    client: XServerFrontendClientId,
    point: PrivateDeferredCleanupPoint,
    expected_destination: &'static str,
) {
    let f = worker_fixture(client);
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let token = collected_for(registry);
    let before = cleanup_seen(&custody, registry, durable);
    let reserved_before = durable.continuations_reserved();
    stage_deferred_cleanup_at(point, || panic!("the visit is lost inside the commitment"));
    let lost = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        custody.visit_deferred_cleanup(Some(&token))
    }));
    let seen = cleanup_seen(&custody, registry, durable);
    let again = custody.visit_deferred_cleanup(Some(&token));
    assert!(lost.is_err());
    assert!(seen.lease_on_record, "{point:?}: the lease is back on the record, not dropped");
    assert_eq!(seen.abandoned, before.abandoned, "{point:?}: nothing counted abandoned");
    assert_eq!(
        seen.destination,
        Some(expected_destination),
        "{point:?}: the destination went back to its reservation"
    );
    assert!(!seen.committed, "{point:?}: nothing committed");
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held), "{point:?}");
    assert_eq!(
        seen.standing,
        PrivateDeferredCleanupStanding::Claimed {
            progress: PrivateDeferredCleanupProgress {
                closure: Some(PrivateHandoverFence::Established),
                home_retained: true,
                home_occupied: true,
                committed: false,
                namespace: None,
            }
        },
        "{point:?}: the phases that completed are kept, the rest is not"
    );
    assert_eq!(again.result, Err(PrivateDeferredCleanupRefusal::Interrupted));
    assert_eq!(
        durable.continuations_reserved(),
        reserved_before,
        "{point:?}: still charged, with no replacement reservation"
    );
    drop(custody);
}

#[test]
fn an_interruption_after_the_lease_left_the_record_returns_it() {
    interrupted_between_lease_and_commit(
        XServerFrontendClientId(9511),
        PrivateDeferredCleanupPoint::AfterLeaseTaken,
        "reserved",
    );
}

#[test]
fn an_interruption_after_the_destination_was_prepared_returns_both() {
    interrupted_between_lease_and_commit(
        XServerFrontendClientId(9512),
        PrivateDeferredCleanupPoint::AfterDestinationPrepared,
        "reserved",
    );
}

#[test]
fn a_record_without_its_lease_refuses_before_committing_and_keeps_the_number() {
    let f = worker_fixture(XServerFrontendClientId(9514));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let token = collected_for(registry);
    // STAGE-ONLY: the record's lease is taken out from under the visit, so
    // the commitment has nothing of this connection's to bind.
    let taken = custody
        .cleanup_record()
        .ordered_continuation
        .lock()
        .expect("a readable slot")
        .take();
    assert!(taken.is_some(), "the fixture's record held its lease");
    let outcome = custody.visit_deferred_cleanup(Some(&token));
    let seen = cleanup_seen(&custody, registry, durable);
    let again = custody.visit_deferred_cleanup(Some(&token));
    assert_eq!(outcome.result, Err(PrivateDeferredCleanupRefusal::LeaseMissing));
    assert!(!seen.committed, "nothing was committed without the lease");
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held), "no number-keyed effect ran");
    assert!(seen.row);
    assert_eq!(seen.home, PrivateHomeStanding::Retained, "the phases before it are kept");
    assert_eq!(
        seen.standing,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::LeaseMissing,
            progress: PrivateDeferredCleanupProgress {
                closure: Some(PrivateHandoverFence::Established),
                home_retained: true,
                home_occupied: true,
                committed: false,
                namespace: None,
            }
        }
    );
    assert_eq!(again.result, Err(PrivateDeferredCleanupRefusal::LeaseMissing), "owned, not spent");
    drop((taken, custody));
}

#[test]
fn a_refusal_after_effects_keeps_them_while_an_early_refusal_has_none() {
    // Early: refused before any effect (no join), progress empty.
    let f = worker_fixture(XServerFrontendClientId(9513));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let lease = f.fixture.keeper.lease();
    let token = collected_for(registry);
    let early = custody.visit_deferred_cleanup(Some(&token));
    let early_seen = cleanup_seen(&custody, registry, durable);
    // Then the worker is collected and the gate is poisoned before the
    // next visit: the fence records Unreadable, the home is retained, the
    // obligation committed, and the visit refuses AFTER those effects.
    let _ = stop_attached_workers(&lease, registry);
    let (workers, _) = collect_attached_workers(&lease, registry);
    assert!(workers[0].joined);
    let gate = Arc::clone(custody.gate());
    assert!(
        std::thread::spawn(move || {
            let _inside = gate.fenced.lock().expect("an open gate");
            panic!("a handover panicked inside this gate");
        })
        .join()
        .is_err()
    );
    let late = custody.visit_deferred_cleanup(Some(&token));
    let late_seen = cleanup_seen(&custody, registry, durable);
    assert_eq!(early.result, Err(PrivateDeferredCleanupRefusal::JoinUnpublished));
    assert_eq!(
        early_seen.standing,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::JoinUnpublished,
            progress: PrivateDeferredCleanupProgress::default(),
        }
    );
    assert_eq!(early_seen.home, PrivateHomeStanding::Live);
    assert_eq!(late.result, Err(PrivateDeferredCleanupRefusal::ClosureUnestablished));
    assert_eq!(late_seen.home, PrivateHomeStanding::Retained);
    assert!(late_seen.committed);
    assert!(matches!(
        late_seen.standing,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::ClosureUnestablished,
            progress: PrivateDeferredCleanupProgress {
                closure: Some(PrivateHandoverFence::Unreadable),
                home_retained: true,
                committed: true,
                ..
            }
        }
    ));
    assert_eq!(late_seen.number, Some(PrivateNumberStanding::Held));
    drop(custody);
}


// Independent probes from the 73543ee9 review, landed as written (names
// kept). FIXTURE-TOKEN SCOPE: the collection tokens are STAGE-ONLY stand-ins
// for the absent service frame; no production collector or new driving is
// added, and nothing here says anything about concurrent visitors.
#[test]
fn review_holder_refusal_restores_the_lease_and_later_resumes_exactly_once() {
    let f = worker_fixture(XServerFrontendClientId(9651));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let durable = &f.fixture.durable;
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    let token = collected_for(registry);
    // A real outstanding promise causes preparation to refuse. Its own
    // destination guard remains here until the second visit can proceed.
    let destination = {
        let slot = custody.cleanup_record().ordered_continuation.lock().unwrap();
        durable.prepare_internal_holder(slot.as_ref().unwrap()).unwrap()
    };
    let before = cleanup_seen(&custody, registry, durable);
    let refused = custody.visit_deferred_cleanup(Some(&token));
    let after_refusal = cleanup_seen(&custody, registry, durable);
    drop(destination);
    let after_returning_destination = cleanup_seen(&custody, registry, durable);
    let resumed = custody.visit_deferred_cleanup(Some(&token));
    let after_resume = cleanup_seen(&custody, registry, durable);
    let repeated = custody.visit_deferred_cleanup(Some(&token));
    assert_eq!(refused.result, Err(PrivateDeferredCleanupRefusal::HolderRefused(PrivateHolderRefusal::AlreadyHeld)));
    assert!(after_refusal.lease_on_record);
    assert_eq!(after_refusal.abandoned, before.abandoned);
    assert_eq!(after_refusal.number, Some(PrivateNumberStanding::Held));
    assert!(after_refusal.row && !after_refusal.committed);
    assert_eq!(after_returning_destination.destination, Some("reserved"));
    let report = resumed.result.expect("the original lease is still usable");
    assert_eq!(report.namespace, PrivateNamespaceClearance::Established);
    assert!(after_resume.committed && !after_resume.lease_on_record);
    assert_eq!(after_resume.number, None);
    assert_eq!(after_resume.abandoned, before.abandoned);
    assert_eq!(repeated, resumed);
    assert_eq!(cleanup_seen(&custody, registry, durable), after_resume);
}

#[test]
fn review_unreadable_home_keeps_its_poison_and_exact_join_evidence() {
    let f = worker_fixture(XServerFrontendClientId(9652));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    start_worker(&f, &custody, None);
    drop(f.fixture.registration);
    let _ = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
    // Poison only after collecting the worker, to isolate maintenance's
    // handling of the original home from the worker's unreadable exit.
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = f.home.state.lock().unwrap();
        panic!("review home holder interrupted");
    }));
    let outcome = custody.visit_deferred_cleanup(Some(&collected_for(registry)));
    let evidence = f.home.borrow(|continuation| match continuation {
        PrivateOrderedContinuation::Setup { evidence, .. }
        | PrivateOrderedContinuation::Serving { evidence, .. } => evidence.clone(),
    }).expect("the original continuation remains");
    assert!(poisoned.is_err());
    assert!(f.home.unreadable());
    assert_eq!(f.home.standing(), PrivateHomeStanding::Retained);
    assert!(evidence.source_poisoned);
    let PrivateOrderedWorkerExit::Joined(join) = evidence.worker else { panic!("joined evidence expected") };
    assert!(std::ptr::eq(join.as_ptr(), Arc::as_ptr(custody.join())));
    assert_eq!(outcome.result.unwrap().namespace, PrivateNamespaceClearance::Established);
    assert!(f.home.peek_retained(|_| ()).is_none(), "retained readers must still see unreadable");
}

#[test]
fn review_joined_home_adds_no_store_cycle_through_the_actual_panic_payload() {
    let (store, join, custody_weak, home) = {
        let f = worker_fixture(XServerFrontendClientId(9653));
        f.permit();
        let custody = custody_for(&f, &f.fixture.keeper);
        let registry = worker_registry(&f.fixture.runner);
        visitable(&f);
        let payload = f.fixture.durable.clone();
        let context = custody.prepare_control().unwrap();
        assert_eq!(context.start(|| std::thread::Builder::new().spawn(move || {
            std::panic::panic_any(payload)
        })), PrivateStartupOutcome::Started);
        drop(f.fixture.registration);
        let workers = stop_and_collect(&f.fixture.keeper.lease(), registry, &custody);
        assert_eq!(workers[0].join, Some(PrivateJoinKind::Panicked));
        let outcome = custody.visit_deferred_cleanup(Some(&collected_for(registry)));
        assert_eq!(outcome.result.unwrap().namespace, PrivateNamespaceClearance::Established);
        let Some(PrivateJoinResult::Panicked(payload)) = custody.join().result() else { panic!("actual panic payload expected") };
        assert!(payload.lock().unwrap().downcast_ref::<PrivateSettlementOwner>().unwrap().is_same_store(&f.fixture.durable));
        (Arc::downgrade(&f.fixture.durable.inner), Arc::downgrade(custody.join()), Arc::downgrade(&custody.custody), Arc::downgrade(&f.home))
    };
    assert!(custody_weak.upgrade().is_none(), "the owner's custody has ended");
    assert!(join.upgrade().is_none(), "the home does not own the panic payload");
    assert!(home.upgrade().is_none(), "no store/home/join/payload cycle");
    assert!(store.upgrade().is_none(), "the exact payload's store is released");
}

// t138. The live idle-window reclaim and the non-blocking reap it needs.
// These run the real reaping record and the real discharge over a worker
// fixture, the same way the controls above do.

#[test]
fn a_running_worker_is_not_reaped_and_its_record_may_be_asked_again() {
    // The whole point of the non-blocking ask. `reap` would wait here, and
    // the service frame that drives this cannot: it is the thread that
    // accepts connections and serves the order, and a blocked ordered
    // delivery is allowed six seconds.
    let f = worker_fixture(XServerFrontendClientId(9520));
    let custody = custody_for(&f, &f.fixture.keeper);
    start_worker(&f, &custody, None);

    let reaping = PrivateReapingRecord::bound_to(&custody).reap_finished();
    assert_eq!(
        reaping.reaped,
        PrivateReaped::StillRunning,
        "a worker parked on its queue is not finished"
    );
    assert_eq!(reaping.exit, None, "nothing was read from the exit record");
    // NOTHING WAS CONSUMED, which is the property the whole design rests on:
    // there is no way to put a handle back, so an ask that would have had to
    // wait must leave the slot exactly as it found it.
    let slot = custody.worker_slot();
    let held = slot.lock().expect("a readable slot");
    assert!(held.handle.is_some(), "the handle is still in the slot");
    assert_eq!(
        held.life,
        PrivateWorkerLife::Running,
        "and its life was not advanced to HandedToJoiner"
    );
    drop(held);

    // So the record is askable again, and the blocking ask still works.
    let lease = f.fixture.keeper.lease();
    let registry = worker_registry(&f.fixture.runner);
    drop(f.fixture.registration);
    let workers = stop_and_collect(&lease, registry, &custody);
    assert!(
        workers.iter().all(|worker| worker.joined),
        "the ordinary collection still joins it: {workers:?}"
    );
    drop(custody);
}

#[test]
fn a_finished_worker_is_reaped_by_the_non_blocking_ask() {
    let f = worker_fixture(XServerFrontendClientId(9521));
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    start_worker(&f, &custody, None);
    let lease = f.fixture.keeper.lease();
    drop(f.fixture.registration);
    let failures = stop_attached_workers(&lease, registry);
    assert!(failures.is_empty(), "{failures:?}");

    // ASKED REPEATEDLY RATHER THAN WAITED FOR, because that is what a caller
    // who may not block does. A stopped thread is not an instantly finished
    // one, and each refusal consumes nothing, so asking again is the
    // sanctioned way to get there.
    let record = PrivateReapingRecord::bound_to(&custody);
    let mut reaped = PrivateReaped::StillRunning;
    for _ in 0..2000 {
        reaped = record.reap_finished().reaped;
        if reaped != PrivateReaped::StillRunning {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(reaped, PrivateReaped::Joined, "the finished worker was joined");
    assert!(
        record.result().is_some(),
        "and its result was published, which is what says it was collected"
    );
    drop(custody);
}

#[test]
fn the_idle_window_reclaims_a_departed_connection_and_a_busy_one_does_not() {
    let f = worker_fixture(XServerFrontendClientId(9522));
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let frontend = f.fixture.runner.frontend.as_ref().expect("a live runner");
    start_worker(&f, &custody, None);
    let lease = f.fixture.keeper.lease();
    drop(f.fixture.registration);
    let failures = stop_attached_workers(&lease, registry);
    assert!(failures.is_empty(), "{failures:?}");

    // A FRAME STILL ACTIVE IS NOT A WINDOW. The count is the whole guard:
    // the token this would mint says no connection frame is active, and with
    // one active that would be a lie. Refusing on the number is what keeps
    // the ordinary token honest rather than widening it.
    assert_eq!(
        reclaim_idle_departures(frontend, &lease, 1),
        0,
        "nothing is reclaimed while a connection frame is active"
    );
    assert_eq!(
        cleanup_seen(&custody, registry, &f.fixture.durable).standing,
        PrivateDeferredCleanupStanding::NotVisited,
        "and the cleanup was not even visited"
    );

    // Then the window. Asked repeatedly for the same reason as above: the
    // discharge refuses JoinUnpublished until the reap lands, and neither
    // step waits.
    let mut progressed = 0;
    for _ in 0..2000 {
        progressed += reclaim_idle_departures(frontend, &lease, 0);
        if matches!(
            cleanup_seen(&custody, registry, &f.fixture.durable).standing,
            PrivateDeferredCleanupStanding::Done(_)
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(progressed > 0, "the window reclaimed something");
    let seen = cleanup_seen(&custody, registry, &f.fixture.durable);
    assert!(
        matches!(seen.standing, PrivateDeferredCleanupStanding::Done(_)),
        "the deferred cleanup discharged during the run, not at shutdown: {seen:?}"
    );
    assert_eq!(
        seen.home,
        PrivateHomeStanding::Retained,
        "and the home its connection left was finally told the connection had gone"
    );
    drop(custody);
}
