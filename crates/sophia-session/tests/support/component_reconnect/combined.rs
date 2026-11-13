//! G3-5b Session controls: the combined row through the controlled driver,
//! `turn_combined`, on the t100 harness with the lean lane negotiated. The
//! combined native fixture is the native and the composition target, and
//! G3-4's stub is the renderer: the real route decides Stopped and Expired,
//! and every render is an injected reply. Stop, close and replacement are
//! the real chain (`stop`, `request_stop`, plain `close`). Rings are set up
//! with today's turns, before any frame is queued, so a job a control left
//! in the row is collected only by that control's own hooked turn.
//!
//! The lane is terminal-only and lean, so no Prepared notice is sent and an
//! accepted ResourceStatus stays unpublished until its frame's terminal.
//! Before a terminal the controls read the owner's own state for the exact
//! frame: a validated copy is held privately for that frame, a refused one
//! is absent. The generic lease refuses frame-owned images until Presented,
//! as production needs, so it is not used. Peer-visible status is asserted
//! only after the actual terminal. Every frame outcome is recorded by candidate,
//! kind and reason, and every wait is a bounded condition.
//!
//! Declared faults are labelled where they are used. They prove that the
//! row keeps what came back and reports once, not that the fault can
//! happen. The two-proposal call order (R4) and the hooks=None accounting
//! comparison are the runtime's lane controls, through the same entry.
use super::super::super::content::reconnect_fixture::project;
use super::*;
use crate::shell_component_connections::combined_row::{CombinedTurn, turn_cursor};
use sophia_backend_live::{
    CombinedNativeFixture, CombinedRelease, CombinedSettleFault, LiveProductionVisualRuntime,
    ShellContentNative, StubReply,
};
use sophia_protocol::shell_files::{ShellFrameCommit, ShellFrameRecord, ShellFrameStreamKey};
use sophia_runtime::{ContentRetireOwner, FrameImageResidency, ShellTransportError};
use sophia_shell_client::{FrameStream, RingUpload};

const WAIT: Duration = Duration::from_secs(10);
const PIXEL: [u8; 4] = [1, 2, 3, 255];
/// Colour over zero alpha: the premultiplied scan refuses it whatever the
/// channel order.
const INVALID: [u8; 4] = [255, 255, 255, 0];

fn exported() -> StubReply {
    StubReply::Exported {
        size: output().size,
        lease: 7,
    }
}

/// The harness beside the lane's native, visual runtime and scene.
struct Lane {
    h: Harness,
    native: CombinedNativeFixture,
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
}

/// One attempt in slot 0: its peer, its frame stream and what it received.
struct Attempt {
    key: ComponentConnectionKey,
    peer: ShellConnection,
    stream: FrameStream,
    /// The last candidate generation submitted on today's content path.
    submitted: u64,
    /// Every frame outcome: candidate generation, kind and reason.
    outcomes: Vec<(u64, u16, u16)>,
    /// The ordinary projection of each bundle submitted on today's path.
    bundles: Vec<LiveShellContentFrame>,
    /// ResourceStatus records for the resource, valid and not.
    accepted: usize,
    refused: usize,
}

impl Attempt {
    /// Candidate `n`'s frame outcomes, as (kind, reason).
    fn terminals(&self, n: u64) -> Vec<(u16, u16)> {
        self.outcomes
            .iter()
            .filter(|(generation, _, _)| *generation == n)
            .map(|&(_, kind, reason)| (kind, reason))
            .collect()
    }
}

impl Lane {
    fn new() -> Self {
        let mut h = Harness::new();
        h.owner.enable_frame_transactions(true, true).unwrap();
        Self {
            h,
            native: CombinedNativeFixture::new(&[output()]),
            runtime: LiveProductionVisualRuntime::new(&[output()], None).unwrap(),
            scene: LiveProductionCpuScene::new(output().size),
        }
    }

    /// One controlled owner turn. Slot 0 composes on the Shell layer with
    /// the hardware-plane cursor; every other slot has no layer and takes
    /// today's path. Slot 0's failure, if any.
    fn turn(&mut self) -> Option<ShellTransportError> {
        let outputs = [output()];
        let (cursor, plane) = turn_cursor(true, None);
        let mut turn = CombinedTurn {
            native: &mut self.native,
            runtime: &self.runtime,
            scene: &self.scene,
            outputs: &outputs,
            layers: std::array::from_fn(|slot| (slot == 0).then_some(LiveShellContentLayer::Shell)),
            cursor,
            plane,
        };
        let mut failures = self.h.owner.processes.turn_combined(&mut turn);
        assert!(failures[1..].iter().all(Option::is_none), "{failures:?}");
        failures[0].take().map(|(_, error)| error)
    }

    /// Today's owner turn, unhooked.
    fn plain_turn(&mut self) {
        for failure in self.h.owner.processes.turn_connected() {
            assert!(failure.is_none(), "{failure:?}");
        }
    }

    fn states(&self) -> Vec<(u64, &'static str)> {
        self.h.owner.processes.combined_row(0).states()
    }

    /// A lane attempt in slot 0 with its stream open and its ring set up.
    fn attempt(&mut self) -> Attempt {
        let (key, mut peer) = wire::connect_lane(&mut self.h.owner, 0);
        self.h
            .owner
            .with_service(key, |_, t| wire::allocate_extent(t, &mut peer, 1))
            .unwrap();
        let mut stream = FrameStream::new();
        stream
            .open(
                &mut peer,
                TransactionId::from_raw(30),
                ShellFrameStreamKey {
                    grant: key.grant,
                    output: OUTPUT,
                    allocation: ALLOCATION,
                    facts_generation: 1,
                    stream_id: 1,
                },
            )
            .unwrap();
        let mut a = Attempt {
            key,
            peer,
            stream,
            submitted: 0,
            outcomes: Vec::new(),
            bundles: Vec::new(),
            accepted: 0,
            refused: 0,
        };
        let deadline = Instant::now() + WAIT;
        loop {
            a.peer.poll_io().unwrap();
            self.plain_turn();
            self.content(&mut a, false);
            if a.stream.has_credit() && a.peer.image_adapter().unwrap().ring_identity().is_some() {
                match a.peer.image_acquire_ring_fill(4, Instant::now()) {
                    Ok(mut fill) => {
                        a.peer.image_abort_ring_fill(&mut fill).unwrap();
                        return a;
                    }
                    Err(
                        sophia_shell_client::ImageAdapterError::Again
                        | sophia_shell_client::ImageAdapterError::Busy,
                    ) => {}
                    Err(error) => panic!("ring setup: {error:?}"),
                }
            }
            assert!(Instant::now() < deadline, "stream/ring setup stalled");
            std::thread::yield_now();
        }
    }

    /// Fills and queues frame `n`: candidate generation `n`, resource
    /// generation `n`, one 1x1 pixel.
    fn submit(&mut self, a: &mut Attempt, n: u64, pixel: [u8; 4]) {
        self.submit_at(a, n, pixel, &[0]);
    }

    /// As `submit`, with the one resource placed once at each x offset.
    fn submit_at(&mut self, a: &mut Attempt, n: u64, pixel: [u8; 4], xs: &[i16]) {
        let mut commit = ShellFrameRecord::Commit(ShellFrameCommit {
            transaction: TransactionId::from_raw(40 + n),
            image: None,
            retire: None,
            candidate: ContentCandidate {
                grant: a.key.grant,
                output: OUTPUT,
                candidate_generation: n,
                facts_generation: 1,
                pacing_permit: 1,
                interaction_generation: 1,
                surfaces: vec![ContentSurface {
                    allocation: ALLOCATION,
                    scale_generation: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: 1,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: xs
                    .iter()
                    .map(|&x| ContentPlacement {
                        resource: ContentResourceId {
                            id: RESOURCE.id,
                            generation: n,
                        },
                        surface_index: 0,
                        destination_x_px: x.into(),
                        destination_y_px: 0,
                    })
                    .collect(),
                targets: vec![],
            },
        });
        a.stream
            .submit_ring(
                &mut a.peer,
                &mut commit,
                RingUpload {
                    resource: RESOURCE.id,
                    resource_generation: n,
                    allocation: 1,
                    allocation_generation: 1,
                    output: 1,
                    output_generation: 1,
                    topology_generation: 1,
                    width: 1,
                    height: 1,
                },
                Instant::now(),
                |pixels| pixels.copy_from_slice(&pixel),
            )
            .unwrap();
        assert!(!a.stream.has_credit());
    }

    /// A connected attempt's content turn: resources and candidates, with
    /// `submit` the ordinary submission of its next candidate, then its
    /// frame and content records. Nothing for a revoked attempt.
    fn content(&mut self, a: &mut Attempt, submit: bool) {
        if self.h.owner.phase(a.key).ok() != Some(ComponentConnectionPhase::Connected) {
            return;
        }
        let backend = &mut self.h.backend;
        let submitted = &mut a.submitted;
        let bundles = &mut a.bundles;
        self.h
            .owner
            .with_service(a.key, |service, t| {
                let allocations = t.content_allocation_snapshots();
                let now = crate::shell_component_connections::content_now_msec();
                t.service_content_resources(now).unwrap();
                t.service_content_candidates(
                    &[ContentCandidateContext {
                        output: OUTPUT,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    now,
                )
                .unwrap();
                if submit
                    && let Some((next, generation)) = t.next_content_submission()
                    && next == OUTPUT
                    && generation > *submitted
                {
                    let bundle = t.begin_content_submission(OUTPUT, generation, now).unwrap();
                    bundles.push(project(t, output(), &bundle));
                    let ShellComponentService::Bar(bar) = service else {
                        panic!("bar required")
                    };
                    prepare(&mut bar.content, t, backend, output(), bundle);
                    *submitted = generation;
                }
                t.poll_io().unwrap();
            })
            .unwrap();
        while let Some(event) = a.peer.take_frame().unwrap() {
            a.stream.observe(&event).unwrap();
            if let ShellFrameRecord::Outcome { outcome, .. } = event {
                a.outcomes
                    .push((outcome.candidate_generation, outcome.kind, outcome.reason));
            }
        }
        while let Some((_, event)) = a.peer.take_content().unwrap() {
            if let ShellContentRecord::ResourceStatus(status) = &event
                && status.resource.id == RESOURCE.id
            {
                if status.status == 2 {
                    a.accepted += 1;
                } else {
                    a.refused += 1;
                }
            }
        }
    }

    /// The owner's own state for candidate `n`'s copy against exactly that
    /// frame, whether or not its status is published yet. Validated, it is
    /// held privately for the frame until Presented; refused or never
    /// admitted, it is absent. No lease is made.
    fn residency(&mut self, a: &Attempt, n: u64) -> FrameImageResidency {
        let owner = ContentRetireOwner {
            grant: a.key.grant,
            output: OUTPUT,
            candidate_generation: n,
        };
        let resource = ContentResourceId {
            id: RESOURCE.id,
            generation: n,
        };
        self.h
            .owner
            .with_service(a.key, |_, t| {
                t.content_frame_image_residency(owner, resource)
            })
            .unwrap()
            .expect("the lane has a content store")
    }

    /// The owner's content charges under `a`'s grant.
    fn usage(&mut self, a: &Attempt) -> Option<sophia_runtime::ContentMemoryUsage> {
        self.h
            .owner
            .with_service(a.key, |_, t| t.content_usage())
            .unwrap()
    }

    /// Hooked turns with `a`'s content until `done`; any slot failure
    /// panics.
    fn drive_until(
        &mut self,
        a: &mut Attempt,
        what: &str,
        mut done: impl FnMut(&mut Self, &Attempt) -> bool,
    ) {
        let deadline = Instant::now() + WAIT;
        while !done(self, a) {
            a.peer.poll_io().unwrap();
            if let Some(error) = self.turn() {
                panic!("{what}: slot failure {error:?}");
            }
            self.content(a, false);
            assert!(
                Instant::now() < deadline,
                "{what} stalled: {:?}",
                self.states()
            );
            std::thread::yield_now();
        }
    }

    /// Hooked turns until the row holds exactly one job, in `state`; the job.
    fn until(&mut self, a: &mut Attempt, state: &'static str) -> u64 {
        self.drive_until(a, state, |lane, _| {
            let states = lane.states();
            states.len() == 1 && states[0].1 == state
        });
        self.states()[0].0
    }

    /// Hooked turns until slot 0 fails; that failure.
    fn until_failure(&mut self, a: &mut Attempt) -> ShellTransportError {
        let deadline = Instant::now() + WAIT;
        loop {
            a.peer.poll_io().unwrap();
            if let Some(error) = self.turn() {
                return error;
            }
            self.content(a, false);
            assert!(Instant::now() < deadline, "no failure: {:?}", self.states());
            std::thread::yield_now();
        }
    }

    /// Drives `a`'s ordinary candidate `generation` to presentation through
    /// hooked turns, with the backend's submission and completion
    /// simulated, as the bundled probe does. Terminal-only sends no Prepared
    /// notice, so the simulation follows the owner's own submission.
    fn finish_ordinary(&mut self, a: &mut Attempt, generation: u64) {
        let deadline = Instant::now() + WAIT;
        let mut completed = false;
        while a.terminals(generation).is_empty() {
            a.peer.poll_io().unwrap();
            if let Some(error) = self.turn() {
                panic!("ordinary {generation}: slot failure {error:?}");
            }
            self.content(a, true);
            if completed {
                let runtime = self.h.backend.runtime();
                self.h
                    .owner
                    .with_service(a.key, |service, t| {
                        let ShellComponentService::Bar(bar) = service else {
                            panic!("bar required")
                        };
                        if bar.observe_presentation(t, runtime).unwrap() {
                            t.poll_io().unwrap();
                        }
                    })
                    .unwrap();
            }
            if a.submitted >= generation && !completed {
                self.h.backend.simulate_submit(output().id);
                self.h.backend.simulate_completion(output().id);
                completed = true;
            }
            assert!(Instant::now() < deadline, "ordinary {generation} stalled");
            std::thread::yield_now();
        }
        assert_eq!(
            a.terminals(generation),
            [(2, ContentReason::None as u16)],
            "ordinary {generation} presented once"
        );
    }

    fn decline(&mut self, job: u64) -> Option<CombinedRelease> {
        self.h
            .owner
            .processes
            .combined_row_mut(0)
            .decline(&mut self.native, job)
    }
}

fn row_error(error: &ShellTransportError, job: u64, what: &str) -> bool {
    matches!(error, ShellTransportError::CombinedRow(text)
        if *text == format!("combined job {job}: {what}"))
}

#[test]
fn an_exported_job_is_held_then_declined_to_one_release() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    assert_eq!(
        lane.native.composes(),
        1,
        "one composition reserved and planned"
    );
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    assert_eq!(lane.until(&mut a, "held"), job);
    lane.drive_until(&mut a, "the owner-side validation result", |lane, a| {
        lane.residency(a, 1) == FrameImageResidency::HeldForFrame
    });
    assert!(
        a.terminals(1).is_empty(),
        "the Held candidate's terminal is c's"
    );
    assert_eq!(lane.decline(job), Some(CombinedRelease::Transferred));
    assert_eq!(lane.native.renderer_releases(), 1);
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 0);
}

#[test]
fn a_declared_compose_fault_fails_the_slot_with_nothing_reserved() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.native.declare_compose_fault("declared compose fault");
    lane.submit(&mut a, 1, PIXEL);
    let error = lane.until_failure(&mut a);
    assert!(
        matches!(&error, ShellTransportError::CombinedComposition(reason)
            if reason == "declared compose fault"),
        "the first error survives: {error:?}"
    );
    assert_eq!(
        lane.native.composes(),
        1,
        "Fail ends the image service: no second proposal composes"
    );
    assert_eq!(
        lane.h.owner.processes.combined_row(0).pending(),
        0,
        "no ticket"
    );
    assert_eq!(
        lane.h.owner.processes.image_pending(0),
        0,
        "nothing admitted"
    );
    assert_eq!(lane.native.renderer_releases(), 0);
    // The controlled driver's stop is the real chain.
    lane.h.owner.stop(a.key).unwrap();
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert!(lane.turn().is_none());
    assert_eq!(lane.native.composes(), 1, "a revoked slot proposes nothing");
}

/// R1a: J1 renders and exports before A stops; only its collection waits.
/// B's ordinary job is admitted and validated while J1 is still the row's,
/// J1's old result then settles against its own stopped entry without
/// touching B, and B's combined successor reserves on the same facade only
/// after J1's Release receipt transferred.
#[test]
fn an_exported_late_result_is_collected_after_stop_and_replacement() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let j1 = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    lane.h.owner.stop(a.key).unwrap();
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert_eq!(
        lane.states(),
        [(j1, "in flight")],
        "close collected nothing"
    );
    drop(a);
    let mut b = lane.attempt();
    lane.submit(&mut b, 1, PIXEL);
    let deadline = Instant::now() + WAIT;
    while lane.residency(&b, 1) != FrameImageResidency::HeldForFrame {
        b.peer.poll_io().unwrap();
        lane.plain_turn();
        lane.content(&mut b, false);
        assert!(Instant::now() < deadline, "B's ordinary copy stalled");
        std::thread::yield_now();
    }
    assert_eq!(lane.states(), [(j1, "in flight")], "J1 is still the row's");
    assert_eq!(lane.native.composes(), 1, "today's turns composed nothing");
    // One hooked visit: J1 is delivered once to its stopped entry, its
    // old charge settles, and the walk gives its Held lease up.
    assert!(lane.turn().is_none());
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.renderer_releases(), 1);
    lane.finish_ordinary(&mut b, 1);
    // The terminal publishes the status lean held back.
    lane.drive_until(&mut b, "B's published status", |_, b| b.accepted > 0);
    assert_eq!(
        (b.accepted, b.refused),
        (1, 0),
        "B's one result stands; J1's never reached B"
    );
    assert_eq!(
        lane.residency(&b, 1),
        FrameImageResidency::Public,
        "the Presented terminal transferred B's image"
    );
    assert_eq!(lane.h.owner.processes.image_pending(0), 0);
    // The successor: the same facade reserves now that J1's receipt moved.
    lane.submit(&mut b, 2, PIXEL);
    let j2 = lane.until(&mut b, "in flight");
    assert_ne!(j2, j1);
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    assert_eq!(lane.until(&mut b, "held"), j2);
    assert_eq!(lane.decline(j2), Some(CombinedRelease::Transferred));
    assert_eq!(lane.native.renderer_releases(), 2);
}

/// R1b: J1's Release finds the queue Full. The debt survives close and the
/// replacement, holds the facade's cell so B's reserve takes today's path,
/// and is what refuses final quiescence once every other blocker is gone.
/// The walk flushes it once the queue drains.
#[test]
fn full_release_debt_survives_reconnect_and_holds_final_quiescence() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let j1 = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    lane.h.owner.stop(a.key).unwrap();
    drop(a);
    lane.native.fill_renderer_queue();
    assert!(lane.turn().is_none());
    assert_eq!(lane.states(), [(j1, "debt")]);
    let mut b = lane.attempt();
    lane.submit(&mut b, 1, PIXEL);
    lane.drive_until(&mut b, "B's ordinary copy", |lane, b| {
        lane.residency(b, 1) == FrameImageResidency::HeldForFrame
    });
    assert_eq!(
        lane.native.composes(),
        2,
        "B's frame composed once, was refused a reservation and went ordinary"
    );
    assert_eq!(
        lane.states(),
        [(j1, "debt")],
        "the debt outlived the replacement"
    );
    lane.finish_ordinary(&mut b, 1);
    // Every other final-quiescence blocker goes: B stops, its work ends.
    lane.h.owner.stop(b.key).unwrap();
    drop(b);
    let deadline = Instant::now() + WAIT;
    while lane.h.owner.processes.image_pending(0) != 0 {
        assert!(lane.turn().is_none());
        assert!(Instant::now() < deadline, "B's image work did not end");
        std::thread::yield_now();
    }
    assert_eq!(lane.states(), [(j1, "debt")]);
    let backend = std::mem::replace(
        &mut lane.h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    let Err(backend) = lane.h.owner.processes.finish_after_backend_drop(backend) else {
        panic!("final quiescence ignored the row's Release debt");
    };
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert!(lane.turn().is_none());
    assert!(lane.states().is_empty(), "the receipt transferred");
    assert_eq!(lane.native.renderer_releases(), 1);
    assert!(
        lane.h
            .owner
            .processes
            .finish_after_backend_drop(backend)
            .is_ok(),
        "with the debt transferred nothing refuses"
    );
}

/// R1: an old job's invariant error after replacement. A's J1 has rendered
/// and exported, A stops, and B connects in the same slot; only then is J1
/// refused settlement (a declared fault). The error names J1 under A's
/// grant as a cleanup diagnostic. It is not B's failure: B stays connected,
/// J1 keeps its returned owners and its retry, and B's own combined job then
/// runs on the facade J1's transfer freed.
#[test]
fn an_old_job_refused_after_replacement_stays_a_diagnostic_of_its_attempt() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let j1 = lane.until(&mut a, "in flight");
    let old = a.key;
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    lane.h.owner.stop(old).unwrap();
    drop(a);
    let mut b = lane.attempt();
    lane.native
        .declare_settle_fault(CombinedSettleFault::Refused);
    assert!(lane.turn().is_none(), "A's error is not B's failure");
    let what = format!("combined job {j1}: the issuing facade refused its result");
    assert_eq!(
        lane.h.owner.processes.combined_row(0).cleanup_diagnostics(),
        [(old.grant, what.clone())],
        "the error keeps A's grant"
    );
    assert_eq!(
        lane.states(),
        [(j1, "unsettled")],
        "J1 keeps its returned owners"
    );
    assert_eq!(
        lane.h.owner.phase(b.key).unwrap(),
        ComponentConnectionPhase::Connected
    );
    // The retry settles J1, unreported; its ended grant gives the lease up.
    assert!(lane.turn().is_none());
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.renderer_releases(), 1);
    // B's work goes on, on the same facade.
    lane.submit(&mut b, 1, PIXEL);
    let j2 = lane.until(&mut b, "in flight");
    assert_ne!(j2, j1);
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    assert_eq!(lane.until(&mut b, "held"), j2);
    assert_eq!(lane.decline(j2), Some(CombinedRelease::Transferred));
    assert_eq!(lane.native.renderer_releases(), 2);
    assert_eq!(
        lane.h.owner.processes.combined_row(0).cleanup_diagnostics(),
        [(old.grant, what)],
        "reported once, and nothing of B's"
    );
}

#[test]
fn a_stop_before_render_gives_stopped_with_nothing_held() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    assert!(lane.native.park_combined(), "the validator forwarded J1");
    lane.h.owner.stop(a.key).unwrap();
    drop(a);
    // A revoked slot keeps its visits; nothing has come back, and the job
    // does not end because its grant did.
    assert!(lane.turn().is_none());
    assert_eq!(lane.states(), [(job, "in flight")]);
    // The route reads the stop from the token and does not render.
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    assert_eq!(lane.states(), [(job, "in flight")]);
    assert!(lane.turn().is_none());
    assert!(
        lane.states().is_empty(),
        "Stopped settles, delivers and retires with its ended grant"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "nothing was held");
}

/// R2: the disposition read before settlement picks the row's state. Only
/// an exported render is Held; the rest are Failed with nothing held.
/// Mismatch is Malformed before reconciliation, so no valid status. When
/// the grant ends, Held is given up and Failed retires.
#[test]
fn each_disposition_takes_its_own_row_transition() {
    for (reply, late, after, valid) in [
        (exported(), false, "held", true),
        (StubReply::Failed, false, "failed", true),
        (exported(), true, "failed", true),
        (StubReply::Mismatch, false, "failed", false),
    ] {
        let mut lane = Lane::new();
        let mut a = lane.attempt();
        lane.submit(&mut a, 1, PIXEL);
        let job = lane.until(&mut a, "in flight");
        let at = if late {
            Instant::now() + Duration::from_secs(3600)
        } else {
            Instant::now()
        };
        lane.native.serve_combined(output().id, at, reply);
        assert_eq!(lane.until(&mut a, after), job, "{reply:?} late={late}");
        if valid {
            // The validation result is real; the candidate's terminal is c's.
            lane.drive_until(&mut a, "the owner-side validation result", |lane, a| {
                lane.residency(a, 1) == FrameImageResidency::HeldForFrame
            });
            assert!(a.terminals(1).is_empty(), "{reply:?} late={late}");
        } else {
            // Malformed before reconciliation: the copy's own failure rejects
            // this candidate, once, and nothing is valid.
            lane.drive_until(&mut a, "the candidate's rejection", |_, a| {
                !a.terminals(1).is_empty()
            });
            assert_eq!(
                a.terminals(1),
                [(3, ContentReason::Malformed as u16)],
                "the Mismatch rejected its candidate as Malformed, once"
            );
            assert_eq!(
                lane.residency(&a, 1),
                FrameImageResidency::Absent,
                "never accepted"
            );
            assert_eq!(a.accepted, 0, "no status published as valid");
        }
        assert_eq!(lane.states(), [(job, after)], "the current grant keeps it");
        assert_eq!(lane.native.renderer_releases(), 0);
        lane.h.owner.stop(a.key).unwrap();
        assert!(lane.turn().is_none());
        assert!(lane.states().is_empty(), "{after} ends with its grant");
        assert_eq!(
            lane.native.renderer_releases(),
            usize::from(after == "held"),
            "{reply:?} late={late}"
        );
    }
}

/// Declared fault: the issuing facade refuses settlement once.
#[test]
fn a_settlement_refusal_keeps_the_result_and_reports_once() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .declare_settle_fault(CombinedSettleFault::Refused);
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    let error = lane.until_failure(&mut a);
    assert!(
        row_error(&error, job, "the issuing facade refused its result"),
        "{error:?}"
    );
    assert_eq!(lane.states(), [(job, "unsettled")]);
    assert_eq!(
        lane.until(&mut a, "held"),
        job,
        "the retry settles, unreported"
    );
    assert_eq!(lane.decline(job), Some(CombinedRelease::Transferred));
    assert_eq!(lane.native.renderer_releases(), 1);
}

/// Declared fault: the facade's half ran, and the credit clear is refused
/// once. The retry is only the credit clear: the lease is held once.
#[test]
fn a_refused_credit_retries_only_the_credit() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .declare_settle_fault(CombinedSettleFault::Credit);
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    let error = lane.until_failure(&mut a);
    assert!(
        row_error(&error, job, "the validator refused its credit"),
        "{error:?}"
    );
    assert_eq!(lane.states(), [(job, "credit")]);
    assert_eq!(lane.until(&mut a, "held"), job);
    assert_eq!(lane.decline(job), Some(CombinedRelease::Transferred));
    assert_eq!(lane.native.renderer_releases(), 1);
}

/// Declared fault: the runtime refuses the delivery once. The row keeps the
/// settled result, reports it once and offers it again.
#[test]
fn a_refused_delivery_is_offered_again_and_reported_once() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.h
        .owner
        .processes
        .combined_row_mut(0)
        .declare_delivery_refusal();
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    let error = lane.until_failure(&mut a);
    assert!(
        row_error(&error, job, "the runtime refused its result"),
        "{error:?}"
    );
    assert_eq!(lane.states(), [(job, "undelivered")]);
    assert_eq!(lane.until(&mut a, "held"), job);
    lane.drive_until(&mut a, "the owner-side validation result", |lane, a| {
        lane.residency(a, 1) == FrameImageResidency::HeldForFrame
    });
    assert!(a.terminals(1).is_empty());
    assert_eq!(lane.decline(job), Some(CombinedRelease::Transferred));
}

/// A failed validation returns the plan: the row cancels the reservation,
/// with confirmation, and never takes or settles a render.
#[test]
fn a_returned_plan_cancels_its_reservation_only() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, INVALID);
    lane.until(&mut a, "in flight");
    lane.drive_until(&mut a, "the returned plan", |lane, _| {
        lane.h.owner.processes.combined_row(0).pending() == 0
    });
    assert_eq!(
        lane.h.owner.processes.combined_row(0).unconfirmed_cancels(),
        0,
        "the cancellation was confirmed"
    );
    // The failed copy rejects its candidate once, as Malformed.
    lane.drive_until(&mut a, "the candidate's rejection", |_, a| {
        !a.terminals(1).is_empty()
    });
    assert_eq!(
        a.terminals(1),
        [(3, ContentReason::Malformed as u16)],
        "the failed validation rejected its candidate as Malformed, once"
    );
    assert_eq!(
        lane.residency(&a, 1),
        FrameImageResidency::Absent,
        "never accepted"
    );
    assert_eq!(a.accepted, 0);
    assert_eq!(lane.native.renderer_releases(), 0);
    // The facade's cell is free again: the next admission reserves.
    let (head, _inbox) = ShellContentNative::reserve_combined_frame(&mut lane.native, output().id)
        .expect("the cancelled reservation freed the cell");
    assert!(matches!(
        ShellContentNative::cancel_combined_frame(&mut lane.native, &head),
        Ok(true)
    ));
}

/// R3, declared fault: the plan is refused after admission consumed the
/// frame, so this is the refused send, not a deferral. The runtime cancels
/// that one reservation and the row confirms it; no entry outlives the
/// proposal. The consumed candidate is withdrawn Cancelled and rejected
/// once; nothing re-proposes it. A genuinely new candidate, sent once the
/// rejection returned the frame credit, is the next admission. Same-frame
/// reserve, cancel, reserve is a pre-admission deferral and stays the
/// runtime's call-order control.
#[test]
fn a_refused_plan_cancels_once_and_settles_its_consumed_candidate() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.native.declare_plan_refusal();
    lane.submit(&mut a, 1, PIXEL);
    lane.drive_until(&mut a, "the consumed candidate's rejection", |_, a| {
        !a.terminals(1).is_empty()
    });
    assert_eq!(a.terminals(1), [(3, ContentReason::Cancelled as u16)]);
    assert_eq!(
        lane.h.owner.processes.combined_row(0).pending(),
        0,
        "no entry outlived its proposal"
    );
    assert_eq!(
        lane.h.owner.processes.combined_row(0).unconfirmed_cancels(),
        0,
        "the refused admission's cancellation was confirmed"
    );
    assert_eq!(
        lane.native.composes(),
        1,
        "the consumed frame is not proposed again"
    );
    assert_eq!(
        lane.residency(&a, 1),
        FrameImageResidency::Absent,
        "never accepted"
    );
    assert_eq!(lane.native.renderer_releases(), 0);
    // The next admission is a new candidate, after the credit came back.
    lane.drive_until(&mut a, "the frame credit", |_, a| a.stream.has_credit());
    lane.submit(&mut a, 2, PIXEL);
    let job = lane.until(&mut a, "in flight");
    assert_eq!(lane.native.composes(), 2);
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    assert_eq!(lane.until(&mut a, "held"), job);
    assert_eq!(lane.decline(job), Some(CombinedRelease::Transferred));
    assert_eq!(
        a.terminals(1),
        [(3, ContentReason::Cancelled as u16)],
        "rejected once"
    );
}

#[path = "combined_r4.rs"]
mod r4;
