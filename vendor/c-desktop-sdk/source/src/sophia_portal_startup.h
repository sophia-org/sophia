#ifndef SOPHIA_PORTAL_STARTUP_H
#define SOPHIA_PORTAL_STARTUP_H
#include "sophia_portal_snapshot.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_PO_WIRE_ERROR (-7)
/* Private caller-owned state; must not move after initialization. Wire and
 * storage are borrowed exclusively. Caller closes the role fd on failure. */
struct sophia_po {
  struct sophia_9p_client *wire;
  struct sophia_ps_fetch fetch;
  struct sophia_pf_api info;
  struct sophia_9p_handle pending;
  uint32_t root, remote_error;
  unsigned stage, active;
  uint64_t now, deadline;
  int error, wire_error;
  unsigned char scratch[SOPHIA_PF_API_MAX_BYTES];
};
/* Requires the sole root from completed launch authentication, with quiescent
 * role transport. Retain the launch relay separately. This does no auth,
 * attach, version or reconnect; root ownership alone proves no authority. */
int sophia_po_init(struct sophia_po *, struct sophia_9p_client *, uint32_t root,
                   uint64_t now, uint64_t deadline);
/* One bounded service call; no sleep. Deadline spans all metadata operations.
 */
int sophia_po_dispatch(struct sophia_po *, uint64_t now, size_t budget);
int sophia_po_ready(const struct sophia_po *);
int sophia_po_wants_write(const struct sophia_po *);
void sophia_po_close(struct sophia_po *);
/* Ready hands the same root/wire and decoded info to the session. */
#ifdef __cplusplus
}
#endif
#endif
