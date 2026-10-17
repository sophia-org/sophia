#ifndef SOPHIA_PORTAL_REQUESTER_UPLOAD_H
#define SOPHIA_PORTAL_REQUESTER_UPLOAD_H
#include "sophia_9p_client.h"
#include "sophia_portal_requester_state.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pqu_operation_kind {
  SOPHIA_PQU_NONE,
  SOPHIA_PQU_WALK,
  SOPHIA_PQU_OPEN,
  SOPHIA_PQU_WRITE,
  SOPHIA_PQU_CLUNK
};
struct sophia_pqu_operation {
  enum sophia_pqu_operation_kind kind;
  uint32_t fid;
  uint64_t offset;
  const unsigned char *bytes;
  size_t length;
};
/* Private caller storage. Buffer is borrowed, immutable and disjoint from this
 * object for its lifetime. No allocation, socket I/O or authority acquisition.
 */
struct sophia_pqu {
  const unsigned char *bytes;
  size_t length, written, sent;
  uint32_t fid, iounit, remote_error;
  uint64_t now, deadline;
  unsigned stage, outstanding, settling;
  int error;
};
/* Only after correlated Begin custody with the same declared payload length. */
int sophia_pqu_init(struct sophia_pqu *, const void *, size_t, uint32_t maximum,
                    uint64_t now, uint64_t deadline);
int sophia_pqu_operation(struct sophia_pqu *, uint64_t now, uint32_t msize,
                         struct sophia_pqu_operation *);
/* WALK is [upload, 0] from the admitted root, OPEN uses write-only flags=1.
 * Supply the allocated fid only for WALK; otherwise NOFID. Driver matches tags.
 */
int sophia_pqu_issued(struct sophia_pqu *, uint64_t now, uint32_t msize,
                      uint32_t fid);
int sophia_pqu_reply(struct sophia_pqu *, const struct sophia_9p_reply *,
                     uint64_t now);
/* Holds the writer after all bytes. Release after End custody/terminal
 * settlement; release before End abandons upload and can revoke, but clunk is
 * not an outcome. */
int sophia_pqu_release(struct sophia_pqu *, uint64_t now);
/* After a correlated terminal event: stop writes, drain reply, then clunk.
 * ESTALE during this drain never causes reopen or replay. */
int sophia_pqu_settle(struct sophia_pqu *, uint64_t now);
int sophia_pqu_held(const struct sophia_pqu *);
int sophia_pqu_done(const struct sophia_pqu *);
void sophia_pqu_close(struct sophia_pqu *);
/* Caller closes the role fd on failure or abandonment before reusing storage.
 */
#ifdef __cplusplus
}
#endif
#endif
