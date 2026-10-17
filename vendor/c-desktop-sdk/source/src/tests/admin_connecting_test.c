#include "sophia_admin_connecting.h"
#include <assert.h>
#include <errno.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int pair[2];
  struct sophia_9p_client wire;
  struct sophia_ad_connecting c;
  struct sophia_ad *session = malloc(sizeof *session);
  size_t size = sophia_9p_storage_bytes(4096, 2);
  void *storage = malloc(size);
  unsigned char bytes[256];
  uint64_t wake;
  const unsigned char bad_version[] = {16, 0, 0, 0, 101, 255, 255, 0,
                                       16, 0, 0, 3, 0,   'b', 'a', 'd'};
  assert(session && storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
  assert(!sophia_9p_init(&wire, pair[0], 4096, 2, 4, storage, size));
  assert(!sophia_ad_connect_init(&c, &wire, session, 10, 20));
  assert(sophia_ad_connect_next_wake(&c, &wake) && wake == 20);
  assert(sophia_ad_connect_dispatch(&c, 9, 4096) ==
             SOPHIA_AD_CLOCK_REGRESSION &&
         !c.error);
  assert(!sophia_ad_connect_dispatch(&c, 10, 0));
  assert(recv(pair[1], bytes, sizeof bytes, MSG_DONTWAIT) == -1 &&
         (errno == EAGAIN || errno == EWOULDBLOCK));
  assert(!sophia_ad_connect_dispatch(&c, 19, 1));
  assert(recv(pair[1], bytes, sizeof bytes, MSG_DONTWAIT) == 1);
  assert(sophia_ad_connect_dispatch(&c, 20, 4096) == SOPHIA_AD_SNAPSHOT &&
         c.component_error == SOPHIA_AS_DEADLINE);
  assert(!sophia_ad_connect_ready(&c) && !sophia_ad_connect_take_session(&c));
  assert(!sophia_ad_connect_wants_write(&c));
  close(pair[0]);
  assert(read(pair[1], bytes, sizeof bytes) == 0);
  close(pair[1]);
  assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
  assert(!sophia_9p_init(&wire, pair[0], 4096, 2, 4, storage, size));
  assert(!sophia_ad_connect_init(&c, &wire, session, 0, 100));
  assert(!sophia_ad_connect_dispatch(&c, 0, 4096));
  assert(recv(pair[1], bytes, sizeof bytes, MSG_DONTWAIT) > 7 &&
         bytes[4] == 100);
  assert(write(pair[1], bad_version, sizeof bad_version) ==
         (ssize_t)sizeof bad_version);
  assert(sophia_ad_connect_dispatch(&c, 1, 4096) < 0);
  assert(!sophia_ad_connect_ready(&c));
  close(pair[0]);
  assert(read(pair[1], bytes, sizeof bytes) == 0);
  close(pair[1]);
  free(storage);
  free(session);
  return 0;
}
