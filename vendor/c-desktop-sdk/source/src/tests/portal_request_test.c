#include "sophia_portal_request.h"
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
  char name[32] = {0};
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
      assert(!strcmp(name, "api") || !strcmp(name, "capture") ||
             !strcmp(name, "capture-status"));
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
    case 116: {
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
    assert(writes == 2 && clunks == 7);
  else
    assert(writes == (unsigned)(fault != 4 && fault != 7 && fault < 8));
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(20);
  for (fault = 0; fault <= 9; ++fault) {
    int pair[2], status;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle handle;
    struct sophia_9p_reply reply;
    struct sophia_po opening;
    struct sophia_pcr request;
    struct sophia_pf_capture_request capture = {9, 7};
    enum sophia_pf_capture_state outcome;
    uint32_t root;
    uint64_t now = 1, last = 8, wake;
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
    assert(sophia_pcr_init(&request, &opening, capture, 9, now, 100) ==
           SOPHIA_PCR_INVALID);
    assert(sophia_po_ready(&opening));
    opening.info.request = SOPHIA_PF_REQUEST_UNAVAILABLE;
    assert(sophia_pcr_init(&request, &opening, capture, 8, now, 100) ==
           SOPHIA_PCR_UNSUPPORTED);
    opening.info.request = SOPHIA_PF_OUTPUT_CAPTURE_V1;
    assert(sophia_pcr_init(&request, &opening, capture, 8, now, now) ==
           SOPHIA_PCR_CAPTURE);
    assert(sophia_pcr_init(&request, &opening, capture, 8, 0, 100) ==
           SOPHIA_PCR_CLOCK);
    assert(sophia_po_ready(&opening));
    do {
      assert(!sophia_pcr_init(&request, &opening, capture, last, now, 100));
      assert(!sophia_po_ready(&opening));
      assert(sophia_pcr_dispatch(&request, now - 1, 4096) == SOPHIA_PCR_CLOCK &&
             !request.error);
      if (fault >= 8) {
        if (fault == 8)
          assert(sophia_pcr_dispatch(&request, 100, 4096));
        else
          sophia_pcr_close(&request);
        assert(request.capture.custody == SOPHIA_PC_DROPPED_UNSENT);
        break;
      }
      while (!request.error && !sophia_pcr_result(&request, &outcome)) {
        if (sophia_pcr_next_wake(&request, &wake) && wake < 100 && wake > now)
          now = wake;
        (void)sophia_pcr_dispatch(&request, now, fault ? 4096 : 1);
        sched_yield();
      }
      if (fault) {
        assert(request.error && !sophia_pcr_result(&request, &outcome));
        assert(request.capture.custody == (fault == 1 ? SOPHIA_PC_REFUSED
                                           : fault == 4 || fault == 7
                                               ? SOPHIA_PC_DROPPED_UNSENT
                                               : SOPHIA_PC_UNKNOWN));
        assert(sophia_pcr_dispatch(&request, now, 4096));
        break;
      }
      assert(request.capture.custody == SOPHIA_PC_SETTLED &&
             outcome == SOPHIA_PF_READY);
      assert(!sophia_pcr_next_wake(&request, &wake));
      assert(!sophia_pcr_finish(&request, &outcome, &last) &&
             last == capture.serial);
      assert(sophia_po_ready(&opening) && opening.now == now &&
             wire.fid_count == 1);
      ++capture.serial;
    } while (capture.serial <= 10);
    close(pair[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           WEXITSTATUS(status) == 0);
  }
  return 0;
}
