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
  unsigned char h[7], b[4096], r[4096], record[32];
  unsigned versions = 0, attaches = 0, writes = 0, snapshots = 0, clunks = 0;
  uint64_t serial = 0;
  char names[16][32] = {{0}};
  unsigned old = 65536, flushes = 0;
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2);
    assert(n <= sizeof b && exact(fd, b, n));
    unsigned fid = n >= 4 ? (unsigned)get(b, 4) : 16;
    const char *name = fid < 16 ? names[fid] : "";
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
      assert(!strcmp(name, "api") || !strcmp(name, "capture") ||
             !strcmp(name, "capture-status") || !strcmp(name, "watch"));
      if (!strcmp(name, "capture-status"))
        ++snapshots;
      r[0] = 1;
      if (fault == 7 && !strcmp(name, "capture"))
        r[2] = 128;
      send_reply(fd, 111, tag, r, 15);
      break;
    case 12:
      assert(get(b + 4, 4) == (uint64_t)!strcmp(name, "capture"));
      put(r + 13, !strcmp(name, "capture") ? (fault == 4 ? 23 : 24) : 7, 4);
      send_reply(fd, 13, tag, r, 17);
      break;
    case 118:
      assert(!strcmp(name, "capture") && n == 40 && !get(b + 4, 8) &&
             get(b + 12, 4) == 24);
      assert(get(b + 16, 8) == 1 && get(b + 32, 8) == 7);
      serial = get(b + 24, 8);
      assert(serial == 8 + ++writes);
      snapshots = 0;
      if (fault == 6)
        goto finished;
      put(r, fault == 1 ? 11 : fault == 2 ? 23 : 24, 4);
      send_reply(fd, fault == 1 ? 7 : 119, tag, r, 4);
      break;
    case 108:
      assert(n == 2 && get(b, 2) == old);
      old = 65536;
      ++flushes;
      send_reply(fd, 109, tag, NULL, 0);
      break;
    case 116: {
      if (!strcmp(name, "watch")) {
        assert(old == 65536 && !get(b + 4, 8) && get(b + 12, 4) == 1);
        old = tag;
        break;
      }
      const char *api = "sophia-portal-client version=1 epoch=7 "
                        "request=output-capture-v1 fd_transfer=none\n";
      const void *data;
      size_t length, offset = (size_t)get(b + 4, 8),
                     count = (size_t)get(b + 12, 4);
      if (!strcmp(name, "api")) {
        data = api;
        length = strlen(api);
      } else {
        assert(!strcmp(name, "capture-status"));
        put(record, 1, 8);
        put(record + 8, serial + (fault == 3), 8);
        put(record + 16, 7, 8);
        put(record + 24, snapshots == 1 ? 1 : 3, 8);
        data = record;
        length = sizeof record;
      }
      assert(count <= 7 && offset <= length);
      if (count > length - offset)
        count = length - offset;
      put(r, count, 4);
      memcpy(r + 4, (const unsigned char *)data + offset, count);
      send_reply(fd, 117, tag, r, count + 4);
      break;
    }
    case 120:
      ++clunks;
      if (fault == 5 && !strcmp(name, "capture-status") && snapshots == 2) {
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
  if (!fault)
    assert(writes == 2 && clunks == 9 && flushes == 2);
  else
    assert(writes == (unsigned)(fault != 4 && fault != 7 && fault < 8));
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(20);
  for (fault = 0; fault < 8; ++fault) {
    int pair[2], status;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle handle;
    struct sophia_9p_reply reply;
    struct sophia_po opening;
    struct sophia_pcs session;
    enum sophia_pcs_event event;
    enum sophia_pc_custody custody;
    uint32_t remote;
    uint64_t result_serial;
    unsigned char snapshot[101];
    struct sophia_pf_capture_request capture = {9, 7};
    enum sophia_pf_capture_state outcome;
    uint32_t root;
    uint64_t now = 1, last = 8, wake;
    size_t bytes = sophia_9p_storage_bytes(4096, 4);
    void *storage = malloc(bytes);
    assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    child = fork();
    assert(child >= 0);
    if (!child) {
      close(pair[0]);
      peer(pair[1], fault);
    }
    close(pair[1]);
    assert(!sophia_9p_init(&wire, pair[0], 4096, 4, 4, storage, bytes));
    assert(!sophia_9p_version(&wire, &handle));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
    /* Stand-in for a completed authenticated attach; no production claim. */
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
    do {
      int cancelled = 0;
      assert(!sophia_pcs_watch(&session, 100));
      while (!sophia_pcs_watch_waiting(&session)) {
        assert(!sophia_pcs_dispatch(&session, now, 1));
        sched_yield();
      }
      assert(sophia_pcs_capture(&session, capture, capture.serial, 100) ==
             SOPHIA_PCR_INVALID);
      assert(!session.error);
      assert(!sophia_pcs_capture(&session, capture, last, 100));
      assert(sophia_pcs_capture(&session, capture, last, 100) ==
             SOPHIA_PS_BUSY);
      assert(sophia_pcs_snapshot(&session, SOPHIA_PS_STATUS, snapshot,
                                 sizeof snapshot, 100) == SOPHIA_PS_BUSY);
      while (!session.error &&
             (!sophia_pcs_capture_result(&session, &outcome, &result_serial) ||
              !sophia_pcs_watch_event(&session, &event, &remote))) {
        assert(sophia_pcs_capture_custody(&session, &custody));
        if (!cancelled && custody == SOPHIA_PC_ACCEPTED) {
          assert(!sophia_pcs_cancel_watch(&session));
          cancelled = 1;
        }
        if (sophia_pcs_deadline(&session, &wake) && wake < 100 && wake > now)
          now = wake;
        (void)sophia_pcs_dispatch(&session, now, 1);
        sched_yield();
      }
      assert(sophia_pcs_capture_custody(&session, &custody));
      if (fault) {
        assert(session.error &&
               !sophia_pcs_capture_result(&session, &outcome, &result_serial));
        assert(!sophia_pcs_watch_event(&session, &event, &remote));
        assert(custody == (fault == 1                 ? SOPHIA_PC_REFUSED
                           : fault == 4 || fault == 7 ? SOPHIA_PC_DROPPED_UNSENT
                                                      : SOPHIA_PC_UNKNOWN));
        break;
      }
      assert(cancelled && custody == SOPHIA_PC_SETTLED &&
             outcome == SOPHIA_PF_READY);
      assert(result_serial == capture.serial && event == SOPHIA_PCS_CANCELLED);
      assert(!sophia_pcs_deadline(&session, &wake));
      assert(!sophia_pcs_release_capture(&session));
      assert(!sophia_pcs_release_watch(&session));
      last = result_serial;
      ++capture.serial;
    } while (capture.serial <= 10);
    if (!fault) {
      assert(!sophia_pcs_finish(&session));
      assert(sophia_po_ready(&opening) && opening.now == now &&
             wire.fid_count == 1);
    } else
      assert(sophia_pcs_finish(&session));
    close(pair[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           WEXITSTATUS(status) == 0);
  }
  return 0;
}
