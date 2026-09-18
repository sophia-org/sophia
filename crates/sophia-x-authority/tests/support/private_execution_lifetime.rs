// Same-thread execution continuity and independently owned loss records.

fn assert_retained_execution_then_abandoned(outcome: &ProducedOutcome) {
    let retained = outcome
        .execution
        .expect("the outer scope kept the original execution");
    assert_eq!(
        retained.availability,
        PrivateExecutionAvailability::Retained
    );
    assert!(
        outcome.execution_inventory_matches,
        "inventory borrows only its exact original executor"
    );
    assert!(
        outcome.execution_collected,
        "the actual collection token survives handoff"
    );
    assert_eq!(
        outcome.execution_abandoned,
        vec![PrivateExecutionReading {
            instance: retained.instance,
            availability: PrivateExecutionAvailability::Abandoned,
        }],
        "ending the executing thread records history loss beside its unresolved hold"
    );
}

#[test]
fn execution_handoff_preserves_native_history_budget_cursors_and_failed_supervisor() {
    use sophia_input_authority::{CleanupReadiness, ServiceWork};
    let (mut runner, owner, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let origin = runner.frontend().broker.registry.clone();
    let instance = runner.frontend().instance;
    let authority = runner.keyboards.authority;
    let service_origin = runner.service_origin;
    let namespace = runner.namespace;
    let seat = runner.seat;
    let gate = runner
        .frontend()
        .admission
        .watch
        .get()
        .expect("sealed gate")
        .clone();
    assert_eq!(
        runner
            .keyboards
            .apply(seat, 42, true)
            .map(|(_, _, after)| after),
        Some(1)
    );
    runner.reclaim_cursor = 7;
    drop(
        runner
            .service
            .start(
                Duration::ZERO,
                ServiceWork::Cleanup,
                CleanupReadiness::Eligible,
            )
            .unwrap(),
    );
    let usage = runner.service.usage();
    let watch = runner.watch.as_ref().unwrap();
    drop(
        watch
            .begin_dequeued(std::time::Instant::now())
            .expect("original supervisor"),
    );
    let failure = gate.failure().expect("abandoned original operation");
    runner.close_admission();
    let witness = runner.lifetime.0.clone();
    let mut keeper = PrivateServiceExecutionKeeper::new();
    let private = keeper.retain(runner, None);
    let resources = keeper
        .resources_for_inventory(&private.terminal)
        .expect("same inventory");
    assert!(Arc::ptr_eq(&resources.lifetime.0, &witness));
    assert_eq!(resources.keyboards.authority, authority);
    assert_eq!(
        resources.keyboards.modifiers(seat),
        Some(1),
        "held Shift was not rebuilt neutral"
    );
    assert_eq!((resources.namespace, resources.seat), (namespace, seat));
    assert_eq!(resources.service_origin, service_origin);
    assert_eq!(resources.service.usage(), usage);
    assert!(
        resources.service.is_interrupted(),
        "the unfinished budget stays latched"
    );
    assert_eq!(resources.reclaim_cursor, 7);
    assert!(resources.prefer_cleanup);
    assert!(
        matches!(resources.watch.as_ref().unwrap().begin_dequeued(std::time::Instant::now()),
        Err(private_watchdog::PrivateWatchdogRefusal::Failed(found)) if found == failure)
    );
    assert_eq!(
        gate.failure(),
        Some(failure),
        "same supervisor failure, no restarted watch"
    );
    assert!(keeper.resources_for(&origin, instance).is_ok());
    let settlement = private.shutdown();
    drop((settlement, keeper, owner));
    assert_eq!(
        witness.reading().availability,
        PrivateExecutionAvailability::Abandoned
    );
}

#[test]
fn execution_keeper_refuses_foreign_origin_instance_and_inventory() {
    let (runner_a, owner_a, _ra, _ca, _aa, _da) = prepared_runner_fixture();
    let (runner_b, owner_b, _rb, _cb, _ab, _db) = prepared_runner_fixture();
    let origin_a = runner_a.frontend().broker.registry.clone();
    let origin_b = runner_b.frontend().broker.registry.clone();
    let instance = runner_a.frontend().instance;
    assert_eq!(
        runner_b.frontend().instance,
        instance,
        "colliding local instance numbers"
    );
    let mut keeper = PrivateServiceExecutionKeeper::new();
    let private = keeper.retain(runner_a, None);
    assert!(matches!(
        keeper.resources_for(&origin_b, instance),
        Err(PrivateExecutionBorrowRefusal::ForeignInvocation)
    ));
    assert!(matches!(
        keeper.resources_for(&origin_a, instance + 1),
        Err(PrivateExecutionBorrowRefusal::ForeignInvocation)
    ));
    assert!(matches!(
        keeper.resources_for_inventory(&runner_b.frontend().terminal),
        Err(PrivateExecutionBorrowRefusal::ForeignInvocation)
    ));
    assert!(keeper.resources_for_inventory(&private.terminal).is_ok());
    drop((
        private.shutdown(),
        runner_b.shutdown(),
        keeper,
        owner_a,
        owner_b,
    ));
}

#[test]
fn an_occupied_execution_keeper_refuses_service_before_binding_without_replacing_history() {
    let (mut runner, owner, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let seat = runner.seat;
    runner.keyboards.apply(seat, 42, true).expect("held Shift");
    runner.close_admission();
    let mut keeper = PrivateServiceExecutionKeeper::new();
    let private = keeper.retain(runner, None);
    let first = keeper.execution();
    let other_owner = service_owner(&PrivateSettlementOwner::default(), 4);
    let other = PrivateXServerFrontend::new(private_service_parts(4), &other_owner)
        .unwrap_or_else(|(refusal, _)| panic!("second frontend: {refusal:?}"));
    let path = private_service_socket("occupied-execution-keeper");
    let (tx, _rx) = sync_channel(4);
    let (_commands, commands) = sync_channel(1);
    let (port, access) = PrivateProducerAccess::for_service();
    let result = serve_private_frontend_until_stopped(
        other,
        &other_owner.lease(),
        &mut keeper,
        private_service_config(&path, NamespaceId::from_raw(9801), 4),
        tx,
        commands,
        port,
        Arc::new(|_| {}),
    );
    assert!(
        matches!(result, Err(PrivateServiceFailure::Failed { ref error, .. }) if error.to_string().contains("already retains"))
    );
    assert!(!path.exists(), "refusal precedes binding");
    assert_eq!(access.standing(), PrivatePortStanding::Ended);
    assert_eq!(keeper.execution(), first);
    assert_eq!(
        keeper
            .resources_for_inventory(&private.terminal)
            .unwrap()
            .keyboards
            .modifiers(seat),
        Some(1)
    );
    drop((result, private.shutdown(), keeper, owner));
}
