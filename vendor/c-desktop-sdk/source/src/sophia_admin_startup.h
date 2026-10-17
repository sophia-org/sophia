#ifndef SOPHIA_ADMIN_STARTUP_H
#define SOPHIA_ADMIN_STARTUP_H
#include "sophia_admin_snapshot.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_AO_WIRE_ERROR (-7)
/* Private state for caller allocation. Borrows a versioned exclusive pipeline;
 * caller retains fd/storage ownership and closes it on any terminal failure. */
struct sophia_ao {
  struct sophia_9p_client *wire;
  struct sophia_as_fetch fetch;
  struct sophia_af_info info;
  struct sophia_9p_handle pending;
  uint32_t root, ctl, remote_error;
  unsigned stage, active;
  uint64_t now, deadline;
  int error, wire_error;
  unsigned char scratch[SOPHIA_AF_LINE_MAX_BYTES];
};
/* No I/O. Requires versioned/quiescent wire with no fids. HostDomain admission
 * belongs to the export; no launch authentication or fallback is attempted. */
int sophia_ao_init(struct sophia_ao *, struct sophia_9p_client *, uint64_t now, uint64_t deadline);
/* One bounded pipeline service call per dispatch, then queue the next operation.
 * No sleeps. Clock regression returns -4 without changing state. */
int sophia_ao_dispatch(struct sophia_ao *, uint64_t now, size_t byte_budget);
int sophia_ao_ready(const struct sophia_ao *);
int sophia_ao_wants_write(const struct sophia_ao *);
void sophia_ao_close(struct sophia_ao *);
/* On readiness the same wire, root and ctl in this object pass to the session
 * driver. Stop dispatching startup after that handoff. No second attach/version. */
#ifdef __cplusplus
}
#endif
#endif
