#ifndef SOPHIA_SHELL_CONTENT_RESOURCE_H
#define SOPHIA_SHELL_CONTENT_RESOURCE_H
#include "sophia_shell_wire.h"
#include "sophia_shell_content_types.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_SHELL_RESOURCE_MAX_BYTES 4194304u
#define SOPHIA_SHELL_RESOURCE_MAX_CHUNK_BYTES 65488u

struct sophia_shell_resource_key {
    struct sophia_shell_content_grant grant;
    struct sophia_shell_content_id resource;
};
struct sophia_shell_resource_begin {
    struct sophia_shell_resource_key key;
    uint32_t width, height, scale_numerator, scale_denominator;
    uint16_t pixel_format;
    uint32_t chunk_count;
    uint64_t total_bytes;
};
struct sophia_shell_resource_chunk {
    struct sophia_shell_resource_key key;
    uint32_t ordinal;
    uint64_t offset;
    const uint8_t *bytes;
    size_t byte_count;
};
struct sophia_shell_resource_end {
    struct sophia_shell_resource_key key;
    uint64_t total_bytes;
    uint32_t chunk_count;
};
struct sophia_shell_resource_reply {
    uint16_t kind;
    uint64_t transaction;
    struct sophia_shell_resource_key key;
    uint16_t status, reason; /* status zero for Released */
    uint32_t next_ordinal; /* zero for Released */
    uint64_t admitted_bytes; /* zero for Released */
};
/* Wire-shape checks match the protocol codec's prototype ceilings. Negotiated
 * limits, upload ordering/credit, ownership and transaction matching are the
 * caller's separate obligations. Decoding Released alone does not release an
 * arbitrary current resource: match the exact outstanding generation first.
 * No allocation, I/O, pixel mutation, retention or implicit resource reuse. */
int sophia_shell_resource_reply_decode(const struct sophia_shell_frame *frame,
                                      struct sophia_shell_resource_reply *out);
/* Encode complete frames. On refusal dst and frame_bytes stay unchanged.
 * All inputs, including chunk bytes, must be disjoint from dst/frame_bytes.
 * Begin uses canonical whole-row chunks under the wire prototype (65488 bytes);
 * clients must additionally check compatibility with their negotiated limits. */
int sophia_shell_resource_begin_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_resource_begin *value, size_t *frame_bytes);
int sophia_shell_resource_chunk_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_resource_chunk *value, size_t *frame_bytes);
int sophia_shell_resource_end_encode(uint8_t *dst, size_t capacity, uint64_t transaction,
    const struct sophia_shell_resource_end *value, size_t *frame_bytes);
/* kind must be Cancel=169 or Retire=170. Neither grants local reuse; retain the
 * outstanding exact owner until its corresponding terminal lifecycle outcome. */
int sophia_shell_resource_control_encode(uint8_t *dst, size_t capacity, uint16_t kind,
    uint64_t transaction, const struct sophia_shell_resource_key *value, size_t *frame_bytes);
#ifdef __cplusplus
}
#endif
#endif
