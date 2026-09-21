// The real control writer against an actual socket: record ordering,
// stale FocusOut suppression, and what an abandoned or failed write owes.
// Split from the fixture module purely for size; same module scope.
    fn spawn_real_writer(
        fixture: &mut Fixture,
        stream: Arc<Mutex<UnixStream>>,
        priority: Arc<AtomicUsize>,
    ) -> X11ControlWriter {
        let channels = fixture.channels.take().unwrap();
        let (ack_sender, _acks) = sync_channel(16);
        spawn_x11_control_writer(
            stream,
            priority,
            Arc::new(X11WirePermission::open()),
            XByteOrder::LittleEndian,
            Arc::new(AtomicU16::new(1)),
            fixture.projection.clone(),
            writer_windows(SurfaceId::new(252, 1)),
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(Mutex::new(BTreeMap::new())),
            fixture.selections.clone(),
            Arc::new(AtomicU16::new(0)),
            fixture.state.atoms.clone(),
            fixture.state.properties.clone(),
            fixture.state.runtime.clone(),
            fixture.state.control_runtime_pending.clone(),
            crate::XWireClientResourceRange {
                base: 0x200000,
                mask: 0x1fffff,
            },
            namespace(),
            client(),
            Some(fixture.private.broker.registry.clone()),
            X11ControlChannels::ClientBound {
                receiver: channels.control,
                acknowledgements: ack_sender,
                completion: None,
            },
        )
        .unwrap()
    }
    fn start_real_writer(fixture: &mut Fixture) -> (X11ControlWriter, UnixStream) {
        let (stream, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let writer = spawn_real_writer(
            fixture,
            Arc::new(Mutex::new(stream)),
            Arc::new(AtomicUsize::new(0)),
        );
        (writer, peer)
    }
    fn core_change(
        fixture: &Fixture,
        target: XResourceId,
        stream: &Arc<Mutex<UnixStream>>,
        priority: &Arc<AtomicUsize>,
    ) -> (XDispatchResult, X11PendingFocusPublication) {
        let context = crate::XDispatchContext {
            byte_order: XByteOrder::LittleEndian,
            namespace: namespace(),
            transaction: TransactionId::from_raw(991),
            server_time: 4_242,
            sequence: 3,
            major_opcode: 42,
            client_id: client().raw(),
            injection: crate::XTestAdmission::Absent,
        };
        let mut runtime = fixture.state.runtime.lock().unwrap();
        let (output, pending) = x11_dispatch_private_focus(
            &mut runtime,
            context,
            client(),
            &fixture.projection,
            &fixture.private.broker.registry,
            target,
            1,
            crate::X_CURRENT_TIME,
            fixture.state.runtime.clone(),
            fixture.state.control_runtime_pending.clone(),
            stream.clone(),
            priority.clone(),
            Arc::new(X11WirePermission::open()),
        )
        .unwrap();
        (output, pending.unwrap())
    }

    #[test]
    fn real_writer_omits_stale_focusout_after_newer_core_focusin_for_the_same_window() {
        let mut fixture = fixture();
        let old = fixture.reserve(window());
        fixture
            .apply(&old, X11FocusChange::Surface { window: window() })
            .unwrap();
        XServerFrontendRouteRegistry::record_private_focus_queued(&old);
        fixture
            .private
            .broker
            .registry
            .route_focus_out(
                XServerFrontendSurfaceRoute {
                    client: client(),
                    namespace: namespace(),
                    admission: Some(namespaced(client(), namespace())),
                    window: window(),
                },
                7,
                None,
            )
            .unwrap();
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let stream = Arc::new(Mutex::new(stream));
        let priority = Arc::new(AtomicUsize::new(0));
        let sequence = AtomicU16::new(2);
        for target in [root(), window()] {
            let (output, mut pending) = core_change(&fixture, target, &stream, &priority);
            assert!(
                !fixture.published(),
                "core focus remains ineligible before its output"
            );
            let frames = output.encoded_outputs(XByteOrder::LittleEndian);
            let count = frames.len();
            pending.records = Some(frames);
            pending.write_output(&sequence, 2).unwrap();
            let mut frame = [0_u8; 32];
            for _ in 0..count {
                std::io::Read::read_exact(&mut peer, &mut frame).unwrap();
            }
            assert_eq!(
                frame[0], 9,
                "new core FocusIn really leaves the source stream"
            );
            assert_eq!(
                u32::from_le_bytes(frame[4..8].try_into().unwrap()),
                target.local.raw() as u32
            );
        }
        let revision = fixture
            .private
            .broker
            .registry
            .private_applied
            .get()
            .unwrap()
            .publication
            .lock()
            .unwrap()
            .revision;
        let writer = spawn_real_writer(&mut fixture, stream, priority);
        let mut bytes = [0_u8; 32];
        let read = std::io::Read::read(&mut peer, &mut bytes);
        assert!(
            read.as_ref().is_err_and(|error| matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            )),
            "stale FocusOut must not leave writer: {read:?}"
        );
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            window().local.raw()
        );
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .private_applied
                .get()
                .unwrap()
                .publication
                .lock()
                .unwrap()
                .revision,
            revision
        );
        assert!(fixture.published());
        assert!(writer_join(writer));
    }

    #[test]
    fn real_writer_matching_focusout_emits_exact_record_and_invalidates_publication() {
        let mut fixture = fixture();
        let old = fixture.reserve(window());
        fixture
            .apply(&old, X11FocusChange::Surface { window: window() })
            .unwrap();
        XServerFrontendRouteRegistry::record_private_focus_queued(&old);
        fixture
            .private
            .broker
            .registry
            .route_focus_out(
                XServerFrontendSurfaceRoute {
                    client: client(),
                    namespace: namespace(),
                    admission: Some(namespaced(client(), namespace())),
                    window: window(),
                },
                7,
                None,
            )
            .unwrap();
        let (writer, mut peer) = start_real_writer(&mut fixture);
        let mut bytes = [0_u8; 32];
        std::io::Read::read_exact(&mut peer, &mut bytes).unwrap();
        assert_eq!(bytes[0], 10, "FocusOut");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            window().local.raw() as u32
        );
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            root().local.raw()
        );
        assert!(!fixture.published());
        assert_eq!(
            fixture
                .state
                .runtime
                .lock()
                .unwrap()
                .input_focus(namespace())
                .0,
            window()
        );
        assert!(writer_join(writer));
    }

    #[test]
    fn real_control_output_precedes_same_window_core_focusin_after_dependent_clear() {
        let mut fixture = fixture();
        let old = fixture.reserve(window());
        fixture
            .apply(&old, X11FocusChange::Surface { window: window() })
            .unwrap();
        XServerFrontendRouteRegistry::record_private_focus_queued(&old);
        fixture
            .private
            .broker
            .registry
            .route_focus_out(
                XServerFrontendSurfaceRoute {
                    client: client(),
                    namespace: namespace(),
                    admission: Some(namespaced(client(), namespace())),
                    window: window(),
                },
                7,
                None,
            )
            .unwrap();
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let stream = Arc::new(Mutex::new(stream));
        let priority = Arc::new(AtomicUsize::new(0));
        // Only transport is held: runtime/common/X remain free for producers.
        let transport_gate = stream.lock().unwrap();
        let writer = spawn_real_writer(&mut fixture, stream.clone(), priority.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while fixture.projection.load(Ordering::Acquire) != root().local.raw()
            && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            root().local.raw()
        );
        assert_eq!(priority.load(Ordering::Acquire), 1);
        let (back, mut pending) = core_change(&fixture, window(), &stream, &priority);
        assert!(back.outputs.iter().any(|output| matches!(output, crate::XClientOutput::Event(crate::XClientEvent::Focus { event, focused: true, .. }) if *event == window())));
        assert!(
            !fixture.published(),
            "ordered keys cannot overtake unwritten core FocusIn"
        );
        let frames = back.encoded_outputs(XByteOrder::LittleEndian);
        let frame_count = frames.len();
        pending.records = Some(frames);
        let core = std::thread::spawn(move || {
            pending.write_output(&AtomicU16::new(2), 3).unwrap();
        });
        drop(transport_gate);
        let mut record = [0_u8; 32];
        std::io::Read::read_exact(&mut peer, &mut record).unwrap();
        assert_eq!(record[0], 10, "older control FocusOut wins transport first");
        assert_eq!(
            u32::from_le_bytes(record[4..8].try_into().unwrap()),
            window().local.raw() as u32
        );
        for _ in 0..frame_count {
            std::io::Read::read_exact(&mut peer, &mut record).unwrap();
        }
        assert_eq!(
            record[0], 9,
            "newer core FocusIn restores client focus last"
        );
        assert_eq!(
            u32::from_le_bytes(record[4..8].try_into().unwrap()),
            window().local.raw() as u32
        );
        core.join().unwrap();
        assert!(writer_join(writer));
        assert!(fixture.published());
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            window().local.raw()
        );
    }

    #[test]
    fn abandoned_core_output_keeps_focus_unpublished() {
        let fixture = fixture();
        let (stream, _peer) = UnixStream::pair().unwrap();
        let (output, pending) = core_change(
            &fixture,
            window(),
            &Arc::new(Mutex::new(stream)),
            &Arc::new(AtomicUsize::new(0)),
        );
        assert!(!output.outputs.is_empty());
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            window().local.raw()
        );
        drop(pending);
        assert!(!fixture.published());
    }

    #[test]
    fn superseded_core_output_neither_emits_stale_focusin_nor_republishes() {
        let fixture = fixture();
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let (output, mut pending) = core_change(
            &fixture,
            window(),
            &Arc::new(Mutex::new(stream)),
            &Arc::new(AtomicUsize::new(0)),
        );
        pending.records = Some(output.encoded_outputs(XByteOrder::LittleEndian));
        let newer = fixture.reserve(root());
        fixture.apply(&newer, X11FocusChange::Clear).unwrap();
        pending.write_output(&AtomicU16::new(1), 3).unwrap();
        assert_eq!(pending.emission, X11CoreFocusEmission::Superseded);
        assert!(fixture.published());
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            root().local.raw()
        );
        let mut frame = [0_u8; 32];
        assert!(
            std::io::Read::read(&mut peer, &mut frame).is_err_and(|error| matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ))
        );
    }

    #[test]
    fn failed_core_write_retains_records_as_indeterminate_and_never_replays_or_publishes() {
        let fixture = fixture();
        let (stream, peer) = UnixStream::pair().unwrap();
        let (output, mut pending) = core_change(
            &fixture,
            window(),
            &Arc::new(Mutex::new(stream)),
            &Arc::new(AtomicUsize::new(0)),
        );
        pending.records = Some(output.encoded_outputs(XByteOrder::LittleEndian));
        peer.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(pending.write_output(&AtomicU16::new(1), 3).is_err());
        assert_eq!(pending.emission, X11CoreFocusEmission::Indeterminate);
        assert!(!pending.records.as_ref().unwrap().is_empty());
        assert!(!fixture.published());
        assert!(
            pending
                .write_output(&AtomicU16::new(1), 3)
                .unwrap_err()
                .to_string()
                .contains("indeterminate")
        );
    }

    #[test]
    fn root_to_root_dependent_clear_cannot_be_republished_by_pending_core_output() {
        let fixture = fixture();
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let (output, mut pending) = core_change(
            &fixture,
            root(),
            &Arc::new(Mutex::new(stream)),
            &Arc::new(AtomicUsize::new(0)),
        );
        pending.records = Some(output.encoded_outputs(XByteOrder::LittleEndian));
        assert_eq!(
            fixture.focus_out(&pending.claim),
            X11DependentFocusEffect::ProjectionCleared
        );
        pending.write_output(&AtomicU16::new(1), 3).unwrap();
        assert_eq!(pending.emission, X11CoreFocusEmission::Superseded);
        assert!(!fixture.published());
        let mut frame = [0_u8; 32];
        assert!(
            std::io::Read::read(&mut peer, &mut frame).is_err_and(|error| matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ))
        );
    }
