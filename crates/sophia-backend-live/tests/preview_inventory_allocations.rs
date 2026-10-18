#[path = "../src/production_session/native_scanout/persistent_native_scanout/store_inventory.rs"]
mod store_inventory;

use std::collections::BTreeMap;

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn preview_inventory_avoids_collection_and_changes_keep_last_entry() {
    let current = BTreeMap::from([(11, 0), (22, 1)]);
    let heads = [(22, 1), (11, 0), (22, 1)];
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        std::hint::black_box(
            std::hint::black_box(heads)
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
        );
    }
    let collected = ALLOCATOR.stats() - before;
    let before = ALLOCATOR.stats();
    for _ in 0..1000 {
        assert!(
            store_inventory::replacement(&current, std::hint::black_box(heads).into_iter())
                .is_none()
        );
    }
    let borrowed = ALLOCATOR.stats() - before;
    println!(
        "collected={} borrowed={}",
        collected.allocations, borrowed.allocations
    );
    assert!(collected.allocations > 0);
    assert_eq!(borrowed.allocations + borrowed.reallocations, 0);

    for entries in [
        vec![],
        vec![(11, 0)],
        vec![(11, 2), (22, 1)],
        vec![(11, 0), (22, 1), (33, 2)],
        vec![(11, 2), (11, 0), (22, 1)],
        vec![(11, 0), (11, 2), (22, 1)],
    ] {
        let expected = entries.iter().copied().collect::<BTreeMap<_, _>>();
        let replacement = store_inventory::replacement(&current, entries.into_iter());
        assert_eq!(replacement.as_ref(), Some(&expected));
    }
    assert!(store_inventory::replacement(&BTreeMap::new(), [].into_iter()).is_none());
}
