#include "sophia_portal_watch.h"
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
static int exact(int fd, void *p, size_t n) {
  size_t used = 0;
  while (used < n) {
    ssize_t r = read(fd, (unsigned char *)p + used, n - used);
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
static void send_reply(int fd, unsigned type, unsigned tag, const void *p,
                       size_t n) {
  unsigned char b[4096];
  size_t used = 0;
  put(b, n + 7, 4);
  b[4] = (unsigned char)type;
  put(b + 5, tag, 2);
  if (n)
    memcpy(b + 7, p, n);
  while (used < n + 7) {
    ssize_t r = write(fd, b + used, n + 7 - used);
    assert(r > 0);
    used += (size_t)r;
  }
}
static void peer(int fd, unsigned fault) {
  unsigned char h[7], b[4096], r[4096];
  char name[32] = {0};
  unsigned versions = 0, attaches = 0, watches = 0, clunks = 0;
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2);
    assert(n <= sizeof b && exact(fd, b, n));
    memset(r, 0, sizeof r);
    switch (h[4]) {
    case 100:
      assert(++versions == 1);
      send_reply(fd, 101, tag, b, n);
      break;
    case 104:
      assert(++attaches == 1);
      r[0] = 128;
      send_reply(fd, 105, tag, r, 13);
      break;
    case 110:
      assert(get(b + 8, 2) == 1 && get(b + 10, 2) < sizeof name);
      memset(name, 0, sizeof name);
      memcpy(name, b + 12, (size_t)get(b + 10, 2));
      assert(!strcmp(name, "api") || !strcmp(name, "watch"));
      if (!strcmp(name, "watch"))
        ++watches;
      r[0] = 1;
      if (fault == 7 && !strcmp(name, "watch"))
        r[2] = 128;
      send_reply(fd, 111, tag, r, 15);
      break;
    case 12:
      assert(!get(b + 4, 4));
      if (fault == 6 && !strcmp(name, "watch")) {
        put(r, 116, 4);
        send_reply(fd, 7, tag, r, 4);
      } else
        send_reply(fd, 13, tag, r, 17);
      break;
    case 116:
      if (!strcmp(name, "api")) {
        const char *api = "sophia-portal-client version=1 epoch=7 "
                          "request=unavailable fd_transfer=none\n";
        size_t offset = (size_t)get(b + 4, 8), count = (size_t)get(b + 12, 4),
               length = strlen(api);
        assert(offset <= length);
        if (count > length - offset)
          count = length - offset;
        put(r, count, 4);
        memcpy(r + 4, api + offset, count);
        send_reply(fd, 117, tag, r, count + 4);
      } else {
        assert(!get(b + 4, 8) && get(b + 12, 4) == 1);
        if (fault == 5)
          goto finished;
        if (fault == 1 || fault == 2) {
          put(r, fault == 2, 4);
          r[4] = 42;
          send_reply(fd, 117, tag, r, fault == 2 ? 5 : 4);
        } else {
          put(r, fault == 3 ? 5 : 116, 4);
          send_reply(fd, 7, tag, r, 4);
        }
      }
      break;
    case 120:
      if (!strcmp(name, "watch"))
        ++clunks;
      if (fault == 4 && !strcmp(name, "watch")) {
        put(r, 5, 4);
        send_reply(fd, 7, tag, r, 4);
      } else
        send_reply(fd, 121, tag, NULL, 0);
      break;
    default:
      assert(0);
    }
  }
finished:
  assert(watches == ((!fault || fault == 6) ? 2u : 1u));
  assert(clunks == ((!fault || fault == 6) ? 2u : (unsigned)(fault == 4)));
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(20);
  for (fault = 0; fault <= 7; ++fault) {
    int pair[2], status;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle handle;
    struct sophia_9p_reply reply;
    struct sophia_po opening;
    struct sophia_pw watch;
    uint32_t root;
    unsigned i;
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
    assert(!sophia_9p_version(&wire, &handle));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
    /* Supplied admission; production authentication has a separate gate. */
    assert(!sophia_9p_attach(&wire, "", "", &handle, &root));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
    assert(!sophia_po_init(&opening, &wire, root, 0, 100));
    while (!sophia_po_ready(&opening)) {
      assert(!sophia_po_dispatch(&opening, 1, 4096));
      sched_yield();
    }
    for (i = 0; i < ((!fault || fault == 6) ? 2u : 1u); ++i) {
      assert(!sophia_pw_init(&watch, &opening, 10, 100));
      assert(sophia_pw_dispatch(&watch, 9, 4096) ==
                 SOPHIA_PS_CLOCK_REGRESSION &&
             !watch.error);
      while (!watch.error && !sophia_pw_revoked(&watch)) {
        (void)sophia_pw_dispatch(&watch, 10, (!fault || fault == 6) ? 1 : 4096);
        sched_yield();
      }
      if (!fault || fault == 6) {
        assert(sophia_pw_revoked(&watch));
        assert(!sophia_pw_finish(&watch));
        assert(sophia_po_ready(&opening) && wire.fid_count == 1);
      } else {
        assert(!sophia_pw_revoked(&watch));
        assert(watch.error == (fault == 3 || fault == 4 ? SOPHIA_PS_REMOTE
                               : fault == 5             ? SOPHIA_PO_WIRE_ERROR
                                                        : SOPHIA_PS_INVALID));
        if (fault == 3 || fault == 4)
          assert(watch.remote_error == 5);
        assert(sophia_pw_finish(&watch) == SOPHIA_PS_INVALID);
      }
    }
    close(pair[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           WEXITSTATUS(status) == 0);
  }
  return 0;
}
