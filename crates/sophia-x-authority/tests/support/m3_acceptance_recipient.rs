// Integrated recipient-loss alternative. Both connections, the leased
// producer, queue, native execution and writers are the production service's.
// The only interruption pauses publication of an actual socket shutdown.
static RECIPIENT_END_PAUSES: Mutex<Vec<(PrivateEndpointIdentity, Pause)>> = Mutex::new(Vec::new());

pub(crate) fn before_recipient_termination(endpoint: &PrivateEndpointIdentity) {
    let pause = {
        let mut pending = RECIPIENT_END_PAUSES.lock().unwrap();
        pending
            .iter()
            .position(|(original, _)| original.matches(endpoint))
            .map(|at| pending.remove(at).1)
    };
    if let Some(pause) = pause {
        pause.wait();
    }
}

#[derive(Debug)]
struct RecipientDebt {
    native: bool,
    recipient: bool,
    ended: bool,
    present: bool,
    dispatch: PrivateDispatchPhase,
}

fn recipient_debt(service: &LifecycleService, id: u64) -> Option<RecipientDebt> {
    let (sent, received) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            let frontend = runner.frontend();
            let state = frontend
                .terminal
                .settling
                .iter()
                .find(|release| release.delivery() == Some(XAuthorityInputDeliveryId::from_raw(id)))
                .map(|release| RecipientDebt {
                    native: release.native_recorded,
                    recipient: release.custody.recipient_termination,
                    ended: release.native().unwrap().endpoint().ordered_termination(),
                    present: frontend
                        .controller
                        .under_common_as_origin(|authority, issuer| {
                            authority.reconciliation_record_present(issuer, release.incarnation)
                        })
                        .unwrap()
                        .unwrap(),
                    dispatch: release.dispatch(),
                });
            sent.send(state).unwrap();
        }),
    );
    received
        .recv_timeout(Duration::from_secs(2))
        .expect("actual runner observed")
}

#[test]
fn a_recipient_disconnect() {
    let mut service =
        LifecycleService::launch_with_capacity("recipient-only-end", 11700, None, false, 2);
    service.start();
    let (mut recipient, recipient_custody) = service.connect();
    let (surface, sequence, _unused) = focus_window(&service, &mut recipient, 0x310101, 11700);
    let (mut submitter, submitter_custody) = service.connect();
    let submitting = submitter_custody.cleanup_record().client;
    let receiving = recipient_custody.cleanup_record().client;
    assert_ne!(submitting, receiving);
    let ingress = service
        .access
        .ingress_for(&service.owner.lease(), submitting, DeviceId::from_raw(2))
        .unwrap();
    ingress
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11701),
                272,
                true,
            ),
        )
        .unwrap();
    assert_eq!(
        read_event(&mut recipient, 3),
        Some(expected_button_event(true, sequence, 0x310101, 1))
    );
    let press = service
        .deliveries
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        (press.client, press.delivery.raw(), press.outcome),
        (receiving, 11701, XAuthorityInputDeliveryOutcome::Flushed)
    );

    // Hold the original writer before submitting the release. Native work
    // continues on the service thread while the recipient half is withheld.
    let home = recipient_custody.cleanup_record().ordered_home.clone();
    let held = home.state.lock().unwrap();
    ingress
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11702),
                272,
                false,
            ),
        )
        .unwrap();
    assert!(waited_for(|| recipient_debt(&service, 11702).is_some_and(
        |debt| debt.native && debt.present && debt.dispatch == PrivateDispatchPhase::Enqueued
    )));
    let cell = delivery_cell(&service.registry, 11702).unwrap();
    assert!(cell.answer().is_none());
    // STAGE ONLY: temporarily withhold the routing row, with its original
    // value restored before the writer can proceed. Absence supplies no bit.
    let row = service
        .registry
        .clients
        .lock()
        .unwrap()
        .remove(&receiving)
        .unwrap();
    let missing = recipient_debt(&service, 11702).unwrap();
    service
        .registry
        .clients
        .lock()
        .unwrap()
        .insert(receiving, row);
    assert!(missing.native && missing.present && !missing.recipient && !missing.ended);
    let (sent, received) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            let endpoint = runner
                .frontend()
                .terminal
                .settling
                .iter()
                .find(|release| {
                    release.delivery() == Some(XAuthorityInputDeliveryId::from_raw(11702))
                })
                .unwrap()
                .native()
                .unwrap()
                .endpoint()
                .clone();
            sent.send(endpoint).unwrap();
        }),
    );
    let endpoint = received.recv_timeout(Duration::from_secs(2)).unwrap();
    let (pause, release) = Pause::pair();
    RECIPIENT_END_PAUSES
        .lock()
        .unwrap()
        .push((endpoint.clone(), pause));
    recipient.shutdown(Shutdown::Read).unwrap();
    drop(held);
    let ended_on = release.entered();
    let failed = cell
        .answer()
        .expect("actual failed write answered before shutdown publication");
    assert_eq!(failed.outcome, XAuthorityInputDeliveryOutcome::WriteFailed);
    let before = recipient_debt(&service, 11702).unwrap();
    assert!(before.native && before.present && !before.recipient && !before.ended);
    release.release();
    assert!(waited_for(|| endpoint.ordered_termination()));
    assert!(waited_for(
        || recipient_debt(&service, 11702).is_none_or(|debt| debt.recipient && !debt.present)
    ));
    assert_eq!(cell.answer(), Some(failed));
    assert_eq!(service.access.standing(), PrivatePortStanding::Ready);
    submitter.write_all(&[43, 0, 1, 0]).unwrap();
    submitter
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reply = [0; 32];
    submitter.read_exact(&mut reply).unwrap();
    assert_eq!(
        reply[0], 1,
        "the distinct submitting socket still answers requests"
    );
    ingress
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11703),
                272,
                false,
            ),
        )
        .unwrap();
    let live = json!({"submitting":submitting.0,"recipient":receiving.0,"reply":reply.to_vec(),"port":"Ready","accepted_after_recipient_end":11703});
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert!(closed.succeeded, "{:?}", closed.error);
    let actors = service.finish(&[recipient_custody, submitter_custody]);
    emit_case(
        "A.recipient_disconnect",
        &[
            ("submitter_remains_live", live),
            (
                "exact_recipient_termination",
                json!({"endpoint":format!("{endpoint:?}"),"thread":format!("{ended_on:?}"),"original_receipt":format!("{failed:?}")}),
            ),
            ("missing_route_is_not_proof", json!(format!("{missing:?}"))),
            ("failed_send_is_not_proof", json!(format!("{before:?}"))),
        ],
        &actors,
    );
}
