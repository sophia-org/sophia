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

#[test]
fn live_state_only_disposal_requires_its_explicit_source_disposition_and_actual_termination() {
    let mut fixture = state_only_fixture(9974);
    let surface = fixture.surface;
    state_only_execute(
        &mut fixture,
        None,
        key_service_route(surface, 997410, 42, true),
    );
    assert_eq!(
        fixture
            .runner
            .frontend
            .as_mut()
            .unwrap()
            .dispatch_one_press(),
        Some(true)
    );
    state_only_execute(
        &mut fixture,
        None,
        state_only_key_route(surface, 997411, 42),
    );
    let (socket, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let (mut writer, _output) = serving_owner_for(&mut fixture, socket);
    for expected in [4, 5, 2] {
        assert!((0..8).any(|_| matches!(
            writer.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Flushed
        )));
        let mut frame = [0; 32];
        peer.read_exact(&mut frame).unwrap();
        assert_eq!(frame[0], expected);
    }
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert!(matches!(
        private.settle_one_receipt(),
        Some(PrivateReceiptStep::Settled { .. })
    ));
    assert_eq!(private.record_one_native(), Some(true));
    for _ in 0..12 {
        private.terminal.dispose_live_native_one();
    }
    assert_eq!(private.terminal.settling.len(), 1);
    assert_eq!(
        private.terminal.settling[0].binding,
        PrivateReleaseBinding::RecipientTerminationRequired
    );
    assert!(private.terminal.settling[0].custody.completion.is_none());
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    assert!(matches!(
        private.settle_one_terminated_recipient(),
        PrivateReceiptStep::Settled { debt_settled: true }
    ));
    // STAGE ONLY: missing completion by itself is never a no-output receipt.
    private.terminal.settling[0].binding = PrivateReleaseBinding::Reached;
    for _ in 0..8 {
        assert!(!private.terminal.dispose_live_native_one());
    }
    private.terminal.settling[0].binding = PrivateReleaseBinding::RecipientTerminationRequired;
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
    assert!(private.terminal.settling.is_empty());
}

#[test]
fn live_recipient_termination_cannot_supply_the_withheld_native_half() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId(9975));
    attempt_run(&mut fixture, 99750, 272, true);
    attempt_run(&mut fixture, 99751, 273, true);
    attempt_run(&mut fixture, 99752, 272, false);
    let private = fixture.runner.frontend.as_mut().unwrap();
    for _ in 0..2 {
        assert_eq!(private.dispatch_one_press(), Some(true));
        flush_live_native_capsule(fixture.channels.ordered.try_recv().unwrap());
    }
    assert!(
        private.terminal.settling[0]
            .native()
            .unwrap()
            .proof()
            .is_none()
    );
    assert_eq!(private.record_one_native(), None);
    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut writer, _output) = serving_owner_for(&mut fixture, socket);
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert!(matches!(
        private.settle_one_terminated_recipient(),
        PrivateReceiptStep::Settled {
            debt_settled: false
        }
    ));
    let release = &private.terminal.settling[0];
    assert!(release.custody.recipient_termination && !release.native_recorded);
    assert!(
        private
            .controller
            .under_common_as_origin(|authority, issuer| authority
                .reconciliation_record_present(issuer, release.incarnation))
            .unwrap()
            .unwrap()
    );
    for _ in 0..8 {
        assert!(!private.terminal.dispose_live_native_one());
    }
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "the other source hold still owns native retirement"
    );
}
