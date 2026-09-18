#include "../sophia_shell_content_limits.h"
#include "fields.h"

int sophia_shell_content_refusal_decode(const struct sophia_shell_frame *f,
                                       struct sophia_shell_content_refusal *out)
{
    if (!f || !f->payload || !out) return SOPHIA_SHELL_ARGUMENT;
    if (f->kind != 160 || f->transaction || f->payload_bytes != 12 ||
        shell_get16(f->payload+2)) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_content_refusal v = {shell_get16(f->payload), shell_get64(f->payload+4)};
    if (v.reason < 1 || v.reason > 4 || !v.denied_capabilities) return SOPHIA_SHELL_INVALID;
    *out = v;
    return SOPHIA_SHELL_OK;
}

int sophia_shell_content_limits_decode(const struct sophia_shell_frame *f,
                                      struct sophia_shell_content_limits *out)
{
    if (!f || !f->payload || !out) return SOPHIA_SHELL_ARGUMENT;
    if (f->kind != 161 || f->transaction || f->payload_bytes != 264 ||
        shell_get32(f->payload+260)) return SOPHIA_SHELL_INVALID;
    const uint8_t *p = f->payload;
    struct sophia_shell_content_limits v = {0};
    v.grant = (struct sophia_shell_content_grant){shell_get64(p), shell_get64(p+8)};
#define U64(name, offset, cap) v.name = shell_get64(p+offset); if (v.name > cap) return SOPHIA_SHELL_INVALID
#define U32(name, offset, cap) v.name = shell_get32(p+offset); if (v.name > cap) return SOPHIA_SHELL_INVALID
    v.limits_generation = shell_get64(p+16);
    U64(max_resource_bytes, 24, 4194304u);
    U64(max_staging_bytes, 32, 8388608u);
    U64(max_resident_bytes, 40, 16777216u);
    U64(max_retiring_bytes, 48, 16777216u);
    U64(max_session_retiring_bytes, 56, 67108864u);
    U64(pixel_format_mask, 64, 1u);
    U64(effect_mask, 72, 0u);
    U32(max_frame_payload, 80, 65536u);
    U32(max_chunk_bytes, 84, 65488u);
    U32(max_width_px, 88, 8192u);
    U32(max_height_px, 92, 4096u);
    U32(max_live_resources, 96, 64u);
    U32(max_resource_ids, 100, 4096u);
    U32(max_open_transfers, 104, 4u);
    U32(max_outputs, 108, 16u);
    U32(max_allocations_total, 112, 16u);
    U32(max_allocations_per_output, 116, 4u);
    U32(max_panels_per_output, 120, 1u);
    U32(max_popouts_per_output, 124, 3u);
    U32(max_candidate_surfaces, 128, 8u);
    U32(max_candidate_placements, 132, 32u);
    U32(max_candidate_targets, 136, 64u);
    U32(max_candidate_bytes, 140, 8192u);
    U32(max_pending_allocation_requests, 144, 8u);
    U32(max_open_candidates_total, 148, 2u);
    U32(max_open_candidates_per_output, 152, 1u);
    U32(max_pending_candidates_total, 156, 8u);
    U32(max_pending_candidates_per_output, 160, 1u);
    U32(max_pending_actions, 164, 16u);
    U32(max_frame_demands_per_output, 168, 1u);
    U32(max_control_records, 172, 64u);
    U32(reserved_control_queue_bytes, 176, 65536u);
    U32(max_input_queue_bytes, 180, 131072u);
    U32(max_output_queue_bytes, 184, 262144u);
    U32(max_frames_per_service_tick, 188, 16u);
    U32(max_panel_extent, 192, 512u);
    U32(max_popout_extent_px, 196, 1024u);
    U32(max_reservation_extent, 200, 512u);
    U32(max_content_coverage_percent, 204, 50u);
    U32(max_margin_logical, 208, 512u);
    U32(max_scale_numerator, 212, 32u);
    U32(max_scale_denominator, 216, 4u);
    U32(allocation_timeout_ms, 220, 1000u);
    U32(transfer_timeout_ms, 224, 2000u);
    U32(transfer_idle_timeout_ms, 228, 500u);
    U32(candidate_timeout_ms, 232, 1000u);
    U32(preparation_timeout_ms, 236, 1000u);
    U32(presentation_timeout_ms, 240, 2000u);
    U32(action_ack_timeout_ms, 244, 1000u);
    U32(permit_timeout_ms, 248, 250u);
    U32(peer_write_timeout_ms, 252, 2000u);
    U32(max_candidate_rate_millihz, 256, 120000u);
#undef U64
#undef U32
    if (!v.grant.connection_epoch || !v.grant.content_grant_epoch || !v.limits_generation ||
        v.pixel_format_mask != 1 || v.effect_mask != 0 || !v.max_width_px || !v.max_height_px ||
        !v.max_resource_bytes || !v.max_open_transfers || !v.max_live_resources ||
        v.max_resource_ids < v.max_live_resources || !v.max_outputs ||
        !v.max_allocations_per_output || v.max_allocations_total < v.max_allocations_per_output ||
        !v.max_candidate_surfaces || !v.max_candidate_placements ||
        !v.max_open_candidates_per_output || v.max_open_candidates_total < v.max_open_candidates_per_output ||
        !v.max_pending_candidates_per_output || v.max_pending_candidates_total < v.max_pending_candidates_per_output ||
        !v.max_scale_numerator || !v.max_scale_denominator || !v.max_candidate_rate_millihz ||
        !v.max_frames_per_service_tick || v.max_frame_demands_per_output != 1 ||
        !v.max_control_records || v.max_candidate_bytes < 40 ||
        v.max_input_queue_bytes < v.max_frame_payload+24 ||
        v.max_output_queue_bytes < v.reserved_control_queue_bytes ||
        v.reserved_control_queue_bytes < 1024 || v.max_chunk_bytes < v.max_width_px*4 ||
        v.max_chunk_bytes+48 > v.max_frame_payload ||
        v.max_staging_bytes < v.max_resource_bytes || v.max_resident_bytes < v.max_resource_bytes ||
        v.max_retiring_bytes < v.max_resource_bytes ||
        v.max_session_retiring_bytes < v.max_staging_bytes+v.max_resident_bytes+v.max_retiring_bytes ||
        v.max_reservation_extent > v.max_panel_extent ||
        !v.allocation_timeout_ms || !v.transfer_timeout_ms || !v.transfer_idle_timeout_ms ||
        !v.candidate_timeout_ms || !v.preparation_timeout_ms || !v.presentation_timeout_ms ||
        !v.action_ack_timeout_ms || !v.permit_timeout_ms || !v.peer_write_timeout_ms ||
        v.transfer_idle_timeout_ms > v.transfer_timeout_ms) return SOPHIA_SHELL_INVALID;
    *out = v;
    return SOPHIA_SHELL_OK;
}
