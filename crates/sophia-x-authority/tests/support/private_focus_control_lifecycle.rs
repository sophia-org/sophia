// Included inside the private focus fixture; this runs the actual writer.
fn answer_private_focus(fixture: &Fixture, claim: PrivateFocusClaim) -> XAuthorityControlOutcome {
    let (sender, receiver) = sync_channel(2);
    let (ack_sender, acks) = sync_channel(2);
    sender.send(X11RoutedControl::Authority {
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(148), surface: SurfaceId::new(252, 1),
        }, focus: None, claim: Some(claim), completion: None,
    }).unwrap();
    let (stream, _peer) = UnixStream::pair().unwrap();
    let writer = spawn_x11_control_writer(
        X11ClientOutput::shared(stream, 0), Arc::new(AtomicUsize::new(0)),
        Arc::new(X11WirePermission::open()), XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)), fixture.projection.clone(),
        writer_windows(SurfaceId::new(252, 1)), Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(BTreeMap::new())), fixture.selections.clone(),
        Arc::new(AtomicU16::new(0)), fixture.state.atoms.clone(), fixture.state.properties.clone(),
        fixture.state.runtime.clone(), fixture.state.control_runtime_pending.clone(),
        crate::XWireClientResourceRange { base: 0x200000, mask: 0x1fffff },
        namespace(), client(), Some(fixture.private.broker.registry.clone()),
        X11ControlChannels::ClientBound { receiver: receiver.into(), acknowledgements: ack_sender.into(), completion: None },
    ).unwrap();
    let answer = acks.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(writer_join(writer));
    assert!(acks.try_recv().is_err());
    answer.acknowledgement.outcome
}

#[test]
fn the_real_writer_retires_superseded_focus_without_clearing_newer_focus() {
    let fixture = fixture();
    let old = fixture.reserve(window());
    let newer = fixture.reserve(window());
    fixture.apply(&newer, X11FocusChange::Surface { window: window() }).unwrap();
    assert_eq!(answer_private_focus(&fixture, old), XAuthorityControlOutcome::Superseded);
    assert_eq!(fixture.projection.load(Ordering::Acquire), window().local.raw());
    assert_eq!(fixture.state.runtime.lock().unwrap().input_focus(namespace()), (window(), 1));
    assert!(fixture.published());
}

#[test]
fn the_real_writer_retires_focus_after_its_admission_closed_or_was_removed() {
    for removed in [false, true] {
        let fixture = fixture();
        let old = fixture.reserve(window());
        if removed {
            revoke_focus_fixture(&fixture);
        } else {
            let lifecycle = fixture.private.broker.registry.input_recovery.lifecycle.get().unwrap()
                .register(client(), namespaced(client(), namespace())).unwrap();
            lifecycle.close();
        }
        assert_eq!(answer_private_focus(&fixture, old), XAuthorityControlOutcome::AdmissionWithdrawn);
        assert_eq!(fixture.projection.load(Ordering::Acquire), root().local.raw());
        assert!(!fixture.published());
    }
}
