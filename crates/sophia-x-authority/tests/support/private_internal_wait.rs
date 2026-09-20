// An internal wait is not recipient nonresponse. The six-second allowance
// that ends a delivery TimedOut is measured from the send onwards, out of
// the time the recipient kept the writer waiting for writability, and
// nothing else: not the time the delivery sat in its queue, not the time the
// writer waited for the connection's output lock behind a control write.
// This holds that lock past the whole allowance while a delivery waits
// behind it, and the delivery is still flushed when the lock goes.
//
// This is the obligation `native_internal_wait` names. The meter it names
// is fed by the live ordered writer; what is witnessed here is that the
// live path feeds it only with waiting on the recipient.

/// This connection's output lock, the one every writer of its socket takes,
/// reached through the home the registry keeps for it.
#[cfg(unix)]
fn output_lock_of(custody: &PrivateEvidenceCustody) -> Arc<Mutex<UnixStream>> {
    let home = Arc::clone(&custody.cleanup_record().ordered_home);
    let held = home
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let PrivateOrderedContinuation::Serving { owner, .. } = held
        .payload
        .as_ref()
        .expect("a serving connection has a continuation")
    else {
        panic!("a serving connection's continuation is its serving owner");
    };
    Arc::clone(&owner.output)
}

#[cfg(unix)]
#[test]
fn a_delivery_that_waited_behind_the_output_lock_past_the_allowance_is_still_flushed() {
    let mut service =
        LifecycleService::launch_with_capacity("internal-wait", 11703, None, false, 2);
    service.start();
    let mut reader = RealReader::open(&service, 0x0f41);
    reader.focus(&service, 117030);
    let keys = reader.ingress(&service, 1);
    let lease = service.owner.lease();
    let mut others = Vec::new();

    // THE LOCK IS HELD AS A CONTROL WRITE WOULD HOLD IT, from before the
    // delivery is accepted until past the whole allowance. The writer takes
    // the delivery from its queue, waits here, and accrues nothing: the
    // recipient was never asked for anything it did not take.
    let output = output_lock_of(&reader.custody);
    let held = output.lock().expect("the connection's output lock");
    let submitted = Instant::now();
    submit_within(&keys, &lease, key_service_route(reader.surface, 117031, 30, true), 3);
    let allowance = Duration::from_secs(6);
    assert!(
        receipt_of_delivery(&service.deliveries, 117031, allowance + Duration::from_millis(500), &mut others).is_none(),
        "nothing can be written while the lock is held, so no receipt comes"
    );
    assert!(
        read_event(&mut reader.peer, 1).is_none(),
        "and no bytes reached the wire behind the lock"
    );
    drop(held);

    // RELEASED, AND FLUSHED. Had the wait behind the lock counted toward the
    // allowance, this delivery would already be past it and would be
    // answered TimedOut with its socket ended.
    let receipt = receipt_of_delivery(&service.deliveries, 117031, Duration::from_secs(3), &mut others)
        .expect("the delivery is answered once the lock goes");
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::Flushed,
        "a wait behind the output lock is not the recipient's nonresponse: {receipt:?}"
    );
    assert!(
        submitted.elapsed() >= allowance,
        "the delivery really waited past the allowance: {:?}",
        submitted.elapsed()
    );
    // The event reports the pointer where the instance starts it, the
    // root's centre, in the root and in a window at the origin alike.
    let at_centre = |pressed| {
        let mut event = expected_key_service_event(reader.sequence, reader.window, 38, pressed, 0);
        event[20..28].copy_from_slice(&[128, 2, 104, 1, 128, 2, 104, 1]);
        event
    };
    assert_eq!(
        read_event(&mut reader.peer, 3),
        Some(at_centre(true)),
        "and the event reaches the recipient whole"
    );
    assert!(others.is_empty(), "no other receipt was published: {others:?}");

    // THE RELEASE FOLLOWS ON THE SAME CONNECTION, which is still served.
    submit_within(&keys, &lease, key_service_route(reader.surface, 117032, 30, false), 3);
    assert_eq!(read_event(&mut reader.peer, 3), Some(at_centre(false)));
    assert_eq!(
        receipt_of_delivery(&service.deliveries, 117032, Duration::from_secs(3), &mut others)
            .expect("settled")
            .outcome,
        XAuthorityInputDeliveryOutcome::Flushed
    );
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert!(closed.succeeded, "{:?}", closed.error);
}
