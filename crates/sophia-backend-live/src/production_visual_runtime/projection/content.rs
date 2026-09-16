use super::*;

pub(super) fn presented_content_matches(
    presented: &OutputFrameDamageSnapshot,
    frame: &LiveShellContentFrame,
) -> bool {
    presented_content_list_matches(&presented.compositor_display_list, frame)
}

pub(in crate::production_visual_runtime) fn presented_content_list_matches(
    display_list: &CompositorDamageList,
    frame: &LiveShellContentFrame,
) -> bool {
    let images = display_list.content_images().collect::<Vec<_>>();
    !images.is_empty()
        && images.len() == frame.images.len()
        && images
            .into_iter()
            .zip(&frame.images)
            .all(|(presented, current)| *presented == current.content_identity())
}

pub(super) fn content_binding_from_frame(
    owned: &AdmittedShellContent,
    viewport: Rect,
    layout_generation: u64,
) -> sophia_engine::PresentedContentBinding {
    let frame = &owned.frame;
    sophia_engine::PresentedContentBinding {
        output: frame.content_output,
        candidate_generation: frame.candidate_generation,
        presentation_epoch: 0,
        interaction_generation: frame.interaction_generation,
        transform: owned.transform.clone(),
        authority_current: !viewport.is_empty()
            && layout_generation != 0
            && owned.transform.viewport == viewport
            && owned.transform.layout_generation == layout_generation,
        targets: frame.targets.clone(),
        allocations: frame.allocations.clone(),
    }
}

pub(super) fn same_content_binding(
    previous: Option<&sophia_engine::PresentedContentBinding>,
    next: Option<&sophia_engine::PresentedContentBinding>,
) -> bool {
    match (previous, next) {
        (None, None) => true,
        (Some(previous), Some(next)) => {
            previous.output == next.output
                && previous.transform == next.transform
                && previous.authority_current == next.authority_current
                && previous.candidate_generation == next.candidate_generation
                && previous.interaction_generation == next.interaction_generation
                && previous
                    .targets
                    .iter()
                    .zip(&next.targets)
                    .all(|(previous, next)| {
                        let mut previous = previous.clone();
                        let mut next = next.clone();
                        previous.presentation_epoch = 0;
                        previous.continuity = None;
                        next.presentation_epoch = 0;
                        next.continuity = None;
                        previous == next
                    })
                && previous.targets.len() == next.targets.len()
                && previous.allocations == next.allocations
        }
        _ => false,
    }
}

/// Keep the old transform while its pixels remain displayed. Revocation or a
/// topology change removes authority, not the fact that shell pixels occlude
/// applications. A reconnected grant cannot inherit the old targets.
pub(super) fn retain_presented_content_binding(
    presented: &OutputFrameDamageSnapshot,
    current: &AdmittedShellContent,
    previous: Option<&sophia_engine::PresentedContentBinding>,
    viewport: Rect,
    layout_generation: u64,
) -> Option<sophia_engine::PresentedContentBinding> {
    let mut images = presented
        .compositor_display_list
        .content_images()
        .peekable();
    images.peek()?;
    let Some(previous) = previous else {
        // There are known displayed shell pixels but no retained interaction
        // snapshot. This is occlusion only: never lend them the new targets.
        let mut blocked = content_binding_from_frame(current, viewport, layout_generation);
        blocked.authority_current = false;
        blocked.targets.clear();
        blocked.allocations.clear();
        return Some(blocked);
    };
    let matches = images.all(|image| {
        image.resource.grant == current.frame.grant
            && matches!(image.node, CompositorNodeId::ShellContent { output, candidate, .. }
                if output == current.frame.output && candidate == previous.candidate_generation)
    });
    let mut retained = previous.clone();
    retained.authority_current &= matches
        && previous.output == current.frame.content_output
        && previous.transform.viewport == viewport
        && previous.transform.layout_generation == layout_generation;
    Some(retained)
}
