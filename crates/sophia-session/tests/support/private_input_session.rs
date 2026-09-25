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

/// The bound on readiness and on the wire. `wire.rs` reads this from its parent.
///
/// NOT THE BOUND ON A WAIT FOR THE BRIDGE. Those are `STALL`, `CEILING` and
/// `UNSIGNALLED` below, and they are measured in opportunities rather than in
/// seconds. This one bounds a socket read, a socket write and the readiness
/// of a service that has just been started, which are single blocking calls
/// with nothing to observe in between.
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

    /// Drive one coordinator step, keeping whatever it decided.
    ///
    /// The effects go into the harvest so a readiness poll cannot eat them; the
    /// report comes back so a caller that needs the commit outcomes themselves
    /// -- what was applied, and whether this service held a mapping fact for it
    /// -- can read them without draining the harvest to find out.
    fn pump(&mut self) -> Pumped {
        let mut committed = self
            .handle_mut()
            .apply_committed(Duration::from_millis(5))
            .expect("the bridge is readable");
        // THE VERDICT IS TAKEN BEFORE THE HARVEST. The effects leave for the
        // harvest on the next line, and whether any of them was submitted is
        // the one signal that says the order took something. A caller reading
        // the returned report could not tell: its `effects` is always empty.
        let advanced = advanced(&committed);
        self.harvest.append(&mut committed.effects);
        Pumped {
            advanced,
            report: committed,
        }
    }

    /// One pump, judged for a wait: `found` reads the step's report and the
    /// harvest it has just fed, and a service that has ended is `Lost` at once.
    fn step<T>(
        &mut self,
        found: impl FnOnce(&mut Self, &crate::private_input::PrivateInputCommitted) -> Option<T>,
    ) -> Progress<T> {
        let pumped = self.pump();
        if ended(&pumped.report) {
            return Progress::Lost(self.ended_report());
        }
        match found(self, &pumped.report) {
            Some(value) => Progress::Done(value),
            None if pumped.advanced => Progress::Worked,
            None => Progress::Idle,
        }
    }

    /// Stop a service that reported itself gone, and say what it said.
    ///
    /// THE SERVICE'S OWN ACCOUNT, NOT THE WAIT'S. What a wait can see of a dead
    /// service is `Ended` on every call; what is worth reading is how it ended,
    /// which only the stop reports. The handle is taken, so nothing pumps a
    /// stopped service afterwards.
    fn ended_report(&mut self) -> String {
        let status = self.handle().status();
        let outcome = self.handle.take().expect("a service to stop").stop();
        format!(
            "the private input service ended while this waited; status {status:?}; \
             invocation {:?}; failure {:?}; execution {:?}; at close {:?}; thread {:?}",
            outcome.invocation,
            outcome.failure,
            outcome.execution,
            outcome.execution_at_close,
            outcome.service_thread
        )
    }

    /// Wait until a custody place reports a started worker.
    ///
    /// REQUIRED BEFORE ANY FAULT. Registered attachment is production and the
    /// acceptance rows collect registered workers, so `NeverStarted` is not
    /// lifetime evidence -- it is the state before the thing being tested has
    /// happened. Damaging a service that has not yet started a worker would
    /// prove nothing about custody at all.
    fn running_row(&mut self) -> sophia_x_authority::PrivateCustodySnapshotRow {
        wait_for(
            self,
            "a custody place reporting a started worker",
            |fixture| format!("{:?}", fixture.custody()),
            |fixture| {
                fixture.step(|fixture, _| {
                    fixture
                        .custody()
                        .rows
                        .iter()
                        .find(|row| row.worker == PrivateCustodyWorkerStanding::Running)
                        .copied()
                })
            },
        )
    }

    /// Drive commits until the real admission effect exists, and return the
    /// surface the coordinator actually committed.
    ///
    /// THE SURFACE COMES FROM THE COMMIT, NOT FROM THE WIRE. An earlier version
    /// built `SurfaceId::new(window, 1)` out of the peer's XID, which is not an
    /// admitted target and was never a surface this service had routed
    /// anything to; input submitted against it proved nothing about delivery.
    fn admitted_surface(&mut self) -> sophia_protocol::SurfaceId {
        let effect = wait_for(
            self,
            "a committed admission for the create/map/draw",
            |fixture| format!("the harvest holds {:?}", fixture.harvest),
            // THE WHOLE HARVEST, not just this step. The admission may have
            // been committed while something else was driving the service.
            |fixture| {
                fixture.step(|fixture, _| {
                    fixture
                        .harvest
                        .iter()
                        .find(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
                        .copied()
                })
            },
        );
        let submitted = effect
            .submitted()
            .expect("the order took the admission it committed");
        self.await_ack(submitted);
        effect.surface()
    }

    /// Wait for the exact acknowledgement of one submitted control.
    ///
    /// MATCHED WHOLE. Transaction, surface and kind all have to agree, and the
    /// outcome has to be `Delivered`; any acknowledgement arriving while this
    /// waits is not evidence about this one.
    fn await_ack(&self, submitted: crate::private_input::PrivateInputSubmitted) {
        // ANY ACKNOWLEDGEMENT IS PROGRESS, including one for another
        // transaction: the service ran and answered something.
        wait_for(
            &mut (),
            &format!("the acknowledgement for {submitted:?}"),
            |_| format!("the service is {:?}", self.handle().readiness()),
            |_| {
                let mut drained = false;
                for ack in self
                    .handle()
                    .drain_acknowledgements_within(Duration::from_millis(10))
                {
                    drained = true;
                    let seen = ack.acknowledgement;
                    if seen.transaction == submitted.transaction
                        && seen.surface == submitted.surface
                        && seen.kind == submitted.kind
                    {
                        assert_eq!(
                            seen.outcome,
                            sophia_x_authority::XAuthorityControlOutcome::Delivered,
                            "the control this waited for was acknowledged but not delivered: {seen:?}"
                        );
                        return Progress::Done(());
                    }
                }
                if drained {
                    Progress::Worked
                } else {
                    not_yet(self.handle())
                }
            },
        );
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
        let settled = spin_for(
            self,
            &format!("a terminal answer for delivery {:?}", accepted.delivery),
            |fixture| {
                format!(
                    "the ledger still holds it as {:?}",
                    fixture.handle().runtime.observer.state(accepted.delivery)
                )
            },
            |fixture| match fixture.handle().runtime.observer.settled(accepted.delivery) {
                Some(settled) => Progress::Done(settled),
                None => not_yet(fixture.handle()),
            },
        );
        assert_eq!(
            settled.outcome,
            sophia_x_authority::XAuthorityInputDeliveryOutcome::Flushed,
            "the obligation this waited on settled without being delivered: {settled:?}"
        );
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

include!("private_input_session/worker_lifetime.rs");

include!("private_input_session/admission_refusal.rs");

include!("private_input_session/receipt_custody.rs");

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

include!("private_input_session/collection_and_backpressure.rs");

/// Wait for `step` to answer `Done`, bounded by a stall and by a ceiling.
///
/// THE BUDGET IS OPPORTUNITIES, NOT SECONDS. Every attempt contains its own
/// wait, so a busy host stretches the budget by itself: a pump spends its
/// whole `within` in a blocking receive, and a sleeping attempt returns when
/// this thread is next scheduled. A wall clock instead measures the machine,
/// which is how a control that was merely starved came to report a defect.
///
/// `goal` NAMES THE THING WAITED FOR AND IS NOT A SENTENCE. A noun phrase has
/// no truth value, so it cannot read as a true statement in a panic, which is
/// what fourteen of these messages used to do. `seen` is called only on
/// failure, so a site keeps its diagnostic without paying for it while
/// passing.
#[track_caller]
fn wait_for<S, T>(
    subject: &mut S,
    goal: &str,
    seen: impl Fn(&S) -> String,
    step: impl FnMut(&mut S) -> Progress<T>,
) -> T {
    bounded(STALL, subject, goal, seen, step)
}

/// Wait for `attempt` to answer `Done`, with no progress signal to reset on.
///
/// WHY THESE ARE NOT `wait_for`. Nothing observable stands between asking and
/// the answer: the delivery ledger offers a terminal answer for one identity
/// and no count of anything, and the step that would produce a progress
/// signal runs on the serving thread rather than here. Pumping the bridge
/// would manufacture a signal from work that cannot advance this wait, which
/// is worse than saying plainly that this bound is a clock. Grep this name to
/// find every wait still measured that way.
#[track_caller]
fn spin_for<S, T>(
    subject: &mut S,
    goal: &str,
    seen: impl Fn(&S) -> String,
    attempt: impl FnMut(&mut S) -> Progress<T>,
) -> T {
    bounded(UNSIGNALLED, subject, goal, seen, attempt)
}

/// The one loop under both waits; `stall` is how long nothing may happen.
///
/// EVERY MESSAGE IS GENERATED HERE, so a site cannot write one that describes
/// success, and each says which bound fired, how long the wait had been
/// going, and what the site could see at that moment.
#[track_caller]
fn bounded<S, T>(
    stall: Duration,
    subject: &mut S,
    goal: &str,
    seen: impl Fn(&S) -> String,
    mut step: impl FnMut(&mut S) -> Progress<T>,
) -> T {
    let began = std::time::Instant::now();
    let ceiling = began + CEILING;
    let mut quiet_until = began + stall;
    loop {
        match step(subject) {
            Progress::Done(value) => return value,
            Progress::Worked => {
                quiet_until = std::time::Instant::now() + stall;
                continue;
            }
            Progress::Idle => {}
            // `seen` is not consulted here: `why` is the service's own
            // account, and reading the subject after its service was stopped
            // for that account would only find the handle gone.
            Progress::Lost(why) => panic!(
                "{goal} cannot happen now, after {:?}: {why}",
                began.elapsed()
            ),
        }
        let now = std::time::Instant::now();
        assert!(
            now < ceiling,
            "{goal} did not happen, though the bridge kept working, in {:?}: {}",
            now - began,
            seen(subject)
        );
        assert!(
            now < quiet_until,
            "{goal} did not happen, and nothing this wait could see moved for {stall:?}, after {:?}: {}",
            now - began,
            seen(subject)
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// What one pump did, with the verdict its report cannot carry.
struct Pumped {
    /// Whether the bridge moved, judged while the effects were still in the
    /// report. See [`advanced`].
    advanced: bool,
    /// The step's report. Its `effects` is empty: they went to the harvest.
    report: crate::private_input::PrivateInputCommitted,
}

/// Whether one coordinator step moved the bridge, as opposed to restating a
/// refusal it had already made.
///
/// A DEFERRED REFUSAL IS NOT PROGRESS. The bridge stops at the first entry the
/// order defers and reports that entry's effect and its refusal again on every
/// call, so a head that cannot go out produces a non-empty report for ever. A
/// wait that read any non-empty report as movement would restart its budget on
/// exactly the state the budget exists to catch -- and the control that
/// saturates the queue on purpose is the one it would then never catch.
fn advanced(report: &crate::private_input::PrivateInputCommitted) -> bool {
    report.batches_observed > 0
        || report.commits > 0
        || report
            .effects
            .iter()
            .any(|effect| effect.submitted().is_some())
        || report.refused.iter().any(released)
}

/// Whether the bridge reported that its service has ended.
///
/// RESTATED ON EVERY CALL, LIKE A DEFERRED REFUSAL, and just as much not
/// progress; but unlike one it is final, so a wait that sees it has nothing
/// left to wait for. Measured 2026-09-20: a service that died under load
/// reported this for 150 seconds while a control waited out its bound, and
/// the failure named the bound rather than the death.
fn ended(report: &crate::private_input::PrivateInputCommitted) -> bool {
    use crate::private_input::PrivateInputControlError as Error;
    report
        .refused
        .iter()
        .any(|refusal| matches!(refusal, Error::Ended))
}

/// `Idle`, unless the service has ended, which is `Lost` with its readiness.
///
/// FOR A WAIT THAT NEVER SEES THE BRIDGE. One that reads the boundary or the
/// delivery ledger gets no `Ended`; readiness is the one cheap fact that still
/// says the service is gone, and every wait in here begins after it was
/// `Ready`, so anything else is an ending.
fn not_yet<T>(handle: &PrivateInputHandle) -> Progress<T> {
    match handle.readiness() {
        PrivateInputReadiness::Ready => Progress::Idle,
        gone => Progress::Lost(format!("the service is {gone:?}")),
    }
}

/// Whether a refusal took its entry off the queue.
///
/// The complement of the order's own retryable set: a retryable refusal keeps
/// the entry at the head and says the same thing on the next call, so it is
/// not news. `Ended` and `Exhausted` are excluded for the same reason -- both
/// are restated on every call while the work stays exactly where it was.
fn released(refusal: &crate::private_input::PrivateInputControlError) -> bool {
    use crate::private_input::PrivateInputControlError as Error;
    use sophia_x_authority::AdmissionRefusal as Refusal;
    matches!(
        refusal,
        Error::ConnectionGone
            | Error::Refused(
                Refusal::Exhausted | Refusal::ConsumerGone | Refusal::ForeignServiceOwner,
                _,
            )
    )
}

#[test]
fn only_a_step_that_moved_the_bridge_counts_as_progress() {
    use crate::private_input::{
        PrivateInputCommitted, PrivateInputCommittedEffect, PrivateInputControlError as Error,
        PrivateInputSubmitted,
    };
    use sophia_x_authority::{
        AdmissionRefusal as Refusal, XAuthorityClientControlCommand, XAuthorityControlCommand,
        XAuthorityControlKind, XServerFrontendClientId,
    };
    let transaction = sophia_protocol::TransactionId::from_raw(7);
    let surface = sophia_protocol::SurfaceId::new(11, 1);
    let command = |refusal| {
        Error::Refused(
            refusal,
            XAuthorityClientControlCommand {
                client: XServerFrontendClientId::from_raw(1),
                command: XAuthorityControlCommand::ClearFocus {
                    transaction,
                    surface,
                },
            },
        )
    };
    let effect = |submitted| {
        PrivateInputCommittedEffect::new(
            transaction,
            surface,
            XAuthorityControlKind::ClearFocus,
            None,
            submitted,
        )
    };

    // An idle step is exactly the default, so the default is the base case.
    assert!(!advanced(&PrivateInputCommitted::default()));
    for (moved, report) in [
        (
            true,
            PrivateInputCommitted {
                batches_observed: 1,
                ..Default::default()
            },
        ),
        (
            true,
            PrivateInputCommitted {
                commits: 1,
                ..Default::default()
            },
        ),
        (
            true,
            PrivateInputCommitted {
                effects: vec![effect(Some(PrivateInputSubmitted {
                    transaction,
                    surface,
                    kind: XAuthorityControlKind::ClearFocus,
                }))],
                ..Default::default()
            },
        ),
        // THE SHAPE THIS PREDICATE EXISTS FOR. A deferred head reports its
        // effect with nothing submitted and its retryable refusal, on every
        // call, while the queue does not move. Reading that as movement is
        // what would let a wedged bridge reset a budget for ever.
        (
            false,
            PrivateInputCommitted {
                effects: vec![effect(None)],
                refused: vec![command(Refusal::Saturated)],
                ..Default::default()
            },
        ),
        (
            false,
            PrivateInputCommitted {
                refused: vec![command(Refusal::Unavailable)],
                ..Default::default()
            },
        ),
        (
            false,
            PrivateInputCommitted {
                refused: vec![command(Refusal::AuthorityUnreadable)],
                ..Default::default()
            },
        ),
        // These took the entry off the queue, so the next call will say
        // something different.
        (
            true,
            PrivateInputCommitted {
                refused: vec![command(Refusal::Exhausted)],
                ..Default::default()
            },
        ),
        (
            true,
            PrivateInputCommitted {
                refused: vec![command(Refusal::ConsumerGone)],
                ..Default::default()
            },
        ),
        (
            true,
            PrivateInputCommitted {
                refused: vec![command(Refusal::ForeignServiceOwner)],
                ..Default::default()
            },
        ),
        (
            true,
            PrivateInputCommitted {
                refused: vec![Error::ConnectionGone],
                ..Default::default()
            },
        ),
        // Restated on every call while the work stays staged.
        (
            false,
            PrivateInputCommitted {
                refused: vec![Error::Ended],
                ..Default::default()
            },
        ),
        (
            false,
            PrivateInputCommitted {
                refused: vec![Error::Exhausted],
                ..Default::default()
            },
        ),
        (
            false,
            PrivateInputCommitted {
                refused: vec![Error::Unavailable],
                ..Default::default()
            },
        ),
    ] {
        assert_eq!(advanced(&report), moved, "{report:?}");
    }
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

    // SHARED BETWEEN THE STEP AND THE DIAGNOSTIC, which is why it is a cell:
    // the step appends, and the failure message reads.
    let seen = std::cell::RefCell::new(Vec::new());
    wait_for(
        &mut fixture,
        "a commit that applied a surface for the draw",
        |_| format!("outcomes so far {:?}", seen.borrow()),
        |fixture| {
            fixture.step(|_, report| {
                let mut seen = seen.borrow_mut();
                let mut committed_an_applied_surface = false;
                for outcome in &report.outcomes {
                    seen.push(outcome.outcome);
                    if outcome.outcome == sophia_protocol::TransactionOutcome::Committed
                        && !outcome.applied.is_empty()
                    {
                        committed_an_applied_surface = true;
                    }
                }
                committed_an_applied_surface.then_some(())
            })
        },
    );
    let seen = seen.into_inner();

    assert!(
        !seen.contains(&sophia_protocol::TransactionOutcome::RejectedStaleSurface),
        "the draw was reconciled to the Engine's predecessor rather than \
         rejected as stale: {seen:?}"
    );
    drop(peer);
}
