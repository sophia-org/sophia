#define _POSIX_C_SOURCE 200809L
#include "sophia_admin_connecting.h"
#include <assert.h>
#include <errno.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static uint64_t now_ms(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static uint64_t get(const unsigned char *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (8 * i);
  return v;
}
static void put(unsigned char *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (unsigned char)(v >> (8 * i));
}
static int exact(int fd, void *data, size_t n) {
  size_t used = 0;
  while (used < n) {
    ssize_t r = read(fd, (char *)data + used, n - used);
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
  unsigned char b[4096];
  size_t used = 0;
  put(b, n + 7, 4);
  b[4] = (unsigned char)type;
  put(b + 5, tag, 2);
  if (n)
    memcpy(b + 7, body, n);
  while (used < n + 7) {
    ssize_t r = write(fd, b + used, n + 7 - used);
    assert(r > 0);
    used += (size_t)r;
  }
}
static void peer(int fd, unsigned fault) {
  unsigned files[256] = {0}, versions = 0, attaches = 0, writes = 0,
           catalogs = 0, polls = 0;
  char objects[256][512], expected[128];
  unsigned char h[7], b[4096], r[4096];
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2), fid;
    assert(n <= sizeof b && exact(fd, b, n));
    fid = (unsigned)get(b, 4);
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
      unsigned to = (unsigned)get(b + 4, 4);
      size_t length = (size_t)get(b + 10, 2);
      assert(to < 256 && get(b + 8, 2) == 1);
      if (length == 4 && !memcmp(b + 12, "info", 4))
        files[to] = 1;
      else if (length == 3 && !memcmp(b + 12, "ctl", 3))
        files[to] = 2;
      else if (length == 8 && !memcmp(b + 12, "commands", 8))
        files[to] = 3;
      else {
        assert(length == 6 && !memcmp(b + 12, "status", 6));
        files[to] = 4;
      }
      r[0] = 1;
      send_reply(fd, 111, tag, r, 15);
      break;
    }
    case 12:
      assert(fid < 256 && get(b + 4, 4) == (files[fid] == 2));
      switch (files[fid]) {
      case 1:
        strcpy(objects[fid], "v1 info 1 0 7 10000 2000 60000\n");
        break;
      case 2:
        objects[fid][0] = 0;
        break;
      case 3:
        ++catalogs;
        strcpy(objects[fid], "v1 catalog 12 2\nv1 command policy 8:next "
                             "tab\nv1 command session 14:reload-profile\n");
        break;
      case 4:
        assert(writes);
        ++polls;
        if (polls == 1)
          (void)snprintf(objects[fid], 512, "v1 pending 7 %u 12\n", writes);
        else
          (void)snprintf(objects[fid], 512, "v1 outcome 7 %u %u %s %u:ok\n",
                         writes, fault == 3 ? 13 : 12,
                         fault == 6    ? "completed"
                         : writes == 2 ? "unchanged"
                                       : "committed",
                         fault == 5 ? 3 : 2);
        break;
      default:
        assert(0);
      }
      put(r + 13, files[fid] == 2 ? 512 : 7, 4);
      send_reply(fd, 13, tag, r, 17);
      break;
    case 116: {
      size_t offset = (size_t)get(b + 4, 8), count = (size_t)get(b + 12, 4),
             length;
      assert(fid < 256 && files[fid] != 2 && count <= 7);
      length = strlen(objects[fid]);
      assert(offset <= length);
      if (count > length - offset)
        count = length - offset;
      put(r, count, 4);
      memcpy(r + 4, objects[fid] + offset, count);
      send_reply(fd, 117, tag, r, count + 4);
      break;
    }
    case 118:
      assert(fid < 256 && files[fid] == 2 && !get(b + 4, 8));
      ++writes;
      polls = 0;
      (void)snprintf(
          expected, sizeof expected, "v1 invoke 7 %u 12 %s\n", writes,
          writes == 2 ? "session 14:reload-profile" : "policy 8:next tab");
      assert(get(b + 12, 4) == strlen(expected) && n == 16 + strlen(expected) &&
             !memcmp(b + 16, expected, strlen(expected)));
      if (fault == 1) {
        put(r, 11, 4);
        send_reply(fd, 7, tag, r, 4);
      } else if (fault == 2) {
        close(fd);
        assert(writes == 1 && catalogs == 1);
        _exit(0);
      } else if (fault == 4) {
        put(r, get(b + 12, 4) - 1, 4);
        send_reply(fd, 119, tag, r, 4);
      } else
        send_reply(fd, 119, tag, b + 12, 4);
      break;
    case 120:
      assert(fid < 256);
      files[fid] = 0;
      send_reply(fd, 121, tag, NULL, 0);
      break;
    default:
      assert(0);
    }
  }
  assert(writes == (fault ? 1u : 3u) && catalogs == (fault ? 1u : 2u));
  close(fd);
  _exit(0);
}
int main(void) {
  unsigned fault;
  alarm(15);
  for (fault = 0; fault <= 6; ++fault) {
    int pair[2], exit_status;
    pid_t child;
    uint64_t start = now_ms(), request, wake;
    struct sophia_9p_client wire;
    struct sophia_ad_connecting connecting;
    struct sophia_ad *s = malloc(sizeof *s);
    struct sophia_af_command command = {SOPHIA_AF_POLICY, "next tab", 8};
    size_t size = sophia_9p_storage_bytes(4096, 2);
    void *storage = malloc(size);
    unsigned id;
    assert(s && storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    child = fork();
    assert(child >= 0);
    if (!child) {
      close(pair[0]);
      peer(pair[1], fault);
    }
    close(pair[1]);
    assert(!sophia_9p_init(&wire, pair[0], 4096, 2, 4, storage, size));
    assert(!sophia_ad_connect_init(&connecting, &wire, s, 0, 2000));
    while (!sophia_ad_connect_ready(&connecting)) {
      assert(now_ms() - start < 2000);
      assert(!sophia_ad_connect_dispatch(&connecting, now_ms() - start, 512));
      sched_yield();
    }
    assert(!sophia_ad_connect_next_wake(&connecting, &wake));
    assert(sophia_ad_connect_take_session(&connecting) == s);
    assert(!sophia_ad_connect_take_session(&connecting));
    while (!sophia_ad_catalog(s)) {
      assert(now_ms() - start < 2000);
      assert(!sophia_ad_dispatch(s, now_ms() - start, 1));
      sched_yield();
    }
    assert(sophia_ad_catalog(s)->count == 2 && !sophia_ad_next_wake(s, &wake));
    command.name = "unknown";
    command.name_length = 7;
    assert(sophia_ad_invoke(s, &command, now_ms() - start, &request) ==
           SOPHIA_AD_UNADVERTISED);
    for (id = 1; id <= (fault ? 1u : 3u); ++id) {
      if (id == 3) {
        assert(!sophia_ad_catalog(s));
        assert(!sophia_ad_refresh_catalog(s, now_ms() - start));
        while (!sophia_ad_catalog(s)) {
          assert(now_ms() - start < 2000);
          assert(!sophia_ad_dispatch(s, now_ms() - start, 1));
          sched_yield();
        }
      }
      command.owner = id == 2 ? SOPHIA_AF_SESSION : SOPHIA_AF_POLICY;
      command.name = id == 2 ? "reload-profile" : "next tab";
      command.name_length = strlen(command.name);
      assert(!sophia_ad_invoke(s, &command, now_ms() - start, &request) &&
             request == id);
      assert(sophia_ad_invoke(s, &command, now_ms() - start, &request) ==
             SOPHIA_AD_BUSY);
      while (!s->error && sophia_ad_latest(s)->custody != SOPHIA_AC_SETTLED) {
        assert(now_ms() - start < 2000);
        (void)sophia_ad_dispatch(s, now_ms() - start, 1);
        sched_yield();
      }
      if (fault) {
        assert(s->error &&
               sophia_ad_latest(s)->custody ==
                   (fault == 1 ? SOPHIA_AC_REFUSED : SOPHIA_AC_UNKNOWN));
        assert(sophia_ad_invoke(s, &command, now_ms() - start, &request) ==
               s->error);
      } else {
        const struct sophia_ac *out = sophia_ad_latest(s);
        assert(!s->error && out->outcome == (id == 2 ? SOPHIA_AF_UNCHANGED
                                                     : SOPHIA_AF_COMMITTED));
        assert(out->detail_length == 2 && !memcmp(out->detail, "ok", 2) &&
               !sophia_ad_next_wake(s, &wake));
      }
    }
    sophia_ad_close(s);
    if (!fault)
      assert(sophia_ad_latest(s)->custody == SOPHIA_AC_SETTLED);
    close(pair[0]);
    free(storage);
    free(s);
    assert(waitpid(child, &exit_status, 0) == child && WIFEXITED(exit_status) &&
           !WEXITSTATUS(exit_status));
  }
  return 0;
}
