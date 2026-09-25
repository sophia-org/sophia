// Selection rules for merging authority batches into one production cycle.
//
// Composing once per batch made a burst of client draws cost one frame each,
// so displayed content trailed the client by the whole backlog. These pin the
// fail-closed boundaries that make merging safe.

use super::*;
use std::sync::Arc;

/// A closure that mutates a batch before it is merged, so a test can block
/// one merge without spelling the boxed closure type at every use.
type BatchBlocker = Box<dyn Fn(&mut XAuthorityObservedTransactionBatch)>;

fn content_batch(transaction: u64) -> XAuthorityObservedTransactionBatch {
    let mut batch = wm_update_coordinator_batch(TransactionId::from_raw(transaction));
    batch
        .cpu_buffer_updates
        .push(cpu_buffer_replacement(transaction));
    batch
}

fn cpu_buffer_replacement(handle: u64) -> sophia_x_authority::XAuthorityCpuBufferUpdate {
    sophia_x_authority::XAuthorityCpuBufferUpdate::Replace(
        sophia_x_authority::XAuthorityCpuBufferSnapshot {
            handle,
            drawable: sophia_x_authority::XResourceId::new(handle, 1),
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0; 16]),
        },
    )
}

#[test]
fn pure_content_batches_merge_into_one_run() {
    let head = content_batch(1);
    let queued = (2..=6).map(content_batch).collect::<Vec<_>>();
    assert_eq!(
        authority_merge_run_len(&head, queued.iter(), true, AUTHORITY_MERGE_RUN_LIMIT),
        6
    );
}

#[test]
fn a_resource_or_present_batch_ends_the_run() {
    // Each of these carries an edge whose ordering against a later batch in
    // the same cycle is not something merging may reorder.
    let blockers: Vec<BatchBlocker> = vec![
        Box::new(|batch| batch.removed_surfaces.push(SurfaceId::new(9, 1))),
        Box::new(|batch| {
            batch
                .surface_output_reservations
                .push(sophia_protocol::SurfaceOutputReservations {
                    surface: SurfaceId::new(9, 1),
                    reservations: Vec::new(),
                })
        }),
    ];
    for blocker in blockers {
        let head = content_batch(1);
        let mut third = content_batch(3);
        blocker(&mut third);
        let queued = [content_batch(2), third, content_batch(4)];
        assert_eq!(
            authority_merge_run_len(&head, queued.iter(), true, AUTHORITY_MERGE_RUN_LIMIT),
            2,
            "a blocking batch must neither join the run nor be consumed"
        );
    }
}

#[test]
fn a_raster_response_batch_may_open_but_not_join_a_run() {
    let identity = sophia_protocol::SurfaceRasterResponseIdentity {
        transaction: TransactionId::from_raw(77),
        surface: SurfaceId::new(4, 1),
        source_content_generation: 3,
        requirement_generation: 1,
    };

    let mut head = content_batch(1);
    head.raster_responses.push(identity);
    assert_eq!(
        authority_merge_run_len(
            &head,
            [content_batch(2), content_batch(3)].iter(),
            true,
            AUTHORITY_MERGE_RUN_LIMIT,
        ),
        3,
        "a response batch may open a run"
    );

    let mut follower = content_batch(3);
    follower.raster_responses.push(identity);
    assert_eq!(
        authority_merge_run_len(
            &content_batch(1),
            [content_batch(2), follower].iter(),
            true,
            AUTHORITY_MERGE_RUN_LIMIT,
        ),
        2,
        "a response is judged against its own cycle, so it never joins one"
    );
}

#[test]
fn a_busy_admission_pipeline_commits_one_batch_per_cycle() {
    let head = content_batch(1);
    let queued = [content_batch(2), content_batch(3)];
    assert_eq!(
        authority_merge_run_len(&head, queued.iter(), false, AUTHORITY_MERGE_RUN_LIMIT),
        1
    );
}

#[test]
fn a_repeated_transaction_identity_ends_the_run() {
    let head = content_batch(1);
    let queued = [content_batch(2), content_batch(2), content_batch(4)];
    assert_eq!(
        authority_merge_run_len(&head, queued.iter(), true, AUTHORITY_MERGE_RUN_LIMIT),
        2,
        "group bucketing is per projection call, so a repeat would split a group"
    );
}

#[test]
fn the_merge_run_never_exceeds_its_bound() {
    let head = content_batch(1);
    let queued = (2..=200).map(content_batch).collect::<Vec<_>>();
    assert_eq!(authority_merge_run_len(&head, queued.iter(), true, 8), 8);
    assert_eq!(
        authority_merge_run_len(&head, queued.iter(), true, AUTHORITY_MERGE_RUN_LIMIT),
        AUTHORITY_MERGE_RUN_LIMIT
    );
}

#[test]
fn a_metadata_only_batch_neither_merges_nor_counts_as_engine_work() {
    let empty = wm_update_coordinator_batch(TransactionId::from_raw(5));
    assert!(!authority_batch_is_pure_content(&empty));
    assert!(!authority_batch_has_engine_work(&empty));
    assert_eq!(
        authority_merge_run_len(&empty, [content_batch(6)].iter(), true, 16),
        1
    );
    assert!(authority_batch_has_engine_work(&content_batch(7)));
}

#[test]
fn a_single_batch_run_reproduces_the_historical_cadence() {
    let head = content_batch(1);
    assert_eq!(
        authority_merge_run_len(&head, std::iter::empty(), true, AUTHORITY_MERGE_RUN_LIMIT),
        1
    );
    assert_eq!(
        authority_merge_run_len(&head, [content_batch(2)].iter(), true, 1),
        1,
        "a limit of one is exactly today's behavior"
    );
}

#[test]
fn a_run_stays_within_the_runtime_observation_budget() {
    // Every commit contributes one runtime observation, and the session
    // runtime rejects a tick whose batch exceeds its maximum. A physical run
    // failed in KmsSubmit with exactly that error when the run was bounded by
    // batch count instead, so the budget is counted in transactions.
    let head = content_batch(1);
    let queued = (2..=200).map(content_batch).collect::<Vec<_>>();
    let len = authority_merge_run_len(&head, queued.iter(), true, AUTHORITY_MERGE_RUN_LIMIT);

    let committed = std::iter::once(&head)
        .chain(queued.iter().take(len - 1))
        .map(|batch| batch.transactions.len())
        .sum::<usize>();
    assert!(
        committed < sophia_runtime::MAX_SESSION_RUNTIME_OBSERVATION_BATCH,
        "a merged run must leave room for the fixed per-tick observations, \
         got {committed} commits"
    );
}

#[test]
fn a_transaction_heavy_batch_ends_the_run_early() {
    let head = content_batch(1);
    let mut heavy = content_batch(2);
    for index in 0..=AUTHORITY_MERGE_TRANSACTION_LIMIT {
        heavy
            .transactions
            .push(merge_surface_transaction(index as u64));
    }
    assert_eq!(
        authority_merge_run_len(
            &head,
            [heavy, content_batch(3)].iter(),
            true,
            AUTHORITY_MERGE_RUN_LIMIT,
        ),
        1,
        "one oversized batch must not drag the run past the observation budget"
    );
}

fn merge_surface_transaction(index: u64) -> SurfaceTransaction {
    SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(9_000 + index),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(50 + u32::try_from(index).unwrap_or(0), 1),
        namespace: None,
        target_geometry: Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        presentation_extent: Size {
            width: 4,
            height: 4,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            Size {
                width: 4,
                height: 4,
            },
        ),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    }
}
