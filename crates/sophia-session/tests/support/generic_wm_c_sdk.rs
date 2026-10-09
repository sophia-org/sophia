//! The QEMU guests' generic test WM (tools/qemu_generic_wm.c) against the
//! production WM file export, reactor, profile reducer and transport driver.
//! This test plays Session's side; Engine settlement and presentation are the
//! guest scenario's. It checks the WM's protocol and placement rule: every
//! surface of each Cycle output at that output's work-area origin, at its own
//! size bounded by the work area, unassigned surfaces on the active output,
//! and the newest of the active output focused.
use super::super::super::startup::tests::profile;
use super::super::{NinePPolicyAdapter, WmFileLimits, WmQids};
use super::{
    PolicyTransportCommand, PolicyTransportEvent, PolicyTransportWorker, enqueue, fixture,
};
use sophia_protocol::*;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::live_session::c_sdk_fixture_process as process;

fn next(worker: &PolicyTransportWorker, wm: &process::Process) -> PolicyTransportEvent {
    wm.check_output();
    match worker.event_timeout(Duration::from_secs(10)) {
        Ok(PolicyTransportEvent::Failed(error)) => {
            panic!("WM file driver failed: {error}; wm: {}", wm.diagnostic())
        }
        Ok(event) => event,
        Err(error) => panic!("WM file event: {error}; wm: {}", wm.diagnostic()),
    }
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

struct Launched {
    // Field order is drop order: the worker and WM end before the scratch
    // directory holding the binary and socket is removed.
    worker: PolicyTransportWorker,
    wm: process::Process,
    configuration: PolicyConfiguration,
    /// The Configuration's transaction; every projection's follows it.
    configured: TransactionId,
    _scratch: process::Scratch,
}

/// Starts the WM with `args`, answers its Configuration with a commit, and
/// returns once it is ready for its first Cycle.
fn launch(args: &[&str]) -> Launched {
    let scratch = process::Scratch::new();
    let binary = process::compile(
        &scratch.0,
        &["nine_p", "wm_files", "wm_session"],
        "../../../../tools/qemu_generic_wm.c",
    );
    let socket = scratch.0.join("wm.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    // The fixture clears the environment; env(1) sets the one variable Session
    // gives a protected WM launch.
    let mut command = Command::new("/usr/bin/env");
    command
        .arg(format!("SOPHIA_WM_9P_SOCKET={}", socket.display()))
        .arg(&binary)
        .args(args);
    let wm = process::Process::spawn(&mut command, &scratch.0, "wm");
    let until = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("WM accept: {error}"),
        }
        assert!(
            !wm.exited(),
            "WM exited before connect: {}",
            wm.diagnostic()
        );
        assert!(Instant::now() < until, "WM connect deadline");
        std::thread::sleep(Duration::from_millis(5));
    };
    let caps = sophia_runtime::select_policy_capabilities(u64::MAX, u64::MAX, true);
    let adapter = NinePPolicyAdapter::supplied(
        stream,
        9,
        WmFileLimits {
            capability_ceiling: caps,
            profile_required: true,
        },
        WmQids::new(),
    )
    .unwrap();
    let worker = PolicyTransportWorker::spawn(adapter, 9, Some(profile())).unwrap();
    assert!(matches!(
        next(&worker, &wm),
        PolicyTransportEvent::Negotiated
    ));
    let PolicyTransportEvent::Configuration {
        transaction,
        configuration,
    } = next(&worker, &wm)
    else {
        panic!("configuration missing");
    };
    // Configured for the profile Session activated.
    assert_eq!(configuration.connection_epoch, 9);
    assert_eq!(configuration.generation, profile().generation);
    enqueue(
        &worker,
        PolicyTransportCommand::ConfigurationOutcome {
            transaction,
            generation: configuration.generation,
            outcome: PolicyProjectionOutcome::Committed,
        },
    );
    assert!(matches!(
        next(&worker, &wm),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    Launched {
        worker,
        wm,
        configuration,
        configured: transaction,
        _scratch: scratch,
    }
}

#[test]
fn generic_test_wm_places_and_focuses_from_session_geometry() {
    let Launched {
        worker,
        wm,
        configuration,
        configured: transaction,
        _scratch,
    } = launch(&[]);
    assert!(configuration.actions.is_empty());

    // Two outputs with offset work areas. Output 1 is active and holds two
    // surfaces plus one unassigned; output 2 holds one larger than its area.
    let mut scene = fixture::scene();
    let mut second = scene.outputs[0];
    scene.outputs[0].bounds = rect(0, 0, 1280, 800);
    scene.outputs[0].work_area = rect(0, 32, 1280, 768);
    second.output = OutputId::from_raw(2);
    second.policy_key = Some(23);
    second.bounds = rect(1280, 0, 1024, 768);
    second.work_area = rect(1280, 0, 1024, 768);
    scene.outputs.push(second);
    let template = scene.surfaces[0];
    let surface = |index, output: Option<u64>, width, height| {
        let mut value = template;
        value.surface = SurfaceId::new(index, 1);
        value.current_output = output.map(OutputId::from_raw);
        value.geometry = rect(5, 6, width, height);
        value
    };
    scene.surfaces = vec![
        surface(3, Some(1), 400, 300),
        surface(4, Some(2), 2000, 2000),
        surface(5, Some(1), 640, 360),
        surface(6, None, 200, 100),
    ];
    scene.session_operations.clear();
    enqueue(
        &worker,
        PolicyTransportCommand::Cycle {
            snapshot_transaction: TransactionId::from_raw(100),
            request_transaction: TransactionId::from_raw(101),
            scene: Box::new(scene),
            actions: vec![],
            classifications: vec![],
            launch_origins: vec![],
            request: PolicyProjectionRequest {
                connection_epoch: 9,
                request_id: 55,
                scene_generation: 7,
                policy_generation: profile().generation,
                affected_outputs: vec![OutputId::from_raw(1), OutputId::from_raw(2)],
                cause: PolicyRequestCause::SceneChanged,
            },
        },
    );
    let PolicyTransportEvent::Projection(projection) = next(&worker, &wm) else {
        panic!("projection missing");
    };
    assert!(projection.transaction.raw() > transaction.raw());
    assert_eq!(projection.request_id, 55);
    assert_eq!(projection.base_generation, 7);
    assert_eq!(projection.active_output, OutputId::from_raw(1));
    let placed = |output: &PolicyOutputProjection| {
        output
            .placements
            .iter()
            .map(|p| (p.surface.index(), p.geometry, p.requested_size))
            .collect::<Vec<_>>()
    };
    let size = |width, height| Some(Size { width, height });
    assert_eq!(projection.outputs.len(), 2);
    assert_eq!(projection.outputs[0].output, OutputId::from_raw(1));
    assert_eq!(
        placed(&projection.outputs[0]),
        vec![
            (3, rect(0, 32, 400, 300), size(400, 300)),
            (5, rect(0, 32, 640, 360), size(640, 360)),
            (6, rect(0, 32, 200, 100), size(200, 100)),
        ]
    );
    assert_eq!(projection.outputs[0].focus, Some(SurfaceId::new(6, 1)));
    assert_eq!(projection.outputs[1].output, OutputId::from_raw(2));
    assert_eq!(
        placed(&projection.outputs[1]),
        vec![(4, rect(1280, 0, 1024, 768), size(1024, 768))]
    );
    assert_eq!(projection.outputs[1].focus, None);
    for output in &projection.outputs {
        for placement in &output.placements {
            assert_eq!(placement.surface_generation, 8);
            assert_eq!(placement.crop, None);
            assert_eq!(placement.transform, PolicyTransform::Identity);
            assert_eq!(placement.presentation, PolicyPresentationState::default());
        }
    }
    enqueue(
        &worker,
        PolicyTransportCommand::ProjectionOutcome {
            transaction: projection.transaction,
            request_id: 55,
            scene_generation: 7,
            outcome: PolicyProjectionOutcome::Committed,
            expect_session_operation: false,
        },
    );
    assert!(matches!(
        next(&worker, &wm),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    assert!(!wm.exited(), "the WM stops only with its session");
    let log = wm.diagnostic();
    assert!(
        log.contains("sophia_qemu_wm schema=1 status=active generation=3"),
        "{log}"
    );
    assert!(!log.contains("status=failed"), "{log}");
    drop(worker);
}

/// The controlled-repaint opt-in (t307): with `--hold-shift` the WM registers
/// one action, and each Action Cycle naming it toggles every placement between
/// x offsets 0 and 8 inside its work area. Other Cycles keep the offset, and a
/// placement as wide as its work area never moves. Each proposal and outcome
/// is reported with its identities.
#[test]
fn generic_test_wm_hold_shift_toggles_placement_on_its_action() {
    let Launched {
        worker,
        wm,
        configuration,
        configured,
        _scratch,
    } = launch(&["--hold-shift"]);
    let hold_shift = WmActionId::from_raw(1);
    assert_eq!(
        configuration.actions,
        vec![PolicyActionRegistration {
            action: hold_shift,
            name: "hold-shift".to_owned(),
            session_operation_slot: None,
        }]
    );

    let mut scene = fixture::scene();
    scene.outputs.truncate(1);
    scene.outputs[0].bounds = rect(0, 0, 1280, 800);
    scene.outputs[0].work_area = rect(0, 0, 1280, 800);
    let template = scene.surfaces[0];
    let surface = |index, width, height| {
        let mut value = template;
        value.surface = SurfaceId::new(index, 1);
        value.current_output = Some(OutputId::from_raw(1));
        value.geometry = rect(100, 100, width, height);
        value
    };
    scene.surfaces = vec![surface(3, 400, 300), surface(4, 2000, 300)];
    scene.session_operations.clear();
    let action = |activation_serial| PolicyRequestCause::Action {
        activation_serial,
        action: hold_shift,
    };
    // (request id, cause, expected x of the 400 px surface)
    let steps = [
        (60, PolicyRequestCause::SceneChanged, 0),
        (61, action(1), 8),
        (62, PolicyRequestCause::SceneChanged, 8),
        (63, action(2), 0),
        (64, action(3), 8),
    ];
    let mut previous = configured;
    for (step, (request_id, cause, x)) in steps.into_iter().enumerate() {
        let step = step as u64;
        let mut snapshot = scene.clone();
        snapshot.generation = 7 + step;
        enqueue(
            &worker,
            PolicyTransportCommand::Cycle {
                snapshot_transaction: TransactionId::from_raw(100 + 2 * step),
                request_transaction: TransactionId::from_raw(101 + 2 * step),
                scene: Box::new(snapshot),
                actions: configuration.actions.clone(),
                classifications: vec![],
                launch_origins: vec![],
                request: PolicyProjectionRequest {
                    connection_epoch: 9,
                    request_id,
                    scene_generation: 7 + step,
                    policy_generation: profile().generation,
                    affected_outputs: vec![OutputId::from_raw(1)],
                    cause,
                },
            },
        );
        let PolicyTransportEvent::Projection(projection) = next(&worker, &wm) else {
            panic!("projection missing at request {request_id}");
        };
        assert!(projection.transaction.raw() > previous.raw());
        previous = projection.transaction;
        assert_eq!(projection.request_id, request_id);
        assert_eq!(projection.outputs.len(), 1);
        let placed = projection.outputs[0]
            .placements
            .iter()
            .map(|p| (p.surface.index(), p.geometry))
            .collect::<Vec<_>>();
        assert_eq!(
            placed,
            vec![(3, rect(x, 0, 400, 300)), (4, rect(0, 0, 1280, 300))],
            "request {request_id}"
        );
        enqueue(
            &worker,
            PolicyTransportCommand::ProjectionOutcome {
                transaction: projection.transaction,
                request_id,
                scene_generation: 7 + step,
                outcome: PolicyProjectionOutcome::Committed,
                expect_session_operation: false,
            },
        );
        assert!(matches!(
            next(&worker, &wm),
            PolicyTransportEvent::ReadyForCycle { .. }
        ));
        let log = wm.diagnostic();
        let (shift, serial) = match cause {
            PolicyRequestCause::Action {
                activation_serial, ..
            } => (1, activation_serial),
            _ => (0, 0),
        };
        let cause = if shift == 1 { 1 } else { 0 };
        let proposed = format!(
            "sophia_qemu_wm_hold schema=1 status=proposed transaction={} request_id={request_id} \
             cause={cause} activation_serial={serial} shift={shift} offset_x={x} placements=2\n",
            projection.transaction.raw()
        );
        let outcome = format!(
            "sophia_qemu_wm_hold schema=1 status=outcome transaction={} request_id={request_id} \
             scene_generation={} outcome=1\n",
            projection.transaction.raw(),
            7 + step
        );
        assert!(log.contains(&proposed), "{proposed}{log}");
        // The WM reports the outcome after it reads it, which may follow the
        // worker's readiness.
        let until = Instant::now() + Duration::from_secs(5);
        while !wm.diagnostic().contains(&outcome) {
            assert!(Instant::now() < until, "{outcome}{}", wm.diagnostic());
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert!(!wm.exited(), "the WM stops only with its session");
    assert!(!wm.diagnostic().contains("status=failed"));
    drop(worker);
}
