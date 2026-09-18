#ifndef SOPHIA_SHELL_CONTENT_FEEDBACK_H
#define SOPHIA_SHELL_CONTENT_FEEDBACK_H
#include "sophia_shell_wire.h"
#include "sophia_shell_content_types.h"
#ifdef __cplusplus
extern "C" {
#endif
struct sophia_shell_content_rect { int32_t x, y; uint32_t width, height; };
struct sophia_shell_output_fact {
    struct sophia_shell_content_id output;
    uint32_t local_width, local_height, scale_numerator, scale_denominator;
    uint64_t scale_generation;
};
struct sophia_shell_output_facts {
    uint64_t generation;
    uint32_t count;
    struct sophia_shell_output_fact outputs[16];
};
struct sophia_shell_allocation_result {
    uint64_t request_id;
    uint16_t status, reason;
    struct sophia_shell_content_id output, allocation, parent;
    uint64_t scale_generation;
    struct sophia_shell_content_rect logical, pixel;
    uint32_t scale_numerator, scale_denominator, allowed_reservation_extent;
    int16_t margins[4];
    struct sophia_shell_content_rect acknowledged_anchor;
};
struct sophia_shell_candidate_outcome {
    uint64_t generation;
    struct sophia_shell_content_id output;
    uint16_t kind, reason;
    uint64_t presentation_epoch, work_area_generation, wm_commit_generation;
};
struct sophia_shell_frame_permit {
    struct sophia_shell_content_id output;
    uint64_t demand_id, permit_id;
    uint16_t state, reason;
    uint32_t ttl_ms, max_candidate_bytes;
};
/* Exact identity carried by both Action and ActionAck. Does not assert currency. */
struct sophia_shell_content_action_identity {
    struct sophia_shell_content_id output;
    uint64_t candidate_generation, presentation_epoch, interaction_generation;
    struct sophia_shell_content_id allocation;
    uint64_t target_id, target_generation, action_id, event_id;
};
struct sophia_shell_content_action {
    struct sophia_shell_content_action_identity identity;
    uint16_t kind, reason;
};
struct sophia_shell_content_feedback {
    uint16_t kind;
    uint64_t transaction;
    struct sophia_shell_content_grant grant;
    union {
        struct sophia_shell_output_facts facts;
        struct sophia_shell_allocation_result allocation;
        struct sophia_shell_candidate_outcome candidate;
        struct sophia_shell_frame_permit permit;
        struct sophia_shell_content_action action;
    } value;
};
/* Decode only kinds 162/164/175/177/179. No allocations or borrowed payloads.
 * On failure out is untouched. Geometry/status follow the wire contract; caller
 * must validate current grant, request, output and exact presented identity.
 * Prepared is not Presented. Cancellation is not an ACK obligation. */
int sophia_shell_content_feedback_decode(const struct sophia_shell_frame *frame,
                                        struct sophia_shell_content_feedback *out);
#ifdef __cplusplus
}
#endif
#endif
