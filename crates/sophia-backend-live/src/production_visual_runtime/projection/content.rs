use super::*;

pub(super) fn presented_content_matches(
    presented: &OutputFrameDamageSnapshot,
    frame: &LiveShellContentFrame,
) -> bool {
    let images = presented
        .compositor_display_list
        .content_images()
        .collect::<Vec<_>>();
    !images.is_empty()
        && images.len() == frame.images.len()
        && images.iter().all(|image| {
            matches!(
                image.node,
                sophia_engine::CompositorNodeId::ShellContent {
                    output,
                    candidate,
                    ..
                } if output == frame.output && candidate == frame.candidate_generation
            )
        })
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
