#![cfg(test)]
//! G3-5a controls. The fixture is one native authority (G3-5 design review
//! DR1): ordinary and combined frames share one counter, an accepted combined
//! retirement protects the output from ordinary replacement, and a protected
//! ordinary generation refuses combined acceptance. Then the bridge's public
//! accept, `ShellContentNative::accept_combined_shell_content`, with a real
//! visual runtime and Shell candidate: its success to tracked submission and
//! a returned refusal. The render is G3-4's injected exported reply, except
//! in the last control, where G3-5d d-1's running worker renders through its
//! deterministic seam.
use super::super::accept_device as device;

use super::*;
use crate::production_visual_runtime::lifecycle_tests::resources::{
    BindingEvents, installed_shell, shell_frame, upload,
};
use crate::{
    CombinedAcceptError, CombinedAcceptFailed, CombinedAcceptRefusal, CombinedDisposition,
    LiveProductionVisualRuntime, LiveShellContentFrame, LiveShellContentLayer,
};
use crate::{LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status, ShellContentNative};
use device::{CountingDevice, Refuse};
use sophia_engine::{HeadlessCompositorBackendAssembly, HeadlessOutput, OutputFrameDamageSnapshot};
use sophia_renderer_live::{
    LiveCompositionTrace, LiveOwnedMixedCompositionFrame, LiveRendererImportHealth,
    LiveRendererImportPathStatus, LiveRendererRuntimeObservation, LiveRendererSelectionObservation,
};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
const LEASE: u64 = 81;
const SHELL: LiveShellContentLayer = LiveShellContentLayer::Shell;
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
    _validator: Validator,
    combined: CombinedHead,
    job: u64,
    frame: LiveProductionNativeFrameId,
    mixed: LiveOwnedMixedCompositionFrame,
}

fn held() -> Held {
    held_on(
        CombinedNativeFixture::new(&outputs()),
        |fixture, key, size| fixture.stub().serve_exported(key, size, LEASE),
    )
}

/// The first head's frame, forwarded through its reservation and settled
/// Held, on `fixture`; `serve` drives a renderer the test owns, if any.
fn held_on(
    mut fixture: CombinedNativeFixture,
    serve: impl FnOnce(
        &mut CombinedNativeFixture,
        crate::LiveRendererWorkerOutputKey,
        sophia_protocol::Size,
    ),
) -> Held {
    let output = fixture.heads[0].output;
    let frame = ShellContentNative::allocate_combined_frame(&mut fixture);
    let (combined, inbox) = ShellContentNative::reserve_combined_frame(&mut fixture, output.id)
        .expect("the serving facade reserves");
    let key = crate::LiveRendererWorkerOutputKey::from_raw(output.id.raw());
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = validator
        .reserve_combined(8, Instant::now() + WAIT)
        .unwrap();
    let head = &fixture.heads[0];
    let trace = Some(LiveCompositionTrace {
        output: output.id,
        head: head.head,
        scene_generation: frame.raw(),
    });
    // Not Clone: the plan and the expectation each build their own.
    let mixed = || LiveOwnedMixedCompositionFrame {
        trace,
        ..Default::default()
    };
    let identity =
        fixture
            .native_owner
            .frame(output.id, head.head, head.target_generation, frame.raw());
    let plan = CombinedRenderPlan::new(
        Instant::now() + WAIT,
        crate::api::LiveGbmEglFrameTargetRecord::new(output.size),
        crate::PendingRenderedFrame::Mixed(mixed(), Some(identity)),
        Vec::new(),
        None,
        Box::new([]),
    );
    let _owner_plan = validator
        .submit_combined(&reservation, pixels(), plan, inbox)
        .ok()
        .expect("combined submission");
    serve(&mut fixture, key, output.size);
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
        _validator: validator,
        combined,
        job,
        frame,
        mixed: mixed(),
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

    /// The accept through the bridge's public entry.
    fn bridge_accept(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        frame: LiveShellContentFrame,
        accept: CombinedAccept,
    ) -> Result<(), Box<CombinedAcceptFailed>> {
        ShellContentNative::accept_combined_shell_content(
            &mut self.fixture,
            runtime,
            frame,
            SHELL,
            &self.combined,
            accept,
        )
    }

    /// The native half alone (G3-4's), for the cross-path control that
    /// needs only an accepted native retirement.
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
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
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
    assert_eq!(held.fixture.stub().releases(), 1);
}

/// A visual runtime on the fixture's outputs, the store that owns the
/// candidate's lease, and a Shell candidate for head 0's output.
fn candidate(
    held: &Held,
    generation: u64,
) -> (
    LiveProductionVisualRuntime,
    sophia_runtime::ContentResourceStore,
    LiveShellContentFrame,
) {
    let outputs = held
        .fixture
        .heads
        .iter()
        .map(|head| head.output)
        .collect::<Vec<_>>();
    let runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let grant = sophia_protocol::ContentGrant {
        connection_epoch: 200,
        content_grant_epoch: 200,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(sophia_protocol::ContentLimits::prototype(grant))
            .unwrap();
    let resource = sophia_protocol::ContentResourceId {
        id: 1,
        generation: 1,
    };
    let frame = shell_frame(
        held.output(),
        generation,
        upload(&mut store, grant, resource),
    );
    (runtime, store, frame)
}

/// Gate 4 binding repair: the bridge accept writes the ordinary emitter's
/// `sophia_shell_native_binding` record once for the fixture's one target,
/// naming the committed native frame, the fixture's owner and head, and
/// the candidate's grant and generation; the install and claim are as in
/// the control below.
#[test]
fn the_bridge_accept_writes_one_binding_record_for_the_committed_frame() {
    let mut fixture = CombinedNativeFixture::new(&outputs());
    // One frame drawn and left aside, so the held frame, the job and the
    // candidate generation are three different numbers.
    ShellContentNative::allocate_combined_frame(&mut fixture);
    let mut held = held_on(fixture, |fixture, key, size| {
        fixture.stub().serve_exported(key, size, LEASE)
    });
    let output = held.output();
    let (mut visual, _store, candidate) = candidate(&held, 3);
    assert_eq!(held.frame.raw(), 2);
    assert_ne!(held.job, 2);
    assert_ne!(held.job, 3);
    let head = &held.fixture.heads[0];
    assert_eq!(
        (
            head.head.raw(),
            head.target_generation,
            head.refresh_millihz
        ),
        (1, 1, 60_000),
        "the fixture head as native_head_fixture declares it"
    );
    assert_eq!(
        held.fixture.head_targets(output.id).len(),
        1,
        "one current target, before any record is counted"
    );
    let identity = held.fixture.native_owner.frame(
        output.id,
        head.head,
        head.target_generation,
        held.frame.raw(),
    );
    let expected = format!(
        "sophia_shell_native_binding schema=1 connection_epoch={} content_grant_epoch={} output={} candidate_generation={} native_owner={} native_frame={} head={} target_generation={} heads=1 mode_refresh_millihz={}",
        candidate.grant.connection_epoch,
        candidate.grant.content_grant_epoch,
        output.id.raw(),
        candidate.candidate_generation,
        identity.owner(),
        identity.frame(),
        head.head.raw(),
        head.target_generation,
        head.refresh_millihz,
    );
    let events = Arc::new(BindingEvents::new(None));
    let dispatch = tracing::Dispatch::new(events.clone());
    let accept = held.accept();
    let accepted = tracing::dispatcher::with_default(&dispatch, || {
        held.bridge_accept(&mut visual, candidate.clone(), accept)
    });
    assert!(accepted.is_ok(), "the bridge accepts the held result");
    assert_eq!(events.seen(), vec![(expected, None)]);
    assert_eq!(
        installed_shell(&visual, output.id, SHELL, held.frame),
        (Some(candidate.clone()), Some(candidate.grant)),
        "the runtime installs the candidate and binds its claim to the plan's native frame"
    );
    assert_eq!(
        held.fixture.heads[0].rendering_content,
        Some(crate::LiveProductionScanoutContent::RetainedMixed {
            frame: held.frame,
            requires_retirement: true,
            logical_content_checksum: Some(held.frame.raw()),
            nonzero_rgb_pixels: 0,
        }),
        "the native head renders retirement-bearing shell content"
    );
    assert!(held.fixture.protected(output.id));
}

#[test]
fn the_bridge_accept_installs_binds_and_reaches_submitted_waiting_for_page_flip() {
    let mut held = held();
    let output = held.output();
    let (mut visual, _store, candidate) = candidate(&held, 1);
    let accept = held.accept();
    assert!(
        held.bridge_accept(&mut visual, candidate.clone(), accept)
            .is_ok(),
        "the bridge accepts the held result"
    );
    assert_eq!(
        installed_shell(&visual, output.id, SHELL, held.frame),
        (Some(candidate.clone()), Some(candidate.grant)),
        "the runtime installs the candidate and binds its claim to the plan's native frame"
    );
    assert_eq!(
        held.fixture.heads[0].rendering_content,
        Some(crate::LiveProductionScanoutContent::RetainedMixed {
            frame: held.frame,
            requires_retirement: true,
            logical_content_checksum: Some(held.frame.raw()),
            nonzero_rgb_pixels: 0,
        }),
        "the native head renders retirement-bearing shell content"
    );
    assert!(held.fixture.protected(output.id));
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

#[test]
fn a_refused_bridge_accept_returns_the_frame_and_keeps_the_held_result() {
    let mut held = held();
    let output = held.output();
    let (mut visual, _store, candidate) = candidate(&held, 1);
    let mut accept = held.accept();
    accept.correlation.native = None;
    let Err(failed) = held.bridge_accept(&mut visual, candidate.clone(), accept) else {
        panic!("a missing captured identity is accepted");
    };
    assert!(
        matches!(
            failed.error,
            CombinedAcceptError::Native(CombinedAcceptRefusal::Identity)
        ),
        "the native preflight's refusal comes back through the bridge"
    );
    assert_eq!(failed.frame, candidate, "the refused frame comes back");
    assert_eq!(
        installed_shell(&visual, output.id, SHELL, held.frame),
        (None, None),
        "the runtime installs nothing and binds no claim"
    );
    assert_eq!(held.fixture.heads[0].rendering_content, None);
    assert!(!held.fixture.protected(output.id));
    assert!(!held.fixture.exporters[0].worker_in_flight());
    assert_eq!(
        ShellContentNative::release_combined_held(&mut held.fixture, &held.combined, held.job),
        CombinedRelease::Transferred,
        "the facade still holds the result for disposal"
    );
    assert_eq!(held.fixture.stub().releases(), 1);
}

// G3-5b: the owner's worker fact and the served dispositions.

#[test]
fn a_busy_renderer_worker_declines_the_lane() {
    let held = held().accepted();
    assert!(
        held.fixture.exporters[0].worker_in_flight(),
        "the accepted combined frame is the worker's render in flight"
    );
    let (runtime, _store, mut frame) = candidate(&held, 1);
    for image in &mut frame.images {
        let description = image.resource.shell().unwrap().description().clone();
        image.resource = sophia_engine::CompositorImageSource::Provisional(description);
    }
    let scene = crate::LiveProductionCpuScene::new(held.output().size);
    match ShellContentNative::compose_combined_frame(
        &held.fixture,
        &runtime,
        &scene,
        SHELL,
        &frame,
        crate::LiveProductionCursorPresentation::HardwarePlane,
        None,
    ) {
        Err(crate::CombinedComposeRefusal::Ineligible(crate::CombinedIneligible::WorkerBusy)) => {}
        Err(other) => panic!("declined for another reason: {other:?}"),
        Ok(_) => panic!("a busy worker's output composed for the lane"),
    }
}

/// One combined frame on head 0's output, served with `reply` on time or
/// after its deadline, then settled through the bridge and given up: the
/// disposition read before settlement, the Release outcome, and the
/// releases the renderer received.
fn served(reply: crate::StubReply, late: bool) -> (CombinedDisposition, CombinedRelease, usize) {
    let outputs = outputs();
    let mut fixture = CombinedNativeFixture::new(&outputs);
    let output = outputs[0];
    let (combined, inbox) = ShellContentNative::reserve_combined_frame(&mut fixture, output.id)
        .expect("the serving facade reserves");
    let frame = ShellContentNative::allocate_combined_frame(&mut fixture);
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let deadline = Instant::now() + WAIT;
    let reservation = validator.reserve_combined(8, deadline).unwrap();
    let head = &fixture.heads[0];
    let identity =
        fixture
            .native_owner
            .frame(output.id, head.head, head.target_generation, frame.raw());
    let plan = CombinedRenderPlan::new(
        deadline,
        crate::api::LiveGbmEglFrameTargetRecord::new(output.size),
        crate::PendingRenderedFrame::Mixed(Default::default(), Some(identity)),
        Vec::new(),
        None,
        Box::new([]),
    );
    let _owner_plan = validator
        .submit_combined(&reservation, pixels(), plan, inbox)
        .ok()
        .expect("combined submission");
    let at = if late {
        deadline + WAIT
    } else {
        Instant::now()
    };
    fixture.serve_combined(output.id, at, reply);
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
    let disposition = result.disposition();
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
        "every disposition settles its own token and returns its pixels"
    );
    let release = ShellContentNative::release_combined_held(&mut fixture, &combined, job);
    (disposition, release, fixture.renderer_releases())
}

#[test]
fn only_an_exported_render_is_held_and_released() {
    let size = outputs()[0].size;
    let exported = crate::StubReply::Exported { size, lease: LEASE };
    assert_eq!(
        served(exported, false),
        (
            CombinedDisposition::Rendered,
            CombinedRelease::Transferred,
            1
        ),
        "an exported render is Rendered, held, and released once"
    );
    for (reply, late, disposition) in [
        (
            crate::StubReply::Failed,
            false,
            CombinedDisposition::RenderFailed,
        ),
        (
            crate::StubReply::Mismatch,
            false,
            CombinedDisposition::Mismatch,
        ),
        (exported, true, CombinedDisposition::Expired),
    ] {
        let (read, release, releases) = served(reply, late);
        assert_eq!(read, disposition, "{reply:?} late={late}");
        assert_ne!(
            release,
            CombinedRelease::Transferred,
            "{disposition:?} holds nothing to give up"
        );
        assert_eq!(releases, 0, "{disposition:?} sends no Release");
    }
}

/// The fixture's head describes the output it was asked for, at a
/// non-default size (the Session controls' 64x32): selection, output,
/// render target and presentation state agree, so a snapshot of that
/// target is a presentation the head accepts.
#[test]
fn a_fixture_head_describes_a_non_default_output_coherently() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: 64,
            height: 32,
        },
        scale: 1,
    };
    let fixture = CombinedNativeFixture::new(&[output]);
    let head = &fixture.heads[0];
    assert_eq!(head.output, output, "the head serves the output");
    assert_eq!(
        head.selection.size, output.size,
        "the selection is the output's size"
    );
    let targets = fixture.head_targets(output.id);
    assert_eq!(targets.len(), 1, "one render target");
    assert_eq!(
        (targets[0].native_size, targets[0].scale),
        (output.size, 1),
        "the render target is the output's size"
    );
    let frame = snapshot(HeadlessOutput {
        id: output.id,
        size: targets[0].native_size,
        scale: targets[0].scale,
    });
    assert!(
        head.output_frames.presentation_for(frame).is_ok(),
        "the head accepts a snapshot of its own render target"
    );
}

#[test]
fn a_running_seam_render_is_held_accepted_and_reaches_submitted_waiting_for_page_flip() {
    let mut held = held_on(CombinedNativeFixture::running(&outputs()), |_, _, _| ());
    let output = held.output();
    let slots = held
        .fixture
        .slot_metrics(output.id)
        .expect("the head has a worker");
    assert_eq!(
        (slots.acquisitions, slots.leased, slots.stale_releases),
        (1, 1, 0),
        "the running worker leased one slot for the seam's export"
    );
    let (mut visual, _store, candidate) = candidate(&held, 1);
    let accept = held.accept();
    assert!(
        held.bridge_accept(&mut visual, candidate.clone(), accept)
            .is_ok(),
        "the bridge accepts the running worker's held result"
    );
    assert_eq!(
        installed_shell(&visual, output.id, SHELL, held.frame),
        (Some(candidate.clone()), Some(candidate.grant)),
        "the runtime installs the candidate and binds its claim"
    );
    assert_eq!(
        held.fixture
            .service_native(output.id)
            .expect("the native service runs"),
        Some(Status::SubmittedWaitingForPageFlip),
        "one native service submits the accepted frame"
    );
    assert_eq!(held.fixture.device_counts(output.id), Some((1, 1, 0)));
    let head = &held.fixture.heads[0];
    assert_eq!(
        head.rendering_content, None,
        "the rendering owner was taken"
    );
    assert!(head.output_frames.rendering().is_none());
    assert_eq!(
        head.output_frames
            .submitted()
            .map(|presentation| &presentation.snapshot),
        Some(&snapshot(output)),
        "the accept's frame was promoted to submitted"
    );
    assert_eq!(
        head.submitted_content,
        Some(crate::LiveProductionScanoutContent::RetainedMixed {
            frame: held.frame,
            requires_retirement: true,
            logical_content_checksum: Some(held.frame.raw()),
            nonzero_rgb_pixels: 0,
        }),
        "the head keeps the retirement-bearing content it submitted"
    );
    assert_eq!(
        held.fixture
            .slot_metrics(output.id)
            .map(|slots| slots.leased),
        Some(1),
        "the submitted lease keeps its slot; this fixture completes no flip"
    );
}

#[test]
fn a_submission_naming_another_target_is_not_recorded_as_the_heads() {
    let mut held = held_on(CombinedNativeFixture::running(&outputs()), |_, _, _| ());
    let output = held.output();
    let (mut visual, _store, candidate) = candidate(&held, 1);
    let accept = held.accept();
    assert!(
        held.bridge_accept(&mut visual, candidate, accept).is_ok(),
        "the bridge accepts the running worker's held result"
    );
    // The head moves to a new target after the accept: the identity the
    // worker captured now names the old one.
    held.fixture.heads[0].target_generation += 1;
    let error = held
        .fixture
        .service_native(output.id)
        .expect_err("a foreign captured identity is refused");
    assert_eq!(
        error.to_string(),
        "accepted native submission belongs to another owner/head/target"
    );
    let head = &held.fixture.heads[0];
    assert!(
        head.rendering_content.is_some() && head.output_frames.rendering().is_some(),
        "the head still holds its rendering frame"
    );
    assert_eq!(
        head.submitted_content, None,
        "nothing was recorded submitted"
    );
}

/// The binding controls run beside tests that reach the same emitters with
/// no subscriber. A record written under the scoped subscriber must arrive
/// even when another thread was first to use its callsite.
#[test]
fn a_scoped_binding_record_survives_another_threads_first_use_of_its_callsite() {
    fn emit() {
        tracing::info!(
            target: "sophia_scanout_evidence",
            "sophia_shell_native_binding probe"
        );
    }
    let events = Arc::new(BindingEvents::new(None));
    let dispatch = tracing::Dispatch::new(events.clone());
    std::thread::spawn(emit).join().unwrap();
    tracing::dispatcher::with_default(&dispatch, emit);
    assert_eq!(events.seen().len(), 1);
}
