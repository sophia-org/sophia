//! Per-head import evidence (t307). A facade records the first frame its
//! renderer returns for its head, and every later frame across which that
//! renderer's DMA-BUF import cache imported, evicted or changed its live
//! entries. Hits change with every sampled frame, so they never cause a
//! record and are carried only as context, with the snapshot counters.
//!
//! The counters are the returned renderer's own, not a sum over heads. Each
//! record names that renderer by its image-store identity, which is minted
//! once per worker core: two heads served by one shared core print the same
//! identity and the same counters, never two importers.
//!
//! Proof-only and bounded: on only under
//! `SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=final-regions`, the opt-in of the
//! region-frame records these join by owner, output, head and frame, and at
//! most `RECORD_LIMIT` records per facade, then one `capped` record.

use super::super::correlation::LiveRendererFrameCorrelation;
use sophia_renderer_live::LiveNativePersistentRenderStats;

pub(super) const RECORD_LIMIT: u32 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeadImportDecision {
    Initial,
    Changed,
    /// The bound is spent: one record says so, and nothing follows it.
    Capped,
}

#[derive(Debug, Default)]
pub(super) struct HeadImportTrace {
    enabled: bool,
    /// Imports, evictions and live entries at the last record.
    last: Option<(usize, usize, usize)>,
    records: u32,
    capped: bool,
}

impl HeadImportTrace {
    pub(super) fn from_environment() -> Self {
        Self::new(
            std::env::var("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE")
                .is_ok_and(|trace| trace == "final-regions"),
        )
    }

    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// What one returned frame's counters call for, if anything.
    pub(super) fn observe(
        &mut self,
        stats: &LiveNativePersistentRenderStats,
    ) -> Option<HeadImportDecision> {
        if !self.enabled || self.capped {
            return None;
        }
        let cache = stats.import_cache;
        let current = (cache.imports, cache.evictions, cache.live_entries);
        let decision = match self.last {
            None => HeadImportDecision::Initial,
            Some(last) if last != current => HeadImportDecision::Changed,
            Some(_) => return None,
        };
        if self.records == RECORD_LIMIT {
            self.capped = true;
            return Some(HeadImportDecision::Capped);
        }
        self.last = Some(current);
        self.records += 1;
        Some(decision)
    }

    pub(super) fn record(
        &mut self,
        correlation: &LiveRendererFrameCorrelation,
        renderer: u64,
        stats: &LiveNativePersistentRenderStats,
    ) {
        let Some(decision) = self.observe(stats) else {
            return;
        };
        let line = head_import_record(decision, correlation, renderer, stats, self.records);
        tracing::info!("{line}");
    }
}

/// The record's text. Identities a legacy frame lacks are `none`.
pub(super) fn head_import_record(
    decision: HeadImportDecision,
    correlation: &LiveRendererFrameCorrelation,
    renderer: u64,
    stats: &LiveNativePersistentRenderStats,
    records: u32,
) -> String {
    let field = |value: Option<u64>| value.map_or_else(|| "none".to_owned(), |v| v.to_string());
    let native = correlation.native;
    let trace = correlation.trace;
    let output = native
        .map(|n| n.output().raw())
        .or(trace.map(|t| t.output.raw()));
    let head = native
        .map(|n| n.head().raw())
        .or(trace.map(|t| t.head.raw()));
    let identity = format!(
        "renderer={renderer} owner={} output={} head={}",
        field(native.map(|n| n.owner())),
        field(output),
        field(head),
    );
    if decision == HeadImportDecision::Capped {
        return format!(
            "sophia_live_head_renderer_imports schema=1 status=capped {identity} records={records}"
        );
    }
    let cache = stats.import_cache;
    format!(
        "sophia_live_head_renderer_imports schema=1 status=observed reason={} {identity} frame={} target_generation={} scene_generation={} imports={} evictions={} live_entries={} hits={} descriptor_mismatches={} capacity_rejections={} snapshot_captures={} snapshot_promotions={} snapshot_live_entries={} records={records}",
        match decision {
            HeadImportDecision::Initial => "initial",
            _ => "changed",
        },
        field(native.map(|n| n.frame())),
        field(native.map(|n| n.target_generation())),
        field(trace.map(|t| t.scene_generation)),
        cache.imports,
        cache.evictions,
        cache.live_entries,
        cache.hits,
        cache.descriptor_mismatches,
        cache.capacity_rejections,
        stats.snapshot_captures,
        stats.snapshot_promotions,
        stats.snapshot_live_entries,
    )
}

#[path = "../../../../../tests/support/renderer_head_imports.rs"]
mod tests;
