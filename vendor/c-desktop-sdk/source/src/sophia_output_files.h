#ifndef SOPHIA_OUTPUT_FILES_H
#define SOPHIA_OUTPUT_FILES_H
#include <stddef.h>
#include <stdint.h>

/* Output file records, API version 1 (spec/sophia-output-files-v1.kdl).
 * Records are complete little-endian byte strings with no socket framing.
 * Decoding checks structure; a published Topology is authoritative and must
 * also satisfy the snapshot invariants. Proposal semantics (known heads and
 * modes, stale generations, geometry, primary index) belong to the server's
 * topology owner, so a well-formed proposal naming an unknown head decodes. */
#define SOPHIA_OF_HEADER_BYTES 32u
#define SOPHIA_OF_MAX_RECORD 65536u
#define SOPHIA_OF_MAX_CANDIDATE 1784u
#define SOPHIA_OF_MIN_CANDIDATE 48u
#define SOPHIA_OF_SUBMIT_BYTES 24u
#define SOPHIA_OF_ACK_BYTES 16u
#define SOPHIA_OF_LIMITS_RECORD 72u
#define SOPHIA_OF_MAX_HEADS 16u
#define SOPHIA_OF_MAX_GROUPS 16u
#define SOPHIA_OF_MAX_MODES_PER_HEAD 128u
#define SOPHIA_OF_MAX_MEMBERS 4u
#define SOPHIA_OF_MAX_LABEL 64u
#define SOPHIA_OF_MAX_MODES 2048u
#define SOPHIA_OF_MAX_TOPOLOGY 52216u
#define SOPHIA_OF_REVISION 1u
#define SOPHIA_OF_CAP_OBSERVE 1u
#define SOPHIA_OF_CAP_CONFIGURE 2u
#define SOPHIA_OF_HEAD_CONNECTED 1u
#define SOPHIA_OF_HEAD_ENABLED 2u
#define SOPHIA_OF_HEAD_VRR_CAPABLE 4u

enum sophia_of_kind {
  SOPHIA_OF_LIMITS = 1,
  SOPHIA_OF_TOPOLOGY = 2,
  SOPHIA_OF_NEGOTIATED = 16,
  SOPHIA_OF_REFUSED = 17,
  SOPHIA_OF_SUBMITTED = 18,
  SOPHIA_OF_OBJECT_PUBLISHED = 19,
  SOPHIA_OF_OUTCOME = 32,
  SOPHIA_OF_NEGOTIATE = 256,
  SOPHIA_OF_PROPOSAL = 257
};
enum sophia_of_refusal {
  SOPHIA_OF_UNSUPPORTED_REVISION = 1,
  SOPHIA_OF_OBSERVATION_REQUIRED = 2
};
enum sophia_of_outcome_kind {
  SOPHIA_OF_VALIDATED = 1,
  SOPHIA_OF_COMMITTED = 2,
  SOPHIA_OF_STALE = 3,
  SOPHIA_OF_REJECTED = 4,
  SOPHIA_OF_ROLLED_BACK = 5,
  SOPHIA_OF_FAILED = 6
};
/* Reasons are an open vocabulary: unknown values are preserved. */
enum sophia_of_reason {
  SOPHIA_OF_REASON_NONE = 0,
  SOPHIA_OF_REASON_STALE = 1,
  SOPHIA_OF_REASON_PREPARATION = 2,
  SOPHIA_OF_REASON_APPLY = 3,
  SOPHIA_OF_REASON_HEAD_LOST = 4,
  SOPHIA_OF_REASON_FIRST_PRESENTATION = 5,
  SOPHIA_OF_REASON_ROLLBACK = 6,
  SOPHIA_OF_REASON_INVARIANT = 7
};
enum sophia_of_intent { SOPHIA_OF_VALIDATE_ONLY = 1, SOPHIA_OF_APPLY = 2 };
/* A head's transform mask sets bit (value - 1) for each supported value. */
enum sophia_of_transform {
  SOPHIA_OF_NORMAL = 1,
  SOPHIA_OF_ROTATE90 = 2,
  SOPHIA_OF_ROTATE180 = 3,
  SOPHIA_OF_ROTATE270 = 4,
  SOPHIA_OF_FLIPPED = 5,
  SOPHIA_OF_FLIPPED90 = 6,
  SOPHIA_OF_FLIPPED180 = 7,
  SOPHIA_OF_FLIPPED270 = 8
};
enum sophia_of_vrr {
  SOPHIA_OF_VRR_DISABLED = 1,
  SOPHIA_OF_VRR_AUTOMATIC = 2,
  SOPHIA_OF_VRR_ALWAYS = 3
};
enum sophia_of_mapping {
  SOPHIA_OF_FIT = 1,
  SOPHIA_OF_COVER = 2,
  SOPHIA_OF_EXACT = 3
};

struct sophia_of_header {
  uint16_t kind;
  uint64_t epoch, submission, sequence;
};
/* The seven revision-1 fixed bounds are checked, not stored. */
struct sophia_of_limits {
  uint32_t journal_records, journal_bytes, staging_bytes;
  uint32_t assembly_timeout_ms, ack_timeout_ms, max_domain_transactions;
};
struct sophia_of_negotiate {
  uint16_t minimum_revision, maximum_revision;
  uint64_t capabilities; /* Unknown bits are intersected by the server. */
};
struct sophia_of_submitted {
  uint64_t submission;
  uint16_t kind;
};
struct sophia_of_published {
  uint64_t topology_epoch, qid_path;
};
struct sophia_of_outcome {
  uint64_t transaction, topology_epoch;
  uint16_t outcome, reason;
};
struct sophia_of_member {
  uint64_t head;
  uint16_t mapping;
};
struct sophia_of_head_target {
  uint64_t head, generation, mode;
  uint16_t transform, vrr;
};
struct sophia_of_proposal_group {
  uint64_t output; /* Zero requests a new logical output identity. */
  int32_t x, y, width, height;
  uint16_t member_count;
  struct sophia_of_member members[SOPHIA_OF_MAX_MEMBERS];
};
struct sophia_of_proposal {
  uint64_t transaction, base_topology_epoch;
  uint16_t intent, primary_group_index, head_count, group_count;
  struct sophia_of_head_target heads[SOPHIA_OF_MAX_HEADS];
  struct sophia_of_proposal_group groups[SOPHIA_OF_MAX_GROUPS];
};
/* Every record except Topology, whose rows are decoded separately. */
struct sophia_of_record {
  struct sophia_of_header header;
  union {
    struct sophia_of_limits limits;
    struct sophia_of_negotiate negotiate;
    uint64_t granted_capabilities; /* Negotiated */
    uint16_t refusal;
    struct sophia_of_submitted submitted;
    struct sophia_of_published published;
    struct sophia_of_outcome outcome;
    struct sophia_of_proposal proposal;
  } value;
};

struct sophia_of_mode {
  uint64_t mode;
  int32_t width, height;
  uint32_t refresh_millihz;
  uint16_t preferred;
};
struct sophia_of_head {
  uint64_t head, generation;
  uint16_t flags, transforms;
  uint64_t current_mode; /* Zero is absent. */
  uint16_t first_mode, mode_count, label_bytes;
  char label[SOPHIA_OF_MAX_LABEL + 1]; /* UTF-8, NUL-terminated copy. */
};
struct sophia_of_group {
  uint64_t output, generation;
  int32_t x, y, width, height;
  uint16_t member_count;
  struct sophia_of_member members[SOPHIA_OF_MAX_MEMBERS];
};
/* Modes are one flattened table; head i owns [first_mode, +mode_count). */
struct sophia_of_topology {
  uint64_t topology_epoch, primary_output;
  uint16_t head_count, group_count, mode_count;
  struct sophia_of_head heads[SOPHIA_OF_MAX_HEADS];
  struct sophia_of_group groups[SOPHIA_OF_MAX_GROUPS];
  struct sophia_of_mode modes[SOPHIA_OF_MAX_MODES];
};

/* Return 0 on success, -1 for an invalid record, -4 for invalid arguments or
 * insufficient capacity. Outputs are unchanged on error, except that a failed
 * topology_decode zeroes *out. No allocation. decode accepts every kind but
 * Topology; topology_decode accepts only it. Label bytes are preserved exactly
 * (label_bytes); the NUL-terminated copy is a convenience. */
int sophia_of_decode(const void *src, size_t bytes,
                     struct sophia_of_record *out);
int sophia_of_encode(void *dst, size_t capacity,
                     const struct sophia_of_record *record, size_t *bytes);
int sophia_of_topology_decode(const void *src, size_t bytes,
                              struct sophia_of_header *header,
                              struct sophia_of_topology *out);
int sophia_of_topology_encode(void *dst, size_t capacity, uint64_t epoch,
                              const struct sophia_of_topology *topology,
                              size_t *bytes);
/* The snapshot invariants alone, for callers building a Topology. */
int sophia_of_topology_valid(const struct sophia_of_topology *);
int sophia_of_submit_encode(uint8_t dst[24], uint64_t epoch,
                            uint64_t submission, uint32_t bytes);
int sophia_of_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence);
#endif
