#![cfg(feature = "libdrm-events")]
// Exercise the production reduction with out-of-order per-output observations.
include!("../src/production_session/native_scanout/persistent_native_scanout/render_metrics.rs");

#[test]
fn shared_context_counts_once_and_uses_the_latest_gauges() {
    use sophia_renderer_live::LiveNativePersistentRenderStats as Stats;
    let old = Stats {
        observation_order: 1,
        snapshot_captures: 2,
        snapshot_live_entries: 2,
        ..Stats::default()
    };
    let new = Stats {
        observation_order: 2,
        snapshot_captures: 3,
        snapshot_live_entries: 1,
        ..Stats::default()
    };
    for mut observations in [
        vec![(Some(1), old), (Some(1), new)],
        vec![(Some(1), new), (Some(1), old)],
    ] {
        observations.push((Some(2), old));
        observations.push((None, old));
        retain_latest_context_metrics(&mut observations);
        assert_eq!(
            observations
                .iter()
                .map(|(_, s)| s.snapshot_captures)
                .sum::<usize>(),
            7
        );
        assert_eq!(
            observations
                .iter()
                .map(|(_, s)| s.snapshot_live_entries)
                .sum::<usize>(),
            5
        );
    }
}
