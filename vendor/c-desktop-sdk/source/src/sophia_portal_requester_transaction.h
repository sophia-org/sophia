#ifndef SOPHIA_PORTAL_REQUESTER_TRANSACTION_H
#define SOPHIA_PORTAL_REQUESTER_TRANSACTION_H
#include "sophia_9p_client.h"
#include "sophia_portal_requester_state.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pqt_operation_kind {
  SOPHIA_PQT_NONE,
  SOPHIA_PQT_WALK,
  SOPHIA_PQT_OPEN,
  SOPHIA_PQT_WRITE,
  SOPHIA_PQT_CLUNK
};
struct sophia_pqt_operation {
  enum sophia_pqt_operation_kind kind;
  const char *name;
  uint32_t fid, flags;
  uint64_t offset;
  const unsigned char *bytes;
  size_t length;
};
/* Fixed caller storage, private fields. No transport ownership or I/O.
 * One driver matches tags, issues operations and closes on terminal error.
 * Operation bytes borrow this object until the matched reply. */
struct sophia_pqt {
  struct sophia_pqs_submission submission;
  unsigned stage, outstanding, has_assembly_due;
  uint32_t transaction, submit, iounit, remote_error;
  size_t offset, sent;
  uint64_t now, deadline, assembly_timeout, assembly_due;
  enum sophia_pqs_error error;
};
int sophia_pqt_init(struct sophia_pqt *, uint64_t epoch, uint64_t id,
                    const struct sophia_pq_candidate *, uint64_t now,
                    uint64_t deadline, uint32_t assembly_timeout_ms);
/* Returns error enum; successful NONE means waiting/done. */
int sophia_pqt_operation(struct sophia_pqt *, uint64_t now, uint32_t msize,
                         struct sophia_pqt_operation *);
/* After pipeline admission; Walk supplies fid, others supply NOFID. */
int sophia_pqt_issued(struct sophia_pqt *, uint64_t now, uint32_t msize,
                      uint32_t fid);
int sophia_pqt_reply(struct sophia_pqt *, const struct sophia_9p_reply *,
                     uint64_t now);
int sophia_pqt_receipt(struct sophia_pqt *, const struct sophia_pq_event *,
                       uint64_t now);
int sophia_pqt_retry(struct sophia_pqt *, uint64_t now);
int sophia_pqt_release(struct sophia_pqt *, uint64_t acknowledged,
                       uint64_t now);
int sophia_pqt_done(const struct sophia_pqt *);
/* Returns 1 and supplies the next deadline, or 0 after done/failure. */
int sophia_pqt_deadline(const struct sophia_pqt *, uint64_t *);
void sophia_pqt_close(struct sophia_pqt *);
#ifdef __cplusplus
}
#endif
#endif
