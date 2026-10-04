/* One service pass of the session-lock-provider stand-in, and the failure
 * line it reports. Shared with the stand-in's control test
 * (tools/tests/qemu_lock_provider_service_control.c), so the test runs the
 * stand-in's own path against real peers.
 *
 * The pass records what sophia_lc_service returned, the wire's terminal
 * value and errno, all taken the moment the call returns, before anything
 * else can change errno. errno is cleared before the call and means
 * something only when the wire's terminal value is SOPHIA_9P_IO. */
#ifndef SOPHIA_QEMU_LOCK_PROVIDER_SERVICE_H
#define SOPHIA_QEMU_LOCK_PROVIDER_SERVICE_H
#include "sophia_lock_client.h"
#include <errno.h>
#include <stdio.h>

struct standin_service {
  int rc, wire, error;
};

static int standin_service(struct sophia_lc_client *client,
                           const struct sophia_9p_client *wire, size_t budget,
                           struct standin_service *out) {
  int rc;
  errno = 0;
  rc = sophia_lc_service(client, budget);
  out->error = errno;
  out->rc = rc;
  out->wire = wire->terminal;
  return rc;
}

/* The verifier reads this line: the client's state, the server's errno and
 * refusal, then the recorded pass. */
static int standin_failure(char *line, size_t room, const char *mode,
                           const char *step,
                           const struct sophia_lc_client *client,
                           const struct standin_service *pass) {
  return snprintf(line, room,
                  "sophia_qemu_lock_provider schema=1 mode=%s state=failed "
                  "step=%s client=%d remote=%u refusal=%u service_rc=%d "
                  "wire=%d errno=%d\n",
                  mode, step, (int)sophia_lc_state(client),
                  sophia_lc_remote_error(client), sophia_lc_refusal(client),
                  pass->rc, pass->wire, pass->error);
}
#endif
