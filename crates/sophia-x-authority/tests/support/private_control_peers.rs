use super::private_maintenance_scheduler::MaintainedService;
use super::*;

fn peer_window(
    service: &MaintainedService,
) -> (UnixStream, SurfaceId, Arc<PrivateControlClientSource>) {
    let mut client = connect_private_client(&service.path);
    let window = handshake_ids(&mut client) | 0x0f01;
    let (surface, _) = selecting_window(&mut client, &service.transactions, window, 1 << 21);
    let recipient = service
        .registry
        .surface_route_observation(surface)
        .unwrap()
        .unwrap()
        .client;
    let source = service
        .registry
        .client_senders(recipient)
        .unwrap()
        .connection_state
        .get()
        .unwrap()
        .control_source
        .get()
        .unwrap()
        .upgrade()
        .unwrap();
    (client, surface, source)
}

fn submit_focus(
    service: &MaintainedService,
    source: &PrivateControlClientSource,
    surface: SurfaceId,
    number: u64,
) {
    let owner = service.owner.clone();
    service
        .access
        .control_producer(&owner.lease())
        .unwrap()
        .submit(
            &owner.lease(),
            XAuthorityClientControlCommand {
                client: source.endpoint.client,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(number),
                    surface,
                },
            },
        )
        .unwrap();
}

#[test]
fn reversed_control_and_custody_order_eventually_visits_each_exact_recipient() {
    let service = MaintainedService::launch_distinct();
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let (first, first_surface, first_source) = peer_window(&service);
    let (second, second_surface, second_source) = peer_window(&service);
    let order: Vec<_> = service
        .owner
        .inventory
        .kept
        .lock()
        .unwrap()
        .places
        .iter()
        .filter_map(|place| place.as_ref().map(|place| place.cleanup_record().client))
        .collect();
    assert_eq!(
        order,
        vec![first_source.endpoint.client, second_source.endpoint.client]
    );
    let completion = service.registry.control_completion().unwrap();
    let owner = service.owner.clone();
    let producer = service.access.control_producer(&owner.lease()).unwrap();
    for (source, surface, count) in [
        (&second_source, second_surface, 1),
        (&first_source, first_surface, 2),
    ] {
        source.fail_after_effect.store(true, Ordering::Release);
        producer
            .submit(
                &owner.lease(),
                configure(source.endpoint.client, surface, 99900 + count),
            )
            .unwrap();
        assert!(waited_for(
            || completion.cleanups_owed().unwrap().len() == count as usize
        ));
    }
    let tokens: Vec<_> = completion
        .cleanups_owed()
        .unwrap()
        .into_iter()
        .map(|cleanup| cleanup.token)
        .collect();
    drop(first);
    assert!(waited_for(|| first_source
        .teardown
        .lock()
        .unwrap()
        .finished));
    drop(second);
    assert!(waited_for(|| second_source
        .teardown
        .lock()
        .unwrap()
        .finished));
    service
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    assert!(
        service
            .closed
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .2
    );
    for _ in 0..1600 {
        let _ = super::final_custody_step(&service);
    }
    for token in tokens {
        assert_eq!(completion.state_of(token), ControlRecordState::Retired);
    }
    assert_eq!(service.owner.store.reserved(), Some(0));
    assert!(service.acks.try_recv().is_err());
    service.finish();
}

fn focus_peer_completion(fail_peer_write: bool) {
    let service = MaintainedService::launch_distinct();
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let (mut first, first_surface, first_source) = peer_window(&service);
    let (mut second, second_surface, second_source) = peer_window(&service);
    submit_focus(&service, &first_source, first_surface, 99910);
    assert_eq!(
        service
            .acks
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(read_event(&mut first, 5).unwrap()[0] & 0x7f, 9);
    if fail_peer_write {
        // The original connection still sends requests, but its actual receive
        // half is shut down. The real peer writer must encounter EPIPE.
        first.shutdown(Shutdown::Read).unwrap();
    }
    submit_focus(&service, &second_source, second_surface, 99911);
    let ack = service.acks.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        ack.acknowledgement.transaction,
        TransactionId::from_raw(99911)
    );
    assert_eq!(
        ack.acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(read_event(&mut second, 5).unwrap()[0] & 0x7f, 9);
    let completion = service.registry.control_completion().unwrap();
    if fail_peer_write {
        assert!(waited_for(|| service
            .registry
            .client_senders(first_source.endpoint.client)
            .unwrap()
            .control_writer_gone
            .load(Ordering::Acquire)));
        let token = {
            let held = completion.inner.lock().unwrap();
            let record = held
                .records
                .iter()
                .find(|record| record.identity.transaction == TransactionId::from_raw(99911))
                .expect("origin success and peer quiescence cannot discard failed original output");
            assert!(matches!(record.phase, ControlPhase::Settled(_)));
            assert_eq!(record.dependents, 0);
            let source = record.source.as_ref().unwrap().lock().unwrap();
            assert!(source.peer_debt_pending());
            assert_eq!(source.focus_peers.len(), 1);
            assert!(Arc::ptr_eq(
                &source.focus_peers[0].recipient,
                &first_source.endpoint.registration
            ));
            assert!(!source.focus_peers[0].flushed);
            assert!(
                source
                    .dependent_records
                    .iter()
                    .any(|record| record[0] & 0x7f == 10)
            );
            record.token
        };
        drop(first);
        drop(second);
        assert!(waited_for(|| first_source
            .teardown
            .lock()
            .unwrap()
            .finished
            && second_source.teardown.lock().unwrap().finished));
        let _ = service
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect);
        assert!(
            service
                .closed
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .2
        );
        for _ in 0..900 {
            let _ = super::final_custody_step(&service);
        }
        assert_eq!(completion.state_of(token), ControlRecordState::Outstanding);
        assert!(service.owner.store.reserved().unwrap() > 0);
    } else {
        assert_eq!(read_event(&mut first, 5).unwrap()[0] & 0x7f, 10);
        assert!(
            waited_for(|| completion.outstanding() == Some(0)),
            "actual original peer flush retires its source debt"
        );
        assert!(
            waited_for(|| service.owner.store.reserved() == Some(0)),
            "ordinary success returns exactly its carried credits"
        );
        for round in 0..8 {
            let (target, surface) = if round % 2 == 0 {
                (&first_source, first_surface)
            } else {
                (&second_source, second_surface)
            };
            submit_focus(&service, target, surface, 99920 + round);
            assert_eq!(
                service
                    .acks
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .acknowledgement
                    .outcome,
                XAuthorityControlOutcome::Delivered
            );
            let first_type = read_event(&mut first, 5).unwrap()[0] & 0x7f;
            let second_type = read_event(&mut second, 5).unwrap()[0] & 0x7f;
            assert_eq!(
                (first_type, second_type),
                if round % 2 == 0 { (9, 10) } else { (10, 9) }
            );
            assert!(waited_for(
                || completion.outstanding() == Some(0) && service.owner.store.reserved() == Some(0)
            ));
        }
        drop(first);
        drop(second);
        let _ = service
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect);
        assert!(
            service
                .closed
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .2
        );
    }
    assert!(service.acks.try_recv().is_err());
    service.finish();
}

#[test]
fn origin_ack_and_failed_peer_write_keep_exact_source_payload_and_credit() {
    focus_peer_completion(true);
}

#[test]
fn origin_ack_and_original_peer_flush_retire_and_return_credit_once() {
    focus_peer_completion(false);
}
