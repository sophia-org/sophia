//! Exact acknowledged allocation geometry to retained composition records.
use super::*;

/// The candidate tables a render bundle and a frame commit share.
pub(crate) struct CandidateView<'a> {
    pub grant: sophia_protocol::ContentGrant,
    pub output: ContentOutputId,
    pub candidate_generation: u64,
    pub interaction_generation: u64,
    pub surfaces: &'a [sophia_protocol::ContentSurface],
    pub placements: &'a [sophia_protocol::ContentPlacement],
    pub targets: &'a [sophia_protocol::ContentTarget],
}

/// Converts one validated content bundle into the immutable renderer record.
/// Candidate validation owns references and table shape; this seam owns the
/// exact allocation-local physical placement and retains each resource lease.
pub(super) fn project_render_bundle(
    bundle: &ContentRenderBundle,
    output: HeadlessOutput,
    output_identity: ContentOutputId,
    allocations: &[ContentAllocationSnapshot],
) -> Result<LiveShellContentFrame, &'static str> {
    let view = CandidateView {
        grant: bundle.grant,
        output: bundle.output,
        candidate_generation: bundle.candidate_generation,
        interaction_generation: bundle.interaction_generation,
        surfaces: &bundle.surfaces,
        placements: &bundle.placements,
        targets: &bundle.targets,
    };
    project_candidate(view, output, output_identity, allocations, |id| {
        let lease = bundle.resource(id)?.clone();
        Some((lease.description().clone(), lease.into()))
    })
}

/// G3-5b service 1: a frame commit's candidate before admission, its one
/// copied resource as the provisional image admission will validate. Any
/// other resource is absent here, so its frame takes the ordinary path.
#[cfg(feature = "experimental-combined-frame-job")]
#[allow(
    dead_code,
    reason = "the backend's lifecycle tests mount this file without the Session row, and Session has no row without its three gates"
)]
pub(crate) fn project_frame_commit(
    candidate: &sophia_protocol::ContentCandidate,
    description: &sophia_protocol::ContentResourceBegin,
    output: HeadlessOutput,
    allocations: &[ContentAllocationSnapshot],
) -> Result<LiveShellContentFrame, &'static str> {
    let view = CandidateView {
        grant: candidate.grant,
        output: candidate.output,
        candidate_generation: candidate.candidate_generation,
        interaction_generation: candidate.interaction_generation,
        surfaces: &candidate.surfaces,
        placements: &candidate.placements,
        targets: &candidate.targets,
    };
    project_candidate(view, output, candidate.output, allocations, |id| {
        (id == description.resource).then(|| {
            (
                description.clone(),
                sophia_engine::CompositorImageSource::Provisional(description.clone()),
            )
        })
    })
}

/// The one checked placement algorithm. `resource` gives a placement's
/// resource description and image source, or `None` when it is absent.
fn project_candidate(
    bundle: CandidateView<'_>,
    output: HeadlessOutput,
    output_identity: ContentOutputId,
    allocations: &[ContentAllocationSnapshot],
    mut resource: impl FnMut(
        sophia_protocol::ContentResourceId,
    ) -> Option<(
        sophia_protocol::ContentResourceBegin,
        sophia_engine::CompositorImageSource,
    )>,
) -> Result<LiveShellContentFrame, &'static str> {
    if bundle.output != output_identity
        || bundle.candidate_generation == 0
        || output.id.raw() != output_identity.id
        || output.size.width <= 0
        || output.size.height <= 0
    {
        return Err("content bundle targets stale output facts");
    }
    let mut nodes = BTreeSet::new();
    let mut allocation_rows = Vec::new();
    let mut images = Vec::with_capacity(bundle.placements.len());
    for (placement_index, placement) in bundle.placements.iter().enumerate() {
        let surface_index = usize::from(placement.surface_index);
        let surface = bundle
            .surfaces
            .get(surface_index)
            .ok_or("content placement names an absent surface")?;
        let allocation = allocations
            .iter()
            .find(|allocation| allocation.allocation == surface.allocation)
            .ok_or("content placement names a lost allocation")?;
        if allocation.output != output_identity {
            return Err("content placement crosses outputs");
        }
        let (description, source) =
            resource(placement.resource).ok_or("content placement names an absent resource")?;
        let description = &description;
        let width = i32::try_from(description.width_px)
            .map_err(|_| "content width exceeds renderer geometry")?;
        let height = i32::try_from(description.height_px)
            .map_err(|_| "content height exceeds renderer geometry")?;
        let geometry_px = Rect {
            x: allocation
                .pixel
                .x
                .checked_add(placement.destination_x_px)
                .ok_or("content placement x overflow")?,
            y: allocation
                .pixel
                .y
                .checked_add(placement.destination_y_px)
                .ok_or("content placement y overflow")?,
            width,
            height,
        };
        let allocation_width = i32::try_from(allocation.pixel.width)
            .map_err(|_| "content allocation width exceeds renderer geometry")?;
        let allocation_height = i32::try_from(allocation.pixel.height)
            .map_err(|_| "content allocation height exceeds renderer geometry")?;
        if geometry_px.x < allocation.pixel.x
            || geometry_px.y < allocation.pixel.y
            || geometry_px.x.saturating_add(width)
                > allocation.pixel.x.saturating_add(allocation_width)
            || geometry_px.y.saturating_add(height)
                > allocation.pixel.y.saturating_add(allocation_height)
        {
            return Err("content placement escapes its allocation");
        }
        let node = CompositorNodeId::ShellContent {
            grant: bundle.grant,
            output: output.id,
            candidate: bundle.candidate_generation,
            surface: placement.surface_index,
            placement: u16::try_from(placement_index)
                .map_err(|_| "content placement identity exceeds renderer bound")?,
        };
        if !nodes.insert(node) {
            return Err("content placement repeats a renderer node");
        }
        // A visible surface occludes even when its query has no actionable
        // rows. Targets describe actions, not whether rendered content exists.
        let row = (allocation.allocation, allocation.logical, allocation.pixel);
        if !allocation_rows.contains(&row) {
            allocation_rows.push(row);
        }
        images.push(CompositorContentImage {
            node,
            generation: placement.resource.generation,
            output_size_px: output.size,
            geometry_px,
            size_px: Size { width, height },
            stride: description
                .width_px
                .checked_mul(4)
                .ok_or("content stride overflow")?,
            format: DRM_FORMAT_ARGB8888,
            resource: source,
        });
    }
    if images.is_empty() {
        return Err("content candidate has no visible placements");
    }
    let mut targets = Vec::with_capacity(bundle.targets.len());
    for target in bundle.targets {
        let surface = bundle
            .surfaces
            .get(usize::from(target.surface_index))
            .ok_or("content target names an absent surface")?;
        let allocation = allocations
            .iter()
            .find(|candidate| candidate.allocation == surface.allocation)
            .ok_or("content target names a lost allocation")?;
        targets.push(sophia_engine::PresentedContentTarget {
            continuity: None,
            scale_generation: allocation.scale_generation,
            grant: bundle.grant,
            output: bundle.output,
            candidate_generation: bundle.candidate_generation,
            presentation_epoch: 0,
            interaction_generation: bundle.interaction_generation,
            allocation: allocation.allocation,
            allocation_logical: allocation.logical,
            allocation_pixel: allocation.pixel,
            target_id: target.target_id,
            target_generation: target.target_generation,
            action_id: target.action_id,
            bounds_px: target.bounds_px,
        });
    }
    let mut popouts = Vec::new();
    for (index, surface) in bundle.surfaces.iter().enumerate() {
        if surface.role != 2
            || !allocation_rows
                .iter()
                .any(|row| row.0 == surface.allocation)
        {
            continue;
        }
        let parent = bundle
            .surfaces
            .get(usize::from(surface.parent_surface_index))
            .filter(|parent| parent.role == 1)
            .ok_or("content popout has no parent panel")?;
        popouts.push(sophia_engine::PresentedContentPopout {
            allocation: surface.allocation,
            parent: parent.allocation,
            surface_index: u16::try_from(index).map_err(|_| "content surface index overflow")?,
        });
    }
    Ok(LiveShellContentFrame {
        output: OutputId::from_raw(output_identity.id),
        content_output: output_identity,
        grant: bundle.grant,
        candidate_generation: bundle.candidate_generation,
        interaction_generation: bundle.interaction_generation,
        images,
        targets,
        popouts,
        allocations: allocation_rows,
    })
}
