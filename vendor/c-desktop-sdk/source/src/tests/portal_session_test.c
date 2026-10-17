#include "sophia_portal_session.h"
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
/* Cases 5/6 forbid watch data; 7 cancels before dispatching the walk, 8 while
 * open is held by the peer, and 9 after status and revocation arrive together. */
static void peer(int fd, int barrier, unsigned fault) {
  unsigned char h[7], b[4096], r[4096];
  char names[16][32] = {{0}};
  unsigned old = 65536, flushes = 0;
  unsigned versions = 0, attaches = 0, watches = 0, clunks = 0;
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2);
    unsigned fid;
    const char *name;
    assert(n <= sizeof b);
    if (!exact(fd, b, n)) {
      assert(fault >= 2);
      break;
    }
    fid = n >= 4 ? (unsigned)get(b, 4) : 0;
    name = fid < 16 ? names[fid] : "";
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
      assert(get(b + 8, 2) == 1 && get(b + 10, 2) < 32);
      fid = (unsigned)get(b + 4, 4);
      assert(fid < 16);
      memset(names[fid], 0, 32);
      memcpy(names[fid], b + 12, (size_t)get(b + 10, 2));
      name = names[fid];
      assert(!strcmp(name, "api") || !strcmp(name, "watch") ||
             !strcmp(name, "status"));
      if (!strcmp(name, "watch"))
        ++watches;
      r[0] = 1;
      send_reply(fd, 111, tag, r, 15);
      break;
    case 12:
      assert(fault != 7 || strcmp(name, "watch"));
      assert(!get(b + 4, 4));
      if (fault == 8 && !strcmp(name, "watch")) {
        unsigned char signal = 1;
        assert(write(barrier, &signal, 1) == 1 && exact(barrier, &signal, 1));
      }
      send_reply(fd, 13, tag, r, 17);
      break;
    case 116:
      if (strcmp(name, "watch")) {
        const char *api = !strcmp(name, "api")
                              ? "sophia-portal-client version=1 epoch=7 "
                                "request=unavailable fd_transfer=none\n"
                              : "request=unavailable transfer=ready\n";
        size_t offset = (size_t)get(b + 4, 8), count = (size_t)get(b + 12, 4),
               length = strlen(api);
        assert(offset <= length);
        if (count > length - offset)
          count = length - offset;
        put(r, count, 4);
        memcpy(r + 4, api + offset, count);
        if (fault == 9 && !strcmp(name, "status") && !offset) {
          unsigned char batch[128], signal = 1;
          size_t first = count + 11;
          assert(write(barrier, &signal, 1) == 1 && exact(barrier, &signal, 1));
          put(batch, first, 4);
          batch[4] = 117;
          put(batch + 5, tag, 2);
          memcpy(batch + 7, r, count + 4);
          put(batch + first, 11, 4);
          batch[first + 4] = 7;
          put(batch + first + 5, old, 2);
          put(batch + first + 7, 116, 4);
          assert(write(fd, batch, first + 11) == (ssize_t)(first + 11));
          assert(write(barrier, &signal, 1) == 1);
        } else
          send_reply(fd, 117, tag, r, count + 4);
      } else {
        assert(fault != 7 && fault != 8);
        assert(!get(b + 4, 8) && get(b + 12, 4) == 1);
        assert(old == 65536);
        old = tag;
      }
      break;
    case 108:
      ++flushes;
      assert(n == 2 && get(b, 2) == old);
      if (fault == 1 || fault == 2) {
        put(r, 116, 4);
        send_reply(fd, 7, old, r, 4);
      }
      if (fault == 5 || fault == 6) {
        put(r, fault == 6, 4);
        r[4] = 42;
        send_reply(fd, 117, old, r, fault == 6 ? 5 : 4);
        /* No Rflush: malformed data must fail without waiting for cleanup. */
        break;
      }
      if (fault == 3) {
        put(r, 5, 4);
        send_reply(fd, 7, tag, r, 4);
      } else if (fault == 4)
        goto finished;
      else
        send_reply(fd, 109, tag, NULL, 0);
      break;
    case 120:
      if (!strcmp(name, "watch"))
        ++clunks;
      if (fault == 2 && !strcmp(name, "watch")) {
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
  assert(watches == 1 && flushes == (unsigned)(fault < 7));
  assert(clunks == (unsigned)(fault < 3 || fault >= 7));
  close(barrier);
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(20);
  for (fault = 0; fault < 10; ++fault) {
    int pair[2], control[2], status;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle handle;
    struct sophia_9p_reply reply;
    struct sophia_po opening;
    struct sophia_pcs session;
    enum sophia_pcs_event event;
    uint32_t remote;
    unsigned char snapshot[101];
    const void *result;
    size_t length;
    uint32_t root;
    size_t bytes = sophia_9p_storage_bytes(4096, 4);
    void *storage = malloc(bytes);
    assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, control));
    child = fork();
    assert(child >= 0);
    if (!child) {
      close(pair[0]);
      close(control[0]);
      peer(pair[1], control[1], fault);
    }
    close(pair[1]);
    close(control[1]);
    assert(!sophia_9p_init(&wire, pair[0], 4096, 4, 4, storage, bytes));
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
    assert(!sophia_pcs_init(&session, &opening));
    assert(!sophia_pcs_watch(&session, 100));
    while (fault != 7 && fault != 8 && !sophia_pcs_watch_waiting(&session)) {
      assert(!sophia_pcs_dispatch(&session, 10, 1));
      sched_yield();
    }
    assert(!sophia_pcs_snapshot(&session, SOPHIA_PS_STATUS, snapshot,
                                sizeof snapshot, 100));
    if (fault == 8 || fault == 9) {
      unsigned char signal;
      for (;;) {
        ssize_t n = recv(control[0], &signal, 1, MSG_DONTWAIT);
        if (n == 1)
          break;
        assert(n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK));
        assert(!sophia_pcs_dispatch(&session, 10, 1));
        sched_yield();
      }
      if (fault == 9) {
        assert(write(control[0], &signal, 1) == 1 &&
               exact(control[0], &signal, 1));
        /* C routes both decoded replies in this dispatch; cancellation must
         * preserve the revocation without flushing an already settled tag. */
        assert(!sophia_pcs_dispatch(
            &session, 10, strlen("request=unavailable transfer=ready\n") + 22));
      }
    }
    assert(!sophia_pcs_cancel_watch(&session));
    if (fault == 8) {
      unsigned char signal = 1;
      assert(write(control[0], &signal, 1) == 1);
    }
    assert(sophia_pcs_dispatch(&session, 0, 4096) ==
               SOPHIA_PS_CLOCK_REGRESSION &&
           !session.error);
    while (!session.error &&
           (!sophia_pcs_watch_event(&session, &event, &remote) ||
            !sophia_pcs_snapshot_result(&session, &result, &length))) {
      (void)sophia_pcs_dispatch(&session, 10, 1);
      sched_yield();
    }
    if (fault < 2 || fault >= 7) {
      const char *expected = "request=unavailable transfer=ready\n";
      assert(event == (fault == 1 || fault == 9 ? SOPHIA_PCS_REVOKED
                                                : SOPHIA_PCS_CANCELLED));
      assert(length == strlen(expected) && !memcmp(result, expected, length));
      assert(!sophia_pcs_release_watch(&session));
      assert(!sophia_pcs_release_snapshot(&session));
      assert(!sophia_pcs_finish(&session));
      assert(sophia_po_ready(&opening) && wire.fid_count == 1);
    } else {
      assert(session.error);
      if (fault == 5 || fault == 6)
        assert(session.error == SOPHIA_PS_INVALID);
      assert(!sophia_pcs_watch_event(&session, &event, &remote));
      assert(!sophia_pcs_snapshot_result(&session, &result, &length));
      assert(sophia_pcs_finish(&session) == session.error);
    }
    close(pair[0]);
    close(control[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           WEXITSTATUS(status) == 0);
  }
  return 0;
}
