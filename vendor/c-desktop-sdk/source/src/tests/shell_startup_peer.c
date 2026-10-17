#define _POSIX_C_SOURCE 200809L
#include "sophia_shell_session.h"
#include <assert.h>
#include <poll.h>
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
int main(int argc, char **argv) {
  struct sophia_ss *session = calloc(1, sizeof *session);
  struct sophia_ss_config config = {
      {8, 8, 1 | 4}, SOPHIA_SF_DESCRIPTOR, 4096, 4, 8192, NULL, 0};
  struct sockaddr_un address = {0};
  struct sophia_ss_obligations obligations;
  uint8_t go;
  size_t size = sophia_ss_storage_bytes(4096, 8192);
  void *storage = malloc(size);
  uint64_t deadline;
  int fd, content;
  alarm(5);
  assert(argc == 3 && session && storage);
  content = !strcmp(argv[2], "content");
  if (content)
    config.offer.required_capabilities |= 128;
  assert(read(STDIN_FILENO, &go, 1) == 1 && go == 1);
  fd = socket(AF_UNIX, SOCK_STREAM, 0);
  assert(fd >= 0 && strlen(argv[1]) < sizeof address.sun_path);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, argv[1]);
  assert(!connect(fd, (struct sockaddr *)&address, sizeof address));
  assert(!sophia_ss_open_fd(session, fd, &config, storage, size));
  deadline = now() + 3000;
  do {
    int ack;
    assert(now() < deadline);
    assert(!sophia_ss_dispatch(session, POLLIN | POLLOUT, 1, now()));
    ack = sophia_ss_ack(session);
    assert(!ack || ack == SOPHIA_9P_BUSY);
    assert(!sophia_ss_obligations(session, &obligations));
  } while (sophia_ss_state(session) != SOPHIA_SS_READY ||
           obligations.acked < 2);
  assert(sophia_ss_epoch(session) == 41);
  assert(sophia_ss_welcome(session)->selected_revision == 8);
  assert(sophia_ss_welcome(session)->capabilities ==
         (3u | 4u | (content ? 128u : 0u)));
  assert(!!sophia_ss_limits(session) == content);
  {
    const struct sophia_sf_record *event = NULL, *object = NULL;
    struct sophia_sf_object_published publications[2];
    int status;
    for (unsigned i = 0; i < 2; i++) {
      do {
        assert(now() < deadline);
        assert(!sophia_ss_dispatch(session, POLLIN | POLLOUT, 1, now()));
        status = sophia_ss_event(session, &event);
      } while (status == SOPHIA_9P_AGAIN);
      assert(!status && event->header.kind == SOPHIA_SF_OBJECT_PUBLISHED);
      for (unsigned check = 0; check < 4; check++) {
        assert(!sophia_ss_dispatch(session, POLLIN | POLLOUT, 1, now()));
        assert(!sophia_ss_event(session, &event));
        assert(event->header.sequence == 3 + i);
        assert(sophia_ss_ack_limit(session) == 2);
      }
      publications[i] = event->value.object_published;
      assert(publications[i].object_kind ==
             (i ? SOPHIA_SF_TABS : SOPHIA_SF_DESCRIPTORS));
      assert(publications[i].generation == 17 + i);
      assert(!sophia_ss_consume(session));
    }
    for (int i = 1; i >= 0; i--) {
      struct sophia_sf_object_published publication = publications[i];
      if (i == 1) {
        assert(!sophia_ss_object(session, publication.object_kind,
                                 publication.generation + 1, publication.qid));
        do {
          assert(now() < deadline);
          assert(!sophia_ss_dispatch(session, POLLIN | POLLOUT, 1, now()));
          status = sophia_ss_object_result(session, &object);
        } while (status == SOPHIA_9P_BUSY);
        assert(status == SOPHIA_9P_AGAIN);
        assert(sophia_ss_ack_limit(session) == 2);
      }
      assert(!sophia_ss_object(session, publication.object_kind,
                               publication.generation, publication.qid));
      do {
        assert(now() < deadline);
        assert(!sophia_ss_dispatch(session, POLLIN | POLLOUT, 1, now()));
        status = sophia_ss_object_result(session, &object);
      } while (status == SOPHIA_9P_BUSY);
      assert(!status && object->header.kind == publication.object_kind);
      if (i == 1)
        assert(sophia_ss_ack_limit(session) == 2);
    }
    assert(!sophia_ss_obligations(session, &obligations));
    assert(obligations.acked == 2 && obligations.ack_limit > 2 &&
           !obligations.objects);
    assert(!sophia_ss_ack(session));
    assert(!sophia_ss_obligations(session, &obligations) &&
           obligations.acked == 2);
    while (obligations.acked < obligations.ack_limit) {
      assert(now() < deadline);
      assert(!sophia_ss_dispatch(session, POLLIN | POLLOUT, 1, now()));
      assert(!sophia_ss_obligations(session, &obligations));
    }
  }
  sophia_ss_close(session);
  close(fd);
  free(storage);
  free(session);
  return 0;
}
