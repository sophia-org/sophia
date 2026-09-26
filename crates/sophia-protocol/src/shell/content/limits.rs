//! Wire-neutral content limits: the advertised per-connection resource profile.
use super::ContentGrant;

/// Immutable limits for one granted connection. Defaults are the r5 ADR profile;
/// a session may impose tighter coherent bounds, never infer permission from them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentLimits {
    pub grant: ContentGrant,
    pub limits_generation: u64,
    pub max_resource_bytes: u64,
    pub max_staging_bytes: u64,
    pub max_resident_bytes: u64,
    pub max_retiring_bytes: u64,
    pub max_session_retiring_bytes: u64,
    pub pixel_format_mask: u64,
    pub effect_mask: u64,
    pub max_frame_payload: u32,
    pub max_chunk_bytes: u32,
    pub max_width_px: u32,
    pub max_height_px: u32,
    pub max_live_resources: u32,
    pub max_resource_ids: u32,
    pub max_open_transfers: u32,
    pub max_outputs: u32,
    pub max_allocations_total: u32,
    pub max_allocations_per_output: u32,
    pub max_panels_per_output: u32,
    pub max_popouts_per_output: u32,
    pub max_candidate_surfaces: u32,
    pub max_candidate_placements: u32,
    pub max_candidate_targets: u32,
    pub max_candidate_bytes: u32,
    pub max_pending_allocation_requests: u32,
    pub max_open_candidates_total: u32,
    pub max_open_candidates_per_output: u32,
    pub max_pending_candidates_total: u32,
    pub max_pending_candidates_per_output: u32,
    pub max_pending_actions: u32,
    pub max_frame_demands_per_output: u32,
    pub max_control_records: u32,
    pub reserved_control_queue_bytes: u32,
    pub max_input_queue_bytes: u32,
    pub max_output_queue_bytes: u32,
    pub max_frames_per_service_tick: u32,
    pub max_panel_extent: u32,
    pub max_popout_extent_px: u32,
    pub max_reservation_extent: u32,
    pub max_content_coverage_percent: u32,
    pub max_margin_logical: u32,
    pub max_scale_numerator: u32,
    pub max_scale_denominator: u32,
    pub allocation_timeout_ms: u32,
    pub transfer_timeout_ms: u32,
    pub transfer_idle_timeout_ms: u32,
    pub candidate_timeout_ms: u32,
    pub preparation_timeout_ms: u32,
    pub presentation_timeout_ms: u32,
    pub action_ack_timeout_ms: u32,
    pub permit_timeout_ms: u32,
    pub peer_write_timeout_ms: u32,
    pub max_candidate_rate_millihz: u32,
}

impl ContentLimits {
    /// Validate an advertised profile before allocating or accepting obligations.
    pub fn validate(&self) -> Result<(), crate::InvalidRecord> {
        let cap = Self::prototype(self.grant);
        let invalid = || crate::InvalidRecord("content limits");
        if self.grant.connection_epoch == 0
            || self.grant.content_grant_epoch == 0
            || self.limits_generation == 0
            || self.pixel_format_mask != 1
            || self.effect_mask != 0
        {
            return Err(invalid());
        }
        if self.max_resource_bytes > cap.max_resource_bytes {
            return Err(invalid());
        }
        if self.max_staging_bytes > cap.max_staging_bytes {
            return Err(invalid());
        }
        if self.max_resident_bytes > cap.max_resident_bytes {
            return Err(invalid());
        }
        if self.max_retiring_bytes > cap.max_retiring_bytes {
            return Err(invalid());
        }
        if self.max_session_retiring_bytes > cap.max_session_retiring_bytes {
            return Err(invalid());
        }
        if self.max_frame_payload > cap.max_frame_payload {
            return Err(invalid());
        }
        if self.max_chunk_bytes > cap.max_chunk_bytes {
            return Err(invalid());
        }
        if self.max_width_px > cap.max_width_px {
            return Err(invalid());
        }
        if self.max_height_px > cap.max_height_px {
            return Err(invalid());
        }
        if self.max_live_resources > cap.max_live_resources {
            return Err(invalid());
        }
        if self.max_resource_ids > cap.max_resource_ids {
            return Err(invalid());
        }
        if self.max_open_transfers > cap.max_open_transfers {
            return Err(invalid());
        }
        if self.max_outputs > cap.max_outputs {
            return Err(invalid());
        }
        if self.max_allocations_total > cap.max_allocations_total {
            return Err(invalid());
        }
        if self.max_allocations_per_output > cap.max_allocations_per_output {
            return Err(invalid());
        }
        if self.max_panels_per_output > cap.max_panels_per_output {
            return Err(invalid());
        }
        if self.max_popouts_per_output > cap.max_popouts_per_output {
            return Err(invalid());
        }
        if self.max_candidate_surfaces > cap.max_candidate_surfaces {
            return Err(invalid());
        }
        if self.max_candidate_placements > cap.max_candidate_placements {
            return Err(invalid());
        }
        if self.max_candidate_targets > cap.max_candidate_targets {
            return Err(invalid());
        }
        if self.max_candidate_bytes > cap.max_candidate_bytes {
            return Err(invalid());
        }
        if self.max_pending_allocation_requests > cap.max_pending_allocation_requests {
            return Err(invalid());
        }
        if self.max_open_candidates_total > cap.max_open_candidates_total {
            return Err(invalid());
        }
        if self.max_open_candidates_per_output > cap.max_open_candidates_per_output {
            return Err(invalid());
        }
        if self.max_pending_candidates_total > cap.max_pending_candidates_total {
            return Err(invalid());
        }
        if self.max_pending_candidates_per_output > cap.max_pending_candidates_per_output {
            return Err(invalid());
        }
        if self.max_pending_actions > cap.max_pending_actions {
            return Err(invalid());
        }
        if self.max_frame_demands_per_output > cap.max_frame_demands_per_output {
            return Err(invalid());
        }
        if self.max_control_records > cap.max_control_records {
            return Err(invalid());
        }
        if self.reserved_control_queue_bytes > cap.reserved_control_queue_bytes {
            return Err(invalid());
        }
        if self.max_input_queue_bytes > cap.max_input_queue_bytes {
            return Err(invalid());
        }
        if self.max_output_queue_bytes > cap.max_output_queue_bytes {
            return Err(invalid());
        }
        if self.max_frames_per_service_tick > cap.max_frames_per_service_tick {
            return Err(invalid());
        }
        if self.max_panel_extent > cap.max_panel_extent {
            return Err(invalid());
        }
        if self.max_popout_extent_px > cap.max_popout_extent_px {
            return Err(invalid());
        }
        if self.max_reservation_extent > cap.max_reservation_extent {
            return Err(invalid());
        }
        if self.max_content_coverage_percent > cap.max_content_coverage_percent {
            return Err(invalid());
        }
        if self.max_margin_logical > cap.max_margin_logical {
            return Err(invalid());
        }
        if self.max_scale_numerator > cap.max_scale_numerator {
            return Err(invalid());
        }
        if self.max_scale_denominator > cap.max_scale_denominator {
            return Err(invalid());
        }
        if self.allocation_timeout_ms > cap.allocation_timeout_ms {
            return Err(invalid());
        }
        if self.transfer_timeout_ms > cap.transfer_timeout_ms {
            return Err(invalid());
        }
        if self.transfer_idle_timeout_ms > cap.transfer_idle_timeout_ms {
            return Err(invalid());
        }
        if self.candidate_timeout_ms > cap.candidate_timeout_ms {
            return Err(invalid());
        }
        if self.preparation_timeout_ms > cap.preparation_timeout_ms {
            return Err(invalid());
        }
        if self.presentation_timeout_ms > cap.presentation_timeout_ms {
            return Err(invalid());
        }
        if self.action_ack_timeout_ms > cap.action_ack_timeout_ms {
            return Err(invalid());
        }
        if self.permit_timeout_ms > cap.permit_timeout_ms {
            return Err(invalid());
        }
        if self.peer_write_timeout_ms > cap.peer_write_timeout_ms {
            return Err(invalid());
        }
        if self.max_candidate_rate_millihz > cap.max_candidate_rate_millihz {
            return Err(invalid());
        }
        if self.max_width_px == 0
            || self.max_height_px == 0
            || self.max_resource_bytes == 0
            || self.max_open_transfers == 0
            || self.max_live_resources == 0
            || self.max_resource_ids < self.max_live_resources
            || self.max_outputs == 0
            || self.max_allocations_per_output == 0
            || self.max_allocations_total < self.max_allocations_per_output
            || self.max_candidate_surfaces == 0
            || self.max_candidate_placements == 0
            || self.max_open_candidates_per_output == 0
            || self.max_open_candidates_total < self.max_open_candidates_per_output
            || self.max_pending_candidates_per_output == 0
            || self.max_pending_candidates_total < self.max_pending_candidates_per_output
            || self.max_scale_numerator == 0
            || self.max_scale_denominator == 0
            || self.max_candidate_rate_millihz == 0
            || self.max_frames_per_service_tick == 0
            || self.max_frame_demands_per_output != 1
            || self.max_control_records < 1
            || self.max_candidate_bytes < 40
            || self.max_input_queue_bytes < self.max_frame_payload + 24
            || self.max_output_queue_bytes < self.reserved_control_queue_bytes
            || self.reserved_control_queue_bytes < 1024
            || self.max_chunk_bytes < self.max_width_px * 4
            || self.max_chunk_bytes + 48 > self.max_frame_payload
            || self.max_staging_bytes < self.max_resource_bytes
            || self.max_resident_bytes < self.max_resource_bytes
            || self.max_retiring_bytes < self.max_resource_bytes
            || self.max_session_retiring_bytes
                < self.max_staging_bytes + self.max_resident_bytes + self.max_retiring_bytes
            || self.max_reservation_extent > self.max_panel_extent
        {
            return Err(invalid());
        }
        if [
            self.allocation_timeout_ms,
            self.transfer_timeout_ms,
            self.transfer_idle_timeout_ms,
            self.candidate_timeout_ms,
            self.preparation_timeout_ms,
            self.presentation_timeout_ms,
            self.action_ack_timeout_ms,
            self.permit_timeout_ms,
            self.peer_write_timeout_ms,
        ]
        .contains(&0)
            || self.transfer_idle_timeout_ms > self.transfer_timeout_ms
        {
            return Err(invalid());
        }
        Ok(())
    }
}

impl ContentLimits {
    pub fn prototype(grant: ContentGrant) -> Self {
        Self {
            grant,
            limits_generation: 1,
            max_resource_bytes: 4194304,
            max_staging_bytes: 8388608,
            max_resident_bytes: 16777216,
            max_retiring_bytes: 16777216,
            max_session_retiring_bytes: 67108864,
            pixel_format_mask: 1,
            effect_mask: 0,
            max_frame_payload: 65536,
            max_chunk_bytes: 65488,
            max_width_px: 8192,
            max_height_px: 4096,
            max_live_resources: 64,
            max_resource_ids: 4096,
            max_open_transfers: 4,
            max_outputs: 16,
            max_allocations_total: 16,
            max_allocations_per_output: 4,
            max_panels_per_output: 1,
            max_popouts_per_output: 3,
            max_candidate_surfaces: 8,
            max_candidate_placements: 32,
            max_candidate_targets: 64,
            max_candidate_bytes: 8192,
            max_pending_allocation_requests: 8,
            max_open_candidates_total: 2,
            max_open_candidates_per_output: 1,
            max_pending_candidates_total: 8,
            max_pending_candidates_per_output: 1,
            max_pending_actions: 16,
            max_frame_demands_per_output: 1,
            max_control_records: 64,
            reserved_control_queue_bytes: 65536,
            max_input_queue_bytes: 131072,
            max_output_queue_bytes: 262144,
            max_frames_per_service_tick: 16,
            max_panel_extent: 512,
            max_popout_extent_px: 1024,
            max_reservation_extent: 512,
            max_content_coverage_percent: 50,
            max_margin_logical: 512,
            max_scale_numerator: 32,
            max_scale_denominator: 4,
            allocation_timeout_ms: 1000,
            transfer_timeout_ms: 2000,
            transfer_idle_timeout_ms: 500,
            candidate_timeout_ms: 1000,
            preparation_timeout_ms: 1000,
            presentation_timeout_ms: 2000,
            action_ack_timeout_ms: 1000,
            permit_timeout_ms: 250,
            peer_write_timeout_ms: 2000,
            max_candidate_rate_millihz: 120000,
        }
    }
}
