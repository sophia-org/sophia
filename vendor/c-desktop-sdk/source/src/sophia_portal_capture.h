#ifndef SOPHIA_PORTAL_CAPTURE_H
#define SOPHIA_PORTAL_CAPTURE_H
#include "sophia_portal_files.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pc_custody {
  SOPHIA_PC_QUEUED = 1,
  SOPHIA_PC_ISSUED,
  SOPHIA_PC_ACCEPTED,
  SOPHIA_PC_SETTLED,
  SOPHIA_PC_REFUSED,
  SOPHIA_PC_DROPPED_UNSENT,
  SOPHIA_PC_UNKNOWN
};
enum sophia_pc_error {
  SOPHIA_PC_INVALID = -1,
  SOPHIA_PC_BUSY = -2,
  SOPHIA_PC_CLOCK_REGRESSION = -3,
  SOPHIA_PC_DEADLINE = -4,
  SOPHIA_PC_PROTOCOL = -5,
  SOPHIA_PC_REFUSAL = -6,
  SOPHIA_PC_CLOSED = -7
};
/* Caller-owned inline storage, no allocation. Copying is not permission to
 * replay. The driver owns increasing serials within the recipient owner (also
 * across reconnects), wire correlation and closing on terminal errors. */
struct sophia_pc {
  unsigned char bytes[SOPHIA_PF_CAPTURE_BYTES];
  struct sophia_pf_capture_request request;
  uint64_t now, deadline, poll_at;
  unsigned status_inflight;
  enum sophia_pc_custody custody;
  enum sophia_pf_capture_state outcome;
  int error;
  uint32_t remote_error;
};
/* Caller-monotonic deadline at most ten seconds away; no I/O or admission. */
int sophia_pc_init(struct sophia_pc *, struct sophia_pf_capture_request,
                   uint64_t now, uint64_t deadline);
int sophia_pc_advance(struct sophia_pc *, uint64_t now);
/* Submit bytes as one offset-zero write on write-only capture, respecting
 * msize/iounit. Call issued only after pipeline admission, not on EAGAIN. */
int sophia_pc_issued(struct sophia_pc *, uint64_t now);
/* remote=1: complete Rlerror errno; remote=0: Rwrite count. Short or lost
 * replies mean unknown custody, never permission to retry. */
int sophia_pc_write_reply(struct sophia_pc *, int remote, uint32_t value,
                          uint64_t now);
/* Start a fresh immutable status snapshot; EOF/clunk share this deadline. */
int sophia_pc_start_status(struct sophia_pc *, uint64_t now,
                           uint64_t *deadline);
/* Only EOF/clunk-verified bytes. Ready is historical, not a frame grant. */
int sophia_pc_status(struct sophia_pc *, const void *, size_t, uint64_t now);
int sophia_pc_next_wake(const struct sophia_pc *, uint64_t *time);
/* Cancels local observation only; no server-side cancellation or replay. */
void sophia_pc_close(struct sophia_pc *);
#ifdef __cplusplus
}
#endif
#endif
