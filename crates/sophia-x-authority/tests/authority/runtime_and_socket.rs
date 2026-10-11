#[test]
fn x_authority_runtime_sequence_emits_surface_transaction_and_portal_prompt() {
    let source_namespace = NamespaceId::from_raw(24);
    let target_namespace = NamespaceId::from_raw(25);
    let mut runtime = XAuthorityRuntime::new();

    assert_eq!(
        runtime
            .apply(create_window_request(
                TransactionId::from_raw(106),
                source_namespace
            ))
            .surfaces
            .len(),
        1
    );
    assert_eq!(
        runtime
            .apply(create_second_window_request(
                TransactionId::from_raw(107),
                target_namespace
            ))
            .surfaces
            .len(),
        1
    );
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(108),
        namespace: source_namespace,
        kind: XAuthorityRequestKind::SetSelectionOwner {
            selection: 77,
            owner: Some(XResourceId::new(0xc0, 1)),
            timestamp: 10,
            selection_timestamp: 10,
            kind: XSelectionChangeKind::SetOwner,
        },
    });
    let present = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(109),
        namespace: source_namespace,
        kind: XAuthorityRequestKind::PresentPixmap {
            window: XResourceId::new(0xc0, 1),
            pixmap: 0x778,
            damage: Region::single(Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 60,
            }),
            previous_committed_generation: 1,
            timeout_msec: 250,
        },
    });
    let selection = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(110),
        namespace: target_namespace,
        kind: XAuthorityRequestKind::RequestSelection {
            requestor: XResourceId::new(0xc1, 1),
            selection: 77,
            target: 78,
            target_name: "UTF8_STRING".to_owned(),
            property: 79,
            time: 11,
            transfer: PortalTransferId::from_raw(12),
        },
    });

    assert_eq!(runtime.resource_count(), 2);
    assert_eq!(runtime.window_count(), 2);
    assert_eq!(present.transactions.len(), 1);
    assert_eq!(
        present.transactions[0].readiness,
        SurfaceTransactionReadiness::Ready
    );
    assert_eq!(selection.portal_commands.len(), 1);
}

#[test]
fn x_authority_runtime_selection_error_emits_native_failure_artifact() {
    let namespace = NamespaceId::from_raw(26);
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(create_window_request(
        TransactionId::from_raw(111),
        namespace,
    ));

    let response = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(112),
        namespace,
        kind: XAuthorityRequestKind::RequestSelection {
            requestor: XResourceId::new(0xc0, 1),
            selection: 88,
            target: 89,
            target_name: "UTF8_STRING".to_owned(),
            property: 90,
            time: 12,
            transfer: PortalTransferId::from_raw(13),
        },
    });

    assert_eq!(
        response.outcome,
        XAuthorityResponseOutcome::Rejected(XAuthorityRuntimeError::UnknownSourceOwner)
    );
    assert_eq!(response.selection_artifacts.len(), 1);
}

#[test]
fn cross_namespace_grant_installs_bounded_utf8_and_reports_stale_owner() {
    let source = NamespaceId::from_raw(31);
    let target = NamespaceId::from_raw(32);
    let transfer = PortalTransferId::from_raw(41);
    let mut atoms = XAtomTable::new();
    let selection = atoms.intern("CLIPBOARD", false).unwrap().unwrap();
    let utf8 = atoms.intern("UTF8_STRING", false).unwrap().unwrap();
    let property = atoms.intern("SOPHIA_TEST", false).unwrap().unwrap();
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(create_window_request(TransactionId::from_raw(1), source));
    runtime.apply(create_second_window_request(
        TransactionId::from_raw(2),
        target,
    ));
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(3),
        namespace: source,
        kind: XAuthorityRequestKind::SetSelectionOwner {
            selection,
            owner: Some(XResourceId::new(0xc0, 1)),
            timestamp: 10,
            selection_timestamp: 10,
            kind: XSelectionChangeKind::SetOwner,
        },
    });
    let request = |transfer| XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(4),
        namespace: target,
        kind: XAuthorityRequestKind::RequestSelection {
            requestor: XResourceId::new(0xc1, 1),
            selection,
            target: utf8,
            target_name: "UTF8_STRING".to_owned(),
            property,
            time: 11,
            transfer,
        },
    };
    runtime.apply(request(transfer));
    let retained = sophia_protocol::PortalRequest {
        transfer: PortalTransfer {
            transfer,
            source_namespace: source,
            target_namespace: target,
            kind: PortalTransferKind::Clipboard,
            mime_type: Some("UTF8_STRING".into()),
            byte_size: 0,
            decision: PortalDecision::Pending,
            generation: 1,
        },
        deadline_msec: 2_000,
    };
    assert_eq!(runtime.clipboard_source_generation(&retained), Some(1));
    for field in 0..5 {
        let mut forged = retained.clone();
        match field {
            0 => forged.transfer.source_namespace = target,
            1 => forged.transfer.target_namespace = source,
            2 => forged.transfer.generation = 2,
            3 => forged.transfer.mime_type = Some("TARGETS".into()),
            _ => forged.transfer.transfer = PortalTransferId::from_raw(999),
        }
        assert_eq!(runtime.clipboard_source_generation(&forged), None,
            "source facts require the retained conversion, field {field}");
    }
    let primary = atoms.intern("PRIMARY", false).unwrap().unwrap();
    for timestamp in [11, 12] {
        runtime.apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(30 + timestamp),
            namespace: source,
            kind: XAuthorityRequestKind::SetSelectionOwner {
                selection: primary,
                owner: Some(XResourceId::new(0xc0, 1)),
                timestamp: timestamp as u32,
                selection_timestamp: timestamp as u32,
                kind: XSelectionChangeKind::SetOwner,
            },
        });
    }
    assert_eq!(runtime.clipboard_source_generation(&retained), Some(1),
        "PRIMARY changes cannot replace CLIPBOARD source facts");
    let grant = PortalGrant {
        transfer,
        source_namespace: source,
        target_namespace: target,
        kind: PortalTransferKind::Clipboard,
        source_generation: 1,
        broker_generation: 1,
        deadline_msec: 2_000,
        state: PortalGrantState::Active,
    };
    let mut properties = XPropertyTable::new();
    let frame = encode_portal_clipboard_payload_frame(transfer, b"hello").unwrap();
    let outcome = runtime
        .execute_clipboard_payload_frame(&frame, &grant, &mut atoms, &mut properties)
        .unwrap();
    let ClipboardSelectionExecutionOutcome::Handoff(handoff) = outcome else {
        panic!("expected handoff");
    };
    assert_eq!(handoff.notify.property, property);
    let record = properties
        .get(target, XResourceId::new(0xc1, 1), property)
        .unwrap();
    assert_eq!(record.property_type, utf8);
    assert_eq!(record.format, 8);
    assert_eq!(record.bytes, b"hello");
    assert_eq!(runtime.clipboard_source_generation(&retained), None,
        "settled conversion cannot supply fresh source authority");

    let stale_transfer = PortalTransferId::from_raw(42);
    runtime.apply(request(stale_transfer));
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(5),
        namespace: source,
        kind: XAuthorityRequestKind::SetSelectionOwner {
            selection,
            owner: Some(XResourceId::new(0xc0, 1)),
            timestamp: 12,
            selection_timestamp: 12,
            kind: XSelectionChangeKind::SetOwner,
        },
    });
    let stale_grant = PortalGrant {
        transfer: stale_transfer,
        ..grant
    };
    let mut stale_request = retained.clone();
    stale_request.transfer.transfer = stale_transfer;
    assert_eq!(runtime.clipboard_source_generation(&stale_request), Some(4),
        "current owner generation must not echo the stale request");
    let outcome = runtime
        .execute_clipboard_payload(
            stale_transfer,
            &stale_grant,
            b"stale",
            &mut atoms,
            &mut properties,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        ClipboardSelectionExecutionOutcome::Failed {
            error: ClipboardSelectionExecutionError::StaleOwnerGeneration,
            notify: ClipboardSelectionNotify {
                property: X_ATOM_NONE,
                ..
            }
        }
    ));

    let targets = atoms.intern("TARGETS", false).unwrap().unwrap();
    let targets_transfer = PortalTransferId::from_raw(43);
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(6),
        namespace: target,
        kind: XAuthorityRequestKind::RequestSelection {
            requestor: XResourceId::new(0xc1, 1),
            selection,
            target: targets,
            target_name: "TARGETS".to_owned(),
            property,
            time: 13,
            transfer: targets_transfer,
        },
    });
    let targets_grant = PortalGrant {
        transfer: targets_transfer,
        source_generation: 4,
        ..stale_grant
    };
    let outcome = runtime
        .execute_clipboard_payload(
            targets_transfer,
            &targets_grant,
            b"",
            &mut atoms,
            &mut properties,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        ClipboardSelectionExecutionOutcome::Handoff(_)
    ));
    let record = properties
        .get(target, XResourceId::new(0xc1, 1), property)
        .unwrap();
    assert_eq!(record.property_type, X_ATOM_ATOM);
    assert_eq!(record.format, 32);
    let advertised = record
        .bytes
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(advertised[0], targets);
    assert_eq!(advertised[1], utf8);
    assert_eq!(atoms.name(advertised[2]), Some("text/plain;charset=utf-8"));

    for (raw, error) in [
        (50, ClipboardSelectionExecutionError::Denied),
        (51, ClipboardSelectionExecutionError::Expired),
        (52, ClipboardSelectionExecutionError::Disconnected),
        (53, ClipboardSelectionExecutionError::ExecutorFailure),
    ] {
        let transfer = PortalTransferId::from_raw(raw);
        runtime.apply(request(transfer));
        let outcome = runtime.fail_clipboard_transfer(transfer, error).unwrap();
        assert!(matches!(
            outcome,
            ClipboardSelectionExecutionOutcome::Failed {
                error: actual,
                notify: ClipboardSelectionNotify { property: X_ATOM_NONE, .. }
            } if actual == error
        ));
    }
}

#[cfg(unix)]
#[test]
fn x_authority_socket_round_trips_repeated_requests() {
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-authority-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        run_x_authority_socket_server_once(&server_path).unwrap();
    });

    wait_for_socket(&socket_path);
    let mut stream = UnixStream::connect(&socket_path).unwrap();
    write_x_authority_request(
        &mut stream,
        &create_window_request(TransactionId::from_raw(113), NamespaceId::from_raw(27)),
    )
    .unwrap();
    let first = read_x_authority_response(&mut stream).unwrap();
    write_x_authority_request(
        &mut stream,
        &XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(114),
            namespace: NamespaceId::from_raw(27),
            kind: XAuthorityRequestKind::MapWindow {
                window: XResourceId::new(0xc0, 1),
                generation: 2,
            },
        },
    )
    .unwrap();
    let second = read_x_authority_response(&mut stream).unwrap();

    assert_eq!(first.surfaces.len(), 1);
    assert_eq!(second.surfaces.len(), 1);
    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    let _ = server.join();
}

