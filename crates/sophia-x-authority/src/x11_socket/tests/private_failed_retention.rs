// Controls for the settlement store's failed-instance retention under
// recovery: the storage reserved before exposure survives an idle recovery,
// and ordinary recovery beside an instance retained over an uncollected actor
// recovers the ordinary one and leaves the standing one whole.

/// Poison a live instance's admission queue, the way a failing instance's
/// queue becomes unreadable, then shut the instance down so it is retained.
fn retain_poisoned(private: crate::PrivateXServerFrontend) {
    let admission = Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();
    drop(private.shutdown());
}

#[test]
fn idle_recovery_preserves_the_failure_storage_reserved_before_exposure() {
    let durable = PrivateSettlementOwner::with_capacity(2);
    let keeper = service_owner(&durable, 2);
    let private = private_over(&keeper, 2);
    let reserved_before = durable.records_even_if_poisoned().failed.capacity();
    assert_eq!(reserved_before, 2, "the failure buffer was reserved at construction");
    assert_eq!(durable.recover_failed(), Some(0), "nothing to recover");
    let reserved_after_idle = durable.records_even_if_poisoned().failed.capacity();
    assert_eq!(
        reserved_after_idle, reserved_before,
        "an idle recovery keeps the reserved buffer"
    );
    retain_poisoned(private);
    let held = durable.records_even_if_poisoned();
    assert_eq!(
        (held.failed.len(), held.failed.capacity(), held.failure_slots),
        (1, reserved_before, 1),
        "the later failure handed its queue into the reserved storage without allocating"
    );
    drop(held);
    drop((keeper, durable));
}

#[test]
fn ordinary_recovery_beside_a_standing_uncollected_instance_recovers_only_the_ordinary_one() {
    let durable = PrivateSettlementOwner::with_capacity(3);
    let keeper = service_owner(&durable, 3);
    let ordinary = private_over(&keeper, 3);
    let standing = private_over(&keeper, 3);
    let reserved = durable.records_even_if_poisoned().failed.capacity();
    // The ordinary failure: an unreadable queue, retained as recoverable.
    retain_poisoned(ordinary);
    // The standing one: the retention handle's own path for an instance
    // whose actor was not collected, here built from the second instance's
    // origin and queue (STAGE-ONLY construction of the handle; its Drop is
    // the product's).
    drop(PrivateSettlement {
        origin: standing.broker.registry.clone(),
        durable: durable.clone(),
        queue: Arc::clone(&standing.admission.ready),
        pending: Vec::new(),
        outstanding: Vec::new(),
        queue_unreadable: false,
        settling: None,
        terminal: None,
        uncollected: vec![7],
    });
    let slots_before = durable.failure_slots_charged();
    assert_eq!(durable.failed_instances(), Some(2));
    assert_eq!(durable.uncollected_instances(), Some(vec![vec![7]]));
    assert_eq!(durable.recover_failed(), Some(0), "the ordinary instance had accepted nothing");
    let held = durable.records_even_if_poisoned();
    assert_eq!(held.failed.len(), 1, "the ordinary instance was recovered, the standing one kept");
    assert_eq!(held.failed[0].uncollected, vec![7], "with its reason");
    assert_eq!(held.failed.capacity(), reserved, "in the reserved storage");
    assert_eq!(
        held.failure_slots,
        slots_before.expect("readable").saturating_sub(1),
        "one charge released: the ordinary instance's, not the standing one's"
    );
    drop(held);
    assert_eq!(durable.uncollected_instances(), Some(vec![vec![7]]));
    drop((standing, keeper, durable));
}
