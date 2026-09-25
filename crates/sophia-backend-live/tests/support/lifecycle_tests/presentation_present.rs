//! t246, the overview crash (2026-09-25, installed 8bd8c41c): an application that
//! Presents while ReplaceApplications covers its output, with no preview of
//! it there, is sampled by no lowered list. The Present source collection
//! refused that frame ("visible Present surface is missing from the
//! presentation order", red at de5173d5) and the owner loop made it a session
//! fatal. The collection now resolves exactly the sampled union: an unsampled
//! current Present is released under a presentation that replaces its
//! outputs, the frames capture no image of it, and the Present driver's
//! no-captured-image branch then settles or defers it with its clearing
//! repaint. A sampled source that cannot be resolved, or a Present whose own
//! output was omitted, still refuses the frame.
//!
//! The driver's settlement of an uncaptured Present is the shared runtime
//! helper `settle_uncaptured_present`, which these tests execute against the
//! lifecycle target with a Present queued through the runtime's own
//! scheduler and feedback coordinator: the clearing repaint, frame-tick
//! parking, first-visibility parking, the runtime service's release and
//! expiry, and backend-produced Skipped completion and Idle ready for routing,
//! with presentation retirement. No independent wire client is driven here.
//!
//! Known limit, not broadened without a reproduction: while a presentation
//! withholds its whole tier for a missing source, the ordinary draw returns
//! and samples the surface, but `present_sampling` and
//! `surface_hidden_by_policy` still see a replacing presentation. The
//! collection then consumes the sampled Present as usual. A first Present
//! parked in that window waits for its budget rather than being released
//! early; admission refuses and removal revokes such a presentation.
//!
//! Fixture limit: drive_gpu_presentation itself, which takes the concrete
//! native scanout, is not executed. Its DMA-BUF import and mixed-frame build,
//! its gate and busy-output checks, and its computation of first_presentation
//! run only on a device; the tests pass first_presentation as the driver
//! computes it (a committed surface, or a spent budget, is not first).
use super::presentation_instances::{
    commit_cpu_surface, presentation_output, published, rect, region, shown_instance,
};
use super::*;
use std::time::{Duration, Instant};

const PRESENT_IMAGE: u64 = 437;

fn dma_frame() -> sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame {
    let fd: std::os::fd::OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
    sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame {
        width: 16,
        height: 16,
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd,
                offset: 0,
                stride: 16 * 4,
            }),
            None,
            None,
            None,
        ],
    }
}

/// The Present being composed: the application's committed DMA-BUF.
fn current_present(application: SurfaceId) -> sophia_renderer_live::LiveOwnedHeadCompositionSource {
    sophia_renderer_live::LiveOwnedHeadCompositionSource {
        surface: application,
        source: BufferSource::DmaBuf { handle: 77 },
        kind: sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf {
            image_id: sophia_renderer_live::LiveRendererImageId::from_raw(PRESENT_IMAGE),
            frame: dma_frame(),
        },
    }
}

fn commit_dma_surface(
    runtime: &mut LiveProductionVisualRuntime,
    surface: SurfaceId,
    generation: u64,
    geometry: Rect,
) {
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let transaction = SurfaceTransaction {
        transaction: TransactionId::from_raw(700 + generation),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 77 }, size),
        damage: Region::single(rect(0, 0, size.width, size.height)),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: generation - 1,
        input_region: None,
    };
    runtime
        .prepare_authority_transactions(transaction.transaction, &[transaction], &[])
        .unwrap();
}

struct PresentScene {
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
    target: MirroredTarget,
    output: OutputId,
    application: SurfaceId,
    previewed: SurfaceId,
}

fn present_scene() -> PresentScene {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let application = SurfaceId::new(7, 1);
    let previewed = SurfaceId::new(8, 1);
    commit_dma_surface(&mut runtime, application, 1, rect(0, 0, 16, 16));
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        previewed,
        88,
        1,
        rect(20, 0, 16, 16),
    );
    runtime.presentation_order = vec![application, previewed];
    runtime.surface_outputs.insert(application, output);
    runtime.surface_outputs.insert(previewed, output);
    PresentScene {
        runtime,
        scene,
        target: MirroredTarget::new(&outputs),
        output,
        application,
        previewed,
    }
}

impl PresentScene {
    fn replace_with(&mut self, instances: Vec<PolicySurfaceInstance>) {
        let output = self.output;
        self.runtime
            .set_policy_presentation(
                Some(published(
                    1,
                    vec![presentation_output(
                        output,
                        PolicyPresentationMode::ReplaceApplications,
                    )],
                    instances,
                    vec![region(
                        output,
                        1,
                        0,
                        PolicyPresentationRegionRole::Backdrop,
                        rect(0, 0, 64, 32),
                        rect(0, 0, 64, 32),
                    )],
                )),
                &self.scene,
                None,
            )
            .unwrap();
    }

    /// The Present path's composition for this Present, as present.rs runs it:
    /// the applicable output's display list against the candidate, the source
    /// collection, the head frames, and whether they capture the Present.
    fn compose_present(
        &self,
    ) -> Result<
        (
            Vec<sophia_renderer_live::LiveOwnedHeadCompositionSource>,
            bool,
        ),
        Box<dyn std::error::Error>,
    > {
        let (sources, frames) = self.compose_present_frames()?;
        let captured = crate::live_present_head_frames_capture_image(
            &frames,
            sophia_renderer_live::LiveRendererImageId::from_raw(PRESENT_IMAGE),
        );
        Ok((sources, captured))
    }

    /// The Present's sources and head frames, per output, as the driver
    /// builds them before its capture check.
    #[allow(clippy::type_complexity)]
    fn compose_present_frames(
        &self,
    ) -> Result<
        (
            Vec<sophia_renderer_live::LiveOwnedHeadCompositionSource>,
            Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
        ),
        Box<dyn std::error::Error>,
    > {
        let viewport = self.runtime.outputs.logical_viewport(self.output).unwrap();
        let committed = self.runtime.committed_surfaces().to_vec();
        let list = self.runtime.display_list_for_output(
            self.output,
            viewport,
            &committed,
            &self.runtime.presentation_order,
        )?;
        let cpu_layers = self
            .scene
            .presentation_variant_layers(&committed, &self.runtime.sampled_surface_order());
        let sources = crate::live_present_head_composition_sources(
            self.application,
            current_present(self.application),
            self.runtime.present_sampling(&[self.output]),
            &committed,
            [&list],
            &cpu_layers,
            |_| None,
            |_| None,
        )?;
        let frames = self.runtime.compose_native_head_frames_from_sources(
            &self.target,
            self.output,
            &committed,
            list,
            1,
            &sources,
        )?;
        Ok((sources, vec![(self.output, frames)]))
    }
}

/// A displayed application whose output a presentation then replaces, with
/// no preview of it: the Present is sampled while displayed, then collected
/// without it (the preview's source still resolved), captured by no frame,
/// and so routed to the no-captured-image branch rather than refused; the
/// withdrawal recovers it.
#[test]
fn a_replaced_present_without_a_preview_is_released_and_captured_by_no_frame() {
    let mut scene = present_scene();
    let (_, displayed) = scene.compose_present().unwrap();
    assert!(displayed, "while displayed, the Present is captured");

    let preview = shown_instance(scene.output, 2, 1, scene.previewed, rect(40, 4, 8, 8));
    scene.replace_with(vec![preview]);
    let (sources, captured) = scene
        .compose_present()
        .expect("an unsampled Present is not a missing source");
    assert!(
        !captured,
        "no frame captures the replaced Present: the path skips or defers it"
    );
    assert!(
        sources
            .iter()
            .all(|source| source.surface != scene.application),
        "the unsampled current Present is released"
    );
    assert!(
        sources
            .iter()
            .any(|source| source.surface == scene.previewed),
        "the sources the lists sample are still resolved"
    );

    scene
        .runtime
        .set_policy_presentation(None, &scene.scene, None)
        .unwrap();
    let (_, recovered) = scene.compose_present().unwrap();
    assert!(
        recovered,
        "withdrawn, the application's Present is captured again"
    );
}

/// A presenting surface the presentation previews is sampled by the
/// instance alone: the current Present is its source and its frame captures
/// it, though the application's own draw is replaced.
#[test]
fn a_previewed_presenting_surface_samples_the_current_present() {
    let mut scene = present_scene();
    let preview = shown_instance(scene.output, 2, 1, scene.application, rect(40, 4, 8, 8));
    scene.replace_with(vec![preview]);
    let (sources, captured) = scene.compose_present().unwrap();
    assert!(captured, "the preview draws the Present being composed");
    assert!(
        sources
            .iter()
            .any(|source| source.surface == scene.application
                && source.source == BufferSource::DmaBuf { handle: 77 })
    );
}

/// Releasing an unsampled current Present does not relax the refusal of a
/// sampled source that cannot be resolved: a preview of a DMA-BUF surface
/// with no displayed or retained image still refuses the frame.
#[test]
fn a_sampled_source_that_cannot_be_resolved_still_refuses_the_frame() {
    let mut scene = present_scene();
    let neighbour = SurfaceId::new(9, 1);
    commit_dma_surface(&mut scene.runtime, neighbour, 1, rect(40, 0, 16, 16));
    let preview = shown_instance(scene.output, 2, 1, neighbour, rect(40, 4, 8, 8));
    scene.replace_with(vec![preview]);
    let error = scene
        .compose_present()
        .expect_err("a sampled but unresolvable source still refuses")
        .to_string();
    assert_eq!(error, "retained head plan has no authority-owned source");
}

/// An unsampled Present is released only under a presentation that replaces
/// its output. Without one, a list set that omits the Present's owner is a
/// caller error and still refuses, never a silently lost Present.
#[test]
fn an_unsampled_present_without_a_replacing_presentation_still_refuses() {
    let scene = present_scene();
    assert_eq!(
        scene.runtime.present_sampling(&[scene.output]),
        crate::LivePresentSampling::Required
    );
    let committed = scene.runtime.committed_surfaces().to_vec();
    let empty = CompositorDisplayList::empty(scene.output);
    let error = crate::live_present_head_composition_sources(
        scene.application,
        current_present(scene.application),
        scene.runtime.present_sampling(&[scene.output]),
        &committed,
        [&empty],
        &[],
        |_| None,
        |_| None,
    )
    .expect_err("an omitted owner refuses")
    .to_string();
    assert_eq!(
        error,
        "visible Present surface is missing from the presentation order"
    );
}

/// An Overlay presentation does not replace the application: its own draw
/// stays, so its Present stays required and captured.
#[test]
fn an_overlay_presentation_keeps_the_present_required_and_captured() {
    let mut scene = present_scene();
    let output = scene.output;
    scene
        .runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![shown_instance(
                    output,
                    2,
                    1,
                    scene.previewed,
                    rect(40, 4, 8, 8),
                )],
                vec![],
            )),
            &scene.scene,
            None,
        )
        .unwrap();
    assert_eq!(
        scene.runtime.present_sampling(&[output]),
        crate::LivePresentSampling::Required
    );
    let (_, captured) = scene.compose_present().unwrap();
    assert!(captured);
}

impl PresentScene {
    /// Queue this application's Present through the runtime's own scheduler
    /// and take it to the head of the runnable queue, as the driver finds it.
    fn queue_present(
        &mut self,
        id: u64,
        now: Instant,
    ) -> (TransactionId, sophia_protocol::SurfaceTransactionKey) {
        let handle = BufferHandle::from_raw(id);
        let transaction = TransactionId::from_raw(id);
        self.runtime
            .presentation_feedback
            .resources_mut()
            .register_source(
                present_descriptor(handle),
                vec![std::fs::File::open("/dev/null").unwrap().into()],
            )
            .unwrap();
        let group = present_group(transaction, self.application, handle);
        self.runtime
            .present_scheduler
            .enqueue_group(
                &group,
                &[],
                self.runtime.presentation_feedback.resources_mut(),
                now,
            )
            .unwrap();
        assert_eq!(
            self.runtime
                .present_scheduler
                .poll_gate(self.runtime.presentation_feedback.resources_mut(), now)
                .unwrap(),
            crate::LiveProductionPresentGate::Ready(transaction)
        );
        (
            transaction,
            self.runtime
                .present_scheduler
                .front()
                .unwrap()
                .candidate
                .key(),
        )
    }

    /// The driver's settlement of this Present, which no frame captures.
    fn settle(
        &mut self,
        transaction: TransactionId,
        candidate: sophia_protocol::SurfaceTransactionKey,
        first_presentation: bool,
        now: Instant,
    ) {
        let (_, frames) = self.compose_present_frames().unwrap();
        assert!(!crate::live_present_head_frames_capture_image(
            &frames,
            sophia_renderer_live::LiveRendererImageId::from_raw(PRESENT_IMAGE),
        ));
        let committed = self.runtime.committed_surfaces().to_vec();
        self.runtime
            .settle_uncaptured_present(
                &mut self.target,
                crate::production_visual_runtime::present::UncapturedPresent {
                    transaction,
                    candidate,
                    surface: self.application,
                    image: sophia_renderer_live::LiveRendererImageId::from_raw(PRESENT_IMAGE),
                    first_presentation,
                    paced_interval: Duration::from_millis(16),
                },
                &committed,
                frames,
                now,
            )
            .unwrap();
    }
}

fn present_descriptor(handle: BufferHandle) -> DmaBufDescriptor {
    DmaBufDescriptor {
        handle,
        size: Size {
            width: 16,
            height: 16,
        },
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
    }
}

fn present_group(
    transaction: TransactionId,
    surface: SurfaceId,
    handle: BufferHandle,
) -> crate::LiveProductionAuthorityGroup {
    let size = Size {
        width: 16,
        height: 16,
    };
    crate::LiveProductionAuthorityGroup {
        transaction,
        transactions: vec![SurfaceTransaction {
            input_region: None,
            transaction,
            authority: AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: rect(0, 0, 16, 16),
            presentation_extent: size,
            content: SurfaceContentSet::singleton(
                BufferSource::DmaBuf {
                    handle: handle.raw(),
                },
                size,
            ),
            damage: Region::single(rect(0, 0, 16, 16)),
            readiness: SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: 0,
        }],
        cpu_buffer_updates: Vec::new(),
        removed_surfaces: Vec::new(),
        present_submissions: vec![crate::LiveProductionPresentSubmission {
            transaction,
            surface,
            buffer: handle,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
            layout_disposition: crate::LiveProductionPresentDisposition::Immediate,
        }],
        software_present_submissions: Vec::new(),
    }
}

/// The Present's settlement as the client is told it: Complete with the
/// Skipped disposition, then Idle, and no presentation or source still held
/// for it.
fn assert_skipped_and_released(scene: &mut PresentScene, transaction: TransactionId) {
    let mut feedback = Vec::new();
    scene
        .runtime
        .drain_present_feedback_into(&mut feedback)
        .unwrap();
    assert_eq!(feedback.len(), 1, "one settlement for {transaction:?}");
    assert_eq!(
        feedback[0].feedback,
        vec![
            LivePresentProtocolFeedback::Complete {
                transaction,
                ust: 0,
                msc: 0,
                disposition: LivePresentBufferDisposition::Skipped,
            },
            LivePresentProtocolFeedback::Idle { transaction },
        ]
    );
    assert_eq!(
        scene
            .runtime
            .presentation_feedback
            .resources()
            .state(transaction),
        None,
        "the presentation is retired"
    );
    assert_eq!(scene.runtime.diagnostics().live_presentations, 0);
}

/// Production owner path, existing visible surface: a displayed application
/// whose output a presentation replaces, without a preview of it, Presents.
/// The driver's settlement queues the clearing repaint on the target and
/// parks the Present to the next frame tick, where the runtime's service
/// rejects it as Skipped and retires it; each repeated hidden Present takes
/// the same one-frame path; withdrawal restores capture.
#[test]
fn a_hidden_present_of_a_visible_surface_repaints_parks_and_retires_until_restored() {
    let mut scene = present_scene();
    let start = Instant::now();
    let preview = shown_instance(scene.output, 2, 1, scene.previewed, rect(40, 4, 8, 8));
    scene.replace_with(vec![preview]);
    for (round, id) in [900_u64, 901].into_iter().enumerate() {
        let now = start + Duration::from_millis(40 * round as u64);
        let (transaction, candidate) = scene.queue_present(id, now);
        scene.settle(transaction, candidate, false, now);
        if round == 0 {
            assert!(
                scene.target.queue.get(scene.output).is_some(),
                "the clearing repaint reaches the heads"
            );
        }
        assert_eq!(
            scene.runtime.present_scheduler.paced_skips(),
            round + 1,
            "round {round}: parked to the frame tick, not lost or failed"
        );
        scene.target.drain();
        scene
            .runtime
            .service_first_visibility_presentations(now + Duration::from_millis(20));
        assert_skipped_and_released(&mut scene, transaction);
    }
    scene
        .runtime
        .set_policy_presentation(None, &scene.scene, None)
        .unwrap();
    let (_, restored) = scene.compose_present().unwrap();
    assert!(
        restored,
        "withdrawn, the application's Present is captured again"
    );
}

/// Production owner path, first Present: an application whose first
/// Present finds its output replaced, without a preview, is parked for
/// first visibility with no repaint; the runtime's service expires that
/// budget, the driver no longer parks it, and the settlement rejects it as
/// Skipped and retires it rather than letting it wait forever.
#[test]
fn a_hidden_first_present_waits_within_its_budget_then_is_skipped_and_retired() {
    let mut scene = present_scene();
    let start = Instant::now();
    let preview = shown_instance(scene.output, 2, 1, scene.previewed, rect(40, 4, 8, 8));
    scene.replace_with(vec![preview]);
    let (transaction, candidate) = scene.queue_present(910, start);
    scene.settle(transaction, candidate, true, start);
    assert!(
        scene.target.queue.get(scene.output).is_none(),
        "a first Present parks without a repaint"
    );
    assert!(
        scene
            .runtime
            .present_scheduler
            .awaiting_first_visibility()
            .any(|(surface, _, reason)| surface == scene.application
                && reason == crate::LiveProductionFirstVisibilityReason::OutsideHeadFrames),
        "it waits for first visibility"
    );
    // Hidden by the presentation, it is not released by its own geometry
    // while it waits: it stays parked under its first deadline.
    let within = start + Duration::from_millis(1_000);
    scene.runtime.service_first_visibility_presentations(within);
    assert!(
        scene
            .runtime
            .present_scheduler
            .awaiting_first_visibility()
            .any(|(surface, _, _)| surface == scene.application),
        "a surface the presentation hides is not released as visible"
    );
    let later = start + Duration::from_millis(2_100);
    scene.runtime.service_first_visibility_presentations(later);
    assert!(
        scene
            .runtime
            .present_scheduler
            .front_first_visibility_exhausted(),
        "the service expires the budget and returns it for ordinary rejection"
    );
    assert_eq!(
        scene
            .runtime
            .present_scheduler
            .front()
            .unwrap()
            .candidate
            .key(),
        candidate
    );
    // The driver's first_presentation is false once the budget is spent.
    scene.settle(transaction, candidate, false, later);
    assert!(scene.runtime.present_scheduler.front().is_none());
    assert_skipped_and_released(&mut scene, transaction);
}

/// Recovery: a hidden first Present parked outside the head frames is
/// released as soon as the presentation stops hiding it, within its budget.
#[test]
fn a_hidden_first_present_is_released_when_the_presentation_withdraws() {
    let mut scene = present_scene();
    let start = Instant::now();
    let preview = shown_instance(scene.output, 2, 1, scene.previewed, rect(40, 4, 8, 8));
    scene.replace_with(vec![preview]);
    let (transaction, candidate) = scene.queue_present(920, start);
    scene.settle(transaction, candidate, true, start);
    scene
        .runtime
        .set_policy_presentation(None, &scene.scene, None)
        .unwrap();
    scene
        .runtime
        .service_first_visibility_presentations(start + Duration::from_millis(100));
    assert!(
        !scene
            .runtime
            .present_scheduler
            .awaiting_first_visibility()
            .any(|(surface, _, _)| surface == scene.application),
        "no longer hidden, it is released to present"
    );
    let released = scene
        .runtime
        .present_scheduler
        .front()
        .expect("released to the queue's head");
    assert_eq!(
        (released.submission.transaction, released.candidate.key()),
        (transaction, candidate),
        "the parked Present itself is released, without a fresh Present from anyone"
    );
    assert!(
        !scene
            .runtime
            .present_scheduler
            .front_first_visibility_exhausted()
    );
    let (_, captured) = scene.compose_present().unwrap();
    assert!(captured, "and its next composition captures it");
}
