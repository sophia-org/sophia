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
//! Fixture limit: these tests run the driver's composition steps (display
//! lists, source collection, head frames, the capture check) against the
//! lifecycle target. They do not execute the driver's settlement branch
//! (defer_first_visibility / skip_unpresentable / the clearing repaint queue),
//! because drive_gpu_presentation takes the concrete native scanout, which
//! needs a device.
use super::presentation_instances::{
    commit_cpu_surface, presentation_output, published, rect, region, shown_instance,
};
use super::*;

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
        let captured = crate::live_present_head_frames_capture_image(
            &[(self.output, frames)],
            sophia_renderer_live::LiveRendererImageId::from_raw(PRESENT_IMAGE),
        );
        Ok((sources, captured))
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
