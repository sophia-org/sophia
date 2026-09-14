/// Effect-producer composition controls using actual runtime, connection
/// attachment and origin-issued claims. Shared writer/core call sites are in
/// the integration patch; these do not substitute for its socket controls.
mod private_applied_focus {
    use super::*;

    struct Fixture {
        private: PrivateXServerFrontend,
        registration: XServerFrontendClientRouteRegistration,
        channels: Option<XServerFrontendClientRouteChannels>,
        state: X11CoreSocketServerState,
        projection: Arc<AtomicU64>,
        selections: Arc<Mutex<XCoreEventSelectionState>>,
    }
    fn namespace() -> NamespaceId {
        NamespaceId::from_raw(252)
    }
    fn client() -> XServerFrontendClientId {
        XServerFrontendClientId::from_raw(252)
    }
    fn window() -> XResourceId {
        XResourceId::new(0x200252, 1)
    }
    fn root() -> XResourceId {
        XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1)
    }
    fn fixture() -> Fixture {
        fixture_with_focus_preparation(true)
    }
    fn fixture_with_focus_preparation(prepare: bool) -> Fixture {
        let private = private_for_roles();
        let admission = namespaced(client(), namespace());
        private.participant.admit(client(), admission).unwrap();
        let (registration, channels) = private
            .broker
            .registry
            .register_client_with_admission(client(), Some(admission))
            .unwrap();
        let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        selections
            .lock()
            .unwrap()
            .update(window(), Some(1 << 21), None);
        let projection = Arc::new(AtomicU64::new(root().local.raw()));
        private
            .broker
            .registry
            .attach_connection_state(
                &registration,
                namespace(),
                selections.clone(),
                projection.clone(),
            )
            .unwrap();
        private
            .broker
            .registry
            .install_private_applied(&private.participant, namespace())
            .unwrap();
        let state = writer_runtime(SurfaceId::new(252, 1));
        if prepare {
            state
                .runtime
                .lock()
                .unwrap()
                .prepare_input_focus_namespace(namespace());
        }
        Fixture {
            private,
            registration,
            channels: Some(channels),
            state,
            projection,
            selections,
        }
    }
    impl Fixture {
        fn reserve(&self, window: XResourceId) -> PrivateFocusClaim {
            self.private
                .broker
                .registry
                .reserve_private_focus(client(), window)
                .unwrap()
                .unwrap()
        }
        fn apply(
            &self,
            claim: &PrivateFocusClaim,
            change: X11FocusChange,
        ) -> Result<X11AppliedFocus, X11FocusApplyError> {
            let mut runtime =
                lock_x11_control_runtime(&self.state.runtime, &self.state.control_runtime_pending)
                    .unwrap();
            x11_apply_focus_change(
                &mut runtime,
                namespace(),
                client(),
                &self.projection,
                Some(&self.private.broker.registry),
                Some(claim),
                change,
            )
        }
        fn focus_out(&self, claim: &PrivateFocusClaim) -> X11DependentFocusEffect {
            x11_apply_dependent_focus_out(
                namespace(),
                client(),
                claim.issued.window,
                &self.projection,
                Some(&self.private.broker.registry),
                Some(claim),
            )
            .unwrap()
        }
        fn published(&self) -> bool {
            self.private
                .broker
                .registry
                .private_applied
                .get()
                .unwrap()
                .publication
                .lock()
                .unwrap()
                .published
        }
    }

    #[test]
    fn queued_claim_does_not_publish_until_the_actual_focus_producer_applies() {
        let fixture = fixture();
        assert!(
            fixture
                .channels
                .as_ref()
                .unwrap()
                .control
                .try_recv()
                .is_err()
        );
        let claim = fixture.reserve(window());
        let (sender, receiver) = sync_channel(1);
        sender.send(claim.clone()).unwrap();
        XServerFrontendRouteRegistry::record_private_focus_queued(&claim);
        let dependent = fixture
            .private
            .broker
            .registry
            .private_focus_dependency(client(), window())
            .unwrap()
            .unwrap();
        assert_eq!(dependent.issued.generation, claim.issued.generation);
        assert!(!fixture.published());
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            root().local.raw()
        );
        let reached = fixture
            .apply(
                &receiver.try_recv().unwrap(),
                X11FocusChange::Surface { window: window() },
            )
            .unwrap();
        assert_eq!(reached.previous_authority, root());
        assert_eq!(
            fixture
                .state
                .runtime
                .lock()
                .unwrap()
                .input_focus(namespace()),
            (window(), 1)
        );
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            window().local.raw()
        );
        assert!(fixture.published());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn a_pending_generation_cannot_later_clear_a_newer_same_window_focus() {
        let fixture = fixture();
        let old = fixture.reserve(window());
        XServerFrontendRouteRegistry::record_private_focus_queued(&old);
        // Captured before old focus applies: sampling applied revision here
        // would have no evidence to carry to the dependent writer.
        let dependent = fixture
            .private
            .broker
            .registry
            .private_focus_dependency(client(), window())
            .unwrap()
            .unwrap();
        fixture
            .apply(&old, X11FocusChange::Surface { window: window() })
            .unwrap();
        let newer = fixture.reserve(window());
        fixture
            .apply(&newer, X11FocusChange::Surface { window: window() })
            .unwrap();
        let before = fixture
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
        assert_eq!(
            fixture.focus_out(&dependent),
            X11DependentFocusEffect::Superseded
        );
        assert!(fixture.published());
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
            before
        );
    }

    #[test]
    fn matching_dependent_clear_invalidates_agreement_until_the_target_writer_applies() {
        let fixture = fixture();
        let old = fixture.reserve(window());
        fixture
            .apply(&old, X11FocusChange::Surface { window: window() })
            .unwrap();
        assert_eq!(
            fixture.focus_out(&old),
            X11DependentFocusEffect::ProjectionCleared
        );
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            root().local.raw()
        );
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
        assert!(!fixture.published());
        let clear = fixture.reserve(root());
        fixture.apply(&clear, X11FocusChange::Clear).unwrap();
        assert!(fixture.published());
        let owner = fixture
            .private
            .broker
            .registry
            .private_applied
            .get()
            .unwrap();
        let state = owner.publication.lock().unwrap();
        assert_eq!(state.focus, None);
        assert_eq!((state.focus_window, state.focus_revert_to), (root(), 1));
    }

    #[test]
    fn a_superseded_focus_command_refuses_without_replacing_the_newer_publication() {
        let fixture = fixture();
        let old = fixture.reserve(window());
        let newer = fixture.reserve(root());
        fixture.apply(&newer, X11FocusChange::Clear).unwrap();
        assert_eq!(
            fixture.apply(&old, X11FocusChange::Surface { window: window() }),
            Err(X11FocusApplyError::Superseded)
        );
        assert!(fixture.published());
        assert_eq!(
            fixture
                .state
                .runtime
                .lock()
                .unwrap()
                .input_focus(namespace()),
            (root(), 1)
        );
    }

    #[test]
    fn core_focus_preserves_none_root_window_and_exact_revert_semantics() {
        let fixture = fixture();
        for (window, revert_to) in [(XResourceId::new(0, 1), 0), (root(), 2), (window(), 1)] {
            let claim = fixture.reserve(window);
            fixture
                .apply(&claim, X11FocusChange::Core { window, revert_to })
                .unwrap();
            assert_eq!(
                fixture
                    .state
                    .runtime
                    .lock()
                    .unwrap()
                    .input_focus(namespace()),
                (window, revert_to)
            );
            assert_eq!(
                fixture.projection.load(Ordering::Acquire),
                window.local.raw()
            );
            let state = fixture
                .private
                .broker
                .registry
                .private_applied
                .get()
                .unwrap()
                .publication
                .lock()
                .unwrap();
            assert_eq!(
                (state.focus_window, state.focus_revert_to),
                (window, revert_to)
            );
            assert_eq!(state.focus.is_some(), window.local.raw() != 0);
        }
        let invalid = fixture.reserve(window());
        assert_eq!(
            fixture.apply(
                &invalid,
                X11FocusChange::Core {
                    window: window(),
                    revert_to: 3
                }
            ),
            Err(X11FocusApplyError::Runtime(
                crate::XAuthorityRuntimeError::InvalidResource
            ))
        );
        assert_eq!(
            fixture
                .state
                .runtime
                .lock()
                .unwrap()
                .input_focus(namespace()),
            (window(), 1)
        );
        assert!(!fixture.published());
    }

    #[test]
    fn refused_native_change_keeps_write_ahead_generation_against_old_dependent_effects() {
        let fixture = fixture();
        let old = fixture.reserve(window());
        fixture
            .apply(&old, X11FocusChange::Surface { window: window() })
            .unwrap();
        let missing = XResourceId::new(0x299999, 1);
        let failed = fixture.reserve(missing);
        assert!(matches!(
            fixture.apply(&failed, X11FocusChange::Surface { window: missing }),
            Err(X11FocusApplyError::Runtime(_))
        ));
        assert!(!fixture.published());
        assert_eq!(fixture.focus_out(&old), X11DependentFocusEffect::Superseded);
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            window().local.raw()
        );
    }

    #[test]
    fn colliding_foreign_claim_cannot_mutate_the_registered_projection() {
        let first = fixture();
        let second = fixture();
        let foreign = first.reserve(window());
        assert_eq!(
            second.apply(&foreign, X11FocusChange::Surface { window: window() }),
            Err(X11FocusApplyError::State(
                PrivateAppliedRegistryRefusal::ForeignOrigin
            ))
        );
        assert!(!second.published());
        assert_eq!(
            second.projection.load(Ordering::Acquire),
            root().local.raw()
        );
        assert!(Arc::ptr_eq(
            &second
                .registration
                .connection_state
                .get()
                .unwrap()
                .selections,
            &second.selections
        ));
    }

    #[test]
    fn a_private_dependent_without_generation_is_unproved_and_cannot_clear() {
        let fixture = fixture();
        let claim = fixture.reserve(window());
        fixture
            .apply(&claim, X11FocusChange::Surface { window: window() })
            .unwrap();
        assert_eq!(
            x11_apply_dependent_focus_out(
                namespace(),
                client(),
                window(),
                &fixture.projection,
                Some(&fixture.private.broker.registry),
                None
            ),
            Ok(X11DependentFocusEffect::Unproved)
        );
        assert!(fixture.published());
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            window().local.raw()
        );
    }

    #[test]
    fn focus_claim_exhaustion_refuses_without_wrap_or_applied_mutation() {
        let fixture = fixture();
        fixture
            .private
            .broker
            .registry
            .private_applied
            .get()
            .unwrap()
            .next_focus_claim
            .store(u64::MAX, Ordering::Release);
        assert!(matches!(
            fixture
                .private
                .broker
                .registry
                .reserve_private_focus(client(), window()),
            Err(PrivateAppliedRegistryRefusal::FocusIdentityExhausted)
        ));
        assert!(!fixture.published());
    }
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
            sequence: 3,
            major_opcode: 42,
            client_id: client().raw(),
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
            fixture.state.runtime.clone(),
            fixture.state.control_runtime_pending.clone(),
            stream.clone(),
            priority.clone(),
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

    #[test]
    fn focus_storage_preparation_does_not_reset_an_existing_native_focus_or_publish_it() {
        let fixture = fixture();
        let mut runtime = fixture.state.runtime.lock().unwrap();
        runtime.prepare_input_focus_namespace(namespace());
        assert_eq!(runtime.input_focus(namespace()), (root(), 1));
        runtime.set_input_focus(namespace(), window(), 2).unwrap();
        runtime.prepare_input_focus_namespace(namespace());
        assert_eq!(runtime.input_focus(namespace()), (window(), 2));
        assert!(
            !fixture
                .private
                .broker
                .registry
                .private_applied
                .get()
                .unwrap()
                .publication
                .lock()
                .unwrap()
                .published
        );
    }
    include!("private_focus_destruction.rs");
    include!("private_focus_lifecycle.rs");
}
