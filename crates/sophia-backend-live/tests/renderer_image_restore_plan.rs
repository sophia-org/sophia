#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
//! Destination planning for a handoff whose heads changed (t306). Each case
//! asserts which store imports which image from which snapshot, not only how
//! many imports there are.

use sophia_backend_live::{
    LiveRendererImageRestoreImport, LiveRendererImageRestoreSource, LiveRendererImageRestoreStore,
    live_renderer_image_handoff_same_devices, plan_live_renderer_image_restore_destinations,
};
use sophia_renderer_live::LiveRendererImageId;

const MIB: u64 = 1024 * 1024;
const BUDGET: u64 = 512 * MIB;

fn image(raw: u64) -> LiveRendererImageId {
    LiveRendererImageId::from_raw(raw)
}

fn source(device: Option<u32>, images: &[(u64, u64)]) -> LiveRendererImageRestoreSource<u32> {
    LiveRendererImageRestoreSource {
        device,
        images: images
            .iter()
            .map(|&(id, bytes)| (image(id), bytes))
            .collect(),
    }
}

fn store(device: Option<u32>) -> LiveRendererImageRestoreStore<u32> {
    LiveRendererImageRestoreStore {
        device,
        free_bytes: BUDGET,
        free_entries: 256,
    }
}

/// (store, image, source) for every import, in plan order.
fn imports(plan: &[LiveRendererImageRestoreImport]) -> Vec<(usize, u64, usize)> {
    plan.iter()
        .map(|import| (import.store, import.image.raw(), import.source))
        .collect()
}

#[test]
fn removing_the_first_card_moves_its_images_by_device_not_by_index() {
    // Card A (device 10) held image 1 and was unplugged; card B (device 20)
    // held image 2 and is now the only store, at index 0, as card A was.
    let sources = [source(Some(10), &[(1, MIB)]), source(Some(20), &[(2, MIB)])];
    let stores = [store(Some(20))];
    let plan =
        plan_live_renderer_image_restore_destinations(&sources, &stores, &[(0, image(2))]).unwrap();
    // The demanded image comes from its own device; the lost card's image is
    // still placed once, from the only snapshot that holds it.
    assert_eq!(imports(&plan.imports), vec![(0, 2, 1), (0, 1, 0)]);
    assert!(plan.refused_demand.is_empty() && plan.unplaced.is_empty());
}

#[test]
fn a_same_gpu_reshuffle_places_each_demand_where_it_is_sampled() {
    // One GPU; three retired heads, two replacement stores in another order.
    let sources = [
        source(Some(7), &[(1, MIB), (2, MIB)]),
        source(Some(7), &[(3, MIB)]),
        source(Some(7), &[(4, MIB)]),
    ];
    let stores = [store(Some(7)), store(Some(7))];
    let demand = [(1, image(1)), (0, image(3)), (1, image(3))];
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &demand).unwrap();
    assert_eq!(
        imports(&plan.imports),
        // Demand first in (store, image) order, then images 2 and 4 once each,
        // in the store with more room left.
        vec![(0, 3, 1), (1, 1, 0), (1, 3, 1), (0, 2, 0), (0, 4, 2)]
    );
}

#[test]
fn a_shared_store_is_charged_once_for_an_image_two_outputs_sample() {
    let sources = [source(Some(1), &[(5, 100 * MIB)])];
    let stores = [LiveRendererImageRestoreStore {
        device: Some(1),
        free_bytes: 150 * MIB,
        free_entries: 2,
    }];
    // Two outputs on one shared core name the same store.
    let demand = [(0, image(5)), (0, image(5))];
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &demand).unwrap();
    assert_eq!(imports(&plan.imports), vec![(0, 5, 0)]);
}

#[test]
fn an_image_only_the_unplugged_head_held_is_kept_for_a_later_move() {
    // The window sits on the lost head; nothing samples it until the WM moves
    // it, but it must stay owned so the cold migration has a donor.
    let sources = [source(Some(3), &[(8, MIB)]), source(Some(3), &[])];
    let stores = [store(Some(3))];
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &[]).unwrap();
    assert_eq!(imports(&plan.imports), vec![(0, 8, 0)]);
    assert!(plan.unplaced.is_empty());
}

#[test]
fn disjoint_working_sets_that_fit_their_stores_are_not_replicated() {
    // Two stores each held 300 MiB of different images. Copying the union
    // into each would need 600 MiB a store; placing by demand needs 300.
    let a = (1..=3).map(|id| (id, 100 * MIB)).collect::<Vec<_>>();
    let b = (11..=13).map(|id| (id, 100 * MIB)).collect::<Vec<_>>();
    let sources = [source(Some(1), &a), source(Some(1), &b)];
    let stores = [store(Some(1)), store(Some(1))];
    let demand = a
        .iter()
        .map(|&(id, _)| (0, image(id)))
        .chain(b.iter().map(|&(id, _)| (1, image(id))))
        .collect::<Vec<_>>();
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &demand).unwrap();
    let mut placed = imports(&plan.imports);
    placed.sort_unstable();
    assert_eq!(
        placed,
        vec![
            (0, 1, 0),
            (0, 2, 0),
            (0, 3, 0),
            (1, 11, 1),
            (1, 12, 1),
            (1, 13, 1)
        ]
    );
    assert!(plan.unplaced.is_empty() && plan.refused_demand.is_empty());
}

#[test]
fn when_one_store_survives_the_overflow_is_reported_not_an_error() {
    // Store A's 300 MiB is demanded; store B's 300 MiB has nowhere to go but
    // the 212 MiB left: two images fit, the third is unplaced.
    let a = (1..=3).map(|id| (id, 100 * MIB)).collect::<Vec<_>>();
    let b = (11..=13).map(|id| (id, 100 * MIB)).collect::<Vec<_>>();
    let sources = [source(Some(1), &a), source(Some(1), &b)];
    let stores = [store(Some(1))];
    let demand = a.iter().map(|&(id, _)| (0, image(id))).collect::<Vec<_>>();
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &demand).unwrap();
    assert_eq!(
        imports(&plan.imports),
        vec![(0, 1, 0), (0, 2, 0), (0, 3, 0), (0, 11, 1), (0, 12, 1)]
    );
    assert_eq!(plan.unplaced, vec![image(13)]);
    assert!(plan.refused_demand.is_empty());
}

#[test]
fn a_demand_that_does_not_fit_is_refused_and_the_image_still_kept_elsewhere() {
    let sources = [source(Some(1), &[(1, 400 * MIB)])];
    let full = LiveRendererImageRestoreStore {
        device: Some(1),
        free_bytes: 100 * MIB,
        free_entries: 256,
    };
    let stores = [full, store(Some(1))];
    let plan =
        plan_live_renderer_image_restore_destinations(&sources, &stores, &[(0, image(1))]).unwrap();
    assert_eq!(plan.refused_demand, vec![(0, image(1))]);
    // Store 1 keeps it owned, so the cold migration can serve store 0 later.
    assert_eq!(imports(&plan.imports), vec![(1, 1, 0)]);
}

#[test]
fn the_entry_bound_counts_as_well_as_bytes() {
    let sources = [source(Some(1), &[(1, MIB), (2, MIB)])];
    let stores = [LiveRendererImageRestoreStore {
        device: Some(1),
        free_bytes: BUDGET,
        free_entries: 1,
    }];
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &[]).unwrap();
    assert_eq!(imports(&plan.imports), vec![(0, 1, 0)]);
    assert_eq!(plan.unplaced, vec![image(2)]);
}

#[test]
fn a_refused_import_has_the_other_snapshots_to_try_same_device_first() {
    // Three heads held image 1: two on another GPU, one on the store's.
    let sources = [
        source(Some(9), &[(1, MIB)]),
        source(Some(4), &[(1, MIB)]),
        source(Some(9), &[(1, MIB)]),
    ];
    let stores = [store(Some(4))];
    let plan =
        plan_live_renderer_image_restore_destinations(&sources, &stores, &[(0, image(1))]).unwrap();
    assert_eq!(plan.imports.len(), 1);
    assert_eq!(plan.imports[0].source, 1);
    assert_eq!(plan.imports[0].alternates, vec![0, 2]);
}

#[test]
fn an_unknown_device_is_never_the_same_device() {
    // Neither source is known to share the store's device, so source order
    // decides; an unknown identity on both sides is not a match.
    let sources = [source(None, &[(1, MIB)]), source(Some(5), &[(1, MIB)])];
    let stores = [store(None)];
    let plan =
        plan_live_renderer_image_restore_destinations(&sources, &stores, &[(0, image(1))]).unwrap();
    assert_eq!(
        (plan.imports[0].source, &plan.imports[0].alternates),
        (0, &vec![1])
    );
    assert!(live_renderer_image_handoff_same_devices([
        (Some(2), Some(2)),
        (Some(3), Some(3))
    ]));
    assert!(!live_renderer_image_handoff_same_devices([
        (Some(2), Some(2)),
        (None, None)
    ]));
    assert!(!live_renderer_image_handoff_same_devices([(
        Some(2),
        Some(3)
    )]));
}

#[test]
fn a_plan_against_inconsistent_inputs_is_refused() {
    let one = [source(Some(1), &[(1, MIB)])];
    let stores = [store(Some(1))];
    for (sources, demand) in [
        (vec![source(Some(1), &[(0, MIB)])], vec![]),
        (vec![source(Some(1), &[(1, MIB), (1, MIB)])], vec![]),
        (one.to_vec(), vec![(1, image(1))]),
        (one.to_vec(), vec![(0, image(2))]),
    ] {
        assert!(
            plan_live_renderer_image_restore_destinations(&sources, &stores, &demand).is_err(),
            "{sources:?} {demand:?}"
        );
    }
}
