#![cfg(test)]

use super::super::super::correlation::LiveRendererFrameCorrelation;
use super::{HeadImportDecision, HeadImportTrace, RECORD_LIMIT, head_import_record};
use sophia_renderer_live::{LiveCompositionTrace, LiveNativePersistentRenderStats};

fn stats(
    imports: usize,
    evictions: usize,
    live: usize,
    hits: usize,
) -> LiveNativePersistentRenderStats {
    let mut stats = LiveNativePersistentRenderStats::default();
    stats.import_cache.imports = imports;
    stats.import_cache.evictions = evictions;
    stats.import_cache.live_entries = live;
    stats.import_cache.hits = hits;
    stats
}

#[test]
fn off_by_default_records_nothing() {
    let mut trace = HeadImportTrace::new(false);
    assert_eq!(trace.observe(&stats(1, 0, 1, 0)), None);
    assert_eq!(trace.observe(&stats(2, 1, 1, 0)), None);
}

#[test]
fn records_the_first_frame_and_import_changes_but_never_hits_alone() {
    let mut trace = HeadImportTrace::new(true);
    assert_eq!(
        trace.observe(&stats(0, 0, 0, 0)),
        Some(HeadImportDecision::Initial)
    );
    // Hits move with every sampled frame and are context only.
    for hits in 1..10 {
        assert_eq!(trace.observe(&stats(0, 0, 0, hits)), None);
    }
    assert_eq!(
        trace.observe(&stats(1, 0, 0, 10)),
        Some(HeadImportDecision::Changed)
    );
    assert_eq!(
        trace.observe(&stats(1, 0, 1, 11)),
        Some(HeadImportDecision::Changed)
    );
    assert_eq!(
        trace.observe(&stats(1, 1, 1, 12)),
        Some(HeadImportDecision::Changed)
    );
    assert_eq!(trace.observe(&stats(1, 1, 1, 13)), None);
}

#[test]
fn the_bound_ends_in_one_capped_record() {
    let mut trace = HeadImportTrace::new(true);
    for imports in 0..RECORD_LIMIT as usize {
        assert!(trace.observe(&stats(imports, 0, 1, 0)).is_some());
    }
    let next = RECORD_LIMIT as usize;
    assert_eq!(
        trace.observe(&stats(next, 0, 1, 0)),
        Some(HeadImportDecision::Capped)
    );
    assert_eq!(trace.observe(&stats(next + 1, 0, 1, 0)), None);
    assert_eq!(trace.observe(&stats(next + 2, 3, 2, 0)), None);
}

#[test]
fn records_carry_the_region_frame_identities() {
    let owner = crate::NativeFrameOwner::new();
    let output = sophia_protocol::OutputId::from_raw(1);
    let head = sophia_engine::RenderHeadId::from_raw(2);
    let correlation = LiveRendererFrameCorrelation {
        native: Some(owner.frame(output, head, 3, 9)),
        request: None,
        trace: Some(LiveCompositionTrace {
            output,
            head,
            scene_generation: 40,
        }),
        direct_scanout: None,
    };
    let mut counters = stats(2, 1, 1, 7);
    counters.snapshot_captures = 4;
    counters.snapshot_promotions = 3;
    counters.snapshot_live_entries = 1;
    assert_eq!(
        head_import_record(HeadImportDecision::Changed, &correlation, 5, &counters, 2),
        format!(
            "sophia_live_head_renderer_imports schema=1 status=observed reason=changed renderer=5 owner={} output=1 head=2 frame=9 target_generation=3 scene_generation=40 imports=2 evictions=1 live_entries=1 hits=7 descriptor_mismatches=0 capacity_rejections=0 snapshot_captures=4 snapshot_promotions=3 snapshot_live_entries=1 records=2",
            owner.raw()
        )
    );
    assert_eq!(
        head_import_record(HeadImportDecision::Capped, &correlation, 5, &counters, 256),
        format!(
            "sophia_live_head_renderer_imports schema=1 status=capped renderer=5 owner={} output=1 head=2 records=256",
            owner.raw()
        )
    );
}

#[test]
fn a_legacy_frame_names_its_missing_identities() {
    let correlation = LiveRendererFrameCorrelation {
        native: None,
        request: None,
        trace: None,
        direct_scanout: None,
    };
    assert_eq!(
        head_import_record(
            HeadImportDecision::Initial,
            &correlation,
            1,
            &stats(0, 0, 0, 0),
            1
        ),
        "sophia_live_head_renderer_imports schema=1 status=observed reason=initial renderer=1 owner=none output=none head=none frame=none target_generation=none scene_generation=none imports=0 evictions=0 live_entries=0 hits=0 descriptor_mismatches=0 capacity_rejections=0 snapshot_captures=0 snapshot_promotions=0 snapshot_live_entries=0 records=1"
    );
}
