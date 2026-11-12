//! G3-3: a combined frame job's provisional composition and its scene key.
//!
//! The candidate's images enter the real display list as
//! `CompositorImageSource::Provisional`, through the same output
//! composition, translation, head plans and damage snapshot as an ordinary
//! repaint, and the metadata lowering records each one as a placeholder at
//! its final layer index. Nothing installed is written, no cache is filled or
//! reordered, no renderer-image read is pruned or acquired, no DMA-BUF is
//! cloned, and no pixel is read or produced. At consumption the same path
//! runs with the candidate's real frame, and the two keys decide whether the
//! rendered frame still describes the scene.

// No production caller until the owner turn consumes this (G3-4/G3-5); only lifecycle tests reach it.
#![cfg_attr(not(test), allow(dead_code))]

use super::*;
use sophia_engine::CompositorImageSourceIdentity;
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveCpuPixelStorage, LiveHeadCompositionLoweringError,
    LiveImageOccurrence, LiveOwnedMixedCompositionLayer, LiveProvisionalComposition,
    LiveProvisionalDecline,
};

/// The owner's cursor this turn. The owner wiring is G3-5.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CombinedCursor {
    /// On the cursor plane at this root position, or hidden; no layer draws it.
    Plane(Option<sophia_protocol::Point>),
    /// Drawn into the frame by the software compositor.
    Software,
}

impl CombinedCursor {
    /// The owner's presentation, with the plane cursor's position this turn.
    pub(crate) fn new(
        presentation: crate::LiveProductionCursorPresentation,
        plane: Option<sophia_protocol::Point>,
    ) -> Self {
        match presentation {
            crate::LiveProductionCursorPresentation::HardwarePlane => Self::Plane(plane),
            crate::LiveProductionCursorPresentation::Software(_) => Self::Software,
        }
    }

    /// The plane position as bits; None for a software cursor.
    fn key(self) -> Option<Option<(u64, u64)>> {
        match self {
            Self::Plane(position) => {
                Some(position.map(|point| (point.x.to_bits(), point.y.to_bits())))
            }
            Self::Software => None,
        }
    }
}

/// The owner's facts at dispatch that no composition reads (F4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CombinedOwnerState {
    /// A capture of this output is pending or in flight.
    pub capture_active: bool,
    /// The output's renderer worker has no render in flight.
    pub worker_idle: bool,
}

/// One head's provisional frame for a combined job's candidate.
pub(crate) struct LiveProvisionalHeadFrame {
    pub head: sophia_engine::RenderHeadId,
    pub native_owner: crate::NativeFrameOwner,
    pub target_generation: u64,
    pub mapping: sophia_protocol::OutputHeadMapping,
    pub content_layout_generation: u64,
    /// The owner's cursor this turn.
    pub cursor: CombinedCursor,
    /// A cursor the head plan composites into the frame.
    pub plan_cursor: Option<sophia_engine::OutputSceneCursor>,
    /// The output's other installed content: layer, grant, presented
    /// transform and whether its interaction is revoked.
    pub installed: Vec<InstalledContentKey>,
    /// The candidate's layer is installed and closing under its grant.
    pub candidate_revoked: bool,
    pub composition: LiveProvisionalComposition,
}

/// One other installed component's facts on the output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstalledContentKey {
    layer: LiveShellContentLayer,
    grant: sophia_protocol::ContentGrant,
    transform: sophia_engine::PresentedContentTransform,
    interaction_revoked: bool,
}

/// Why a candidate cannot be composed for the combined job. Each falls back
/// to the conventional path before any reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CombinedIneligible {
    /// The output does not map to exactly one head.
    HeadCount,
    /// A capture of the output is pending or in flight.
    CaptureActive,
    /// The renderer worker has a render in flight.
    WorkerBusy,
    /// A layer other than CPU pixels or a solid fill.
    LayerKind,
    ImageReads,
    /// A CPU layer whose pixels carry no version, or a validated binding.
    UnversionedPixels,
    /// The cursor is drawn into the frame, by the software compositor or
    /// the head plan.
    SoftwareCursor,
    /// A candidate image that is not a provisional placeholder.
    CandidateNotProvisional,
    /// An installed shell image that did not lower to its own lease.
    InstalledNotLeased,
}

/// What a recomposition at consumption found (F1, R4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CombinedRecompose {
    Match,
    /// The candidate's own placement, geometry or identity moved.
    CandidateFailure,
    /// The head set, its target generation or its mapping moved.
    TopologyChange,
    /// Something other than the candidate changed in the scene.
    SceneChurn,
}

/// Why a composition produced no frames to key.
#[derive(Debug)]
pub(crate) enum ProvisionalComposeError {
    /// The output has no logical viewport or no head, or the planner did not
    /// cover its head targets.
    Topology,
    /// The metadata lowering declined a layer without rasterizing it.
    Declined(LiveProvisionalDecline),
    /// Any other failure of the ordinary composition path, unclassified.
    Composition(Box<dyn std::error::Error>),
}

impl<E: Into<Box<dyn std::error::Error>>> From<E> for ProvisionalComposeError {
    fn from(error: E) -> Self {
        Self::Composition(error.into())
    }
}

/// Placement as comparable bits; never equality of pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlacementKey {
    target: Rect,
    clip: Option<Rect>,
    transform: [u32; 9],
    alpha: u32,
    sampling: sophia_engine::HeadSamplingClass,
}

impl From<LiveCompositionPlacement> for PlacementKey {
    fn from(placement: LiveCompositionPlacement) -> Self {
        Self {
            target: placement.target,
            clip: placement.clip,
            transform: placement.transform.matrix.map(f32::to_bits),
            alpha: placement.alpha.to_bits(),
            sampling: placement.sampling,
        }
    }
}

/// A CPU layer's render metadata: the buffer's own version, never its bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CpuKey {
    placement: PlacementKey,
    handle: u64,
    size: Size,
    stride: u32,
    format: u32,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LayerKey {
    Cpu(CpuKey, Option<std::num::NonZeroU64>),
    Solid(Rect, sophia_engine::CompositorRgb8),
    /// A class eligibility refuses, compared by kind alone.
    Other(std::mem::Discriminant<LiveOwnedMixedCompositionLayer>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageKey {
    index: usize,
    node: sophia_engine::CompositorNodeId,
    resource: CompositorImageSourceIdentity,
    render: CpuKey,
    /// One of the candidate's own occurrences.
    candidate: bool,
}

/// The explicit metadata a combined job's rendered frame was composed from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CombinedSceneKey {
    head: sophia_engine::RenderHeadId,
    native_owner: crate::NativeFrameOwner,
    target_generation: u64,
    mapping: sophia_protocol::OutputHeadMapping,
    content_layout_generation: u64,
    cursor: Option<Option<(u64, u64)>>,
    plan_cursor: Option<sophia_engine::OutputSceneCursor>,
    installed: Vec<InstalledContentKey>,
    candidate_revoked: bool,
    /// Every lowered layer but the candidate's, in order.
    layers: Vec<LayerKey>,
    images: Vec<ImageKey>,
}

fn image_render(image: &LiveImageOccurrence) -> CpuKey {
    CpuKey {
        placement: image.placement.into(),
        handle: image.handle,
        size: image.size,
        stride: image.stride,
        format: image.format,
        generation: image.generation,
    }
}

fn layer_key(layer: &LiveOwnedMixedCompositionLayer) -> LayerKey {
    match layer {
        LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } => LayerKey::Cpu(
            CpuKey {
                placement: (*placement).into(),
                handle: buffer.handle,
                size: buffer.size,
                stride: buffer.stride,
                format: buffer.format,
                generation: buffer.generation,
            },
            buffer.pixel_version,
        ),
        LiveOwnedMixedCompositionLayer::Solid { geometry, color } => {
            LayerKey::Solid(*geometry, *color)
        }
        other => LayerKey::Other(std::mem::discriminant(other)),
    }
}

/// F4 and R5 before any reservation: the composition's metadata and the
/// owner's capture and worker facts.
pub(crate) fn combined_eligibility(
    frames: &[LiveProvisionalHeadFrame],
    candidate: &LiveShellContentFrame,
    owner: CombinedOwnerState,
) -> Result<(), CombinedIneligible> {
    let [frame] = frames else {
        return Err(CombinedIneligible::HeadCount);
    };
    if owner.capture_active {
        return Err(CombinedIneligible::CaptureActive);
    }
    if !owner.worker_idle {
        return Err(CombinedIneligible::WorkerBusy);
    }
    if frame.cursor == CombinedCursor::Software || frame.plan_cursor.is_some() {
        return Err(CombinedIneligible::SoftwareCursor);
    }
    if !candidate.images.iter().all(|image| {
        matches!(
            image.resource,
            sophia_engine::CompositorImageSource::Provisional(_)
        )
    }) {
        return Err(CombinedIneligible::CandidateNotProvisional);
    }
    let composition = &frame.composition;
    if !composition.frame.image_reads.is_empty() {
        return Err(CombinedIneligible::ImageReads);
    }
    for layer in &composition.frame.layers {
        match layer {
            LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } => {
                if buffer.pixel_version.is_none()
                    || !matches!(
                        buffer.bytes,
                        LiveCpuPixelStorage::Shared(_)
                            | LiveCpuPixelStorage::Content(_)
                            | LiveCpuPixelStorage::Bytes(_)
                            | LiveCpuPixelStorage::Lock(_)
                    )
                {
                    return Err(CombinedIneligible::UnversionedPixels);
                }
            }
            LiveOwnedMixedCompositionLayer::Solid { .. } => {}
            _ => return Err(CombinedIneligible::LayerKind),
        }
    }
    // A layer's position before any placeholder is bound.
    let mut placeholders_before = 0;
    for image in &composition.images {
        if image.provisional {
            placeholders_before += 1;
            continue;
        }
        let lowered = composition
            .frame
            .layers
            .get(image.index - placeholders_before);
        let leased = match (&image.resource, lowered) {
            (
                CompositorImageSourceIdentity::Shell(_),
                Some(LiveOwnedMixedCompositionLayer::Cpu { buffer, .. }),
            ) => matches!(buffer.bytes, LiveCpuPixelStorage::Content(_)),
            (CompositorImageSourceIdentity::Lock(_), Some(_)) => true,
            _ => false,
        };
        if !leased {
            return Err(CombinedIneligible::InstalledNotLeased);
        }
    }
    Ok(())
}

impl CombinedSceneKey {
    /// The key of one head's frame. `candidate` names the candidate's
    /// resources, so a recomposition with its real frame marks the same
    /// occurrences a provisional composition marked as placeholders.
    pub(crate) fn of(
        frame: &LiveProvisionalHeadFrame,
        candidate: &[CompositorImageSourceIdentity],
    ) -> Self {
        let composition = &frame.composition;
        let images = composition
            .images
            .iter()
            .map(|image| ImageKey {
                index: image.index,
                node: image.node,
                resource: image.resource.clone(),
                render: image_render(image),
                candidate: image.provisional || candidate.contains(&image.resource),
            })
            .collect::<Vec<_>>();
        // Final indices: a placeholder holds one without a lowered layer,
        // and a real candidate layer is left out so both keys align.
        let placeholder = |index: usize| {
            composition
                .images
                .iter()
                .any(|image| image.provisional && image.index == index)
        };
        let candidate_layer = |index: usize| {
            images
                .iter()
                .any(|image| image.candidate && image.index == index)
        };
        let mut layers = Vec::with_capacity(composition.frame.layers.len());
        let mut index = 0;
        for layer in &composition.frame.layers {
            while placeholder(index) {
                index += 1;
            }
            if !candidate_layer(index) {
                layers.push(layer_key(layer));
            }
            index += 1;
        }
        Self {
            head: frame.head,
            native_owner: frame.native_owner,
            target_generation: frame.target_generation,
            mapping: frame.mapping,
            content_layout_generation: frame.content_layout_generation,
            cursor: frame.cursor.key(),
            plan_cursor: frame.plan_cursor,
            installed: frame.installed.clone(),
            candidate_revoked: frame.candidate_revoked,
            layers,
            images,
        }
    }

    /// Whether the frame rendered from `self` (the dispatch key) still
    /// describes the scene that `current` (the consumption key) was
    /// recomposed from. Only the candidate's pixel storage and version may
    /// differ, and those are not in an image key.
    pub(crate) fn recompose(&self, current: &Self) -> CombinedRecompose {
        let topology = |key: &Self| {
            (
                key.head,
                key.native_owner,
                key.target_generation,
                key.mapping,
            )
        };
        if topology(self) != topology(current) {
            return CombinedRecompose::TopologyChange;
        }
        // The candidate's own facts, apart from where other layers put it.
        let candidate = |key: &Self| {
            key.images
                .iter()
                .filter(|image| image.candidate)
                .map(|image| (image.node, image.resource.clone(), image.render))
                .collect::<Vec<_>>()
        };
        if current.candidate_revoked || candidate(self) != candidate(current) {
            return CombinedRecompose::CandidateFailure;
        }
        if self != current {
            return CombinedRecompose::SceneChurn;
        }
        CombinedRecompose::Match
    }

    /// Classifies the recomposition at consumption against this dispatch
    /// key (F1, R4). An unclassified composition failure is returned, as the
    /// ordinary repaint of the same state would fail with it.
    pub(crate) fn classify(
        &self,
        current: Result<Vec<LiveProvisionalHeadFrame>, ProvisionalComposeError>,
        candidate: &[CompositorImageSourceIdentity],
    ) -> Result<CombinedRecompose, Box<dyn std::error::Error>> {
        match current {
            Ok(frames) => match &frames[..] {
                [frame] => Ok(self.recompose(&Self::of(frame, candidate))),
                // The output now maps to another number of heads.
                _ => Ok(CombinedRecompose::TopologyChange),
            },
            Err(ProvisionalComposeError::Topology) => Ok(CombinedRecompose::TopologyChange),
            // A raster or translucent rect the scene now needs is not the
            // candidate's; it is neither rasterized nor taken as a match.
            Err(ProvisionalComposeError::Declined(
                LiveProvisionalDecline::ColdTextRaster
                | LiveProvisionalDecline::ColdIndicatorRaster
                | LiveProvisionalDecline::TranslucentRect,
            )) => Ok(CombinedRecompose::SceneChurn),
            Err(ProvisionalComposeError::Declined(LiveProvisionalDecline::LayerIndexOverflow)) => {
                Err("provisional composition overflowed its layer index".into())
            }
            Err(ProvisionalComposeError::Composition(error)) => Err(error),
        }
    }
}

impl LiveProductionVisualRuntime {
    /// Composes `output` as it would look with `candidate` installed at
    /// `layer`, every head through the ordinary projection and the metadata
    /// lowering. Read-only: the installed content, caches, history and
    /// renderer-image reads are untouched. `candidate` may carry provisional images (at dispatch) or
    /// its real leases (at consumption), and `layer` need not hold a frame.
    pub(crate) fn compose_provisional_head_frames<T: NativeCompositionTarget>(
        &self,
        native_scanout: &T,
        scene: &LiveProductionCpuScene,
        layer: LiveShellContentLayer,
        candidate: &LiveShellContentFrame,
        cursor: CombinedCursor,
    ) -> Result<Vec<LiveProvisionalHeadFrame>, ProvisionalComposeError> {
        let output = candidate.output;
        let viewport = self
            .outputs
            .logical_viewport(output)
            .ok_or(ProvisionalComposeError::Topology)?;
        // Metadata only: no read custody and no cloned DMA-BUF (R5).
        let source_set = self.retained_composition_sources(scene, None)?;
        let key = (output, layer);
        let installed_keys = self
            .shell_content
            .iter()
            .filter(|(other, _)| other.0 == output && **other != key)
            .map(|((_, layer), content)| InstalledContentKey {
                layer: *layer,
                grant: content.frame.grant,
                transform: content.transform.clone(),
                interaction_revoked: content.interaction_revoked,
            })
            .collect::<Vec<_>>();
        let candidate_revoked = self.shell_content.get(&key).is_some_and(|content| {
            content.interaction_revoked && content.frame.grant == candidate.grant
        });
        let mut shell_content = self.shell_content.clone();
        let installed = shell_content.remove(&key);
        shell_content.insert(
            key,
            AdmittedShellContent {
                frame: candidate.clone(),
                retained_content_epoch: installed
                    .as_ref()
                    .and_then(|content| content.retained_content_epoch),
                interaction_revoked: installed
                    .as_ref()
                    .is_some_and(|content| content.interaction_revoked),
                // Display lists read only the images; a layer with no
                // installed frame has no committed transform yet.
                transform: installed.map_or(
                    sophia_engine::PresentedContentTransform {
                        viewport,
                        layout_generation: 0,
                    },
                    |content| content.transform,
                ),
            },
        );
        let composition = super::super::output_composition::OutputComposition {
            shell_content: &shell_content,
            ..self.output_composition()
        };
        let owned = self.drawable_order(
            output,
            self.surface_order_for_output(output, &source_set.presentation_order),
            &source_set.committed,
            None,
        );
        let display_list =
            composition.display_list(output, viewport, &source_set.committed, &owned)?;
        let (presented, display_list) = self.translations.project(
            output,
            &source_set.committed,
            display_list,
            self.translation_time(),
        );
        let snapshot = sophia_engine::output_scene_snapshot_from_committed_in_view(
            output,
            source_set.scene_generation.max(1),
            viewport,
            &presented,
            display_list,
            None,
        )?;
        let targets = native_scanout.head_targets(output);
        if targets.is_empty() {
            return Err(ProvisionalComposeError::Topology);
        }
        let plans = sophia_engine::build_output_head_plans(&snapshot, &targets)?;
        if plans.len() != targets.len() {
            return Err(ProvisionalComposeError::Topology);
        }
        let prepared = self.present_scheduler.in_flight_prepared();
        plans
            .iter()
            .map(|plan| {
                let mut composition =
                    sophia_renderer_live::lower_provisional_head_composition_plan(
                        plan,
                        &source_set.sources,
                        &self.indicator_strip_cache.borrow(),
                        &self.text_cache.borrow(),
                    )
                    .map_err(|error| match error {
                        LiveHeadCompositionLoweringError::ProvisionalIneligible(decline) => {
                            ProvisionalComposeError::Declined(decline)
                        }
                        error => ProvisionalComposeError::Composition(error.into()),
                    })?;
                self.attach_frame_damage_history(
                    &mut composition.frame,
                    plan,
                    &source_set.committed,
                    prepared,
                )?;
                Ok(LiveProvisionalHeadFrame {
                    head: plan.head,
                    native_owner: native_scanout.frame_owner(),
                    target_generation: plan.target_generation,
                    mapping: plan.mapping,
                    content_layout_generation: self.content_layout_generation,
                    cursor,
                    plan_cursor: plan.cursor,
                    installed: installed_keys.clone(),
                    candidate_revoked,
                    composition,
                })
            })
            .collect()
    }
}
