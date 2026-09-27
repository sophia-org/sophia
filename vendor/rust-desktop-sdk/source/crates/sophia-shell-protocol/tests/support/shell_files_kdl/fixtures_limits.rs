//! The `Limits` fixture: one distinctly-valued, cap-respecting granted
//! profile, and the field-name-to-value table `checks::checked` compares the
//! real encoded bytes against. `ContentLimits` is 56 fields, one owner (the
//! granted profile), which is why it gets its own file rather than sitting
//! alongside the flat records in [`super::fixtures`].

use super::{fmap, grant};
use sophia_shell_protocol::*;
use std::collections::BTreeMap;

pub fn limits() -> (ContentLimits, BTreeMap<&'static str, i128>) {
    let grant_connection_epoch = 111u64;
    let grant_content_epoch = 222u64;
    let limits_generation = 333u64;
    let max_resource_bytes = 1_048_576u64;
    let max_staging_bytes = 2_097_152u64;
    let max_resident_bytes = 3_145_728u64;
    let max_retiring_bytes = 4_193_280u64;
    let max_session_retiring_bytes = 9_437_160u64;
    let pixel_format_mask = 1u64;
    let effect_mask = 0u64;
    let max_frame_payload = 40_000u32;
    let max_chunk_bytes = 4_000u32;
    let max_width_px = 100u32;
    let max_height_px = 50u32;
    let max_live_resources = 10u32;
    let max_resource_ids = 20u32;
    let max_open_transfers = 2u32;
    let max_outputs = 5u32;
    let max_allocations_total = 9u32;
    let max_allocations_per_output = 3u32;
    let max_panels_per_output = 1u32;
    let max_popouts_per_output = 2u32;
    let max_candidate_surfaces = 4u32;
    let max_candidate_placements = 11u32;
    let max_candidate_targets = 33u32;
    let max_candidate_bytes = 2_048u32;
    let max_pending_allocation_requests = 6u32;
    let max_open_candidates_total = 2u32;
    let max_open_candidates_per_output = 1u32;
    let max_pending_candidates_total = 7u32;
    let max_pending_candidates_per_output = 1u32;
    let max_pending_actions = 12u32;
    let max_frame_demands_per_output = 1u32;
    let max_control_records = 44u32;
    let reserved_control_queue_bytes = 50_000u32;
    let max_input_queue_bytes = 60_000u32;
    let max_output_queue_bytes = 100_000u32;
    let max_frames_per_service_tick = 8u32;
    let max_panel_extent = 400u32;
    let max_popout_extent_px = 900u32;
    let max_reservation_extent = 300u32;
    let max_content_coverage_percent = 42u32;
    let max_margin_logical = 256u32;
    let max_scale_numerator = 24u32;
    let max_scale_denominator = 3u32;
    let allocation_timeout_ms = 900u32;
    let transfer_timeout_ms = 1_800u32;
    let transfer_idle_timeout_ms = 300u32;
    let candidate_timeout_ms = 700u32;
    let preparation_timeout_ms = 600u32;
    let presentation_timeout_ms = 1_500u32;
    let action_ack_timeout_ms = 800u32;
    let permit_timeout_ms = 200u32;
    let peer_write_timeout_ms = 1_900u32;
    let max_candidate_rate_millihz = 90_000u32;

    let limits = ContentLimits {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        limits_generation,
        max_resource_bytes,
        max_staging_bytes,
        max_resident_bytes,
        max_retiring_bytes,
        max_session_retiring_bytes,
        pixel_format_mask,
        effect_mask,
        max_frame_payload,
        max_chunk_bytes,
        max_width_px,
        max_height_px,
        max_live_resources,
        max_resource_ids,
        max_open_transfers,
        max_outputs,
        max_allocations_total,
        max_allocations_per_output,
        max_panels_per_output,
        max_popouts_per_output,
        max_candidate_surfaces,
        max_candidate_placements,
        max_candidate_targets,
        max_candidate_bytes,
        max_pending_allocation_requests,
        max_open_candidates_total,
        max_open_candidates_per_output,
        max_pending_candidates_total,
        max_pending_candidates_per_output,
        max_pending_actions,
        max_frame_demands_per_output,
        max_control_records,
        reserved_control_queue_bytes,
        max_input_queue_bytes,
        max_output_queue_bytes,
        max_frames_per_service_tick,
        max_panel_extent,
        max_popout_extent_px,
        max_reservation_extent,
        max_content_coverage_percent,
        max_margin_logical,
        max_scale_numerator,
        max_scale_denominator,
        allocation_timeout_ms,
        transfer_timeout_ms,
        transfer_idle_timeout_ms,
        candidate_timeout_ms,
        preparation_timeout_ms,
        presentation_timeout_ms,
        action_ack_timeout_ms,
        permit_timeout_ms,
        peer_write_timeout_ms,
        max_candidate_rate_millihz,
    };
    let expected = fmap!(
        grant_connection_epoch,
        grant_content_epoch,
        limits_generation,
        max_resource_bytes,
        max_staging_bytes,
        max_resident_bytes,
        max_retiring_bytes,
        max_session_retiring_bytes,
        pixel_format_mask,
        effect_mask,
        max_frame_payload,
        max_chunk_bytes,
        max_width_px,
        max_height_px,
        max_live_resources,
        max_resource_ids,
        max_open_transfers,
        max_outputs,
        max_allocations_total,
        max_allocations_per_output,
        max_panels_per_output,
        max_popouts_per_output,
        max_candidate_surfaces,
        max_candidate_placements,
        max_candidate_targets,
        max_candidate_bytes,
        max_pending_allocation_requests,
        max_open_candidates_total,
        max_open_candidates_per_output,
        max_pending_candidates_total,
        max_pending_candidates_per_output,
        max_pending_actions,
        max_frame_demands_per_output,
        max_control_records,
        reserved_control_queue_bytes,
        max_input_queue_bytes,
        max_output_queue_bytes,
        max_frames_per_service_tick,
        max_panel_extent,
        max_popout_extent_px,
        max_reservation_extent,
        max_content_coverage_percent,
        max_margin_logical,
        max_scale_numerator,
        max_scale_denominator,
        allocation_timeout_ms,
        transfer_timeout_ms,
        transfer_idle_timeout_ms,
        candidate_timeout_ms,
        preparation_timeout_ms,
        presentation_timeout_ms,
        action_ack_timeout_ms,
        permit_timeout_ms,
        peer_write_timeout_ms,
        max_candidate_rate_millihz
    );
    (limits, expected)
}
