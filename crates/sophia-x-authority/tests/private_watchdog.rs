//! Real owned supervision against blocked worker fixtures, without live sockets.
#![cfg(unix)]

mod implementation {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/x11_socket/routing/private_watchdog.rs"
    ));

    #[test]
    fn failed_transport_registration_drop_keeps_shutdown_ownership() {
        use std::io::Read;
        let mut owner = PrivateWatchdogOwner::prepare(1).unwrap();
        let (socket, mut peer) = UnixStream::pair().unwrap();
        let _live_writer = socket.try_clone().unwrap();
        peer.set_read_timeout(Some(super::TEST_LIMIT)).unwrap();
        let registration = owner
            .attach_transport(socket)
            .unwrap_or_else(|_| panic!("slot refused"));
        let gate = owner.seal().unwrap();
        // Stage an already-recorded failure while the idle supervisor has no
        // notification. Dropping the registration before its shutdown scan is
        // deterministic here; closing only its clone would leave live_writer.
        {
            let mut inventory = owner.shared.lock();
            inventory.failure = Some(PrivateWatchdogFailure {
                cause: PrivateWatchdogCause::OwnerDropped,
                execution: None,
                phase: None,
                elapsed: None,
            });
            owner.shared.closed.store(true, Ordering::Release);
        }
        drop(registration);
        assert!(owner.shared.lock().transports[0].is_some());
        owner.shared.changed.notify_all();
        super::wait_for_exit(&gate);
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }

    #[test]
    fn exhausted_lifecycle_identity_stops_instead_of_wrapping() {
        let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
        let gate = owner.seal().unwrap();
        owner.shared.lock().next_identity = None;
        assert!(matches!(
            owner.begin_dequeued(Instant::now()),
            Err(PrivateWatchdogRefusal::Failed(_))
        ));
        assert_eq!(
            gate.failure().unwrap().cause,
            PrivateWatchdogCause::IdentityExhausted
        );
        assert!(!gate.allows_execution());
        super::wait_for_exit(&gate);
    }

    #[test]
    fn poison_closes_the_lifecycle_and_does_not_orphan_the_supervisor() {
        let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
        let gate = owner.seal().unwrap();
        let execution = owner.begin_dequeued(Instant::now()).unwrap();
        let poison = owner.shared.clone();
        assert!(
            std::thread::spawn(move || {
                let _held = poison.inventory.lock().unwrap();
                panic!("interrupted watchdog state");
            })
            .join()
            .is_err()
        );
        let failure = gate.failure().unwrap();
        assert_eq!(failure.cause, PrivateWatchdogCause::StateUnavailable);
        assert!(!gate.allows_execution());
        super::wait_for_exit(&gate);
        assert!(owner.reap_finished().unwrap().is_ok());
        assert_eq!(
            execution.finish(),
            Err(PrivateWatchdogRefusal::Failed(failure))
        );
        assert!(owner.shared.inventory.is_poisoned());
    }
}

use implementation::{
    PRIVATE_EXECUTION_DEADLINE, PrivateWatchdogCause, PrivateWatchdogGate, PrivateWatchdogOwner,
    PrivateWatchdogRefusal,
};
use sophia_input_authority::ExecutionPhase;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const TEST_LIMIT: Duration = Duration::from_secs(2);

fn wait_for_exit(gate: &PrivateWatchdogGate) {
    let deadline = Instant::now() + TEST_LIMIT;
    while !gate.supervisor_finished() {
        assert!(Instant::now() < deadline, "supervisor did not return");
        std::thread::park_timeout(Duration::from_millis(1));
    }
}

#[test]
fn live_owner_requires_sealed_preparation_and_preserves_transport_on_refusal() {
    let mut owner = PrivateWatchdogOwner::prepare(1).unwrap();
    assert!(matches!(
        owner.begin_dequeued(Instant::now()),
        Err(PrivateWatchdogRefusal::NotSealed)
    ));
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let registration = owner
        .attach_transport(stream)
        .unwrap_or_else(|_| panic!("first slot refused"));
    let (mut excess, mut excess_peer) = UnixStream::pair().unwrap();
    excess = match owner.attach_transport(excess) {
        Err((PrivateWatchdogRefusal::TransportCapacity, returned)) => returned,
        _ => panic!("full transport inventory accepted or lost source"),
    };
    excess.write_all(b"owned").unwrap();
    let mut bytes = [0; 5];
    excess_peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"owned");
    let gate = owner.seal().unwrap();
    assert!(gate.allows_execution());
    drop(registration);
    assert_eq!(peer.read(&mut bytes).unwrap(), 0);
    assert!(matches!(
        owner.attach_transport(excess),
        Err((PrivateWatchdogRefusal::Sealed, _))
    ));
    drop(owner);
    wait_for_exit(&gate);
    assert!(!gate.allows_execution());
    assert_eq!(gate.failure(), None);
}

#[test]
fn completed_execution_releases_the_slot_without_resetting_live_execution() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let mut execution = owner.begin_dequeued(Instant::now()).unwrap();
    assert!(matches!(
        owner.begin_dequeued(Instant::now()),
        Err(PrivateWatchdogRefusal::Busy)
    ));
    assert_eq!(
        execution.committed(),
        Err(PrivateWatchdogRefusal::InvalidTransition)
    );
    execution.applying().unwrap();
    assert_eq!(
        execution.applying(),
        Err(PrivateWatchdogRefusal::InvalidTransition)
    );
    execution.committed().unwrap();
    execution.finish().unwrap();
    owner
        .begin_dequeued(Instant::now())
        .unwrap()
        .finish()
        .unwrap();
    assert!(gate.allows_execution());
    assert_eq!(gate.failure(), None);
    assert!(owner.reap_finished().is_none());
    drop(owner);
    wait_for_exit(&gate);
}

#[test]
fn actual_dequeue_time_is_kept_even_if_registration_is_delayed() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let old = Instant::now() - PRIVATE_EXECUTION_DEADLINE;
    assert!(matches!(
        owner.begin_dequeued(old),
        Err(PrivateWatchdogRefusal::Failed(_))
    ));
    let failure = gate.failure().unwrap();
    assert_eq!(failure.cause, PrivateWatchdogCause::Deadline);
    assert_eq!(failure.phase, Some(ExecutionPhase::BeforeGuards));
    assert!(failure.elapsed.unwrap() >= PRIVATE_EXECUTION_DEADLINE);
    assert!(!gate.allows_execution());
    wait_for_exit(&gate);
    assert!(owner.reap_finished().unwrap().is_ok());
}

#[test]
fn lock_acquisition_stall_expires_without_taking_the_execution_lock() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let common = Arc::new(Mutex::new(()));
    let held_common = common.lock().unwrap();
    let (began_tx, began_rx) = mpsc::sync_channel(1);
    let (returned_tx, returned_rx) = mpsc::sync_channel(1);
    let worker_common = common.clone();
    // The sentinel represents inventory the worker retains while stuck. The
    // supervisor receives none of it and cannot manufacture its disposition.
    let inventory = Arc::new(());
    let weak_inventory = Arc::downgrade(&inventory);
    let worker = std::thread::spawn(move || {
        let _inventory = inventory;
        let execution = owner.begin_dequeued(Instant::now()).unwrap();
        began_tx.send(()).unwrap();
        let _guard = worker_common.lock().unwrap();
        let returned = execution.finish();
        returned_tx.send(returned).unwrap();
    });
    began_rx.recv_timeout(TEST_LIMIT).unwrap();
    wait_for_exit(&gate);
    assert!(!gate.allows_execution());
    let failure = gate.failure().unwrap();
    assert_eq!(failure.cause, PrivateWatchdogCause::Deadline);
    assert_eq!(failure.phase, Some(ExecutionPhase::BeforeGuards));
    assert!(weak_inventory.upgrade().is_some());
    assert!(returned_rx.try_recv().is_err());
    drop(held_common);
    assert!(
        matches!(returned_rx.recv_timeout(TEST_LIMIT).unwrap(), Err(PrivateWatchdogRefusal::Failed(found)) if found == failure)
    );
    worker.join().unwrap();
    assert!(weak_inventory.upgrade().is_none());
}

#[test]
fn post_commit_stall_cannot_be_overwritten_by_late_finish() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let mut execution = owner.begin_dequeued(Instant::now()).unwrap();
    execution.applying().unwrap();
    execution.committed().unwrap();
    wait_for_exit(&gate);
    let failure = gate.failure().unwrap();
    assert_eq!(failure.cause, PrivateWatchdogCause::Deadline);
    assert_eq!(failure.phase, Some(ExecutionPhase::Committed));
    assert_eq!(
        execution.finish(),
        Err(PrivateWatchdogRefusal::Failed(failure))
    );
    assert_eq!(gate.failure(), Some(failure));
    assert!(
        matches!(owner.begin_dequeued(Instant::now()), Err(PrivateWatchdogRefusal::Failed(found)) if found == failure)
    );
    assert!(owner.reap_finished().unwrap().is_ok());
}

#[test]
fn dropping_execution_latches_failure_even_before_the_deadline() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let execution = owner.begin_dequeued(Instant::now()).unwrap();
    drop(execution);
    assert!(!gate.allows_execution());
    let failure = gate.failure().unwrap();
    assert_eq!(failure.cause, PrivateWatchdogCause::ExecutionAbandoned);
    assert_eq!(failure.phase, Some(ExecutionPhase::BeforeGuards));
    wait_for_exit(&gate);
    assert_eq!(gate.failure(), Some(failure));
}

#[test]
fn owner_drop_does_not_wait_for_a_running_execution() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let execution = owner.begin_dequeued(Instant::now()).unwrap();
    let (dropped_tx, dropped_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        drop(owner);
        dropped_tx.send(()).unwrap();
    });
    dropped_rx.recv_timeout(TEST_LIMIT).unwrap();
    assert!(!gate.allows_execution());
    assert_eq!(
        gate.failure().unwrap().cause,
        PrivateWatchdogCause::OwnerDropped
    );
    wait_for_exit(&gate);
    assert!(matches!(
        execution.finish(),
        Err(PrivateWatchdogRefusal::Failed(_))
    ));
}

#[test]
fn independent_descriptor_ends_a_blocked_write_while_output_lock_is_held() {
    let (writer, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(TEST_LIMIT)).unwrap();
    let shutdown = writer.try_clone().unwrap();
    let output = Arc::new(Mutex::new(writer));
    let mut owner = PrivateWatchdogOwner::prepare(1).unwrap();
    let registration = owner
        .attach_transport(shutdown)
        .unwrap_or_else(|_| panic!("transport refused"));
    let gate = owner.seal().unwrap();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let worker_output = output.clone();
    let worker = std::thread::spawn(move || {
        let _registration = registration;
        let mut socket = worker_output.lock().unwrap();
        let mut execution = owner.begin_dequeued(Instant::now()).unwrap();
        execution.applying().unwrap();
        entered_tx.send(()).unwrap();
        let payload = [0_u8; 16_384];
        let failure = loop {
            if let Err(error) = socket.write_all(&payload) {
                break error;
            }
        };
        result_tx
            .send((failure.kind(), execution.finish()))
            .unwrap();
    });
    entered_rx.recv_timeout(TEST_LIMIT).unwrap();
    assert!(output.try_lock().is_err());
    // The peer never reads before the write has failed and the worker returned.
    let (_, finish) = result_rx.recv_timeout(TEST_LIMIT).unwrap();
    assert!(matches!(finish, Err(PrivateWatchdogRefusal::Failed(_))));
    assert_eq!(
        gate.failure().unwrap().phase,
        Some(ExecutionPhase::Applying)
    );
    assert_eq!(
        gate.failure().unwrap().cause,
        PrivateWatchdogCause::Deadline
    );
    worker.join().unwrap();
    wait_for_exit(&gate);
    let mut buffered = Vec::new();
    peer.read_to_end(&mut buffered).unwrap();
    assert!(!buffered.is_empty());
}

#[test]
fn every_attached_transport_is_closed_when_execution_is_abandoned() {
    let mut owner = PrivateWatchdogOwner::prepare(2).unwrap();
    let mut peers = Vec::new();
    let mut registrations = Vec::new();
    let mut live_writers = Vec::new();
    for _ in 0..2 {
        let (stream, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(TEST_LIMIT)).unwrap();
        peers.push(peer);
        live_writers.push(stream.try_clone().unwrap());
        registrations.push(
            owner
                .attach_transport(stream)
                .unwrap_or_else(|_| panic!("slot refused")),
        );
    }
    let gate = owner.seal().unwrap();
    drop(owner.begin_dequeued(Instant::now()).unwrap());
    // Dropping registrations after failure must not race shutdown out of the
    // supervisor's inventory before it has selected both independent fds.
    drop(registrations);
    wait_for_exit(&gate);
    for mut peer in peers {
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
    assert_eq!(
        gate.failure().unwrap().cause,
        PrivateWatchdogCause::ExecutionAbandoned
    );
}

#[test]
fn deliberate_wait_before_dequeue_consumes_no_executor_deadline() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    std::thread::park_timeout(PRIVATE_EXECUTION_DEADLINE + Duration::from_millis(10));
    assert!(gate.allows_execution());
    assert_eq!(gate.failure(), None);
    owner
        .begin_dequeued(Instant::now())
        .unwrap()
        .finish()
        .unwrap();
    drop(owner);
    wait_for_exit(&gate);
}

#[test]
fn a_future_dequeue_time_fails_closed_instead_of_extending_the_deadline() {
    let mut owner = PrivateWatchdogOwner::prepare(0).unwrap();
    let gate = owner.seal().unwrap();
    let future = Instant::now() + Duration::from_secs(3600);
    assert!(matches!(
        owner.begin_dequeued(future),
        Err(PrivateWatchdogRefusal::Failed(_))
    ));
    assert!(!gate.allows_execution());
    assert_eq!(
        gate.failure().unwrap().cause,
        PrivateWatchdogCause::InvalidDequeueTime
    );
    wait_for_exit(&gate);
}
