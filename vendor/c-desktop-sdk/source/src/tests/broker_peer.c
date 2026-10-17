#define _POSIX_C_SOURCE 200809L
#include "sophia_broker_session.h"
#include <assert.h>
#include <sched.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static uint64_t now(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static void wait_reply(struct sophia_9p_client *wire,
                       struct sophia_9p_reply *reply) {
  do {
    assert(sophia_9p_service(wire, 65536) >= 0);
    sched_yield();
  } while (sophia_9p_peek(wire, reply) == SOPHIA_9P_AGAIN);
}
/* Explicit owner-supplied path or stdin; failure never selects another route.
 */
static int supplied_socket(void) {
  const char *path = getenv("SOPHIA_BROKER_9P_SOCKET");
  struct sockaddr_un address = {0};
  int fd;
  if (!path)
    return STDIN_FILENO;
  assert(strlen(path) < sizeof address.sun_path);
  address.sun_family = AF_UNIX;
  memcpy(address.sun_path, path, strlen(path) + 1);
  fd = socket(AF_UNIX, SOCK_STREAM, 0);
  assert(fd >= 0);
  assert(!connect(fd, (const struct sockaddr *)&address, sizeof address));
  return fd;
}
int main(void) {
  struct sophia_9p_client wire;
  struct sophia_9p_handle tag;
  struct sophia_9p_reply reply;
  struct sophia_bo opening;
  struct sophia_br s;
  struct sophia_bf_event event;
  struct sophia_bf_request request;
  size_t size = sophia_9p_storage_bytes(4096, 8);
  void *storage = malloc(size);
  uint32_t root;
  uint64_t deadline = now() + 8000;
  int sent = 0, e, fd;
  alarm(9);
  fd = supplied_socket();
  assert(storage);
  assert(!sophia_9p_init(&wire, fd, 4096, 8, 16, storage, size));
  assert(!sophia_9p_version(&wire, &tag));
  wait_reply(&wire, &reply);
  assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
  assert(!sophia_9p_attach(&wire, "", "", &tag, &root));
  wait_reply(&wire, &reply);
  assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
  assert(!sophia_bo_init(&opening, &wire, root, now(), deadline));
  while (!sophia_bo_ready(&opening)) {
    assert(!sophia_bo_dispatch(&opening, now(), 65536));
    sched_yield();
  }
  assert(!sophia_br_init(&s, &opening, 2, 2, deadline));
  for (;;) {
    uint64_t time = now();
    assert(time < deadline);
    assert(!sophia_br_dispatch(&s, time, 65536));
    while (!(e = sophia_br_event(&s, &event)))
      assert(!sophia_br_consume(&s, time));
    assert(e == SOPHIA_BS_BUSY);
    e = sophia_br_acknowledge(&s, time);
    assert(!e || e == SOPHIA_BS_BUSY);
    if (!sent && sophia_br_transaction_ready(&s) &&
        !sophia_br_request(&s, &request)) {
      struct sophia_bf_row rows[2] = {{0}};
      size_t count = 1;
      rows[0].kind = SOPHIA_BF_RETIRE_SURFACE;
      rows[0].surface_index = request.surface_index;
      rows[0].surface_generation = request.surface_generation;
      if (request.kind == SOPHIA_BF_SET_DISCLOSURE) {
        rows[0].kind = SOPHIA_BF_PUBLISH_RULE;
        rows[0].disclosure = request.value;
        rows[0].trust = 1;
        rows[0].generation = 8;
        rows[1].kind = SOPHIA_BF_EMIT_DESCRIPTOR;
        rows[1].surface_index = request.surface_index;
        rows[1].surface_generation = request.surface_generation;
        rows[1].trust = 1;
        rows[1].attention = 2;
        rows[1].generation = 8;
        rows[1].grant_token = 12;
        rows[1].grant_revocation_epoch = 1;
        rows[1].grant_target_generation = 8;
        count = 2;
      } else
        assert(request.kind == SOPHIA_BF_SURFACE_REMOVED);
      assert(!sophia_br_respond(&s, request.id, rows, count, time));
      sent = 1;
    }
    if (sent && sophia_br_transaction_ready(&s))
      break;
    sched_yield();
  }
  sophia_br_close(&s);
  close(fd);
  free(storage);
  return 0;
}
