use super::private_maintenance_scheduler::{Exit, MaintainedService};
use super::*;

fn command(kind: XAuthorityControlKind, surface: SurfaceId) -> XAuthorityControlCommand {
    use XAuthorityControlCommand as Command;
    use XAuthorityControlKind as Kind;
    let transaction = TransactionId::from_raw(99882);
    let geometry = Rect {
        x: 2,
        y: 3,
        width: 80,
        height: 60,
    };
    let state = sophia_protocol::PolicyPresentationState {
        fullscreen: true,
        maximized: false,
        minimized: false,
    };
    match kind {
        Kind::PublishMetadataRule => Command::PublishMetadataRule {
            transaction,
            surface,
            rule: MetadataDisclosureRule {
                surface,
                disclosure: sophia_protocol::MetadataDisclosure::None,
                trust_level: sophia_protocol::TrustLevel::Unknown,
                icon: None,
                generation: 37,
            },
        },
        Kind::AdmitSurface => Command::AdmitSurface {
            transaction,
            surface,
            geometry,
        },
        Kind::ConfigureSurface => Command::ConfigureSurface {
            transaction,
            surface,
            geometry,
        },
        Kind::SetPresentationState => Command::SetPresentationState {
            transaction,
            surface,
            state,
        },
        Kind::RestorePresentationState => Command::RestorePresentationState {
            transaction,
            surface,
            state,
        },
        Kind::FocusSurface => Command::FocusSurface {
            transaction,
            surface,
        },
        Kind::ClearFocus => Command::ClearFocus {
            transaction,
            surface,
        },
        Kind::CloseSurface => Command::CloseSurface {
            transaction,
            surface,
        },
        Kind::WithdrawSurface => Command::WithdrawSurface {
            transaction,
            surface,
        },
    }
}

fn cleanup_actual_effect(kind: XAuthorityControlKind) {
    let service = MaintainedService::launch(Exit::Stop);
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&service.path);
    let window = handshake_ids(&mut client) | 0x0e01;
    let custody = wait_attached(&service.registry);
    let source = custody
        .cleanup_record()
        .connection_state
        .get()
        .unwrap()
        .control_source
        .get()
        .unwrap()
        .upgrade()
        .unwrap();
    if kind == XAuthorityControlKind::AdmitSurface {
        source.state.set_policy_map_deferred(true).unwrap();
    }
    let (surface, _) = selecting_window(&mut client, &service.transactions, window, 0);
    let resource = XResourceId::new(u64::from(window), 1);
    if kind == XAuthorityControlKind::ClearFocus {
        // Establish a real non-root focus before testing a real clear.
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
                        transaction: TransactionId::from_raw(99880),
                        surface,
                    },
                },
            )
            .unwrap();
        let ack = service.acks.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            ack.acknowledgement.outcome,
            XAuthorityControlOutcome::Delivered
        );
        assert_eq!(
            source
                .state
                .runtime
                .lock()
                .unwrap()
                .input_focus(source.endpoint.namespace)
                .0,
            resource
        );
    }
    source.fail_after_effect.store(true, Ordering::Release);
    let owner = service.owner.clone();
    service
        .access
        .control_producer(&owner.lease())
        .unwrap()
        .submit(
            &owner.lease(),
            XAuthorityClientControlCommand {
                client: source.endpoint.client,
                command: command(kind, surface),
            },
        )
        .unwrap();
    let completion = service.registry.control_completion().unwrap();
    let cleanup = waited_for_value(|| completion.cleanups_owed().unwrap().into_iter().next())
        .expect("the actual writer is interrupted after its first source effect");
    assert_eq!(cleanup.command.command.kind(), kind);
    let execution = completion.execution_of(cleanup.token).unwrap();
    assert!(Arc::ptr_eq(&execution.lock().unwrap().source, &source));
    assert!(!execution.lock().unwrap().peer_generation_begun);
    assert!(
        service.acks.try_recv().is_err(),
        "the interruption produced no acknowledgement"
    );
    match kind {
        XAuthorityControlKind::PublishMetadataRule => {
            assert_eq!(
                source
                    .tables
                    .rules
                    .lock()
                    .unwrap()
                    .get(&surface)
                    .unwrap()
                    .generation,
                37
            );
            assert!(
                !source
                    .tables
                    .generations
                    .lock()
                    .unwrap()
                    .contains_key(&surface)
            );
        }
        XAuthorityControlKind::AdmitSurface | XAuthorityControlKind::ConfigureSurface => {
            assert_eq!(
                source
                    .state
                    .runtime
                    .lock()
                    .unwrap()
                    .window_geometry(source.endpoint.namespace, resource)
                    .unwrap()
                    .width,
                80
            );
            assert_eq!(
                source
                    .endpoint
                    .registration
                    .get()
                    .unwrap()
                    .selections
                    .lock()
                    .unwrap()
                    .geometry(resource)
                    .unwrap()
                    .width,
                8
            );
        }
        XAuthorityControlKind::SetPresentationState
        | XAuthorityControlKind::RestorePresentationState => {
            assert!(
                !source
                    .state
                    .properties
                    .lock()
                    .unwrap()
                    .properties_for_window(source.endpoint.namespace, resource)
                    .is_empty()
            );
        }
        XAuthorityControlKind::FocusSurface => {
            assert_eq!(
                source
                    .state
                    .runtime
                    .lock()
                    .unwrap()
                    .input_focus(source.endpoint.namespace)
                    .0,
                resource
            );
        }
        XAuthorityControlKind::ClearFocus => {
            assert_eq!(
                source
                    .state
                    .runtime
                    .lock()
                    .unwrap()
                    .input_focus(source.endpoint.namespace)
                    .0
                    .local
                    .raw(),
                u64::from(X_SETUP_DEFAULT_ROOT)
            );
        }
        XAuthorityControlKind::WithdrawSurface => {
            assert_eq!(
                source
                    .state
                    .runtime
                    .lock()
                    .unwrap()
                    .window_map_state(source.endpoint.namespace, resource)
                    .unwrap(),
                crate::XMapState::Unmapped
            );
        }
        XAuthorityControlKind::CloseSurface => {
            assert!(
                waited_for(|| source.teardown.lock().unwrap().finished),
                "owned shutdown reaches actual teardown"
            );
        }
    }
    drop(client);
    assert!(waited_for(|| source.teardown.lock().unwrap().finished));
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
    let actual = source.teardown.lock().unwrap().removed.take().unwrap();
    assert!(actual.resources.destroyed_windows.contains(&resource));
    for _ in 0..400 {
        let _ = super::final_custody_step(&service);
    }
    assert_eq!(
        completion.state_of(cleanup.token),
        ControlRecordState::Outstanding
    );
    assert!(service.owner.store.reserved().unwrap() > 0);
    assert_eq!(service.owner.custodies_kept(), 1);
    source.teardown.lock().unwrap().removed = Some(actual);
    for _ in 0..850 {
        let _ = super::final_custody_step(&service);
    }
    assert_eq!(
        completion.state_of(cleanup.token),
        ControlRecordState::Retired
    );
    assert_eq!(service.owner.store.reserved(), Some(0));
    assert!(!source.tables.windows.lock().unwrap().contains_key(&surface));
    assert!(!source.tables.rules.lock().unwrap().contains_key(&surface));
    assert!(
        !source
            .tables
            .generations
            .lock()
            .unwrap()
            .contains_key(&surface)
    );
    assert!(
        service.acks.try_recv().is_err(),
        "cleanup never fabricates an outcome"
    );
    drop(custody);
    service.finish();
}

#[test]
fn metadata_rule_cleanup_uses_actual_insertion_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::PublishMetadataRule);
}
#[test]
fn admit_cleanup_uses_actual_runtime_change_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::AdmitSurface);
}
#[test]
fn presentation_cleanup_uses_actual_property_change_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::SetPresentationState);
}
#[test]
fn restore_cleanup_uses_actual_property_change_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::RestorePresentationState);
}
#[test]
fn focus_cleanup_uses_actual_claim_application_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::FocusSurface);
}
#[test]
fn clear_focus_cleanup_uses_actual_claim_application_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::ClearFocus);
}
#[test]
fn close_cleanup_uses_actual_owned_shutdown_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::CloseSurface);
}
#[test]
fn withdraw_cleanup_uses_actual_unmap_and_original_teardown() {
    cleanup_actual_effect(XAuthorityControlKind::WithdrawSurface);
}

fn completed_source_fixture() -> (
    MaintainedService,
    UnixStream,
    SurfaceId,
    Arc<PrivateControlClientSource>,
) {
    let service = MaintainedService::launch(Exit::Stop);
    service.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&service.path);
    let window = handshake_ids(&mut client) | 0x0e21;
    let (surface, _) = selecting_window(&mut client, &service.transactions, window, 1 << 17);
    let custody = wait_attached(&service.registry);
    let source = custody
        .cleanup_record()
        .connection_state
        .get()
        .unwrap()
        .control_source
        .get()
        .unwrap()
        .upgrade()
        .unwrap();
    (service, client, surface, source)
}

fn collect_after_source_teardown(
    service: &MaintainedService,
    client: UnixStream,
    source: &PrivateControlClientSource,
) {
    drop(client);
    assert!(waited_for(|| source.teardown.lock().unwrap().finished));
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

#[test]
fn generated_local_control_records_survive_interruption_until_exact_recipient_termination() {
    let (service, client, surface, source) = completed_source_fixture();
    source.fail_before_write.store(true, Ordering::Release);
    let owner = service.owner.clone();
    service
        .access
        .control_producer(&owner.lease())
        .unwrap()
        .submit(
            &owner.lease(),
            configure(source.endpoint.client, surface, 99883),
        )
        .unwrap();
    let completion = service.registry.control_completion().unwrap();
    let cleanup =
        waited_for_value(|| completion.cleanups_owed().unwrap().into_iter().next()).unwrap();
    assert_eq!(cleanup.steps.projection, ControlStepState::Completed);
    let execution = completion.execution_of(cleanup.token).unwrap();
    let original = {
        let held = execution.lock().unwrap();
        assert!(held.emission == PrivateControlEmission::Pending);
        assert!(!held.peer_generation_begun);
        assert!(held.generated_events.iter().any(|(_, event)| matches!(
            event,
            XClientEvent::ConfigureNotify {
                width: 80,
                height: 60,
                ..
            }
        )));
        assert!(held.records.iter().any(|bytes| bytes[0] == 22));
        held.records.clone()
    };
    collect_after_source_teardown(&service, client, &source);
    assert_eq!(execution.lock().unwrap().records, original);
    for _ in 0..850 {
        let _ = super::final_custody_step(&service);
    }
    assert_eq!(
        completion.state_of(cleanup.token),
        ControlRecordState::Retired
    );
    assert_eq!(service.owner.store.reserved(), Some(0));
    assert!(service.acks.try_recv().is_err());
    service.finish();
}

#[test]
fn refused_metadata_publication_keeps_its_actual_candidate_after_native_teardown() {
    let (service, client, surface, source) = completed_source_fixture();
    // Labelled pressure fixture: fill the original publication channel before
    // the real source derives and attempts its own candidate.
    let filler = crate::XAuthorityClientMetadataCandidate {
        client: source.endpoint.client,
        candidate: sophia_protocol::ReducedMetadataCandidate {
            surface,
            label: None,
            disclosure: sophia_protocol::MetadataDisclosure::None,
            generation: 999,
        },
    };
    let full = (0..4096).any(|_| {
        matches!(
            service
                .registry
                .metadata_candidate_sender
                .try_send(filler.clone()),
            Err(TrySendError::Full(_))
        )
    });
    assert!(full);
    let owner = service.owner.clone();
    service
        .access
        .control_producer(&owner.lease())
        .unwrap()
        .submit(
            &owner.lease(),
            XAuthorityClientControlCommand {
                client: source.endpoint.client,
                command: command(XAuthorityControlKind::PublishMetadataRule, surface),
            },
        )
        .unwrap();
    let completion = service.registry.control_completion().unwrap();
    let cleanup =
        waited_for_value(|| completion.cleanups_owed().unwrap().into_iter().next()).unwrap();
    let execution = completion.execution_of(cleanup.token).unwrap();
    let original = execution.lock().unwrap().pending_metadata.clone().unwrap();
    assert_eq!(original.surface, surface);
    assert_eq!(original.generation, 1);
    collect_after_source_teardown(&service, client, &source);
    for _ in 0..850 {
        let _ = super::final_custody_step(&service);
    }
    assert_eq!(
        completion.state_of(cleanup.token),
        ControlRecordState::Outstanding
    );
    assert_eq!(
        execution.lock().unwrap().pending_metadata.as_ref(),
        Some(&original)
    );
    assert!(service.owner.store.reserved().unwrap() > 0);
    assert_eq!(service.owner.custodies_kept(), 1);
    assert!(service.acks.try_recv().is_err());
    service.finish();
}
