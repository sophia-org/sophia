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
    frame: &LiveShellContentFrame,
) -> sophia_engine::PresentedContentBinding {
    sophia_engine::PresentedContentBinding {
        output: frame.content_output,
        candidate_generation: frame.candidate_generation,
        presentation_epoch: 0,
        interaction_generation: frame.interaction_generation,
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
                        next.presentation_epoch = 0;
                        previous == next
                    })
                && previous.targets.len() == next.targets.len()
                && previous.allocations == next.allocations
        }
        _ => false,
    }
}

/// Keep the old interaction snapshot while another candidate of the SAME grant
/// waits for its physical retirement. A reconnected grant cannot inherit it.
pub(super) fn retain_presented_content_binding(
    presented: &OutputFrameDamageSnapshot,
    current: &LiveShellContentFrame,
    previous: Option<&sophia_engine::PresentedContentBinding>,
) -> Option<sophia_engine::PresentedContentBinding> {
    let previous = previous?;
    if previous.output != current.content_output {
        return None;
    }
    let mut images = presented
        .compositor_display_list
        .content_images()
        .peekable();
    images.peek()?;
    images
        .all(|image| {
            image.resource.grant == current.grant
                && matches!(image.node, CompositorNodeId::ShellContent { output, candidate, .. }
                if output == current.output && candidate == previous.candidate_generation)
        })
        .then(|| previous.clone())
}
