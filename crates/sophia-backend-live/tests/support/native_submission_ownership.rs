//! Exercise the production submission owner with supplied native facts.
//! No DMA import, image promotion or page-flip retirement is claimed here.
use super::*;

#[derive(Default)]
struct Target {
    frames: BTreeMap<OutputId, LiveProductionNativeFrameId>,
    discarded: Vec<Option<OutputId>>,
    activated: Vec<OutputId>,
}

impl NativeSubmissionTarget for Target {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
        self.frames.get(&output).copied()
    }

    fn discard_presentation_feedback(&mut self, output: Option<OutputId>) {
        self.discarded.push(output);
    }

    fn activate_deferred_mirror_generation(
        &mut self,
        output: OutputId,
    ) -> Result<bool, &'static str> {
        self.activated.push(output);
        Ok(true)
    }
}

const TRANSACTION: TransactionId = TransactionId::from_raw(71);
const SURFACE: SurfaceId = SurfaceId::new(7, 1);

fn fixture() -> (LiveProductionVisualRuntime, Target) {
    let size = Size {
        width: 16,
        height: 16,
    };
    let outputs = [1, 2].map(|id| HeadlessOutput {
        id: OutputId::from_raw(id),
        size,
        scale: 1,
    });
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let buffer = BufferHandle::from_raw(91);
    runtime
        .presentation_feedback
        .resources_mut()
        .register_source(
            DmaBufDescriptor {
                handle: buffer,
                size,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                modifier: DRM_FORMAT_MOD_INVALID,
                plane_count: 1,
                planes: [
                    Some(DmaBufPlaneDescriptor {
                        offset: 0,
                        stride: 64,
                    }),
                    None,
                    None,
                    None,
                ],
            },
            vec![std::fs::File::open("/dev/null").unwrap().into()],
        )
        .unwrap();
    runtime
        .presentation_feedback
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction: TRANSACTION,
            buffer,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };
    let candidate = SurfaceTransaction {
        input_region: None,
        transaction: TRANSACTION,
        authority: AuthorityKind::SophiaX,
        surface: SURFACE,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: buffer.raw(),
            },
            size,
        ),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let prepared = runtime.production.prepare_present_transaction(&candidate);
    assert!(prepared.is_ready());
    let frames = outputs
        .into_iter()
        .map(|output| {
            (
                output.id,
                LiveProductionNativeFrameId::from_raw(output.id.raw() + 10),
            )
        })
        .collect::<BTreeMap<_, _>>();
    runtime.present_scheduler.mark_rendering(
        LiveProductionSubmittedPresent::new(
            frames.clone(),
            outputs[0].id,
            candidate.key(),
            TRANSACTION,
            SURFACE,
            prepared,
            LiveRetainedRendererImageLayer {
                image_id: renderer_image_for_present(TRANSACTION),
                size,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                placement: LiveCompositionPlacement {
                    target: geometry,
                    clip: None,
                    transform: Transform::IDENTITY,
                    alpha: 1.0,
                    sampling: HeadSamplingClass::Exact,
                },
            },
        )
        .unwrap(),
    );
    (
        runtime,
        Target {
            frames,
            ..Default::default()
        },
    )
}

fn submit(
    runtime: &mut LiveProductionVisualRuntime,
    target: &mut Target,
    output: OutputId,
    transaction: TransactionId,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = target.frames[&output];
    runtime.settle_submission_ownership(
        target,
        output,
        LiveProductionScanoutContent::MixedPresent {
            frame,
            transaction,
            nonzero_rgb_pixels: 256,
        },
        Some((frame, TRANSACTION)),
    )
}

#[test]
fn dma_submission_advances_resources_only_after_the_exact_output_cohort() {
    let (mut runtime, mut target) = fixture();
    let outputs = target.frames.keys().copied().collect::<Vec<_>>();
    submit(&mut runtime, &mut target, outputs[0], TRANSACTION).unwrap();
    assert!(!runtime.present_scheduler.has_submitted());
    assert_eq!(
        runtime.presentation_feedback.resources().state(TRANSACTION),
        Some(LiveBufferState::Ready)
    );
    assert!(target.discarded.is_empty());
    assert_eq!(target.activated, vec![outputs[0]]);
    submit(
        &mut runtime,
        &mut target,
        outputs[1],
        TransactionId::from_raw(999),
    )
    .unwrap_err();
    assert!(
        !runtime.present_scheduler.has_submitted(),
        "mismatched submission cannot advance the cohort"
    );
    assert_eq!(
        runtime.presentation_feedback.resources().state(TRANSACTION),
        Some(LiveBufferState::Ready)
    );
    assert_eq!(target.activated, vec![outputs[0]]);
    submit(&mut runtime, &mut target, outputs[1], TRANSACTION).unwrap();
    assert!(
        runtime.present_scheduler.has_submitted(),
        "exact output cohort must become submitted"
    );
    assert_eq!(
        runtime.presentation_feedback.resources().state(TRANSACTION),
        Some(LiveBufferState::Submitted),
        "complete cohort must submit retained Present resources"
    );
    assert_eq!(target.discarded, vec![Some(outputs[1])]);
    assert_eq!(target.activated, outputs);
    submit(&mut runtime, &mut target, outputs[1], TRANSACTION).unwrap();
    assert_eq!(
        target.discarded,
        vec![Some(outputs[1])],
        "duplicate submission cannot resubmit resources"
    );
    assert!(
        runtime.committed_surfaces().is_empty(),
        "submission is not retirement"
    );
}
