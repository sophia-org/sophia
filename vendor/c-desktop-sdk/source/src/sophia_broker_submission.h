#ifndef SOPHIA_BROKER_SUBMISSION_H
#define SOPHIA_BROKER_SUBMISSION_H
#include "sophia_broker_files.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_bs_error {
  SOPHIA_BS_OK,
  SOPHIA_BS_INVALID,
  SOPHIA_BS_BUSY,
  SOPHIA_BS_CLOCK,
  SOPHIA_BS_DEADLINE,
  SOPHIA_BS_CLOSED,
  SOPHIA_BS_PROTOCOL,
  SOPHIA_BS_REMOTE,
  SOPHIA_BS_OVERFLOW
};
enum sophia_bs_custody {
  SOPHIA_BS_QUEUED,
  SOPHIA_BS_ISSUED,
  SOPHIA_BS_WRITE_ACCEPTED,
  SOPHIA_BS_SUBMITTED,
  SOPHIA_BS_REFUSED,
  SOPHIA_BS_DROPPED_UNSENT,
  SOPHIA_BS_UNKNOWN
};
/* Private fixed state. No transport ownership, automatic retry or replay.
 * Input records are borrowed during construction only. Errors are enum values;
 * CLOCK and BUSY leave custody usable. Initializer failure preserves output. */
struct sophia_bs_submission {
  unsigned char bytes[448], control[24];
  size_t length;
  uint64_t epoch, id, now, deadline, receipt;
  uint16_t kind;
  enum sophia_bs_custody custody;
  enum sophia_bs_error error;
  uint32_t remote_error;
  unsigned inflight;
};
int sophia_bs_negotiate(struct sophia_bs_submission *, uint64_t epoch,
                        uint64_t id, uint16_t minimum, uint16_t maximum,
                        uint64_t now, uint64_t deadline);
int sophia_bs_response(struct sophia_bs_submission *, uint64_t epoch,
                       uint64_t id, const struct sophia_bf_request *,
                       const struct sophia_bf_row *, size_t, uint64_t now,
                       uint64_t deadline);
int sophia_bs_advance(struct sophia_bs_submission *, uint64_t now);
int sophia_bs_issued(struct sophia_bs_submission *, uint64_t now);
int sophia_bs_reply(struct sophia_bs_submission *, uint32_t count,
                    uint32_t error, uint64_t now);
int sophia_bs_receipt(struct sophia_bs_submission *,
                      const struct sophia_bf_event *, uint64_t now);
int sophia_bs_releasable(const struct sophia_bs_submission *,
                         uint64_t acknowledged);
void sophia_bs_close(struct sophia_bs_submission *);
#ifdef __cplusplus
}
#endif
#endif
