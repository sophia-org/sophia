#ifndef SOPHIA_SHELL_CONTENT_CONTROL_H
#define SOPHIA_SHELL_CONTENT_CONTROL_H
#include "sophia_shell_content_feedback.h"
#ifdef __cplusplus
extern "C" {
#endif
struct sophia_shell_candidate_end {
    struct sophia_shell_content_grant grant;
    uint64_t generation;
    uint32_t surface_count, placement_count, target_count;
};
struct sophia_shell_frame_demand {
    struct sophia_shell_content_grant grant;
    struct sophia_shell_content_id output, allocation;
    uint64_t demand_id;
    uint16_t reason;
};
struct sophia_shell_demand_cancel {
    struct sophia_shell_content_grant grant;
    struct sophia_shell_content_id output;
    uint64_t demand_id, permit_id;
};
struct sophia_shell_content_action_ack {
    struct sophia_shell_content_grant grant;
    struct sophia_shell_content_action_identity identity;
    uint16_t disposition;
};
/* Complete frame encoders for common kinds 174/176/178/180. No allocation or
 * lifecycle transition. dst/frame_bytes unchanged on refusal; inputs must not
 * overlap either output. ACK encoding does not establish enqueue or receipt;
 * callers must never ACK a cancellation or activate merely Prepared content. */
int sophia_shell_candidate_end_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_candidate_end *value, size_t *frame_bytes);
int sophia_shell_frame_demand_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_frame_demand *value, size_t *frame_bytes);
int sophia_shell_demand_cancel_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_demand_cancel *value, size_t *frame_bytes);
int sophia_shell_content_action_ack_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_content_action_ack *value, size_t *frame_bytes);
#ifdef __cplusplus
}
#endif
#endif
