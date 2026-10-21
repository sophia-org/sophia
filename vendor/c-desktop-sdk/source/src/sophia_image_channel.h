#ifndef SOPHIA_IMAGE_CHANNEL_H
#define SOPHIA_IMAGE_CHANNEL_H
#include <stddef.h>
#include <stdint.h>

/* Shared shell/lock image descriptor channel, version 1, pinned to
 * spec/sophia-image-channel.md. Passive codecs only: no sockets, no
 * descriptors, no admission. Callers own I/O, seals, budgets and custody.
 * Selecting this codec does not change any advertised role revision. */

#define SOPHIA_IC_WIRE_VERSION 1u
#define SOPHIA_IC_HELLO_BYTES 40u
#define SOPHIA_IC_SOURCE_BYTES 200u
#define SOPHIA_IC_METADATA_BYTES 96u
#define SOPHIA_IC_PLANES 4u
#define SOPHIA_IC_MAX_FDS 4u
#define SOPHIA_IC_RECORD_HEADER_BYTES 32u
#define SOPHIA_IC_LIMITS_BYTES 136u
#define SOPHIA_IC_FORMATS_HEADER_BYTES 64u
#define SOPHIA_IC_FORMAT_ROW_BYTES 40u
#define SOPHIA_IC_MAX_FORMAT_ROWS 4096u
#define SOPHIA_IC_MAX_FORMATS_BYTES (64u + 40u * 4096u)
#define SOPHIA_IC_DISCOVERY_MAX 2048u
#define SOPHIA_IC_ENDPOINT_MAX 64u
#define SOPHIA_IC_SEALED_BGRA8_PREMULTIPLIED 1u

enum sophia_ic_role { SOPHIA_IC_ROLE_SHELL = 1, SOPHIA_IC_ROLE_LOCK = 2 };
enum sophia_ic_packet_kind { SOPHIA_IC_HELLO = 1, SOPHIA_IC_SOURCE = 2 };
enum sophia_ic_kind {
    SOPHIA_IC_IMAGE_BEGIN = 0x7000,
    SOPHIA_IC_IMAGE_ADMITTED = 0x7001,
    SOPHIA_IC_SOURCE_RELEASED = 0x7002,
    SOPHIA_IC_CHANNEL_READY = 0x7003,
    SOPHIA_IC_CHANNEL_CLOSED = 0x7004,
    SOPHIA_IC_IMAGE_LIMITS = 0x7005,
    SOPHIA_IC_IMAGE_FORMATS = 0x7006
};
enum sophia_ic_source_kind { SOPHIA_IC_SOURCE_SEALED = 1, SOPHIA_IC_SOURCE_DMABUF = 2 };
/* Source-kind masks in discovery and ChannelReady: bit 0 sealed, bit 1 DMA-BUF,
 * bit 2 owned ring. Shell r10 is exactly the owned ring (sophia_owned_ring.h);
 * lock and legacy shell r9 masks are nonzero subsets of bits 0 and 1. */
#define SOPHIA_IC_KIND_BIT_SEALED 1u
#define SOPHIA_IC_KIND_BIT_DMABUF 2u
#define SOPHIA_IC_KIND_BIT_OWNED_RING 4u
enum sophia_ic_disposition {
    SOPHIA_IC_RETAINED = 1,
    SOPHIA_IC_CAPTURED = 2,
    SOPHIA_IC_BEFORE_USE = 3,
    SOPHIA_IC_CANCELLED = 4,
    SOPHIA_IC_DRAINED = 5
};
enum sophia_ic_close_reason {
    SOPHIA_IC_CLOSED_PEER = 1,
    SOPHIA_IC_CLOSED_PROTOCOL = 2,
    SOPHIA_IC_CLOSED_HELLO_TIMEOUT = 3,
    SOPHIA_IC_CLOSED_REVOKED = 4
};
#define SOPHIA_IC_FORMAT_OPAQUE 1u
#define SOPHIA_IC_FORMAT_VALIDATED 2u

/* Refusal categories. Validation runs in a fixed order and reports the
 * first failed check, preserving the corpus categories. The order is not the
 * contract's field-table order. SOPHIA_IC_OK is zero. */
enum sophia_ic_refusal {
    SOPHIA_IC_OK = 0,
    SOPHIA_IC_LENGTH,
    SOPHIA_IC_VERSION,
    SOPHIA_IC_KIND,
    SOPHIA_IC_ROLE,
    SOPHIA_IC_RESERVED,
    SOPHIA_IC_EPOCH,
    SOPHIA_IC_GENERATION,
    SOPHIA_IC_FD_COUNT,
    SOPHIA_IC_OFFER,
    SOPHIA_IC_IDENTITY,
    SOPHIA_IC_SOURCE_KIND,
    SOPHIA_IC_FORMAT,
    SOPHIA_IC_PLANE_COUNT,
    SOPHIA_IC_PLANE_LAYOUT,
    SOPHIA_IC_DIMENSIONS,
    SOPHIA_IC_STRIDE,
    SOPHIA_IC_BYTE_LENGTH,
    SOPHIA_IC_MODIFIER,
    SOPHIA_IC_DIRECTION,
    SOPHIA_IC_VALUE,
    SOPHIA_IC_ORDER,
    SOPHIA_IC_DISCOVERY_FIELDS,
    SOPHIA_IC_DECIMAL,
    SOPHIA_IC_ENDPOINT,
    SOPHIA_IC_CONSTANTS,
    SOPHIA_IC_COHERENCE,
    SOPHIA_IC_CAPACITY,
    SOPHIA_IC_FORMATS_UNSUPPORTED,
    SOPHIA_IC_ARGUMENT
};
const char *sophia_ic_refusal_name(enum sophia_ic_refusal);

/* Common image metadata, 96 bytes. */
struct sophia_ic_metadata {
    uint64_t offer, resource_id, resource_generation, admission_epoch;
    uint64_t output, allocation, topology_generation;
    uint32_t source_kind, source_format, width, height, stride;
    uint16_t plane_count, descriptor_count;
    uint64_t logical_bytes, modifier;
};
struct sophia_ic_plane {
    uint64_t offset;
    uint32_t stride;
    uint16_t fd_index;
};
/* Hello has kind SOPHIA_IC_HELLO and offer 0; metadata and planes are
 * meaningful only for Source. */
struct sophia_ic_packet {
    uint16_t kind, role, descriptor_count;
    uint64_t connection_epoch, channel_generation, offer;
    struct sophia_ic_metadata metadata;
    struct sophia_ic_plane planes[SOPHIA_IC_PLANES];
};
/* What the owner expects on this channel. Bounds come from the limits
 * object; source_kinds is the granted ChannelReady mask. */
struct sophia_ic_expect {
    uint16_t role;
    uint64_t connection_epoch, channel_generation;
    uint32_t source_kinds, max_width, max_height;
    uint64_t max_resource_bytes;
};

/* Shape of one metadata value without connection bounds: nonzero identities,
 * one source kind, counts, sealed stride/bytes/modifier and checked sizes. */
enum sophia_ic_refusal sophia_ic_metadata_check(const struct sophia_ic_metadata *);
/* Shape plus limits-object bounds and the granted source kinds. */
enum sophia_ic_refusal sophia_ic_metadata_admissible(const struct sophia_ic_metadata *,
                                                     const struct sophia_ic_expect *);
/* Byte-for-byte metadata equality, the admitted-record rule. */
int sophia_ic_metadata_equal(const struct sophia_ic_metadata *, const struct sophia_ic_metadata *);

/* fds is the SCM_RIGHTS count actually received with the packet. */
enum sophia_ic_refusal sophia_ic_packet_decode(const void *, size_t, unsigned fds,
                                               const struct sophia_ic_expect *,
                                               struct sophia_ic_packet *);
enum sophia_ic_refusal sophia_ic_hello_encode(uint8_t dst[SOPHIA_IC_HELLO_BYTES],
                                              const struct sophia_ic_expect *);
enum sophia_ic_refusal sophia_ic_source_encode(uint8_t dst[SOPHIA_IC_SOURCE_BYTES],
                                               const struct sophia_ic_expect *,
                                               const struct sophia_ic_metadata *,
                                               const struct sophia_ic_plane planes[SOPHIA_IC_PLANES]);

/* Role record header, the existing 32-byte layout shared by both roles. */
struct sophia_ic_header {
    uint16_t kind;
    uint64_t epoch, submission, sequence;
};
struct sophia_ic_source_released {
    uint64_t offer, resource_id, resource_generation;
    uint32_t disposition;
};
/* ChannelReady carries the granted source-kind mask in value;
 * ChannelClosed carries the reason. */
struct sophia_ic_channel_state {
    uint64_t channel_generation;
    uint32_t value;
};
struct sophia_ic_limits {
    uint64_t channel_generation, limits_generation, max_resource_bytes;
    uint64_t max_sealed_bytes, max_dmabuf_source_bytes, max_renderer_bytes;
    uint32_t max_renderer_entries, max_offers, max_validation_jobs, max_capture_jobs;
    uint32_t max_outputs, max_rows_per_output, max_width, max_height;
    uint32_t validation_timeout_ms, capture_interest_timeout_ms;
    uint32_t validation_chunk_bytes, max_packets_per_turn;
};
struct sophia_ic_format_row {
    uint64_t output;
    uint32_t device_major, device_minor, fourcc, flags;
    uint64_t modifier;
    uint32_t max_width, max_height;
};
/* rows points into the input buffer given to sophia_ic_record_decode, not
 * into the decoded struct. That buffer must stay alive and unchanged for
 * every use of this view.
 * sophia_ic_format_row_at copies one bounds-checked row out. Packet and
 * discovery results are owned copies. */
struct sophia_ic_formats {
    uint64_t channel_generation, formats_generation, limits_generation;
    uint32_t row_count;
    const uint8_t *rows;
};
struct sophia_ic_record {
    struct sophia_ic_header header;
    union {
        struct sophia_ic_metadata image;
        struct sophia_ic_source_released released;
        struct sophia_ic_channel_state channel;
        struct sophia_ic_limits limits;
        struct sophia_ic_formats formats;
    } value;
};
/* Decodes one complete record in the image namespace. Kinds outside
 * 0x7000-0x7006 are refused; the role codecs still refuse these kinds. */
enum sophia_ic_refusal sophia_ic_record_decode(const void *, size_t, struct sophia_ic_record *);
enum sophia_ic_refusal sophia_ic_format_row_at(const struct sophia_ic_formats *, uint32_t,
                                               struct sophia_ic_format_row *);
/* Encodes any kind except formats; *written is the record size. */
enum sophia_ic_refusal sophia_ic_record_encode(void *, size_t, const struct sophia_ic_record *,
                                               size_t *written);
enum sophia_ic_refusal sophia_ic_formats_encode(void *, size_t, const struct sophia_ic_header *,
                                                const struct sophia_ic_formats *,
                                                const struct sophia_ic_format_row *rows,
                                                size_t *written);

struct sophia_ic_discovery {
    uint16_t role;
    uint64_t connection_epoch, channel_generation;
    char endpoint[SOPHIA_IC_ENDPOINT_MAX + 1];
    uint32_t source_kinds, max_packet_bytes, max_fds, max_offers, max_packets_per_turn;
    uint32_t hello_timeout_ms, validation_timeout_ms, validation_chunk_bytes;
    uint32_t capture_interest_timeout_ms;
    uint64_t formats_generation, limits_generation;
};
enum sophia_ic_refusal sophia_ic_discovery_decode(const void *, size_t, struct sophia_ic_discovery *);

/* The authenticated role's own bounds, from its negotiated limits. */
struct sophia_ic_role_bounds {
    uint64_t max_resource_bytes;
    uint32_t max_width, max_height, max_outputs, transfer_slots;
};
/* One discovery set: discovery, the limits object and the formats object,
 * each already decoded. Checks the cross-object rules: epochs, generations,
 * discovery/limits equalities, source-kind capacity, format bounds and role
 * bounds. On success fills expect for packet decoding; source_kinds there is
 * the discovery mask until ChannelReady narrows it. */
enum sophia_ic_refusal sophia_ic_discovery_coherent(const struct sophia_ic_discovery *,
                                                    const struct sophia_ic_record *limits,
                                                    const struct sophia_ic_record *formats,
                                                    const struct sophia_ic_role_bounds *,
                                                    struct sophia_ic_expect *);
#endif
