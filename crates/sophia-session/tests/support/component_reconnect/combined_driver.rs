//! G3-5d d-2b1 Session controls: the controlled owner turn split into its
//! four steps (REV2 R5, as amended by REV3 section 6), and its failure order.
//!
//! `combined_owner_turn` runs, in this order: the images step (every slot's
//! hooked image-owner service), content (the bar's real content service with
//! the row lent, for each connected slot that has a layer), the walk (every
//! slot) and native service. Each call is recorded just before it is made,
//! so a control reads the driver's actual call order. A slot whose image
//! service failed is stopped through `fail_service` before content runs; a
//! content error and a row error of a slot with no earlier failure go to
//! `fail_service` in their own step. A `fail_service` error returns at once:
//! later steps do not run, and the row keeps its jobs, their debt and the
//! connection. That return is not completed cleanup, and no control here
//! reaches it.
//!
//! Faults are labelled where they are used: the image-service fault (REV3
//! 5.1) and the classification fault prove the order and custody that
//! follow a failure, not that the failure happens. The running controls use
//! d-1's running core, whose worker renders through the deterministic seam
//! and leases the result. The stub controls prove a Full Release's debt
//! (REV3 5.2) and the driver's call order (MD4) only; the running result
//! turn, Submitted and its trace are d-2b2's.
use super::super::super::super::content::reconnect_fixture::adopt_lane_facts;
use super::*;
use crate::live_session::metadata_shell::ContentNative;
use crate::shell_component_connections::combined_row::CombinedConsume;
use sophia_backend_live::LiveProductionCursorPresentation;
use sophia_protocol::{Point, Rect};

/// REV3 5.1's declared image-service fault.
const IMAGE_FAULT: &str = "declared image-service fault";
/// The declared classification fault.
const CLASSIFY_FAULT: &str = "a declared classification fault";

/// One call the driver made, recorded just before it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Images,
    FailService(usize),
    Content(usize),
    Walk,
    Native,
}
use Step::{Content, FailService, Images, Native, Walk};

/// What one controlled owner turn lends the driver. Content needs the
/// visual runtime mutably, so the images step's `CombinedTurn` is built from
/// these parts for that step only.
struct DriverTurn<'a, N> {
    native: &'a mut N,
    runtime: &'a mut LiveProductionVisualRuntime,
    scene: &'a LiveProductionCpuScene,
    outputs: &'a [HeadlessOutput],
    layers: [Option<LiveShellContentLayer>; MAX_SHELL_COMPONENTS],
    cursor: LiveProductionCursorPresentation,
    plane: Option<Point>,
    instant: Instant,
}

/// One slot's content step: `Err` is the content service's error.
type ContentStep<'c, 'a, N> = dyn FnMut(
        &mut ShellComponentSession,
        ComponentConnectionKey,
        &mut DriverTurn<'a, N>,
    ) -> std::result::Result<(), String>
    + 'c;

/// Each slot's first failure in one turn, as text.
type FirstFailures = [Option<String>; MAX_SHELL_COMPONENTS];

/// One content call's observation: the fixture's accept count and its
/// (release attempts, Transferred receipts), read at the call's boundary.
type Observed = (usize, (usize, usize));

impl ShellComponentSession {
    /// The controlled owner turn (module doc). Each slot's first failure in
    /// this turn, or `fail_service`'s own error, returned at once.
    fn combined_owner_turn<'a, N: ShellContentNative>(
        &mut self,
        turn: &mut DriverTurn<'a, N>,
        content: &mut ContentStep<'_, 'a, N>,
        native: &mut dyn FnMut(&mut N),
        steps: &mut Vec<Step>,
    ) -> Result<FirstFailures> {
        let mut first = FirstFailures::default();
        // 1. Images: a slot whose service failed is stopped before content.
        steps.push(Images);
        let failures = self.processes.turn_combined_images(&mut CombinedTurn {
            native: &mut *turn.native,
            runtime: &*turn.runtime,
            scene: turn.scene,
            outputs: turn.outputs,
            layers: turn.layers,
            cursor: turn.cursor,
            plane: turn.plane,
            instant: turn.instant,
        });
        for (key, error) in failures.into_iter().flatten() {
            first[key.slot] = Some(error.to_string());
            steps.push(FailService(key.slot));
            self.fail_service(key, &error)?;
        }
        // 2. Content: connected slots with a ready service and a layer.
        for (slot, failed) in first.iter_mut().enumerate() {
            let Some(key) = self.processes.attempt(slot) else {
                continue;
            };
            let ready = self.ready[slot]
                .as_ref()
                .is_some_and(|ready| ready.key == key);
            if turn.layers[slot].is_none()
                || !ready
                || self.processes.phase(key)? != ComponentConnectionPhase::Connected
            {
                continue;
            }
            steps.push(Content(slot));
            if let Err(error) = content(self, key, turn) {
                *failed = Some(error.clone());
                steps.push(FailService(slot));
                self.fail_service(key, &error)?;
            }
        }
        // 3. Walk: every slot, whatever its phase.
        steps.push(Walk);
        let failures = self
            .processes
            .turn_combined_walk(&mut *turn.native, turn.instant);
        for (key, error) in failures.into_iter().flatten() {
            if first[key.slot].is_none() {
                first[key.slot] = Some(error.to_string());
                steps.push(FailService(key.slot));
                self.fail_service(key, &error)?;
            }
        }
        // 4. Native.
        steps.push(Native);
        native(&mut *turn.native);
        Ok(first)
    }
}

impl Lane {
    /// The lane on d-1's running core, with an owner wake every render
    /// result rings (the core's one owner wake), empty when returned.
    fn running() -> (Self, sophia_wake::Wake) {
        let mut h = Harness::new();
        h.owner.enable_frame_transactions(true, true).unwrap();
        let lane = Self {
            h,
            native: CombinedNativeFixture::running(&[output()]),
            runtime: LiveProductionVisualRuntime::new(&[output()], None).unwrap(),
            scene: LiveProductionCpuScene::new(output().size),
        };
        let wake = sophia_wake::Wake::new().unwrap();
        lane.native.attach_owner_wake(&wake.notifier());
        // Attaching rings once (WakeSlot::set). Nothing has been submitted,
        // so that ring announces no result: clear it now, while the fixture
        // is empty, and never after an admission.
        wake.clear().unwrap();
        assert!(
            !wake.wait(Some(Instant::now())).unwrap(),
            "the attach ring is cleared before any submission"
        );
        (lane, wake)
    }

    /// REV3 5.1's fault, on `slot`'s row.
    fn declare_image_service_fault(&mut self, slot: usize, reason: &'static str) {
        self.h
            .owner
            .processes
            .combined_row_mut(slot)
            .declare_image_service_fault(reason);
    }

    /// Candidate 1 admitted and forwarded through hooked turns, then its
    /// render result's ring: the result waits uncollected, since no owner
    /// turn has run after the one that left the job in flight. The wake was
    /// emptied before submission and only this job publishes a result, so
    /// the ring is its publication; the wait does not clear it.
    fn rendered_uncollected(&mut self, wake: &sophia_wake::Wake, a: &mut Attempt) {
        self.submit(a, 1, PIXEL);
        let job = self.until(a, "in flight");
        assert!(
            wake.wait(Some(Instant::now() + WAIT)).unwrap(),
            "the forwarded frame's render result rings the owner"
        );
        assert_eq!(self.native.seen().len(), 1, "the seam read one occurrence");
        assert_eq!(self.states(), [(job, "in flight")], "not yet collected");
    }

    /// One controlled owner turn through the split driver. Slot 0 composes
    /// on the Shell layer with the plane cursor, as `turn` does; content is
    /// the bar's real content service with the row lent and the fixture as
    /// the native; native service is `service_native` when `serve_native`
    /// (a running fixture) and nothing otherwise. The driver's result, and
    /// the native service's errors, which are kept rather than raised.
    fn driver_turn(
        &mut self,
        steps: &mut Vec<Step>,
        serve_native: bool,
    ) -> (Result<FirstFailures>, Vec<String>) {
        let mut errors = Vec::new();
        let (turned, _) = self.driver_turn_with(steps, &mut |native| {
            if serve_native && let Err(error) = native.service_native(output().id) {
                errors.push(error.to_string());
            }
        });
        (turned, errors)
    }

    /// `driver_turn` with `step4` as step 4 (d-2b2); also the fixture's
    /// accept count read just after each content call, in call order.
    fn driver_turn_with(
        &mut self,
        steps: &mut Vec<Step>,
        step4: &mut dyn FnMut(&mut CombinedNativeFixture),
    ) -> (Result<FirstFailures>, Vec<usize>) {
        self.driver_turn_at(steps, Instant::now(), step4)
    }

    /// `driver_turn_with` with `instant` as the turn's one Instant (d-2b3a),
    /// for a walk past an admitted deadline. The four calls are unchanged
    /// and no job's deadline is touched.
    fn driver_turn_at(
        &mut self,
        steps: &mut Vec<Step>,
        instant: Instant,
        step4: &mut dyn FnMut(&mut CombinedNativeFixture),
    ) -> (Result<FirstFailures>, Vec<usize>) {
        let (turned, observed) = self.driver_turn_observed(steps, instant, None, step4);
        let accepted = observed.into_iter().map(|(accepts, _)| accepts).collect();
        (turned, accepted)
    }

    /// `driver_turn_at` with the hardware cursor plane at `plane` (d-2b3b1;
    /// `driver_turn_at` passes None, its default). Each content call is
    /// followed by an observation of the fixture's accept count and its
    /// (release attempts, Transferred receipts), read at that call's
    /// boundary before the walk. The read is no service, flush or schedule
    /// change: the four calls and their order are unchanged.
    fn driver_turn_observed(
        &mut self,
        steps: &mut Vec<Step>,
        instant: Instant,
        plane: Option<Point>,
        step4: &mut dyn FnMut(&mut CombinedNativeFixture),
    ) -> (Result<FirstFailures>, Vec<Observed>) {
        let outputs = [output()];
        let (cursor, plane) = turn_cursor(true, plane);
        let root = Rect {
            x: 0,
            y: 0,
            width: output().size.width,
            height: output().size.height,
        };
        let Lane {
            h,
            native,
            runtime,
            scene,
        } = self;
        let mut turn = DriverTurn {
            native,
            runtime,
            scene,
            outputs: &outputs,
            layers: std::array::from_fn(|slot| (slot == 0).then_some(LiveShellContentLayer::Shell)),
            cursor,
            plane,
            instant,
        };
        let mut accepted = Vec::new();
        let turned = h.owner.combined_owner_turn(
            &mut turn,
            &mut |owner, key, turn| {
                let served = owner.with_combined_service(key, |service, t, row| {
                    let ShellComponentService::Bar(bar) = service else {
                        panic!("bar required")
                    };
                    adopt_lane_facts(&mut bar.content, output());
                    let served = bar
                        .service_content_on(
                            t,
                            &mut *turn.runtime,
                            turn.scene,
                            ContentNative {
                                native: Some(&mut *turn.native),
                                combined: Some(CombinedConsume {
                                    row,
                                    cursor: turn.cursor,
                                    plane: turn.plane,
                                    instant: turn.instant,
                                }),
                            },
                            turn.outputs,
                            &[(output().id, root)],
                            root,
                        )
                        .map_err(|error| error.to_string());
                    t.poll_io().unwrap();
                    served
                });
                accepted.push((
                    turn.native.accepts(),
                    (
                        turn.native.release_attempts(),
                        turn.native.release_transfers(),
                    ),
                ));
                served.map_err(|error| error.to_string())?
            },
            step4,
            steps,
        );
        (turned, accepted)
    }

    /// A driver turn in which no slot fails and native service succeeds;
    /// its steps.
    fn quiet_driver_turn(&mut self, serve_native: bool) -> Vec<Step> {
        let mut steps = Vec::new();
        let (turned, errors) = self.driver_turn(&mut steps, serve_native);
        let first = turned.expect("the turn completes");
        assert!(first.iter().all(Option::is_none), "{first:?}");
        assert!(errors.is_empty(), "native service: {errors:?}");
        steps
    }

    /// Driver turns with `a`'s peer polled until slot 0 fails; that turn's
    /// first failures and steps.
    fn until_driver_failure(
        &mut self,
        a: &mut Attempt,
        serve_native: bool,
    ) -> (FirstFailures, Vec<Step>) {
        let deadline = Instant::now() + WAIT;
        loop {
            a.peer.poll_io().unwrap();
            let mut steps = Vec::new();
            let (turned, errors) = self.driver_turn(&mut steps, serve_native);
            assert!(errors.is_empty(), "native service: {errors:?}");
            let first = turned.expect("the turn completes");
            if first.iter().any(Option::is_some) {
                return (first, steps);
            }
            observe_peer(a);
            assert!(Instant::now() < deadline, "no failure: {:?}", self.states());
            std::thread::yield_now();
        }
    }

    /// Slot 0's recorded failures and next start.
    fn backoff(&self) -> (u32, Option<Instant>) {
        (self.h.owner.failures[0], self.h.owner.retry_at[0])
    }
}

/// REV3 5.1, running core, labelled fault (MD9). The result turn begins
/// with candidate 1's render result uncollected and the fault declared for
/// slot 0. The images step collects the result into Held, then fails; the
/// slot is stopped before content, so content never meets the job, and the
/// walk releases the ended grant's Held lease. Assertion 1 reads final facts
/// only; under MD9 the first failure is assertion 2.
#[test]
fn a_declared_image_service_failure_stops_the_slot_before_content_and_the_walk_releases_held() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.rendered_uncollected(&wake, &mut a);
    let (failures, _) = lane.backoff();
    lane.declare_image_service_fault(0, IMAGE_FAULT);
    let before = Instant::now();
    let mut steps = Vec::new();
    let (turned, errors) = lane.driver_turn(&mut steps, true);
    assert!(
        steps.contains(&FailService(0))
            && lane.h.owner.phase(a.key).unwrap() == ComponentConnectionPhase::Revoked,
        "5.1-1: fail_service ran for slot 0 and its grant is revoked: {steps:?}"
    );
    assert!(
        !steps.contains(&Content(0)) && lane.native.accepts() == 0,
        "5.1-2: no content entry on a slot whose image service failed: {steps:?}"
    );
    assert_eq!(lane.native.installs(), 0, "5.1-2: no ordinary install");
    let first = turned.expect("the turn completes");
    assert_eq!(
        steps,
        [Images, FailService(0), Walk, Native],
        "the slot failed in step 1; walk and native still ran"
    );
    assert_eq!(
        (
            lane.native.release_attempts(),
            lane.native.release_transfers()
        ),
        (1, 1),
        "5.1-3: the walk released the ended grant's Held lease: Transferred once"
    );
    assert!(lane.states().is_empty(), "5.1-3: the row is clear");
    assert_eq!(
        first,
        [
            Some(ShellTransportError::Io(IMAGE_FAULT.into()).to_string()),
            None,
            None
        ],
        "5.1-4: the first error kept is the declared fault"
    );
    let (recorded, retry) = lane.backoff();
    assert_eq!(recorded, failures + 1, "5.1-5: the failure is recorded");
    assert!(
        retry.is_some_and(|at| at > before),
        "5.1-5: the next start is spaced"
    );
    assert!(errors.is_empty(), "native service: {errors:?}");
}

/// REV2 R3's unclassified-error row, kept for the classification error on
/// the running core (REV3 5.1, last paragraph), labelled fault. Content
/// meets the Held job, the classification fails after the real
/// recomposition, and the arm releases before returning its first error,
/// which stops the slot in step 2. The running core transfers that Release
/// at once, so no Debt is required.
#[test]
fn an_unclassified_error_stops_the_component_and_backs_off() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.rendered_uncollected(&wake, &mut a);
    lane.native.declare_classify_fault(CLASSIFY_FAULT);
    let (failures, _) = lane.backoff();
    let before = Instant::now();
    let (first, steps) = lane.until_driver_failure(&mut a, true);
    assert_eq!(
        first,
        [Some(CLASSIFY_FAULT.to_string()), None, None],
        "the first error is the declared classification fault"
    );
    assert_eq!(
        steps,
        [Images, Content(0), FailService(0), Walk, Native],
        "the content error stopped the slot in step 2; walk and native still ran"
    );
    assert_eq!(lane.native.classifies(), 1);
    assert_eq!(
        (
            lane.native.release_attempts(),
            lane.native.release_transfers()
        ),
        (1, 1),
        "released once, and the running core transferred it"
    );
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked,
        "the grant is revoked"
    );
    let (recorded, retry) = lane.backoff();
    assert_eq!(recorded, failures + 1, "the failure is recorded");
    assert!(
        retry.is_some_and(|at| at > before),
        "the next start is spaced"
    );
}

/// REV3 5.2, stub core, labelled: a Full Release's debt through stop. The
/// renderer's command queue is filled, so the arm's Release cannot
/// transfer; the classification error then stops the slot through
/// `fail_service`. The row keeps the Debt across stop, close and every walk
/// while the queue stays Full, and final quiescence refuses it. Once the
/// queue drains, one walk flushes it. The running core makes no such claim.
#[test]
fn an_unclassified_error_with_a_full_release_keeps_debt_through_stop() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    lane.native.fill_renderer_queue();
    lane.native.declare_classify_fault(CLASSIFY_FAULT);
    let (first, steps) = lane.until_driver_failure(&mut a, false);
    assert_eq!(first, [Some(CLASSIFY_FAULT.to_string()), None, None]);
    assert_eq!(steps, [Images, Content(0), FailService(0), Walk, Native]);
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked,
        "fail_service stopped and closed the attempt"
    );
    assert_eq!(
        lane.states(),
        [(job, "debt")],
        "5.2: the Full Release left Debt through stop and close"
    );
    assert_eq!(lane.native.release_transfers(), 0);
    drop(a);
    let deadline = Instant::now() + WAIT;
    while lane.h.owner.processes.image_pending(0) != 0 {
        assert_eq!(lane.quiet_driver_turn(false), [Images, Walk, Native]);
        assert!(Instant::now() < deadline, "the image work did not end");
        std::thread::yield_now();
    }
    assert_eq!(
        lane.states(),
        [(job, "debt")],
        "5.2: every Full walk kept it"
    );
    let backend = std::mem::replace(
        &mut lane.h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    let Err(backend) = lane.h.owner.processes.finish_after_backend_drop(backend) else {
        panic!("5.2: final quiescence ignored the row's Release debt");
    };
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert_eq!(lane.quiet_driver_turn(false), [Images, Walk, Native]);
    assert!(lane.states().is_empty(), "5.2: the walk flushed the debt");
    assert_eq!(lane.native.release_transfers(), 1, "5.2: Transferred once");
    assert_eq!(
        lane.native.renderer_releases(),
        1,
        "5.2: the drained Release is counted once"
    );
    assert!(
        lane.h
            .owner
            .processes
            .finish_after_backend_drop(backend)
            .is_ok(),
        "with the debt transferred nothing refuses"
    );
}

// G3-5d d-2b2: the running core end to end through this driver.
#[path = "combined_running.rs"]
mod running;

/// The split driver's call order (MD4), stub core, labelled: candidate 1 is
/// Held and validated, and every turn until content accepts it runs the
/// images step, then content on the connected slot, then the walk, then
/// native service, the accepting turn included. It proves the driver's
/// actual call order only; the running result turn's order is d-2b2's
/// evidence.
#[test]
fn the_split_driver_serves_content_before_the_walk_and_native_last() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    let deadline = Instant::now() + WAIT;
    while lane.native.accepts() == 0 {
        a.peer.poll_io().unwrap();
        assert_eq!(
            lane.quiet_driver_turn(false),
            [Images, Content(0), Walk, Native],
            "MD4: content before the walk, native last"
        );
        observe_peer(&mut a);
        assert!(
            Instant::now() < deadline,
            "the Held result was never accepted: {:?}",
            lane.states()
        );
        std::thread::yield_now();
    }
    assert_eq!(lane.native.accepts(), 1, "content accepted the Held result");
}
