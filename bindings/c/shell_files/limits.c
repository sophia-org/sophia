/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_limits(const struct sophia_sf_limits *v)
{
    if (!v->grant_connection_epoch || !v->grant_content_epoch || !v->limits_generation ||
        !v->max_resource_bytes || v->max_resource_bytes > 4194304 ||
        v->max_staging_bytes > 8388608 || v->max_resident_bytes > 16777216 ||
        v->max_retiring_bytes > 16777216 || v->max_session_retiring_bytes > 67108864 ||
        v->pixel_format_mask != 1 || v->effect_mask != 0 || v->max_frame_payload > 65536 ||
        v->max_chunk_bytes > 65488 || !v->max_width_px || v->max_width_px > 8192 ||
        !v->max_height_px || v->max_height_px > 4096 || !v->max_live_resources ||
        v->max_live_resources > 64 || v->max_resource_ids > 4096 || !v->max_open_transfers ||
        v->max_open_transfers > 4 || !v->max_outputs || v->max_outputs > 16 ||
        v->max_allocations_total > 16 || !v->max_allocations_per_output ||
        v->max_allocations_per_output > 4 || v->max_panels_per_output > 1 ||
        v->max_popouts_per_output > 3 || !v->max_candidate_surfaces ||
        v->max_candidate_surfaces > 8 || !v->max_candidate_placements ||
        v->max_candidate_placements > 32 || v->max_candidate_targets > 64 ||
        v->max_candidate_bytes < 40 || v->max_candidate_bytes > 8192 ||
        v->max_pending_allocation_requests > 8 || v->max_open_candidates_total > 2 ||
        !v->max_open_candidates_per_output || v->max_open_candidates_per_output > 1 ||
        v->max_pending_candidates_total > 8 || !v->max_pending_candidates_per_output ||
        v->max_pending_candidates_per_output > 1 || v->max_pending_actions > 16 ||
        v->max_frame_demands_per_output != 1 || v->max_control_records < 1 ||
        v->max_control_records > 64 || v->reserved_control_queue_bytes < 1024 ||
        v->reserved_control_queue_bytes > 65536 || v->max_input_queue_bytes > 131072 ||
        v->max_output_queue_bytes > 262144 || !v->max_frames_per_service_tick ||
        v->max_frames_per_service_tick > 16 || v->max_panel_extent > 512 ||
        v->max_popout_extent_px > 1024 || v->max_reservation_extent > 512 ||
        v->max_content_coverage_percent > 50 || v->max_margin_logical > 512 ||
        !v->max_scale_numerator || v->max_scale_numerator > 32 || !v->max_scale_denominator ||
        v->max_scale_denominator > 4 || !v->allocation_timeout_ms ||
        v->allocation_timeout_ms > 1000 || !v->transfer_timeout_ms ||
        v->transfer_timeout_ms > 2000 || !v->transfer_idle_timeout_ms ||
        v->transfer_idle_timeout_ms > 500 || !v->candidate_timeout_ms ||
        v->candidate_timeout_ms > 1000 || !v->preparation_timeout_ms ||
        v->preparation_timeout_ms > 1000 || !v->presentation_timeout_ms ||
        v->presentation_timeout_ms > 2000 || !v->action_ack_timeout_ms ||
        v->action_ack_timeout_ms > 1000 || !v->permit_timeout_ms || v->permit_timeout_ms > 250 ||
        !v->peer_write_timeout_ms || v->peer_write_timeout_ms > 2000 ||
        !v->max_candidate_rate_millihz || v->max_candidate_rate_millihz > 120000)
        return -1;
    return 0;
}
void sf_put_limits(uint8_t *b, const struct sophia_sf_limits *v)
{
    memset(b, 0, 264);
    sf_put(b + 0, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 8, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 16, (uint64_t)v->limits_generation, 8);
    sf_put(b + 24, (uint64_t)v->max_resource_bytes, 8);
    sf_put(b + 32, (uint64_t)v->max_staging_bytes, 8);
    sf_put(b + 40, (uint64_t)v->max_resident_bytes, 8);
    sf_put(b + 48, (uint64_t)v->max_retiring_bytes, 8);
    sf_put(b + 56, (uint64_t)v->max_session_retiring_bytes, 8);
    sf_put(b + 64, (uint64_t)v->pixel_format_mask, 8);
    sf_put(b + 72, (uint64_t)v->effect_mask, 8);
    sf_put(b + 80, (uint64_t)v->max_frame_payload, 4);
    sf_put(b + 84, (uint64_t)v->max_chunk_bytes, 4);
    sf_put(b + 88, (uint64_t)v->max_width_px, 4);
    sf_put(b + 92, (uint64_t)v->max_height_px, 4);
    sf_put(b + 96, (uint64_t)v->max_live_resources, 4);
    sf_put(b + 100, (uint64_t)v->max_resource_ids, 4);
    sf_put(b + 104, (uint64_t)v->max_open_transfers, 4);
    sf_put(b + 108, (uint64_t)v->max_outputs, 4);
    sf_put(b + 112, (uint64_t)v->max_allocations_total, 4);
    sf_put(b + 116, (uint64_t)v->max_allocations_per_output, 4);
    sf_put(b + 120, (uint64_t)v->max_panels_per_output, 4);
    sf_put(b + 124, (uint64_t)v->max_popouts_per_output, 4);
    sf_put(b + 128, (uint64_t)v->max_candidate_surfaces, 4);
    sf_put(b + 132, (uint64_t)v->max_candidate_placements, 4);
    sf_put(b + 136, (uint64_t)v->max_candidate_targets, 4);
    sf_put(b + 140, (uint64_t)v->max_candidate_bytes, 4);
    sf_put(b + 144, (uint64_t)v->max_pending_allocation_requests, 4);
    sf_put(b + 148, (uint64_t)v->max_open_candidates_total, 4);
    sf_put(b + 152, (uint64_t)v->max_open_candidates_per_output, 4);
    sf_put(b + 156, (uint64_t)v->max_pending_candidates_total, 4);
    sf_put(b + 160, (uint64_t)v->max_pending_candidates_per_output, 4);
    sf_put(b + 164, (uint64_t)v->max_pending_actions, 4);
    sf_put(b + 168, (uint64_t)v->max_frame_demands_per_output, 4);
    sf_put(b + 172, (uint64_t)v->max_control_records, 4);
    sf_put(b + 176, (uint64_t)v->reserved_control_queue_bytes, 4);
    sf_put(b + 180, (uint64_t)v->max_input_queue_bytes, 4);
    sf_put(b + 184, (uint64_t)v->max_output_queue_bytes, 4);
    sf_put(b + 188, (uint64_t)v->max_frames_per_service_tick, 4);
    sf_put(b + 192, (uint64_t)v->max_panel_extent, 4);
    sf_put(b + 196, (uint64_t)v->max_popout_extent_px, 4);
    sf_put(b + 200, (uint64_t)v->max_reservation_extent, 4);
    sf_put(b + 204, (uint64_t)v->max_content_coverage_percent, 4);
    sf_put(b + 208, (uint64_t)v->max_margin_logical, 4);
    sf_put(b + 212, (uint64_t)v->max_scale_numerator, 4);
    sf_put(b + 216, (uint64_t)v->max_scale_denominator, 4);
    sf_put(b + 220, (uint64_t)v->allocation_timeout_ms, 4);
    sf_put(b + 224, (uint64_t)v->transfer_timeout_ms, 4);
    sf_put(b + 228, (uint64_t)v->transfer_idle_timeout_ms, 4);
    sf_put(b + 232, (uint64_t)v->candidate_timeout_ms, 4);
    sf_put(b + 236, (uint64_t)v->preparation_timeout_ms, 4);
    sf_put(b + 240, (uint64_t)v->presentation_timeout_ms, 4);
    sf_put(b + 244, (uint64_t)v->action_ack_timeout_ms, 4);
    sf_put(b + 248, (uint64_t)v->permit_timeout_ms, 4);
    sf_put(b + 252, (uint64_t)v->peer_write_timeout_ms, 4);
    sf_put(b + 256, (uint64_t)v->max_candidate_rate_millihz, 4);
}
int sf_take_limits(const uint8_t *b, struct sophia_sf_limits *v)
{
    v->grant_connection_epoch = (uint64_t)sf_get(b + 0, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 8, 8);
    v->limits_generation = (uint64_t)sf_get(b + 16, 8);
    v->max_resource_bytes = (uint64_t)sf_get(b + 24, 8);
    v->max_staging_bytes = (uint64_t)sf_get(b + 32, 8);
    v->max_resident_bytes = (uint64_t)sf_get(b + 40, 8);
    v->max_retiring_bytes = (uint64_t)sf_get(b + 48, 8);
    v->max_session_retiring_bytes = (uint64_t)sf_get(b + 56, 8);
    v->pixel_format_mask = (uint64_t)sf_get(b + 64, 8);
    v->effect_mask = (uint64_t)sf_get(b + 72, 8);
    v->max_frame_payload = (uint32_t)sf_get(b + 80, 4);
    v->max_chunk_bytes = (uint32_t)sf_get(b + 84, 4);
    v->max_width_px = (uint32_t)sf_get(b + 88, 4);
    v->max_height_px = (uint32_t)sf_get(b + 92, 4);
    v->max_live_resources = (uint32_t)sf_get(b + 96, 4);
    v->max_resource_ids = (uint32_t)sf_get(b + 100, 4);
    v->max_open_transfers = (uint32_t)sf_get(b + 104, 4);
    v->max_outputs = (uint32_t)sf_get(b + 108, 4);
    v->max_allocations_total = (uint32_t)sf_get(b + 112, 4);
    v->max_allocations_per_output = (uint32_t)sf_get(b + 116, 4);
    v->max_panels_per_output = (uint32_t)sf_get(b + 120, 4);
    v->max_popouts_per_output = (uint32_t)sf_get(b + 124, 4);
    v->max_candidate_surfaces = (uint32_t)sf_get(b + 128, 4);
    v->max_candidate_placements = (uint32_t)sf_get(b + 132, 4);
    v->max_candidate_targets = (uint32_t)sf_get(b + 136, 4);
    v->max_candidate_bytes = (uint32_t)sf_get(b + 140, 4);
    v->max_pending_allocation_requests = (uint32_t)sf_get(b + 144, 4);
    v->max_open_candidates_total = (uint32_t)sf_get(b + 148, 4);
    v->max_open_candidates_per_output = (uint32_t)sf_get(b + 152, 4);
    v->max_pending_candidates_total = (uint32_t)sf_get(b + 156, 4);
    v->max_pending_candidates_per_output = (uint32_t)sf_get(b + 160, 4);
    v->max_pending_actions = (uint32_t)sf_get(b + 164, 4);
    v->max_frame_demands_per_output = (uint32_t)sf_get(b + 168, 4);
    v->max_control_records = (uint32_t)sf_get(b + 172, 4);
    v->reserved_control_queue_bytes = (uint32_t)sf_get(b + 176, 4);
    v->max_input_queue_bytes = (uint32_t)sf_get(b + 180, 4);
    v->max_output_queue_bytes = (uint32_t)sf_get(b + 184, 4);
    v->max_frames_per_service_tick = (uint32_t)sf_get(b + 188, 4);
    v->max_panel_extent = (uint32_t)sf_get(b + 192, 4);
    v->max_popout_extent_px = (uint32_t)sf_get(b + 196, 4);
    v->max_reservation_extent = (uint32_t)sf_get(b + 200, 4);
    v->max_content_coverage_percent = (uint32_t)sf_get(b + 204, 4);
    v->max_margin_logical = (uint32_t)sf_get(b + 208, 4);
    v->max_scale_numerator = (uint32_t)sf_get(b + 212, 4);
    v->max_scale_denominator = (uint32_t)sf_get(b + 216, 4);
    v->allocation_timeout_ms = (uint32_t)sf_get(b + 220, 4);
    v->transfer_timeout_ms = (uint32_t)sf_get(b + 224, 4);
    v->transfer_idle_timeout_ms = (uint32_t)sf_get(b + 228, 4);
    v->candidate_timeout_ms = (uint32_t)sf_get(b + 232, 4);
    v->preparation_timeout_ms = (uint32_t)sf_get(b + 236, 4);
    v->presentation_timeout_ms = (uint32_t)sf_get(b + 240, 4);
    v->action_ack_timeout_ms = (uint32_t)sf_get(b + 244, 4);
    v->permit_timeout_ms = (uint32_t)sf_get(b + 248, 4);
    v->peer_write_timeout_ms = (uint32_t)sf_get(b + 252, 4);
    v->max_candidate_rate_millihz = (uint32_t)sf_get(b + 256, 4);
    if (!sf_zero(b + 260, 4))
        return -1;
    return sf_check_limits(v);
}
