#ifndef SOPHIA_ADMIN_SNAPSHOT_H
#define SOPHIA_ADMIN_SNAPSHOT_H
#include "sophia_admin_files.h"
#include "sophia_9p_client.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_as_file { SOPHIA_AS_INFO = 1, SOPHIA_AS_COMMANDS, SOPHIA_AS_STATUS };
enum sophia_as_error { SOPHIA_AS_INVALID = -1, SOPHIA_AS_BUSY = -2,
  SOPHIA_AS_DEADLINE = -3, SOPHIA_AS_CLOCK_REGRESSION = -4,
  SOPHIA_AS_CLOSED = -5, SOPHIA_AS_REMOTE = -6 };
enum sophia_as_operation_kind { SOPHIA_AS_WALK = 1, SOPHIA_AS_OPEN, SOPHIA_AS_READ, SOPHIA_AS_CLUNK };
struct sophia_as_operation {
  enum sophia_as_operation_kind kind;
  enum sophia_as_file file;
  uint32_t fid, count;
  uint64_t offset;
};
/* Private fields for caller-owned allocation; do not modify. Storage is
 * borrowed for the entire fetch and must not overlap this object or replies. */
struct sophia_as_fetch {
  enum sophia_as_file file;
  unsigned stage, outstanding;
  uint32_t fid, iounit, sent, remote_error;
  unsigned char *bytes;
  size_t used, maximum;
  uint64_t now, deadline;
  int error;
};
const char *sophia_as_name(enum sophia_as_file file);
size_t sophia_as_maximum(enum sophia_as_file file);
int sophia_as_init(struct sophia_as_fetch *, enum sophia_as_file,
                    void *storage, size_t capacity, uint64_t now, uint64_t deadline);
/* 1 supplies an operation, 0 means outstanding/ready, negative means failure.
 * No I/O or allocation. Connection driver matches replies and owns tags/fids. */
int sophia_as_operation(const struct sophia_as_fetch *, uint64_t now, uint32_t msize,
                         struct sophia_as_operation *);
/* Call after pipeline admission; only Walk supplies a fid, otherwise NOFID. */
int sophia_as_issued(struct sophia_as_fetch *, uint64_t now, uint32_t msize, uint32_t fid);
/* Terminal failure forbids continuation: driver closes transport. No retries. */
int sophia_as_reply(struct sophia_as_fetch *, const struct sophia_9p_reply *, uint64_t now);
/* Returns 1 only after EOF, syntax validation and successful clunk. */
int sophia_as_result(const struct sophia_as_fetch *, const void **bytes, size_t *length);
void sophia_as_close(struct sophia_as_fetch *);
#ifdef __cplusplus
}
#endif
#endif
