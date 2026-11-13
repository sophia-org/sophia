//! G3-5a controls. The fixture is one native authority (G3-5 design review
//! DR1): ordinary and combined frames share one counter, an accepted combined
//! retirement protects the output from ordinary replacement, and a protected
//! ordinary generation refuses combined acceptance. Then G3-4's success path
//! through the bridge's entries on this authority. The render is G3-4's
//! injected exported reply, not G3-5d's running renderer.
#[path = "combined_frame_accept_device.rs"]
mod device;

use super::*;
use crate::{
    LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status, ShellContentNative, StubRenderer,
};
use device::{CountingDevice, Refuse};
use sophia_engine::{HeadlessCompositorBackendAssembly, HeadlessOutput, OutputFrameDamageSnapshot};
use sophia_renderer_live::{
    LiveCompositionTrace, LiveOwnedMixedCompositionFrame, LiveRendererImportHealth,
    LiveRendererImportPathStatus, LiveRendererRuntimeObservation, LiveRendererSelectionObservation,
};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
const LEASE: u64 = 81;
type Validator = IsbValidationWorker<CombinedRenderPlan>;

fn outputs() -> [HeadlessOutput; 2] {
    [1, 2].map(|raw| HeadlessOutput {
        id: OutputId::from_raw(raw),
        ..HeadlessOutput::deterministic()
    })
}

fn snapshot(output: HeadlessOutput) -> OutputFrameDamageSnapshot {
    OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output,
        surfaces: Vec::new(),
        compositor_display_list: sophia_engine::CompositorDamageList::empty(output.id),
        software_cursor: None,
    }
}

fn pixels() -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "combined-native-fixture",
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

/// One ordinary head frame for `output` at its native target.
fn ordinary(
    fixture: &CombinedNativeFixture,
    output: OutputId,
    checksum: u64,
) -> (OutputId, Vec<crate::LiveProductionHeadCompositionFrame>) {
    let targets = fixture.head_targets(output);
    assert_eq!(targets.len(), 1, "one render target per fixture output");
    let target = targets[0];
    let frame = crate::LiveProductionHeadCompositionFrame {
        head: target.head,
        scene_generation: 1,
        target_generation: target.target_generation,
        mapping: target.mapping,
        logical_content_checksum: checksum,
        frame: LiveOwnedMixedCompositionFrame {
            output_damage_snapshot: Some(snapshot(HeadlessOutput {
                id: output,
                size: target.native_size,
                scale: target.scale,
            })),
            ..Default::default()
        },
    };
    (output, vec![frame])
}

/// A combined result on head 0's output, settled to Held through the
/// bridge's entries, with its frame drawn from the fixture's counter.
struct Held {
    fixture: CombinedNativeFixture,
    stub: StubRenderer,
    _validator: Validator,
    combined: CombinedHead,
    job: u64,
    frame: LiveProductionNativeFrameId,
    mixed: LiveOwnedMixedCompositionFrame,
}

fn held() -> Held {
    let (core, mut stub) = crate::stub_core();
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&core, &outputs);
    let output = outputs[0];
    let frame = ShellContentNative::allocate_combined_frame(&mut fixture);
    let (combined, inbox) = ShellContentNative::reserve_combined_frame(&mut fixture, output.id)
        .expect("the serving facade reserves");
    let key = crate::LiveRendererWorkerOutputKey::from_raw(output.id.raw());
    stub.register(key, &combined.facade);
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = validator
        .reserve_combined(8, Instant::now() + WAIT)
        .unwrap();
    let head = &fixture.heads[0];
    let mixed = LiveOwnedMixedCompositionFrame {
        trace: Some(LiveCompositionTrace {
            output: output.id,
            head: head.head,
            scene_generation: frame.raw(),
        }),
        ..Default::default()
    };
    let identity =
        fixture
            .native_owner
            .frame(output.id, head.head, head.target_generation, frame.raw());
    let plan = CombinedRenderPlan::new(
        Instant::now() + WAIT,
        crate::api::LiveGbmEglFrameTargetRecord::new(output.size),
        crate::PendingRenderedFrame::Mixed(mixed.clone(), Some(identity)),
        Vec::new(),
        None,
        Box::new([]),
    );
    let _owner_plan = validator
        .submit_combined(&reservation, pixels(), plan, inbox)
        .ok()
        .expect("combined submission");
    stub.serve_exported(key, output.size, LEASE);
    let end = Instant::now() + WAIT;
    let result = loop {
        if let Some(result) =
            ShellContentNative::take_combined_frame_result(&mut fixture, &combined)
                .expect("the issuer answers")
        {
            break result;
        }
        assert!(Instant::now() < end, "the stub's frame did not come home");
        std::thread::sleep(Duration::from_millis(1));
    };
    let job = reservation.job();
    assert!(
        ShellContentNative::settle_combined_frame(
            &mut fixture,
            &combined,
            &mut validator,
            reservation,
            result
        )
        .is_ok(),
        "the frame's own reservation settles to Held"
    );
    Held {
        fixture,
        stub,
        _validator: validator,
        combined,
        job,
        frame,
        mixed,
    }
}

impl Held {
    fn output(&self) -> HeadlessOutput {
        self.fixture.heads[0].output
    }

    fn accept(&self) -> CombinedAccept {
        let head = &self.fixture.heads[0];
        let output = head.output;
        CombinedAccept {
            job: self.job,
            frame: self.frame,
            checksum: self.frame.raw(),
            snapshot: snapshot(output),
            correlation: crate::LiveRendererFrameCorrelation {
                native: Some(self.fixture.native_owner.frame(
                    output.id,
                    head.head,
                    head.target_generation,
                    self.frame.raw(),
                )),
                request: Some(crate::COMBINED_REQUEST),
                trace: self.mixed.trace,
                direct_scanout: Some(self.mixed.direct_scanout),
            },
            output_format: None,
        }
    }

    fn accepted(mut self) -> Self {
        let plan = CombinedAcceptNative::preflight_combined_accept(
            &self.fixture,
            &self.combined,
            self.accept(),
        )
        .expect("the held result is acceptable");
        CombinedAcceptNative::commit_combined_accept(&mut self.fixture, &self.combined, plan);
        self
    }
}

#[test]
fn combined_and_ordinary_frames_draw_from_one_counter() {
    let (core, _stub) = crate::stub_core();
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&core, &outputs);
    let output = outputs[1].id;
    let first = ShellContentNative::allocate_combined_frame(&mut fixture);
    let queued = fixture
        .queue_ordinary(vec![ordinary(&fixture, output, 7)])
        .expect("ordinary work admits on an idle output");
    let between = queued[&output];
    assert_eq!(
        between.raw(),
        first.raw() + 1,
        "ordinary work takes the next frame of the same counter"
    );
    let second = ShellContentNative::allocate_combined_frame(&mut fixture);
    assert_eq!(second.raw(), between.raw() + 1);
    let head = &fixture.heads[1];
    let identities = [first, between, second].map(|frame| {
        fixture
            .native_owner
            .frame(output, head.head, head.target_generation, frame.raw())
    });
    assert!(
        identities[0] != identities[1]
            && identities[1] != identities[2]
            && identities[0] != identities[2],
        "combined then ordinary identities are distinct under one owner, head and target"
    );
    assert_eq!(
        fixture
            .deferred
            .get(output)
            .map(|generation| generation.frame),
        Some(between)
    );
}

#[test]
fn an_accepted_combined_retirement_blocks_an_ordinary_replacement() {
    let mut held = held().accepted();
    let output = held.output().id;
    assert!(
        held.fixture.protected(output),
        "the accepted shell content owes a retirement"
    );
    let next = held.fixture.next_frame;
    let required = held.fixture.queue_retained(
        vec![ordinary(&held.fixture, output, 9)],
        &BTreeSet::from([output]),
    );
    assert_eq!(
        required.err().map(|error| error.to_string()).as_deref(),
        Some("composition output already owns a distinct retirement"),
        "a second required retirement is refused by the batch rule"
    );
    let latest = held
        .fixture
        .queue_retained(vec![ordinary(&held.fixture, output, 9)], &BTreeSet::new())
        .expect("latest-scene work is not an error");
    assert!(
        latest.is_empty(),
        "a protected output defers ordinary replacement without a new frame"
    );
    assert!(!held.fixture.deferred.pending(output));
    assert_eq!(held.fixture.next_frame, next, "no frame was allocated");
}

#[test]
fn a_protected_ordinary_generation_blocks_combined_acceptance() {
    let mut held = held();
    let output = held.output().id;
    let queued = held
        .fixture
        .queue_retained(
            vec![ordinary(&held.fixture, output, 9)],
            &BTreeSet::from([output]),
        )
        .expect("a required retirement queues on an idle output");
    assert!(held.fixture.deferred.protected(output));
    assert_eq!(
        CombinedAcceptNative::preflight_combined_accept(
            &held.fixture,
            &held.combined,
            held.accept()
        )
        .err(),
        Some(crate::CombinedAcceptRefusal::RetirementProtected),
        "the ordinary path's queued retirement refuses the combined accept"
    );
    assert_eq!(
        held.fixture
            .deferred
            .get(output)
            .map(|generation| generation.frame),
        queued.get(&output).copied(),
        "the queued generation stays"
    );
    assert_eq!(
        ShellContentNative::release_combined_held(&mut held.fixture, &held.combined, held.job),
        CombinedRelease::Transferred,
        "the held result stays for ordinary disposal"
    );
    assert_eq!(held.stub.releases(), 1);
}

#[test]
fn the_bridge_reaches_submitted_waiting_for_page_flip() {
    let mut held = held().accepted();
    let output = held.output();
    let mut runtime = crate::LiveBackendRuntimeAssembly::from_ready_headless_scanout(
        HeadlessCompositorBackendAssembly::new(output),
        output,
        LiveRendererRuntimeObservation {
            health: LiveRendererImportHealth::CpuFallback,
            xpixmap: LiveRendererImportPathStatus::Disabled,
            dmabuf: LiveRendererImportPathStatus::Disabled,
            selection: LiveRendererSelectionObservation::CpuFallback,
        },
    );
    let device = CountingDevice::new(Refuse::Nothing);
    assert!(held.fixture.exporters[0].worker_in_flight());
    let report = runtime
        .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut held.fixture.exporters[0],
        )
        .expect("the runtime tick runs");
    let submit = report
        .rendered_primary_plane_scanout_submit
        .expect("the tick reached tracked submission");
    assert_eq!(submit.status, Status::SubmittedWaitingForPageFlip);
    assert_eq!(device.counts(), (1, 1, 0));
    let captured = runtime
        .submitted_rendered_frame_correlation(output.id)
        .and_then(|correlation| correlation.native);
    assert_eq!(
        held.fixture.heads[0].accepted_native_frame(captured, held.fixture.native_owner, output.id),
        Ok(held.frame),
        "custody holds the frame drawn from the fixture's counter"
    );
}
