#[path = "../src/production_session/native_scanout/persistent_native_scanout/head_selection.rs"]
mod head_selection;
use head_selection::enabled_head_indices;
use sophia_protocol::OutputId;

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn borrowed_selection_preserves_mirrors_and_observes_topology_changes() {
    let a = OutputId::from_raw(1);
    let b = OutputId::from_raw(2);
    let mut heads = [(false, a), (true, b), (true, a), (true, a)];
    assert!(enabled_head_indices(heads.iter().copied(), a).eq([2, 3]));
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        std::hint::black_box(enabled_head_indices(heads.iter().copied(), a).collect::<Vec<_>>());
    }
    let owned = ALLOCATOR.stats() - before;
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        assert!(enabled_head_indices(std::hint::black_box(&heads).iter().copied(), a).eq([2, 3]));
    }
    let borrowed = ALLOCATOR.stats() - before;
    eprintln!(
        "1000 head selections: owned {}, borrowed {} allocations",
        owned.allocations, borrowed.allocations
    );
    assert!(owned.allocations > 0);
    assert_eq!(borrowed.allocations + borrowed.reallocations, 0);
    heads[2].0 = false;
    heads[3].1 = b;
    assert_eq!(enabled_head_indices(heads.iter().copied(), a).next(), None);
    assert!(enabled_head_indices(heads.iter().copied(), b).eq([1, 3]));
    let many = [(true, a); 33];
    assert!(
        enabled_head_indices(many.into_iter(), a).eq(0..33),
        "head selection adds no capacity limit"
    );
}
