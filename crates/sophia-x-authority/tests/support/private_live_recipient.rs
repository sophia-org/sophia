// Source controls: real admitted pointer requests and original writer capsules.
// The prepared fixture supplies native geometry; socket shutdown is real.

fn fail_live_recipient_capsule(capsule: XAuthorityOrderedDelivery) {
    let served = XAuthorityServedConnection::retained(capsule.endpoint().clone());
    let (sender, queue) = sync_channel(1);
    sender.send(capsule).unwrap();
    let (socket, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let mut in_flight = None;
    let mut refused = None;
    let mut failed = false;
    for _ in 0..8 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Ended { .. } => {
                failed = true;
                break;
            }
            X11OrderedServeStep::Advanced => {}
            other => panic!("expected failed original write: {other:?}"),
        }
    }
    assert!(failed);
}

#[test]
fn live_endpoint_termination_settles_failed_receipt_without_rewriting_it() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId(9971));
    attempt_release(&mut fixture, 99710, 272);
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    flush_live_native_capsule(fixture.channels.ordered.try_recv().unwrap());
    assert_eq!(private.record_one_native(), Some(true));
    assert_eq!(private.attempt_one_delivery(), Some(true));
    fail_live_recipient_capsule(fixture.channels.ordered.try_recv().unwrap());
    let release = &private.terminal.settling[0];
    let cell = release.completion().unwrap().clone();
    let endpoint = release.native().unwrap().endpoint().clone();
    let failed = cell.answer().unwrap();
    assert_eq!(failed.outcome, XAuthorityInputDeliveryOutcome::WriteFailed);
    assert!(
        !endpoint.ordered_termination(),
        "a receipt is not endpoint authority"
    );
    assert!(matches!(
        private.settle_one_receipt(),
        Some(PrivateReceiptStep::ReturnedUnsettled)
    ));
    assert!(!private.owes_terminated_recipient());
    assert!(!private.terminal.dispose_live_native_one());

    let (socket, mut peer) = UnixStream::pair().unwrap();
    let (mut writer, _output) = serving_owner_for(&mut fixture, socket);
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert!(endpoint.ordered_termination());
    assert!(matches!(
        private.settle_one_terminated_recipient(),
        PrivateReceiptStep::Settled { debt_settled: true }
    ));
    assert_eq!(
        cell.answer(),
        Some(failed),
        "the original failure is immutable"
    );
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
    assert!(private.terminal.settling.is_empty());
    assert!(
        fixture
            .ingress
            .submit(
                &fixture.keeper.lease(),
                button_to(
                    fixture.surface,
                    XAuthorityInputDeliveryId::from_raw(99712),
                    273,
                    true,
                )
            )
            .is_ok(),
        "recipient shutdown did not revoke the submitter"
    );
}

#[test]
fn live_termination_keeps_original_pending_capsule_until_its_authority_accepts() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId(9972));
    attempt_release(&mut fixture, 99720, 272);
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    flush_live_native_capsule(fixture.channels.ordered.try_recv().unwrap());
    assert_eq!(private.record_one_native(), Some(true));
    // A missing registry row refuses handover; it does not prove termination.
    let registry = private.broker.registry.clone();
    let row = registry
        .clients
        .lock()
        .unwrap()
        .remove(&fixture.client)
        .unwrap();
    assert_eq!(private.attempt_one_delivery(), Some(false));
    assert!(!private.owes_terminated_recipient());
    registry.clients.lock().unwrap().insert(fixture.client, row);
    let release = &private.terminal.settling[0];
    let cell = release.completion().unwrap().clone();
    let id = release.delivery().unwrap();
    let frames = match release.custody.pending.as_ref().unwrap() {
        PrivatePendingDelivery::Capsule(capsule) => order_pass_frames(capsule),
        _ => panic!("original capsule expected"),
    };
    // STAGE ONLY: withhold the original ticket so its finalizer must refuse.
    let ticket = registry
        .input_recovery
        .state
        .lock()
        .unwrap()
        .tickets
        .remove(&id)
        .unwrap();
    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut writer, _output) = serving_owner_for(&mut fixture, socket);
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert!(matches!(
        private.settle_one_terminated_recipient(),
        PrivateReceiptStep::Settled { debt_settled: true }
    ));
    let release = &private.terminal.settling[0];
    assert!(Arc::ptr_eq(release.completion().unwrap(), &cell));
    assert!(cell.answer().is_none());
    assert!(
        matches!(release.custody.pending.as_ref(), Some(PrivatePendingDelivery::Capsule(c)) if order_pass_frames(c) == frames)
    );
    assert!(!private.terminal.dispose_live_native_one());
    registry
        .input_recovery
        .state
        .lock()
        .unwrap()
        .tickets
        .insert(id, ticket);
    let _ = private.settle_one_terminated_recipient();
    assert_eq!(
        cell.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected
    );
    assert!(private.terminal.settling[0].custody.pending.is_none());
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
}

#[test]
fn live_termination_visits_charge_once_and_leave_other_native_classes_a_turn() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId(9973));
    attempt_release(&mut fixture, 99730, 272);
    attempt_release(&mut fixture, 99732, 273);
    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut writer, _output) = serving_owner_for(&mut fixture, socket);
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    let private = fixture.runner.frontend.as_mut().unwrap();
    // STAGE ONLY: first cursor position cannot supply native provenance.
    let native = private.terminal.settling[0].native.take().unwrap();
    private.terminal.recipient_termination_turn = 2;
    let mut charged = 0;
    let step = private
        .deliver_one(&mut |_, _| {
            charged += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(charged, 1);
    assert!(matches!(
        step,
        PrivateDeliveryStep::Receipt {
            step: PrivateReceiptStep::Unanswered
        }
    ));
    private.terminal.settling[0].native = Some(native);
    let mut other_class = false;
    for _ in 0..12 {
        let mut charged = 0;
        let step = private
            .deliver_one(&mut |_, _| {
                charged += 1;
                Ok(())
            })
            .unwrap();
        assert!(charged <= 1);
        other_class |= matches!(step, PrivateDeliveryStep::Recorded { recorded: true });
    }
    assert!(
        other_class,
        "recipient visits did not monopolize the native budget"
    );
}
