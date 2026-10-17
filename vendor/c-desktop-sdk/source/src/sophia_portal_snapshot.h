#ifndef SOPHIA_PORTAL_SNAPSHOT_H
#define SOPHIA_PORTAL_SNAPSHOT_H
#include "sophia_9p_client.h"
#include "sophia_portal_files.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_ps_file {
  SOPHIA_PS_API = 1,
  SOPHIA_PS_CAPTURE_STATUS,
  SOPHIA_PS_STATUS,
  SOPHIA_PS_TRANSFER
};
#define SOPHIA_PS_TRANSFER_MAX_BYTES 65536u
enum sophia_ps_error {
  SOPHIA_PS_INVALID = -1,
  SOPHIA_PS_BUSY = -2,
  SOPHIA_PS_DEADLINE = -3,
  SOPHIA_PS_CLOCK_REGRESSION = -4,
  SOPHIA_PS_CLOSED = -5,
  SOPHIA_PS_REMOTE = -6
};
enum sophia_ps_operation_kind {
  SOPHIA_PS_WALK = 1,
  SOPHIA_PS_OPEN,
  SOPHIA_PS_READ,
  SOPHIA_PS_CLUNK
};
struct sophia_ps_operation {
  enum sophia_ps_operation_kind kind;
  enum sophia_ps_file file;
  uint32_t fid, count;
  uint64_t offset;
};
/* Private fields for caller-owned allocation; do not modify. Storage is
 * borrowed for the entire fetch and must not overlap this object or replies.
 * TRANSFER needs 64 KiB; init neither clears nor allocates it. Partial bytes
 * remain scratch until EOF and clunk. A separately walked watch is not known
 * to refer to this transfer; no private qid arithmetic is permitted. */
struct sophia_ps_fetch {
  enum sophia_ps_file file;
  unsigned stage, outstanding;
  uint32_t fid, iounit, sent, remote_error;
  unsigned char *bytes;
  size_t used, maximum;
  uint64_t now, deadline;
  int error;
};
const char *sophia_ps_name(enum sophia_ps_file file);
size_t sophia_ps_maximum(enum sophia_ps_file file);
int sophia_ps_init(struct sophia_ps_fetch *, enum sophia_ps_file, void *storage,
                   size_t capacity, uint64_t now, uint64_t deadline);
/* 1 supplies an operation, 0 means outstanding/ready, negative means failure.
 * No I/O or allocation. Connection driver matches replies and owns tags/fids.
 */
int sophia_ps_operation(const struct sophia_ps_fetch *, uint64_t now,
                        uint32_t msize, struct sophia_ps_operation *);
/* Call after pipeline admission; only Walk supplies a fid, otherwise NOFID. */
int sophia_ps_issued(struct sophia_ps_fetch *, uint64_t now, uint32_t msize,
                     uint32_t fid);
/* Terminal failure forbids continuation: driver closes transport. No retries.
 */
int sophia_ps_reply(struct sophia_ps_fetch *, const struct sophia_9p_reply *,
                    uint64_t now);
/* Returns 1 only after EOF, syntax validation and successful clunk. */
int sophia_ps_result(const struct sophia_ps_fetch *, const void **bytes,
                     size_t *length);
void sophia_ps_close(struct sophia_ps_fetch *);
#ifdef __cplusplus
}
#endif
#endif
