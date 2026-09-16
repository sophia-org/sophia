/// Production-type composition controls: these call the same attachment as
/// dispatch setup and use real registration/participant ownership. They do not
/// create a socket connection or claim complete dispatch lifetime coverage.
mod private_applied_registry {
    use super::*;

    fn namespace() -> NamespaceId {
        NamespaceId::from_raw(77)
    }
    fn client() -> XServerFrontendClientId {
        XServerFrontendClientId::from_raw(700)
    }
    fn selections() -> Arc<Mutex<XCoreEventSelectionState>> {
        Arc::new(Mutex::new(XCoreEventSelectionState::default()))
    }
    fn focus() -> Arc<AtomicU64> {
        Arc::new(AtomicU64::new(u64::from(X_SETUP_DEFAULT_ROOT)))
    }
    fn register(private: &PrivateXServerFrontend) -> XServerFrontendClientRouteRegistration {
        let admission = namespaced(client(), namespace());
        private.participant.admit(client(), admission).unwrap();
        private
            .broker
            .registry
            .register_client_with_admission(client(), Some(admission))
            .unwrap()
            .0
    }
    fn install(private: &PrivateXServerFrontend) {
        private
            .broker
            .registry
            .install_private_applied(&private.participant, namespace())
            .unwrap();
    }

    #[test]
    fn setup_before_runner_preparation_retains_actual_state_and_does_not_publish_focus() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let registration = register(&private);
        let selections = selections();
        let focus = focus();
        private
            .broker
            .registry
            .attach_connection_state(
                &registration,
                namespace(),
                selections.clone(),
                focus.clone(),
            )
            .unwrap();
        assert!(selections.lock().unwrap().private_origin.is_none());
        let window = XResourceId::new(800, 1);
        selections.lock().unwrap().update(window, Some(1), None);
        focus.store(window.local.raw(), Ordering::Release);
        let mut runner = private
            .prepare_runner(namespace(), &service_keeper)
            .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
        let private = runner.frontend.as_ref().unwrap();
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                let registered = private
                    .broker
                    .registry
                    .applied_client(&clients, client(), &bindings.bound[&client()])
                    .unwrap();
                assert!(Arc::ptr_eq(&registered.connection.selections, &selections));
                let selected = registered.lock_selections().unwrap();
                assert!(selected.selects(window, 1));
                assert_eq!(
                    registered.focused_projection().load(Ordering::Acquire),
                    window.local.raw()
                );
                let publication = registered.lock_publication().unwrap();
                assert!(
                    !publication.published,
                    "retaining the focus atomic is not applied publication"
                );
            })
            .unwrap();
        assert!(runner.ingress_for(client(), DeviceId::from_raw(1)).is_ok());
    }

    #[test]
    fn setup_after_runner_preparation_binds_once_and_cannot_replace_its_projection() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let mut runner = private
            .prepare_runner(namespace(), &service_keeper)
            .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
        let private = runner.frontend.as_ref().unwrap();
        let registration = register(private);
        let selections = selections();
        let focus = focus();
        private
            .broker
            .registry
            .attach_connection_state(
                &registration,
                namespace(),
                selections.clone(),
                focus.clone(),
            )
            .unwrap();
        assert_eq!(
            selections.lock().unwrap().private_origin,
            Some(PrivateAppliedSelectionOrigin {
                authority: private.controller.identity().unwrap(),
                namespace: namespace(),
                client: client(),
            })
        );
        private
            .broker
            .registry
            .attach_connection_state(
                &registration,
                namespace(),
                selections.clone(),
                focus.clone(),
            )
            .unwrap();
        assert_eq!(
            private.broker.registry.attach_connection_state(
                &registration,
                namespace(),
                self::selections(),
                focus
            ),
            Err(PrivateAppliedRegistryRefusal::DifferentConnectionState)
        );
        assert!(runner.ingress_for(client(), DeviceId::from_raw(1)).is_ok());
    }

    #[test]
    fn runner_preparation_refuses_unreadable_connection_state_and_returns_the_same_frontend() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let registration = register(&private);
        let selections = selections();
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections.clone(), focus())
            .unwrap();
        let original_clients = private.broker.registry.clients.clone();
        let original_authority = private.controller.identity().unwrap();
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _held = selections.lock().unwrap();
            panic!("connection selections interrupted before runner preparation");
        }));
        let (cause, returned) = match private.prepare_runner(namespace(), &service_keeper) {
            Ok(_) => panic!("runner must refuse unreadable connection state"),
            Err(refused) => refused,
        };
        assert_eq!(cause, PrivateRunnerRefusal::StateUnavailable);
        assert!(Arc::ptr_eq(
            &original_clients,
            &returned.broker.registry.clients
        ));
        assert_eq!(returned.controller.identity().unwrap(), original_authority);
        assert!(!returned.ordered_runner);
        assert!(!returned.keyboards_issued.load(Ordering::Acquire));
        assert!(returned.native_owner.is_none());
        assert!(selections.is_poisoned());
        assert!(
            returned
                .broker
                .registry
                .clients
                .lock()
                .unwrap()
                .contains_key(&client())
        );
        assert!(
            !returned
                .broker
                .registry
                .private_applied
                .get()
                .unwrap()
                .ready
                .load(Ordering::Acquire)
        );
    }

    #[test]
    fn registration_drop_removes_discoverability_and_owns_no_historical_state_map() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        install(&private);
        let registration = register(&private);
        let selections = selections();
        let focus = focus();
        let weak_selections = Arc::downgrade(&selections);
        let weak_slot = Arc::downgrade(&registration.connection_state);
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections, focus)
            .unwrap();
        assert!(weak_selections.upgrade().is_some());
        drop(registration);
        assert!(weak_slot.upgrade().is_none());
        assert!(weak_selections.upgrade().is_none());
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::MissingClient)
                ));
            })
            .unwrap();
    }

    #[test]
    fn foreign_registration_cannot_attach_under_colliding_client_names() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let first = private_for_roles(&service_keeper);
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let second = private_for_roles(&service_keeper);
        let _first_registration = register(&first);
        let second_registration = register(&second);
        assert_eq!(
            first.broker.registry.attach_connection_state(
                &second_registration,
                namespace(),
                selections(),
                focus()
            ),
            Err(PrivateAppliedRegistryRefusal::ForeignOrigin)
        );
        assert!(
            first.broker.registry.clients.lock().unwrap()[&client()]
                .connection_state
                .get()
                .is_none()
        );
    }

    #[test]
    fn colliding_origins_keep_their_actual_projection_and_refuse_a_foreign_guard() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let first = private_for_roles(&service_keeper);
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let second = private_for_roles(&service_keeper);
        let first_registration = register(&first);
        let second_registration = register(&second);
        let first_selections = selections();
        let second_selections = selections();
        let window = XResourceId::new(800, 1);
        first_selections
            .lock()
            .unwrap()
            .update(window, Some(1), None);
        second_selections
            .lock()
            .unwrap()
            .update(window, Some(2), None);
        first
            .broker
            .registry
            .attach_connection_state(&first_registration, namespace(), first_selections, focus())
            .unwrap();
        second
            .broker
            .registry
            .attach_connection_state(
                &second_registration,
                namespace(),
                second_selections,
                focus(),
            )
            .unwrap();
        install(&first);
        install(&second);
        first
            .participant
            .under_boundary(|_, _, bindings| {
                let first_clients = first.broker.registry.clients.lock().unwrap();
                let second_clients = second.broker.registry.clients.lock().unwrap();
                let binding = &bindings.bound[&client()];
                let reached = first
                    .broker
                    .registry
                    .applied_client(&first_clients, client(), binding)
                    .unwrap();
                assert!(reached.lock_selections().unwrap().selects(window, 1));
                assert!(matches!(
                    first
                        .broker
                        .registry
                        .applied_client(&second_clients, client(), binding),
                    Err(PrivateAppliedRegistryRefusal::ForeignOrigin)
                ));
            })
            .unwrap();
        second
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = second.broker.registry.clients.lock().unwrap();
                let reached = second
                    .broker
                    .registry
                    .applied_client(&clients, client(), &bindings.bound[&client()])
                    .unwrap();
                assert!(reached.lock_selections().unwrap().selects(window, 2));
            })
            .unwrap();
        assert!(matches!(
            first
                .broker
                .registry
                .install_private_applied(&second.participant, namespace()),
            Err(PrivateAppliedRegistryRefusal::ForeignOrigin)
        ));
    }

    #[test]
    fn stored_admission_cannot_substitute_for_a_replacement_or_closed_boundary_binding() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let registration = register(&private);
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections(), focus())
            .unwrap();
        install(&private);
        private.participant.revoke_namespace(namespace()).unwrap();
        lifecycle_drain(&private.terminal.lifecycle);
        let mut replacement = namespaced(client(), namespace());
        replacement.client_id = ClientAdmissionId::from_raw(client().raw() + 1);
        private.participant.admit(client(), replacement).unwrap();
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::ForeignOrigin)
                ));
                // Composition of an interrupted revocation retaining its closed row.
                bindings.bound.get_mut(&client()).unwrap().closed = true;
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::AdmissionClosed)
                ));
            })
            .unwrap();
    }

    #[test]
    fn preparation_failure_keeps_partial_binding_unavailable_without_replacing_its_origin() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let registration = register(&private);
        let selections = selections();
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections.clone(), focus())
            .unwrap();
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _held = selections.lock().unwrap();
            panic!("connection selections interrupted");
        }));
        assert!(matches!(
            private
                .broker
                .registry
                .install_private_applied(&private.participant, namespace()),
            Err(PrivateAppliedRegistryRefusal::SelectionUnavailable)
        ));
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::PreparationIncomplete)
                ));
            })
            .unwrap();
        assert!(
            !private
                .broker
                .registry
                .private_applied
                .get()
                .unwrap()
                .ready
                .load(Ordering::Acquire)
        );
    }

    #[test]
    fn missing_setup_and_unreadable_publication_remain_distinct_refusals() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let registration = register(&private);
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::NoPrivateOwner)
                ));
            })
            .unwrap();
        install(&private);
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::MissingConnectionState)
                ));
            })
            .unwrap();
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections(), focus())
            .unwrap();
        let publication = &private
            .broker
            .registry
            .private_applied
            .get()
            .unwrap()
            .publication;
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _held = publication.lock().unwrap();
            panic!("publication interrupted");
        }));
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                let reached = private
                    .broker
                    .registry
                    .applied_client(&clients, client(), &bindings.bound[&client()])
                    .unwrap();
                assert!(matches!(
                    reached.lock_publication(),
                    Err(PrivateAppliedRegistryRefusal::PublicationUnavailable)
                ));
            })
            .unwrap();
    }

    #[test]
    fn registration_without_admission_cannot_borrow_private_projection_authority() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let admission = namespaced(client(), namespace());
        private.participant.admit(client(), admission).unwrap();
        let (registration, _channels) = private.broker.registry.register_client(client()).unwrap();
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections(), focus())
            .unwrap();
        install(&private);
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private.broker.registry.clients.lock().unwrap();
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::MissingAdmission)
                ));
            })
            .unwrap();
    }

    #[test]
    fn poison_is_unavailable_even_when_a_caller_can_reach_retained_table_storage() {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = private_for_roles(&service_keeper);
        let registration = register(&private);
        private
            .broker
            .registry
            .attach_connection_state(&registration, namespace(), selections(), focus())
            .unwrap();
        install(&private);
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _held = private.broker.registry.clients.lock().unwrap();
            panic!("client table interrupted");
        }));
        private
            .participant
            .under_boundary(|_, _, bindings| {
                let clients = private
                    .broker
                    .registry
                    .clients
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                assert!(
                    clients.contains_key(&client()),
                    "storage is retained but not execution permission"
                );
                assert!(matches!(
                    private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()]
                    ),
                    Err(PrivateAppliedRegistryRefusal::RegistryUnavailable)
                ));
            })
            .unwrap();
        assert!(matches!(
            private
                .broker
                .registry
                .install_private_applied(&private.participant, namespace()),
            Err(PrivateAppliedRegistryRefusal::RegistryUnavailable)
        ));
    }
}
