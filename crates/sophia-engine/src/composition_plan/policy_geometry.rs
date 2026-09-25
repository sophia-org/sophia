// How a head draws WM policy targets (surface instances and policy regions),
// shared by the head plan and read-only presentation admission (t244).

/// The logical-to-native transform of one head and the rectangle everything
/// it paints is bounded by; None when the scene projects to nothing.
fn head_transform(
    logical_viewport: Rect,
    target: HeadRenderTarget,
) -> Option<(HeadLogicalTransform, Rect)> {
    let source = Size {
        width: logical_viewport.width,
        height: logical_viewport.height,
    };
    let projected_scene = project_scene(source, target.native_size, target.mapping);
    if projected_scene.is_empty() {
        return None;
    }
    // Everything the scene draws is bounded by this, not by the framebuffer.
    let painted = scene_clip(projected_scene, target.native_size);
    Some((
        HeadLogicalTransform {
            source,
            projected_scene,
        },
        painted,
    ))
}

/// An instance as a head draws it: destination and clip projected outward,
/// the clip bounded by the painted scene; None when it draws no pixel there.
fn project_policy_instance(
    instance: crate::CompositorSurfaceInstance,
    logical_viewport: Rect,
    transform: HeadLogicalTransform,
    painted: Rect,
) -> Option<crate::CompositorSurfaceInstance> {
    let destination = transform.project_root_rect_outward(logical_viewport, instance.destination);
    let clip = intersect_rect(
        transform.project_root_rect_outward(logical_viewport, instance.clip),
        painted,
    );
    (!intersect_rect(destination, clip).is_empty()).then_some(crate::CompositorSurfaceInstance {
        destination,
        clip,
        ..instance
    })
}

/// Whether a WM policy command (a surface instance or a policy region's rect
/// or border) draws at least one clipped pixel on `target`, by exactly the
/// arithmetic the head plan draws it with. Other commands answer true. A
/// read-only admission check: a publication whose target this answers false
/// for on any head would present an undrawn target, and is refused whole
/// before commit rather than revoked after it (t244).
pub fn head_draws_policy_command(
    command: &CompositorDisplayCommand,
    logical_viewport: Rect,
    target: HeadRenderTarget,
) -> bool {
    let Some((transform, painted)) = head_transform(logical_viewport, target) else {
        return false;
    };
    match command {
        CompositorDisplayCommand::SurfaceInstance(instance) => {
            !intersect_rect(instance.visible(), logical_viewport).is_empty()
                && project_policy_instance(*instance, logical_viewport, transform, painted)
                    .is_some()
        }
        CompositorDisplayCommand::Rect(rect)
            if matches!(rect.node, CompositorNodeId::PolicyRegion { .. }) =>
        {
            !intersect_rect(rect.geometry, logical_viewport).is_empty()
                && !project_rect(*rect, logical_viewport, transform, painted)
                    .geometry
                    .is_empty()
        }
        CompositorDisplayCommand::Border(border)
            if matches!(border.node, CompositorNodeId::PolicyRegion { .. }) =>
        {
            !intersect_rect(border.outer, logical_viewport).is_empty()
                && policy_border_draws(&project_border(
                    *border,
                    logical_viewport,
                    transform,
                    painted,
                ))
        }
        _ => true,
    }
}
