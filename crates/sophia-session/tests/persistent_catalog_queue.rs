//! Actual shared launch queue; supplied authorized events, no socket, Presented
//! validation, worker or process execution. Those are separate service gates.
use sophia_protocol::*;
use sophia_session::application_catalog::*;
use sophia_session::session_actions::*;
use std::sync::Arc;

fn entry() -> Arc<ApplicationCatalogEntry> {
    let catalog = build_application_catalog(
        &sophia_config::ApplicationCatalogConfig {
            name: "test".into(),
            sources: vec![],
            applications: vec!["terminal".into()],
            terminal: None,
            terminal_arguments: vec![],
        },
        &[RegisteredCatalogApplication {
            name: "terminal".into(),
            command: ApplicationLaunchCommand {
                executable: std::env::current_exe().unwrap(),
                arguments: vec![],
                working_directory: None,
            },
        }],
        &ApplicationCatalogEnvironment {
            search_path: vec![],
            locale: "C".into(),
            current_desktop: vec![],
        },
    )
    .unwrap();
    Arc::new(catalog.entries.into_iter().next().unwrap())
}
fn action(epoch: u64, event: u64) -> CatalogActivation {
    CatalogActivation {
        catalog_generation: 7,
        action: ContentAction {
            grant: ContentGrant {
                connection_epoch: epoch,
                content_grant_epoch: 2,
            },
            output: ContentOutputId {
                id: 2,
                generation: 1,
            },
            candidate_generation: 3,
            presentation_epoch: 4,
            interaction_generation: 5,
            allocation: ContentAllocationId {
                id: 6,
                generation: 1,
            },
            target_id: 7,
            target_generation: 1,
            action_id: 1,
            event_id: event,
            kind: 1,
            reason: 0,
        },
    }
}
fn enqueue(
    queue: &mut SessionLaunchQueue,
    value: CatalogActivation,
    entry: Arc<ApplicationCatalogEntry>,
) -> Result<TransactionId, NativeCatalogLaunchRefusal> {
    queue.set_output_launch_contexts(&[PolicyOutputLaunchContext {
        output: OutputId::from_raw(2),
        output_generation: 1,
        epoch: 1,
        token: 10,
    }]);
    queue.enqueue_persistent_catalog(value, entry, SessionApplicationId::from_raw(2), 0)
}
fn dispatch(queue: &mut SessionLaunchQueue) -> Arc<NativeCatalogLaunch> {
    let intent = queue.begin_next(true).unwrap();
    assert!(queue.dispatch_catalog(intent.transaction));
    assert!(queue.take_catalog_dispatch().is_none());
    let launch = queue.take_native_catalog_dispatch().unwrap();
    assert!(queue.take_native_catalog_dispatch().is_none());
    launch
}

#[test]
fn catalog_destination_is_frozen_across_focus_workspace_and_queue_delay() {
    let mut queue = SessionLaunchQueue::default();
    let entry = entry();
    let action = action(1, 10);
    enqueue(&mut queue, action.clone(), entry.clone()).unwrap();
    queue.set_output_launch_contexts(&[
        PolicyOutputLaunchContext {
            output: OutputId::from_raw(1),
            output_generation: 1,
            epoch: 1,
            token: 999,
        },
        PolicyOutputLaunchContext {
            output: OutputId::from_raw(2),
            output_generation: 1,
            epoch: 1,
            token: 20,
        },
    ]);
    let launch = dispatch(&mut queue);
    assert_eq!(launch.destination.output.raw(), 2);
    assert_eq!(launch.destination.token, 10);
    let mut forged = (*launch).clone();
    forged.destination.token = 999;
    assert!(!queue.native_catalog_admission(&forged));
    assert!(queue.begin_native_catalog_execution(
        &launch,
        action.action.grant,
        entry.command.as_ref().unwrap()
    ));
    let first = queue.observe_surface(SurfaceId::new(20, 1)).unwrap();
    assert_eq!(first.destination, Some(launch.destination));
    assert!(queue.observe_surface(SurfaceId::new(20, 1)).is_none());
    assert!(
        queue
            .observe_surface(SurfaceId::new(21, 1))
            .unwrap()
            .destination
            .is_none()
    );
}

#[test]
fn catalog_refuses_missing_context_and_replacement_before_execution() {
    let entry = entry();
    for replacement in [
        vec![],
        vec![PolicyOutputLaunchContext {
            output: OutputId::from_raw(2),
            output_generation: 2,
            epoch: 1,
            token: 10,
        }],
        vec![PolicyOutputLaunchContext {
            output: OutputId::from_raw(2),
            output_generation: 1,
            epoch: 2,
            token: 10,
        }],
    ] {
        let mut queue = SessionLaunchQueue::default();
        assert_eq!(
            queue.enqueue_persistent_catalog(
                action(1, 10),
                entry.clone(),
                SessionApplicationId::from_raw(2),
                0
            ),
            Err(NativeCatalogLaunchRefusal::Stale)
        );
        enqueue(&mut queue, action(1, 10), entry.clone()).unwrap();
        let launch = dispatch(&mut queue);
        queue.set_output_launch_contexts(&replacement);
        assert!(!queue.begin_native_catalog_execution(
            &launch,
            action(1, 10).action.grant,
            entry.command.as_ref().unwrap()
        ));
    }
}

#[test]
fn persistent_origin_and_command_are_exact_and_execution_is_consumed_once() {
    let mut queue = SessionLaunchQueue::default();
    let entry = entry();
    let action = action(1, 10);
    let transaction = enqueue(&mut queue, action.clone(), entry.clone()).unwrap();
    assert_eq!(
        enqueue(&mut queue, action.clone(), entry.clone()),
        Err(NativeCatalogLaunchRefusal::Stale)
    );
    let launch = dispatch(&mut queue);
    assert_eq!(launch.transaction, transaction);
    assert_eq!(launch.cause, CatalogLaunchCause::Persistent(action.clone()));
    assert!(Arc::ptr_eq(&launch.entry, &entry));
    let mut changed = (*launch).clone();
    if let CatalogLaunchCause::Persistent(value) = &mut changed.cause {
        value.catalog_generation += 1;
    }
    assert!(!queue.native_catalog_admission(&changed));
    let mut wrong_command = entry.command.clone().unwrap();
    wrong_command.arguments.push("unexpected".into());
    assert!(!queue.begin_native_catalog_execution(&launch, action.action.grant, &wrong_command));
    let command = entry.command.as_ref().unwrap();
    let mut wrong_grant = action.action.grant;
    wrong_grant.connection_epoch += 1;
    assert!(!queue.begin_native_catalog_execution(&launch, wrong_grant, command));
    assert!(queue.begin_native_catalog_execution(&launch, action.action.grant, command));
    assert!(!queue.begin_native_catalog_execution(&launch, action.action.grant, command));
    assert_eq!(queue.revoke_native_catalog_grant(action.action.grant), 0);
    assert!(queue.matches_child_launch(transaction, true, Some(&launch)));
    queue.cancel_native_catalog(&launch);
    assert!(queue.admission().is_none());
}

#[test]
fn revocation_removes_only_exact_peer_and_preserves_neighbor_dispatch() {
    let mut queue = SessionLaunchQueue::default();
    let entry = entry();
    let old = action(1, 10);
    let neighbor = action(2, 10);
    enqueue(&mut queue, old.clone(), entry.clone()).unwrap();
    let expected = enqueue(&mut queue, neighbor.clone(), entry.clone()).unwrap();
    let old_launch = dispatch(&mut queue);
    assert_eq!(queue.revoke_native_catalog_grant(old.action.grant), 1);
    assert_eq!(queue.revoke_native_catalog_grant(old.action.grant), 0);
    assert!(!queue.begin_native_catalog_execution(
        &old_launch,
        old.action.grant,
        entry.command.as_ref().unwrap()
    ));
    let next = dispatch(&mut queue);
    assert_eq!(next.transaction, expected);
    assert_eq!(next.cause, CatalogLaunchCause::Persistent(neighbor));
    queue.reject_native_before_execution(&old_launch);
    assert!(queue.native_catalog_admission(&next));
}

#[test]
fn wrong_slot_cancel_and_capacity_refuse_without_extra_queue_entry() {
    let entry = entry();
    let mut queue = SessionLaunchQueue::default();
    for variant in 0..3 {
        let mut value = action(1, 10);
        match variant {
            0 => value.action.action_id = 2,
            1 => value.action.kind = 2,
            _ => value.catalog_generation = 0,
        }
        assert_eq!(
            enqueue(&mut queue, value, entry.clone()),
            Err(NativeCatalogLaunchRefusal::Unauthorized)
        );
        assert!(queue.begin_next(true).is_none());
    }
    for event in 1..=16 {
        enqueue(&mut queue, action(1, event), entry.clone()).unwrap();
    }
    assert_eq!(
        enqueue(&mut queue, action(1, 17), entry.clone()),
        Err(NativeCatalogLaunchRefusal::Capacity)
    );
    assert_eq!(
        queue.revoke_native_catalog_grant(action(1, 1).action.grant),
        16
    );
    assert!(queue.begin_next(true).is_none());
}
