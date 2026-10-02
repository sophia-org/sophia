// Results are per output; their render statistics belong to the shared context.
// A later result may already have reached another output. Gauges must come from
// the newest observation, never a sum or a fieldwise maximum.
fn retain_latest_context_metrics(
    snapshots: &mut [(Option<usize>, sophia_renderer_live::LiveNativePersistentRenderStats)],
) {
    let mut latest = std::collections::BTreeMap::new();
    for (index, (identity, stats)) in snapshots.iter().enumerate() {
        if let Some(identity) = identity {
            let entry = latest.entry(*identity).or_insert(index);
            if stats.observation_order > snapshots[*entry].1.observation_order {
                *entry = index;
            }
        }
    }
    for (index, (identity, stats)) in snapshots.iter_mut().enumerate() {
        if identity.is_some_and(|identity| latest[&identity] != index) {
            *stats = Default::default();
        }
    }
}
