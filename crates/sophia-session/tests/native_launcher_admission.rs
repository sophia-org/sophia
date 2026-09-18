#![cfg(feature = "native-session")]
//! Real socket/store intake and the shared Session admission sequence. Renderer
//! completion/protection are supplied. Only the explicit process control spawns
//! a device-hidden /bin/true child; it opens no display connection.
use sophia_engine::PresentedContentTarget;
use sophia_protocol::*;
use sophia_runtime::ShellTransportError;
use sophia_session::session_actions::SessionLaunchQueue;
use sophia_session::shell_native_launcher::NativeLauncherActionService;
#[path = "support/content_actions/native_launcher_fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn keyboard_admission_is_once_and_independent_of_ack_order_or_disposition() {
    for ack_first in [false, true] {
        for disposition in [1, 2] {
            let mut h = Harness::new();
            let activation = h.accept();
            if ack_first {
                assert!(h.acknowledge(activation, disposition));
            }
            assert_eq!(h.activate(activation, 0).status, 1);
            if !ack_first {
                assert!(h.acknowledge(activation, disposition));
            }
            assert_eq!(h.queue.pending_len(), 1);
            assert!(!h.service(0));
            assert_eq!(h.activate(activation, 0).status, 2);
            assert_eq!(h.queue.pending_len(), 1);
            let payload = h.dispatch();
            assert_eq!(
                payload.cause,
                sophia_session::session_actions::CatalogLaunchCause::Transient(activation)
            );
            assert!(h.queue.native_catalog_admission(&payload));
            assert_ne!(payload.transaction, tx(20)); // client request transaction
            h.queue.cancel_catalog(payload.transaction);
            assert!(h.queue.native_catalog_admission(&payload));
            h.queue.cancel_native_catalog(&payload);
            assert!(!h.queue.native_catalog_admission(&payload));
        }
    }
}

#[test]
fn presented_catalog_and_issued_accept_are_both_required_before_queue_insertion() {
    for fault in 0..6 {
        let mut h = Harness::new();
        let mut activation = h.accept();
        match fault {
            0 => activation.event.binding.focus_lease += 1,
            1 => activation.event.event_id += 1,
            2 => activation.slot = 1, // visible, but keyboard selected slot is 2
            3 => activation.event.binding.catalog_generation += 1,
            4 => activation.event.binding.presentation_epoch += 1,
            5 => activation.event.binding.opening += 1,
            _ => unreachable!(),
        }
        assert_ne!(h.activate(activation, 0).status, 1);
        assert_eq!(h.queue.pending_len(), 0);
    }
}

#[test]
fn capacity_refusal_does_not_make_the_same_accept_replayable() {
    let mut h = Harness::new();
    let activation = h.accept();
    assert_eq!(
        h.activate(
            activation,
            sophia_session::session_actions::SESSION_ACTION_APPLICATION_CAPACITY
        )
        .status,
        5
    );
    assert_eq!(h.queue.pending_len(), 0);
    assert_eq!(h.activate(activation, 0).status, 2);
    assert_eq!(h.queue.pending_len(), 0);
}

#[test]
fn pointer_requires_actual_ledger_event_and_can_choose_an_unselected_visible_row() {
    for ack_first in [false, true] {
        let mut h = Harness::new();
        let event_id = h
            .service
            .issue(
                &mut h.peer.transport.connection(&mut h.epochs),
                h.target.clone(),
                tx(30),
                1,
            )
            .unwrap()
            .unwrap();
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        let (_, ShellContentRecord::Action(action)) =
            decode_shell_content_frame(&h.peer.read()).unwrap()
        else {
            panic!();
        };
        let activation = NativeLauncherActivation {
            event: NativeLauncherEvent {
                binding: h.focus,
                event_id,
                state_revision: 1,
            },
            cause: 2,
            slot: 1,
        };
        if ack_first {
            h.peer
                .send_content(ShellContentRecord::ActionAck(ContentActionAck {
                    grant: action.grant,
                    output: action.output,
                    candidate_generation: action.candidate_generation,
                    presentation_epoch: action.presentation_epoch,
                    interaction_generation: action.interaction_generation,
                    allocation: action.allocation,
                    target_id: action.target_id,
                    target_generation: action.target_generation,
                    action_id: action.action_id,
                    event_id: action.event_id,
                    disposition: 2,
                }));
            assert_eq!(
                h.service
                    .service_acks(&mut h.peer.transport.connection(&mut h.epochs), 1, 1)
                    .unwrap(),
                1
            );
        }
        let mut wrong = activation;
        wrong.event.event_id += 1;
        assert_eq!(h.activate(wrong, 0).status, 2);
        assert_eq!(h.queue.pending_len(), 0);
        assert_eq!(h.activate(activation, 0).status, 1);
        assert_eq!(h.queue.pending_len(), 1);
        assert_eq!(h.activate(activation, 0).status, 2);
    }
}

#[test]
fn revoked_exact_grant_cancels_native_queue_but_not_a_retained_worker_payload() {
    let mut h = Harness::new();
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let payload = h.dispatch();
    let mut wrong = GRANT;
    wrong.content_grant_epoch += 1;
    assert_eq!(h.queue.revoke_native_catalog_grant(wrong), 0);
    assert!(h.queue.native_catalog_admission(&payload));
    assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 1);
    assert!(!h.queue.native_catalog_admission(&payload));
    assert!(payload.entry.command.is_some());
    assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 0);
}

#[test]
fn saturated_socket_defers_admission_then_delivers_one_exact_queue_outcome() {
    let mut h = Harness::new();
    let activation = h.accept();
    let frame = encode_shell_content_frame(
        tx(40),
        &ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: GRANT,
            facts_generation: 5,
            outputs: vec![facts()],
        }),
    )
    .unwrap();
    let mut sent = 0;
    let mut full = false;
    for _ in 0..4096 {
        match h.peer.transport.send_async(&mut h.epochs, frame.clone()) {
            Ok(()) => sent += 1,
            Err(ShellTransportError::ActivationQueueSaturated) => {
                full = true;
                break;
            }
            Err(error) => panic!("unexpected backpressure: {error}"),
        }
    }
    assert!(full);
    h.peer.send(ShellNativeLauncherRecord::Activate(activation));
    assert!(!h.service(0));
    assert_eq!(h.queue.pending_len(), 0);
    for _ in 0..sent {
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        assert_eq!(h.peer.read(), frame);
    }
    assert!(h.service(0));
    assert_eq!(h.queue.pending_len(), 1);
    assert_eq!(h.outcome().status, 1);
    assert_eq!(h.activate(activation, 0).status, 2);
    assert_eq!(h.queue.pending_len(), 1);
}

#[test]
fn native_worker_returns_exact_queue_owner_without_restoring_revoked_authority() {
    use sophia_session::application_catalog::*;
    use std::sync::Arc;
    for changed in [false, true] {
        let mut h = Harness::new();
        let activation = h.accept();
        assert_eq!(h.activate(activation, 0).status, 1);
        let payload = h.dispatch();
        let registered = ["app1", "app2"].map(|name| RegisteredCatalogApplication {
            name: name.into(),
            command: ApplicationLaunchCommand {
                executable: std::env::current_exe().unwrap(),
                arguments: if changed {
                    vec!["changed".into()]
                } else {
                    vec![]
                },
                working_directory: None,
            },
        });
        let mut worker = ApplicationCatalogWorker::start(
            sophia_config::ApplicationCatalogConfig {
                name: "native-fixture".into(),
                sources: vec![],
                applications: vec!["app1".into(), "app2".into()],
                terminal: None,
                terminal_arguments: vec![],
            },
            registered.into(),
            ApplicationCatalogEnvironment {
                search_path: vec![],
                locale: "C".into(),
                current_desktop: vec![],
            },
        )
        .unwrap();
        assert!(worker.verify_native(Arc::clone(&payload)));
        assert!(!worker.verify_native(Arc::clone(&payload)));
        worker.request_shutdown();
        assert!(!worker.verify_native(Arc::clone(&payload)));
        assert!(!worker.poll_shutdown().unwrap()); // the result is still owned
        assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let result = loop {
            if let Some(result) = worker.poll() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        let ApplicationCatalogWorkerResult::NativeVerified(returned, command) = result else {
            panic!("wrong worker result");
        };
        assert!(Arc::ptr_eq(&payload, &returned));
        assert_eq!(
            returned.cause,
            sophia_session::session_actions::CatalogLaunchCause::Transient(activation)
        );
        assert_eq!(command.is_err(), changed);
        if let Ok(command) = command {
            assert_eq!(Some(command), payload.entry.command);
        }
        assert!(!h.queue.native_catalog_admission(&returned));
        while !worker.poll_shutdown().unwrap() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(worker.poll_shutdown().unwrap());
    }
}

#[test]
fn native_execution_attempt_is_exact_once_and_revocation_does_not_undo_it() {
    let mut h = Harness::new();
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let payload = h.dispatch();
    assert!(!h.queue.dispatch_catalog(payload.transaction));
    let command = payload.entry.command.clone().unwrap();
    let mut wrong_grant = GRANT;
    wrong_grant.content_grant_epoch += 1;
    let mut wrong_command = command.clone();
    wrong_command.arguments.push("substitution".into());
    assert!(
        !h.queue
            .begin_native_catalog_execution(&payload, wrong_grant, &command)
    );
    assert!(
        !h.queue
            .begin_native_catalog_execution(&payload, GRANT, &wrong_command)
    );
    assert!(
        h.queue
            .begin_native_catalog_execution(&payload, GRANT, &command)
    );
    assert!(
        !h.queue
            .begin_native_catalog_execution(&payload, GRANT, &command)
    );
    assert!(!h.queue.dispatch_catalog(payload.transaction));
    assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 0);
    assert!(h.queue.native_catalog_admission(&payload));
    // A failed spawn explicitly settles the same admission. No process is
    // spawned here: this control exercises the queue's irreversible attempt.
    h.queue.cancel_native_catalog(&payload);
    assert!(!h.queue.native_catalog_admission(&payload));
    assert!(
        !h.queue
            .begin_native_catalog_execution(&payload, GRANT, &command)
    );
}

#[test]
fn managed_origin_match_requires_native_payload_not_just_catalog_transaction() {
    let mut h = Harness::new();
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let payload = h.dispatch();
    assert!(
        h.queue
            .matches_child_launch(payload.transaction, true, Some(&payload))
    );
    assert!(
        !h.queue
            .matches_child_launch(payload.transaction, true, None)
    );
    assert!(
        !h.queue
            .matches_child_launch(payload.transaction, false, Some(&payload))
    );
    let mut other = (*payload).clone();
    let sophia_session::session_actions::CatalogLaunchCause::Transient(activation) =
        &mut other.cause
    else {
        panic!("transient fixture");
    };
    activation.event.binding.grant.content_grant_epoch += 1;
    assert!(
        !h.queue
            .matches_child_launch(payload.transaction, true, Some(&other))
    );
    other = (*payload).clone();
    other.entry = std::sync::Arc::new((*payload.entry).clone());
    assert!(
        !h.queue
            .matches_child_launch(payload.transaction, true, Some(&other))
    );
    h.queue.cancel_native_catalog(&payload);
    assert!(matches!(
        h.queue.enqueue_catalog(
            sophia_session::session_actions::SessionLaunchIntent {
                transaction: payload.transaction,
                application: SessionApplicationId::from_raw(1),
                placement_classification: None,
            },
            0
        ),
        sophia_session::session_actions::SessionLaunchQueueOutcome::Queued { .. }
    ));
    h.queue.begin_next(true).unwrap();
    assert!(
        h.queue
            .matches_child_launch(payload.transaction, true, None)
    );
    assert!(
        !h.queue
            .matches_child_launch(payload.transaction, true, Some(&payload))
    );
}

#[test]
fn native_process_spawn_retains_origin_and_duplicate_cannot_cancel_started_child() {
    use sophia_session::application_catalog::*;
    use std::sync::Arc;
    let command = ApplicationLaunchCommand {
        executable: "/bin/true".into(),
        arguments: vec![],
        working_directory: None,
    };
    let mut h = Harness::with_command(command.clone());
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let payload = h.dispatch();
    let environment = || CatalogProcessEnvironment {
        display: ":unavailable",
        xauthority: std::path::Path::new("/nonexistent"),
        control_socket: None,
    };
    // Supplied verification result; real worker verification is a separate
    // control above. This exercises real process creation in a private domain.
    let mut child = spawn_native_catalog(
        &h.peer.transport.connection(&mut h.epochs),
        &mut h.queue,
        Arc::clone(&payload),
        Ok(command.clone()),
        environment(),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&child.launch, &payload));
    assert!(
        h.queue
            .matches_child_launch(payload.transaction, true, Some(&child.launch))
    );
    assert!(matches!(
        spawn_native_catalog(
            &h.peer.transport.connection(&mut h.epochs),
            &mut h.queue,
            Arc::clone(&payload),
            Ok(command),
            environment(),
        ),
        Err(NativeCatalogSpawnError::Refused)
    ));
    assert!(h.queue.native_catalog_admission(&payload));
    assert!(child.child.wait().unwrap().success());
    // true exits without a window, so existing catalog policy reports failed
    // admission despite exit 0. This is not a first-window acceptance test.
    assert!(
        h.queue
            .complete_successful_exit(payload.transaction, true)
            .is_none()
    );
    assert_eq!(
        h.queue.fail_current().unwrap().intent.transaction,
        payload.transaction
    );
}

#[test]
fn native_spawn_refuses_failed_verification_and_revoked_connection_before_effect() {
    use sophia_session::application_catalog::*;
    for disconnected in [false, true] {
        let command = ApplicationLaunchCommand {
            executable: "/bin/true".into(),
            arguments: vec![],
            working_directory: None,
        };
        let mut h = Harness::with_command(command.clone());
        let activation = h.accept();
        assert_eq!(h.activate(activation, 0).status, 1);
        let payload = h.dispatch();
        if disconnected {
            h.peer.transport.disconnect(&mut h.epochs).unwrap();
        }
        let verified = if disconnected {
            Ok(command)
        } else {
            Err("changed".into())
        };
        assert!(matches!(
            spawn_native_catalog(
                &h.peer.transport.connection(&mut h.epochs),
                &mut h.queue,
                payload.clone(),
                verified,
                CatalogProcessEnvironment {
                    display: ":unavailable",
                    xauthority: std::path::Path::new("/nonexistent"),
                    control_socket: None
                },
            ),
            Err(NativeCatalogSpawnError::Refused)
        ));
        assert!(!h.queue.native_catalog_admission(&payload));
    }
}

#[test]
fn native_spawn_error_settles_exact_attempt_without_retry() {
    use sophia_session::application_catalog::*;
    let command = ApplicationLaunchCommand {
        executable: "/bin/true".into(),
        arguments: vec![],
        working_directory: Some("/nonexistent-native-launch-fixture".into()),
    };
    let mut h = Harness::with_command(command.clone());
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let payload = h.dispatch();
    assert!(matches!(
        spawn_native_catalog(
            &h.peer.transport.connection(&mut h.epochs),
            &mut h.queue,
            payload.clone(),
            Ok(command),
            CatalogProcessEnvironment {
                display: ":unavailable",
                xauthority: std::path::Path::new("/nonexistent"),
                control_socket: None
            },
        ),
        Err(NativeCatalogSpawnError::Spawn(_))
    ));
    assert!(!h.queue.native_catalog_admission(&payload));
}

#[test]
fn connected_visit_joins_real_pointer_ledger_cancellation_and_launch_queue() {
    for current in [true, false] {
        let mut h = Harness::new();
        let mut binding = sophia_engine::PresentedContentBinding {
            grant: h.focus.grant,
            output: h.focus.output,
            candidate_generation: h.focus.candidate_generation,
            presentation_epoch: h.focus.presentation_epoch,
            interaction_generation: h.focus.interaction_generation,
            transform: sophia_engine::PresentedContentTransform {
                viewport: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                layout_generation: 1,
            },
            authority_current: true,
            targets: vec![h.target.clone()],
            allocations: vec![(
                h.target.allocation,
                h.target.allocation_logical,
                h.target.allocation_pixel,
            )],
        };
        sophia_engine::reconcile_content_continuity(None, &mut binding);
        h.target = binding.targets[0].clone();
        let presented = if current { vec![binding] } else { vec![] };
        let event_id = h
            .service
            .issue(
                &mut h.peer.transport.connection(&mut h.epochs),
                h.target.clone(),
                tx(30),
                1,
            )
            .unwrap()
            .unwrap();
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        assert!(matches!(
            decode_shell_content_frame(&h.peer.read()).unwrap().1,
            ShellContentRecord::Action(_)
        ));
        let activation = NativeLauncherActivation {
            event: NativeLauncherEvent {
                binding: h.focus,
                event_id,
                state_revision: 1,
            },
            cause: 2,
            slot: 1,
        };
        h.peer.send(ShellNativeLauncherRecord::Activate(activation));
        assert_eq!(
            h.service
                .service_connected(
                    &mut h.peer.transport.connection(&mut h.epochs),
                    &h.catalog,
                    &presented,
                    tx(31),
                    &mut h.queue,
                    SessionApplicationId::from_raw(2),
                    0,
                    1001,
                    1
                )
                .unwrap(),
            1
        );
        assert_eq!(h.queue.pending_len(), usize::from(current));
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        if !current {
            let (_, ShellContentRecord::Action(cancel)) =
                decode_shell_content_frame(&h.peer.read()).unwrap()
            else {
                panic!("cancel")
            };
            assert_eq!(cancel.kind, 3); // protocol ActionCancel
            assert_eq!(cancel.event_id, event_id);
        }
        let outcome = h.outcome();
        assert_eq!(outcome.activation, activation);
        assert_eq!(outcome.status, if current { 1 } else { 2 });
        assert_eq!(h.queue.pending_len(), usize::from(current));
        h.peer.send(ShellNativeLauncherRecord::Activate(activation));
        assert_eq!(
            h.service
                .service_connected(
                    &mut h.peer.transport.connection(&mut h.epochs),
                    &h.catalog,
                    &presented,
                    tx(32),
                    &mut h.queue,
                    SessionApplicationId::from_raw(2),
                    0,
                    1002,
                    2
                )
                .unwrap(),
            1
        );
        assert_ne!(h.outcome().status, 1);
        assert_eq!(h.queue.pending_len(), usize::from(current));
    }
}
