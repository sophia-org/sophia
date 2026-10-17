#ifndef SOPHIA_PORTAL_SESSION_H
#define SOPHIA_PORTAL_SESSION_H
#include "sophia_portal_frame.h"
#include "sophia_portal_request.h"
#include "sophia_portal_startup.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pcs_event {
  SOPHIA_PCS_REVOKED = 1,
  SOPHIA_PCS_CANCELLED,
  SOPHIA_PCS_REFUSED
};
/* Fixed private state: one watch and one snapshot/capture on the same role.
 * Exclusively borrows an immovable opening and snapshot storage. Keep relay
 * alive. Close role fd on terminal error; no automatic reconnect or replay. */
struct sophia_pcs {
  struct sophia_po *opening;
  struct sophia_ps_fetch fetch;
  struct sophia_pcr capture;
  unsigned capture_present;
  struct sophia_pfr frame;
  unsigned frame_present;
  struct sophia_9p_handle watch_tag, flush_tag, fetch_tag;
  unsigned stage, watch_active, flushing, cancel, fetch_present, fetch_active;
  uint32_t watch_fid, watch_errno;
  enum sophia_pcs_event event;
  uint64_t now, watch_deadline;
  int error, wire_error;
};
int sophia_pcs_init(struct sophia_pcs *, struct sophia_po *);
int sophia_pcs_watch(struct sophia_pcs *, uint64_t deadline);
/* Drain prior replies, flush pending read, then clunk. Revocation beating
 * Rflush wins. The original deadline bounds cleanup; expiry ends the role. */
int sophia_pcs_cancel_watch(struct sophia_pcs *);
int sophia_pcs_watch_waiting(const struct sophia_pcs *);
int sophia_pcs_watch_event(const struct sophia_pcs *, enum sophia_pcs_event *,
                           uint32_t *remote);
int sophia_pcs_release_watch(struct sophia_pcs *);
/* Caller storage remains borrowed through release. TRANSFER needs 64 KiB. */
int sophia_pcs_snapshot(struct sophia_pcs *, enum sophia_ps_file, void *storage,
                        size_t capacity, uint64_t deadline);
int sophia_pcs_snapshot_result(const struct sophia_pcs *, const void **bytes,
                               size_t *length);
int sophia_pcs_release_snapshot(struct sophia_pcs *);
/* One foreground request beside the watch. Reserve a durable increasing serial
 * before calling. The request/status storage is inline; no replay after loss.
 */
int sophia_pcs_capture(struct sophia_pcs *, struct sophia_pf_capture_request,
                       uint64_t previous_serial, uint64_t deadline);
/* Custody remains inspectable after failure. Result is historical status only,
 * not a frame grant, and remains retained until release_capture. */
int sophia_pcs_capture_custody(const struct sophia_pcs *,
                               enum sophia_pc_custody *);
int sophia_pcs_capture_result(const struct sophia_pcs *,
                              enum sophia_pf_capture_state *, uint64_t *serial);
int sophia_pcs_release_capture(struct sophia_pcs *);
/* One foreground frame beside the watch. Needs six request slots and four fids.
 * Borrows disjoint caller storage through release or terminal close. No copy or
 * allocation; local rejection leaves image bytes unchanged. Insufficient
 * capacity fails after metadata validation. */
int sophia_pcs_frame(struct sophia_pcs *, void *image, size_t capacity,
                     uint64_t deadline);
uint64_t sophia_pcs_frame_received(const struct sophia_pcs *);
int sophia_pcs_frame_result(const struct sophia_pcs *,
                            struct sophia_pf_frame_info *, const void **pixels,
                            size_t *length);
int sophia_pcs_release_frame(struct sophia_pcs *);
int sophia_pcs_dispatch(struct sophia_pcs *, uint64_t now, size_t budget);
int sophia_pcs_wants_write(const struct sophia_pcs *);
int sophia_pcs_immediate(const struct sophia_pcs *);
/* Earliest wake, including paced capture-status observation. Dispatch when due
 * even on an idle descriptor; a status-poll wake is not deadline expiry. */
int sophia_pcs_deadline(const struct sophia_pcs *, uint64_t *deadline);
/* Requires no retained events/results or work. Restores the same opening. */
int sophia_pcs_finish(struct sophia_pcs *);
void sophia_pcs_close(struct sophia_pcs *);
#ifdef __cplusplus
}
#endif
#endif
