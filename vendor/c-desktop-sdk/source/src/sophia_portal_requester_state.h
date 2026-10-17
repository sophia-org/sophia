#ifndef SOPHIA_PORTAL_REQUESTER_STATE_H
#define SOPHIA_PORTAL_REQUESTER_STATE_H
#include "sophia_portal_requester_files.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pqs_error {
  SOPHIA_PQS_OK,
  SOPHIA_PQS_INVALID,
  SOPHIA_PQS_BUSY,
  SOPHIA_PQS_CLOCK,
  SOPHIA_PQS_DEADLINE,
  SOPHIA_PQS_CLOSED,
  SOPHIA_PQS_PROTOCOL,
  SOPHIA_PQS_REMOTE,
  SOPHIA_PQS_OVERFLOW
};
enum sophia_pqs_custody {
  SOPHIA_PQS_QUEUED,
  SOPHIA_PQS_ISSUED,
  SOPHIA_PQS_WRITE_ACCEPTED,
  SOPHIA_PQS_SUBMITTED,
  SOPHIA_PQS_REFUSED,
  SOPHIA_PQS_DROPPED_UNSENT,
  SOPHIA_PQS_UNKNOWN
};
/* Fixed storage; fields are implementation state, not caller-mutable values.
 * No transport or authority is acquired. The owner correlates replies, closes
 * the connection on terminal error and never replays across epochs.
 * Functions return an error enum; CLOCK and BUSY do not poison the state.
 * Reply functions take count and wire errno (zero means successful Rwrite). */
struct sophia_pqs_submission {
  unsigned char bytes[344], control[24];
  size_t length;
  uint64_t epoch, id, now, deadline, receipt;
  enum sophia_pq_kind kind;
  enum sophia_pqs_custody custody;
  enum sophia_pqs_error error;
  uint32_t remote_error;
  unsigned inflight;
};
int sophia_pqs_init(struct sophia_pqs_submission *, uint64_t epoch, uint64_t id,
                    const struct sophia_pq_candidate *, uint64_t now,
                    uint64_t deadline);
int sophia_pqs_advance(struct sophia_pqs_submission *, uint64_t now);
int sophia_pqs_issued(struct sophia_pqs_submission *, uint64_t now);
int sophia_pqs_reply(struct sophia_pqs_submission *, uint32_t count,
                     uint32_t error, uint64_t now);
int sophia_pqs_receipt(struct sophia_pqs_submission *,
                       const struct sophia_pq_event *, uint64_t now);
int sophia_pqs_releasable(const struct sophia_pqs_submission *,
                          uint64_t acknowledged);
void sophia_pqs_close(struct sophia_pqs_submission *);

struct sophia_pqj_journal {
  uint64_t epoch, now, timeout, due, offset, decoded, consumed, acknowledged,
      ack_sequence;
  unsigned char bytes[512], ack[16];
  size_t start, end;
  struct sophia_pq_event head;
  enum sophia_pqs_error error;
  uint32_t remote_error;
  unsigned has_head, has_due, has_ack;
};
int sophia_pqj_init(struct sophia_pqj_journal *, uint64_t epoch, uint64_t now,
                    uint32_t timeout);
int sophia_pqj_advance(struct sophia_pqj_journal *, uint64_t now);
uint32_t sophia_pqj_read_count(const struct sophia_pqj_journal *);
int sophia_pqj_receive(struct sophia_pqj_journal *, const void *, size_t,
                       uint64_t now);
/* Borrowed head remains until consume or terminal failure. */
const struct sophia_pq_event *
sophia_pqj_event(const struct sophia_pqj_journal *);
int sophia_pqj_consume(struct sophia_pqj_journal *, uint64_t now);
/* Freezes cumulative ack in journal.ack; later consumption cannot change it.
 * Only successful ack_reply advances journal.acknowledged. */
int sophia_pqj_begin_ack(struct sophia_pqj_journal *, uint64_t now);
int sophia_pqj_ack_reply(struct sophia_pqj_journal *, uint32_t count,
                         uint32_t error, uint64_t now);
void sophia_pqj_close(struct sophia_pqj_journal *);
#ifdef __cplusplus
}
#endif
#endif
