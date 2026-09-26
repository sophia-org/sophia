//! Real worker/reactor and a raw-wire peer. Settlement outcomes below are
//! scripted; this does not assert Engine acceptance or actual presentation.
#![cfg(test)]
use super::super::startup::tests::{Peer, header, negotiate};
use super::*;
use crate::live_session::policy_transport_worker::{PolicyTransportEvent, PolicyTransportWorker};
use sophia_protocol::*;
use std::sync::mpsc::sync_channel;

use super::super::startup::tests::array_fixture as fixture;

fn enqueue(worker: &PolicyTransportWorker, mut command: PolicyTransportCommand) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match worker.try_command(command) {
            Ok(()) => return,
            Err(value) => command = value,
        }
        assert!(Instant::now() < deadline, "worker command deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn event(worker: &PolicyTransportWorker) -> PolicyTransportEvent {
    worker.event_timeout(Duration::from_secs(2)).unwrap()
}
fn accepted(peer: &mut Peer, bytes: &[u8]) {
    assert_eq!(peer.submit(bytes).unwrap().0, 119);
    let submitted = peer.next_event();
    assert_eq!(
        decode_wm_file_submitted(&submitted).unwrap().submission_id,
        decode_wm_file_record(bytes, WmFileClass::Candidate)
            .unwrap()
            .header
            .submission_id
    );
    peer.ack(&submitted);
    peer.clear_transaction();
}
fn configured() -> (PolicyTransportWorker, Peer, u64) {
    let caps = sophia_runtime::select_policy_capabilities(u64::MAX, u64::MAX, false);
    let (server, client) = UnixStream::pair().unwrap();
    let adapter = NinePPolicyAdapter::supplied(
        server,
        9,
        WmFileLimits {
            capability_ceiling: caps,
            profile_required: false,
        },
        WmQids::new(),
    )
    .unwrap();
    let worker = PolicyTransportWorker::spawn(adapter, 9, None).unwrap();
    let mut peer = Peer::from_stream(client);
    negotiate(&mut peer, caps);
    assert!(matches!(event(&worker), PolicyTransportEvent::Negotiated));
    let config = WmFileConfiguration {
        transaction: TransactionId::from_raw(10),
        configuration: PolicyConfiguration {
            connection_epoch: 9,
            generation: 3,
            actions: vec![],
            chrome: WmChromePolicy::default(),
        },
    };
    accepted(
        &mut peer,
        &encode_wm_file_configuration(header(WmFileKind::Configuration, 2), &config, caps).unwrap(),
    );
    assert!(
        matches!(event(&worker), PolicyTransportEvent::Configuration { transaction, configuration } if transaction == config.transaction && configuration == config.configuration)
    );
    enqueue(
        &worker,
        PolicyTransportCommand::ConfigurationOutcome {
            transaction: config.transaction,
            generation: 3,
            outcome: PolicyProjectionOutcome::Committed,
        },
    );
    let bytes = peer.next_event();
    assert_eq!(
        decode_wm_file_configuration_outcome(&bytes, caps).unwrap(),
        WmFileConfigurationOutcome {
            transaction: config.transaction,
            generation: 3,
            outcome: PolicyProjectionOutcome::Committed
        }
    );
    peer.ack(&bytes);
    assert!(
        matches!(event(&worker), PolicyTransportEvent::ReadyForCycle { capabilities } if capabilities == caps)
    );
    (worker, peer, caps)
}
fn receipt() -> PolicyPresentationReceipt {
    PolicyPresentationReceipt {
        connection_epoch: 9,
        publication_generation: 1,
        output: OutputId::from_raw(1),
        output_generation: 1,
        presentation_epoch: 1,
        outcome: PolicyPresentationOutcome::Presented,
    }
}

#[test]
fn full_worker_transports_all_commands_without_owning_their_outcomes() {
    let (worker, mut peer, caps) = configured();
    let scene = fixture::scene();
    let request = PolicyProjectionRequest {
        connection_epoch: 9,
        request_id: 5,
        scene_generation: scene.generation,
        policy_generation: 3,
        affected_outputs: vec![scene.active_output],
        cause: PolicyRequestCause::SceneChanged,
    };
    enqueue(
        &worker,
        PolicyTransportCommand::Cycle {
            snapshot_transaction: TransactionId::from_raw(100),
            request_transaction: TransactionId::from_raw(101),
            scene: Box::new(scene.clone()),
            actions: vec![],
            classifications: vec![],
            launch_origins: vec![],
            request: request.clone(),
        },
    );
    let bytes = peer.next_event();
    let cycle = decode_wm_file_cycle(&bytes, caps).unwrap();
    assert_eq!(cycle.request, request);
    assert_eq!(cycle.snapshot_transaction.raw(), 100);
    assert_eq!(cycle.request_transaction.raw(), 101);
    peer.open(6, b"snapshot", 0);
    let read = peer
        .rpc(
            116,
            &[
                6u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &65500u32.to_le_bytes(),
            ]
            .concat(),
        )
        .unwrap();
    assert_eq!(read.0, 117);
    let snapshot = decode_wm_file_snapshot(&read.1[4..], caps).unwrap();
    assert_eq!(snapshot.transaction, cycle.snapshot_transaction);
    assert_eq!(snapshot.snapshot.scene, scene);
    peer.ack(&bytes);
    let mut proposal = fixture::proposal();
    proposal.transaction = TransactionId::from_raw(20);
    proposal.connection_epoch = 9;
    proposal.request_id = request.request_id;
    proposal.base_generation = scene.generation;
    proposal.launch_contexts.clear();
    proposal.output_launch_contexts.clear();
    accepted(
        &mut peer,
        &encode_wm_file_projection(header(WmFileKind::Projection, 3), &proposal, caps).unwrap(),
    );
    assert!(
        matches!(event(&worker), PolicyTransportEvent::Projection(value) if *value == proposal)
    );
    enqueue(
        &worker,
        PolicyTransportCommand::ProjectionOutcome {
            transaction: proposal.transaction,
            request_id: request.request_id,
            scene_generation: scene.generation,
            outcome: PolicyProjectionOutcome::Committed,
            expect_session_operation: true,
        },
    );
    let bytes = peer.next_event();
    let outcome = decode_wm_file_projection_outcome(&bytes, caps).unwrap();
    assert!(outcome.expect_session_operation);
    assert_eq!(outcome.transaction, proposal.transaction);
    peer.ack(&bytes);
    let operation = WmFileSessionOperation {
        transaction: TransactionId::from_raw(30),
        request: PolicySessionOperationRequest {
            connection_epoch: 9,
            request_id: request.request_id,
            operation: 1,
            target: None,
        },
    };
    accepted(
        &mut peer,
        &encode_wm_file_session_operation(
            header(WmFileKind::SessionOperation, 4),
            &operation,
            caps,
        )
        .unwrap(),
    );
    assert!(
        matches!(event(&worker), PolicyTransportEvent::SessionOperation { transaction, request } if transaction == operation.transaction && request == operation.request)
    );
    enqueue(
        &worker,
        PolicyTransportCommand::SessionOperationOutcome {
            transaction: operation.transaction,
            request_id: operation.request.request_id,
            outcome: PolicyProjectionOutcome::Committed,
        },
    );
    let bytes = peer.next_event();
    let outcome = decode_wm_file_session_operation_outcome(&bytes, caps).unwrap();
    assert_eq!(outcome.transaction, operation.transaction);
    assert_eq!(outcome.outcome.outcome, PolicyProjectionOutcome::Committed);
    peer.ack(&bytes);
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    let receipt = receipt();
    enqueue(
        &worker,
        PolicyTransportCommand::PresentationReceipt {
            transaction: TransactionId::from_raw(40),
            receipt,
        },
    );
    let bytes = peer.next_event();
    assert_eq!(
        decode_wm_file_presentation_receipt(&bytes, caps).unwrap(),
        WmFilePresentationReceipt {
            transaction: TransactionId::from_raw(40),
            receipt
        }
    );
    peer.ack(&bytes);
    let dirty = PolicyDirtyRequest {
        connection_epoch: 9,
        policy_generation: 3,
        affected_outputs: vec![scene.active_output],
    };
    accepted(
        &mut peer,
        &encode_wm_file_dirty(header(WmFileKind::Dirty, 5), &dirty, caps).unwrap(),
    );
    assert!(matches!(event(&worker),PolicyTransportEvent::Dirty(value) if value==dirty));
    assert!(worker.try_command(PolicyTransportCommand::Stop).is_ok());
    drop(worker);
}

#[test]
fn real_adapter_stop_bypasses_full_journal_and_command_queue() {
    let (worker, _peer, _) = configured();
    // 64 retained receipts, one borrowed in-flight command, one queued command.
    for i in 0..66 {
        enqueue(
            &worker,
            PolicyTransportCommand::PresentationReceipt {
                transaction: TransactionId::from_raw(100 + i),
                receipt: receipt(),
            },
        );
    }
    assert!(
        worker
            .try_command(PolicyTransportCommand::PresentationReceipt {
                transaction: TransactionId::from_raw(999),
                receipt: receipt()
            })
            .is_err()
    );
    let (done, result) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        assert!(worker.try_command(PolicyTransportCommand::Stop).is_ok());
        drop(worker);
        done.send(()).unwrap();
    });
    result.recv_timeout(Duration::from_secs(1)).unwrap();
    thread.join().unwrap();
}
