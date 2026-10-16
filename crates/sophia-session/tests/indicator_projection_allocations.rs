#[path = "../src/shell_indicator_projection.rs"]
mod projection;
use projection::{indicator_snapshot, indicator_snapshot_matches};
use sophia_protocol::{
    OutputId, PolicyProjectionIndicator, PolicyProjectionOutputStatus, WmActionId,
};

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn unchanged_projection_is_allocation_free_and_every_wire_field_is_compared() {
    let output = OutputId::from_raw(1);
    let publication = sophia_engine::PolicyIndicatorPublication {
        tab_groups: vec![],
        generation: 7,
        connection_epoch: Some(3),
        indicators: vec![PolicyProjectionIndicator {
            output,
            slot: 0,
            indicator: 1,
            action: Some(WmActionId::from_raw(2)),
            state_bits: 1,
            label: "web".into(),
        }],
        output_statuses: vec![PolicyProjectionOutputStatus {
            output,
            focus_bits: 1,
            layout: "scroll".into(),
        }],
    };
    let snapshot = indicator_snapshot(&publication, Some(output), 5);
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        assert!(indicator_snapshot_matches(
            &snapshot,
            &publication,
            Some(output),
            5
        ));
    }
    let after = ALLOCATOR.stats();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.reallocations, before.reallocations);
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        std::hint::black_box(indicator_snapshot(&publication, Some(output), 5));
    }
    let after = ALLOCATOR.stats();
    assert!(after.allocations > before.allocations);
    eprintln!(
        "indicator projection: 1000 owned snapshots = {} allocations; 1000 borrowed comparisons = 0",
        after.allocations - before.allocations
    );

    let mut launches = sophia_session::session_actions::SessionLaunchQueue::default();
    let contexts = [sophia_protocol::PolicyOutputLaunchContext {
        output,
        output_generation: 1,
        epoch: 2,
        token: 3,
    }];
    launches.set_output_launch_contexts(&contexts);
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        launches.set_output_launch_contexts(&contexts);
    }
    let after = ALLOCATOR.stats();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.reallocations, before.reallocations);
    for change in 0..12 {
        let mut s = snapshot.clone();
        match change {
            0 => s.connection_epoch += 1,
            1 => s.generation += 1,
            2 => s.active_output = None,
            3 => s.statuses[0].output = OutputId::from_raw(2),
            4 => s.statuses[0].focus_bits = 0,
            5 => s.statuses[0].layout.push('x'),
            6 => s.indicators[0].output = OutputId::from_raw(2),
            7 => s.indicators[0].indicator += 1,
            8 => s.indicators[0].action = 0,
            9 => s.indicators[0].slot += 1,
            10 => s.indicators[0].state_bits = 0,
            _ => s.indicators[0].label.push('x'),
        }
        assert!(
            !indicator_snapshot_matches(&s, &publication, Some(output), 5),
            "field {change}"
        );
    }
    let mut removed = publication.clone();
    removed.indicators.clear();
    assert!(!indicator_snapshot_matches(
        &snapshot,
        &removed,
        Some(output),
        5
    ));
    removed = publication.clone();
    removed.output_statuses.clear();
    assert!(!indicator_snapshot_matches(
        &snapshot,
        &removed,
        Some(output),
        5
    ));
}
