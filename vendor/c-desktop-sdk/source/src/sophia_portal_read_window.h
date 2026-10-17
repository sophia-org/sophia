#ifndef SOPHIA_PORTAL_READ_WINDOW_H
#define SOPHIA_PORTAL_READ_WINDOW_H
#include "sophia_portal_files.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_PR_WINDOW 4u
enum sophia_pr_error {
  SOPHIA_PR_INVALID = -1,
  SOPHIA_PR_CLOSED = -2,
  SOPHIA_PR_EXHAUSTED = -3
};
struct sophia_pr_request {
  uint64_t token, offset;
  uint32_t count;
};
struct sophia_pr_slot {
  uint64_t token, offset;
  uint32_t count;
  unsigned pending;
};
/* Private fields; fixed storage, no payload allocation or transport ownership.
 */
struct sophia_pr_window {
  struct sophia_pr_slot slots[SOPHIA_PR_WINDOW];
  uint64_t length, next, received, serial;
  uint32_t chunk;
  int error;
};
int sophia_pr_init(struct sophia_pr_window *, uint64_t length, uint32_t msize,
                   uint32_t iounit, uint32_t chunk);
/* 1 reserves a range, 0 means full/done, negative means error. If wire
 * admission is busy, retain and submit this exact request once. */
int sophia_pr_reserve(struct sophia_pr_window *, struct sophia_pr_request *);
/* Match the reply first, validate here, then place borrowed data at offset.
 * Short reads schedule only unread suffixes. Failure leaves offset unchanged.
 */
int sophia_pr_accept(struct sophia_pr_window *, uint64_t token, size_t length,
                     uint64_t *offset);
/* Payload only: caller still verifies EOF, retained metadata and clunk. */
int sophia_pr_payload_complete(const struct sophia_pr_window *);
/* Required after wire error, expiry, revocation or cancellation; no replay. */
void sophia_pr_close(struct sophia_pr_window *);
#ifdef __cplusplus
}
#endif
#endif
