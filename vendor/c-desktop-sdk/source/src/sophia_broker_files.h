#ifndef SOPHIA_BROKER_FILES_H
#define SOPHIA_BROKER_FILES_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Passive API-1 records; encoding and decoding confer no broker authority.
 * Decoders return zero on success, -1 on invalid input, preserving outputs. */
struct sophia_bf_limits {
  uint32_t max_surfaces;
  uint16_t max_label_bytes;
  uint32_t journal_records, journal_bytes, staging_bytes;
  uint32_t response_timeout_ms, ack_progress_timeout_ms, assembly_timeout_ms;
};
int sophia_bf_decode_api(const void *, size_t, uint64_t *);
int sophia_bf_decode_limits(const void *, size_t, uint64_t,
                            struct sophia_bf_limits *);
enum sophia_bf_request_kind {
  SOPHIA_BF_SURFACE_ADMITTED = 1,
  SOPHIA_BF_CANDIDATE_REDUCED,
  SOPHIA_BF_ATTENTION_CHANGED,
  SOPHIA_BF_SURFACE_REMOVED,
  SOPHIA_BF_SET_DISCLOSURE
};
struct sophia_bf_request {
  uint64_t id;
  enum sophia_bf_request_kind kind;
  uint32_t surface_index, surface_generation;
  uint16_t value;
  unsigned redacted, label_present;
  /* Borrowed from the record; present-empty is distinct from absent. */
  const char *label;
  size_t label_length;
  uint64_t candidate_generation;
};
enum sophia_bf_event_kind {
  SOPHIA_BF_NEGOTIATED = 16,
  SOPHIA_BF_REFUSED,
  SOPHIA_BF_SUBMITTED,
  SOPHIA_BF_REQUEST = 32
};
struct sophia_bf_event {
  uint64_t epoch, sequence;
  enum sophia_bf_event_kind kind;
  uint32_t max_surfaces;
  uint16_t max_label_bytes, candidate_kind;
  uint64_t submission;
  struct sophia_bf_request request;
};
int sophia_bf_decode_event(const void *, size_t, uint64_t,
                           struct sophia_bf_event *);
enum sophia_bf_row_kind {
  SOPHIA_BF_PUBLISH_RULE = 1,
  SOPHIA_BF_EMIT_DESCRIPTOR,
  SOPHIA_BF_RETIRE_SURFACE,
  SOPHIA_BF_REJECTED,
  SOPHIA_BF_NO_CHANGE
};
/* Unused fields must be zero/absent. Label borrowed only during encoding. */
struct sophia_bf_row {
  enum sophia_bf_row_kind kind;
  uint32_t surface_index, surface_generation;
  uint16_t value, disclosure, trust, attention;
  unsigned icon_present, redacted, label_present;
  uint64_t icon, generation, grant_token, grant_revocation_epoch,
      grant_target_generation;
  const char *label;
  size_t label_length;
};
/* Writes exactly 448 bytes on success; leaves output unchanged on failure.
 * Current owner state, grants and atomic enqueue capacity remain Session
 * checks. */
int sophia_bf_encode_response(uint64_t epoch, uint64_t submission,
                              const struct sophia_bf_request *,
                              const struct sophia_bf_row *, size_t rows,
                              void *output, size_t capacity);
/* Encoders return zero on success and leave output unchanged on error.
 * Negotiation preserves unsupported/inverted ranges for server refusal.
 * Submit length is 40..448; full candidate validation is separate. */
int sophia_bf_encode_negotiate(uint64_t, uint64_t, uint16_t, uint16_t, void *,
                               size_t);
int sophia_bf_encode_submit(uint64_t, uint64_t, uint32_t, void *, size_t);
int sophia_bf_encode_ack(uint64_t, uint64_t, void *, size_t);
#ifdef __cplusplus
}
#endif
#endif
