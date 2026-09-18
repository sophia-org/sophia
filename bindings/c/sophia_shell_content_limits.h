#ifndef SOPHIA_SHELL_CONTENT_LIMITS_H
#define SOPHIA_SHELL_CONTENT_LIMITS_H
#include "sophia_shell_wire.h"
#include "sophia_shell_content_types.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Immutable negotiated bounds, not permission or a resource reservation. */
struct sophia_shell_content_limits {
    struct sophia_shell_content_grant grant;
    uint64_t limits_generation;
    uint64_t max_resource_bytes;
    uint64_t max_staging_bytes;
    uint64_t max_resident_bytes;
    uint64_t max_retiring_bytes;
    uint64_t max_session_retiring_bytes;
    uint64_t pixel_format_mask;
    uint64_t effect_mask;
    uint32_t max_frame_payload;
    uint32_t max_chunk_bytes;
    uint32_t max_width_px;
    uint32_t max_height_px;
    uint32_t max_live_resources;
    uint32_t max_resource_ids;
    uint32_t max_open_transfers;
    uint32_t max_outputs;
    uint32_t max_allocations_total;
    uint32_t max_allocations_per_output;
    uint32_t max_panels_per_output;
    uint32_t max_popouts_per_output;
    uint32_t max_candidate_surfaces;
    uint32_t max_candidate_placements;
    uint32_t max_candidate_targets;
    uint32_t max_candidate_bytes;
    uint32_t max_pending_allocation_requests;
    uint32_t max_open_candidates_total;
    uint32_t max_open_candidates_per_output;
    uint32_t max_pending_candidates_total;
    uint32_t max_pending_candidates_per_output;
    uint32_t max_pending_actions;
    uint32_t max_frame_demands_per_output;
    uint32_t max_control_records;
    uint32_t reserved_control_queue_bytes;
    uint32_t max_input_queue_bytes;
    uint32_t max_output_queue_bytes;
    uint32_t max_frames_per_service_tick;
    uint32_t max_panel_extent;
    uint32_t max_popout_extent_px;
    uint32_t max_reservation_extent;
    uint32_t max_content_coverage_percent;
    uint32_t max_margin_logical;
    uint32_t max_scale_numerator;
    uint32_t max_scale_denominator;
    uint32_t allocation_timeout_ms;
    uint32_t transfer_timeout_ms;
    uint32_t transfer_idle_timeout_ms;
    uint32_t candidate_timeout_ms;
    uint32_t preparation_timeout_ms;
    uint32_t presentation_timeout_ms;
    uint32_t action_ack_timeout_ms;
    uint32_t permit_timeout_ms;
    uint32_t peer_write_timeout_ms;
    uint32_t max_candidate_rate_millihz;
};
struct sophia_shell_content_refusal { uint16_t reason; uint64_t denied_capabilities; };
/* Only the exact respective server record is accepted. Full shape, prototype
 * caps and cross-field coherence are checked before assigning out. No memory
 * allocation or connection/lifecycle transition; caller must match the welcome
 * connection epoch, supported capabilities and intended role separately. */
int sophia_shell_content_limits_decode(const struct sophia_shell_frame *frame,
                                      struct sophia_shell_content_limits *out);
int sophia_shell_content_refusal_decode(const struct sophia_shell_frame *frame,
                                       struct sophia_shell_content_refusal *out);
#ifdef __cplusplus
}
#endif
#endif
