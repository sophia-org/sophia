use super::*;

pub(super) fn allocation(
    allocations: &[ContentAllocationSnapshot],
    id: ContentAllocationId,
) -> Result<&ContentAllocationSnapshot, ContentCandidateError> {
    allocations
        .iter()
        .find(|allocation| allocation.allocation == id)
        .ok_or(ContentCandidateError::AllocationLost)
}

pub(super) fn validate_surfaces(
    surfaces: &[ContentSurface],
    allocations: &[ContentAllocationSnapshot],
    output: ContentOutputId,
) -> Result<(), ContentCandidateError> {
    let mut ids = BTreeSet::new();
    for (index, surface) in surfaces.iter().enumerate() {
        let actual = allocation(allocations, surface.allocation)?;
        if actual.output != output
            || !ids.insert(surface.allocation)
            || surface.scale_generation != actual.scale_generation
            || surface.role != actual.role
            || surface.edge != actual.edge
            || surface.margins != actual.margins
            || surface.anchor_parent_rect != actual.anchor_parent_rect
            || surface.reservation_extent > actual.allowed_reservation_extent
        {
            return Err(ContentCandidateError::AllocationLost);
        }
        if surface.role == 1 || surface.role == 3 {
            if surface.parent_surface_index != u16::MAX
                || actual.parent != ContentAllocationId::default()
            {
                return Err(ContentCandidateError::Malformed);
            }
        } else {
            let parent_index = usize::from(surface.parent_surface_index);
            if parent_index >= index
                || surfaces[parent_index].role != 1
                || surfaces[parent_index].allocation != actual.parent
            {
                return Err(ContentCandidateError::Malformed);
            }
        }
    }
    Ok(())
}

pub(super) fn validate_targets(
    targets: &[ContentTarget],
    surfaces: &[ContentSurface],
    allocations: &[ContentAllocationSnapshot],
) -> Result<(), ContentCandidateError> {
    let mut identities = BTreeSet::new();
    for target in targets {
        let Some(surface) = surfaces.get(usize::from(target.surface_index)) else {
            return Err(ContentCandidateError::Malformed);
        };
        let actual = allocation(allocations, surface.allocation)?;
        if !identities.insert((target.target_id, target.target_generation, target.action_id))
            || !inside(target.bounds_px, actual.pixel.width, actual.pixel.height)
        {
            return Err(ContentCandidateError::Malformed);
        }
    }
    for (index, left) in targets.iter().enumerate() {
        for right in &targets[index + 1..] {
            if left.surface_index == right.surface_index
                && rectangles_overlap(left.bounds_px, right.bounds_px)
            {
                return Err(ContentCandidateError::Malformed);
            }
        }
    }
    Ok(())
}

pub(super) fn rectangles_overlap(left: ContentPixelRect, right: ContentPixelRect) -> bool {
    let left_right = i64::from(left.x) + i64::from(left.width);
    let left_bottom = i64::from(left.y) + i64::from(left.height);
    let right_right = i64::from(right.x) + i64::from(right.width);
    let right_bottom = i64::from(right.y) + i64::from(right.height);
    i64::from(left.x) < right_right
        && i64::from(right.x) < left_right
        && i64::from(left.y) < right_bottom
        && i64::from(right.y) < left_bottom
}

pub(super) fn valid_chunk_rows(
    chunk: &ContentCandidateChunk,
    max_margin: u32,
    native: bool,
) -> bool {
    chunk.surfaces.iter().all(|surface| {
        (if native {
            surface.role == 3
                && surface.reservation_extent == 0
                && surface.anchor_parent_rect == ContentPixelRect::default()
        } else {
            (1..=2).contains(&surface.role)
        }) && (1..=4).contains(&surface.edge)
            && [
                surface.margins.top,
                surface.margins.right,
                surface.margins.bottom,
                surface.margins.left,
            ]
            .into_iter()
            .all(|margin| i32::from(margin).unsigned_abs() <= max_margin)
    }) && chunk.placements.iter().all(|placement| {
        placement.resource.id > 0
            && placement.resource.generation > 0
            && placement.destination_x_px >= 0
            && placement.destination_y_px >= 0
    }) && chunk.targets.iter().all(|target| {
        target.action_kind == if native { 2 } else { 1 }
            && target.target_id > 0
            && target.target_generation > 0
            && target.action_id > 0
            && target.bounds_px.width > 0
            && target.bounds_px.height > 0
    })
}

pub(super) fn validate_placements(
    placements: &[ContentPlacement],
    surfaces: &[ContentSurface],
    allocations: &[ContentAllocationSnapshot],
    resources: &ContentResourceStore,
    grant: ContentGrant,
) -> Result<BTreeSet<ContentResourceId>, ContentCandidateError> {
    let mut resource_ids = BTreeSet::new();
    for placement in placements {
        let Some(surface) = surfaces.get(usize::from(placement.surface_index)) else {
            return Err(ContentCandidateError::Malformed);
        };
        let actual = allocation(allocations, surface.allocation)?;
        let lease = resources.lease(grant, placement.resource)?;
        let description = lease.description();
        if description.rendered_scale_numerator != actual.scale_numerator
            || description.rendered_scale_denominator != actual.scale_denominator
            || placement.destination_x_px < 0
            || placement.destination_y_px < 0
            || u64::try_from(placement.destination_x_px).unwrap_or(u64::MAX)
                + u64::from(description.width_px)
                > u64::from(actual.pixel.width)
            || u64::try_from(placement.destination_y_px).unwrap_or(u64::MAX)
                + u64::from(description.height_px)
                > u64::from(actual.pixel.height)
        {
            return Err(ContentCandidateError::Malformed);
        }
        resource_ids.insert(placement.resource);
    }
    Ok(resource_ids)
}

pub(super) fn inside(rect: ContentPixelRect, width: u32, height: u32) -> bool {
    rect.x >= 0
        && rect.y >= 0
        && u64::try_from(rect.x).unwrap_or(u64::MAX) + u64::from(rect.width) <= u64::from(width)
        && u64::try_from(rect.y).unwrap_or(u64::MAX) + u64::from(rect.height) <= u64::from(height)
}
