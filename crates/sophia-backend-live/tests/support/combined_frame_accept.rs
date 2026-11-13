#![cfg(test)]
//! G3-4 (GATE3-SCOPE, AUTHORITY-STEP3 F2 and R5): the native accept of a held
//! combined result, then the real `poll()`, export, preparation and tracked
//! submission of the generic runtime tick around one deterministic device,
//! then the head operations the owner runs on its report.
//!
//! A stub serves the real combined route; its exported render, carrying the
//! plan's own correlation, is the deterministic render boundary. Admission,
//! the validator's forward, settlement to Held, the accept, the facade's
//! in-flight reply and everything after are production code. Not run here:
//! the owner's tick wrapper (its scheduling, counters, cursor ride, pacing,
//! layout witness, retirement and `submitted_content`), the mirror tick, real
//! page flips, KMS, the GPU and timing. This is not full-owner integration.

use super::super::accept_device as device;

use super::*;
use crate::{
    LiveRendererWorkerOutputKey, LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport,
    LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status, StubRenderer,
};
use device::{CountingDevice, Refuse};
use sophia_engine::{HeadlessCompositorBackendAssembly, HeadlessOutput};
use sophia_renderer_live::{
    LiveCompositionTrace, LiveOwnedMixedCompositionFrame, LiveRendererImportHealth,
    LiveRendererImportPathStatus, LiveRendererRuntimeObservation, LiveRendererSelectionObservation,
};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
/// The admitted native frame, and the content and checksum it carries.
const FRAME: u64 = 41;
const LEASE: u64 = 81;

#[cfg(feature = "experimental-shell-capture")]
#[path = "combined_frame_accept_capture.rs"]
mod capture;
type Validator = IsbValidationWorker<CombinedRenderPlan>;
use super::super::super::composition_queue::{
    DeferredNativeCompositions as Deferred, LiveProductionQueuedMirrorGeneration,
    LiveProductionQueuedMirrorHeadFrame,
};

struct NoDevice;

impl RenderDeviceDiscoveryBackend for NoDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> io::Result<std::fs::File> {
        Err(io::Error::other("no render device in this test"))
    }
}

fn key(raw: u64) -> LiveRendererWorkerOutputKey {
    LiveRendererWorkerOutputKey::from_raw(raw)
}

fn exporter(core: &Arc<crate::NativeGbmRendererWorkerCore>, raw: u64) -> Exporter<NoDevice> {
    let mut exporter = Exporter::new(NoDevice);
    exporter.set_output(key(raw));
    exporter.attach_shared_worker(core);
    exporter
}

/// A head on `output`, sized as the runtime's deterministic output.
fn head(id: u32, output: u64) -> LiveProductionNativeHead {
    let mut head = super::super::super::native_head_fixture::head(
        id,
        output,
        crate::LibdrmNativePlaneFormatCapabilities::parse(201, 0, &[]),
    );
    head.output = HeadlessOutput {
        id: OutputId::from_raw(output),
        ..HeadlessOutput::deterministic()
    };
    head.output_frames = OutputFramePresentationState::new(head.output).unwrap();
    head
}

fn content(frame: u64) -> LiveProductionScanoutContent {
    LiveProductionScanoutContent::HeadComposition {
        frame: LiveProductionNativeFrameId::from_raw(frame),
        logical_content_checksum: frame,
        nonzero_rgb_pixels: 0,
    }
}

/// The content a combined shell frame must install, written out here so the
/// expectation does not follow the production constructor.
fn shell(frame: u64) -> LiveProductionScanoutContent {
    LiveProductionScanoutContent::RetainedMixed {
        frame: LiveProductionNativeFrameId::from_raw(frame),
        requires_retirement: true,
        logical_content_checksum: Some(frame),
        nonzero_rgb_pixels: 0,
    }
}

fn snapshot(output: HeadlessOutput) -> sophia_engine::OutputFrameDamageSnapshot {
    sophia_engine::OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output,
        surfaces: Vec::new(),
        compositor_display_list: sophia_engine::CompositorDamageList::empty(output.id),
        software_cursor: None,
    }
}

fn mixed(head: &LiveProductionNativeHead) -> LiveOwnedMixedCompositionFrame {
    LiveOwnedMixedCompositionFrame {
        trace: Some(LiveCompositionTrace {
            output: head.output.id,
            head: head.head,
            scene_generation: FRAME,
        }),
        ..Default::default()
    }
}

fn identity(
    owner: crate::NativeFrameOwner,
    head: &LiveProductionNativeHead,
) -> crate::LiveNativeFrameIdentity {
    owner.frame(head.output.id, head.head, head.target_generation, FRAME)
}

/// The correlation the plan carries, as the owner records it at dispatch.
fn expected(
    owner: crate::NativeFrameOwner,
    head: &LiveProductionNativeHead,
) -> crate::LiveRendererFrameCorrelation {
    let frame = mixed(head);
    crate::LiveRendererFrameCorrelation {
        native: Some(identity(owner, head)),
        request: Some(crate::COMBINED_REQUEST),
        trace: frame.trace,
        direct_scanout: Some(frame.direct_scanout),
    }
}

fn pixels() -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "combined-accept",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(&[0; 8]).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

/// Heads on outputs 1 and 2 with their facades on one stub-served core, and
/// output 1's combined result settled to Held.
struct Fixture {
    native_owner: crate::NativeFrameOwner,
    heads: Vec<LiveProductionNativeHead>,
    exporters: Vec<Exporter<NoDevice>>,
    core: Arc<crate::NativeGbmRendererWorkerCore>,
    stub: StubRenderer,
    _validator: Validator,
    runtime: crate::LiveBackendRuntimeAssembly,
    deferred: Deferred,
    /// The owner's capture endpoint as the guards read it: none unless a
    /// control installs a real owner (G3-5d d-0).
    capture: Box<dyn Fn(OutputId) -> bool>,
    combined: CombinedHead,
    job: u64,
}

impl Fixture {
    fn held() -> Self {
        let (core, mut stub) = crate::stub_core();
        let native_owner = crate::NativeFrameOwner::new();
        let mut heads = vec![head(1, 1), head(2, 2)];
        let mut exporters = vec![exporter(&core, 1), exporter(&core, 2)];
        let mut routes = CombinedRoutes {
            native_owner,
            heads: &mut heads,
            exporters: &mut exporters,
        };
        let (combined, inbox) = routes
            .reserve(OutputId::from_raw(1))
            .expect("the serving facade reserves");
        stub.register(key(1), &combined.facade);
        let mut validator = Validator::for_combined_plans(1, 8).unwrap();
        let reservation = validator
            .reserve_combined(8, Instant::now() + WAIT)
            .unwrap();
        let plan = CombinedRenderPlan::new(
            Instant::now() + WAIT,
            crate::api::LiveGbmEglFrameTargetRecord::new(routes.heads[0].output.size),
            crate::PendingRenderedFrame::Mixed(
                mixed(&routes.heads[0]),
                Some(identity(native_owner, &routes.heads[0])),
            ),
            Vec::new(),
            None,
            Box::new([]),
        );
        let _owner_plan = validator
            .submit_combined(&reservation, pixels(), plan, inbox)
            .ok()
            .expect("combined submission");
        stub.serve_exported(key(1), routes.heads[0].output.size, LEASE);
        let end = Instant::now() + WAIT;
        let result = loop {
            if let Some(result) = routes.take(&combined).expect("the issuer answers") {
                break result;
            }
            assert!(Instant::now() < end, "the stub's frame did not come home");
            std::thread::sleep(Duration::from_millis(1));
        };
        let job = reservation.job();
        assert!(
            routes
                .settle(&combined, &mut validator, reservation, result)
                .is_ok(),
            "the frame's own reservation settles to Held"
        );
        let output = heads[0].output;
        let runtime = crate::LiveBackendRuntimeAssembly::from_ready_headless_scanout(
            HeadlessCompositorBackendAssembly::new(output),
            output,
            LiveRendererRuntimeObservation {
                health: LiveRendererImportHealth::CpuFallback,
                xpixmap: LiveRendererImportPathStatus::Disabled,
                dmabuf: LiveRendererImportPathStatus::Disabled,
                selection: LiveRendererSelectionObservation::CpuFallback,
            },
        );
        Self {
            native_owner,
            heads,
            exporters,
            core,
            stub,
            _validator: validator,
            runtime,
            deferred: Deferred::default(),
            capture: Box::new(|_| false),
            combined,
            job,
        }
    }

    /// What the owner froze at dispatch for the held result.
    fn accept(&self) -> CombinedAccept {
        CombinedAccept {
            job: self.job,
            frame: LiveProductionNativeFrameId::from_raw(FRAME),
            checksum: FRAME,
            snapshot: snapshot(self.heads[0].output),
            correlation: expected(self.native_owner, &self.heads[0]),
            output_format: None,
        }
    }

    fn preflight(
        &self,
        accept: CombinedAccept,
    ) -> Result<CombinedAcceptPlan, CombinedAcceptRefusal> {
        preflight_accept(
            self.native_owner,
            &self.heads,
            &self.exporters,
            &self.deferred,
            &*self.capture,
            &self.combined,
            accept,
        )
    }

    /// The consumption guards alone, as G3-5c's check runs them before any
    /// classification.
    fn check(&self, accept: &CombinedAccept) -> Result<usize, CombinedAcceptRefusal> {
        combined_consumption_guards(
            self.native_owner,
            &self.heads,
            &self.exporters,
            &self.deferred,
            &*self.capture,
            &self.combined,
            accept,
        )
    }

    fn commit(&mut self, plan: CombinedAcceptPlan) {
        let Self {
            native_owner,
            heads,
            exporters,
            combined,
            ..
        } = self;
        CombinedRoutes {
            native_owner: *native_owner,
            heads,
            exporters,
        }
        .commit_accept(combined, plan);
    }

    fn accepted() -> Self {
        let mut fixture = Self::held();
        let plan = fixture
            .preflight(fixture.accept())
            .expect("the held result is acceptable");
        fixture.commit(plan);
        fixture
    }

    fn release(&mut self) -> CombinedRelease {
        let Self {
            native_owner,
            heads,
            exporters,
            combined,
            job,
            ..
        } = self;
        CombinedRoutes {
            native_owner: *native_owner,
            heads,
            exporters,
        }
        .release_held(combined, *job)
    }

    /// Queues on head `index`'s output an earlier generation owing a
    /// retirement, as mirror deferral leaves one before its service runs.
    fn queue_protected(&mut self, index: usize) -> LiveProductionNativeFrameId {
        let head = &self.heads[index];
        let frame = LiveProductionNativeFrameId::from_raw(FRAME - 1);
        let generation = LiveProductionQueuedMirrorGeneration {
            output: head.output.id,
            frame,
            logical_content_checksum: Some(frame.raw()),
            heads: vec![LiveProductionQueuedMirrorHeadFrame {
                capture_reservation: None,
                head_index: index,
                identity: self.native_owner.frame(
                    head.output.id,
                    head.head,
                    head.target_generation,
                    frame.raw(),
                ),
                content: shell(frame.raw()),
                frame: Default::default(),
                output_damage_snapshot: None,
                cpu_nonzero_pixel_bytes: 0,
            }],
        };
        let configured = self.heads.iter().map(|head| head.output.id).collect();
        assert!(
            self.deferred
                .admit_batch(vec![generation], &configured)
                .is_ok()
        );
        assert!(self.deferred.protected(self.heads[index].output.id));
        frame
    }

    fn quiescence(&self) -> Option<&'static str> {
        super::super::super::topology::heads_quiescence_blocker(&self.heads, &self.exporters)
    }

    /// The generic runtime tick on output 1's real exporter, with the
    /// owner's renderer-content advance around it. `worker_was_in_flight`
    /// is read from the exporter before the tick, as the owner reads it.
    fn tick(
        &mut self,
        device: &CountingDevice,
    ) -> (LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport, bool) {
        let worker_was_in_flight = self.exporters[0].worker_in_flight();
        let report = self
            .runtime
            .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
                sophia_engine::CompositorBackendTickInput::default(),
                device,
                &mut self.exporters[0],
            )
            .expect("the runtime tick runs");
        let worker_is_in_flight = self.exporters[0].worker_in_flight();
        let head = &mut self.heads[0];
        assert_eq!(
            advance_live_production_renderer_content(
                worker_was_in_flight,
                worker_is_in_flight,
                &mut head.pending_content,
                &mut head.rendering_content,
            ),
            Ok(false),
            "an accepted render starts no ordinary one"
        );
        let submit = report
            .rendered_primary_plane_scanout_submit
            .expect("the tick reached tracked submission");
        (submit, worker_was_in_flight)
    }

    /// The head, output frame and exporter state a refusal must not move.
    fn observed(&self) -> Observed {
        let head = &self.heads[0];
        (
            [
                head.pending_content,
                head.rendering_content,
                head.submitted_content,
            ],
            head.last_checksum,
            head.output_frames.clone(),
            self.exporters[0].worker_in_flight(),
            self.exporters[0].pending_frame(),
        )
    }
}

type Observed = (
    [Option<LiveProductionScanoutContent>; 3],
    u64,
    OutputFramePresentationState,
    bool,
    bool,
);

#[test]
fn an_accepted_render_reaches_submitted_waiting_for_page_flip() {
    let mut fixture = Fixture::held();
    let output = fixture.heads[0].output;
    let plan = fixture
        .preflight(fixture.accept())
        .expect("the held result is acceptable");
    fixture.commit(plan);
    // F2's writes 2 to 4.
    let head = &fixture.heads[0];
    assert!(
        head.retirement_protected(),
        "the accepted content owes the shell retirement"
    );
    assert_eq!(head.rendering_content, Some(shell(FRAME)));
    assert_eq!(head.last_checksum, FRAME);
    assert_eq!(head.pending_content, None);
    assert!(head.output_frames.pending().is_none());
    assert_eq!(
        head.output_frames.rendering().map(|frame| &frame.snapshot),
        Some(&snapshot(output)),
        "the plan's snapshot is the rendering frame"
    );
    assert!(fixture.exporters[0].worker_in_flight());
    assert!(fixture.exporters[0].pending_frame());

    let device = CountingDevice::new(Refuse::Nothing);
    let (submit, worker_was_in_flight) = fixture.tick(&device);
    assert!(worker_was_in_flight);
    assert_eq!(submit.status, Status::SubmittedWaitingForPageFlip);
    assert_eq!(
        device.counts(),
        (1, 1, 0),
        "one framebuffer and one real commit, through the device"
    );
    assert!(
        !fixture.exporters[0].worker_in_flight(),
        "poll took the reply"
    );
    let captured = fixture
        .runtime
        .submitted_rendered_frame_correlation(output.id);
    assert_eq!(
        captured,
        Some(expected(fixture.native_owner, &fixture.heads[0])),
        "custody holds the admitted identity under the combined request"
    );
    let captured = captured.and_then(|correlation| correlation.native);

    // The owner's shared identity check, its negatives first.
    let head = &mut fixture.heads[0];
    assert!(
        head.accepted_native_frame(None, fixture.native_owner, output.id)
            .is_err(),
        "a submission without a captured identity is refused"
    );
    assert!(
        head.accepted_native_frame(captured, crate::NativeFrameOwner::new(), output.id)
            .is_err(),
        "another owner's captured identity is refused"
    );
    head.target_generation += 1;
    assert!(
        head.accepted_native_frame(captured, fixture.native_owner, output.id)
            .is_err(),
        "another target's captured identity is refused"
    );
    head.target_generation -= 1;
    head.head = sophia_engine::RenderHeadId::from_raw(7);
    assert!(
        head.accepted_native_frame(captured, fixture.native_owner, output.id)
            .is_err(),
        "another head's captured identity is refused"
    );
    head.head = sophia_engine::RenderHeadId::from_raw(1);
    let native_frame = head
        .accepted_native_frame(captured, fixture.native_owner, output.id)
        .expect("the captured identity is this owner's, head's and target's");
    assert_eq!(native_frame, LiveProductionNativeFrameId::from_raw(FRAME));
    let nonzero = fixture.exporters[0].composition_nonzero_rgb_pixels();
    let head = &mut fixture.heads[0];
    let taken = head.take_accepted_content(native_frame, nonzero);
    assert_eq!(
        taken,
        Ok(Some(shell(FRAME).with_nonzero_rgb_pixels(nonzero))),
        "the accepted content is the rendering one"
    );
    assert!(
        taken.unwrap().unwrap().requires_retirement(),
        "the taken content still owes the shell retirement"
    );
    assert_eq!(head.rendering_content, None);
    assert!(head.output_frames.rendering().is_none());
    assert_eq!(
        head.output_frames.submitted().map(|frame| &frame.snapshot),
        Some(&snapshot(output)),
        "rendering promoted to submitted"
    );
    assert_eq!(
        fixture.stub.releases(),
        0,
        "custody holds the lease; no ordinary render was queued"
    );
}

/// The owner's failure arm after a failed tick: the rendering content is
/// taken and its output frame discarded, since the worker was in flight.
fn failed(
    refuse: Refuse,
) -> (
    LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport,
    CountingDevice,
    Fixture,
) {
    let mut fixture = Fixture::accepted();
    let device = CountingDevice::new(refuse);
    let (submit, worker_was_in_flight) = fixture.tick(&device);
    assert!(
        worker_was_in_flight,
        "read from the exporter before the tick"
    );
    assert_eq!(submit.status, Status::PrimaryPlaneSubmitFailed);
    let head = &mut fixture.heads[0];
    assert_eq!(
        head.take_failed_content(worker_was_in_flight),
        Some(shell(FRAME)),
        "the failed content is the rendering one"
    );
    assert_eq!(head.rendering_content, None);
    assert!(
        head.output_frames.rendering().is_none(),
        "the failed render's frame is discarded"
    );
    assert!(head.output_frames.submitted().is_none());
    assert!(!fixture.exporters[0].worker_in_flight());
    let _ = fixture.stub.releases();
    (submit, device, fixture)
}

#[test]
fn a_preparation_failure_takes_rendering_content_and_discards_rendering() {
    let (submit, device, _fixture) = failed(Refuse::Framebuffer);
    let (framebuffers, commits, test_commits) = device.counts();
    assert!(framebuffers >= 1, "preparation asked the device");
    assert_eq!((commits, test_commits), (0, 0), "nothing was committed");
    assert_eq!(submit.commit_submit, None);
}

#[test]
fn a_definitive_submission_failure_takes_rendering_content() {
    let (submit, device, _fixture) = failed(Refuse::Commit);
    assert_eq!(device.counts(), (1, 1, 0));
    assert_eq!(
        submit.commit_submit,
        Some(crate::LibdrmNativeAtomicCommitSubmitStatus::Rejected)
    );
}

/// One refused case: the preflight answers `refusal` and moves nothing, and
/// the held result is still there for ordinary disposal.
fn refused(
    name: &str,
    refusal: CombinedAcceptRefusal,
    change: impl FnOnce(&mut Fixture, &mut CombinedAccept),
) -> Fixture {
    let mut fixture = Fixture::held();
    let mut accept = fixture.accept();
    change(&mut fixture, &mut accept);
    let before = fixture.observed();
    assert_eq!(fixture.preflight(accept).err(), Some(refusal), "{name}");
    assert!(fixture.observed() == before, "{name}: nothing moved");
    assert_eq!(fixture.stub.releases(), 0, "{name}: nothing released");
    assert_eq!(
        fixture.release(),
        CombinedRelease::Transferred,
        "{name}: the held result stays for disposal"
    );
    assert_eq!(fixture.stub.releases(), 1);
    fixture
}

#[test]
fn each_refused_preflight_changes_no_state() {
    use CombinedAcceptRefusal as R;
    let another_size = sophia_protocol::Size {
        width: 640,
        height: 480,
    };
    refused("stale job", R::NotHeld, |_, accept| accept.job += 1);
    refused("disabled head", R::Topology, |f, _| {
        f.heads[0].enabled = false
    });
    refused("remapped head", R::Topology, |f, _| {
        f.heads[0].output.id = OutputId::from_raw(9);
    });
    refused("mirrored output", R::Topology, |f, _| {
        f.heads[1].output = f.heads[0].output;
    });
    refused("stale target generation", R::TargetGeneration, |f, _| {
        f.heads[0].target_generation += 1;
    });
    refused("head pending content", R::HeadPendingContent, |f, _| {
        f.heads[0].pending_content = Some(content(99));
    });
    refused("head rendering content", R::HeadRenderingContent, |f, _| {
        f.heads[0].rendering_content = Some(content(99));
    });
    refused("retirement owed", R::RetirementProtected, |f, _| {
        f.heads[0].submitted_content = Some(LiveProductionScanoutContent::RetainedMixed {
            frame: LiveProductionNativeFrameId::from_raw(98),
            requires_retirement: true,
            logical_content_checksum: None,
            nonzero_rgb_pixels: 0,
        });
    });
    refused("no combined request", R::Identity, |_, accept| {
        accept.correlation.request = None;
    });
    refused("another owner's identity", R::Identity, |f, accept| {
        accept.correlation.native = Some(identity(crate::NativeFrameOwner::new(), &f.heads[0]));
    });
    refused("another frame's identity", R::Identity, |_, accept| {
        accept.frame = LiveProductionNativeFrameId::from_raw(FRAME + 1);
    });
    refused("pending output frame", R::PendingFrame, |f, _| {
        let output = f.heads[0].output;
        f.heads[0].output_frames.queue(snapshot(output)).unwrap();
    });
    refused("rendering output frame", R::RenderingTransition, |f, _| {
        let output = f.heads[0].output;
        f.heads[0].output_frames.queue(snapshot(output)).unwrap();
        f.heads[0].output_frames.mark_rendering().unwrap();
    });
    refused(
        "snapshot of another output",
        R::PendingTransition,
        |_, accept| {
            accept.snapshot.output.size = another_size;
        },
    );
}

#[test]
fn a_protected_queued_generation_refuses_only_its_own_output() {
    let mut queued = None;
    let fixture = refused(
        "a protected queued generation",
        CombinedAcceptRefusal::RetirementProtected,
        |f, _| queued = Some(f.queue_protected(0)),
    );
    let output = fixture.heads[0].output.id;
    assert!(
        fixture.deferred.protected(output),
        "the queue keeps its retirement"
    );
    assert_eq!(
        fixture
            .deferred
            .get(output)
            .map(|generation| generation.frame),
        queued,
        "the queued generation is unchanged"
    );

    let mut fixture = Fixture::held();
    fixture.queue_protected(1);
    assert!(
        fixture.preflight(fixture.accept()).is_ok(),
        "another output's queued retirement does not refuse this one"
    );
}

#[test]
fn only_the_issuing_owner_head_and_facade_can_accept() {
    let mut fixture = Fixture::held();
    let before = fixture.observed();
    let issuing = fixture.native_owner;
    fixture.native_owner = crate::NativeFrameOwner::new();
    assert_eq!(
        fixture.preflight(fixture.accept()).err(),
        Some(CombinedAcceptRefusal::NotIssuing),
        "a rebuilt native owner is not the issuer"
    );
    fixture.native_owner = issuing;
    fixture.heads[0].head = sophia_engine::RenderHeadId::from_raw(7);
    assert_eq!(
        fixture.preflight(fixture.accept()).err(),
        Some(CombinedAcceptRefusal::NotIssuing),
        "another head is not the issuer"
    );
    fixture.heads[0].head = sophia_engine::RenderHeadId::from_raw(1);
    let replacement = exporter(&fixture.core, 1);
    let original = std::mem::replace(&mut fixture.exporters[0], replacement);
    assert_eq!(
        fixture.preflight(fixture.accept()).err(),
        Some(CombinedAcceptRefusal::NotIssuing),
        "a replacement facade under the same key is not the issuer"
    );
    fixture.exporters[0] = original;
    assert!(fixture.observed() == before, "nothing moved");
    assert_eq!(fixture.release(), CombinedRelease::Transferred);
}

#[test]
fn an_ordinary_frame_queued_during_validation_declines_as_head_busy() {
    refused(
        "an ordinary frame queued during validation",
        CombinedAcceptRefusal::OrdinaryPending,
        |f, _| {
            let frame = mixed(&f.heads[0]);
            f.exporters[0].set_pending_mixed_frame(frame);
        },
    );
}

#[test]
fn an_accepted_combined_render_blocks_a_topology_install() {
    let held = Fixture::held();
    assert_eq!(held.quiescence(), None, "a held result is not in flight");
    let mut fixture = Fixture::accepted();
    assert_eq!(fixture.quiescence(), Some("head_rendering_content"));
    let rendering = fixture.heads[0].rendering_content.take();
    assert_eq!(
        fixture.quiescence(),
        Some("exporter_worker_in_flight"),
        "the accepted render in flight blocks topology"
    );
    fixture.heads[0].rendering_content = rendering;
}
