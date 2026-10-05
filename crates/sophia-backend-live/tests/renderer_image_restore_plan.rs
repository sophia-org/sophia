#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
//! Destination planning for a handoff whose heads changed (t306). Each case
//! asserts which store imports which image from which snapshot, not only how
//! many imports there are.

use sophia_backend_live::{
    LiveRendererImageImport, LiveRendererImageRestoreExecution, LiveRendererImageRestoreImport,
    LiveRendererImageRestorePlan, LiveRendererImageRestoreSource, LiveRendererImageRestoreStore,
    LiveRendererImageRetryGate, LiveRendererImageStoreAttempt, classify_live_renderer_image_import,
    execute_live_renderer_image_restore_plan, live_renderer_image_handoff_same_devices,
    plan_live_renderer_image_restore_destinations,
};
use sophia_renderer_live::LiveRendererImageId;
use std::collections::{BTreeMap, BTreeSet};

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
fn larger_optional_images_are_placed_first_so_a_feasible_scene_fits() {
    // REVIEW-CODEX-05 R1: two same-GPU stores of 512 MiB, nothing demanded.
    // Images 1 and 2 (200 MiB each) came from one store, image 3 (400 MiB)
    // from the other. Placing by id spread the 200s and stranded the 400.
    let sources = [
        source(Some(1), &[(1, 200 * MIB), (2, 200 * MIB)]),
        source(Some(1), &[(3, 400 * MIB)]),
    ];
    let stores = [store(Some(1)), store(Some(1))];
    let plan = plan_live_renderer_image_restore_destinations(&sources, &stores, &[]).unwrap();
    assert!(plan.unplaced.is_empty(), "{plan:?}");
    assert_eq!(
        imports(&plan.imports),
        vec![(0, 3, 1), (1, 1, 0), (1, 2, 0)]
    );
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
    // An unknown destination shares a device with neither source, so source
    // order decides. The known source comes first: a planner that wrongly
    // matched None with None would pick the second.
    let sources = [source(Some(5), &[(1, MIB)]), source(None, &[(1, MIB)])];
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

#[test]
fn only_a_refused_import_tries_another_snapshot_and_busy_is_told_from_full() {
    use sophia_renderer_live::LiveRendererScanoutBufferExportDetail as D;
    let classify = |detail| classify_live_renderer_image_import(Err(detail));
    for detail in [
        D::DmaBufImageCreateFailed,
        D::DmaBufImageBindFailed,
        D::DmaBufImportFailed,
    ] {
        assert_eq!(classify(detail), LiveRendererImageImport::Refused);
    }
    // A store with no room waits for storage to change.
    assert_eq!(
        classify(D::RendererImageStoreFull),
        LiveRendererImageImport::Deferred { busy: false }
    );
    // GPU work in flight is retried soon (REVIEW-CODEX-06 R1, -07).
    for detail in [
        D::RendererImageTransferBusy,
        D::WorkerPending,
        D::WorkerQueueFull,
    ] {
        assert_eq!(
            classify(detail),
            LiveRendererImageImport::Deferred { busy: true }
        );
    }
    for detail in [
        D::EglMakeCurrentFailed,
        D::InvalidRendererImageId,
        D::WorkerDisconnected,
    ] {
        assert_eq!(classify(detail), LiveRendererImageImport::Failed(detail));
    }
}

#[test]
fn an_existing_id_is_reported_for_confirmation_not_counted_as_placed() {
    // REVIEW-CODEX-06 R4: false means the store already had the id, possibly
    // only staged; the caller confirms promotion before counting it.
    assert_eq!(
        classify_live_renderer_image_import(Ok(false)),
        LiveRendererImageImport::Existing
    );
    assert_eq!(
        classify_live_renderer_image_import(Ok(true)),
        LiveRendererImageImport::Placed
    );
}

const BUSY: LiveRendererImageStoreAttempt = LiveRendererImageStoreAttempt::Deferred { busy: true };
const FULL: LiveRendererImageStoreAttempt = LiveRendererImageStoreAttempt::Deferred { busy: false };
const REFUSED: LiveRendererImageStoreAttempt = LiveRendererImageStoreAttempt::Refused;
const PLACED: LiveRendererImageStoreAttempt = LiveRendererImageStoreAttempt::Placed;

fn planned(store: usize, raw: u64) -> LiveRendererImageRestoreImport {
    LiveRendererImageRestoreImport {
        store,
        image: image(raw),
        source: 0,
        alternates: Vec::new(),
    }
}

/// Carries out `imports` with each (store, image) answering as `answers`
/// says, and returns what was left plus every (store, image) asked, in order.
fn execute(
    stores: &[LiveRendererImageRestoreStore<u32>],
    imports: Vec<LiveRendererImageRestoreImport>,
    demand: &[(usize, u64)],
    answers: &[((usize, u64), LiveRendererImageStoreAttempt)],
) -> (LiveRendererImageRestoreExecution, Vec<(usize, u64)>) {
    let plan = LiveRendererImageRestorePlan {
        imports,
        ..LiveRendererImageRestorePlan::default()
    };
    let demand = demand
        .iter()
        .map(|&(store, raw)| (store, image(raw)))
        .collect::<BTreeSet<_>>();
    let answers = answers.iter().copied().collect::<BTreeMap<_, _>>();
    let mut asked = Vec::new();
    let execution = execute_live_renderer_image_restore_plan::<_, ()>(
        &plan,
        stores,
        &demand,
        |import, store| {
            let key = (store, import.image.raw());
            asked.push(key);
            Ok(answers[&key])
        },
    )
    .unwrap();
    (execution, asked)
}

/// Whether the owner offers the pending images again before storage changes.
fn retried_before_storage_changes(execution: &LiveRendererImageRestoreExecution) -> bool {
    let mut gate = LiveRendererImageRetryGate::default();
    gate.observe(11, execution.busy);
    gate.due(11)
}

fn missing(execution: &LiveRendererImageRestoreExecution) -> Vec<(usize, u64)> {
    execution
        .missing
        .iter()
        .map(|(store, image)| (*store, image.raw()))
        .collect()
}

fn held(execution: &LiveRendererImageRestoreExecution) -> Vec<(usize, u64)> {
    execution
        .held
        .iter()
        .map(|(store, image)| (*store, image.raw()))
        .collect()
}

#[test]
fn a_busy_store_keeps_its_retry_whatever_the_alternate_store_answers() {
    // REVIEW-CODEX-08: the chosen store is behind GPU work; the alternate is
    // full, refuses, or is itself the busy one. The image stays pending and
    // is retried when the work settles, not only when storage changes.
    let stores = [store(Some(1)), store(Some(1))];
    for (first, second) in [(BUSY, FULL), (BUSY, REFUSED), (FULL, BUSY)] {
        let (execution, asked) = execute(
            &stores,
            vec![planned(0, 7)],
            &[],
            &[((0, 7), first), ((1, 7), second)],
        );
        assert_eq!(asked, [(0, 7), (1, 7)], "{first:?} then {second:?}");
        assert!(held(&execution).is_empty(), "{first:?} then {second:?}");
        assert_eq!(missing(&execution), [(0, 7)], "{first:?} then {second:?}");
        assert!(execution.busy, "{first:?} then {second:?}");
        assert!(
            retried_before_storage_changes(&execution),
            "{first:?} then {second:?}"
        );
    }
}

#[test]
fn an_alternate_store_that_keeps_the_image_leaves_no_retry_behind() {
    let stores = [store(Some(1)), store(Some(1))];
    for first in [BUSY, FULL] {
        let (execution, asked) = execute(
            &stores,
            vec![planned(0, 7)],
            &[],
            &[((0, 7), first), ((1, 7), PLACED)],
        );
        assert_eq!(asked, [(0, 7), (1, 7)], "{first:?}");
        assert_eq!(held(&execution), [(1, 7)], "{first:?}");
        assert!(missing(&execution).is_empty(), "{first:?}");
        assert!(!execution.busy, "{first:?}");
        assert!(!retried_before_storage_changes(&execution), "{first:?}");
    }
}

#[test]
fn stores_that_are_all_full_sleep_until_storage_changes() {
    let stores = [store(Some(1)), store(Some(1))];
    let (execution, asked) = execute(
        &stores,
        vec![planned(0, 7)],
        &[],
        &[((0, 7), FULL), ((1, 7), FULL)],
    );
    assert_eq!(asked, [(0, 7), (1, 7)]);
    assert_eq!(missing(&execution), [(0, 7)]);
    assert!(!execution.busy);
    assert!(!retried_before_storage_changes(&execution));
}

#[test]
fn a_placed_image_does_not_settle_another_images_busy_store() {
    let stores = [store(Some(1)), store(Some(1))];
    let (execution, _) = execute(
        &stores,
        vec![planned(0, 7), planned(0, 8)],
        &[],
        &[
            ((0, 7), BUSY),
            ((1, 7), PLACED),
            ((0, 8), BUSY),
            ((1, 8), FULL),
        ],
    );
    assert_eq!(held(&execution), [(1, 7)]);
    assert_eq!(missing(&execution), [(0, 8)]);
    assert!(execution.busy);
    // The image placed second does not hide the first image's busy store.
    let (execution, _) = execute(
        &stores,
        vec![planned(0, 8), planned(0, 7)],
        &[],
        &[
            ((0, 8), BUSY),
            ((1, 8), FULL),
            ((0, 7), FULL),
            ((1, 7), PLACED),
        ],
    );
    assert_eq!(held(&execution), [(1, 7)]);
    assert!(execution.busy);
}

#[test]
fn a_demanded_destination_is_not_traded_for_another_store() {
    let stores = [store(Some(1)), store(Some(1))];
    let (execution, asked) = execute(
        &stores,
        vec![planned(0, 7)],
        &[(0, 7)],
        &[((0, 7), BUSY), ((1, 7), PLACED)],
    );
    assert_eq!(asked, [(0, 7)]);
    assert!(held(&execution).is_empty());
    assert_eq!(missing(&execution), [(0, 7)]);
    assert!(execution.busy);
}

#[test]
fn alternate_stores_are_tried_same_device_first_and_never_twice_for_one_image() {
    let stores = [store(Some(1)), store(Some(2)), store(Some(1)), store(None)];
    let (execution, asked) = execute(
        &stores,
        vec![planned(0, 7)],
        &[],
        &[
            ((0, 7), FULL),
            ((2, 7), REFUSED),
            ((1, 7), FULL),
            ((3, 7), PLACED),
        ],
    );
    assert_eq!(asked, [(0, 7), (2, 7), (1, 7), (3, 7)]);
    assert_eq!(held(&execution), [(3, 7)]);
    // A store already holding the image from an earlier import is skipped.
    let (execution, asked) = execute(
        &stores,
        vec![planned(3, 7), planned(0, 7)],
        &[(3, 7)],
        &[
            ((3, 7), PLACED),
            ((0, 7), FULL),
            ((2, 7), FULL),
            ((1, 7), FULL),
        ],
    );
    assert_eq!(asked, [(3, 7), (0, 7), (2, 7), (1, 7)]);
    assert_eq!(held(&execution), [(3, 7)]);
    assert_eq!(missing(&execution), [(0, 7)]);
}
