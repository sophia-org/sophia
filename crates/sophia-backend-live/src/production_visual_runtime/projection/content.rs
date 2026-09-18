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
    let images = display_list
        .content_images()
        .filter(|image| {
            matches!(image.node,
        CompositorNodeId::ShellContent { grant, output, .. }
        if grant == frame.grant && output == frame.output)
        })
        .collect::<Vec<_>>();
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
        grant: frame.grant,
        output: frame.content_output,
        candidate_generation: frame.candidate_generation,
        presentation_epoch: 0,
        interaction_generation: frame.interaction_generation,
        transform: owned.transform.clone(),
        authority_current: !owned.interaction_revoked
            && !viewport.is_empty()
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
            previous.grant == next.grant
                && previous.output == next.output
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

/// Follow actual display order, including revoked owners whose pixels remain
/// visible. Current admission order cannot promote a prepared replacement.
pub(super) fn presented_content_bindings(
    presented: &OutputFrameDamageSnapshot,
    admitted: &BTreeMap<ShellContentKey, AdmittedShellContent>,
    previous: &[sophia_engine::PresentedContentBinding],
    output: OutputId,
    viewport: Rect,
    layout_generation: u64,
) -> Vec<sophia_engine::PresentedContentBinding> {
    let mut bindings = Vec::new();
    for image in presented.compositor_display_list.content_images() {
        let CompositorNodeId::ShellContent {
            grant,
            output: image_output,
            candidate,
            ..
        } = image.node
        else {
            continue;
        };
        if image_output != output
            || bindings
                .iter()
                .any(|b: &sophia_engine::PresentedContentBinding| b.grant == grant)
        {
            continue;
        }
        let current = admitted.iter().find_map(|((id, _), owned)| {
            (*id == output && owned.frame.grant == grant).then_some(owned)
        });
        let old = previous.iter().find(|binding| binding.grant == grant);
        let binding = if let Some(current) =
            current.filter(|owned| presented_content_matches(presented, &owned.frame))
        {
            content_binding_from_frame(current, viewport, layout_generation)
        } else if let Some(old) = old {
            let mut retained = old.clone();
            retained.authority_current &= current.is_some_and(|owned| !owned.interaction_revoked && owned.frame.content_output == old.output)
                && old.transform.viewport == viewport
                && old.transform.layout_generation == layout_generation
                && presented.compositor_display_list.content_images()
                    .filter(|image| matches!(image.node, CompositorNodeId::ShellContent { grant: g, output: o, .. } if g == grant && o == output))
                    .all(|image| matches!(image.node, CompositorNodeId::ShellContent { candidate, .. } if candidate == old.candidate_generation));
            retained
        } else {
            // The actual pixels identify this grant, but no presentation input
            // metadata survived. Zero generations explicitly carry no authority;
            // do not borrow another grant's targets or current allocation.
            sophia_engine::PresentedContentBinding {
                grant,
                output: sophia_protocol::ContentOutputId {
                    id: output.raw(),
                    generation: 0,
                },
                candidate_generation: candidate,
                presentation_epoch: 0,
                interaction_generation: 0,
                transform: sophia_engine::PresentedContentTransform {
                    viewport,
                    layout_generation,
                },
                authority_current: false,
                targets: Vec::new(),
                allocations: Vec::new(),
            }
        };
        bindings.push(binding);
    }
    bindings
}
