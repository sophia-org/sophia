// Pointer evidence the owner loop prints once per routed button: which
// surface the button reached, and what the hit test had to choose from.
//
// Two records, one cause. The batch tallies count routed buttons and cannot
// separate a click that hit what the user aimed at from one that missed a
// popup and landed on the window beneath -- both are routed and unsuppressed.
// The target record names the surface and its presentation role, because the
// role is the question: a menu is client_positioned and the window under it is
// policy_managed. The projection record names what the pointer phase was
// given: the input projection as surface:rank pairs, and the subset under the
// pointer. A popup composited on top but absent here, or present and ranked
// beneath the window it covers, are different defects with the same symptom,
// and neither is visible from the target alone.
//
// Lists are bounded and rank-descending so the line stays one line. The
// containment test ignores transforms, which presented projections never set.

/// Whether `point` lies inside `layer`'s untransformed geometry.
fn layer_contains(layer: &sophia_protocol::LayerSnapshot, point: sophia_protocol::Point) -> bool {
    let g = layer.geometry;
    point.x >= f64::from(g.x)
        && point.y >= f64::from(g.y)
        && point.x < f64::from(g.x) + f64::from(g.width)
        && point.y < f64::from(g.y) + f64::from(g.height)
}

/// At most eight `surface:rank` pairs, or `-` when there are none. The redactor
/// admits exactly this shape and drops anything wider.
fn ranked_pairs(layers: &[&sophia_protocol::LayerSnapshot]) -> String {
    if layers.is_empty() {
        return "-".to_owned();
    }
    layers
        .iter()
        .take(8)
        .map(|layer| format!("{}:{}", layer.surface.index(), layer.stack_rank))
        .collect::<Vec<_>>()
        .join(",")
}

fn emit_pointer_evidence(
    targets: &[sophia_protocol::SurfaceId],
    roles: &std::collections::BTreeMap<
        sophia_protocol::SurfaceId,
        sophia_protocol::SurfacePresentationRole,
    >,
    projection_layers: &[sophia_protocol::LayerSnapshot],
    projection_count: usize,
    projection_epoch: u64,
    pointer: Option<sophia_protocol::Point>,
) {
    if targets.is_empty() {
        return;
    }
    let mut ranked: Vec<&sophia_protocol::LayerSnapshot> = projection_layers.iter().collect();
    ranked.sort_by_key(|layer| std::cmp::Reverse(layer.stack_rank));
    let under: Vec<&sophia_protocol::LayerSnapshot> = pointer.map_or_else(Vec::new, |point| {
        ranked
            .iter()
            .copied()
            .filter(|layer| layer_contains(layer, point))
            .collect()
    });
    for surface in targets {
        crate::session_println!(
            "sophia_live_session_pointer_projection schema=1 status=button_routed target={} epoch={} projections={} layers={} contains={} under={} all={}",
            surface.index(),
            projection_epoch,
            projection_count,
            projection_layers.len(),
            under.len(),
            ranked_pairs(&under),
            ranked_pairs(&ranked),
        );
    }
    for surface in targets {
        crate::session_println!(
            "sophia_live_session_pointer_target schema=1 status=button_routed surface={} generation={} role={}",
            surface.index(),
            surface.generation(),
            match roles.get(surface) {
                Some(sophia_protocol::SurfacePresentationRole::ClientPositioned) => {
                    "client_positioned"
                }
                Some(sophia_protocol::SurfacePresentationRole::PolicyManaged) => "policy_managed",
                None => "unknown",
            },
        );
    }
}
