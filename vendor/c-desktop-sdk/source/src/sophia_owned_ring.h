#ifndef SOPHIA_OWNED_RING_H
#define SOPHIA_OWNED_RING_H
/* Shell r10 owned-ring records, pinned to spec/sophia-owned-ring-v1.kdl and
 * spec/sophia-owned-ring.md. Passive codecs and geometry only: decoding
 * proves shape, never admission, custody or authority. Offer capacity is an
 * admission check against the registered ring, made separately by callers.
 * Lock never uses these records. */
#include "sophia_image_channel.h"
#include <stddef.h>
#include <stdint.h>

#define SOPHIA_OR_REGISTER_BYTES 72u
#define SOPHIA_OR_OFFER_BYTES 152u
#define SOPHIA_OR_SOURCE_BYTES 72u
#define SOPHIA_OR_EVENT_BYTES 120u
#define SOPHIA_OR_SLOT_HEADER_BYTES 4096u
#define SOPHIA_OR_MAX_SLOTS 4u
#define SOPHIA_OR_MAX_CAPACITY (4u << 20)
#define SOPHIA_OR_CONTROL_FILE "owned-ring-v1"

enum sophia_or_event_kind {
    SOPHIA_OR_RING_ADMITTED = 1,
    SOPHIA_OR_RING_READY,
    SOPHIA_OR_RING_REFUSED,
    SOPHIA_OR_SLOT_RELEASED
};

/* Words 1..5 of every record; all nonzero. */
struct sophia_or_identity {
    uint64_t connection_epoch, content_epoch, channel_generation, ring, ring_generation;
};
/* Register (D4C1 kind 1) and the identical Source packet (D4F1). capacity is
 * the fixed per-slot pixel capacity in bytes. */
struct sophia_or_registration {
    struct sophia_or_identity identity;
    uint32_t slots;
    uint64_t capacity;
};
/* Offer (D4C1 kind 2). Pixels are width*height*4 tightly packed bytes. */
struct sophia_or_offer {
    struct sophia_or_identity identity;
    uint64_t offer;
    uint32_t slot;
    uint64_t slot_generation, resource_id, resource_generation;
    uint64_t allocation_id, allocation_generation, output_id, output_generation;
    uint64_t topology_generation;
    uint32_t width, height;
};
/* Event (D4E1), a shell journal record. sequence is its journal sequence. */
struct sophia_or_event {
    enum sophia_or_event_kind kind;
    struct sophia_or_identity identity;
    uint64_t sequence, offer;
    uint32_t slot;
    uint64_t slot_generation, resource_id, resource_generation;
    uint16_t reason;
    uint8_t changed_generation;
};

/* slots 1..4 and a capacity that is nonzero, divisible by four and at most
 * 4 MiB. Fills the slot extent (4096 + capacity rounded up to 4096) and the
 * exact source size; either output may be NULL. */
enum sophia_ic_refusal sophia_or_geometry(uint32_t slots, uint64_t capacity, size_t *extent,
                                          size_t *source_bytes);
/* Bytes 4..8 of a record: the dispatch that precedes any version-1 header
 * decode, used only on r10 shell connections with bit 15. */
int sophia_or_is_event(const void *, size_t);
int sophia_or_is_source(const void *, size_t);

enum sophia_ic_refusal sophia_or_register_encode(uint8_t dst[SOPHIA_OR_REGISTER_BYTES],
                                                 const struct sophia_or_registration *);
enum sophia_ic_refusal sophia_or_source_encode(uint8_t dst[SOPHIA_OR_SOURCE_BYTES],
                                               const struct sophia_or_registration *);
enum sophia_ic_refusal sophia_or_offer_encode(uint8_t dst[SOPHIA_OR_OFFER_BYTES],
                                              const struct sophia_or_offer *);
enum sophia_ic_refusal sophia_or_event_encode(uint8_t dst[SOPHIA_OR_EVENT_BYTES],
                                              const struct sophia_or_event *);
/* A control-file write: Register (72 bytes, *kind 1, fills registration) or
 * Offer (152 bytes, *kind 2, fills offer). */
enum sophia_ic_refusal sophia_or_control_decode(const void *, size_t, int *kind,
                                                struct sophia_or_registration *,
                                                struct sophia_or_offer *);
enum sophia_ic_refusal sophia_or_source_decode(const void *, size_t,
                                               struct sophia_or_registration *);
enum sophia_ic_refusal sophia_or_event_decode(const void *, size_t, struct sophia_or_event *);
int sophia_or_identity_equal(const struct sophia_or_identity *, const struct sophia_or_identity *);
#endif
