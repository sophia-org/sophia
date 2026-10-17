#ifndef SOPHIA_PORTAL_REQUESTER_STARTUP_H
#define SOPHIA_PORTAL_REQUESTER_STARTUP_H
#include "sophia_9p_client.h"
#include "sophia_portal_requester_files.h"
#include "sophia_portal_snapshot.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Same negative errors as portal snapshot, plus -7 for wire failure.
 * Caller-owned immovable state borrows wire exclusively. Close its fd on error.
 * Caller supplies a trusted requester root and retains its authority channel;
 * metadata and fid ownership alone do not prove admission. No attach/version,
 * authentication, reconnect or negotiation occurs here. */
struct sophia_pqo {
  struct sophia_9p_client *wire;
  struct sophia_9p_handle pending;
  struct sophia_pq_limits limits;
  uint32_t root, fid, sent, iounit, remote_error;
  uint64_t epoch, now, deadline;
  size_t used;
  unsigned stage, active, limits_file;
  int error, wire_error;
  unsigned char bytes[128];
};
int sophia_pqo_init(struct sophia_pqo *, struct sophia_9p_client *,
                    uint32_t root, uint64_t now, uint64_t deadline);
int sophia_pqo_dispatch(struct sophia_pqo *, uint64_t now, size_t budget);
int sophia_pqo_ready(const struct sophia_pqo *);
int sophia_pqo_wants_write(const struct sophia_pqo *);
void sophia_pqo_close(struct sophia_pqo *);
/* Ready transfers exclusive use of the same root/wire to the requester session,
 * with verified epoch/limits. Do not drive opening after that transfer. */
#ifdef __cplusplus
}
#endif
#endif
