//! C SDK against the production WM file export, reactor, profile reducer and
//! transport driver. Admission and policy outcomes are supplied by this test;
//! no WM product, launch authentication, Engine commit or presentation proof.
use super::super::super::startup::tests::profile;
use super::super::{NinePPolicyAdapter, WmFileLimits, WmQids};
use super::{
    PolicyTransportCommand, PolicyTransportEvent, PolicyTransportWorker, enqueue, fixture,
};
use sophia_protocol::*;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::time::{Duration, Instant};

#[path = "policy_file_c_sdk/process.rs"]
mod process;

fn next(worker: &PolicyTransportWorker, peer: &process::Process) -> PolicyTransportEvent {
    peer.check_output();
    match worker.event_timeout(Duration::from_secs(10)) {
        Ok(PolicyTransportEvent::Failed(error)) => {
            panic!(
                "WM file driver failed: {error}; peer: {}",
                peer.diagnostic()
            )
        }
        Ok(event) => event,
        Err(error) => panic!("WM file event: {error}; peer: {}", peer.diagnostic()),
    }
}

#[test]
fn c_sdk_drives_profile_configuration_snapshot_and_policy_exchange() {
    let scratch = process::Scratch::new();
    let binary = process::compile(&scratch.0);
    let socket = scratch.0.join("wm.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut command = Command::new("/usr/bin/nice");
    command.args(["-n", "19"]).arg(&binary).arg(&socket);
    let peer = process::Process::spawn(&mut command, &scratch.0, "peer");
    let until = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("C peer accept: {error}"),
        }
        assert!(
            !peer.exited(),
            "C peer exited before connect: {}",
            peer.diagnostic()
        );
        assert!(Instant::now() < until, "C peer connect deadline");
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
        next(&worker, &peer),
        PolicyTransportEvent::Negotiated
    ));
    let PolicyTransportEvent::Configuration {
        transaction,
        configuration,
    } = next(&worker, &peer)
    else {
        panic!("configuration missing");
    };
    assert_eq!(transaction, TransactionId::from_raw(10));
    assert_eq!(configuration.connection_epoch, 9);
    assert_eq!(configuration.generation, 3);
    assert!(configuration.actions.is_empty());
    enqueue(
        &worker,
        PolicyTransportCommand::ConfigurationOutcome {
            transaction,
            generation: 3,
            outcome: PolicyProjectionOutcome::Committed,
        },
    );
    assert!(
        matches!(next(&worker, &peer), PolicyTransportEvent::ReadyForCycle { capabilities } if capabilities == caps)
    );
    let PolicyTransportEvent::Dirty(dirty) = next(&worker, &peer) else {
        panic!("dirty notification missing");
    };
    assert_eq!(dirty.connection_epoch, 9);
    assert_eq!(dirty.policy_generation, 3);
    assert_eq!(dirty.affected_outputs, [OutputId::from_raw(1)]);

    // At 80 bytes per surface this snapshot exceeds the peer's 4096 msize.
    // Every row must cross the real export and multiple bounded SDK reads.
    let mut scene = fixture::scene();
    let surface = scene.surfaces[0];
    scene.surfaces = (3..67)
        .map(|index| {
            let mut value = surface;
            value.surface = SurfaceId::new(index, 1);
            value
        })
        .collect();
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
                policy_generation: 3,
                affected_outputs: vec![OutputId::from_raw(1)],
                cause: PolicyRequestCause::SceneChanged,
            },
        },
    );
    let PolicyTransportEvent::Projection(projection) = next(&worker, &peer) else {
        panic!("projection missing");
    };
    let mut expected = fixture::proposal();
    expected.connection_epoch = 9;
    expected.request_id = 55;
    expected.indicators.clear();
    expected.output_statuses.clear();
    expected.tab_groups.clear();
    expected.translation_groups.clear();
    expected.launch_contexts.clear();
    expected.output_launch_contexts.clear();
    expected.presentation = None;
    assert_eq!(*projection, expected);
    enqueue(
        &worker,
        PolicyTransportCommand::ProjectionOutcome {
            transaction: projection.transaction,
            request_id: 55,
            scene_generation: 7,
            outcome: PolicyProjectionOutcome::Committed,
            expect_session_operation: true,
        },
    );
    let PolicyTransportEvent::SessionOperation {
        transaction,
        request,
    } = next(&worker, &peer)
    else {
        panic!("session operation missing");
    };
    assert_eq!(transaction, TransactionId::from_raw(12));
    assert_eq!(request.connection_epoch, 9);
    assert_eq!(request.request_id, 55);
    assert_eq!(request.operation, 11);
    assert_eq!(request.target, None);
    enqueue(
        &worker,
        PolicyTransportCommand::SessionOperationOutcome {
            transaction,
            request_id: 55,
            outcome: PolicyProjectionOutcome::Committed,
        },
    );
    assert!(matches!(
        next(&worker, &peer),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    enqueue(
        &worker,
        PolicyTransportCommand::PresentationReceipt {
            transaction: TransactionId::from_raw(102),
            receipt: PolicyPresentationReceipt {
                connection_epoch: 9,
                publication_generation: 1,
                output: OutputId::from_raw(1),
                output_generation: 3,
                presentation_epoch: 1,
                outcome: PolicyPresentationOutcome::Presented,
            },
        },
    );
    assert_eq!(
        peer.finish(Duration::from_secs(10)),
        "sophia_c_wm_sdk status=pass profiles=2 snapshots=1 surfaces=64 projections=1 operations=1 receipts=1\n"
    );
    drop(worker);
}
