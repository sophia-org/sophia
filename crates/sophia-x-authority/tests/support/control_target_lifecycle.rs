// Exercise the production writer, including the window lookup and runtime.
#[test]
fn a_real_focus_writer_distinguishes_an_unmapped_target_from_bad_authority() {
    let surface = SurfaceId::new(252, 1);
    let client = XServerFrontendClientId(252);
    let state = writer_runtime(surface);
    let namespace = NamespaceId::from_raw(252);
    let window = XResourceId::new(0x200252, 1);
    {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.apply(crate::XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(147), namespace,
            kind: crate::XAuthorityRequestKind::MapWindow { window, generation: 1 },
        });
        runtime.unmap_window(namespace, window).unwrap();
    }
    let (commands, receiver) = sync_channel(4);
    let (acks, answered) = sync_channel(4);
    commands.send(X11RoutedControl::Authority {
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(148), surface,
        }, focus: None, claim: None, completion: None,
    }).unwrap();
    let (writer, _peer) = writer_start(None, &state, receiver, None, acks, client,
        writer_windows(surface), Arc::new(AtomicUsize::new(0)));
    let result = answered.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
    assert!(writer_join(writer));
    assert_eq!(result.acknowledgement.outcome, XAuthorityControlOutcome::TargetNotViewable,
        "an unmapped target is an ordinary lifecycle race");
    assert_ne!(result.acknowledgement.outcome, XAuthorityControlOutcome::Delivered);
    assert!(answered.try_recv().is_err());
}

fn lifecycle_writer_answer(
    state: &X11CoreSocketServerState,
    command: XAuthorityControlCommand,
    windows: Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>>,
) -> XAuthorityControlOutcome {
    let (commands, receiver) = sync_channel(4);
    let (acks, answered) = sync_channel(4);
    commands.send(X11RoutedControl::Authority {
        command, focus: None, claim: None, completion: None,
    }).unwrap();
    let (writer, _peer) = writer_start(None, state, receiver, None, acks,
        XServerFrontendClientId(252), windows, Arc::new(AtomicUsize::new(0)));
    let answer = answered.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
    assert_eq!(answer.acknowledgement.transaction, command.transaction());
    assert_eq!(answer.acknowledgement.surface, command.surface());
    assert_eq!(answer.acknowledgement.kind, command.kind());
    assert!(writer_join(writer));
    assert!(answered.try_recv().is_err());
    answer.acknowledgement.outcome
}

#[test]
fn a_real_writer_classifies_destruction_after_the_route_lookup() {
    let surface = SurfaceId::new(252, 1);
    let state = writer_runtime(surface);
    state.runtime.lock().unwrap().destroy_window(
        NamespaceId::from_raw(252), XResourceId::new(0x200252, 1)).unwrap();
    for command in [
        configure(XServerFrontendClientId(252), surface, 149).command,
        XAuthorityControlCommand::FocusSurface { transaction: TransactionId::from_raw(150), surface },
        XAuthorityControlCommand::WithdrawSurface { transaction: TransactionId::from_raw(151), surface },
        XAuthorityControlCommand::AdmitSurface { transaction: TransactionId::from_raw(152), surface,
            geometry: Rect { x: 0, y: 0, width: 80, height: 60 } },
    ] {
        // Leave the route projection intact: the runtime lookup must classify it too.
        assert_eq!(lifecycle_writer_answer(&state, command, writer_windows(surface)),
            XAuthorityControlOutcome::UnknownSurface);
    }
}

#[test]
fn a_real_writer_refuses_withdrawn_admission_without_mapping_the_window() {
    let surface = SurfaceId::new(252, 1);
    let state = writer_runtime(surface);
    {
        let mut runtime = state.runtime.lock().unwrap();
        let namespace = NamespaceId::from_raw(252);
        let window = XResourceId::new(0x200252, 1);
        runtime.set_policy_map_deferred(true);
        assert_eq!(runtime.apply(crate::XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(152), namespace,
            kind: crate::XAuthorityRequestKind::MapWindow { window, generation: 1 },
        }).outcome, crate::XAuthorityResponseOutcome::Accepted);
        assert_eq!(runtime.window_policy_map_pending(namespace, window), Ok(true));
        runtime.unmap_window(namespace, window).unwrap();
    }
    let outcome = lifecycle_writer_answer(&state, XAuthorityControlCommand::AdmitSurface {
        transaction: TransactionId::from_raw(153), surface,
        geometry: Rect { x: 0, y: 0, width: 80, height: 60 },
    }, writer_windows(surface));
    assert_eq!(outcome, XAuthorityControlOutcome::AdmissionWithdrawn);
    assert_eq!(state.runtime.lock().unwrap().window_map_state(
        NamespaceId::from_raw(252), XResourceId::new(0x200252, 1)).unwrap(), crate::XMapState::Unmapped);
    assert_eq!(writer_window_width(&state), 40);
}

#[test]
fn a_real_writer_keeps_invalid_commands_distinct_from_lifecycle_races() {
    let surface = SurfaceId::new(252, 1);
    let state = writer_runtime(surface);
    let invalid_geometry = XAuthorityControlCommand::ConfigureSurface {
        transaction: TransactionId::from_raw(154), surface, geometry: Rect::default(),
    };
    assert_eq!(lifecycle_writer_answer(&state, invalid_geometry, writer_windows(surface)),
        XAuthorityControlOutcome::InvalidSize);
    let invalid_state = XAuthorityControlCommand::SetPresentationState {
        transaction: TransactionId::from_raw(155), surface,
        state: sophia_protocol::PolicyPresentationState {
            fullscreen: true, maximized: true, ..Default::default()
        },
    };
    assert_eq!(lifecycle_writer_answer(&state, invalid_state, writer_windows(surface)),
        XAuthorityControlOutcome::AuthorityRejected);
    assert_eq!(writer_window_width(&state), 40);
}
