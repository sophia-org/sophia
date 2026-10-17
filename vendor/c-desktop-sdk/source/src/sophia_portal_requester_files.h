#ifndef SOPHIA_PORTAL_REQUESTER_FILES_H
#define SOPHIA_PORTAL_REQUESTER_FILES_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_PQ_CANDIDATE_MAX 344u
enum sophia_pq_kind {
  SOPHIA_PQ_LIMITS = 1,
  SOPHIA_PQ_NEGOTIATED = 16,
  SOPHIA_PQ_REFUSED = 17,
  SOPHIA_PQ_SUBMITTED = 18,
  SOPHIA_PQ_DECISION = 32,
  SOPHIA_PQ_OUTCOME = 33,
  SOPHIA_PQ_NEGOTIATE = 256,
  SOPHIA_PQ_REQUEST = 257,
  SOPHIA_PQ_BEGIN = 258,
  SOPHIA_PQ_END = 259,
  SOPHIA_PQ_CANCEL = 260
};
enum sophia_pq_transfer_kind {
  SOPHIA_PQ_CLIPBOARD = 1,
  SOPHIA_PQ_DRAG_AND_DROP,
  SOPHIA_PQ_FILE_HANDOFF,
  SOPHIA_PQ_SCREEN_CAPTURE,
  SOPHIA_PQ_SCREEN_RECORDING,
  SOPHIA_PQ_URI_OPEN,
  SOPHIA_PQ_NOTIFICATION
};
enum sophia_pq_outcome {
  SOPHIA_PQ_COMPLETED = 2,
  SOPHIA_PQ_REVOKED = 3,
  SOPHIA_PQ_EXPIRED = 4
};
struct sophia_pq_request {
  uint64_t transfer, source, target;
  enum sophia_pq_transfer_kind kind;
  /* Borrowed only during encode. Present-empty differs from absent. */
  unsigned mime_present;
  const char *mime;
  size_t mime_length;
  uint64_t byte_size, generation, deadline_msec;
};
struct sophia_pq_candidate {
  enum sophia_pq_kind kind;
  union {
    struct {
      uint16_t minimum, maximum;
    } negotiate;
    struct sophia_pq_request request;
    struct {
      uint64_t transfer, total_bytes;
    } payload;
    uint64_t cancel;
  } data;
};
struct sophia_pq_limits {
  uint32_t max_transfers, max_payload_bytes, journal_records, journal_bytes,
      assembly_timeout_ms, ack_progress_timeout_ms, max_retained_transfers;
  uint64_t broker_generation;
};
struct sophia_pq_grant {
  uint64_t transfer;
  enum sophia_pq_transfer_kind kind;
  uint64_t source, target, source_generation, broker_generation, deadline_msec;
};
struct sophia_pq_event {
  uint64_t epoch, sequence;
  enum sophia_pq_kind kind;
  union {
    struct {
      uint64_t submission;
      enum sophia_pq_kind candidate_kind;
    } submitted;
    /* Denied keeps only transfer; all other grant fields are zero. */
    struct {
      unsigned allowed;
      struct sophia_pq_grant grant;
    } decision;
    struct {
      uint64_t transfer;
      enum sophia_pq_outcome state;
    } outcome;
  } data;
};
/* Passive exact codecs, no allocation or authority. Return 0 or -1 and leave
 * all output unchanged on failure. A decoded outcome is not execution proof
 * without a correctly admitted, correlated client session. */
int sophia_pq_decode_api(const void *, size_t, uint64_t *epoch);
int sophia_pq_decode_limits(const void *, size_t, uint64_t *epoch,
                            struct sophia_pq_limits *);
int sophia_pq_decode_event(const void *, size_t, struct sophia_pq_event *);
/* Zero request ID/generation/deadline and unsupported revision ranges remain
 * representable: the lifecycle answers Denied and negotiation answers Refused.
 * Namespace identifiers must be nonzero and distinct. */
int sophia_pq_encode_candidate(uint64_t epoch, uint64_t submission,
                               const struct sophia_pq_candidate *, void *,
                               size_t, size_t *length);
int sophia_pq_encode_submit(uint64_t epoch, uint64_t submission,
                            uint32_t candidate_bytes, unsigned char out[24]);
int sophia_pq_encode_ack(uint64_t epoch, uint64_t sequence,
                         unsigned char out[16]);
#ifdef __cplusplus
}
#endif
#endif
