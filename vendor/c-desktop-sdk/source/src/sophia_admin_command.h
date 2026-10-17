#ifndef SOPHIA_ADMIN_COMMAND_H
#define SOPHIA_ADMIN_COMMAND_H
#include "sophia_admin_files.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_ac_custody { SOPHIA_AC_QUEUED = 1, SOPHIA_AC_ISSUED, SOPHIA_AC_ACCEPTED,
  SOPHIA_AC_SETTLED, SOPHIA_AC_REFUSED, SOPHIA_AC_DROPPED_UNSENT, SOPHIA_AC_UNKNOWN };
enum sophia_ac_error { SOPHIA_AC_INVALID = -1, SOPHIA_AC_BUSY = -2,
  SOPHIA_AC_CLOCK_REGRESSION = -3, SOPHIA_AC_DEADLINE = -4,
  SOPHIA_AC_PROTOCOL = -5, SOPHIA_AC_REFUSAL = -6, SOPHIA_AC_CLOSED = -7 };
/* Private fields for caller allocation. Copying is not permission to replay. */
struct sophia_ac {
  unsigned char bytes[512], detail[256];
  size_t length, detail_length;
  uint64_t connection, request, generation, now, deadline, operation_deadline, poll_at;
  uint32_t frame, remote_error;
  enum sophia_af_owner owner;
  enum sophia_af_outcome outcome;
  enum sophia_ac_custody custody;
  unsigned reload, status_inflight;
  int error;
};
/* Requires a fully decoded unchanged catalog. Allocates nothing. Session owns
 * monotonic request sequencing and allows only one pending command. */
int sophia_ac_init(struct sophia_ac *, const struct sophia_af_info *,
                    const struct sophia_af_catalog *, const struct sophia_af_command *,
                    uint64_t request, uint64_t now);
int sophia_ac_advance(struct sophia_ac *, uint64_t now);
/* Call after pipeline admits the complete immutable ctl write. */
int sophia_ac_issued(struct sophia_ac *, uint64_t now);
/* remote=1 means a complete Rlerror and value is errno; otherwise Rwrite count. */
int sophia_ac_write_reply(struct sophia_ac *, int remote, uint32_t value, uint64_t now);
/* Starts a status snapshot only when due; returns absolute snapshot deadline. */
int sophia_ac_start_status(struct sophia_ac *, uint64_t now, uint64_t *deadline);
/* Supply only the complete EOF/clunk-verified snapshot. */
int sophia_ac_status(struct sophia_ac *, const void *bytes, size_t length, uint64_t now);
int sophia_ac_next_wake(const struct sophia_ac *, uint64_t *time);
int sophia_ac_invalidate_catalog(const struct sophia_ac *);
void sophia_ac_close(struct sophia_ac *);
#ifdef __cplusplus
}
#endif
#endif
