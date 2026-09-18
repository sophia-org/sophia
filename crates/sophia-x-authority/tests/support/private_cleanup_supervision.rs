// Private runner controls: actual producer reservations and completion records,
// with no connection worker. Public watchdog controls cover real socket ending.

#[test]
fn closing_production_keeps_accepted_commands_owned_without_routing_them() {
    let (mut runner, owner, registration, channels, _acks, _deliveries) = prepared_runner_fixture();
    let lease = owner.lease();
    let producer = runner.control_producer(&lease).unwrap();
    producer
        .submit(
            &lease,
            configure(
                XServerFrontendClientId::from_raw(9000),
                SurfaceId::new(9000, 1),
                98700,
            ),
        )
        .unwrap();
    let before = owner.store().reserved();
    runner.close_admission();
    runner.close_admission();
    let refused = producer
        .submit(
            &lease,
            configure(
                XServerFrontendClientId::from_raw(9000),
                SurfaceId::new(9000, 1),
                98701,
            ),
        )
        .is_err();
    let progress = runner.service_turn(&lease).unwrap();
    let pending = runner
        .frontend()
        .admission
        .ready
        .lock()
        .unwrap()
        .ready
        .len();
    let sent = channels.control.try_recv();
    let reserved = owner.store().reserved();
    drop(registration);
    drop(runner.shutdown());
    assert!(refused);
    assert_eq!((progress.taken, progress.routed), (0, 0));
    assert_eq!(
        pending, 1,
        "the accepted original is still held in its queue"
    );
    assert!(matches!(sent, Err(std::sync::mpsc::TryRecvError::Empty)));
    assert_eq!(reserved, before);
}

#[test]
fn closing_production_preserves_accounted_cleanup_on_the_same_runner() {
    let (mut runner, owner, registration, channels, _acks, _deliveries) = prepared_runner_fixture();
    let lease = owner.lease();
    route_one_control(&mut runner, &lease, &channels, 98710).unwrap();
    let budget_origin = runner.service_origin;
    let reserved = owner.store().reserved().unwrap();
    runner.close_admission();
    let pending = runner.service_turn(&lease).unwrap();
    let before_receipt = runner.frontend().outstanding.len();
    // Supplied exact writer publication, not a real socket or writer receipt.
    retire_control(&runner, 98710);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut reclaimed = 0;
    let mut charged = 0;
    while reclaimed == 0 && Instant::now() < deadline {
        let progress = runner.service_turn(&lease).unwrap();
        reclaimed += progress.reclaimed;
        charged += progress.starts;
        if reclaimed == 0 {
            std::thread::park_timeout(Duration::from_millis(1));
        }
    }
    let origin_unchanged = runner.service_origin == budget_origin;
    let remaining = runner.frontend().outstanding.len();
    let after = owner.store().reserved();
    let gate_closed = !runner.frontend().admission.lifecycle_open();
    let failure = runner.frontend().admission.watch.get().unwrap().failure();
    drop(registration);
    drop(runner.shutdown());
    assert_eq!(pending.reclaimed, 0);
    assert_eq!(before_receipt, 1);
    assert_eq!((reclaimed, remaining), (1, 0));
    assert!(charged >= 1 && origin_unchanged && gate_closed);
    assert_eq!(after, Some(reserved - 1));
    assert_eq!(failure, None);
}
