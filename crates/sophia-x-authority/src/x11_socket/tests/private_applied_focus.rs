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
        /// Last field, so it is dropped after the instance it keeps for.
        _keeper: crate::PrivateServiceOwner,
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
    /// Maps a focus fixture's window so it is genuinely viewable.
    ///
    /// X11 refuses to focus a window that is not viewable, which means the
    /// window and every ancestor mapped. Creating one leaves it `Unmapped`,
    /// so a fixture that only creates its window is focusing something the
    /// protocol would refuse, and any rule that reads viewability fails it
    /// for the fixture's reason rather than the rule's.
    fn map_focus_test_window(runtime: &Mutex<XAuthorityRuntime>, target: XResourceId) {
        let mut runtime = runtime.lock().unwrap();
        let response = runtime.apply(crate::XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(812),
            namespace: namespace(),
            kind: crate::XAuthorityRequestKind::MapWindow {
                window: target,
                generation: 1,
            },
        });
        assert_eq!(response.outcome, crate::XAuthorityResponseOutcome::Accepted);
        assert_eq!(
            Ok(crate::XMapState::Viewable),
            runtime.window_map_state(namespace(), target),
            "a focus fixture's window has to be viewable or it is testing the wrong refusal"
        );
    }
    fn fixture() -> Fixture {
        fixture_with_focus_preparation(true)
    }

    #[test]
    fn every_focus_fixture_window_is_viewable_before_anything_focuses_it() {
        let fixture = fixture();
        let child = XResourceId::new(0x200_9111, 1);
        create_focus_test_window(&fixture, child, window());
        let runtime = fixture.state.runtime.lock().unwrap();
        for (name, target) in [("the fixture window", window()), ("a child of it", child)] {
            assert_eq!(
                Ok(crate::XMapState::Viewable),
                runtime.window_map_state(namespace(), target),
                "{name} is not viewable, so a viewability rule would refuse it"
            );
        }
    }
    fn fixture_with_focus_preparation(prepare: bool) -> Fixture {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
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
        map_focus_test_window(&state.runtime, window());
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
            _keeper: service_keeper,
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
    include!("private_focus_real_writer.rs");

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

    #[test]
    fn the_prepared_focus_is_published_when_the_runtime_agrees_it_is_on_the_root() {
        let fixture = fixture();
        assert!(!fixture.published(), "installation alone applies nothing");

        let runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .publish_prepared_focus(&runtime),
            Ok(true)
        );
        assert!(fixture.published());
        // Once. Saying it again is not a second application.
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .publish_prepared_focus(&runtime),
            Ok(false)
        );
        assert!(fixture.published());
    }

    #[test]
    fn a_retained_focus_on_a_window_is_not_published_by_preparation() {
        let fixture = fixture();
        {
            // What an earlier invocation left behind: focus on a window this
            // owner has no route for. Publishing the root over it would route
            // by a focus the runtime does not have.
            let mut runtime = fixture.state.runtime.lock().unwrap();
            runtime.set_input_focus(namespace(), window(), 2).unwrap();
        }

        let runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .publish_prepared_focus(&runtime),
            Ok(false)
        );
        assert!(!fixture.published());
    }

    #[test]
    fn a_focus_change_already_begun_owns_publication_over_preparation() {
        let fixture = fixture();
        let claim = fixture.reserve(window());
        fixture
            .apply(&claim, X11FocusChange::Surface { window: window() })
            .unwrap();
        let before = fixture.published();

        let runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .publish_prepared_focus(&runtime),
            Ok(false)
        );
        assert_eq!(fixture.published(), before);
    }
}
