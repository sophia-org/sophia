//! G3-3: a combined frame job's provisional composition and its scene key.
//!
//! The candidate's images enter the real display list as
//! `CompositorImageSource::Provisional`, through the same output
//! composition, translation, head plans and damage snapshot as an ordinary
//! repaint, and the metadata lowering records each one as a placeholder at
//! its final layer index. Nothing installed is written, no cache is filled or
//! reordered, and no pixel is read or produced. At consumption the same path
//! runs with the candidate's real frame, and the two keys decide whether the
//! rendered frame still describes the scene.

// No production caller until the owner turn consumes this (G3-4/G3-5); only lifecycle tests reach it.
#![cfg_attr(not(test), allow(dead_code))]

use super::*;
use sophia_engine::CompositorImageSourceIdentity;
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveCpuPixelStorage, LiveImageOccurrence,
    LiveOwnedMixedCompositionLayer, LiveProvisionalComposition,
};

/// One head's provisional frame for a combined job's candidate.
pub(crate) struct LiveProvisionalHeadFrame {
    pub head: sophia_engine::RenderHeadId,
    pub native_owner: crate::NativeFrameOwner,
    pub target_generation: u64,
    pub mapping: sophia_protocol::OutputHeadMapping,
    pub content_layout_generation: u64,
    /// The plan's software cursor, drawn into the frame.
    pub cursor: Option<sophia_engine::OutputSceneCursor>,
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
    /// A layer other than CPU pixels or a solid fill.
    LayerKind,
    ImageReads,
    /// A CPU layer whose pixels carry no version, or a validated binding.
    UnversionedPixels,
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
    cursor: Option<sophia_engine::OutputSceneCursor>,
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

fn layer_key(layer: &LiveOwnedMixedCompositionLayer) -> Option<LayerKey> {
    match layer {
        LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } => Some(LayerKey::Cpu(
            CpuKey {
                placement: (*placement).into(),
                handle: buffer.handle,
                size: buffer.size,
                stride: buffer.stride,
                format: buffer.format,
                generation: buffer.generation,
            },
            buffer.pixel_version,
        )),
        LiveOwnedMixedCompositionLayer::Solid { geometry, color } => {
            Some(LayerKey::Solid(*geometry, *color))
        }
        _ => None,
    }
}

/// F4 and R5 from metadata alone, before any reservation. The capture and
/// idle-worker conditions are the owner's and are checked where it reserves.
pub(crate) fn combined_eligibility(
    frames: &[LiveProvisionalHeadFrame],
    candidate: &LiveShellContentFrame,
) -> Result<(), CombinedIneligible> {
    let [frame] = frames else {
        return Err(CombinedIneligible::HeadCount);
    };
    if frame.cursor.is_some() {
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
    ) -> Option<Self> {
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
                layers.push(layer_key(layer)?);
            }
            index += 1;
        }
        Some(Self {
            head: frame.head,
            native_owner: frame.native_owner,
            target_generation: frame.target_generation,
            mapping: frame.mapping,
            content_layout_generation: frame.content_layout_generation,
            cursor: frame.cursor,
            installed: frame.installed.clone(),
            candidate_revoked: frame.candidate_revoked,
            layers,
            images,
        })
    }

    /// Whether the frame rendered from `self` (the dispatch key) still
    /// describes the scene that `current` (the consumption key) was
    /// recomposed from. Only the candidate's pixel storage and version may
    /// differ, and those are not in an image key.
    pub(crate) fn recompose(&self, current: Option<&Self>) -> CombinedRecompose {
        let Some(current) = current else {
            return CombinedRecompose::TopologyChange;
        };
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
}

impl LiveProductionVisualRuntime {
    /// Composes `output` as it would look with `candidate` installed at
    /// `layer`, every head through the ordinary projection and the metadata
    /// lowering. Read-only: the installed content, caches and history are
    /// untouched. `candidate` may carry provisional images (at dispatch) or
    /// its real leases (at consumption), and `layer` need not hold a frame.
    pub(crate) fn compose_provisional_head_frames<T: NativeCompositionTarget>(
        &self,
        native_scanout: &T,
        scene: &LiveProductionCpuScene,
        layer: LiveShellContentLayer,
        candidate: &LiveShellContentFrame,
    ) -> Result<Vec<LiveProvisionalHeadFrame>, Box<dyn std::error::Error>> {
        let output = candidate.output;
        let viewport = self
            .outputs
            .logical_viewport(output)
            .ok_or("provisional composition targets an unknown logical output")?;
        let source_set = self.retained_composition_source_set(scene, None)?;
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
        let plans = sophia_engine::build_output_head_plans(&snapshot, &targets)?;
        if plans.len() != targets.len() {
            return Err("head composition planner returned partial target coverage".into());
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
                    )?;
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
                    cursor: plan.cursor,
                    installed: installed_keys.clone(),
                    candidate_revoked,
                    composition,
                })
            })
            .collect()
    }
}
