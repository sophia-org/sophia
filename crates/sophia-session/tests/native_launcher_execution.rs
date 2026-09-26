#![cfg(feature = "native-session")]
//! Actual private socket intake, worker verification and short-lived process
//! execution. Supplied presentation/protection; no display or native compositor.
use sophia_engine::PresentedContentTarget;
use sophia_protocol::*;
use sophia_session::application_catalog as catalog;
use sophia_session::session_actions::SessionLaunchQueue;
use sophia_session::shell_native_launcher::NativeLauncherActionService;
#[allow(dead_code)]
#[path = "support/content_actions/native_launcher_fixture.rs"]
mod fixture;
use fixture::*;

fn environment() -> catalog::CatalogProcessEnvironment<'static> {
    catalog::CatalogProcessEnvironment {
        display: ":unavailable",
        xauthority: std::path::Path::new("/nonexistent"),
        control_socket: None,
        inspection_socket: None,
    }
}

#[test]
fn transient_connection_cannot_execute_a_persistent_cause_with_the_same_grant() {
    let mut h = Harness::with_command(catalog::ApplicationLaunchCommand {
        executable: "/bin/true".into(),
        arguments: vec![],
        working_directory: None,
    });
    let entry = h.catalog.entry(1).unwrap();
    let activation = CatalogActivation {
        catalog_generation: h.catalog.wire().generation,
        action: ContentAction {
            grant: GRANT,
            output: OUTPUT,
            candidate_generation: 1,
            presentation_epoch: 11,
            interaction_generation: 1,
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            event_id: 1,
            kind: 1,
            reason: 0,
        },
    };
    h.queue
        .enqueue_persistent_catalog(
            activation,
            entry.clone(),
            SessionApplicationId::from_raw(2),
            0,
        )
        .unwrap();
    let launch = h.dispatch();
    let connection = h.peer.transport.connection(&mut h.epochs);
    assert!(connection.supports_native_launcher());
    assert!(!connection.supports_persistent_catalog());
    match catalog::spawn_native_catalog(
        &connection,
        &mut h.queue,
        launch,
        Ok(entry.command.clone().unwrap()),
        environment(),
    ) {
        Err(catalog::NativeCatalogSpawnError::Refused) => {}
        Ok(mut child) => {
            child.child.wait().unwrap();
            panic!("wrong role executed");
        }
        Err(catalog::NativeCatalogSpawnError::Spawn(error)) => {
            panic!("wrong role attempted spawn: {error}")
        }
    }
    assert!(h.queue.admission().is_none());
}
fn service(command: catalog::ApplicationLaunchCommand) -> catalog::NativeCatalogService {
    catalog::NativeCatalogService::start(
        sophia_config::ApplicationCatalogConfig {
            name: "test".into(),
            sources: vec![],
            applications: vec!["app1".into(), "app2".into()],
            terminal: None,
            terminal_arguments: vec![],
        },
        ["app1", "app2"]
            .map(|name| catalog::RegisteredCatalogApplication {
                name: name.into(),
                command: command.clone(),
            })
            .into(),
        catalog::ApplicationCatalogEnvironment {
            search_path: vec![],
            locale: "C".into(),
            current_desktop: vec![],
        },
    )
    .unwrap()
}

#[test]
fn worker_to_process_join_preserves_revocation_deadline_and_stop_boundaries() {
    for mode in 0..6 {
        let command = catalog::ApplicationLaunchCommand {
            executable: "/bin/true".into(),
            arguments: vec![],
            working_directory: None,
        };
        let mut h = Harness::with_command(command.clone());
        let activation = h.accept();
        assert_eq!(h.activate(activation, 0).status, 1);
        let intent = h.queue.begin_next(true).unwrap();
        assert!(h.queue.dispatch_catalog(intent.transaction));
        let mut owner = service(command);
        assert!(matches!(
            owner.service(
                Some(&h.peer.transport.connection(&mut h.epochs)),
                &mut h.queue,
                environment(),
                10
            ),
            catalog::NativeCatalogServiceEvent::Idle
        ));
        match mode {
            1 => owner.request_shutdown(&mut h.queue),
            2 => {
                h.peer.transport.disconnect(&mut h.epochs).unwrap();
            }
            4 => {
                assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 1);
            }
            _ => {}
        }
        let now = if mode == 3 {
            5010
        } else if mode == 5 {
            9
        } else {
            11
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let event = loop {
            let event = owner.service(
                Some(&h.peer.transport.connection(&mut h.epochs)),
                &mut h.queue,
                environment(),
                now,
            );
            if !matches!(event, catalog::NativeCatalogServiceEvent::Idle) {
                break event;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        if mode == 0 {
            let catalog::NativeCatalogServiceEvent::Started(mut child) = event else {
                panic!("expected real spawn");
            };
            assert_eq!(
                child.launch.cause,
                sophia_session::session_actions::CatalogLaunchCause::Transient(activation)
            );
            assert!(
                h.queue
                    .matches_child_launch(intent.transaction, true, Some(&child.launch))
            );
            assert!(child.child.wait().unwrap().success());
            h.queue.fail_current().unwrap(); // true produced no window
        } else {
            assert!(matches!(
                event,
                catalog::NativeCatalogServiceEvent::Rejected
            ));
            assert!(h.queue.admission().is_none());
        }
        owner.request_shutdown(&mut h.queue);
        while !owner.poll_shutdown().unwrap() {
            let event = owner.service(None, &mut h.queue, environment(), now);
            assert!(!matches!(
                event,
                catalog::NativeCatalogServiceEvent::Started(_)
            ));
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(owner.poll_shutdown().unwrap());
    }
}

#[test]
fn stopping_during_catalog_refresh_drains_result_before_join_without_publication() {
    let command = catalog::ApplicationLaunchCommand {
        executable: "/bin/true".into(),
        arguments: vec![],
        working_directory: None,
    };
    let mut owner = service(command);
    let mut launches = SessionLaunchQueue::default();
    assert!(owner.refresh(8));
    assert!(!owner.refresh(9));
    owner.request_shutdown(&mut launches);
    assert!(!owner.refresh(10));
    assert!(!owner.poll_shutdown().unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut discarded = 0;
    while !owner.poll_shutdown().unwrap() {
        match owner.service(None, &mut launches, environment(), 1) {
            catalog::NativeCatalogServiceEvent::Idle => {}
            catalog::NativeCatalogServiceEvent::Rejected => discarded += 1,
            _ => panic!("stopped refresh must not publish or spawn"),
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(discarded, 1);
}

#[test]
fn terminal_drain_joins_refresh_without_environment_or_connection() {
    let mut owner = service(catalog::ApplicationLaunchCommand {
        executable: "/bin/true".into(),
        arguments: vec![],
        working_directory: None,
    });
    let mut queue = SessionLaunchQueue::default();
    assert!(owner.refresh(7));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !owner.drain_shutdown(&mut queue).unwrap() {
        assert!(
            std::time::Instant::now() < deadline,
            "shutdown retained undrained catalog result"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(owner.drain_shutdown(&mut queue).unwrap());
    assert!(!owner.refresh(8));
    assert!(queue.admission().is_none());
}

#[test]
fn terminal_drain_rejects_exact_queued_verification_and_never_executes_it() {
    let command = catalog::ApplicationLaunchCommand {
        executable: "/bin/true".into(),
        arguments: vec![],
        working_directory: None,
    };
    let mut h = Harness::with_command(command.clone());
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let intent = h.queue.begin_next(true).unwrap();
    assert!(h.queue.dispatch_catalog(intent.transaction));
    let mut owner = service(command);
    assert!(matches!(
        owner.service(
            Some(&h.peer.transport.connection(&mut h.epochs)),
            &mut h.queue,
            environment(),
            10,
        ),
        catalog::NativeCatalogServiceEvent::Idle
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !owner.drain_shutdown(&mut h.queue).unwrap() {
        assert!(
            h.queue.admission().is_none(),
            "shutdown kept native execution admission"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "shutdown retained verification result"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(h.queue.admission().is_none());
    assert!(owner.drain_shutdown(&mut h.queue).unwrap());
    assert!(!owner.refresh(9));
}
