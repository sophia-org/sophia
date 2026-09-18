#![cfg(all(test, unix))]

//! Focused controls over Session's private input service.
//!
//! WHAT THESE ESTABLISH AND WHAT THEY DO NOT. These are lifetime and custody
//! controls: that a running service is visible as running, that stopping
//! collects it, that custody read after collection says so, and that a
//! controller which goes out of scope still stops and joins even while an
//! adapter holds the runtime. The wire proof -- committed surfaces, focus, keys
//! and pointer frames -- belongs to the acceptance rows and is not duplicated
//! here.
//!
//! FAULTS ARE RAISED AGAINST A SERVICE THAT IS ACTUALLY RUNNING. A poisoned
//! lock or an exhausted bound means nothing if it is arranged before anything
//! exists to be damaged, so every control here connects a real peer over the
//! real socket first, and reads custody only after collection has happened.

// MOUNTED READ-ONLY AND NOT EDITED. This is the acceptance rows' own wire
// code; these controls use the part of it they need. The allowance is here
// rather than in that file precisely so it stays untouched: a fixture that
// exercises fewer of its frames than the acceptance rows do is not a reason to
// change it.
#[allow(dead_code)]
#[path = "private_input_acceptance/wire.rs"]
mod wire;

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sophia_input_authority::{InstanceId, SeatBinding};
use sophia_protocol::{
    NamespaceId, OutputId, OutputTopologyEntry, OutputTopologySnapshot, Rect, SeatId, Size,
};

use super::{
    PrivateInputConfig, PrivateInputGrantPolicy, PrivateInputHandle, PrivateInputInstanceCookie,
    PrivateInputReadiness, PrivateInputService, PrivateInputThreadJoin,
};
use sophia_x_authority::{PrivateCustodyJoinStanding, PrivateCustodyWorkerStanding};
use wire::{Order, Peer};

/// The bound every wait in here uses. `wire.rs` reads this from its parent.
pub const WAIT: Duration = Duration::from_secs(8);
const COOKIE: [u8; 32] = [0x73; 32];
static NEXT: AtomicU64 = AtomicU64::new(1);

fn config(socket: &Path, grants: PrivateInputGrantPolicy) -> PrivateInputConfig {
    let instance = InstanceId::new(911);
    let output = OutputId::from_raw(1);
    PrivateInputConfig {
        session_generation: 1,
        profile: sophia_protocol::NamespaceProfile::Confined,
        capabilities: sophia_protocol::NamespaceCapabilities::NONE,
        frame_clock: sophia_engine::DeterministicFrameClock::new(1, 16),
        socket_path: socket.to_owned(),
        namespace: NamespaceId::from_raw(911),
        binding: SeatBinding::new(instance, SeatId::from_raw(1)),
        cookie: PrivateInputInstanceCookie {
            instance,
            cookie: COOKIE,
        },
        grants,
        max_concurrent_clients: NonZeroUsize::new(4).unwrap(),
        input_capacity: NonZeroUsize::new(8).unwrap(),
        advertised_buttons: 9,
        output_topology: OutputTopologySnapshot {
            generation: 1,
            primary: output,
            outputs: vec![OutputTopologyEntry {
                output,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 320,
                    height: 240,
                },
                pixel_size: Size {
                    width: 320,
                    height: 240,
                },
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            }],
        },
    }
}

/// One started service, with its own directory and socket.
struct Fixture {
    handle: Option<PrivateInputHandle>,
    directory: PathBuf,
    socket: PathBuf,
}

impl Fixture {
    fn started(grants: PrivateInputGrantPolicy) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "m4-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let socket = directory.join("private.sock");
        let handle = PrivateInputService::start(config(&socket, grants)).unwrap();
        assert_eq!(
            handle.await_ready(WAIT).unwrap(),
            PrivateInputReadiness::Ready
        );
        Self {
            handle: Some(handle),
            directory,
            socket,
        }
    }

    fn handle(&self) -> &PrivateInputHandle {
        self.handle.as_ref().unwrap()
    }

    fn handle_mut(&mut self) -> &mut PrivateInputHandle {
        self.handle.as_mut().unwrap()
    }

    fn connect(&self) -> Peer {
        Peer::connect(&self.socket, Order::Little, Some(COOKIE)).unwrap()
    }

    /// Custody as it stands, read through the owner's own lease.
    fn custody(&self) -> sophia_x_authority::PrivateCustodySnapshot {
        let runtime = &self.handle().runtime;
        runtime
            .owner
            .custody_snapshot(&runtime.owner.lease())
            .unwrap()
    }

    /// Wait until a custody place reports a started worker.
    ///
    /// REQUIRED BEFORE ANY FAULT. Registered attachment is production and the
    /// acceptance rows collect registered workers, so `NeverStarted` is not
    /// lifetime evidence -- it is the state before the thing being tested has
    /// happened. Damaging a service that has not yet started a worker would
    /// prove nothing about custody at all.
    fn running_row(&mut self) -> sophia_x_authority::PrivateCustodySnapshotRow {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            let _ = self.handle_mut().apply_committed(Duration::from_millis(5));
            let snapshot = self.custody();
            if let Some(row) = snapshot
                .rows
                .iter()
                .find(|row| row.worker == PrivateCustodyWorkerStanding::Running)
            {
                return *row;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no custody place reported a started worker within the bound: {snapshot:?}"
            );
            std::thread::yield_now();
        }
    }

    /// Issue real input authority for the first admitted connection.
    ///
    /// THE ACTUAL ADAPTER CUSTODY. Cloning the runtime `Arc` is not this: a
    /// submission is what the design deliberately hands out, and it is the
    /// thing whose presence used to make the controller's drop skip its stop.
    fn issue_submission(&self) -> crate::private_input::PrivateInputSubmission {
        let admitted = self.handle().admitted().unwrap();
        let seen = *admitted.first().expect("the peer is admitted");
        let record = self
            .handle()
            .admission_record(seen.admission)
            .unwrap()
            .expect("the policy kept its own record");
        assert!(
            record.instance_verified,
            "the peer presented evidence bound to this instance"
        );
        self.handle()
            .issue(record.context, sophia_protocol::DeviceId::from_raw(1))
            .expect("grants are enabled and the connection is live")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.handle.take());
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

/// A diagnostic control, NOT the acceptance row.
///
/// `lifetime` is reserved for the full five-subcase body -- stop, command
/// loss, service error, serving-thread unwind and retained work -- and this
/// name exists so that this control cannot be mistaken for it or bound in its
/// place. A real service, a real connected peer, custody read while it runs and
/// again after it is collected, and an execution reported from after the join
/// rather than from before it.
///
/// EVERY ASSERTION HERE IS ABOUT SOMETHING THAT ACTUALLY HAPPENED. The peer is
/// a real X client over the real socket, so the worker whose custody this reads
/// is a worker that was actually started for it. Custody after collection is
/// read from the outcome's own retention rather than from a service that is
/// still running, which is the only point at which "collected" means anything.
#[test]
fn running_worker_is_collected() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();

    // A STARTED WORKER, WAITED FOR RATHER THAN ASSUMED. Registered attachment
    // is production; a place reporting NeverStarted is the state before the
    // thing under test has happened, not evidence about it.
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    assert_eq!(
        running.handle_present,
        Some(true),
        "a running worker's handle is in its slot: {running:?}"
    );
    assert_eq!(
        running.departing,
        Some(false),
        "nothing has told this connection to depart: {running:?}"
    );
    assert_eq!(
        running.join,
        PrivateCustodyJoinStanding::Unpublished,
        "no join can have been published while the worker runs: {running:?}"
    );
    assert!(
        running.publication_right_unclaimed,
        "no attempt has claimed the right to publish a join: {running:?}"
    );

    let whole = fixture.custody();
    assert!(!whole.inventory_poisoned, "{whole:?}");
    assert!(whole.taken >= 1, "{whole:?}");
    assert_eq!(
        whole.places, 4,
        "the inventory is sized to the configured client bound: {whole:?}"
    );

    // COLLECTED, then custody read afterwards through a runtime kept for the
    // purpose rather than through a service that is still running.
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "the service thread is joined by the stop: {outcome:?}"
    );

    // A JOINED THREAD IS NOT A LIVE EXECUTION. The reading reported comes from
    // the durable witness after the join, so it cannot claim an execution
    // belonging to a thread that has already gone.
    if let Some(execution) = outcome.execution {
        assert_ne!(
            execution.availability,
            sophia_x_authority::PrivateExecutionAvailability::Retained,
            "a joined thread must not report a retained execution: {outcome:?}"
        );
    }

    // EXACT RETAINED WORK. The bridge was readable throughout, so the count is
    // a count and not an absence, and custody is retained exactly when
    // something is still owed.
    let undelivered = outcome
        .bridge_undelivered
        .expect("the bridge was readable, so it reports a count");
    assert_eq!(
        outcome.retains_obligations(),
        undelivered > 0 || !outcome.settlement.readable,
        "custody is retained exactly when something is still owed: {outcome:?}"
    );

    // AFTER-JOIN CUSTODY. The worker was collected, so its place says so: the
    // handle has gone to whoever joined it and the result is published.
    let after = runtime
        .owner
        .custody_snapshot(&runtime.owner.lease())
        .expect("the owner still keeps its inventory after the stop");
    assert!(!after.inventory_poisoned, "{after:?}");
    let collected = after
        .rows
        .iter()
        .find(|row| row.join != PrivateCustodyJoinStanding::Unpublished);
    assert!(
        collected.is_some(),
        "collection publishes a join result into the custody place: {after:?}"
    );
    let collected = collected.unwrap();
    assert_ne!(
        collected.worker,
        PrivateCustodyWorkerStanding::Unreadable,
        "collection leaves a readable place: {collected:?}"
    );
    assert_ne!(
        collected.worker,
        PrivateCustodyWorkerStanding::NeverStarted,
        "a place that published a join started something: {collected:?}"
    );
    assert!(
        !collected.publication_right_unclaimed,
        "the attempt that published took the right: {collected:?}"
    );

    drop(peer);
    drop(outcome);
    drop(runtime);
}

/// A handle that goes out of scope while an adapter still holds the runtime
/// still stops and joins.
///
/// WHAT THIS CATCHES. Drop used to stop only when it held the last `Arc`, so a
/// live submission -- the one thing this design deliberately hands to adapters
/// -- turned a drop into no stop at all, and the runtime then dropped its
/// `JoinHandle` without joining. The submission is taken while the service is
/// genuinely running and is still alive when the controller goes.
#[test]
fn dropping_the_controller_stops_even_while_a_submission_is_held() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);

    // A REAL SUBMISSION, HELD ACROSS THE DROP. This is the custody the design
    // hands to adapters, and holding it is what used to make the controller's
    // drop skip its stop and leave the JoinHandle dropped unjoined.
    let submission = fixture.issue_submission();
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    drop(fixture.handle.take());

    assert!(
        runtime.stop_once().is_none(),
        "the controller's drop performed the one stop even though a submission was live"
    );
    assert!(
        runtime
            .thread
            .lock()
            .map(|held| held.is_none())
            .unwrap_or(false),
        "the service thread was joined rather than dropped unjoined"
    );
    // Still held, which is the whole point of the arrangement.
    drop(submission);
    drop(peer);
    drop(runtime);
}

/// An explicit stop followed by the controller's drop joins exactly once.
#[test]
fn stopping_twice_joins_once() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::Disabled);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();
    assert_eq!(outcome.service_thread, PrivateInputThreadJoin::Joined);
    assert!(
        runtime.stop_once().is_none(),
        "the explicit stop is the only stop"
    );
}

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
    let _window = peer.create_map_and_draw();
    let _ = fixture
        .handle_mut()
        .apply_committed(Duration::from_millis(10));

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::thread::spawn(move || {
        let _held = runtime.admitted_surfaces.lock().unwrap();
        panic!("poisoning the surface ledger on purpose");
    });
    assert!(poisoner.join().is_err());

    // Refused, and specifically not reported as a step that committed nothing.
    assert!(
        fixture
            .handle_mut()
            .apply_committed(Duration::from_millis(10))
            .is_err(),
        "an unreadable ledger is refused rather than read as empty"
    );
    drop(peer);
}

/// A stale connection is refused before any control identity is spent on it.
#[test]
fn a_control_for_a_departed_connection_is_refused() {
    let fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let admitted = fixture.handle().admitted().unwrap();
    let seen = *admitted.first().expect("the peer is admitted");

    let connection = crate::private_input::PrivateInputConnection {
        client: seen.client,
        admission: seen.admission,
        // A generation no live row carries.
        connection_generation: seen.connection_generation.wrapping_add(1),
    };
    let refused = fixture.handle().submit_action(
        connection,
        crate::private_input::PrivateInputAction::ClearFocus {
            surface: sophia_protocol::SurfaceId::new(1, 1),
        },
    );
    assert!(
        matches!(
            refused,
            Err(crate::private_input::PrivateInputControlError::ConnectionGone)
        ),
        "a connection generation no live row carries is refused: {refused:?}"
    );
    drop(peer);
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
    let refused = PrivateInputService::start(configured).err();
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
    let refused = PrivateInputService::start(configured).err();
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
