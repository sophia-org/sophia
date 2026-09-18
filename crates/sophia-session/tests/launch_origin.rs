use sophia_protocol::*;
use sophia_session::launch_origin::*;

fn peer(client: u64, namespace: u64) -> ClientAdmissionContext {
    ClientAdmissionContext::new(
        ClientAdmissionId::from_raw(client),
        NamespaceContext::new(
            NamespaceId::from_raw(namespace),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        ClientAuthProvenance::new(ClientAuthenticationMethod::PeerCredentials, 1).unwrap(),
    )
    .unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        start_time: u64::from(pid),
    }
}
fn surface(index: u32) -> SurfaceId {
    SurfaceId::new(index, 1)
}
fn context(index: u32, token: u64) -> PolicyLaunchContext {
    PolicyLaunchContext {
        surface: surface(index),
        epoch: 7,
        token,
    }
}
fn host(registry: &mut LaunchOriginRegistry, id: u32, token: u64) {
    registry.admit(peer(u64::from(id), 1), process(id), &[]);
    registry.observe_toplevel(surface(id), peer(u64::from(id), 1));
    registry.publish(7, &[context(id, token)]);
}

#[test]
fn delayed_child_freezes_origin_before_focus_and_source_placement_change() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    host(&mut registry, 20, 202);
    registry.focused(Some(surface(10)));
    registry.admit(peer(30, 1), process(30), &[process(29), process(10)]);
    registry.focused(Some(surface(20)));
    registry.publish(7, &[context(10, 303)]);
    registry.observe_toplevel(surface(30), peer(30, 1));
    assert_eq!(registry.origins([surface(30)]), vec![context(30, 101)]);
    // Reconciliation/rejection does not consume a grant. Only committed admission does.
    assert_eq!(registry.origins([surface(30)]), vec![context(30, 101)]);
    registry.committed([surface(30)]);
    assert!(registry.origins([surface(30)]).is_empty());
    registry.observe_toplevel(surface(31), peer(30, 1));
    assert!(registry.origins([surface(31)]).is_empty());
}

#[test]
fn catalog_origin_uses_captured_token_not_new_workspace_and_dies_with_epoch() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    let captured = PolicyOutputLaunchContext {
        output: OutputId::from_raw(2),
        output_generation: 3,
        epoch: 7,
        token: 99,
    };
    registry.publish_outputs(7, &[captured]);
    host(&mut registry, 10, 101);
    registry.publish_outputs(
        7,
        &[PolicyOutputLaunchContext {
            token: 100,
            ..captured
        }],
    );
    assert!(registry.register_catalog_origin(surface(10), TransactionId::from_raw(4), captured));
    assert_eq!(registry.origins([surface(10)]), vec![context(10, 99)]);
    assert!(!registry.register_catalog_origin(surface(20), TransactionId::from_raw(4), captured));
    registry.publish_outputs(6, &[]);
    assert_eq!(registry.output_contexts().len(), 1);
    registry.set_epoch(8);
    assert!(registry.origins([surface(10)]).is_empty());
    assert!(registry.catalog_attribution(surface(10)).is_none());
    assert!(!registry.register_catalog_origin(surface(10), TransactionId::from_raw(4), captured));
}

#[test]
fn concurrent_children_cannot_exchange_launch_contexts_or_registered_identity() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    host(&mut registry, 20, 202);
    registry.admit(peer(30, 1), process(30), &[process(10)]);
    registry.admit(peer(40, 1), process(40), &[process(20)]);
    registry.observe_toplevel(surface(40), peer(40, 1));
    registry.observe_toplevel(surface(30), peer(30, 1));
    assert_eq!(
        registry.origins([surface(40), surface(30)]),
        vec![context(40, 202), context(30, 101)]
    );
    assert!(registry.belongs_to_process(peer(30, 1), process(10)));
    assert!(!registry.belongs_to_process(peer(30, 1), process(20)));
    assert!(!registry.belongs_to_process(peer(30, 2), process(10)));
}

#[test]
fn closest_ancestor_and_focus_evidence_disambiguate_multiple_windows() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    registry.observe_toplevel(surface(11), peer(10, 1));
    registry.publish(7, &[context(11, 111)]);
    registry.admit(peer(30, 1), process(30), &[process(10)]);
    registry.observe_toplevel(surface(30), peer(30, 1));
    assert!(registry.origins([surface(30)]).is_empty());
    registry.focused(Some(surface(11)));
    registry.admit(peer(40, 1), process(40), &[process(10)]);
    registry.observe_toplevel(surface(40), peer(40, 1));
    assert_eq!(registry.origins([surface(40)]), vec![context(40, 111)]);
    registry.publish(7, &[context(40, 404)]);
    registry.admit(peer(50, 1), process(50), &[process(40), process(10)]);
    registry.observe_toplevel(surface(50), peer(50, 1));
    assert_eq!(registry.origins([surface(50)]), vec![context(50, 404)]);
}

#[test]
fn source_exit_after_capture_keeps_frozen_origin_but_reuse_and_epoch_do_not() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    registry.admit(peer(30, 1), process(30), &[process(10)]);
    registry.revoke(peer(10, 1));
    registry.observe_toplevel(surface(30), peer(30, 1));
    assert_eq!(registry.origins([surface(30)]), vec![context(30, 101)]);
    registry.admit(
        peer(40, 1),
        process(40),
        &[ProcessIdentity {
            pid: 10,
            start_time: 999,
        }],
    );
    registry.observe_toplevel(surface(40), peer(40, 1));
    assert!(registry.origins([surface(40)]).is_empty());
    registry.set_epoch(8);
    assert!(registry.origins([surface(30)]).is_empty());
    registry.publish(7, &[context(30, 101)]);
    registry.admit(peer(50, 1), process(50), &[process(30)]);
    registry.observe_toplevel(surface(50), peer(50, 1));
    assert!(registry.origins([surface(50)]).is_empty());
}

#[test]
fn cross_namespace_and_existing_instance_connections_do_not_inherit_origin() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    registry.admit(peer(30, 2), process(30), &[process(10)]);
    registry.observe_toplevel(surface(30), peer(30, 2));
    assert!(registry.origins([surface(30)]).is_empty());
    registry.admit(peer(40, 1), process(40), &[]);
    registry.observe_toplevel(surface(40), peer(40, 1));
    assert!(registry.origins([surface(40)]).is_empty());
}

#[test]
fn ancestry_rechecks_start_identity_parent_links_and_depth() {
    let read = |pid| {
        Some(ProcessSnapshot {
            identity: process(pid),
            parent: pid.saturating_sub(1),
        })
    };
    assert_eq!(
        process_ancestors(process(3), read),
        vec![process(2), process(1)]
    );
    assert!(
        process_ancestors(
            ProcessIdentity {
                pid: 3,
                start_time: 99
            },
            read
        )
        .is_empty()
    );
    assert!(process_ancestors(process(100), read).is_empty());
    assert!(process_ancestors(process(3), |pid| (pid != 2).then(|| read(pid).unwrap())).is_empty());
    let mut reads = 0;
    assert!(
        process_ancestors(process(3), |pid| {
            reads += 1;
            let mut p = read(pid).unwrap();
            if reads > 3 {
                p.identity.start_time += 100;
            }
            Some(p)
        })
        .is_empty()
    );
    assert!(
        process_ancestors(process(3), |pid| Some(ProcessSnapshot {
            identity: process(pid),
            parent: pid
        }))
        .is_empty()
    );
}

#[test]
fn current_process_has_stable_proc_identity() {
    let process = read_process(std::process::id()).unwrap();
    assert!(process.identity.start_time > 0);
    assert!(!process_ancestors(process.identity, read_process).is_empty());
}

#[test]
fn nearer_uncommitted_source_does_not_borrow_a_grandparents_context() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    registry.admit(peer(20, 1), process(20), &[process(10)]);
    registry.observe_toplevel(surface(20), peer(20, 1));
    registry.admit(peer(30, 1), process(30), &[process(20), process(10)]);
    registry.observe_toplevel(surface(30), peer(30, 1));
    assert!(registry.origins([surface(30)]).is_empty());
}

#[test]
fn unconsumed_connection_grant_is_invalidated_by_restart_and_bounds_fall_back() {
    let mut registry = LaunchOriginRegistry::default();
    registry.set_epoch(7);
    host(&mut registry, 10, 101);
    registry.admit(peer(20, 1), process(20), &[process(10)]);
    registry.set_epoch(8);
    registry.observe_toplevel(surface(20), peer(20, 1));
    assert!(registry.origins([surface(20)]).is_empty());
    for id in 100..1200 {
        registry.admit(peer(id, 1), process(id as u32), &[process(10)]);
    }
    registry.observe_toplevel(surface(2000), peer(1199, 1));
    assert!(!registry.belongs_to_process(peer(1199, 1), process(10)));
    assert!(registry.origins([surface(2000)]).is_empty());
}
