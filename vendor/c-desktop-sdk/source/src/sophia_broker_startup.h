#ifndef SOPHIA_BROKER_STARTUP_H
#define SOPHIA_BROKER_STARTUP_H
#include "sophia_9p_client.h"
#include "sophia_broker_files.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_bo_error {
  SOPHIA_BO_INVALID = -1,
  SOPHIA_BO_BUSY = -2,
  SOPHIA_BO_DEADLINE = -3,
  SOPHIA_BO_CLOCK_REGRESSION = -4,
  SOPHIA_BO_CLOSED = -5,
  SOPHIA_BO_REMOTE = -6,
  SOPHIA_BO_WIRE = -7
};
/* Discovery only; negotiation follows on the same connection.
 * Caller-owned immovable state borrows wire exclusively. Close its fd on error.
 * Caller supplies a admitted metadata broker root and retains its authority
 * channel; metadata and fid ownership alone do not prove admission. No
 * attach/version, authentication, reconnect or negotiation occurs here. */
struct sophia_bo {
  struct sophia_9p_client *wire;
  struct sophia_9p_handle pending;
  struct sophia_bf_limits limits;
  uint32_t root, fid, sent, iounit, remote_error;
  uint64_t epoch, now, deadline;
  size_t used;
  unsigned stage, active, limits_file;
  int error, wire_error;
  unsigned char bytes[128];
};
int sophia_bo_init(struct sophia_bo *, struct sophia_9p_client *, uint32_t root,
                   uint64_t now, uint64_t deadline);
int sophia_bo_dispatch(struct sophia_bo *, uint64_t now, size_t budget);
int sophia_bo_ready(const struct sophia_bo *);
int sophia_bo_wants_write(const struct sophia_bo *);
void sophia_bo_close(struct sophia_bo *);
/* Ready transfers exclusive use of the same root/wire to the broker session,
 * with verified epoch/limits. Do not drive opening after that transfer. */
#ifdef __cplusplus
}
#endif
#endif
