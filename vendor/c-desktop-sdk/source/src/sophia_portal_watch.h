#ifndef SOPHIA_PORTAL_WATCH_H
#define SOPHIA_PORTAL_WATCH_H
#include "sophia_portal_startup.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Private fixed state, no allocation. Exclusively borrows an immovable ready
 * opening. Retain the launch relay separately; close the role fd on failure or
 * local cancellation. A separately walked transfer may name another object. */
struct sophia_pw {
  struct sophia_po *opening;
  struct sophia_9p_handle pending;
  uint32_t fid, remote_error;
  unsigned stage, active;
  uint64_t now, deadline;
  int error, wire_error;
};
/* Uses snapshot/startup errors. Queues work, no socket I/O. */
int sophia_pw_init(struct sophia_pw *, struct sophia_po *, uint64_t now,
                   uint64_t deadline);
int sophia_pw_dispatch(struct sophia_pw *, uint64_t now, size_t budget);
int sophia_pw_wants_write(const struct sophia_pw *);
/* Waiting means the nonempty offset-zero read is queued, not acknowledged. */
int sophia_pw_waiting(const struct sophia_pw *);
/* ESTALE on the retained object, followed by successful clunk. */
int sophia_pw_revoked(const struct sophia_pw *);
/* Restore the same opening after revocation. No automatic reopening. */
int sophia_pw_finish(struct sophia_pw *);
/* Local cancellation, not revocation. Caller must close the role fd; this API
 * does not flush and preserve the connection. */
void sophia_pw_close(struct sophia_pw *);
#ifdef __cplusplus
}
#endif
#endif
