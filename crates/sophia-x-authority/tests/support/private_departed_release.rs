// The release a departing source owes: what its custody says before anything
// runs, and that a suppressed release says the opposite on its own ground.

/// The admitted delivery of a capsule, for controls that only ever assemble
/// admitted capsules. Production reads `admitted_delivery`, which does not
/// presume; this presumes, and says so when it is wrong.
#[cfg(unix)]
impl XAuthorityOrderedDelivery {
    pub(crate) fn delivery(&self) -> XAuthorityInputDeliveryId {
        self.admitted_delivery()
            .expect("this control assembled an admitted capsule")
    }
}

/// A custody made for an event nobody admitted owes that event until it is
/// enqueued; one made with neither cell owes nothing.
#[test]
fn an_unadmitted_custody_owes_an_event_and_a_suppressed_one_does_not() {
    let suppressed = PrivateDeliveryCustody::new(1, None);
    assert!(!suppressed.owes_event());
    assert!(!suppressed.handover_unfinished());
    assert!(!suppressed.owes_handover());
    assert!(suppressed.writer_outcome().is_none());

    let mut owed = PrivateDeliveryCustody::unadmitted(1);
    assert!(owed.owes_event());
    assert!(owed.handover_unfinished() && owed.owes_handover());
    assert!(owed.handover_permitted());
    assert!(owed.completion.is_none() && owed.unadmitted.is_some());
    assert!(owed.writer_outcome().is_none());

    // ENQUEUED ENDS THE HANDOVER, as for a requested event.
    owed.dispatch = PrivateDispatchPhase::Enqueued;
    assert!(!owed.handover_unfinished());
    assert!(!owes_after_termination(PrivateDeliveryCustody::unadmitted(2)));

    // AN INDETERMINATE HANDOVER IS UNFINISHED BUT NOT REPEATABLE: it stays in
    // the ordering comparison and is not offered again.
    let mut begun = PrivateDeliveryCustody::unadmitted(3);
    begun.dispatch = PrivateDispatchPhase::Indeterminate;
    assert!(begun.handover_unfinished() && !begun.handover_permitted());

    // THE ADMITTED SHAPE ANSWERS THE SAME QUESTIONS THE SAME WAY, so the
    // ordering sites cannot tell the two apart, which is the point.
    let admitted = PrivateDeliveryCustody::new(4, Some(Arc::default()));
    assert!(admitted.owes_event() && admitted.handover_unfinished());
}

fn owes_after_termination(mut custody: PrivateDeliveryCustody) -> bool {
    custody.recipient_termination = true;
    custody.handover_unfinished()
}

/// The writer answers an unadmitted custody through its own cell, once, and
/// the custody reads it back the way it reads an admitted answer.
#[test]
fn an_unadmitted_custody_is_answered_through_its_own_cell_once() {
    let (deliveries, _receipts) = std::sync::mpsc::channel();
    let recovery = InputRecovery::new(4, Some(deliveries), Arc::default());
    let client = XServerFrontendClientId::from_raw(7);
    let custody = PrivateDeliveryCustody::unadmitted(1);
    let finalizer = custody
        .finalizer(&recovery, None, client)
        .expect("an unadmitted custody has something to answer for it");
    assert_eq!(
        finalizer.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Answered
    );
    assert_eq!(
        custody.writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    // A second answer for one write is a contradiction; the first stands.
    assert_eq!(
        finalizer.finalize(XAuthorityInputDeliveryOutcome::WriteFailed),
        PrivateAdjudication::AlreadyAnswered
    );
    assert_eq!(
        custody.writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    // Settled only once nothing is pending and no attempt is out.
    assert!(custody.writer_settled());

    // A suppressed custody has nothing to answer through.
    let suppressed = PrivateDeliveryCustody::new(2, None);
    assert!(suppressed.finalizer(&recovery, None, client).is_none());
    // An admitted custody without its delivery has nothing either: the
    // finalizer would name an admission it cannot find.
    let admitted = PrivateDeliveryCustody::new(3, Some(Arc::default()));
    assert!(admitted.finalizer(&recovery, None, client).is_none());
}

/// Drive a key capsule through a real writer to a socket and read the event
/// back, asserting its kind and X keycode.
fn flush_key_capsule(capsule: XAuthorityOrderedDelivery, kind: u8, detail: u8) {
    let served = XAuthorityServedConnection::retained(capsule.endpoint().clone());
    let (sender, queue) = sync_channel(1);
    sender.send(capsule).unwrap();
    let (socket, mut recipient) = UnixStream::pair().unwrap();
    let mut in_flight = None;
    let mut refused = None;
    let mut flushed = false;
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
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            X11OrderedServeStep::Advanced => {}
            other => panic!("the key capsule did not flush: {other:?}"),
        }
    }
    assert!(flushed, "a capsule nobody admitted is still answered and retired");
    let mut bytes = [0; 32];
    recipient.read_exact(&mut bytes).unwrap();
    assert_eq!((bytes[0], bytes[1]), (kind, detail), "the event the recipient read");
}

/// A fixture with its pointer pair fully delivered and disposed, so the
/// ledger holds no other record whose holders reached zero and the departed
/// visit's cursor finds exactly the key.
fn departed_fixture(client: u64) -> PreparedOrderedFixture {
    let mut f = state_only_fixture(client);
    flush_live_native_capsule(f.channels.ordered.try_recv().unwrap());
    flush_live_native_capsule(f.channels.ordered.try_recv().unwrap());
    let private = f.runner.frontend.as_mut().unwrap();
    assert!(matches!(
        private.settle_one_receipt(),
        Some(PrivateReceiptStep::Settled { debt_settled: true })
    ));
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
    assert!(private.terminal.settling.is_empty() && private.terminal.holds.is_empty());
    f
}

/// Press key 42 (X keycode 50) from the fixture's own source and hand the
/// press to the recipient, leaving the hold in place.
fn press_and_deliver(f: &mut PreparedOrderedFixture, id: u64) {
    let surface = f.surface;
    let press = state_only_execute(f, None, key_service_route(surface, id, 42, true));
    assert!(press.first_press && press.keyboard_applied);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    flush_key_capsule(f.channels.ordered.try_recv().unwrap(), 2, 50);
    assert_eq!(f.runner.frontend().terminal.holds.len(), 1);
}

/// Revoke the grant behind the one hold, as a departing connection does.
fn revoke_the_holders_grant(f: &mut PreparedOrderedFixture) -> sophia_input_authority::RetiredDebt {
    let private = f.runner.frontend.as_mut().unwrap();
    let grant = private.terminal.holds[0].native.as_ref().unwrap().grant();
    private
        .authority()
        .under_common_as_origin(|authority, issuer| authority.revoke_grant(issuer, grant))
        .unwrap()
        .unwrap()
}

/// Drive delivery turns, keyboards lent, until `accept` says the step is the
/// one waited for; bounded, so a step that never comes fails by name.
fn drive_until(
    f: &mut PreparedOrderedFixture,
    turns: usize,
    goal: &str,
    mut accept: impl FnMut(&PrivateDeliveryStep) -> bool,
) -> PrivateDeliveryStep {
    let PrivatePreparedRunner {
        frontend, keyboards, ..
    } = &mut f.runner;
    let private = frontend.as_mut().unwrap();
    for _ in 0..turns {
        let step = private
            .deliver_one(Some(keyboards), &mut |_, _| Ok(()))
            .unwrap();
        if accept(&step) {
            return step;
        }
    }
    panic!("{goal} did not happen within {turns} delivery turns");
}

/// The whole path: a source departs holding a key, the terminal builds the
/// release it owes, the recipient's writer answers into the custody's own
/// cell, the ledger settles, and the record is disposed.
#[test]
fn a_departed_sources_release_reaches_its_recipient_and_settles_through_its_own_cell() {
    let mut f = departed_fixture(9791);
    press_and_deliver(&mut f, 979110);
    let incarnation = f.runner.frontend().terminal.holds[0].incarnation;
    let debt = revoke_the_holders_grant(&mut f);
    assert_eq!((debt.owed_releases, debt.survivors), (1, 0));

    // THE RELEASE IS BUILT ON THE VISIT'S TURN, with a cell of its own and no
    // delivery identity, and it is already the recipient's output head.
    drive_until(&mut f, 16, "the departed release", |step| {
        matches!(step, PrivateDeliveryStep::DepartedRelease { released: true })
    });
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Released, 0),
        "the keyboard was released under the reconciliation permit"
    );
    {
        let private = f.runner.frontend();
        assert!(private.terminal.holds.is_empty());
        let release = &private.terminal.settling[0];
        assert_eq!(release.incarnation, incarnation);
        assert_eq!(release.binding, PrivateReleaseBinding::Reached);
        assert!(release.delivery.is_none() && release.custody.completion.is_none());
        assert!(release.custody.unadmitted.is_some());
        assert!(matches!(release.event, Some(XAuthorityInputEvent::Key(_))));
        assert!(release.custody_handover_unfinished());
        assert!(matches!(
            private.output_head(f.client),
            Some((_, PrivateOutputSite::Release(0)))
        ));
    }

    // ENQUEUED THROUGH THE ORDINARY ATTEMPT, once its native half is recorded.
    drive_until(&mut f, 32, "the release being enqueued", |step| {
        matches!(step, PrivateDeliveryStep::Dispatched { enqueued: true, .. })
    });
    let capsule = f.channels.ordered.try_recv().unwrap();
    assert!(capsule.admitted_delivery().is_none(), "nobody admitted it");
    assert!(capsule.finalizer().is_some(), "and its writer can still answer");
    let frame = &order_pass_frames(&capsule)[0];
    assert_eq!((frame[0], frame[1]), (3, 50), "a bare KeyRelease of the key held");
    {
        let release = &f.runner.frontend().terminal.settling[0];
        assert_eq!(release.custody.dispatch, PrivateDispatchPhase::Enqueued);
        assert!(release.custody.attempt.is_some());
    }

    // ANSWERED INTO ITS OWN CELL by a real writer, then settled and disposed
    // exactly as a requested release is.
    flush_key_capsule(capsule, 3, 50);
    assert_eq!(
        f.runner.frontend().terminal.settling[0]
            .custody
            .writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    drive_until(&mut f, 16, "the receipt settling the debt", |step| {
        matches!(
            step,
            PrivateDeliveryStep::Receipt {
                step: PrivateReceiptStep::Settled { debt_settled: true }
            }
        )
    });
    drive_until(&mut f, 16, "the record's disposal", |step| {
        matches!(step, PrivateDeliveryStep::NativeDisposal { disposed: true })
    });
    let private = f.runner.frontend();
    assert!(private.terminal.settling.is_empty());
    assert!(
        private
            .authority()
            .under_common(|authority| authority.next_debt(&mut 0).is_none())
            .unwrap(),
        "no debt remains in the ledger"
    );
}

/// The recipient is gone by the time the bytes go: the write fails, the
/// answer is recorded once and never rewritten, and the endpoint's own
/// termination settles what the write could not.
#[test]
fn a_departed_sources_release_to_a_gone_recipient_finishes_after_a_failed_write() {
    let mut f = departed_fixture(9792);
    press_and_deliver(&mut f, 979210);
    assert_eq!(revoke_the_holders_grant(&mut f).owed_releases, 1);
    drive_until(&mut f, 16, "the departed release", |step| {
        matches!(step, PrivateDeliveryStep::DepartedRelease { released: true })
    });
    drive_until(&mut f, 32, "the release being enqueued", |step| {
        matches!(step, PrivateDeliveryStep::Dispatched { enqueued: true, .. })
    });
    fail_live_recipient_capsule(f.channels.ordered.try_recv().unwrap());
    let endpoint = f.runner.frontend().terminal.settling[0]
        .native()
        .unwrap()
        .endpoint()
        .clone();
    assert_eq!(
        f.runner.frontend().terminal.settling[0]
            .custody
            .writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed)
    );
    // A FAILED WRITE IS NOT PROOF EITHER WAY, so the debt stays owed and the
    // capsule is never sent again -- exactly as for a requested release.
    drive_until(&mut f, 16, "the failed receipt returning", |step| {
        matches!(
            step,
            PrivateDeliveryStep::Receipt {
                step: PrivateReceiptStep::ReturnedUnsettled
            }
        )
    });
    {
        let release = &f.runner.frontend().terminal.settling[0];
        assert_eq!(release.custody.dispatch, PrivateDispatchPhase::Unrepeatable);
        assert!(!release.owes_delivery_attempt());
    }
    // THE ENDPOINT'S TERMINATION IS THE RECIPIENT FACT, established by the
    // recipient's own writer closing, not by the receipt.
    let (socket, mut peer) = UnixStream::pair().unwrap();
    let (mut writer, _output) = serving_owner_for(&mut f, socket);
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    assert!(endpoint.ordered_termination());
    let private = f.runner.frontend.as_mut().unwrap();
    assert!(matches!(
        private.settle_one_terminated_recipient(),
        PrivateReceiptStep::Settled { debt_settled: true }
    ));
    let release = &private.terminal.settling[0];
    assert!(release.custody.recipient_termination);
    assert_eq!(
        release.custody.writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "the failed answer stands; termination is recorded beside it"
    );
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
    assert!(private.terminal.settling.is_empty());
}

/// The recipient departs before the release is ever enqueued: its endpoint's
/// termination answers the release ClientDisconnected through the custody's
/// own cell, and the record is disposed without a write.
#[test]
fn a_departed_sources_release_to_a_gone_recipient_finishes_when_never_enqueued() {
    let mut f = departed_fixture(9793);
    press_and_deliver(&mut f, 979310);
    assert_eq!(revoke_the_holders_grant(&mut f).owed_releases, 1);
    drive_until(&mut f, 16, "the departed release", |step| {
        matches!(step, PrivateDeliveryStep::DepartedRelease { released: true })
    });
    let endpoint = f.runner.frontend().terminal.settling[0]
        .native()
        .unwrap()
        .endpoint()
        .clone();
    let (socket, mut peer) = UnixStream::pair().unwrap();
    let (mut writer, _output) = serving_owner_for(&mut f, socket);
    writer
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    assert!(endpoint.ordered_termination());
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.record_one_native(), Some(true));
    assert!(matches!(
        private.settle_one_terminated_recipient(),
        PrivateReceiptStep::Settled { debt_settled: true }
    ));
    let release = &private.terminal.settling[0];
    assert!(release.custody.recipient_termination);
    assert_eq!(release.custody.dispatch, PrivateDispatchPhase::Unrepeatable);
    assert_eq!(
        release.custody.writer_outcome(),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected)
    );
    assert!(!release.owes_delivery_attempt());
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
    assert!(private.terminal.settling.is_empty());
}

/// A source that departs while another still holds the key owes nothing: the
/// visit finds no release to build, the survivor's own release ends the
/// aggregate once, and that release is an ordinary admitted one.
#[test]
fn a_survivor_of_a_departed_source_releases_once_and_as_itself() {
    use sophia_input_authority::ReleaseOutcome;
    let mut f = departed_fixture(9794);
    let surface = f.surface;
    let secondary = f
        .runner
        .ingress_for(&f.keeper.lease(), f.client, DeviceId::from_raw(2))
        .unwrap();
    press_and_deliver(&mut f, 979410);
    let joined = state_only_execute(
        &mut f,
        Some(&secondary),
        key_service_route(surface, 979411, 42, true),
    );
    assert!(!joined.first_press && !joined.keyboard_applied);
    let debt = revoke_the_holders_grant(&mut f);
    assert_eq!((debt.owed_releases, debt.survivors), (0, 1));
    let PrivatePreparedRunner {
        frontend, keyboards, ..
    } = &mut f.runner;
    let private = frontend.as_mut().unwrap();
    for _ in 0..16 {
        let step = private
            .deliver_one(Some(keyboards), &mut |_, _| Ok(()))
            .unwrap();
        assert!(
            !matches!(step, PrivateDeliveryStep::DepartedRelease { released: true }),
            "a survivor's hold is not a departed source's debt"
        );
    }
    assert_eq!(private.terminal.holds.len(), 1);
    assert!(f.channels.ordered.try_recv().is_err(), "nothing was sent for the departure");
    let released = state_only_execute(
        &mut f,
        Some(&secondary),
        key_service_route(surface, 979412, 42, false),
    );
    assert!(matches!(released.release, Some(ReleaseOutcome::DeliverTo(_))));
    let release = &f.runner.frontend().terminal.settling[0];
    assert!(release.custody.completion.is_some() && release.custody.unadmitted.is_none());
    assert_eq!(release.binding, PrivateReleaseBinding::Reached);
}
