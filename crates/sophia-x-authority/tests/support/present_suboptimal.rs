#![cfg(all(test, unix))]

use super::*;

#[test]
fn wire_present_reservations_follow_acceptance_destroy_and_disconnect() {
    let path = std::env::temp_dir().join(format!(
        "sophia-present-suboptimal-{}.sock",
        std::process::id()
    ));
    let config = XServerFrontendConfig::new(&path, NS)
        .unwrap()
        .with_device_bundle(Arc::new(
            XServerFrontendDeviceBundle::new(1, Arc::new(Provider(identity())), None).unwrap(),
        ));
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let broker = XServerFrontendRouteBroker::new(
        NonZeroUsize::new(crate::X_PRESENT_PER_CLIENT_CAPACITY).unwrap(),
    );
    let (sender, observed) = std::sync::mpsc::channel();
    let mut socket = UnixStream::connect(&path).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    frontend
        .serve_next_concurrently_routed_traced(
            &broker,
            Arc::new(move |trace| {
                let _ = sender.send(trace);
                Ok(None)
            }),
        )
        .unwrap();
    socket
        .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    let mut header = [0; 8];
    socket.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 1);
    let mut setup = vec![0; usize::from(u16::from_le_bytes(header[6..8].try_into().unwrap())) * 4];
    socket.read_exact(&mut setup).unwrap();
    let base = u32::from_le_bytes(setup[4..8].try_into().unwrap());
    let window = base | 1;
    let pixmap = base | 2;
    let mut create = vec![0; 32];
    create[0] = 1;
    create[2..4].copy_from_slice(&8u16.to_le_bytes());
    create[4..8].copy_from_slice(&window.to_le_bytes());
    create[8..12].copy_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    create[16..18].copy_from_slice(&32u16.to_le_bytes());
    create[18..20].copy_from_slice(&32u16.to_le_bytes());
    socket.write_all(&create).unwrap();
    let created = observed.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(created.major_opcode, 1);
    assert!(created.failure.is_none());
    let mut map = vec![8, 0, 2, 0];
    map.extend(window.to_le_bytes());
    socket.write_all(&map).unwrap();
    let mapped = observed.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(mapped.major_opcode, 8);
    assert!(mapped.failure.is_none());
    {
        let mut runtime = frontend.state.runtime.lock().unwrap();
        let pixmap = XResourceId::new(u64::from(pixmap), 1);
        runtime
            .create_dri3_pixmap_from_buffers(
                NS,
                pixmap,
                1,
                1,
                32,
                32,
                [128, 0, 0, 0],
                [0; 4],
                32,
                32,
                TILED,
            )
            .unwrap();
        runtime
            .attach_dri3_plane_fds(
                NS,
                pixmap,
                vec![Arc::new(File::open("/dev/null").unwrap().into())],
            )
            .unwrap();
    }

    // Keep accepted requests simultaneously pending so a later request cannot
    // overwrite the permission of an earlier exact transaction.
    let mut accepted = Vec::new();
    for (index, (options, permission)) in [
        (0x08u32, Some(true)),
        (0x00, Some(false)),
        (0x02, Some(false)),
        (0x0a, Some(false)),
        (0x09, Some(true)),
        (0x01, Some(false)),
        (0x0d, None),
        (0x05, None),
        (0x18, None),
        (0x00, Some(false)),
    ]
    .into_iter()
    .enumerate()
    {
        let serial = 100 + u32::try_from(index).unwrap();
        let mut request = [0; 72];
        request[0] = crate::X_PRESENT_MAJOR_OPCODE;
        request[1] = crate::X_PRESENT_PIXMAP_MINOR_OPCODE;
        request[2..4].copy_from_slice(&18u16.to_le_bytes());
        request[4..8].copy_from_slice(&window.to_le_bytes());
        request[8..12].copy_from_slice(&pixmap.to_le_bytes());
        request[12..16].copy_from_slice(&serial.to_le_bytes());
        request[40..44].copy_from_slice(&options.to_le_bytes());
        socket.write_all(&request).unwrap();
        let trace = observed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(trace.major_opcode, crate::X_PRESENT_MAJOR_OPCODE);
        assert_eq!(
            trace.minor_opcode,
            u16::from(crate::X_PRESENT_PIXMAP_MINOR_OPCODE)
        );
        let pending = broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap();
        if let Some(permission) = permission {
            let submission = trace.present_submission.expect("accepted DMA Present");
            assert_eq!(submission.transaction, trace.transaction);
            let entry = pending.get(&trace.transaction).unwrap();
            assert_eq!(entry.serial, serial);
            assert_eq!(entry.suboptimal, permission, "options={options:#x}");
            accepted.push((trace.transaction, permission));
        } else {
            assert!(trace.present_submission.is_none());
            assert!(!pending.contains_key(&trace.transaction));
            let mut error = [0; 32];
            socket.read_exact(&mut error).unwrap();
            assert_eq!(error[0], 0);
            assert_eq!(error[1], crate::XErrorCode::BadValue.wire_code());
        }
        for (transaction, permission) in &accepted {
            assert_eq!(pending.get(transaction).unwrap().suboptimal, *permission);
        }
        assert_eq!(pending.len(), accepted.len());
    }

    let cancelled = accepted.remove(0).0;
    broker.registry.cancel_present(cancelled).unwrap();
    assert!(
        !broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .contains_key(&cancelled)
    );
    // Even explicit reuse of the registry key starts from the supplied metadata.
    broker
        .registry
        .queue_present(
            cancelled,
            created.client,
            XResourceId::new(u64::from(window), 1),
            XResourceId::new(u64::from(pixmap), 1),
            200,
            None,
            false,
        )
        .unwrap();
    assert!(
        !broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()[&cancelled]
            .suboptimal
    );
    for (transaction, _) in accepted {
        broker.registry.cancel_present(transaction).unwrap();
    }
    broker.registry.cancel_present(cancelled).unwrap();
    let window_id = XResourceId::new(u64::from(window), 1);
    let pixmap_id = XResourceId::new(u64::from(pixmap), 1);
    // Exercise the real destruction/connection cleanup seams with the same
    // reservations and preparations that timed admission will own. The wire
    // deadline scheduler is separate; no completion is injected here.
    for round in 0..2 {
        let transactions: Vec<_> = (0..crate::X_PRESENT_PER_CLIENT_CAPACITY)
            .map(|i| TransactionId::from_raw(10_000 + round * 100 + i as u64))
            .collect();
        for transaction in &transactions {
            broker
                .registry
                .queue_present(
                    *transaction,
                    created.client,
                    window_id,
                    pixmap_id,
                    1,
                    None,
                    false,
                )
                .unwrap();
            frontend
                .state
                .runtime
                .lock()
                .unwrap()
                .prepare_standard_pixmap(
                    created.client.raw(),
                    *transaction,
                    NS,
                    window_id,
                    pixmap_id,
                    (0, 0),
                    None,
                    None,
                    crate::XPresentFenceResources::default(),
                )
                .unwrap();
        }
        if round == 1 {
            break;
        }
        broker
            .registry
            .select_present_input(
                created.client,
                XResourceId::new(u64::from(base | 3), 1),
                window_id,
                6,
            )
            .unwrap();
        broker
            .registry
            .prepare_present_msc_notify(window_id, 3, 99)
            .unwrap();
        let mut destroy = vec![4, 0, 2, 0];
        destroy.extend(window.to_le_bytes());
        socket.write_all(&destroy).unwrap();
        let destroyed = observed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(destroyed.major_opcode, 4);
        assert!(destroyed.failure.is_none());
        assert_eq!(
            frontend
                .state
                .runtime
                .lock()
                .unwrap()
                .prepared_present_count(),
            0
        );
        assert!(
            broker
                .registry
                .pending_presentations
                .entries
                .lock()
                .unwrap()
                .is_empty()
        );
        assert!(
            broker
                .registry
                .pending_msc_notifies
                .lock()
                .unwrap()
                .is_empty()
        );
        assert!(
            broker
                .registry
                .present_subscriptions
                .lock()
                .unwrap()
                .is_empty()
        );
        // Late backend feedback must be ignored; no Complete or Idle can be
        // delivered to the destroyed window or a later user of its XID.
        for transaction in transactions {
            assert!(
                !broker
                    .registry
                    .route_present_complete(transaction, 100, 1, XPresentCompletionMode::Skip)
                    .unwrap()
            );
            assert!(!broker.registry.route_present_idle(transaction).unwrap());
        }
        socket.write_all(&create).unwrap();
        assert!(
            observed
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .failure
                .is_none()
        );
        socket.write_all(&map).unwrap();
        assert!(
            observed
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .failure
                .is_none()
        );
        // The next iteration reserves all 64 slots again on this connection.
    }
    drop(socket);
    frontend.wait_for_clients().unwrap();
    assert_eq!(
        frontend
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
    assert!(
        broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
}
