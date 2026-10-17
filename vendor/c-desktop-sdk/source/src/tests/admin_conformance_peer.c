#define _POSIX_C_SOURCE 200809L
#include "sophia_admin_connecting.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
static uint64_t start;
static uint64_t clock_ms(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static uint64_t now(void) {
  uint64_t n = clock_ms() - start;
  assert(n < 10000);
  return n;
}
static void wait_io(int fd, int writable, uint64_t wake) {
  struct pollfd p = {fd, POLLIN | (writable ? POLLOUT : 0), 0};
  uint64_t n = now();
  int r = poll(&p, 1, (int)(wake > n ? wake - n : 0));
  assert(r >= 0 || errno == EINTR);
}
static void turn(struct sophia_ad *s) {
  uint64_t wake = 10000;
  (void)sophia_ad_next_wake(s, &wake);
  wait_io(s->wire->fd, sophia_ad_wants_write(s), wake);
  (void)sophia_ad_dispatch(s, now(), 65536);
}
static void outcome(struct sophia_ad *s, uint64_t id,
                    enum sophia_af_outcome expected) {
  const struct sophia_ac *r;
  while ((r = sophia_ad_latest(s))->custody != SOPHIA_AC_SETTLED) {
    assert(!s->error);
    turn(s);
  }
  assert(r->request == id && r->outcome == expected);
}
static void report(const char *path, const char *text) {
  FILE *f = fopen(path, "w");
  assert(f && fputs(text, f) >= 0 && !fclose(f));
}
int main(int argc, char **argv) {
  struct sockaddr_un address = {0};
  struct sophia_9p_client wire;
  struct sophia_ad_connecting connecting;
  struct sophia_ad *s = malloc(sizeof *s);
  struct sophia_af_command command = {SOPHIA_AF_POLICY, "focus next", 10};
  size_t size = sophia_9p_storage_bytes(65536, 2);
  void *storage = malloc(size);
  int fd = socket(AF_UNIX, SOCK_STREAM, 0), revoke;
  uint64_t request, wake;
  assert(argc == 4 && s && storage && fd >= 0);
  start = clock_ms();
  assert(!strcmp(argv[3], "settle") || !strcmp(argv[3], "revoke") ||
         !strcmp(argv[3], "loss") || !strcmp(argv[3], "recover") ||
         !strcmp(argv[3], "denied"));
  revoke = strcmp(argv[3], "settle") != 0;
  assert(strlen(argv[1]) < sizeof address.sun_path);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, argv[1]);
  assert(!connect(fd, (struct sockaddr *)&address, sizeof address));
  assert(!sophia_9p_init(&wire, fd, 65536, 2, 4, storage, size));
  assert(!sophia_ad_connect_init(&connecting, &wire, s, 0, 2000));
  while (!sophia_ad_connect_ready(&connecting)) {
    int result;
    wake = 2000;
    (void)sophia_ad_connect_next_wake(&connecting, &wake);
    wait_io(fd, sophia_ad_connect_wants_write(&connecting), wake);
    result = sophia_ad_connect_dispatch(&connecting, now(), 65536);
    if (result) {
      assert(!strcmp(argv[3], "denied"));
      /* Admission closes the reachable socket before negotiation, not by
       * timeout. */
      assert(result == SOPHIA_AD_WIRE &&
             (connecting.component_error == SOPHIA_9P_IO ||
              connecting.component_error == SOPHIA_9P_CLOSED));
      assert(!sophia_ad_connect_ready(&connecting));
      assert(!sophia_ad_connect_take_session(&connecting));
      close(fd);
      free(storage);
      free(s);
      return 0;
    }
  }
  assert(strcmp(argv[3], "denied"));
  assert(sophia_ad_connect_take_session(&connecting) == s);
  assert(sophia_ad_catalog(s)->generation == 7);
  assert(!sophia_ad_invoke(s, &command, now(), &request) && request == 1);
  if (revoke) {
    while (!s->error)
      turn(s);
    assert(sophia_ad_latest(s)->custody == SOPHIA_AC_UNKNOWN);
    assert(sophia_ad_invoke(s, &command, now(), &request));
    if (!strcmp(argv[3], "recover")) {
      struct sophia_9p_client new_wire;
      struct sophia_ad_connecting c;
      struct sophia_ad *fresh = malloc(sizeof *fresh);
      struct sophia_af_command probe = {SOPHIA_AF_POLICY, "probe", 5};
      void *new_storage = malloc(size);
      char path[256];
      int new_fd = socket(AF_UNIX, SOCK_STREAM, 0);
      uint64_t begun;
      assert(fresh && new_storage && new_fd >= 0);
      report(argv[2], "lost\n");
      for (;;) {
        char text[280] = {0};
        FILE *f = fopen(argv[2], "r");
        struct timespec pause = {0, 1000000};
        size_t n;
        assert(f);
        (void)fgets(text, sizeof text, f);
        assert(!fclose(f));
        n = strlen(text);
        if (n > 9 && !strncmp(text, "recover ", 8) && text[n - 1] == '\n') {
          assert(n - 9 < sizeof path);
          memcpy(path, text + 8, n - 9);
          path[n - 9] = 0;
          break;
        }
        (void)now();
        nanosleep(&pause, NULL);
      }
      memset(&address, 0, sizeof address);
      address.sun_family = AF_UNIX;
      assert(strlen(path) < sizeof address.sun_path);
      strcpy(address.sun_path, path);
      assert(!connect(new_fd, (struct sockaddr *)&address, sizeof address));
      assert(
          !sophia_9p_init(&new_wire, new_fd, 65536, 2, 4, new_storage, size));
      begun = now();
      assert(
          !sophia_ad_connect_init(&c, &new_wire, fresh, begun, begun + 2000));
      while (!sophia_ad_connect_ready(&c)) {
        wake = 10000;
        (void)sophia_ad_connect_next_wake(&c, &wake);
        wait_io(new_fd, sophia_ad_connect_wants_write(&c), wake);
        assert(!sophia_ad_connect_dispatch(&c, now(), 65536));
      }
      assert(sophia_ad_connect_take_session(&c) == fresh);
      assert(fresh->info.session_low != s->info.session_low ||
             fresh->info.session_high != s->info.session_high);
      assert(sophia_ad_catalog(fresh)->generation == 9);
      assert(!sophia_ad_invoke(fresh, &probe, now(), &request) && request == 1);
      outcome(fresh, 1, SOPHIA_AF_REJECTED);
      assert(sophia_ad_latest(s)->custody == SOPHIA_AC_UNKNOWN);
      sophia_ad_close(fresh);
      close(new_fd);
      free(new_storage);
      free(fresh);
    }
  } else {
    outcome(s, 1, SOPHIA_AF_COMMITTED);
    report(argv[2], "first\n");
    for (;;) {
      char text[32] = {0};
      FILE *f = fopen(argv[2], "r");
      struct timespec pause = {0, 1000000};
      assert(f);
      (void)fgets(text, sizeof text, f);
      assert(!fclose(f));
      if (!strcmp(text, "continue\n"))
        break;
      (void)now();
      nanosleep(&pause, NULL);
    }
    assert(!sophia_ad_invoke(s, &command, now(), &request) && request == 2);
    outcome(s, 2, SOPHIA_AF_STALE);
    assert(!sophia_ad_catalog(s));
    assert(!sophia_ad_refresh_catalog(s, now()));
    while (!sophia_ad_catalog(s)) {
      assert(!s->error);
      turn(s);
    }
    assert(sophia_ad_catalog(s)->generation == 8);
    assert(!sophia_ad_invoke(s, &command, now(), &request) && request == 3);
    outcome(s, 3, SOPHIA_AF_COMMITTED);
    sophia_ad_close(s);
    assert(sophia_ad_latest(s)->custody == SOPHIA_AC_SETTLED);
  }
  report(argv[2], "ok\n");
  close(fd);
  free(storage);
  free(s);
  return 0;
}
