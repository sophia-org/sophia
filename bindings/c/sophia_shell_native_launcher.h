#ifndef SOPHIA_SHELL_NATIVE_LAUNCHER_H
#define SOPHIA_SHELL_NATIVE_LAUNCHER_H
#include "sophia_shell_wire.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Structural validation of revision-7 kinds 187..197, including bounded UTF-8
 * and rows. No allocation or I/O; frame/payload are unchanged and remain borrowed.
 * This does NOT authorize a lease, catalog, candidate, event or launch. */
int sophia_shell_native_launcher_validate(const struct sophia_shell_frame *frame);

struct sophia_shell_native_grant {
    uint64_t connection_epoch, content_grant_epoch;
};
struct sophia_shell_native_id { uint64_t id, generation; };
struct sophia_shell_native_opening {
    struct sophia_shell_native_grant grant;
    uint64_t opening;
    struct sophia_shell_native_id output;
    uint64_t catalog_generation, state_revision;
};
struct sophia_shell_native_binding {
    struct sophia_shell_native_grant grant;
    uint64_t opening;
    struct sophia_shell_native_id output, allocation;
    uint64_t catalog_generation, candidate_generation, presentation_epoch;
    uint64_t interaction_generation, state_revision, focus_lease;
};
struct sophia_shell_native_event {
    struct sophia_shell_native_binding binding;
    uint64_t event_id, state_revision;
};
struct sophia_shell_native_input {
    struct sophia_shell_native_event event;
    uint64_t issued_mono_usec;
    uint16_t kind, text_bytes;
    /* Borrowed, not NUL terminated; invalid after the source frame is consumed. */
    const uint8_t *text;
};
struct sophia_shell_native_activation {
    struct sophia_shell_native_event event;
    uint16_t cause, slot;
};
struct sophia_shell_native_outcome {
    struct sophia_shell_native_activation activation;
    uint16_t status, reason;
};
struct sophia_shell_native_revoked {
    struct sophia_shell_native_binding binding;
    uint16_t reason;
};
struct sophia_shell_native_closed {
    struct sophia_shell_native_grant grant;
    uint64_t opening;
    uint16_t reason;
};
/* Inbound native records only. Content/resource/catalog messages use their own
 * codecs. Decoding supplies data, never current authority or input promotion. */
struct sophia_shell_native_message {
    uint16_t kind;
    uint64_t transaction;
    union {
        struct sophia_shell_native_opening opening;
        struct sophia_shell_native_binding focus;
        struct sophia_shell_native_revoked revoked;
        struct sophia_shell_native_input input;
        struct sophia_shell_native_outcome outcome;
        struct sophia_shell_native_closed closed;
    } value;
};
int sophia_shell_native_launcher_decode(const struct sophia_shell_frame *frame,
                                       struct sophia_shell_native_message *out);

struct sophia_shell_native_allocation {
    struct sophia_shell_native_grant grant;
    uint64_t opening;
    struct sophia_shell_native_id output;
    uint64_t request_id;
    struct sophia_shell_native_id prior;
    uint16_t operation, edge;
    uint32_t desired_width, desired_height;
    int16_t margins[4]; /* top, right, bottom, left */
};
struct sophia_shell_native_ack {
    struct sophia_shell_native_event event;
    uint16_t disposition;
};
/* Encode complete frames with the same structural checks as the Rust codec.
 * No allocation/I/O/authorization. On failure dst and frame_bytes are unchanged.
 * Arguments must not overlap dst. Queue/retain the result before consuming an
 * input obligation; encoding alone does not mean enqueue or peer receipt. */
int sophia_shell_native_allocation_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_allocation *value,
    size_t *frame_bytes);
int sophia_shell_native_ack_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_ack *value,
    size_t *frame_bytes);
int sophia_shell_native_activation_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_activation *value,
    size_t *frame_bytes);

#define SOPHIA_SHELL_NATIVE_MAX_ROWS 32u
struct sophia_shell_native_candidate {
    struct sophia_shell_native_grant grant;
    uint64_t candidate_generation;
    struct sophia_shell_native_id output;
    uint64_t facts_generation, pacing_permit, interaction_generation;
    uint32_t placement_count;
    uint64_t opening, catalog_generation, state_revision;
    uint16_t selected, row_count;
    uint16_t rows[SOPHIA_SHELL_NATIVE_MAX_ROWS];
};
struct sophia_shell_native_surface {
    struct sophia_shell_native_id allocation;
    uint64_t scale_generation;
    uint16_t edge;
    int16_t margins[4];
};
struct sophia_shell_native_placement {
    struct sophia_shell_native_id resource;
    int32_t x, y;
};
struct sophia_shell_native_target {
    uint64_t id, generation;
    uint16_t slot;
    int32_t x, y;
    uint32_t width, height;
};
/* Native role has exactly one parentless transient surface, no reservation.
 * A chunk can omit its surface and carry subsequent placements/targets. Counts
 * bound access to the fixed arrays. Chunk/candidate assembly and content credit
 * remain the lifecycle owner's responsibility, not the codec's. */
struct sophia_shell_native_chunk {
    struct sophia_shell_native_grant grant;
    uint64_t candidate_generation;
    uint32_t ordinal, surface_count, placement_count, target_count;
    struct sophia_shell_native_surface surface;
    struct sophia_shell_native_placement placements[SOPHIA_SHELL_NATIVE_MAX_ROWS];
    struct sophia_shell_native_target targets[SOPHIA_SHELL_NATIVE_MAX_ROWS];
};
int sophia_shell_native_candidate_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_candidate *value,
    size_t *frame_bytes);
int sophia_shell_native_chunk_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_chunk *value,
    size_t *frame_bytes);
#ifdef __cplusplus
}
#endif
#endif
