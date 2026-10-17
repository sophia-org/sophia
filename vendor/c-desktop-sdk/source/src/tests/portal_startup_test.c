#include "sophia_portal_startup.h"
#include <assert.h>
#include <errno.h>
#include <sched.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static uint64_t get(const unsigned char *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static void put(unsigned char *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (unsigned char)(v >> (i * 8));
}
static int exact(int fd, void *buffer, size_t n) {
  unsigned char *p = buffer;
  size_t used = 0;
  while (used < n) {
    ssize_t r = read(fd, p + used, n - used);
    if (r < 0 && errno == EINTR)
      continue;
    if (!r) {
      assert(!used);
      return 0;
    }
    assert(r > 0);
    used += (size_t)r;
  }
  return 1;
}
static void send_reply(int fd, unsigned type, unsigned tag, const void *body,
                       size_t n) {
  unsigned char out[4096];
  size_t used = 0;
  put(out, n + 7, 4);
  out[4] = (unsigned char)type;
  put(out + 5, tag, 2);
  if (n)
    memcpy(out + 7, body, n);
  while (used < n + 7) {
    ssize_t r = write(fd, out + used, n + 7 - used);
    assert(r > 0);
    used += (size_t)r;
  }
}
static void peer(int fd, unsigned fault) {
  unsigned char h[7], b[4096], response[4096];
  unsigned versions = 0, attaches = 0, clunks = 0, walks = 0;
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2);
    assert(n <= sizeof b && exact(fd, b, n));
    memset(response, 0, sizeof response);
    switch (h[4]) {
    case 100:
      assert(++versions == 1);
      send_reply(fd, 101, tag, b, n);
      break;
    case 104:
      assert(++attaches == 1);
      response[0] = 128;
      send_reply(fd, 105, tag, response, 13);
      break;
    case 110:
      assert(get(b + 8, 2) == 1);
      ++walks;
      assert(get(b + 10, 2) == 3 && !memcmp(b + 12, "api", 3) && walks == 1);
      response[0] = 1;
      response[2] = fault == 2 ? 128 : 0;
      send_reply(fd, 111, tag, response, 15);
      break;
    case 12:
      assert(get(b + 4, 4) == 0);
      response[0] = fault == 3 ? 128 : 0;
      put(response + 13, 7, 4);
      send_reply(fd, 13, tag, response, 17);
      break;
    case 116: {
      const char *info = fault == 1 ? "sophia-portal-client version=2 epoch=7 "
                                      "request=unavailable fd_transfer=none\n"
                                    : "sophia-portal-client version=1 epoch=7 "
                                      "request=unavailable fd_transfer=none\n";
      size_t offset = (size_t)get(b + 4, 8), count = (size_t)get(b + 12, 4),
             length = strlen(info);
      assert(count <= 7 && offset <= length);
      if (count > length - offset)
        count = length - offset;
      put(response, count, 4);
      memcpy(response + 4, info + offset, count);
      send_reply(fd, 117, tag, response, count + 4);
      break;
    }
    case 120:
      ++clunks;
      if (fault == 4) {
        put(response, 5, 4);
        send_reply(fd, 7, tag, response, 4);
      } else
        send_reply(fd, 121, tag, NULL, 0);
      break;
    default:
      assert(0);
    }
  }
  if (!fault)
    assert(versions == 1 && attaches == 1 && walks == 1 && clunks == 1);
  if (fault == 5)
    assert(versions == 1 && attaches == 1 && !walks);
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(15);
  for (fault = 0; fault <= 5; ++fault) {
    int pair[2], status;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle version;
    uint32_t root;
    struct sophia_9p_reply reply;
    struct sophia_po opening;
    size_t bytes = sophia_9p_storage_bytes(4096, 2);
    void *storage = malloc(bytes);
    assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    child = fork();
    assert(child >= 0);
    if (!child) {
      close(pair[0]);
      peer(pair[1], fault);
    }
    close(pair[1]);
    assert(!sophia_9p_init(&wire, pair[0], 4096, 2, 4, storage, bytes));
    assert(!sophia_9p_version(&wire, &version));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
    /* Scripted attach stands in for completed authentication. */
    assert(!sophia_9p_attach(&wire, "", "", &version, &root));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
    assert(sophia_po_init(&opening, &wire, SOPHIA_9P_NOFID, 10, 100) ==
           SOPHIA_PS_INVALID);
    assert(!sophia_po_init(&opening, &wire, root, 10, 100));
    assert(sophia_po_wants_write(&opening));
    assert(sophia_po_dispatch(&opening, 9, 4096) ==
               SOPHIA_PS_CLOCK_REGRESSION &&
           !opening.error);
    if (fault == 5)
      assert(sophia_po_dispatch(&opening, 100, 4096) == SOPHIA_PS_DEADLINE);
    else {
      while (!sophia_po_ready(&opening) && !opening.error) {
        (void)sophia_po_dispatch(&opening, 11, fault ? 4096 : 1);
        sched_yield();
      }
      if (!fault) {
        assert(sophia_po_ready(&opening) && opening.info.epoch == 7 &&
               wire.fid_count == 1);
        assert(!sophia_po_dispatch(&opening, 1000, 4096) &&
               !sophia_po_wants_write(&opening));
      } else
        assert(opening.error && !sophia_po_ready(&opening));
    }
    sophia_po_close(&opening);
    close(pair[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           WEXITSTATUS(status) == 0);
  }
  return 0;
}
