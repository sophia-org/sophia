use super::private_control_peers::{peer_window, submit_focus};
use super::private_maintenance_scheduler::MaintainedService;
use super::*;
use std::io::Write;

fn subscribe(client: &mut UnixStream, window: u32, mask: u32) {
    let mut request = vec![2, 0, 4, 0];
    request.extend(window.to_le_bytes());
    request.extend((1u32 << 11).to_le_bytes());
    request.extend(mask.to_le_bytes());
    request.extend([43, 0, 1, 0]); // GetInputFocus orders the selection request.
    client.write_all(&request).unwrap();
    assert_eq!(read_event(client, 5).unwrap()[0], 1);
}

fn finish_clients(
    service: &MaintainedService,
    clients: [UnixStream; 2],
    sources: [&PrivateControlClientSource; 2],
) {
    drop(clients);
    assert!(waited_for(|| sources.iter().all(|source| source
        .teardown
        .lock()
        .unwrap()
        .finished)));
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
}

fn generic_peer(fail: bool, withhold: bool, replace: bool) {
    let service = MaintainedService::launch_distinct();
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let (first, surface, source) = peer_window(&service);
    let (mut second, _, peer) = peer_window(&service);
    let window = source.tables.windows.lock().unwrap()[&surface];
    if !fail {
        let event_id = peer
            .tables
            .windows
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .local
            .raw() as u32
            + 9;
        let mut select = vec![
            crate::X_PRESENT_MAJOR_OPCODE,
            crate::X_PRESENT_SELECT_INPUT_MINOR_OPCODE,
            4,
            0,
        ];
        select.extend(event_id.to_le_bytes());
        select.extend((window.local.raw() as u32).to_le_bytes());
        select.extend(1u32.to_le_bytes());
        second.write_all(&select).unwrap();
    }
    subscribe(
        &mut second,
        window.local.raw() as u32,
        (1 << 17) | (1 << 22),
    );
    let completion = service.registry.control_completion().unwrap();
    let producer = service
        .access
        .control_producer(&service.owner.lease())
        .unwrap();
    if fail {
        second.shutdown(Shutdown::Read).unwrap();
    }
    let rounds = if fail { 1 } else { 12 };
    let mut last = None;
    for round in 0..rounds {
        let mut command = configure(source.endpoint.client, surface, 99950 + round);
        if let XAuthorityControlCommand::ConfigureSurface { geometry, .. } = &mut command.command {
            geometry.width += round as i32;
        }
        producer.submit(&service.owner.lease(), command).unwrap();
        assert_eq!(
            service
                .acks
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .acknowledgement
                .outcome,
            XAuthorityControlOutcome::Delivered
        );
        if !fail {
            let present = read_within(&mut second, 40, 5).unwrap();
            assert_eq!(
                (present[0], present[1]),
                (35, crate::X_PRESENT_MAJOR_OPCODE)
            );
            assert_eq!(read_event(&mut second, 5).unwrap()[0] & 0x7f, 22);
            assert!(waited_for(
                || completion.outstanding() == Some(0) && service.owner.store.reserved() == Some(0)
            ));
            producer
                .submit(
                    &service.owner.lease(),
                    XAuthorityClientControlCommand {
                        client: source.endpoint.client,
                        command: XAuthorityControlCommand::SetPresentationState {
                            transaction: TransactionId::from_raw(99970 + round),
                            surface,
                            state: sophia_protocol::PolicyPresentationState {
                                fullscreen: round % 2 == 0,
                                maximized: false,
                                minimized: false,
                            },
                        },
                    },
                )
                .unwrap();
            assert_eq!(
                service
                    .acks
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .acknowledgement
                    .outcome,
                XAuthorityControlOutcome::Delivered
            );
            assert_eq!(read_event(&mut second, 5).unwrap()[0] & 0x7f, 28);
            if round == 0 {
                // The first state creates both _NET_WM_STATE and WM_STATE;
                // later toggles change only _NET_WM_STATE.
                assert_eq!(read_event(&mut second, 5).unwrap()[0] & 0x7f, 28);
            }
            assert!(waited_for(
                || completion.outstanding() == Some(0) && service.owner.store.reserved() == Some(0)
            ));
        } else {
            assert!(waited_for(|| completion
                .inner
                .lock()
                .unwrap()
                .records
                .iter()
                .any(|record| {
                    record.dependents == 0
                        && record.source.as_ref().is_some_and(|source| {
                            let operation = source.lock().unwrap();
                            operation
                                .protocol_receipts
                                .iter()
                                .any(|receipt| receipt.wire_record.lock().unwrap().is_some())
                        })
                })));
            let held = completion.inner.lock().unwrap();
            let record = held
                .records
                .iter()
                .find(|record| record.identity.transaction == TransactionId::from_raw(99950))
                .unwrap();
            let execution = record.source.as_ref().unwrap().lock().unwrap();
            assert_eq!(execution.protocol_receipts.len(), 1);
            let receipt = execution.protocol_receipts[0].clone();
            assert!(Arc::ptr_eq(&receipt.recipient, &peer.endpoint.registration));
            assert!(
                !receipt.settled(),
                "enqueue and dependent Drop cannot prove a failed write"
            );
            assert!(receipt.wire_record.lock().unwrap().as_ref().unwrap().len() >= 32);
            last = Some((record.token, receipt));
        }
    }
    finish_clients(&service, [first, second], [&source, &peer]);
    if let Some((token, receipt)) = last {
        let execution = completion.execution_of(token).unwrap();
        if replace {
            // A replacement registration cannot borrow the original endpoint's
            // positive termination, even with the identical event and bytes.
            let replacement = Arc::new(PrivateControlProtocolReceipt {
                recipient: Arc::new(std::sync::OnceLock::new()),
                endpoint: receipt.endpoint.clone(),
                event: receipt.event,
                flushed: AtomicBool::new(false),
                terminated: AtomicBool::new(false),
                wire_record: Mutex::new(receipt.wire_record.lock().unwrap().clone()),
            });
            execution.lock().unwrap().protocol_receipts[0] = replacement.clone();
            for _ in 0..1000 {
                let _ = super::final_custody_step(&service);
            }
            assert_eq!(completion.state_of(token), ControlRecordState::Outstanding);
            assert!(!replacement.settled());
            execution.lock().unwrap().protocol_receipts[0] = receipt.clone();
        }
        // Remove the exact physical recipient custody, not the origin's.
        let withheld = if withhold {
            let mut kept = service.owner.inventory.kept.lock().unwrap();
            let index = kept
                .places
                .iter()
                .position(|place| {
                    place
                        .as_ref()
                        .is_some_and(|place| place.cleanup_record().client == peer.endpoint.client)
                })
                .unwrap();
            Some((index, kept.places[index].take().unwrap()))
        } else {
            None
        };
        for _ in 0..1000 {
            let _ = super::final_custody_step(&service);
        }
        if let Some((index, custody)) = withheld {
            assert_eq!(completion.state_of(token), ControlRecordState::Outstanding);
            assert!(!receipt.settled());
            service.owner.inventory.kept.lock().unwrap().places[index] = Some(custody);
            for _ in 0..1000 {
                let _ = super::final_custody_step(&service);
            }
        }
        assert!(!receipt.flushed.load(Ordering::Acquire));
        assert!(receipt.terminated.load(Ordering::Acquire));
        assert_eq!(completion.state_of(token), ControlRecordState::Retired);
        assert_eq!(service.owner.store.reserved(), Some(0));
    }
    assert!(service.acks.try_recv().is_err());
    service.finish();
}

#[test]
fn original_generic_peer_flush_reuses_configure_and_presentation_credits() {
    generic_peer(false, false, false);
}

#[test]
fn failed_generic_peer_keeps_payload_until_exact_collected_recipient_termination() {
    generic_peer(true, false, false);
}

#[test]
fn withheld_generic_recipient_custody_keeps_failed_output_and_control_credit() {
    generic_peer(true, true, false);
}

#[test]
fn replacement_protocol_registration_cannot_borrow_original_termination() {
    generic_peer(true, false, true);
}

#[test]
fn newer_core_focus_supersedes_exact_queued_focus_out_without_false_flush() {
    let service = MaintainedService::launch_distinct();
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let (mut first, first_surface, first_source) = peer_window(&service);
    let (mut second, second_surface, second_source) = peer_window(&service);
    submit_focus(&service, &first_source, first_surface, 99990);
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
    let (arrived, observed) = sync_channel(1);
    let (resume, released) = sync_channel(1);
    *first_source.before_dependent.lock().unwrap() = Some((arrived, released));
    submit_focus(&service, &second_source, second_surface, 99991);
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        service
            .acks
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(read_event(&mut second, 5).unwrap()[0] & 0x7f, 9);
    let completion = service.registry.control_completion().unwrap();
    let (token, execution) = {
        let held = completion.inner.lock().unwrap();
        let record = held
            .records
            .iter()
            .find(|record| record.identity.transaction == TransactionId::from_raw(99991))
            .unwrap();
        (record.token, record.source.as_ref().unwrap().clone())
    };
    let window = first_source.tables.windows.lock().unwrap()[&first_surface];
    let mut request = vec![42, 0, 3, 0];
    request.extend((window.local.raw() as u32).to_le_bytes());
    request.extend(0u32.to_le_bytes());
    first.write_all(&request).unwrap();
    // Source focus changes before its eventual reply can acquire the control
    // output priority held by the deliberately paused old dependent.
    assert!(waited_for(|| {
        let held = execution.lock().unwrap();
        let claim = held.focus_peers[0].claim.as_ref().unwrap();
        first_source
            .endpoint
            .registration
            .get()
            .unwrap()
            .applied_focus_generation
            .load(Ordering::Acquire)
            != claim.issued.generation
    }));
    resume.send(()).unwrap();
    assert!(waited_for(
        || completion.state_of(token) == ControlRecordState::Retired
    ));
    {
        let held = execution.lock().unwrap();
        assert!(held.focus_peers[0].superseded);
        assert!(!held.focus_peers[0].flushed);
        assert!(held.dependent_records.is_empty());
    }
    assert!(waited_for(|| service.owner.store.reserved() == Some(0)));
    finish_clients(&service, [first, second], [&first_source, &second_source]);
    service.finish();
}
