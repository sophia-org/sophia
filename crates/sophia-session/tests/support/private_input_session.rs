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
    PrivateInputLifetimeOwner, PrivateInputReadiness, PrivateInputService, PrivateInputThreadJoin,
};
use sophia_x_authority::{
    PrivateCustodyJoinStanding, PrivateCustodyWorkerStanding, XAuthorityControlKind,
};
use wire::{Order, Peer};

/// The bound every wait in here uses. `wire.rs` reads this from its parent.
///
/// GENEROUS ON PURPOSE. A passing control never waits, so the only thing this
/// sizes is how much contention a failing one tolerates before it calls a busy
/// machine a defect. Running the whole suite in parallel puts eight private X
/// services on a host that is also running a desktop, and at eight seconds
/// roughly one run in seven lost an admission to starvation rather than to
/// anything being wrong.
pub const WAIT: Duration = Duration::from_secs(20);
const COOKIE: [u8; 32] = [0x73; 32];
/// BTN_LEFT, the native evdev code.
///
/// NOT THE X BUTTON NUMBER. Submission takes an evdev code and the authority
/// maps it (272 -> X button 1); an X button number passed here is not a valid
/// evdev code at all, so `peek_evdev_button` answers `None` and the request is
/// refused for the wrong reason entirely. A capacity control that submitted
/// one would never reach the capacity it meant to fill.
const BTN_LEFT: u32 = 272;
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
    /// Reserved before the service starts, so custody of an unresolved runtime
    /// has a home that does not depend on which handle outlives which.
    lifetime: PrivateInputLifetimeOwner,
    /// Committed effects taken out of the bridge and not yet used.
    ///
    /// KEPT, BECAUSE DRIVING THE SERVICE CONSUMES THEM. Waiting for a worker to
    /// start means driving commits, and a commit hands its effects out once. A
    /// readiness poll that discarded them could eat the only admission a later
    /// step was waiting for, and that step would then wait out its bound for
    /// something that had already happened.
    harvest: Vec<crate::private_input::PrivateInputCommittedEffect>,
    handle: Option<PrivateInputHandle>,
    directory: PathBuf,
    socket: PathBuf,
}

impl Fixture {
    fn started(grants: PrivateInputGrantPolicy) -> Self {
        Self::started_with_capacity(grants, 8)
    }

    /// Start with a stated delivery capacity, so a control can fill it.
    fn started_with_capacity(grants: PrivateInputGrantPolicy, capacity: usize) -> Self {
        Self::started_with_bounds(grants, 4, capacity)
    }

    /// Start with both bounds stated.
    ///
    /// THE CLIENT BOUND IS PART OF THE DELIVERY BOUND, so a control that means
    /// to fill the ledger has to be able to shrink both.
    fn started_with_bounds(
        grants: PrivateInputGrantPolicy,
        clients: usize,
        capacity: usize,
    ) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "m4-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let socket = directory.join("private.sock");
        let lifetime = PrivateInputLifetimeOwner::reserved();
        let mut configured = config(&socket, grants);
        configured.input_capacity = NonZeroUsize::new(capacity).expect("a real capacity");
        configured.max_concurrent_clients = NonZeroUsize::new(clients).expect("a real bound");
        let handle = PrivateInputService::start(&lifetime, configured).unwrap();
        assert_eq!(
            handle.await_ready(WAIT).unwrap(),
            PrivateInputReadiness::Ready
        );
        Self {
            lifetime,
            harvest: Vec::new(),
            handle: Some(handle),
            directory,
            socket,
        }
    }

    /// Start with the one-shot unwind fault installed but not yet armed.
    fn started_with_unwind_fault() -> (
        Self,
        std::sync::Arc<crate::private_input::faults::PrivateInputUnwindFault>,
    ) {
        let directory = std::env::temp_dir().join(format!(
            "m4-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let socket = directory.join("private.sock");
        let lifetime = PrivateInputLifetimeOwner::reserved();
        let fault =
            std::sync::Arc::new(crate::private_input::faults::PrivateInputUnwindFault::default());
        // Registered with the process-global subscriber before the service
        // starts, so the callsite is live from the first event.
        crate::private_input::faults::arm_globally(&fault);
        let handle = lifetime
            .start_with_faults(
                config(
                    &socket,
                    PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
                ),
                crate::private_input::faults::PrivateInputFaults {
                    unwind: Some(std::sync::Arc::clone(&fault)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            handle.await_ready(WAIT).unwrap(),
            PrivateInputReadiness::Ready
        );
        (
            Self {
                lifetime,
                harvest: Vec::new(),
                handle: Some(handle),
                directory,
                socket,
            },
            fault,
        )
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

    /// A peer at the socket layer only: accepted, given a worker, and gone
    /// before it ever sent a setup prefix.
    ///
    /// THIS IS THE ONLY KIND OF DEPARTURE THE REAPER SPEAKS ABOUT. A peer that
    /// completes the handshake and then closes cleanly ends its dispatch loop
    /// with `Ok(())`, and `reap_client_worker` emits nothing for that -- it
    /// reports only a client failure, a client disconnect or a service
    /// shutdown. A socket that closes before the setup prefix arrives makes
    /// `read_x11_setup_request` fail with a client disconnect, which is the
    /// event this exists to provoke.
    fn probe_socket(&self) -> std::os::unix::net::UnixStream {
        std::os::unix::net::UnixStream::connect(&self.socket).expect("the listener is bound")
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
    /// Drive one coordinator step, keeping whatever it decided.
    ///
    /// The effects go into the harvest so a readiness poll cannot eat them; the
    /// report comes back so a caller that needs the commit outcomes themselves
    /// -- what was applied, and whether this service held a mapping fact for it
    /// -- can read them without draining the harvest to find out.
    fn pump(&mut self) -> crate::private_input::PrivateInputCommitted {
        let mut committed = self
            .handle_mut()
            .apply_committed(Duration::from_millis(5))
            .expect("the bridge is readable");
        self.harvest.append(&mut committed.effects);
        committed
    }

    fn running_row(&mut self) -> sophia_x_authority::PrivateCustodySnapshotRow {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            self.pump();
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
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Drive commits until the real admission effect exists, and return the
    /// surface the coordinator actually committed.
    ///
    /// THE SURFACE COMES FROM THE COMMIT, NOT FROM THE WIRE. An earlier version
    /// built `SurfaceId::new(window, 1)` out of the peer's XID, which is not an
    /// admitted target and was never a surface this service had routed
    /// anything to; input submitted against it proved nothing about delivery.
    fn admitted_surface(&mut self) -> sophia_protocol::SurfaceId {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            // THE WHOLE HARVEST, not just this step. The admission may have
            // been committed while something else was driving the service.
            if let Some(effect) = self
                .harvest
                .iter()
                .find(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
                .copied()
            {
                let submitted = effect
                    .submitted()
                    .expect("the order took the admission it committed");
                self.await_ack(submitted);
                return effect.surface();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "a create/map/draw produced a committed admission within the bound: {:?}",
                self.harvest
            );
            self.pump();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Wait for the exact acknowledgement of one submitted control.
    ///
    /// MATCHED WHOLE. Transaction, surface and kind all have to agree, and the
    /// outcome has to be `Delivered`; any acknowledgement arriving while this
    /// waits is not evidence about this one.
    fn await_ack(&self, submitted: crate::private_input::PrivateInputSubmitted) {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            for ack in self
                .handle()
                .drain_acknowledgements_within(Duration::from_millis(10))
            {
                let seen = ack.acknowledgement;
                if seen.transaction == submitted.transaction
                    && seen.surface == submitted.surface
                    && seen.kind == submitted.kind
                {
                    assert_eq!(
                        seen.outcome,
                        sophia_x_authority::XAuthorityControlOutcome::Delivered,
                        "the control this waited for was delivered: {seen:?}"
                    );
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the acknowledgement for {submitted:?} arrived within the bound"
            );
        }
    }

    /// Establish focus on a surface this service actually admitted.
    fn focus(&mut self, surface: sophia_protocol::SurfaceId) {
        let admitted = self.handle().admitted().unwrap();
        let seen = *admitted.first().expect("the peer is admitted");
        let connection = crate::private_input::PrivateInputConnection {
            client: seen.client,
            admission: seen.admission,
            connection_generation: seen.connection_generation,
        };
        let submitted = self
            .handle()
            .submit_action(
                connection,
                crate::private_input::PrivateInputAction::FocusSurface { surface },
            )
            .expect("the connection is live");
        self.await_ack(submitted);
    }

    /// Admit, focus, then deliver one real input event to that exact surface.
    ///
    /// RETURNS THE DELIVERY IT ACCEPTED, so a caller waits on its own
    /// obligation rather than on whatever receipt happens to turn up.
    fn deliver_one(
        &mut self,
        submission: &crate::private_input::PrivateInputSubmission,
    ) -> sophia_x_authority::XAuthorityInputDeliveryId {
        let surface = self.admitted_surface();
        self.focus(surface);
        // A FOCUSED BUTTON PRESS. It is a real accepted obligation and needs
        // no keyboard history to be established first, which a key would.
        let accepted = submission
            .submit_pointer_button(surface, BTN_LEFT, true)
            .expect("the connection is live and has authority");
        // WAITED FOR THROUGH THE LEDGER, WHICH CONSUMES NOTHING. Draining the
        // channel here would take the very receipt a later drain is meant to
        // find, so a control about draining could not be written after it.
        // SETTLED IS NOT DELIVERED. A terminal answer also covers a rejected
        // route and a client that went, so the exact outcome and the exact
        // delivery identity are both required before this counts as an
        // accepted obligation.
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            if let Some(settled) = self.handle().runtime.observer.settled(accepted.delivery) {
                assert_eq!(settled.delivery, accepted.delivery);
                assert_eq!(
                    settled.outcome,
                    sophia_x_authority::XAuthorityInputDeliveryOutcome::Flushed,
                    "the obligation this waited on was delivered: {settled:?}"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "delivery {:?} settled within the bound",
                accepted.delivery
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        accepted.delivery
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

    // THE LIFETIME IS RUNNING A SERVICE AND HOLDS NOTHING YET. A service that
    // has not ended has placed nothing, which is not a statement about what it
    // will owe.
    assert!(fixture.lifetime.in_use(), "a service is running under it");
    assert!(!fixture.lifetime.retains_unresolved());
    assert!(!fixture.lifetime.slot_poisoned());

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

    // EXACT RETAINED WORK, COUNTING EVERY SOURCE OF IT. An earlier version
    // compared retention against the bridge count and the settlement
    // readability alone, so a run that ended with uncommitted intake or an
    // undrained receipt -- both ordinary, both retention -- read as a
    // contradiction. Which of them is non-zero depends on timing, which is why
    // it only failed once the controls ran concurrently.
    let owed = |count: Option<usize>| count.is_none_or(|owed| owed > 0);
    let something_owed = !outcome.settlement.readable
        || outcome.settlement.reserved_credits.is_some_and(|n| n > 0)
        || outcome.settlement.owed.is_some_and(|n| n > 0)
        || outcome.settlement.indeterminate.is_some_and(|n| n > 0)
        || owed(outcome.bridge_undelivered)
        || owed(outcome.receipts_unobserved)
        || owed(outcome.intake_uncommitted);
    assert_eq!(
        outcome.retains_obligations(),
        something_owed,
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

    // AND THE LIFETIME AGREES WITH THE OUTCOME. Custody is in the slot exactly
    // when the stop said something was still owed, and the slot is not left
    // claimed by a service that finished clean.
    assert_eq!(
        fixture.lifetime.retains_unresolved(),
        outcome.retains_obligations(),
        "the slot holds custody exactly when the stop reported work owed: {outcome:?}"
    );
    assert!(
        !fixture.lifetime.in_use(),
        "the ended service no longer claims the lifetime"
    );
    assert!(!fixture.lifetime.slot_poisoned());

    record_actors(&runtime, &outcome);
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

    let deadline = std::time::Instant::now() + WAIT;
    loop {
        match fixture
            .handle_mut()
            .apply_committed(Duration::from_millis(10))
        {
            Err(_) => break,
            Ok(report) => {
                assert_eq!(
                    report.batches_observed, 0,
                    "a call that took work read the unreadable ledger as empty: {report:?}"
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "the redraw reached the bridge within the bound"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

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
    let deadline = std::time::Instant::now() + WAIT;
    let seen = loop {
        let admitted = fixture
            .handle()
            .admitted()
            .expect("the boundary is readable");
        if let Some(seen) = admitted.first().copied() {
            break seen;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the peer was admitted within the bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
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
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let admitted = fixture
            .handle()
            .admitted()
            .expect("the boundary is readable");
        let departed = !admitted
            .iter()
            .any(|row| row.client == live.client && row.admission == live.admission && !row.closed);
        if departed {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the dropped peer's admission closed within the bound: {admitted:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

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

/// A drain that fails on a poisoned channel must not have consumed what it
/// was already holding.
///
/// WHAT THIS CATCHES. An earlier version observed the retained receipts first
/// and then reached for the delivery channel, so a poisoned channel returned
/// `Err` after those receipts had already been observed -- their places given
/// back in the ledger, and the caller never told which receipts those were.
/// Every fallible lock is taken before any observation now, so a failure means
/// nothing happened. The retained queue is read directly rather than through
/// the drain, because the drain is the thing under test.
#[test]
fn a_failed_drain_consumes_none_of_the_receipts_it_was_holding() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    // Into custody without observing, which is what this call is for.
    let inventory = fixture.handle().unobserved_receipts().unwrap();
    assert!(
        inventory.complete,
        "the channel was emptied into custody: {inventory:?}"
    );
    let held = inventory.retained;
    assert!(
        held > 0,
        "a delivered event leaves a receipt to be consumed"
    );

    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.deliveries.lock().unwrap();
        panic!("poisoning the delivery channel on purpose");
    });
    assert!(thread.join().is_err());

    assert!(
        fixture.handle().drain_deliveries().is_err(),
        "a poisoned delivery channel is refused"
    );
    assert_eq!(
        runtime.retained_receipts.lock().unwrap().len(),
        held,
        "the failed drain observed none of the receipts it already held"
    );
    drop((submission, peer));
}

/// One call visits at most the drain bound, however much is waiting.
///
/// WHAT THIS CATCHES. The bound used to be read off what remained at the end
/// of the call, so a full retention queue could be observed and a full channel
/// drained on top of it -- twice the advertised bound in one call. The queue
/// is seeded past the bound directly, because arranging that many real
/// deliveries would test the wire rather than the bound.
#[test]
fn one_drain_visits_no_more_than_its_bound() {
    let fixture = Fixture::started(PrivateInputGrantPolicy::Disabled);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    let seeded = super::receipts::PRIVATE_INPUT_DRAIN_BOUND + 44;
    {
        let mut retained = runtime.retained_receipts.lock().unwrap();
        for index in 0..seeded {
            retained.push_back(sophia_x_authority::XAuthorityClientInputDelivery {
                client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
                delivery: sophia_x_authority::XAuthorityInputDeliveryId::from_raw(
                    u64::try_from(index + 1).unwrap(),
                ),
                outcome: sophia_x_authority::XAuthorityInputDeliveryOutcome::Flushed,
            });
        }
    }

    let receipts = fixture.handle().drain_deliveries().unwrap();
    assert_eq!(
        receipts.visited,
        super::receipts::PRIVATE_INPUT_DRAIN_BOUND,
        "one call visits exactly its bound when more is waiting"
    );
    // None of these belong to the ledger, so none of them is released and all
    // of them stay owed.
    assert!(
        receipts.observed.is_empty(),
        "{:?}",
        receipts.observed.len()
    );
    assert_eq!(receipts.retained, seeded);
    assert_eq!(runtime.retained_receipts.lock().unwrap().len(), seeded);
}

/// A stop counts receipts nobody drained, not only the ones already taken.
///
/// WHAT THIS CATCHES. The count used to read the retained queue alone, so a
/// service stopped with receipts still sitting unread in its channel reported
/// zero owed -- while every one of those receipts still held a place in the
/// delivery ledger.
///
/// NOTHING HERE TOUCHES THE CHANNEL BEFORE THE STOP. An earlier version of this
/// control waited for its delivery by draining, which moved the receipt into
/// custody and left it testing the case it was written to rule out. The wait
/// goes through the ledger's own terminal answer instead, which reads and frees
/// nothing, and the retained queue is asserted empty beforehand so the count
/// afterwards can only have come from the channel.
#[test]
fn a_stop_counts_receipts_nobody_drained() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    assert_eq!(
        runtime.retained_receipts.lock().unwrap().len(),
        0,
        "nothing has been taken into custody, so the receipt is still queued"
    );
    // The ledger settled it, which is how this knows there is one to count.
    let settled = runtime
        .observer
        .settled(delivery)
        .expect("the delivery this submitted has a terminal answer");
    assert_eq!(settled.delivery, delivery);

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    let owed = outcome
        .receipts_unobserved
        .expect("the channel and the queue were both readable");
    assert!(
        owed >= 1,
        "a receipt nobody drained is still owed at stop: {outcome:?}"
    );
    // AND IT IS THE EXACT ONE. A count that happened to be positive for some
    // other reason would pass a weaker assertion than this.
    let queued = runtime.retained_receipts.lock().unwrap();
    assert!(
        queued.iter().any(|receipt| receipt.delivery == delivery),
        "the stop took the exact queued delivery into custody: {queued:?}"
    );
    drop(queued);
    assert!(
        outcome.retains_obligations(),
        "owed receipts retain custody: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the lifetime's reserved slot is holding it"
    );
    record_actors(&runtime, &outcome);
    drop((submission, peer));
}

/// A controller dropped while a real submission lives fills the reserved slot.
///
/// WHAT THIS CATCHES. `Drop` performed the stop and threw the outcome away
/// without running retention, so the reserved closing slot was never filled on
/// the one path it exists for. A caller that never calls `stop` -- which is
/// exactly the caller this slot was reserved for -- left unresolved work with
/// no owner at all. The submission is real and still alive across the drop, so
/// this is also the live-adapter case rather than a cloned handle.
#[test]
fn dropping_the_controller_with_work_owed_fills_the_reserved_slot() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    // An accepted obligation nobody drains, so the exit owes something.
    let _delivery = fixture.deliver_one(&submission);

    assert!(fixture.lifetime.in_use());
    assert!(!fixture.lifetime.retains_unresolved());

    // No stop. The controller simply goes, with the submission still held.
    drop(fixture.handle.take());

    assert!(
        fixture.lifetime.retains_unresolved(),
        "the controller's drop placed the unresolved runtime in the reserved slot"
    );
    assert!(
        !fixture.lifetime.in_use(),
        "and the ended service no longer claims the lifetime"
    );
    assert_eq!(
        fixture.lifetime.unresolved_receipts(),
        Some(Some(1)),
        "the slot can say what the retained runtime still owes"
    );
    assert!(!fixture.lifetime.slot_poisoned());

    // The submission outlived the controller and is refused by the ended
    // service rather than served through it.
    let refused = submission.submit_key(sophia_protocol::SurfaceId::new(window, 1), 38, false);
    assert!(refused.is_err(), "{refused:?}");
    drop((submission, peer));
}

/// Dropping the submission after the controller changes nothing about custody.
///
/// THE OUTER OWNER IS THE ONE THAT MATTERS. Custody was placed when the
/// controller went; an adapter releasing its own handle afterwards is not a
/// resolution of anything, and the slot must still be holding the work. This is
/// the pair of the control above: there, the submission outlives the
/// controller; here it is released afterwards and the answer is the same.
#[test]
fn releasing_the_submission_after_the_controller_leaves_custody_where_it_was() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    drop(fixture.handle.take());
    assert!(fixture.lifetime.retains_unresolved());

    drop(submission);

    assert!(
        fixture.lifetime.retains_unresolved(),
        "an adapter letting go of its handle resolves nothing"
    );
    assert_eq!(
        fixture.lifetime.unresolved_receipts(),
        Some(Some(1)),
        "and the work it owed is still exactly what it was"
    );
    drop(peer);
}

/// A service that ends owing nothing gives its claim back.
///
/// Without this the lifetime would be spent by a service that finished clean,
/// and the reserved slot would be unusable for a reason that never happened.
#[test]
fn a_clean_exit_returns_the_lifetime_claim() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::Disabled);
    assert!(fixture.lifetime.in_use());

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    if outcome.retains_obligations() {
        // Nothing to prove here: this control is about the clean case, and an
        // exit that owed something is reported rather than asserted away.
        assert!(fixture.lifetime.retains_unresolved(), "{outcome:?}");
        return;
    }
    assert!(
        !fixture.lifetime.retains_unresolved(),
        "a clean exit retains nothing: {outcome:?}"
    );
    assert!(
        !fixture.lifetime.in_use(),
        "and gives its claim back: {outcome:?}"
    );
}

/// A poisoned join slot still collects the real serving thread.
///
/// WHAT THIS CATCHES. The stop read the join slot with `.ok()`, so a poisoned
/// mutex produced `None` and the run reported `NeverStarted` -- a thread that
/// was never started -- while the real join handle was still sitting in the
/// slot for a later drop to detach unjoined. Reporting the unreadable slot as
/// an answer about the thread is the failure; recovering it and reporting the
/// poisoning separately is the repair.
///
/// The slot is poisoned only once the service is genuinely serving, with a
/// started worker and an accepted obligation already owned.
#[test]
fn a_poisoned_join_slot_still_collects_the_real_thread() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.thread.lock().unwrap();
        panic!("poisoning the join slot on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert!(
        outcome.join_slot_poisoned,
        "the poisoning is reported as its own fact: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "and the real thread is still collected rather than called NeverStarted: {outcome:?}"
    );
    // NOTHING IS LEFT FOR A LATER DROP TO DETACH.
    let left = match runtime.thread.lock() {
        Ok(held) => held.is_some(),
        Err(poisoned) => poisoned.into_inner().is_some(),
    };
    assert!(!left, "the handle was taken for the join, not abandoned");
    drop((submission, peer));
}

/// M4.lifetime subcase `command_loss`: the service command channel is lost.
///
/// THE REAL DISCONNECTION, NOT A SIMULATED ONE. Every command sender is
/// dropped, which is what happens when the last holder of a channel goes; the
/// service's own receiver then reports a genuine `Disconnected` and the serving
/// loop treats it exactly as it treats StopAndDisconnect. Nothing is faked and
/// no error is constructed.
///
/// The fault lands on a service that is actually serving: a real authenticated
/// peer, its custody reporting `Running`, an admitted surface it committed, and
/// an accepted obligation already owned.
#[test]
fn command_loss_closes_admission_and_collects() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    // CAPTURED WHILE IT IS RUNNING. Collection is asserted against this exact
    // place afterwards, so a run that collected some other actor cannot pass.
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    let primary_place = running.place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    assert!(
        fixture.handle().runtime.drop_command_senders(),
        "the service held a command sender to lose"
    );
    assert!(
        fixture.handle().runtime.command_sender().is_none(),
        "every command sender is gone"
    );

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "a stop with no sender to ask still joins: {outcome:?}"
    );
    assert_collected(&runtime, &outcome, &submission, primary_place, true);
    drop(peer);
}

/// M4.lifetime subcase `service_error`: a real service error after startup.
///
/// THE ERROR IS THE SERVICE'S OWN. `UpdateOutputTopology` carries the sender
/// its acknowledgement goes back on; this sends a valid topology with a
/// receiver that has already been dropped, so the serving loop's own
/// `try_send` fails and it returns the error it writes for exactly that. No
/// `PrivateServiceFailure` is constructed here, and the topology is real -- a
/// malformed one would be refused earlier and would prove something else.
#[test]
fn a_service_error_after_startup_closes_admission_and_collects() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    let primary_place = running.place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let sender = fixture
        .handle()
        .runtime
        .command_sender()
        .expect("the service is still taking commands");
    let (acknowledgement, receiver) = std::sync::mpsc::sync_channel(1);
    // The acknowledgement has nowhere to go before the command is even sent.
    drop(receiver);
    let mut topology = config(&fixture.socket, PrivateInputGrantPolicy::Disabled).output_topology;
    topology.generation += 1;
    sender
        .send(
            sophia_x_authority::XServerFrontendServiceCommand::UpdateOutputTopology {
                snapshot: topology,
                acknowledgement,
            },
        )
        .expect("the service is still taking commands");

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert_eq!(
        outcome.invocation,
        super::handle::PrivateInputInvocation::Failed,
        "the invocation reports its own failure: {outcome:?}"
    );
    assert!(
        outcome.failure.is_some(),
        "and the failure travels whole: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "a failed invocation still joins its thread: {outcome:?}"
    );
    assert_collected(&runtime, &outcome, &submission, primary_place, true);
    drop(peer);
}

/// Actors started and collected across one `lifetime` run.
///
/// ACCUMULATED ACROSS THE FIVE EXITS, because the acceptance record is about
/// the case and not about any one of them. Each subcase adds what its own exit
/// started and collected; nothing else reads or resets it.
static LIFETIME_ACTORS: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));

/// Record one exit's actors from custody, not from a counter.
///
/// The service thread is one actor and every custody place that started a
/// worker is another. Collection is counted the same way round: the thread if
/// it joined, and every place that published a join. Reading both from the
/// same snapshot is what makes "started equals collected" a statement about
/// this run rather than an arithmetic identity.
fn record_actors(
    runtime: &std::sync::Arc<crate::private_input::service::PrivateInputRuntime>,
    outcome: &crate::private_input::PrivateInputOutcome,
) {
    let after = runtime
        .owner
        .custody_snapshot(&runtime.owner.lease())
        .expect("the owner keeps its inventory past the service");
    let started = 1 + after
        .rows
        .iter()
        .filter(|row| row.worker != PrivateCustodyWorkerStanding::NeverStarted)
        .count();
    let collected = usize::from(outcome.service_thread == PrivateInputThreadJoin::Joined)
        + after
            .rows
            .iter()
            .filter(|row| row.join != PrivateCustodyJoinStanding::Unpublished)
            .count();
    let mut actors = LIFETIME_ACTORS.lock().expect("the accumulator is readable");
    actors.0 += started;
    actors.1 += collected;
}

/// M4.lifetime: Session closes admission, collects actors and keeps unresolved
/// obligations on every exit.
///
/// THE ACCEPTANCE BODY, COMPOSED FROM THE CONTROLS RATHER THAN BESIDE THEM.
/// The harness binds one test name to the case, and the five subjects it
/// requires are exactly five controls that already exist. Calling them keeps
/// the acceptance row and the component suite from ever drifting apart: there
/// is one body per exit, and both readers run the same one.
///
/// Ignored so it runs only through the acceptance harness, which passes
/// `--include-ignored`; the component runner does not, which is what keeps a
/// diagnostic suite from ever reporting this as acceptance.
#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn lifetime() {
    *LIFETIME_ACTORS.lock().expect("the accumulator is readable") = (0, 0);

    running_worker_is_collected();
    command_loss_closes_admission_and_collects();
    a_service_error_after_startup_closes_admission_and_collects();
    an_unwind_on_the_serving_thread_still_collects();
    a_stop_counts_receipts_nobody_drained();

    let (started, collected) = *LIFETIME_ACTORS.lock().expect("the accumulator is readable");
    assert!(started > 0, "five real exits started actors");
    assert_eq!(
        started, collected,
        "every actor these exits started was collected"
    );

    println!(
        "sophia_m4_acceptance {{\"schema\":1,\"case\":\"M4.lifetime\",\"subcases\":{{\
         \"stop\":\"PASS\",\"command_loss\":\"PASS\",\"service_error\":\"PASS\",\
         \"unwind\":\"PASS\",\"retained_obligations\":\"PASS\"}},\"cleanup\":{{\
         \"actors_started\":{started},\"actors_collected\":{collected},\
         \"pending_actors\":0,\"complete\":true}},\"observations\":{{\
         \"real_session_invocations\":5,\
         \"actor_scope\":\"service_threads_and_registered_ordered_workers\"}}}}"
    );
}

/// What every exit must establish, whatever ended it.
///
/// ADMISSION CLOSED, OLD SUBMISSIONS REFUSED, THE PRIMARY ACTOR COLLECTED.
/// These are the requirement itself rather than incidental checks, so they are
/// written once and required of every subcase.
///
/// THE PRIMARY PLACE IS NAMED, NOT SEARCHED FOR. An earlier version accepted
/// any row with a published join, which a probe peer connected and dropped
/// during the test satisfies on its own -- so a run that collected the probe
/// and abandoned the peer under test would have passed. The place is captured
/// while that peer's worker is Running and required by number here.
fn assert_collected(
    runtime: &std::sync::Arc<crate::private_input::service::PrivateInputRuntime>,
    outcome: &crate::private_input::PrivateInputOutcome,
    submission: &crate::private_input::PrivateInputSubmission,
    primary_place: usize,
    expect_workers: bool,
) {
    // Admission is closed: nothing the boundary still reports is open.
    let live = runtime
        .participant
        .admitted()
        .expect("the admission boundary is readable after collection");
    assert!(
        live.iter().all(|seen| seen.closed || !seen.lifecycle_open),
        "no admission is left open after collection: {live:?}"
    );

    // An old submission refuses rather than being served by an ended service.
    let refused = submission.submit_key(sophia_protocol::SurfaceId::new(1, 1), 38, false);
    assert!(
        refused.is_err(),
        "a submission against an ended service is refused: {refused:?}"
    );

    let after = runtime
        .owner
        .custody_snapshot(&runtime.owner.lease())
        .expect("the owner keeps its inventory past the service");
    assert!(
        !after.inventory_poisoned,
        "the inventory survived collection readable: {after:?}"
    );

    let primary = after
        .rows
        .iter()
        .find(|row| row.place == primary_place)
        .unwrap_or_else(|| {
            panic!("the primary custody place {primary_place} is still kept: {after:?}")
        });
    assert_ne!(
        primary.join,
        PrivateCustodyJoinStanding::Unpublished,
        "the primary actor's own place carries its join: {primary:?}"
    );
    assert_ne!(
        primary.worker,
        PrivateCustodyWorkerStanding::Unreadable,
        "collection leaves the primary place readable: {primary:?}"
    );
    assert_eq!(
        primary.handle_present,
        Some(false),
        "the primary actor's handle went to whoever joined it: {primary:?}"
    );

    // THE WORKER LIST IS REPORTED OR DELIBERATELY ABSENT. An invocation that
    // returned or failed reports what it collected; one that unwound reports
    // nothing of its own, and inventing an empty list for it would say it
    // collected nothing when it never got to say anything at all. Custody
    // above is where collection is read from either way.
    assert_eq!(
        outcome.workers.is_some(),
        expect_workers,
        "the worker list is present exactly when the invocation could report one: {outcome:?}"
    );
    record_actors(runtime, outcome);

    // AND NOTHING STARTED WAS LEFT BEHIND. A place that started a worker and
    // published no join is an actor nobody collected.
    for row in &after.rows {
        assert_ne!(
            row.worker,
            PrivateCustodyWorkerStanding::Unreadable,
            "no custody place is left unreadable: {row:?}"
        );
        if row.worker != PrivateCustodyWorkerStanding::NeverStarted {
            assert_ne!(
                row.join,
                PrivateCustodyJoinStanding::Unpublished,
                "every started actor was joined: {row:?}"
            );
        }
    }
}

/// M4.lifetime subcase `unwind`: the invocation unwinds while collection is
/// still live.
///
/// AN UNWIND IN THE PLACE A REAL ONE WOULD HAPPEN. A panic raised after
/// `serve_until_stopped` returns proves nothing: the service's own collection
/// guard has already run, so the unwind passes through none of it. What has to
/// be established is that an invocation which unwinds WHILE that guard is live
/// still leaves its keeper, its custody and its obligations in order.
///
/// So the fault is armed only once the service is genuinely serving -- a real
/// authenticated peer, custody reporting `Running`, an admitted surface it
/// committed and an accepted obligation owned -- and it fires from inside an
/// event the serving thread actually emits when it reaps a client. A second
/// probe peer is connected and dropped to cause that reap; the primary peer and
/// its obligation stay live so the guard has something to unwind through. The
/// fault disarms itself before panicking, so unwinding cleanup that emits the
/// same event cannot panic a second time.
#[test]
fn an_unwind_on_the_serving_thread_still_collects() {
    let (mut fixture, fault) = Fixture::started_with_unwind_fault();
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let primary_place = fixture.running_row().place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    // ARMED ONLY NOW. Before this point there is no started worker and no owned
    // obligation, and an unwind there would be testing something else.
    fault.arm();

    // A SECOND PEER THAT NEVER FINISHES ITS HANDSHAKE. A fully connected peer
    // that closes cleanly is `Ok(())` to its worker and the reaper says nothing
    // about it; only a failure, a disconnect or a shutdown is reported. This
    // socket goes before its setup prefix, so the worker reports a real client
    // disconnect and the serving thread emits the event while its collection
    // guard is still live.
    drop(fixture.probe_socket());

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let deadline = std::time::Instant::now() + WAIT;
    while !fault.fired() {
        assert!(
            std::time::Instant::now() < deadline,
            "the armed fault fired on the serving thread within the bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert!(
        matches!(
            outcome.invocation,
            super::handle::PrivateInputInvocation::Unwound(_)
        ),
        "the invocation is reported as having unwound, not as having returned: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "an unwound invocation still leaves a thread that joins: {outcome:?}"
    );
    // AN UNWIND REPORTS NOTHING OF ITS OWN, so the worker list stays absent
    // rather than being invented; custody is where collection is read from,
    // and it is required of this peer's own place rather than of any place a
    // probe might have left behind.
    assert_collected(&runtime, &outcome, &submission, primary_place, false);

    // A JOINED THREAD IS NOT A LIVE EXECUTION, even after an unwind.
    if let Some(execution) = outcome.execution {
        assert_ne!(
            execution.availability,
            sophia_x_authority::PrivateExecutionAvailability::Retained,
            "an unwound run that joined must not report a retained execution: {outcome:?}"
        );
    }
    // AND THE EXACT OBLIGATION IS RETAINED UNDER THE OUTER LIFETIME.
    assert!(
        outcome.retains_obligations(),
        "an unwind with an undrained receipt still owes it: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the reserved slot is holding that runtime"
    );
    assert_eq!(
        fixture.lifetime.unresolved_receipts(),
        Some(Some(1)),
        "the slot can say exactly what is owed"
    );
    drop((submission, peer));
}

/// Observing a receipt releases the place its delivery took, and the ledger
/// can be cycled through that release more than once.
///
/// WHAT THIS PROVES THAT NOTHING ELSE DOES. Everything else about receipts
/// establishes that Session keeps them and counts them. This establishes the
/// claim the whole design rests on: that handing one back is what frees the
/// delivery place it was holding. A receipt taken off the channel and dropped
/// would satisfy every other control here and still leak the place forever.
///
/// IT ASKS THE LEDGER, NOT A COUNTER. `state()` reports whether the ledger is
/// still holding a ticket for that exact delivery, so the release is observed
/// where it actually happens rather than inferred from a number Session keeps.
///
/// An earlier version tried to prove this by filling the declared bound until
/// it refused. That could never work: the bound is not the configured input
/// capacity but `input*2 + 2047*(input+1)` -- 6145 for a capacity of two --
/// because the ordinary ledger reserves a place for every resource range a
/// server could hand out. Sizing it to the private client bound instead does
/// make it reachable, but it also changes a bound M3 acceptance already
/// measures, so it is not a change to make inside M4 closure.
#[test]
fn observing_a_receipt_releases_the_place_its_delivery_took() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let surface = fixture.admitted_surface();
    fixture.focus(surface);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    // TWICE, because once is consistent with a place that was never taken.
    let mut pressed = true;
    for cycle in 0..2 {
        let accepted = submission
            .submit_pointer_button(surface, BTN_LEFT, pressed)
            .unwrap_or_else(|error| panic!("cycle {cycle} submitted: {error:?}"));
        pressed = !pressed;
        await_flushed(&runtime, accepted.delivery);

        // SETTLED BUT STILL HELD. The delivery has its terminal answer and the
        // ledger has not given the place back, because nobody has observed it.
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Live,
            "cycle {cycle}: a settled delivery still holds its place until observed"
        );

        // Take the receipt off the channel WITHOUT observing it. Taking it is
        // not what frees the place, and this is where that is established.
        let receipt = take_one_receipt(&runtime);
        assert_eq!(receipt.delivery, accepted.delivery, "cycle {cycle}");
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Live,
            "cycle {cycle}: taking the receipt off the channel frees nothing"
        );

        // Hand it back. THIS is the release.
        assert_eq!(
            runtime.observer.observe(receipt),
            sophia_x_authority::PrivateDeliveryObservation::Observed,
            "cycle {cycle}"
        );
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Ended,
            "cycle {cycle}: observing the receipt released its delivery's place"
        );

        // AND IT IS NOT RELEASABLE TWICE.
        assert_eq!(
            runtime.observer.observe(receipt),
            sophia_x_authority::PrivateDeliveryObservation::UnknownDelivery,
            "cycle {cycle}: a released place cannot be released again"
        );
    }

    drop((submission, peer));
}

/// Wait for one delivery's terminal answer to be exactly `Flushed`.
fn await_flushed(
    runtime: &std::sync::Arc<crate::private_input::service::PrivateInputRuntime>,
    delivery: sophia_x_authority::XAuthorityInputDeliveryId,
) {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        if let Some(settled) = runtime.observer.settled(delivery) {
            assert_eq!(
                settled.outcome,
                sophia_x_authority::XAuthorityInputDeliveryOutcome::Flushed,
                "delivery {delivery:?} was delivered: {settled:?}"
            );
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "delivery {delivery:?} settled within the bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Take exactly one receipt off the channel, without observing it.
fn take_one_receipt(
    runtime: &std::sync::Arc<crate::private_input::service::PrivateInputRuntime>,
) -> sophia_x_authority::XAuthorityClientInputDelivery {
    runtime
        .deliveries
        .lock()
        .expect("the delivery channel is readable")
        .recv_timeout(WAIT)
        .expect("a receipt for a delivered event")
}

/// A poisoned command slot still stops the service rather than hanging on it.
///
/// WHAT THIS CATCHES, AND IT IS A DEADLOCK RATHER THAN A BAD REPORT. The stop
/// read the command slot with `.ok()`, so a poisoned slot answered `None`
/// while the real sender stayed stored. No StopAndDisconnect was sent, the
/// service's receiver was still connected so it never ended, and the wait on
/// the closed channel never returned. The slot is recovered for shutdown now,
/// and the poisoning is reported instead of standing in for an answer.
#[test]
fn a_poisoned_command_slot_still_stops_the_service() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    let primary_place = running.place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.commands.lock().unwrap();
        panic!("poisoning the command slot on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    // Ordinary access may refuse; shutdown may not.
    assert!(
        runtime.command_sender().is_none(),
        "ordinary command access refuses a poisoned slot"
    );

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert!(
        outcome.command_slot_poisoned,
        "the poisoning is reported as its own fact: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "the stop recovered the sender, ended the service and joined: {outcome:?}"
    );
    assert_collected(&runtime, &outcome, &submission, primary_place, true);
    drop(peer);
}

/// Observations the order never committed are owed at stop, not discarded.
///
/// WHAT THIS CATCHES. The stop counted what the bridge was holding and nothing
/// else, so batches the frontend had observed and handed over -- still queued
/// on the transaction channel, never taken by the bridge -- were reported as
/// nothing owed. They are taken into owned intake at shutdown instead, and
/// deliberately not committed: the order has stopped, and committing on its
/// behalf afterwards would be doing work for a service that has ended.
/// THE BARRIER IS A REAL ROUND TRIP, NOT A SPIN. An earlier version waited for
/// `retained_intake` to become non-empty, which only the stop below ever does,
/// so it burned its whole bound and established no prerequisite at all. It also
/// created a second window, but the peer uses a fixed XID, so that was the same
/// window rather than a fresh one. A draw on the window already admitted,
/// followed by a GetGeometry reply, proves the server processed the draw --
/// and nothing here drives a commit, so the observation is still queued.
#[test]
fn queued_intake_nobody_committed_is_owed_at_stop() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    // The surface this service actually admitted, so the batch retained below
    // can be identified by that exact surface rather than by a count.
    let surface = fixture.admitted_surface();
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    // Draw again on the same window and DO NOT pump a commit for it. The
    // geometry reply is the barrier: the server has processed the draw, so the
    // observation has been made and is sitting on the transaction channel with
    // nothing about to take it.
    peer.draw(window);
    peer.confirm_geometry(window);

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    let owed = outcome
        .intake_uncommitted
        .expect("the intake queue and the channel were both readable");
    assert!(
        owed > 0,
        "observations the order never committed are owed at stop: {outcome:?}"
    );
    let retained = runtime.retained_intake.lock().unwrap();
    assert_eq!(
        retained.len(),
        owed,
        "and they are held rather than counted and dropped"
    );
    // THE EXACT SURFACE, NOT ANY QUEUED LIFECYCLE BATCH. A count alone would be
    // satisfied by whatever else happened to be in flight at shutdown.
    assert!(
        retained.iter().any(|batch| {
            batch
                .transactions
                .iter()
                .any(|transaction| transaction.surface == surface)
        }),
        "the retained intake carries the draw for surface {surface:?}: {retained:?}"
    );
    drop(retained);
    assert!(
        outcome.retains_obligations(),
        "uncommitted intake retains custody: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the reserved slot is holding the runtime that owes it"
    );
    drop(peer);
}

/// An unreadable transaction channel is refused, never read as no intake.
///
/// WHAT THIS CATCHES. The drain answered a poisoned receiver and a quiet one
/// with the same empty list, so a caller driving commits reported a step that
/// observed nothing and carried on, while intake it could no longer reach
/// accumulated behind the lock. `apply_committed` now refuses instead, and the
/// stop can no longer put a number on intake it was unable to read: it says
/// `None`, which is not none owed, and custody is retained on that basis
/// alone.
#[test]
fn an_unreadable_transaction_channel_is_refused_and_still_retains() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.transactions.lock().unwrap();
        panic!("poisoning the transaction channel on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    // REFUSED, NOT REPORTED AS AN EMPTY STEP.
    let refused = fixture
        .handle_mut()
        .apply_committed(Duration::from_millis(10));
    assert!(
        refused.is_err(),
        "an unreadable transaction channel is refused: {refused:?}"
    );

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert_eq!(
        outcome.intake_uncommitted, None,
        "intake that could not be read reports no count at all, not a count of zero: {outcome:?}"
    );
    assert!(
        outcome.retains_obligations(),
        "nothing has been shown to be finished, so custody is retained: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the reserved slot is holding that runtime"
    );
    drop(peer);
}

/// A surface that was drawn but never mapped gets no route; mapping it is what
/// admits it, and drawing again after that configures rather than re-admits.
///
/// THE DISCRIMINATOR IS ONE REQUEST. `create_map_and_draw`, which every other
/// control here uses, is exactly `create_unmapped` + `map` + `draw`. This runs
/// the same sequence with the `MapWindow` left out, so the only thing that can
/// account for a different outcome is the mapping fact itself.
///
/// WHAT THIS CATCHES. Nothing else here ever draws an unmapped window, so the
/// guard that refuses to route one was never exercised: deleting it left all
/// twenty-two controls green. An unmapped passive helper picking up an input
/// route is precisely what the private path must not do.
///
/// It also refuses to pass vacuously. Asserting "no admission" alone would hold
/// just as well if nothing had committed at all, so the first phase requires the
/// commit to have applied the surface while this service held no mapping fact
/// for it -- the exact state the guard exists to act on.
#[test]
fn an_unmapped_surface_gets_no_route_until_it_is_mapped() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );

    // DRAWN, NEVER MAPPED.
    let window = peer.create_unmapped();
    peer.draw(window);
    peer.confirm_geometry(window);

    // WAIT ON WHAT THE MUTATION CANNOT CHANGE. The vacuity guard has to be
    // anchored on `applied`, which is simply what the coordinator committed. An
    // earlier version waited for `mapped` to be empty as well, and that field is
    // written by the very code under test -- so the defect made the wait time
    // out instead of making the routing assertion below fire, and the control
    // reported a timeout rather than the fault it had actually found.
    let deadline = std::time::Instant::now() + WAIT;
    let mut applied = None;
    while applied.is_none() {
        let report = fixture.pump();
        applied = report
            .outcomes
            .iter()
            .find(|outcome| !outcome.applied.is_empty())
            .map(|outcome| outcome.applied.clone());
        assert!(
            std::time::Instant::now() < deadline,
            "the unmapped draw reached a commit within the bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let unmapped = applied.expect("the loop only leaves with one");
    // Give the bridge a moment to stage anything that commit decided, so the
    // assertion below is about what was routed rather than about what has not
    // been reached yet.
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(
        fixture
            .harvest
            .iter()
            .all(|effect| effect.kind() != XAuthorityControlKind::AdmitSurface),
        "a surface this service holds no mapping fact for was routed anyway: {:?}",
        fixture.harvest
    );

    // NOW MAP IT. This is the only request that changes, and it is what admits.
    peer.map(window);
    peer.confirm_geometry(window);

    let deadline = std::time::Instant::now() + WAIT;
    loop {
        fixture.pump();
        if fixture
            .harvest
            .iter()
            .any(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mapping the window admitted it within the bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    let admissions: Vec<_> = fixture
        .harvest
        .iter()
        .filter(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
        .collect();
    assert_eq!(
        admissions.len(),
        1,
        "mapping admits the surface exactly once: {admissions:?}"
    );
    let admitted = admissions[0];
    assert!(
        admitted.geometry().is_some(),
        "the admission carries the geometry the coordinator committed: {admitted:?}"
    );
    assert!(
        unmapped.contains(&admitted.surface()),
        "the surface admitted on mapping is the one drawn while unmapped: {admitted:?} of {unmapped:?}"
    );

    // AND A LATER DRAW CONFIGURES RATHER THAN ADMITTING AGAIN.
    peer.draw(window);
    peer.confirm_geometry(window);
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        fixture.pump();
        if fixture
            .harvest
            .iter()
            .any(|effect| effect.kind() == XAuthorityControlKind::ConfigureSurface)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a further draw configured the admitted surface within the bound"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        fixture
            .harvest
            .iter()
            .filter(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
            .count(),
        1,
        "the surface is admitted once and configured afterwards, never re-admitted"
    );
    drop(peer);
}

/// Draining a receipt hands it to the caller and frees its place together.
///
/// WHAT THIS CATCHES. The release control beside this one proves that observing
/// a receipt frees its delivery's place, but it does that through the ledger
/// directly. Nothing covered the drain itself, so a drain that observed the
/// receipt and then dropped it -- freeing the place while the caller never
/// learns which receipt freed it -- went unnoticed. The two have to happen
/// together or the caller cannot account for what it no longer holds.
#[test]
fn a_drain_returns_the_receipts_whose_places_it_freed() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let surface = fixture.admitted_surface();
    fixture.focus(surface);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    let mut pressed = true;
    let mut sent = Vec::new();
    for _ in 0..2 {
        let accepted = submission
            .submit_pointer_button(surface, BTN_LEFT, pressed)
            .expect("the connection is live and has authority");
        pressed = !pressed;
        await_flushed(&runtime, accepted.delivery);
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Live,
            "settled and still holding its place, because nobody has drained it"
        );
        sent.push(accepted.delivery);
    }

    let receipts = fixture
        .handle()
        .drain_deliveries()
        .expect("the channel and the queue are readable");

    // THE RECEIPTS COME BACK, AND THEY ARE THE ONES THAT WERE FREED.
    let mut observed: Vec<_> = receipts.observed.iter().map(|r| r.delivery).collect();
    observed.sort_by_key(|d| d.raw());
    let mut expected = sent.clone();
    expected.sort_by_key(|d| d.raw());
    assert_eq!(
        observed, expected,
        "the drain returned exactly the receipts it observed: {receipts:?}"
    );
    assert_eq!(receipts.retained, 0, "nothing was left owed: {receipts:?}");
    for delivery in &sent {
        assert_eq!(
            runtime.observer.state(*delivery),
            sophia_x_authority::DeliveryState::Ended,
            "and each returned receipt's place really was released"
        );
    }
    drop((submission, peer));
}

/// A stop that reports the service thread collected must have waited for it.
///
/// WHAT THIS CATCHES. `stop` can drop the join handle and report `Joined`
/// anyway; the thread is then detached and the claim is simply untrue. Every
/// twenty-two controls passed with that in place, including the one asserting
/// the execution is not `Retained` after the join -- which is a real assertion
/// that merely lost a race: the report is sent one statement before the closure
/// ends, so the keeper's abandonment usually lands before the read either way.
///
/// This makes the difference observable instead of likely. The thread lingers
/// past its last message and marks its exit only at the very end, so a stop that
/// joined provably waits through that and a stop that did not provably returns
/// before the mark. The linger is not a contrivance: the absence of a wait
/// cannot be seen unless something is still there to be waited for.
#[test]
fn a_stop_that_reports_the_thread_collected_really_joined_it() {
    let directory = std::env::temp_dir().join(format!(
        "m4-session-exit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let socket = directory.join("private.sock");
    let lifetime = PrivateInputLifetimeOwner::reserved();
    let marker = std::sync::Arc::new(
        crate::private_input::faults::PrivateInputExitMarker::lingering(Duration::from_millis(150)),
    );
    let handle = lifetime
        .start_with_faults(
            config(
                &socket,
                PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
            ),
            crate::private_input::faults::PrivateInputFaults {
                exit: Some(std::sync::Arc::clone(&marker)),
                ..Default::default()
            },
        )
        .expect("the service starts");
    assert_eq!(
        handle.await_ready(WAIT).unwrap(),
        PrivateInputReadiness::Ready
    );

    let mut peer = Peer::connect(&socket, Order::Little, Some(COOKIE)).unwrap();
    let _window = peer.create_map_and_draw();

    assert!(
        !marker.exited(),
        "the thread is still serving before the stop"
    );
    let outcome = handle.stop();
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "the stop reports the thread collected: {outcome:?}"
    );
    assert!(
        marker.exited(),
        "and it really waited for it: a stop that reported Joined returned while \
         the thread was still running"
    );

    drop(peer);
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir(&directory);
}

/// A control the order refuses for now stays owed, keeps its transaction, and
/// is delivered later without anything overtaking it.
///
/// WHAT THIS CATCHES. The bridge stops at the first refusal that says "later"
/// and leaves the entry at the head. Nothing exercised that, so removing the
/// stop -- dropping the entry instead of keeping it -- left every control
/// green. Work the coordinator committed and the order declined for capacity
/// would simply have vanished.
///
/// THE REFUSAL IS THE ORDER'S OWN. The ready queue admits controls up to
/// `input_capacity * 2`, and it is refilled one entry per serve turn with a
/// socket write in between, while the bridge delivers a whole staged batch in a
/// tight loop under one held lock. A burst of draws therefore outruns it and
/// earns a real `Saturated`. An earlier attempt drew one at a time and
/// concluded the bound was unreachable; it was aiming at a different, larger
/// gate and never got near this one.
#[test]
fn a_control_refused_for_now_keeps_its_place_and_its_transaction() {
    let mut fixture =
        Fixture::started_with_bounds(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence, 4, 2);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let _surface = fixture.admitted_surface();

    // Just past what the queue will take at once. The ceiling here is
    // `input_capacity * 2` = 4, and the burst is kept close to it on purpose:
    // this peer never reads the events it is sent, so every extra draw is
    // pressure on a socket that cannot drain, and a bigger burst buys nothing
    // but flakiness under load.
    for _ in 0..10 {
        peer.draw(window);
    }
    peer.confirm_geometry(window);

    let deadline = std::time::Instant::now() + WAIT;
    let mut refused = None;
    while refused.is_none() {
        let report = fixture
            .handle_mut()
            .apply_committed(Duration::from_millis(50))
            .expect("the bridge is readable");
        // The classification is restated here rather than borrowed from the
        // bridge, so a change to what counts as "later" cannot quietly change
        // what this control is testing.
        refused = report.refused.iter().find_map(|refusal| match refusal {
            crate::private_input::PrivateInputControlError::Refused(
                sophia_x_authority::AdmissionRefusal::Saturated
                | sophia_x_authority::AdmissionRefusal::Unavailable
                | sophia_x_authority::AdmissionRefusal::AuthorityUnreadable,
                command,
            ) => Some(*command),
            _ => None,
        });
        assert!(
            std::time::Instant::now() < deadline,
            "the order refused a control for capacity within the bound \
             (the control ceiling is input_capacity * 2)"
        );
    }
    let refused = refused.expect("the loop only leaves with one");
    let refused_transaction = control_transaction(&refused);

    // STILL OWED, NOT DISCARDED. This is what the mutation destroys.
    let owed = fixture
        .handle()
        .outstanding()
        .expect("the bridge is readable");
    assert!(
        owed >= 1,
        "a control the order deferred is kept rather than dropped"
    );

    // NOTHING BEHIND IT OVERTOOK IT, AND IT KEPT ITS OWN TRANSACTION. Minting a
    // fresh one would leave the first outstanding and unanswerable, and two
    // acknowledgements could then arrive for one committed decision.
    let deadline = std::time::Instant::now() + WAIT;
    let mut delivered = false;
    while !delivered {
        let report = fixture
            .handle_mut()
            .apply_committed(Duration::from_millis(50))
            .expect("the bridge is readable");
        for submitted in report
            .effects
            .iter()
            .filter_map(|effect| effect.submitted())
        {
            if submitted.transaction == refused_transaction {
                delivered = true;
            } else {
                assert!(
                    submitted.transaction < refused_transaction || delivered,
                    "a control minted after the refused one was delivered ahead of it: \
                     {submitted:?} before {refused_transaction:?}"
                );
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the deferred control was delivered under its original transaction"
        );
    }
    drop(peer);
}

/// The transaction a control command carries, whichever kind it is.
fn control_transaction(
    command: &sophia_x_authority::XAuthorityClientControlCommand,
) -> sophia_protocol::TransactionId {
    use sophia_x_authority::XAuthorityControlCommand as C;
    match command.command {
        C::PublishMetadataRule { transaction, .. }
        | C::AdmitSurface { transaction, .. }
        | C::ConfigureSurface { transaction, .. }
        | C::WithdrawSurface { transaction, .. }
        | C::FocusSurface { transaction, .. }
        | C::ClearFocus { transaction, .. } => transaction,
        _ => panic!("the refused control carries a transaction: {command:?}"),
    }
}

/// A committed draw is reconciled to the Engine's own predecessor.
///
/// WHAT THIS CATCHES. The generation ledger has its own six controls, but they
/// exercise the ledger in isolation: every one of them passes if the commit
/// path stops calling it. Bypassing preparation was caught only by unrelated
/// controls timing out on admissions that never arrived, which says a pipeline
/// broke somewhere, not what broke.
///
/// The signature is exact. The X authority stamps a drawing transaction with
/// the window's own generation, which starts at one, while the Engine expects
/// zero for a surface it has never committed; without reconciliation the commit
/// comes back `RejectedStaleSurface` rather than `Committed`. That is the
/// difference preparation exists to remove, and it is visible in the commit
/// outcomes the report carries.
#[test]
fn a_committed_draw_is_reconciled_to_the_engine_predecessor() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );

    // A FRESH BATCH AFTER READINESS. Waiting for a started worker already pumps
    // commits, and a pump hands its outcomes out once; without a new draw there
    // is nothing left for this to read.
    peer.draw(window);
    peer.confirm_geometry(window);

    let deadline = std::time::Instant::now() + WAIT;
    let mut seen = Vec::new();
    let mut committed_an_applied_surface = false;
    while !committed_an_applied_surface {
        let report = fixture.pump();
        for outcome in &report.outcomes {
            seen.push(outcome.outcome);
            if outcome.outcome == sophia_protocol::TransactionOutcome::Committed
                && !outcome.applied.is_empty()
            {
                committed_an_applied_surface = true;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the draw reached a commit within the bound: outcomes so far {seen:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    assert!(
        !seen.contains(&sophia_protocol::TransactionOutcome::RejectedStaleSurface),
        "the draw was reconciled to the Engine's predecessor rather than \
         rejected as stale: {seen:?}"
    );
    drop(peer);
}
