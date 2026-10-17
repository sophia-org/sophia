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
  unsigned files[128] = {0}, metadata_reads = 0;
  unsigned old = 65536, flushes = 0, versions = 0, attaches = 0;
  unsigned tags[4], held = 0;
  unsigned char bodies[4][32];
  size_t lengths[4];
  int reversed = 0;
  unsigned char h[7], b[4096], r[4096], record[64];
  const uint64_t fields[8] = {1, 7, 9, 11, 20, 10, 80, 1};
  size_t i;
  for (i = 0; i < 8; ++i)
    put(record + i * 8, fields[i], 8);
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2), fid;
    assert(n <= sizeof b && exact(fd, b, n));
    fid = n >= 4 ? (unsigned)get(b, 4) : 128;
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
    case 110: {
      unsigned newfid = (unsigned)get(b + 4, 4);
      size_t len = (size_t)get(b + 10, 2);
      assert(newfid < 128 && get(b + 8, 2) == 1);
      if (len == 3 && !memcmp(b + 12, "api", 3))
        files[newfid] = 1;
      else if (len == 10 && !memcmp(b + 12, "frame-info", 10))
        files[newfid] = 2;
      else if (len == 5 && !memcmp(b + 12, "watch", 5))
        files[newfid] = 4;
      else {
        assert(len == 5 && !memcmp(b + 12, "frame", 5));
        files[newfid] = 3;
      }
      r[0] = 1;
      send_reply(fd, 111, tag, r, 15);
      break;
    }
    case 12:
      assert(!get(b + 4, 4));
      put(r + 13, 128, 4);
      send_reply(fd, 13, tag, r, 17);
      break;
    case 108:
      assert(n == 2 && get(b, 2) == old);
      old = 65536;
      ++flushes;
      send_reply(fd, 109, tag, NULL, 0);
      break;
    case 116: {
      size_t offset = (size_t)get(b + 4, 8), count = (size_t)get(b + 12, 4),
             length;
      const unsigned char *data;
      const char *api = "sophia-portal-client version=1 epoch=7 "
                        "request=unavailable fd_transfer=none\n";
      assert(fid < 128 && count <= 128);
      if (files[fid] == 4) {
        assert(!offset && count == 1 && old == 65536);
        old = tag;
        break;
      }
      if (files[fid] == 1) {
        data = (const unsigned char *)api;
        length = strlen(api);
      } else if (files[fid] == 2) {
        if (!offset)
          ++metadata_reads;
        if (fault == 1 && metadata_reads == 2) {
          put(r, 116, 4);
          send_reply(fd, 7, tag, r, 4);
          break;
        }
        if (fault == 2 && metadata_reads == 2)
          record[24] = 12;
        data = record;
        length = 64;
      } else {
        assert(files[fid] == 3);
        length = fault == 3 ? 801 : 800;
        assert(offset <= length);
        if (count > length - offset)
          count = length - offset;
        if (count > 17)
          count = 17;
        if (fault == 4 && !offset)
          count = 0;
        put(r, count, 4);
        memset(r + 4, 17, count);
        if (!fault && !reversed) {
          assert(held < 4 && count + 4 <= 32);
          tags[held] = tag;
          lengths[held] = count + 4;
          memcpy(bodies[held], r, count + 4);
          if (++held == 4) {
            while (held) {
              --held;
              send_reply(fd, 117, tags[held], bodies[held], lengths[held]);
            }
            reversed = 1;
          }
        } else
          send_reply(fd, 117, tag, r, count + 4);
        if (fault == 4 && !offset) {
          close(fd);
          _exit(0);
        }
        break;
      }
      assert(offset <= length);
      if (count > length - offset)
        count = length - offset;
      put(r, count, 4);
      memcpy(r + 4, data + offset, count);
      send_reply(fd, 117, tag, r, count + 4);
      break;
    }
    case 120:
      if (fault == 5 && files[fid] == 3) {
        put(r, 5, 4);
        send_reply(fd, 7, tag, r, 4);
      } else
        send_reply(fd, 121, tag, NULL, 0);
      break;
    default:
      assert(0);
    }
  }
  if (!fault)
    assert(reversed && flushes == 2);
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(20);
  for (fault = 0; fault <= 6; ++fault) {
    int pair[2], status;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle handle;
    struct sophia_9p_reply reply;
    struct sophia_po opening;
    struct sophia_pcs session;
    enum sophia_pcs_event event;
    uint32_t remote;
    unsigned iteration;
    unsigned char snapshot[43];
    struct sophia_pf_frame_info info;
    uint32_t root;
    unsigned char image[800];
    const void *pixels;
    size_t length, i;
    size_t size = sophia_9p_storage_bytes(4096, 6);
    void *storage = malloc(size);
    assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    child = fork();
    assert(child >= 0);
    if (!child) {
      close(pair[0]);
      peer(pair[1], fault);
    }
    close(pair[1]);
    assert(!sophia_9p_init(&wire, pair[0], 4096, 6, 4, storage, size));
    assert(!sophia_9p_version(&wire, &handle));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
    assert(!sophia_9p_attach(&wire, "", "", &handle, &root));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
    assert(!sophia_po_init(&opening, &wire, root, 0, 2000));
    while (!sophia_po_ready(&opening)) {
      assert(!sophia_po_dispatch(&opening, 1, 4096));
      sched_yield();
    }
    assert(!sophia_pcs_init(&session, &opening));
    memset(image, 0xa5, sizeof image);
    for (iteration = 0; iteration < (fault ? 1u : 2u); ++iteration) {
      int cancelled = 0;
      assert(!sophia_pcs_watch(&session, 2000));
      while (!sophia_pcs_watch_waiting(&session)) {
        assert(!sophia_pcs_dispatch(&session, 1, 1));
        sched_yield();
      }
      assert(sophia_pcs_frame(&session, &session, sizeof session, 2000) ==
             SOPHIA_PS_INVALID);
      assert(sophia_pcs_frame(&session, image, sizeof image, 1) ==
             SOPHIA_PFR_DEADLINE);
      for (i = 0; i < sizeof image; ++i)
        assert(image[i] == (iteration ? 17 : 0xa5));
      assert(!sophia_pcs_frame(&session, image, fault == 6 ? 799 : 800, 2000));
      assert(sophia_pcs_frame(&session, image, sizeof image, 2000) ==
             SOPHIA_PS_BUSY);
      for (i = 0; i < sizeof image; ++i)
        assert(image[i] == (iteration ? 17 : 0xa5));
      assert(sophia_pcs_snapshot(&session, SOPHIA_PS_STATUS, snapshot,
                                 sizeof snapshot, 2000) == SOPHIA_PS_BUSY);
      while (!session.error &&
             (!sophia_pcs_frame_result(&session, &info, &pixels, &length) ||
              !sophia_pcs_watch_event(&session, &event, &remote))) {
        (void)sophia_pcs_dispatch(&session, 1, fault ? 65536 : 1);
        if (!fault && !cancelled && sophia_pcs_frame_received(&session)) {
          assert(sophia_pcs_frame_received(&session) < 800);
          assert(!sophia_pcs_frame_result(&session, &info, &pixels, &length));
          assert(!sophia_pcs_cancel_watch(&session));
          cancelled = 1;
        }
        sched_yield();
      }
      if (!fault) {
        assert(cancelled && event == SOPHIA_PCS_CANCELLED);
        assert(pixels == image && length == 800 && info.image_bytes == 800);
        for (i = 0; i < length; ++i)
          assert(image[i] == 17);
        assert(!sophia_pcs_release_frame(&session));
        assert(!sophia_pcs_release_watch(&session));
      } else {
        assert(!sophia_pcs_frame_result(&session, &info, &pixels, &length));
        assert(!sophia_pcs_watch_event(&session, &event, &remote));
        if (fault == 6)
          assert(session.error == SOPHIA_PFR_CAPACITY);
      }
    }
    if (!fault) {
      assert(!sophia_pcs_finish(&session));
      assert(sophia_po_ready(&opening) && wire.fid_count == 1);
    } else
      assert(sophia_pcs_finish(&session));
    close(pair[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           !WEXITSTATUS(status));
  }
  return 0;
}
