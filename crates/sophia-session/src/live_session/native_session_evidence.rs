//! Session evidence outlives the replaceable DRM/renderer owner.
use sophia_backend_live::{
    DirectScanoutCost, LivePersistentRenderMetrics, LiveProductionDirectScanoutTotals,
    LiveProductionNativeScanout,
};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub(super) struct NativeEvidenceSnapshot {
    pub submissions: usize,
    pub submit_deferred: usize,
    pub submit_failures: usize,
    pub retirements: usize,
    pub retire_failures: usize,
    pub max_in_flight_ticks: u64,
    pub max_in_flight_per_output: usize,
    pub pending_frame_supersessions: usize,
    pub max_service_skew: usize,
    pub max_submit_to_page_flip: Duration,
    pub callback_accepted: usize,
    pub callback_rejected: usize,
    pub callback_queue_saturated: usize,
    pub nonzero_exports: usize,
    pub kernel_page_flip_timestamps: usize,
    pub kernel_page_flip_timestamp_missing: usize,
    pub vsync_overlap_rejections: usize,
    pub page_flip_phase_rejections: usize,
    pub cursor_updates: usize,
    pub cursor_hidden_updates: usize,
    pub cursor_updates_queued: usize,
    pub cursor_updates_coalesced: usize,
    pub cursor_updates_ridden: usize,
    pub cursor_only_commits: usize,
    pub max_cursor_only_commit: Duration,
    pub cursor_only_commit_total: Duration,
    pub cursor_combined_drops: usize,
    pub cursor_legacy_fallbacks: usize,
    pub cursor_initialization_deferrals: usize,
    pub legacy_cursor_updates_primary_in_flight: usize,
    pub cursor_update_failures: usize,
    pub max_cursor_initialization: Duration,
    pub max_cursor_update: Duration,
    pub max_cursor_queue_delay: Duration,
    pub mixed_exports: usize,
    pub export_attempts: usize,
    pub resources: LivePersistentRenderMetrics,
    pub direct: LiveProductionDirectScanoutTotals,
    pub cost: DirectScanoutCost,
    pub verdicts: [usize; sophia_engine::DirectScanoutVerdict::COUNT],
    pub in_flight: bool,
    pub cleanup_pending: bool,
}

impl NativeEvidenceSnapshot {
    pub fn capture(native: &LiveProductionNativeScanout) -> Self {
        let mut verdicts = [0usize; sophia_engine::DirectScanoutVerdict::COUNT];
        for (_, _, counts) in native.direct_scanout_head_verdicts() {
            for (total, count) in verdicts.iter_mut().zip(counts) {
                *total = total.saturating_add(count);
            }
        }
        Self {
            submissions: native.submissions,
            submit_deferred: native.submit_deferred,
            submit_failures: native.submit_failures,
            retirements: native.retirements,
            retire_failures: native.retire_failures,
            max_in_flight_ticks: native.max_in_flight_ticks,
            max_in_flight_per_output: native.max_in_flight_per_output,
            pending_frame_supersessions: native.pending_frame_supersessions,
            max_service_skew: native.max_service_skew,
            max_submit_to_page_flip: native.max_submit_to_page_flip,
            callback_accepted: native.callback_accepted,
            callback_rejected: native.callback_rejected,
            callback_queue_saturated: native.callback_queue_saturated,
            nonzero_exports: native.nonzero_exports,
            kernel_page_flip_timestamps: native.kernel_page_flip_timestamps,
            kernel_page_flip_timestamp_missing: native.kernel_page_flip_timestamp_missing,
            vsync_overlap_rejections: native.vsync_overlap_rejections,
            page_flip_phase_rejections: native.page_flip_phase_rejections,
            cursor_updates: native.cursor_updates,
            cursor_hidden_updates: native.cursor_hidden_updates,
            cursor_updates_queued: native.cursor_updates_queued,
            cursor_updates_coalesced: native.cursor_updates_coalesced,
            cursor_updates_ridden: native.cursor_updates_ridden,
            cursor_only_commits: native.cursor_only_commits,
            max_cursor_only_commit: native.max_cursor_only_commit,
            cursor_only_commit_total: native.cursor_only_commit_total,
            cursor_combined_drops: native.cursor_combined_drops,
            cursor_legacy_fallbacks: native.cursor_legacy_fallbacks,
            cursor_initialization_deferrals: native.cursor_initialization_deferrals,
            legacy_cursor_updates_primary_in_flight: native.legacy_cursor_updates_primary_in_flight,
            cursor_update_failures: native.cursor_update_failures,
            max_cursor_initialization: native.max_cursor_initialization,
            max_cursor_update: native.max_cursor_update,
            max_cursor_queue_delay: native.max_cursor_queue_delay,
            mixed_exports: native.mixed_exports(),
            export_attempts: native.export_attempts(),
            resources: native.persistent_render_metrics(),
            direct: native.direct_scanout_totals(),
            cost: native.direct_scanout_cost(),
            verdicts,
            in_flight: native.any_head_scanout_in_flight(),
            cleanup_pending: native.any_head_cleanup_pending(),
        }
    }

    /// Add disjoint lifetimes. Gauges describe only the most recent owner;
    /// an unsettled retired owner remains a separate, sticky obligation.
    pub fn append(&mut self, next: &Self) {
        self.submissions = self.submissions.saturating_add(next.submissions);
        self.submit_deferred = self.submit_deferred.saturating_add(next.submit_deferred);
        self.submit_failures = self.submit_failures.saturating_add(next.submit_failures);
        self.retirements = self.retirements.saturating_add(next.retirements);
        self.retire_failures = self.retire_failures.saturating_add(next.retire_failures);
        self.max_in_flight_ticks = self.max_in_flight_ticks.max(next.max_in_flight_ticks);
        self.max_in_flight_per_output = self
            .max_in_flight_per_output
            .max(next.max_in_flight_per_output);
        self.pending_frame_supersessions = self
            .pending_frame_supersessions
            .saturating_add(next.pending_frame_supersessions);
        self.max_service_skew = self.max_service_skew.max(next.max_service_skew);
        self.max_submit_to_page_flip = self
            .max_submit_to_page_flip
            .max(next.max_submit_to_page_flip);
        self.callback_accepted = self
            .callback_accepted
            .saturating_add(next.callback_accepted);
        self.callback_rejected = self
            .callback_rejected
            .saturating_add(next.callback_rejected);
        self.callback_queue_saturated = self
            .callback_queue_saturated
            .saturating_add(next.callback_queue_saturated);
        self.nonzero_exports = self.nonzero_exports.saturating_add(next.nonzero_exports);
        self.kernel_page_flip_timestamps = self
            .kernel_page_flip_timestamps
            .saturating_add(next.kernel_page_flip_timestamps);
        self.kernel_page_flip_timestamp_missing = self
            .kernel_page_flip_timestamp_missing
            .saturating_add(next.kernel_page_flip_timestamp_missing);
        self.vsync_overlap_rejections = self
            .vsync_overlap_rejections
            .saturating_add(next.vsync_overlap_rejections);
        self.page_flip_phase_rejections = self
            .page_flip_phase_rejections
            .saturating_add(next.page_flip_phase_rejections);
        self.cursor_updates = self.cursor_updates.saturating_add(next.cursor_updates);
        self.cursor_hidden_updates = self
            .cursor_hidden_updates
            .saturating_add(next.cursor_hidden_updates);
        self.cursor_updates_queued = self
            .cursor_updates_queued
            .saturating_add(next.cursor_updates_queued);
        self.cursor_updates_coalesced = self
            .cursor_updates_coalesced
            .saturating_add(next.cursor_updates_coalesced);
        self.cursor_updates_ridden = self
            .cursor_updates_ridden
            .saturating_add(next.cursor_updates_ridden);
        self.cursor_only_commits = self
            .cursor_only_commits
            .saturating_add(next.cursor_only_commits);
        self.max_cursor_only_commit = self.max_cursor_only_commit.max(next.max_cursor_only_commit);
        self.cursor_only_commit_total = self
            .cursor_only_commit_total
            .saturating_add(next.cursor_only_commit_total);
        self.cursor_combined_drops = self
            .cursor_combined_drops
            .saturating_add(next.cursor_combined_drops);
        self.cursor_legacy_fallbacks = self
            .cursor_legacy_fallbacks
            .saturating_add(next.cursor_legacy_fallbacks);
        self.cursor_initialization_deferrals = self
            .cursor_initialization_deferrals
            .saturating_add(next.cursor_initialization_deferrals);
        self.legacy_cursor_updates_primary_in_flight = self
            .legacy_cursor_updates_primary_in_flight
            .saturating_add(next.legacy_cursor_updates_primary_in_flight);
        self.cursor_update_failures = self
            .cursor_update_failures
            .saturating_add(next.cursor_update_failures);
        self.max_cursor_initialization = self
            .max_cursor_initialization
            .max(next.max_cursor_initialization);
        self.max_cursor_update = self.max_cursor_update.max(next.max_cursor_update);
        self.max_cursor_queue_delay = self.max_cursor_queue_delay.max(next.max_cursor_queue_delay);
        self.mixed_exports = self.mixed_exports.saturating_add(next.mixed_exports);
        self.export_attempts = self.export_attempts.saturating_add(next.export_attempts);
        self.resources.target_creations = self
            .resources
            .target_creations
            .saturating_add(next.resources.target_creations);
        self.resources.target_recreations = self
            .resources
            .target_recreations
            .saturating_add(next.resources.target_recreations);
        self.resources.pipeline_creations = self
            .resources
            .pipeline_creations
            .saturating_add(next.resources.pipeline_creations);
        self.resources.frame_surface_creations = self
            .resources
            .frame_surface_creations
            .saturating_add(next.resources.frame_surface_creations);
        self.resources.cpu_target_creations = self
            .resources
            .cpu_target_creations
            .saturating_add(next.resources.cpu_target_creations);
        self.resources.dmabuf_target_creations = self
            .resources
            .dmabuf_target_creations
            .saturating_add(next.resources.dmabuf_target_creations);
        self.resources.composition_target_creations = self
            .resources
            .composition_target_creations
            .saturating_add(next.resources.composition_target_creations);
        self.resources.composition_target_reuses = self
            .resources
            .composition_target_reuses
            .saturating_add(next.resources.composition_target_reuses);
        self.resources.generation_replacements = self
            .resources
            .generation_replacements
            .saturating_add(next.resources.generation_replacements);
        self.resources.recovery_replacements = self
            .resources
            .recovery_replacements
            .saturating_add(next.resources.recovery_replacements);
        self.resources.uploads = self
            .resources
            .uploads
            .saturating_add(next.resources.uploads);
        self.resources.snapshot_captures = self
            .resources
            .snapshot_captures
            .saturating_add(next.resources.snapshot_captures);
        self.resources.snapshot_promotions = self
            .resources
            .snapshot_promotions
            .saturating_add(next.resources.snapshot_promotions);
        self.resources.snapshot_rollbacks = self
            .resources
            .snapshot_rollbacks
            .saturating_add(next.resources.snapshot_rollbacks);
        self.resources.snapshot_evictions = self
            .resources
            .snapshot_evictions
            .saturating_add(next.resources.snapshot_evictions);
        self.resources.snapshot_live_entries = next.resources.snapshot_live_entries;
        self.resources.snapshot_live_bytes = next.resources.snapshot_live_bytes;
        self.resources.import_cache_imports = self
            .resources
            .import_cache_imports
            .saturating_add(next.resources.import_cache_imports);
        self.resources.import_cache_hits = self
            .resources
            .import_cache_hits
            .saturating_add(next.resources.import_cache_hits);
        self.resources.import_cache_evictions = self
            .resources
            .import_cache_evictions
            .saturating_add(next.resources.import_cache_evictions);
        self.resources.import_cache_live_entries = next.resources.import_cache_live_entries;
        self.resources.import_cache_descriptor_mismatches = self
            .resources
            .import_cache_descriptor_mismatches
            .saturating_add(next.resources.import_cache_descriptor_mismatches);
        self.resources.import_cache_capacity_rejections = self
            .resources
            .import_cache_capacity_rejections
            .saturating_add(next.resources.import_cache_capacity_rejections);
        self.resources.exact_nearest_draws = self
            .resources
            .exact_nearest_draws
            .saturating_add(next.resources.exact_nearest_draws);
        self.resources.sharp_downscale_draws = self
            .resources
            .sharp_downscale_draws
            .saturating_add(next.resources.sharp_downscale_draws);
        self.resources.sharp_upscale_draws = self
            .resources
            .sharp_upscale_draws
            .saturating_add(next.resources.sharp_upscale_draws);
        self.resources.linear_fallback_draws = self
            .resources
            .linear_fallback_draws
            .saturating_add(next.resources.linear_fallback_draws);
        self.resources.worker_requests = self
            .resources
            .worker_requests
            .saturating_add(next.resources.worker_requests);
        self.resources.worker_completions = self
            .resources
            .worker_completions
            .saturating_add(next.resources.worker_completions);
        self.resources.worker_failures = self
            .resources
            .worker_failures
            .saturating_add(next.resources.worker_failures);
        self.resources.worker_soft_stalls = self
            .resources
            .worker_soft_stalls
            .saturating_add(next.resources.worker_soft_stalls);
        self.resources.worker_hard_stalls = self
            .resources
            .worker_hard_stalls
            .saturating_add(next.resources.worker_hard_stalls);
        self.resources.worker_release_enqueue_failures = self
            .resources
            .worker_release_enqueue_failures
            .saturating_add(next.resources.worker_release_enqueue_failures);
        self.resources.renderer_workers = next.resources.renderer_workers;
        self.resources.worker_result_misroutes = self
            .resources
            .worker_result_misroutes
            .saturating_add(next.resources.worker_result_misroutes);
        self.resources.frame_slot_acquisitions = self
            .resources
            .frame_slot_acquisitions
            .saturating_add(next.resources.frame_slot_acquisitions);
        self.resources.frame_slot_reuses = self
            .resources
            .frame_slot_reuses
            .saturating_add(next.resources.frame_slot_reuses);
        self.resources.frame_slot_deferrals = self
            .resources
            .frame_slot_deferrals
            .saturating_add(next.resources.frame_slot_deferrals);
        self.resources.frame_slot_stale_releases = self
            .resources
            .frame_slot_stale_releases
            .saturating_add(next.resources.frame_slot_stale_releases);
        self.resources.frame_slots_leased = next.resources.frame_slots_leased;
        self.resources.frame_slots_high_watermark = self
            .resources
            .frame_slots_high_watermark
            .max(next.resources.frame_slots_high_watermark);
        self.resources.frame_slot_partial_repaints = self
            .resources
            .frame_slot_partial_repaints
            .saturating_add(next.resources.frame_slot_partial_repaints);
        self.resources.frame_slot_full_repaints = self
            .resources
            .frame_slot_full_repaints
            .saturating_add(next.resources.frame_slot_full_repaints);
        self.resources.frame_slot_history_invalidations = self
            .resources
            .frame_slot_history_invalidations
            .saturating_add(next.resources.frame_slot_history_invalidations);
        self.resources.frame_slot_history_records = next.resources.frame_slot_history_records;
        self.resources.max_worker_request = self
            .resources
            .max_worker_request
            .max(next.resources.max_worker_request);
        self.resources.max_target_create = self
            .resources
            .max_target_create
            .max(next.resources.max_target_create);
        self.resources.max_frame_surface_create = self
            .resources
            .max_frame_surface_create
            .max(next.resources.max_frame_surface_create);
        self.resources.max_render = self.resources.max_render.max(next.resources.max_render);
        self.resources.max_upload = self.resources.max_upload.max(next.resources.max_upload);
        self.direct.attempts = self.direct.attempts.saturating_add(next.direct.attempts);
        self.direct.flips = self.direct.flips.saturating_add(next.direct.flips);
        self.direct.tests = self.direct.tests.saturating_add(next.direct.tests);
        self.direct.test_rejections = self
            .direct
            .test_rejections
            .saturating_add(next.direct.test_rejections);
        self.direct.refusals = self.direct.refusals.saturating_add(next.direct.refusals);
        self.direct.unsupported = self
            .direct
            .unsupported
            .saturating_add(next.direct.unsupported);
        self.direct.fallbacks = self.direct.fallbacks.saturating_add(next.direct.fallbacks);
        self.cost.merge(&next.cost);
        for (total, count) in self.verdicts.iter_mut().zip(next.verdicts) {
            *total = total.saturating_add(count);
        }
        self.in_flight |= next.in_flight;
        self.cleanup_pending |= next.cleanup_pending;
    }

    pub fn clean(&self) -> bool {
        self.submissions > 0
            && self.retirements > 0
            && self.nonzero_exports > 0
            && self.submit_failures == 0
            && self.retire_failures == 0
            && self.callback_rejected == 0
            && self.callback_queue_saturated == 0
            && self.vsync_overlap_rejections == 0
            && self.page_flip_phase_rejections == 0
            && !self.in_flight
            && !self.cleanup_pending
    }
}

#[derive(Debug, Default)]
pub(super) struct NativeSessionEvidence {
    epoch: u64,
    active: bool,
    retained: NativeEvidenceSnapshot,
    pub unsettled_owners: usize,
    pub settlement_failures: usize,
}

impl NativeSessionEvidence {
    pub fn observe_settlement(&mut self, drained: bool, abandoned: usize) {
        if !drained || abandoned != 0 {
            self.settlement_failures = self.settlement_failures.saturating_add(1);
        }
    }

    pub fn enabled(&self) -> bool {
        self.epoch != 0
    }

    pub fn open(&mut self, reason: &str) {
        assert!(
            !self.active,
            "native evidence owner replaced without closing"
        );
        self.epoch = self
            .epoch
            .checked_add(1)
            .expect("native evidence epoch exhausted");
        self.active = true;
        crate::session_println!(
            "sophia_live_native_owner schema=1 status=opened epoch={} reason={reason}",
            self.epoch
        );
    }

    pub fn close(&mut self, snapshot: &NativeEvidenceSnapshot, reason: &str) {
        assert!(self.active, "native evidence owner closed twice");
        self.active = false;
        let settled = !snapshot.in_flight && !snapshot.cleanup_pending;
        self.unsettled_owners = self.unsettled_owners.saturating_add(usize::from(!settled));
        self.retained.append(snapshot);
        crate::session_println!(
            "sophia_live_native_owner schema=1 status=closed epoch={} reason={reason} settled={settled} submissions={} retirements={} submit_failures={} retire_failures={} in_flight={} cleanup_pending={} settlement_failures={}",
            self.epoch,
            snapshot.submissions,
            snapshot.retirements,
            snapshot.submit_failures,
            snapshot.retire_failures,
            snapshot.in_flight,
            snapshot.cleanup_pending,
            self.settlement_failures
        );
    }

    pub fn snapshot(
        &self,
        current: Option<&LiveProductionNativeScanout>,
    ) -> NativeEvidenceSnapshot {
        let mut result = self.retained.clone();
        // A missing owner contributes zero gauges, never zero history.
        result.append(&current.map_or_else(
            NativeEvidenceSnapshot::default,
            NativeEvidenceSnapshot::capture,
        ));
        result
    }
}

#[path = "../../tests/support/native_session_evidence.rs"]
mod tests;
