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
