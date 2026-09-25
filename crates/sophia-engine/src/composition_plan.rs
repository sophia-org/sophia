use crate::prelude::*;
use crate::{
    CompositorBorder, CompositorContentImage, CompositorDisplayCommand, CompositorDisplayList,
    CompositorIndicatorStrip, CompositorNodeId, CompositorRect, CompositorRgb8,
    CompositorSolidRect, CompositorText, HeadRenderTarget, HeadlessOutput, IndicatorChromeStrip,
    OutputFrameDamageSnapshot, OutputFrameSurfaceState, RenderHeadId,
    compositor_display_list_structure_is_valid,
};

pub const MAX_HEAD_COMPOSITION_LAYERS: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputSceneSurface {
    pub surface: SurfaceId,
    pub committed_generation: u64,
    pub geometry: Rect,
    pub clip: Rect,
    pub opacity_millis: u16,
    pub content: SurfaceContentSet,
    pub damage: Region,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputSceneCursor {
    pub geometry: Rect,
    pub source: BufferSource,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputSceneSnapshot {
    pub output: OutputId,
    pub scene_generation: u64,
    /// Root-space logical viewport. Surface, cursor, and damage geometry use
    /// the same space and are localized only when a head plan is built.
    pub logical_viewport: Rect,
    pub surfaces: Vec<OutputSceneSurface>,
    pub display_list: CompositorDisplayList,
    pub cursor: Option<OutputSceneCursor>,
    pub logical_damage: Region,
    pub logical_content_checksum: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeadSamplingClass {
    Exact,
    Downsampled,
    Upsampled,
    /// One axis reduces while the other enlarges.
    Mixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeadBindingOutcome {
    Active,
    Fallback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadLayerBinding {
    pub surface: SurfaceId,
    pub committed_generation: u64,
    pub variant: u32,
    pub source: BufferSource,
    pub source_pixel_size: Size,
    pub density_millis: u32,
    pub opacity_millis: u16,
    /// Original root-space placement for input after this frame retires.
    /// Preserve it through native scaling instead of inverting rounded pixels.
    pub logical_geometry: Rect,
    pub native_geometry: Rect,
    pub native_clip: Rect,
    pub requested_sampling: HeadSamplingClass,
    pub outcome: HeadBindingOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadLogicalTransform {
    pub source: Size,
    pub projected_scene: Rect,
}

impl HeadLogicalTransform {
    pub fn project_local_rect(self, rect: Rect) -> Rect {
        project_child_rect(rect, self.source, self.projected_scene)
    }

    pub fn project_root_rect(self, viewport: Rect, rect: Rect) -> Rect {
        self.project_local_rect(Rect {
            x: rect.x.saturating_sub(viewport.x),
            y: rect.y.saturating_sub(viewport.y),
            width: rect.width,
            height: rect.height,
        })
    }

    pub fn unproject_point(self, x: i32, y: i32) -> Option<(i32, i32)> {
        if self.source.width <= 0
            || self.source.height <= 0
            || self.projected_scene.width <= 0
            || self.projected_scene.height <= 0
        {
            return None;
        }
        let local_x = i64::from(x).checked_sub(i64::from(self.projected_scene.x))?;
        let local_y = i64::from(y).checked_sub(i64::from(self.projected_scene.y))?;
        let source_x = local_x
            .checked_mul(i64::from(self.source.width))?
            .checked_div(i64::from(self.projected_scene.width))?;
        let source_y = local_y
            .checked_mul(i64::from(self.source.height))?
            .checked_div(i64::from(self.projected_scene.height))?;
        Some((i32::try_from(source_x).ok()?, i32::try_from(source_y).ok()?))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadCompositorBorder {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub outer: Rect,
    pub inner: Rect,
    pub color: CompositorRgb8,
    /// The head-native region these bands may paint into.
    ///
    /// Carried rather than applied here because the four bands are the
    /// difference between `outer` and `inner`, and clipping those two is not the
    /// same operation as clipping what they produce. Where the clip leaves them
    /// degenerate the subtraction is still positive, so a window lying entirely
    /// outside this scene would keep a band at its original off-screen
    /// coordinates. The consumer derives the bands from the full rects and
    /// clips each one.
    pub clip: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadCompositorIndicatorStrip {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub strip: IndicatorChromeStrip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadCompositorRect {
    pub opacity: u8,
    pub node: CompositorNodeId,
    pub generation: u64,
    pub geometry: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadCompositorText {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub geometry: Rect,
    pub text: String,
    pub font_size_millis: u32,
    pub color: CompositorRgb8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadCompositorContentImage {
    pub image: CompositorContentImage,
    pub geometry: Rect,
    pub clip: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeadCompositorCommand {
    Background(CompositorSolidRect),
    Surface {
        surface: SurfaceId,
    },
    /// An instance in head-native coordinates: destination and clip are
    /// projected, the clip bounded by the painted scene.
    SurfaceInstance(crate::CompositorSurfaceInstance),
    /// Carried unchanged into the head frame; draws nothing.
    PresentationStamp(crate::CompositorPresentationStamp),
    Border(HeadCompositorBorder),
    Rect(HeadCompositorRect),
    Text(HeadCompositorText),
    IndicatorStrip(HeadCompositorIndicatorStrip),
    ContentImage(HeadCompositorContentImage),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadCompositionPlan {
    pub output: OutputId,
    pub scene_generation: u64,
    pub head: RenderHeadId,
    pub target_generation: u64,
    pub native_size: Size,
    pub target_transform: OutputTransform,
    pub mapping: OutputHeadMapping,
    pub transform: HeadLogicalTransform,
    pub layers: Vec<HeadLayerBinding>,
    pub compositor: Vec<HeadCompositorCommand>,
    pub cursor: Option<OutputSceneCursor>,
    pub repaint: Region,
    pub logical_content_checksum: u64,
    /// Whether this exact frame needs no composition to reach the screen.
    pub direct_scanout: DirectScanoutVerdict,
}

include!("composition_plan/direct_scanout.rs");

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HeadFrameCandidateId(u64);

impl HeadFrameCandidateId {
    pub const INVALID: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadFrameCandidate {
    pub candidate: HeadFrameCandidateId,
    pub output: OutputId,
    pub scene_generation: u64,
    pub head: RenderHeadId,
    pub target_generation: u64,
    pub logical_content_checksum: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeadCompositionPlanError {
    InvalidSnapshot,
    InvalidTarget,
    UnsupportedTargetTransform,
    UnavailableSurfaceVariant,
    WrongOutput,
    LayerCapacityExceeded,
    DuplicateSurface,
    MissingDisplaySurface,
    EmptyProjection,
    /// An instance's resolved source generation is not its source's
    /// committed generation in this snapshot.
    StaleInstanceSource,
}

impl fmt::Display for HeadCompositionPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for HeadCompositionPlanError {}

pub fn build_output_head_plans(
    snapshot: &OutputSceneSnapshot,
    targets: &[HeadRenderTarget],
) -> Result<Vec<HeadCompositionPlan>, HeadCompositionPlanError> {
    if targets.is_empty()
        || targets
            .iter()
            .any(|target| target.output != snapshot.output)
    {
        return Err(HeadCompositionPlanError::WrongOutput);
    }
    targets
        .iter()
        .map(|target| build_head_composition_plan(snapshot, *target))
        .collect()
}

/// Captures one immutable logical-output scene from the already committed
/// Engine surface slice. This is the production fan-out seam: callers take one
/// read, then derive every physical-head plan from this value rather than
/// asking mutable runtime state again per head.
pub fn output_scene_snapshot_from_committed(
    output: HeadlessOutput,
    scene_generation: u64,
    committed: &[CommittedSurfaceState],
    display_list: CompositorDisplayList,
    cursor: Option<OutputSceneCursor>,
) -> Result<OutputSceneSnapshot, HeadCompositionPlanError> {
    let scale = i32::try_from(output.scale)
        .ok()
        .filter(|scale| *scale > 0)
        .ok_or(HeadCompositionPlanError::InvalidSnapshot)?;
    let logical_size = Size {
        width: ceil_div_positive(output.size.width, scale)
            .ok_or(HeadCompositionPlanError::InvalidSnapshot)?,
        height: ceil_div_positive(output.size.height, scale)
            .ok_or(HeadCompositionPlanError::InvalidSnapshot)?,
    };
    output_scene_snapshot_from_committed_in_view(
        output.id,
        scene_generation,
        Rect {
            x: 0,
            y: 0,
            width: logical_size.width,
            height: logical_size.height,
        },
        committed,
        display_list,
        cursor,
    )
}

/// Captures the portion of a root-space committed scene visible through one
/// logical output viewport. Extended outputs can therefore share one committed
/// surface snapshot without treating off-output surfaces as invalid content.
pub fn output_scene_snapshot_from_committed_in_view(
    output: OutputId,
    scene_generation: u64,
    logical_viewport: Rect,
    committed: &[CommittedSurfaceState],
    mut display_list: CompositorDisplayList,
    cursor: Option<OutputSceneCursor>,
) -> Result<OutputSceneSnapshot, HeadCompositionPlanError> {
    if !output.is_valid() || logical_viewport.is_empty() || scene_generation == 0 {
        return Err(HeadCompositionPlanError::InvalidSnapshot);
    }
    display_list.output = output;
    let displayed_surfaces = display_list
        .commands
        .iter()
        .filter_map(|command| match command {
            CompositorDisplayCommand::Surface { surface } => Some(*surface),
            CompositorDisplayCommand::SurfaceInstance(_)
            | CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
        .collect::<BTreeSet<_>>();
    // Sources an instance samples on this output, whether or not the source
    // is itself presented here: a preview-only source is sampled all the
    // same, and its own placement is not drawn.
    let instance_sources = display_list
        .surface_instances()
        .filter(|instance| !intersect_rect(instance.visible(), logical_viewport).is_empty())
        .map(|instance| instance.source)
        .collect::<BTreeSet<_>>();
    let mut logical_damage = Region::empty();
    let mut visible_surfaces = BTreeSet::new();
    let surfaces = committed
        .iter()
        .filter_map(|state| {
            let own = intersect_rect(state.geometry, logical_viewport);
            let shown = displayed_surfaces.contains(&state.surface) && !own.is_empty();
            if !shown && !instance_sources.contains(&state.surface) {
                return None;
            }
            let clip = if shown {
                visible_surfaces.insert(state.surface);
                if !state.damage.is_empty() {
                    logical_damage.rects.push(own);
                }
                own
            } else {
                state.geometry
            };
            Some(OutputSceneSurface {
                surface: state.surface,
                committed_generation: state.committed_generation,
                geometry: state.geometry,
                clip,
                opacity_millis: 1_000,
                content: state.content.clone(),
                damage: state.damage.clone(),
            })
        })
        .collect::<Vec<_>>();
    let sampled_sources = surfaces
        .iter()
        .filter(|surface| !surface.damage.is_empty())
        .map(|surface| surface.surface)
        .collect::<BTreeSet<_>>();
    // A source's content change repaints every instance of it here, while
    // the instance's own interaction generation stays as it was.
    for instance in display_list.surface_instances() {
        let visible = intersect_rect(instance.visible(), logical_viewport);
        if !visible.is_empty() && sampled_sources.contains(&instance.source) {
            logical_damage.rects.push(visible);
        }
    }
    display_list.commands.retain(|command| match command {
        CompositorDisplayCommand::Surface { surface } => visible_surfaces.contains(surface),
        CompositorDisplayCommand::PresentationStamp(_) => true,
        CompositorDisplayCommand::SurfaceInstance(instance) => {
            instance_sources.contains(&instance.source)
                && !intersect_rect(instance.visible(), logical_viewport).is_empty()
        }
        CompositorDisplayCommand::Border(border) => {
            !intersect_rect(border.outer, logical_viewport).is_empty()
        }
        CompositorDisplayCommand::Rect(rect) => {
            !intersect_rect(rect.geometry, logical_viewport).is_empty()
        }
        CompositorDisplayCommand::Text(text) => {
            !intersect_rect(text.geometry, logical_viewport).is_empty()
        }
        CompositorDisplayCommand::IndicatorStrip(strip) => {
            !intersect_rect(strip.strip.geometry, logical_viewport).is_empty()
        }
        // Content geometry is output-local physical, so it is clipped when a
        // head plan knows the exact target rather than against this logical
        // viewport.
        CompositorDisplayCommand::ContentImage(_) => true,
    });
    let cursor =
        cursor.filter(|cursor| !intersect_rect(cursor.geometry, logical_viewport).is_empty());
    let logical_content_checksum = logical_scene_checksum(&surfaces, &display_list, cursor);
    let snapshot = OutputSceneSnapshot {
        output,
        scene_generation,
        logical_viewport,
        surfaces,
        display_list,
        cursor,
        logical_damage,
        logical_content_checksum,
    };
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

/// Computes the exact logical-output retirement set for one root-space scene
/// change. Both old and new geometry participate so movement cannot publish on
/// the destination while leaving stale pixels on the source output.
pub fn applicable_output_retirement_set(
    logical_viewports: &[(OutputId, Rect)],
    previous_geometry: Option<Rect>,
    current_geometry: Rect,
) -> Result<Vec<OutputId>, HeadCompositionPlanError> {
    if logical_viewports.is_empty()
        || logical_viewports.len() > crate::MAX_DRM_KMS_OUTPUTS
        || current_geometry.is_empty()
        || previous_geometry.is_some_and(|geometry| geometry.is_empty())
    {
        return Err(HeadCompositionPlanError::InvalidSnapshot);
    }
    let mut seen = BTreeSet::new();
    let mut applicable = BTreeSet::new();
    for (output, viewport) in logical_viewports {
        if !output.is_valid() || viewport.is_empty() || !seen.insert(*output) {
            return Err(HeadCompositionPlanError::InvalidSnapshot);
        }
        if !intersect_rect(*viewport, current_geometry).is_empty()
            || previous_geometry
                .is_some_and(|geometry| !intersect_rect(*viewport, geometry).is_empty())
        {
            applicable.insert(*output);
        }
    }
    Ok(applicable.into_iter().collect())
}

pub fn build_head_composition_plan(
    snapshot: &OutputSceneSnapshot,
    target: HeadRenderTarget,
) -> Result<HeadCompositionPlan, HeadCompositionPlanError> {
    validate_snapshot(snapshot)?;
    if !target.head.is_valid()
        || !target.output.is_valid()
        || target.target_generation == 0
        || target.native_size.width <= 0
        || target.native_size.height <= 0
        || target.scale == 0
        || target.refresh_millihz == 0
    {
        return Err(HeadCompositionPlanError::InvalidTarget);
    }
    if target.transform != OutputTransform::Normal {
        return Err(HeadCompositionPlanError::UnsupportedTargetTransform);
    }
    if target.output != snapshot.output {
        return Err(HeadCompositionPlanError::WrongOutput);
    }

    let source = Size {
        width: snapshot.logical_viewport.width,
        height: snapshot.logical_viewport.height,
    };
    let projected_scene = project_scene(source, target.native_size, target.mapping);
    if projected_scene.is_empty() {
        return Err(HeadCompositionPlanError::EmptyProjection);
    }
    let transform = HeadLogicalTransform {
        source,
        projected_scene,
    };
    let target_density = projected_density_millis(transform);
    // Everything the scene draws is bounded by this, not by the framebuffer.
    let painted = scene_clip(projected_scene, target.native_size);
    let mut layers = Vec::with_capacity(snapshot.surfaces.len());
    for surface in &snapshot.surfaces {
        let variant = select_variant(&surface.content, target_density)
            .ok_or(HeadCompositionPlanError::UnavailableSurfaceVariant)?;
        let native_geometry =
            transform.project_root_rect(snapshot.logical_viewport, surface.geometry);
        let requested_sampling = head_sampling_class(
            variant.pixel_size,
            Size {
                width: native_geometry.width,
                height: native_geometry.height,
            },
        );
        layers.push(HeadLayerBinding {
            surface: surface.surface,
            committed_generation: surface.committed_generation,
            logical_geometry: surface.geometry,
            variant: variant.variant,
            source: variant.source,
            source_pixel_size: variant.pixel_size,
            density_millis: variant.density_millis,
            opacity_millis: surface.opacity_millis,
            native_geometry,
            native_clip: intersect_rect(
                transform.project_root_rect(snapshot.logical_viewport, surface.clip),
                painted,
            ),
            requested_sampling,
            outcome: if variant.fidelity == sophia_protocol::SurfaceContentFidelity::AuthorityRaster
            {
                HeadBindingOutcome::Active
            } else {
                HeadBindingOutcome::Fallback
            },
        });
    }

    let mut compositor = background_commands(projected_scene, target.native_size);
    let mut instance_sampled = false;
    for command in &snapshot.display_list.commands {
        compositor.push(match command {
            CompositorDisplayCommand::Surface { surface } => {
                HeadCompositorCommand::Surface { surface: *surface }
            }
            CompositorDisplayCommand::PresentationStamp(stamp) => {
                HeadCompositorCommand::PresentationStamp(crate::CompositorPresentationStamp {
                    coverage: intersect_rect(
                        transform.project_root_rect(snapshot.logical_viewport, stamp.coverage),
                        painted,
                    ),
                    ..*stamp
                })
            }
            CompositorDisplayCommand::SurfaceInstance(instance) => {
                let destination =
                    transform.project_root_rect(snapshot.logical_viewport, instance.destination);
                let clip = intersect_rect(
                    transform.project_root_rect(snapshot.logical_viewport, instance.clip),
                    painted,
                );
                if intersect_rect(destination, clip).is_empty() {
                    // Rounded away on this head: nothing to draw or damage.
                    continue;
                }
                if let Some(layer) = layers.iter().find(|layer| layer.surface == instance.source) {
                    instance_sampled |= head_sampling_class(
                        layer.source_pixel_size,
                        Size {
                            width: destination.width,
                            height: destination.height,
                        },
                    ) != HeadSamplingClass::Exact;
                }
                HeadCompositorCommand::SurfaceInstance(crate::CompositorSurfaceInstance {
                    destination,
                    clip,
                    ..*instance
                })
            }
            CompositorDisplayCommand::Border(border) => HeadCompositorCommand::Border(
                project_border(*border, snapshot.logical_viewport, transform, painted),
            ),
            CompositorDisplayCommand::Rect(rect) => HeadCompositorCommand::Rect(project_rect(
                *rect,
                snapshot.logical_viewport,
                transform,
                painted,
            )),
            CompositorDisplayCommand::Text(text) => HeadCompositorCommand::Text(project_text(
                text,
                snapshot.logical_viewport,
                transform,
                painted,
            )),
            CompositorDisplayCommand::IndicatorStrip(strip) => {
                HeadCompositorCommand::IndicatorStrip(project_indicator_strip(
                    strip,
                    snapshot.logical_viewport,
                    transform,
                    painted,
                ))
            }
            CompositorDisplayCommand::ContentImage(image) => {
                let physical = HeadLogicalTransform {
                    source: image.output_size_px,
                    projected_scene,
                };
                let geometry = physical.project_local_rect(image.geometry_px);
                HeadCompositorCommand::ContentImage(HeadCompositorContentImage {
                    image: image.clone(),
                    geometry,
                    clip: intersect_rect(geometry, painted),
                })
            }
        });
    }

    let cursor = snapshot.cursor.map(|cursor| OutputSceneCursor {
        geometry: intersect_rect(
            transform.project_root_rect(snapshot.logical_viewport, cursor.geometry),
            painted,
        ),
        ..cursor
    });
    let sampled = instance_sampled
        || layers
            .iter()
            .any(|layer| layer.requested_sampling != HeadSamplingClass::Exact);
    let mut repaint = Region::empty();
    for rect in &snapshot.logical_damage.rects {
        let projected = transform.project_root_rect(snapshot.logical_viewport, *rect);
        let expanded = if sampled {
            Rect {
                x: projected.x.saturating_sub(1),
                y: projected.y.saturating_sub(1),
                width: projected.width.saturating_add(2),
                height: projected.height.saturating_add(2),
            }
        } else {
            projected
        };
        repaint.push(clip_to_target(expanded, target.native_size));
    }

    let mut plan = HeadCompositionPlan {
        output: snapshot.output,
        scene_generation: snapshot.scene_generation,
        head: target.head,
        target_generation: target.target_generation,
        native_size: target.native_size,
        target_transform: target.transform,
        mapping: target.mapping,
        transform,
        layers,
        compositor,
        cursor,
        repaint,
        logical_content_checksum: snapshot.logical_content_checksum,
        direct_scanout: DirectScanoutVerdict::Eligible,
    };
    // Computed from the finished plan and stored on it, so eligibility is a
    // property of the exact frame that will be presented rather than of the
    // scene it was derived from.
    plan.direct_scanout = direct_scanout_verdict(&plan);
    Ok(plan)
}

/// Reduces a head-native plan into the existing per-output damage ledger
/// shape. The snapshot names selected variant sources and native geometry; it
/// is never cloned from another head's logical or physical output state.
pub fn head_output_damage_snapshot(plan: &HeadCompositionPlan) -> OutputFrameDamageSnapshot {
    let output = HeadlessOutput {
        id: plan.output,
        size: plan.native_size,
        scale: 1,
    };
    let mut surfaces = Vec::with_capacity(plan.layers.len());
    let mut display_list = CompositorDisplayList::empty(plan.output);
    for command in &plan.compositor {
        match command {
            HeadCompositorCommand::Background(_) => {}
            // An instance is compositor content keyed by its own node. It
            // never becomes a frame surface, so it can never become an
            // application input layer when this frame retires.
            HeadCompositorCommand::SurfaceInstance(instance) => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::SurfaceInstance(*instance));
            }
            HeadCompositorCommand::PresentationStamp(stamp) => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::PresentationStamp(*stamp));
            }
            HeadCompositorCommand::Surface { surface } => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::Surface { surface: *surface });
                if let Some(layer) = plan.layers.iter().find(|layer| layer.surface == *surface) {
                    surfaces.push(OutputFrameSurfaceState {
                        surface: *surface,
                        committed_generation: layer.committed_generation,
                        logical_geometry: layer.logical_geometry,
                        geometry: layer.native_geometry,
                        buffer: layer.source,
                        source_size: layer.source_pixel_size,
                    });
                }
            }
            HeadCompositorCommand::Border(border) => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::Border(CompositorBorder {
                        node: border.node,
                        generation: border.generation,
                        outer: border.outer,
                        inner: border.inner,
                        color: border.color,
                    }));
            }
            HeadCompositorCommand::Rect(rect) => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::Rect(CompositorRect {
                        opacity: rect.opacity,
                        node: rect.node,
                        generation: rect.generation,
                        geometry: rect.geometry,
                        color: rect.color,
                    }));
            }
            HeadCompositorCommand::Text(text) => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::Text(CompositorText {
                        node: text.node,
                        generation: text.generation,
                        geometry: text.geometry,
                        text: text.text.clone(),
                        font_size_millis: text.font_size_millis,
                        color: text.color,
                    }));
            }
            HeadCompositorCommand::IndicatorStrip(strip) => {
                display_list
                    .commands
                    .push(CompositorDisplayCommand::IndicatorStrip(
                        CompositorIndicatorStrip {
                            node: strip.node,
                            generation: strip.generation,
                            strip: strip.strip.clone(),
                        },
                    ));
            }
            HeadCompositorCommand::ContentImage(content) => {
                let mut image = content.image.clone();
                image.output_size_px = plan.native_size;
                image.geometry_px = content.geometry;
                display_list
                    .commands
                    .push(CompositorDisplayCommand::ContentImage(image));
            }
        }
    }
    OutputFrameDamageSnapshot {
        output,
        surfaces,
        compositor_display_list: display_list.into(),
        software_cursor: plan.cursor.map(|cursor| cursor.geometry),
    }
}

fn validate_snapshot(snapshot: &OutputSceneSnapshot) -> Result<(), HeadCompositionPlanError> {
    if !snapshot.output.is_valid()
        || snapshot.scene_generation == 0
        || snapshot.logical_viewport.is_empty()
        || snapshot.display_list.output != snapshot.output
        || snapshot
            .surfaces
            .len()
            .saturating_add(snapshot.display_list.surface_instances().count())
            > MAX_HEAD_COMPOSITION_LAYERS
        || !compositor_display_list_structure_is_valid(&snapshot.display_list)
    {
        return Err(HeadCompositionPlanError::InvalidSnapshot);
    }
    let mut surfaces = BTreeSet::new();
    for surface in &snapshot.surfaces {
        if !surface.surface.is_valid()
            || surface.committed_generation == 0
            || surface.geometry.is_empty()
            || surface.clip.is_empty()
            || surface.opacity_millis > 1_000
            || surface.content.logical_extent().width <= 0
            || surface.content.logical_extent().height <= 0
        {
            return Err(HeadCompositionPlanError::InvalidSnapshot);
        }
        if !surfaces.insert(surface.surface) {
            return Err(HeadCompositionPlanError::DuplicateSurface);
        }
    }
    let mut displayed = BTreeSet::new();
    for command in &snapshot.display_list.commands {
        match command {
            CompositorDisplayCommand::Surface { surface } => {
                if !surfaces.contains(surface) {
                    return Err(HeadCompositionPlanError::MissingDisplaySurface);
                }
                if !displayed.insert(*surface) {
                    return Err(HeadCompositionPlanError::DuplicateSurface);
                }
            }
            // Repeated sources are expected; each instance names its own.
            CompositorDisplayCommand::SurfaceInstance(instance) => {
                let Some(source) = snapshot
                    .surfaces
                    .iter()
                    .find(|surface| surface.surface == instance.source)
                else {
                    return Err(HeadCompositionPlanError::MissingDisplaySurface);
                };
                if source.committed_generation != instance.source_generation {
                    return Err(HeadCompositionPlanError::StaleInstanceSource);
                }
            }
            CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => {}
        }
    }
    Ok(())
}

include!("composition_plan/checksum.rs");

fn intersect_rect(first: Rect, second: Rect) -> Rect {
    let left = first.x.max(second.x);
    let top = first.y.max(second.y);
    let right = first
        .x
        .saturating_add(first.width)
        .min(second.x.saturating_add(second.width));
    let bottom = first
        .y
        .saturating_add(first.height)
        .min(second.y.saturating_add(second.height));
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(0),
        height: bottom.saturating_sub(top).max(0),
    }
}

fn select_variant(
    content: &SurfaceContentSet,
    target_density: u32,
) -> Option<&sophia_protocol::SurfaceContentVariant> {
    content
        .variants()
        .iter()
        .filter(|variant| variant.transform == sophia_protocol::SurfaceRasterTransform::Normal)
        .min_by_key(|variant| {
            let class = if variant.density_millis == target_density {
                0
            } else if variant.density_millis > target_density {
                1
            } else {
                2
            };
            let error = variant.density_millis.abs_diff(target_density);
            (class, error, variant.variant)
        })
}

/// Classifies the actual raster extent against the rectangle it will fill.
///
/// Density selects an authority variant; it does not say what the renderer
/// draws. A stale or inset raster may have the requested density while spanning
/// a different extent, so sampling evidence and filter damage must compare the
/// two pixel rectangles directly.
pub const fn head_sampling_class(source: Size, target: Size) -> HeadSamplingClass {
    let reduces = target.width < source.width || target.height < source.height;
    let enlarges = target.width > source.width || target.height > source.height;
    if !reduces && !enlarges {
        HeadSamplingClass::Exact
    } else if reduces && enlarges {
        HeadSamplingClass::Mixed
    } else if reduces {
        HeadSamplingClass::Downsampled
    } else {
        HeadSamplingClass::Upsampled
    }
}

include!("composition_plan/projection_geometry.rs");
