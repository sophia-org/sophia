/// An unreadable bridge is reported as unreadable, never as nothing owed.
///
/// THE FAULT IS RAISED AFTER THE SERVICE IS RUNNING and after a real peer has
/// connected, so what is damaged is a bridge belonging to a service that had
/// something to lose.
#[test]
fn an_unreadable_bridge_retains_custody_rather_than_reporting_nothing_owed() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let _ = fixture
        .handle_mut()
        .apply_committed(Duration::from_millis(10));

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::thread::spawn(move || {
        let _held = runtime.bridge.lock().unwrap();
        panic!("poisoning the bridge on purpose");
    });
    assert!(poisoner.join().is_err(), "the poisoning thread panicked");

    // A poisoned bridge cannot be read, and that is its own answer.
    assert!(fixture.handle().outstanding().is_err());

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();
    assert_eq!(
        outcome.bridge_undelivered, None,
        "an unreadable bridge reports no count at all, not a count of zero: {outcome:?}"
    );
    assert!(
        outcome.retains_obligations(),
        "nothing has been shown to be finished, so custody is retained: {outcome:?}"
    );
    drop(peer);
}

/// A poisoned admission boundary refuses rather than reporting an empty one.
#[test]
fn an_unreadable_surface_ledger_refuses_rather_than_reporting_an_empty_one() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    // A REALLY ADMITTED SURFACE FIRST, so the ledger being poisoned is one that
    // had something to lose.
    let _surface = fixture.admitted_surface();

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.admitted_surfaces.lock().unwrap();
        panic!("poisoning the surface ledger on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    // NEW WORK THE STAGING MUST ACTUALLY TOUCH. An earlier version poisoned the
    // ledger and then called an idle `apply_committed`, which has no staged
    // decision, never reaches `stage_decisions`, and so legitimately returns
    // Ok -- the control was asserting against a step that never took the lock
    // it had damaged. A redraw on the already-admitted window produces a real
    // batch, and the geometry reply orders it.
    peer.draw(window);
    peer.confirm_geometry(window);

    wait_for(
        &mut fixture,
        "the redraw reaching the bridge and being refused on the unreadable ledger",
        |fixture| format!("the order still owes {:?}", fixture.handle().outstanding()),
        |fixture| match fixture
            .handle_mut()
            .apply_committed(Duration::from_millis(10))
        {
            Err(_) => Progress::Done(()),
            Ok(report) => {
                assert_eq!(
                    report.batches_observed, 0,
                    "a call that took work read the unreadable ledger as empty: {report:?}"
                );
                if ended(&report) {
                    Progress::Lost(fixture.ended_report())
                } else if advanced(&report) {
                    Progress::Worked
                } else {
                    Progress::Idle
                }
            }
        },
    );

    // UNREADABLE IS DURABLE, NOT A ONE-OFF.
    assert!(
        fixture
            .handle_mut()
            .apply_committed(Duration::from_millis(10))
            .is_err(),
        "the ledger stays unreadable rather than recovering into an empty read"
    );
    // AND THE REFUSAL CONSUMED NOTHING: the bridge is still readable and still
    // holding the work the refused staging could not place.
    let outstanding = fixture
        .handle()
        .outstanding()
        .expect("the bridge itself is readable");
    assert!(
        outstanding >= 1,
        "the refused staging left its work owed rather than dropping it"
    );
    drop(peer);
}

/// A stale connection is refused before any control identity is spent on it.
#[test]
fn a_control_for_a_departed_connection_is_refused() {
    let fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();

    // WAITED FOR, NOT ASSUMED. An earlier version read `admitted().first()`
    // immediately after connecting and found nothing, because admission is the
    // boundary's act and had not happened yet.
    let seen = spin_for(
        &mut (),
        "the peer's admission at the boundary",
        |_| format!("the boundary holds {:?}", fixture.handle().admitted()),
        |_| {
            let admitted = fixture
                .handle()
                .admitted()
                .expect("the boundary is readable");
            match admitted.first().copied() {
                Some(seen) => Progress::Done(seen),
                None => not_yet(fixture.handle()),
            }
        },
    );
    let live = crate::private_input::PrivateInputConnection {
        client: seen.client,
        admission: seen.admission,
        connection_generation: seen.connection_generation,
    };

    // A GENERATION NO LIVE ROW CARRIES is refused even while the peer is here,
    // which is the identity half of the claim.
    let mismatched = fixture.handle().submit_action(
        crate::private_input::PrivateInputConnection {
            connection_generation: seen.connection_generation.wrapping_add(1),
            ..live
        },
        crate::private_input::PrivateInputAction::ClearFocus {
            surface: sophia_protocol::SurfaceId::new(1, 1),
        },
    );
    assert!(
        matches!(
            mismatched,
            Err(crate::private_input::PrivateInputControlError::ConnectionGone)
        ),
        "a connection generation no live row carries is refused: {mismatched:?}"
    );

    // NOW THE CONNECTION REALLY DEPARTS, and that exact admission must go.
    drop(peer);
    spin_for(
        &mut (),
        "the dropped peer's admission closing",
        |_| format!("the boundary holds {:?}", fixture.handle().admitted()),
        |_| {
            let admitted = fixture
                .handle()
                .admitted()
                .expect("the boundary is readable");
            let departed = !admitted.iter().any(|row| {
                row.client == live.client && row.admission == live.admission && !row.closed
            });
            if departed {
                Progress::Done(())
            } else {
                not_yet(fixture.handle())
            }
        },
    );

    let refused = fixture.handle().submit_action(
        live,
        crate::private_input::PrivateInputAction::ClearFocus {
            surface: sophia_protocol::SurfaceId::new(1, 1),
        },
    );
    assert!(
        matches!(
            refused,
            Err(crate::private_input::PrivateInputControlError::ConnectionGone)
        ),
        "the exact admission that departed is refused: {refused:?}"
    );
}

/// A topology naming a primary it does not contain is refused, and nothing is
/// built for it.
#[test]
fn a_primary_absent_from_the_topology_is_refused_rather_than_replaced() {
    let directory = std::env::temp_dir().join(format!(
        "m4-session-topology-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let socket = directory.join("private.sock");
    let mut configured = config(&socket, PrivateInputGrantPolicy::Disabled);
    configured.output_topology.primary = OutputId::from_raw(77);
    let lifetime = PrivateInputLifetimeOwner::reserved();
    let refused = PrivateInputService::start(&lifetime, configured).err();
    assert!(
        matches!(
            refused,
            Some(crate::private_input::PrivateInputRefusal::Topology(
                crate::private_input::PrivateInputTopologyRefusal::PrimaryAbsent { .. }
            ))
        ),
        "the configured primary is served or nothing is: {refused:?}"
    );
    assert!(
        !socket.exists(),
        "a refused configuration binds no socket at all"
    );
    let _ = std::fs::remove_dir(&directory);
}

/// More than one output is refused explicitly rather than quietly narrowed.
#[test]
fn a_multihead_topology_is_refused_rather_than_narrowed_to_one_head() {
    let directory = std::env::temp_dir().join(format!(
        "m4-session-multihead-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let socket = directory.join("private.sock");
    let mut configured = config(&socket, PrivateInputGrantPolicy::Disabled);
    let second = configured.output_topology.outputs[0];
    configured
        .output_topology
        .outputs
        .push(OutputTopologyEntry {
            output: OutputId::from_raw(2),
            ..second
        });
    let lifetime = PrivateInputLifetimeOwner::reserved();
    let refused = PrivateInputService::start(&lifetime, configured).err();
    assert!(
        matches!(
            refused,
            Some(crate::private_input::PrivateInputRefusal::Topology(
                crate::private_input::PrivateInputTopologyRefusal::MultipleOutputs { count: 2 }
            ))
        ),
        "the assembly ticks one output, so two are refused: {refused:?}"
    );
    assert!(!socket.exists(), "a refused configuration binds no socket");
    let _ = std::fs::remove_dir(&directory);
}
