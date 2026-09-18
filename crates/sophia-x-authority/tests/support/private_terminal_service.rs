use private_maintenance_scheduler::{Exit, MaintainedService};

fn stopped_native_service(key: bool, withhold_custody: bool) {
    let service = MaintainedService::launch(Exit::Stop);
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&service.path);
    let window = handshake_ids(&mut client) | 0x0c01;
    let (surface, sequence) = selecting_window(
        &mut client,
        &service.transactions,
        window,
        3 | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 21),
    );
    let custody = wait_attached(&service.registry);
    let client_id = custody.cleanup_record().client;
    let owner = service.owner.clone();
    let lease = owner.lease();
    let control = service.access.control_producer(&lease).unwrap();
    let ingress = service
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(99001),
                    surface,
                },
            },
        )
        .unwrap();
    assert_eq!(
        ack_for(&service.acks, 99001).map(|ack| ack.acknowledgement.outcome),
        Some(XAuthorityControlOutcome::Delivered)
    );
    assert_eq!(
        read_event(&mut client, 5),
        Some(expected_focus_in(sequence, window))
    );
    if key {
        // Establish the key's pointer geometry through a real motion source.
        // No fixture writes a query projection or replacement XKB history.
        ingress
            .submit(
                &lease,
                motion_to(surface, XAuthorityInputDeliveryId::from_raw(99000)),
            )
            .unwrap();
        let mut motion = expected_button_event(true, sequence, window, 1);
        motion[0] = 6;
        motion[1] = 0;
        assert_eq!(read_event(&mut client, 5), Some(motion));
    }
    let route = if key {
        key_service_route(surface, 99002, 42, true)
    } else {
        button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(99002),
            272,
            true,
        )
    };
    ingress.submit(&lease, route).unwrap();
    let expected = if key {
        expected_key_service_event(sequence, window, 50, true, 0)
    } else {
        expected_button_event(true, sequence, window, 1)
    };
    assert_eq!(read_event(&mut client, 5), Some(expected));
    let press = delivery_cell(&service.registry, 99002).unwrap();
    assert!(waited_for(|| press.answer().is_some()));
    assert_eq!(
        press.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::Flushed
    );
    let _observed_press = service.deliveries.try_iter().collect::<Vec<_>>();
    service
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    assert_eq!(
        service.closed.recv_timeout(Duration::from_secs(5)).unwrap(),
        (false, true, true)
    );
    assert!(eof_within(&mut client, 3));
    let debt = service
        .controller
        .under_common(|authority| authority.next_debt(&mut 0))
        .unwrap()
        .unwrap();
    assert!(!debt.1.native_reconciled && !debt.1.recipient_settled);
    // Withhold the exact externally owned evidence, retaining the same Arc
    // locally. This is a labelled source-evidence control, not a new receipt.
    let withheld = withhold_custody.then(|| {
        let mut kept = owner.inventory.kept.lock().unwrap();
        let index = kept
            .places
            .iter()
            .position(|place| {
                place
                    .as_ref()
                    .is_some_and(|other| Arc::ptr_eq(other, &custody))
            })
            .unwrap();
        (index, kept.places[index].take().unwrap())
    });
    let mut observed_native = false;
    let mut withheld_native_steps = 0;
    let mut disposed = false;
    for _ in 0..400 {
        let step = service.step();
        if step.status == PrivateMaintenanceStatus::Yielded {
            std::thread::sleep(Duration::from_millis(17));
        }
        assert!(
            !matches!(
                step.status,
                PrivateMaintenanceStatus::SupervisionFailed
                    | PrivateMaintenanceStatus::AccountingFailed
            ),
            "{step:?}"
        );
        let native_clear = if key {
            step.modifiers == Some(0)
        } else {
            service
                .registry
                .pointer_state
                .lock()
                .unwrap()
                .values()
                .all(|mapper| mapper.state() & 0x100 == 0)
        };
        if native_clear {
            observed_native = true;
            if withhold_custody {
                let debt = service
                    .controller
                    .under_common(|authority| authority.next_debt(&mut 0))
                    .unwrap()
                    .unwrap();
                if debt.1.native_reconciled {
                    assert!(
                        !debt.1.recipient_settled,
                        "press Flushed is not release settlement; withheld source termination stays owed"
                    );
                    assert_eq!(step.native_records, Some(1));
                    withheld_native_steps += 1;
                    if withheld_native_steps == 40 {
                        break;
                    }
                }
            } else if step.native_records == Some(0) {
                disposed = true;
                break;
            }
        }
    }
    assert!(
        observed_native,
        "the original same-thread history reconciled"
    );
    if withhold_custody {
        assert_eq!(
            withheld_native_steps, 40,
            "several complete maintenance rounds retained the unanswered recipient"
        );
    }
    if let Some((index, exact)) = withheld {
        owner.inventory.kept.lock().unwrap().places[index] = Some(exact);
        for _ in 0..400 {
            let step = service.step();
            if step.status == PrivateMaintenanceStatus::Yielded {
                std::thread::sleep(Duration::from_millis(17));
            }
            if step.native_records == Some(0) {
                disposed = true;
                break;
            }
        }
    }
    assert!(
        disposed,
        "exact native record is eventually disposed after source termination evidence returns"
    );
    assert!(
        service
            .controller
            .under_common(|authority| authority.next_debt(&mut 0).is_none())
            .unwrap()
    );
    assert_eq!(
        press.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::Flushed,
        "cleanup never fabricates or rewrites a writer receipt"
    );
    service.finish();
}

#[test]
fn stopped_service_held_pointer_reconciles_and_disposes_exact_native_record() {
    stopped_native_service(false, false);
}

#[test]
fn stopped_service_held_modifier_waits_for_exact_termination_then_disposes() {
    stopped_native_service(true, true);
}
