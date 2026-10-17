#ifndef SOPHIA_PORTAL_TRANSFER_H
#define SOPHIA_PORTAL_TRANSFER_H
#include "sophia_portal_startup.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Private state; opening and payload storage are borrowed exclusively until
 * finish/close. Neither this state nor opening may move. No allocation or
 * clearing of payload storage. Caller retains relay and closes role on error.
 */
struct sophia_pt {
  struct sophia_po *opening;
  struct sophia_ps_fetch fetch;
  struct sophia_9p_handle pending;
  unsigned active;
  uint64_t now, deadline;
  int error, wire_error;
};
/* Same errors as snapshot/startup; storage needs at least 64 KiB. */
int sophia_pt_init(struct sophia_pt *, struct sophia_po *, void *storage,
                   size_t capacity, uint64_t now, uint64_t deadline);
int sophia_pt_dispatch(struct sophia_pt *, uint64_t now, size_t budget);
int sophia_pt_wants_write(const struct sophia_pt *);
/* Scratch progress, not a completed result or a grant. */
size_t sophia_pt_received(const struct sophia_pt *);
int sophia_pt_result(const struct sophia_pt *, const void **bytes,
                     size_t *length);
/* Successful finish restores the original opening and returns the borrowed
 * payload slice. A separately walked watch is not known to identify it. */
int sophia_pt_finish(struct sophia_pt *, const void **bytes, size_t *length);
void sophia_pt_close(struct sophia_pt *);
#ifdef __cplusplus
}
#endif
#endif
