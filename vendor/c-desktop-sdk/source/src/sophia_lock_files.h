#ifndef SOPHIA_LOCK_FILES_H
#define SOPHIA_LOCK_FILES_H
#include <stddef.h>
#include <stdint.h>

/* Lock provider file records, API version 1 (spec/sophia-lock-files-v1.kdl).
 * A lock provider only renders: it never receives characters, never decides
 * an unlock and cannot enter or leave the locked state. Records are complete
 * little-endian byte strings with no socket framing. Decoding checks byte
 * shape and the contract's stated rules; epochs, allocations, reserved chords
 * and budgets are the server's to judge, so a shift-only chord decodes. */
#define SOPHIA_LF_HEADER_BYTES 32u
#define SOPHIA_LF_MAX_RECORD 65536u
#define SOPHIA_LF_MAX_CANDIDATE 128u
#define SOPHIA_LF_MIN_CANDIDATE 48u
#define SOPHIA_LF_SUBMIT_BYTES 24u
#define SOPHIA_LF_ACK_BYTES 16u
#define SOPHIA_LF_REVISION 1u
#define SOPHIA_LF_MAX_OUTPUTS 16u
#define SOPHIA_LF_MAX_CHORDS 8u
#define SOPHIA_LF_MAX_SIDE 16384u
#define SOPHIA_LF_MAX_RESOURCE_BYTES 1073741824u
#define SOPHIA_LF_CAP_PRESENT 1u
#define SOPHIA_LF_CAP_CHORDS 2u
/* Chord modifier bits: shift, control, alt, super. */
#define SOPHIA_LF_MOD_SHIFT 1u
#define SOPHIA_LF_MOD_CONTROL 2u
#define SOPHIA_LF_MOD_ALT 4u
#define SOPHIA_LF_MOD_SUPER 8u
#define SOPHIA_LF_MOD_MASK 15u

enum sophia_lf_kind {
  SOPHIA_LF_LIMITS = 1,
  SOPHIA_LF_LOCK = 2,
  SOPHIA_LF_NEGOTIATED = 16,
  SOPHIA_LF_REFUSED = 17,
  SOPHIA_LF_SUBMITTED = 18,
  SOPHIA_LF_OBJECT_PUBLISHED = 19,
  SOPHIA_LF_RESOURCE_STATUS = 33,
  SOPHIA_LF_RESOURCE_RELEASED = 34,
  SOPHIA_LF_CANDIDATE_OUTCOME = 35,
  SOPHIA_LF_FRAME_PERMIT = 36,
  SOPHIA_LF_ENTRY = 40,
  SOPHIA_LF_CHORD = 41,
  SOPHIA_LF_NEGOTIATE = 256,
  SOPHIA_LF_RESOURCE_BEGIN = 258,
  SOPHIA_LF_RESOURCE_END = 259,
  SOPHIA_LF_RESOURCE_CANCEL = 260,
  SOPHIA_LF_RESOURCE_RETIRE = 261,
  SOPHIA_LF_CANDIDATE = 262,
  SOPHIA_LF_FRAME_DEMAND = 263
};
enum sophia_lf_refusal {
  SOPHIA_LF_UNSUPPORTED_REVISION = 1,
  SOPHIA_LF_PRESENTATION_REQUIRED = 2,
  SOPHIA_LF_INVALID_CHORD = 3
};
/* A provider draws for locking and locked; it stops at the others. */
enum sophia_lf_phase {
  SOPHIA_LF_UNLOCKED = 1,
  SOPHIA_LF_LOCKING = 2,
  SOPHIA_LF_LOCKED = 3,
  SOPHIA_LF_UNLOCKING = 4
};
/* What the secret did, never what it holds. */
enum sophia_lf_entry {
  SOPHIA_LF_INSERT = 1,
  SOPHIA_LF_DELETE = 2,
  SOPHIA_LF_CLEAR = 3,
  SOPHIA_LF_SUBMIT = 4,
  SOPHIA_LF_CHECKING = 5,
  SOPHIA_LF_FAILED = 6,
  SOPHIA_LF_UNAVAILABLE = 7
};
enum sophia_lf_resource_state {
  SOPHIA_LF_ADMITTED = 1,
  SOPHIA_LF_ACCEPTED = 2,
  SOPHIA_LF_RESOURCE_REJECTED = 3,
  SOPHIA_LF_CANCELLED = 4
};
enum sophia_lf_candidate_status {
  SOPHIA_LF_PREPARED = 1,
  SOPHIA_LF_PRESENTED = 2,
  SOPHIA_LF_CANDIDATE_REJECTED = 3,
  SOPHIA_LF_SUPERSEDED = 4,
  SOPHIA_LF_REVOKED = 5
};
/* Reasons are an open vocabulary: unknown values are preserved. */
enum sophia_lf_reason {
  SOPHIA_LF_REASON_NONE = 0,
  SOPHIA_LF_REASON_STALE_LOCK = 1,
  SOPHIA_LF_REASON_STALE_ALLOCATION = 2,
  SOPHIA_LF_REASON_UNKNOWN_RESOURCE = 3,
  SOPHIA_LF_REASON_SIZE_MISMATCH = 4,
  SOPHIA_LF_REASON_PERMIT = 5,
  SOPHIA_LF_REASON_BUDGET = 6,
  SOPHIA_LF_REASON_TOPOLOGY = 7,
  SOPHIA_LF_REASON_PROVIDER_REPLACED = 8
};

struct sophia_lf_header {
  uint16_t kind;
  uint64_t epoch, submission, sequence;
};
struct sophia_lf_limits {
  uint16_t max_outputs, upload_slots, max_chords, max_live_resources;
  uint32_t max_width_px, max_height_px;
  uint64_t max_resource_bytes;
  uint32_t journal_records, journal_bytes;
  uint32_t assembly_timeout_ms, ack_timeout_ms;
};
/* One whole-output allocation; a resource for it is exactly its size. */
struct sophia_lf_allocation {
  uint64_t output, output_generation, allocation, allocation_generation;
  uint32_t pixel_width, pixel_height, scale_numerator, scale_denominator;
};
/* A zero lock epoch is exactly the unlocked phase; allocations exist only
 * while locking or locked. */
struct sophia_lf_lock {
  uint64_t lock_epoch, topology_generation;
  uint16_t phase, allocation_count;
  struct sophia_lf_allocation allocations[SOPHIA_LF_MAX_OUTPUTS];
};
struct sophia_lf_chord_request {
  uint32_t keysym;   /* XKB keysym, nonzero */
  uint16_t modifiers; /* nonzero, within SOPHIA_LF_MOD_MASK */
};
struct sophia_lf_negotiate {
  uint16_t minimum_revision, maximum_revision, chord_count;
  uint64_t capabilities;
  struct sophia_lf_chord_request chords[SOPHIA_LF_MAX_CHORDS];
};
/* Granted chords keep their request order; a chord's ID is its index. */
struct sophia_lf_negotiated {
  uint16_t granted_chords;
  uint64_t granted_capabilities;
};
struct sophia_lf_submitted {
  uint64_t submission;
  uint16_t kind;
};
struct sophia_lf_published {
  uint64_t object_generation, qid_path;
};
struct sophia_lf_resource {
  uint64_t id, generation;
};
/* ResourceBegin: premultiplied BGRA8, row bytes width * 4. */
struct sophia_lf_resource_begin {
  uint64_t transaction;
  struct sophia_lf_resource resource;
  uint32_t width_px, height_px;
  uint16_t slot;
};
/* ResourceEnd names the whole size; Cancel and Retire carry zero. */
struct sophia_lf_resource_step {
  uint64_t transaction;
  struct sophia_lf_resource resource;
  uint64_t total_bytes;
};
struct sophia_lf_resource_status {
  uint64_t transaction;
  struct sophia_lf_resource resource;
  uint16_t status, reason;
  uint64_t admitted_bytes;
};
struct sophia_lf_resource_released {
  uint64_t transaction;
  struct sophia_lf_resource resource;
  uint16_t reason;
};
struct sophia_lf_candidate {
  uint64_t transaction, lock_epoch, output, output_generation;
  uint64_t allocation, allocation_generation, candidate_generation;
  uint64_t pacing_permit;
  struct sophia_lf_resource resource;
};
struct sophia_lf_candidate_outcome {
  uint64_t transaction, lock_epoch, output, allocation, candidate_generation;
  uint16_t status, reason;
};
struct sophia_lf_frame_demand {
  uint64_t transaction, lock_epoch, allocation, allocation_generation;
  uint64_t demand;
};
struct sophia_lf_frame_permit {
  uint64_t lock_epoch, allocation, allocation_generation, demand;
  uint64_t pacing_permit;
  uint32_t expires_after_ms; /* 1..250 */
};
struct sophia_lf_entry_event {
  uint64_t lock_epoch;
  uint16_t entry, empty_after;
};
struct sophia_lf_chord_event {
  uint64_t lock_epoch;
  uint16_t chord;
};
struct sophia_lf_record {
  struct sophia_lf_header header;
  union {
    struct sophia_lf_limits limits;
    struct sophia_lf_lock lock;
    struct sophia_lf_negotiate negotiate;
    struct sophia_lf_negotiated negotiated;
    uint16_t refusal;
    struct sophia_lf_submitted submitted;
    struct sophia_lf_published published;
    struct sophia_lf_resource_begin resource_begin;
    struct sophia_lf_resource_step resource_step;
    struct sophia_lf_resource_status resource_status;
    struct sophia_lf_resource_released resource_released;
    struct sophia_lf_candidate candidate;
    struct sophia_lf_candidate_outcome candidate_outcome;
    struct sophia_lf_frame_demand frame_demand;
    struct sophia_lf_frame_permit frame_permit;
    struct sophia_lf_entry_event entry;
    struct sophia_lf_chord_event chord;
  } value;
};

/* Return 0 on success, -1 for an invalid record, -4 for invalid arguments or
 * insufficient capacity. Outputs are unchanged on error. No allocation. */
int sophia_lf_decode(const void *src, size_t bytes,
                     struct sophia_lf_record *out);
int sophia_lf_encode(void *dst, size_t capacity,
                     const struct sophia_lf_record *record, size_t *bytes);
int sophia_lf_submit_encode(uint8_t dst[24], uint64_t epoch,
                            uint64_t submission, uint32_t bytes);
int sophia_lf_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence);
#endif
