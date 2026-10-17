#ifndef SOPHIA_PORTAL_FRAME_H
#define SOPHIA_PORTAL_FRAME_H
#include "sophia_portal_read_window.h"
#include "sophia_portal_startup.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pfr_error {
  SOPHIA_PFR_INVALID = -100,
  SOPHIA_PFR_WIRE = -101,
  SOPHIA_PFR_REMOTE = -102,
  SOPHIA_PFR_DEADLINE = -103,
  SOPHIA_PFR_CLOCK = -104,
  SOPHIA_PFR_CLOSED = -105,
  SOPHIA_PFR_CAPACITY = -106
};
struct sophia_pfr_read {
  struct sophia_9p_handle handle;
  struct sophia_pr_request request;
  unsigned active;
};
/* Private fields. Storage and wire are exclusively borrowed; no allocation.
 * Keep image storage disjoint from this state, the wire and its storage.
 * Partial bytes are scratch, never a complete image; retire them on failure.
 * The caller retains the relay and must close the role fd after any failure. */
struct sophia_pfr {
  struct sophia_9p_client *wire;
  struct sophia_pf_frame_info info;
  struct sophia_pr_window window;
  struct sophia_pfr_read reads[4];
  struct sophia_9p_handle pending;
  uint32_t root, info_fid, frame_fid, info_unit, sent, remote_error;
  unsigned stage, active;
  unsigned char record[64], check[64], *pixels;
  size_t used, capacity, length;
  uint64_t now, deadline;
  int error, wire_error;
};
/* Consumes ready startup on success. Insufficient image storage fails after
 * metadata validation with CAPACITY; no image result is exposed. */
int sophia_pfr_init(struct sophia_pfr *, struct sophia_po *, void *image,
                    size_t capacity, uint64_t now, uint64_t deadline);
int sophia_pfr_dispatch(struct sophia_pfr *, uint64_t now, size_t budget);
int sophia_pfr_wants_write(const struct sophia_pfr *);
/* Available only after exact EOF, original metadata recheck and both clunks. */
int sophia_pfr_result(const struct sophia_pfr *, struct sophia_pf_frame_info *,
                      const void **pixels, size_t *length);
void sophia_pfr_close(struct sophia_pfr *);
/* Accepted payload bytes, including incomplete scratch; not image validity. */
uint64_t sophia_pfr_received(const struct sophia_pfr *);
/* After success, start another explicit capture read on the same root/wire.
 * Invalid arguments preserve the completed result. No reconnect or api fetch.
 */
int sophia_pfr_restart(struct sophia_pfr *, void *image, size_t capacity,
                       uint64_t now, uint64_t deadline);
/* On success the same root/wire remain ready for the next operation. */
#ifdef __cplusplus
}
#endif
#endif
