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
    let mut surfaces = Vec::new();
    let mut seen = BTreeSet::new();
    for surface in compositor_display_list
        .commands
        .iter()
        .filter_map(|command| match command {
            CompositorDisplayCommand::Surface { surface } => Some(*surface),
            CompositorDisplayCommand::SurfacePreview(_)
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
    validate_snapshot(current)?;
    let Some(previous) = previous else {
        return Ok(full_output_damage(current.output.size));
    };
    validate_snapshot(previous)?;
    if previous.output.id != current.output.id {
        return Err(OutputFrameDamageError::OutputMismatch);
    }
    if previous.output.size != current.output.size || previous.output.scale != current.output.scale
    {
        return Ok(full_output_damage(current.output.size));
    }

    let mut damage = compositor_display_list_damage(
        &previous.compositor_display_list,
        &current.compositor_display_list,
    );
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
        extend_surface_extents(&mut damage, &previous.surfaces);
        extend_surface_extents(&mut damage, &current.surfaces);
    } else {
        for (before, after) in previous.surfaces.iter().zip(&current.surfaces) {
            if before != after {
                damage.push(before.geometry);
                damage.push(after.geometry);
            }
        }
    }
    if previous.software_cursor != current.software_cursor {
        if let Some(before) = previous.software_cursor {
            damage.push(before);
        }
        if let Some(after) = current.software_cursor {
            damage.push(after);
        }
    }
    Ok(damage)
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
            | CompositorDisplayCommand::SurfacePreview(_)
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
