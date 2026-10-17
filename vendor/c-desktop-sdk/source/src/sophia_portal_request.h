#ifndef SOPHIA_PORTAL_REQUEST_H
#define SOPHIA_PORTAL_REQUEST_H
#include "sophia_portal_capture.h"
#include "sophia_portal_startup.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pcr_error {
  SOPHIA_PCR_INVALID = -100,
  SOPHIA_PCR_UNSUPPORTED = -101,
  SOPHIA_PCR_CLOCK = -102,
  SOPHIA_PCR_CLOSED = -103,
  SOPHIA_PCR_WIRE = -104,
  SOPHIA_PCR_CAPTURE = -105,
  SOPHIA_PCR_SNAPSHOT = -106,
  SOPHIA_PCR_REMOTE = -107
};
/* Private caller-owned state. This state and the borrowed opening must not move
 * or be driven separately until finish/close. Caller closes the role fd on any
 * terminal failure and retains the launch relay separately. No allocation. */
struct sophia_pcr {
  struct sophia_po *opening;
  struct sophia_9p_client *wire;
  struct sophia_pc capture;
  struct sophia_ps_fetch fetch;
  struct sophia_9p_handle pending;
  unsigned char scratch[SOPHIA_PF_CAPTURE_STATUS_BYTES];
  unsigned stage, active;
  uint32_t fid, root, remote_error;
  uint64_t now, deadline, serial;
  int error, wire_error, snapshot_error;
};
/* Reserve a serial above the recipient owner's durable high-water mark before
 * calling; do not reuse it after failure or reconnect. Borrows the ready
 * opening exclusively and marks it closed until a successful finish restores
 * it. */
int sophia_pcr_init(struct sophia_pcr *, struct sophia_po *,
                    struct sophia_pf_capture_request, uint64_t previous_serial,
                    uint64_t now, uint64_t deadline);
/* One bounded service call. Call at next_wake even without socket readiness. */
int sophia_pcr_dispatch(struct sophia_pcr *, uint64_t now, size_t budget);
int sophia_pcr_next_wake(const struct sophia_pcr *, uint64_t *time);
int sophia_pcr_wants_write(const struct sophia_pcr *);
/* Historical terminal status only, never a frame grant. */
int sophia_pcr_result(const struct sophia_pcr *,
                      enum sophia_pf_capture_state *);
/* Restores the original opening with the same root and updated time. Output
 * serial is the next request's high-water mark, not permission to replay. */
int sophia_pcr_finish(struct sophia_pcr *, enum sophia_pf_capture_state *,
                      uint64_t *serial);
void sophia_pcr_close(struct sophia_pcr *);
#ifdef __cplusplus
}
#endif
#endif
