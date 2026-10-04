use crate::prelude::*;
use crate::{
    CompositorDamageList, CompositorDisplayCommand, CompositorDisplayList, HeadlessOutput,
    compositor_display_list_damage, compositor_display_list_structure_is_valid,
};

pub const MAX_OUTPUT_FRAME_SURFACES: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFrameSurfaceState {
    pub surface: SurfaceId,
    pub committed_generation: u64,
    /// Root-space placement for hit-testing this exact presented frame.
    pub logical_geometry: Rect,
    /// Placement in this snapshot's render coordinates; native for head frames.
    pub geometry: Rect,
    pub buffer: BufferSource,
    /// The raster's own pixel size, carried rather than re-derived from
    /// `geometry`: a surface presented before it answered a configure is placed
    /// at one size and drawn at another.
    pub source_size: Size,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFrameDamageSnapshot {
    pub damage_history:
        std::sync::Arc<[std::sync::Arc<super::damage_history::SurfaceDamageTransition>]>,
    pub output: HeadlessOutput,
    pub surfaces: Vec<OutputFrameSurfaceState>,
    pub compositor_display_list: CompositorDamageList,
    pub software_cursor: Option<Rect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFrameDamageError {
    InvalidOutput,
    InvalidOutputSize,
    OutputMismatch,
    InvalidSurface,
    DuplicateSurface,
    SurfaceCapacityExceeded,
    InvalidCompositorDisplayList,
}

impl fmt::Display for OutputFrameDamageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OutputFrameDamageError {}

/// Captures only immutable facts that can change pixels on one output.
///
/// Surface order follows the display list so stacking changes remain visible
/// to damage reduction. Protocol metadata and renderer-native resources never
/// enter this Engine record.
pub fn output_frame_damage_snapshot(
    output: HeadlessOutput,
    compositor_display_list: CompositorDisplayList,
    committed_surfaces: &[CommittedSurfaceState],
    software_cursor: Option<Rect>,
) -> Result<OutputFrameDamageSnapshot, OutputFrameDamageError> {
    validate_output(output)?;
    if compositor_display_list.output != output.id {
        return Err(OutputFrameDamageError::OutputMismatch);
    }
    if !compositor_display_list_structure_is_valid(&compositor_display_list) {
        return Err(OutputFrameDamageError::InvalidCompositorDisplayList);
    }
    // An instance's source generation must be the one this frame samples:
    // it is what repaints the instance when only the source's content
    // changed. Instances are not frame surfaces and never input layers.
    if compositor_display_list.surface_instances().any(|instance| {
        committed_surfaces
            .iter()
            .find(|state| state.surface == instance.source)
            .is_none_or(|state| state.committed_generation != instance.source_generation)
    }) {
        return Err(OutputFrameDamageError::InvalidCompositorDisplayList);
    }
    let mut surfaces = Vec::new();
    let mut seen = BTreeSet::new();
    for surface in compositor_display_list
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
    {
        if !surface.is_valid() {
            return Err(OutputFrameDamageError::InvalidSurface);
        }
        if !seen.insert(surface) {
            return Err(OutputFrameDamageError::DuplicateSurface);
        }
        let Some(committed) = committed_surfaces
            .iter()
            .find(|committed| committed.surface == surface)
        else {
            continue;
        };
        if surfaces.len() >= MAX_OUTPUT_FRAME_SURFACES {
            return Err(OutputFrameDamageError::SurfaceCapacityExceeded);
        }
        surfaces.push(OutputFrameSurfaceState {
            surface,
            committed_generation: committed.committed_generation,
            logical_geometry: committed.geometry,
            geometry: committed.geometry,
            buffer: committed.buffer(),
            source_size: committed.content.canonical_variant().pixel_size,
        });
    }
    Ok(OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output,
        surfaces,
        compositor_display_list: compositor_display_list.into(),
        software_cursor,
    })
}

/// Computes conservative combined client, compositor, and software-cursor
/// damage against the frame that will precede the current snapshot.
pub fn output_frame_damage(
    previous: Option<&OutputFrameDamageSnapshot>,
    current: &OutputFrameDamageSnapshot,
) -> Result<Region, OutputFrameDamageError> {
    output_frame_damage_with_causes(previous, current).map(|(damage, _)| damage)
}

/// The same reduction with bounded evidence. Each fallback reports its first
/// failed proof; distinct changed surfaces can contribute distinct causes.
pub fn output_frame_damage_with_causes(
    previous: Option<&OutputFrameDamageSnapshot>,
    current: &OutputFrameDamageSnapshot,
) -> Result<(Region, super::OutputDamageCauses), OutputFrameDamageError> {
    use super::OutputDamageCause as C;
    let mut causes = super::OutputDamageCauses::default();
    validate_snapshot(current)?;
    let Some(previous) = previous else {
        causes.insert(C::NewOutput);
        return Ok((full_output_damage(current.output.size), causes));
    };
    validate_snapshot(previous)?;
    if previous.output.id != current.output.id {
        return Err(OutputFrameDamageError::OutputMismatch);
    }
    if previous.output.size != current.output.size || previous.output.scale != current.output.scale
    {
        causes.insert(C::OutputChanged);
        return Ok((full_output_damage(current.output.size), causes));
    }

    let mut damage = compositor_display_list_damage(
        &previous.compositor_display_list,
        &current.compositor_display_list,
    );
    if !damage.rects.is_empty() {
        causes.insert(C::Compositor);
    }
    let previous_order = previous
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<Vec<_>>();
    let current_order = current
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<Vec<_>>();
    if previous_order != current_order {
        causes.insert(C::Order);
        extend_surface_extents(&mut damage, &previous.surfaces);
        extend_surface_extents(&mut damage, &current.surfaces);
    } else {
        for (before, after) in previous.surfaces.iter().zip(&current.surfaces) {
            if before != after
                || super::damage_history::surface_damage_identity(before, &previous.damage_history)
                    != super::damage_history::surface_damage_identity(
                        after,
                        &current.damage_history,
                    )
            {
                match super::damage_history::accumulated_surface_damage(
                    before,
                    after,
                    &previous.damage_history,
                    &current.damage_history,
                    &mut causes,
                ) {
                    Ok(precise) => {
                        causes.insert(C::PreciseSurface);
                        damage.rects.extend(precise.rects);
                    }
                    Err(cause) => {
                        causes.insert(cause);
                        damage.push(before.geometry);
                        damage.push(after.geometry);
                    }
                }
            }
        }
    }
    // Preview instances are compositor nodes. Reused public generations can
    // still name different rejected/accepted candidate pixels; their opaque
    // preparation identities must therefore participate in invalidation too.
    for instance in previous
        .compositor_display_list
        .surface_instances()
        .chain(current.compositor_display_list.surface_instances())
    {
        let old = super::damage_history::instance_damage_identity(
            instance.source,
            instance.source_generation,
            &previous.damage_history,
        );
        let new = super::damage_history::instance_damage_identity(
            instance.source,
            instance.source_generation,
            &current.damage_history,
        );
        if old != new {
            causes.insert(C::PreviewIdentity);
            damage.push(instance.visible());
        }
    }
    if previous.software_cursor != current.software_cursor {
        causes.insert(C::Cursor);
        if let Some(before) = previous.software_cursor {
            damage.push(before);
        }
        if let Some(after) = current.software_cursor {
            damage.push(after);
        }
    }
    Ok((damage, causes))
}

fn validate_snapshot(snapshot: &OutputFrameDamageSnapshot) -> Result<(), OutputFrameDamageError> {
    validate_output(snapshot.output)?;
    if snapshot.compositor_display_list.output != snapshot.output.id {
        return Err(OutputFrameDamageError::OutputMismatch);
    }
    if !compositor_display_list_structure_is_valid(&snapshot.compositor_display_list) {
        return Err(OutputFrameDamageError::InvalidCompositorDisplayList);
    }
    if snapshot.surfaces.len() > MAX_OUTPUT_FRAME_SURFACES {
        return Err(OutputFrameDamageError::SurfaceCapacityExceeded);
    }
    let mut seen = BTreeSet::new();
    for surface in &snapshot.surfaces {
        if !surface.surface.is_valid() {
            return Err(OutputFrameDamageError::InvalidSurface);
        }
        if !seen.insert(surface.surface) {
            return Err(OutputFrameDamageError::DuplicateSurface);
        }
    }
    let display_order = snapshot
        .compositor_display_list
        .commands
        .iter()
        .filter_map(|command| match command {
            CompositorDisplayCommand::Surface { surface } if seen.contains(surface) => {
                Some(*surface)
            }
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::SurfaceInstance(_)
            | CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
        .collect::<Vec<_>>();
    let snapshot_order = snapshot
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<Vec<_>>();
    if display_order != snapshot_order {
        return Err(OutputFrameDamageError::InvalidSurface);
    }
    Ok(())
}

fn validate_output(output: HeadlessOutput) -> Result<(), OutputFrameDamageError> {
    if !output.id.is_valid() {
        return Err(OutputFrameDamageError::InvalidOutput);
    }
    if output.size.width <= 0 || output.size.height <= 0 || output.scale == 0 {
        return Err(OutputFrameDamageError::InvalidOutputSize);
    }
    Ok(())
}

fn full_output_damage(size: Size) -> Region {
    Region::single(Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    })
}

fn extend_surface_extents(damage: &mut Region, surfaces: &[OutputFrameSurfaceState]) {
    for surface in surfaces {
        damage.push(surface.geometry);
    }
}
