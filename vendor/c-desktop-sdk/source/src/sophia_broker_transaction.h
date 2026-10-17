#ifndef SOPHIA_BROKER_TRANSACTION_H
#define SOPHIA_BROKER_TRANSACTION_H
#include "sophia_9p_client.h"
#include "sophia_broker_submission.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_bt_operation_kind {
  SOPHIA_BT_NONE,
  SOPHIA_BT_WALK,
  SOPHIA_BT_OPEN,
  SOPHIA_BT_WRITE,
  SOPHIA_BT_CLUNK
};
struct sophia_bt_operation {
  enum sophia_bt_operation_kind kind;
  const char *name;
  uint32_t fid, flags;
  uint64_t offset;
  const unsigned char *bytes;
  size_t length;
};
/* Fixed caller storage, private fields. No transport ownership or I/O.
 * One driver matches tags, issues operations and closes on terminal error.
 * Operation bytes borrow this object until the matched reply. */
struct sophia_bt {
  struct sophia_bs_submission submission;
  unsigned stage, outstanding, has_assembly_due, retain_submit;
  uint32_t transaction, submit, iounit, remote_error;
  size_t offset, sent;
  uint64_t now, deadline, assembly_timeout, assembly_due;
  enum sophia_bs_error error;
};
int sophia_bt_negotiate(struct sophia_bt *, uint64_t epoch, uint64_t id,
                        uint16_t minimum, uint16_t maximum, uint64_t now,
                        uint64_t deadline, uint32_t assembly_timeout_ms);
int sophia_bt_response(struct sophia_bt *, uint64_t epoch, uint64_t id,
                       const struct sophia_bf_request *,
                       const struct sophia_bf_row *, size_t rows, uint64_t now,
                       uint64_t deadline, uint32_t assembly_timeout_ms);
/* Returns error enum; successful NONE means waiting/done. */
int sophia_bt_operation(struct sophia_bt *, uint64_t now, uint32_t msize,
                        struct sophia_bt_operation *);
/* After pipeline admission; Walk supplies fid, others supply NOFID. */
int sophia_bt_issued(struct sophia_bt *, uint64_t now, uint32_t msize,
                     uint32_t fid);
int sophia_bt_reply(struct sophia_bt *, const struct sophia_9p_reply *,
                    uint64_t now);
int sophia_bt_receipt(struct sophia_bt *, const struct sophia_bf_event *,
                      uint64_t now);
int sophia_bt_retry(struct sophia_bt *, uint64_t now);
int sophia_bt_release(struct sophia_bt *, uint64_t acknowledged, uint64_t now);
int sophia_bt_done(const struct sophia_bt *);
/* Retryable submit refusal received; false for unissued work or failure. */
int sophia_bt_retry_ready(const struct sophia_bt *);
/* Returns 1 and supplies the next deadline, or 0 after done/failure. */
int sophia_bt_deadline(const struct sophia_bt *, uint64_t *);
void sophia_bt_close(struct sophia_bt *);
#ifdef __cplusplus
}
#endif
#endif
