/* Independent SDK peer for Sophia's device-free protected launch fixture. */
#define _POSIX_C_SOURCE 200809L
#include "../sophia_launch_auth.h"
#include "../sophia_lock_client.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static uint64_t now_ms(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static void report(const char *path, const char *text) {
  FILE *f = fopen(path, "w");
  assert(f && fputs(text, f) >= 0 && !fclose(f));
}
int main(void) {
  struct sockaddr_un address = {0};
  struct sophia_9p_client role, relay;
  struct sophia_la_client auth;
  struct sophia_lc_client client;
  struct sophia_lf_negotiate offer = {0};
  struct sophia_lc_obligations obligations;
  const char *socket_path = getenv("SOPHIA_TEST_LOCK_SOCKET");
  const char *report_path = getenv("SOPHIA_TEST_LOCK_REPORT");
  size_t bytes = sophia_9p_storage_bytes(4096, 8);
  void *role_storage = malloc(bytes), *relay_storage = malloc(bytes);
  int fd = socket(AF_UNIX, SOCK_STREAM, 0), observed = 0;
  int upload = getenv("SOPHIA_TEST_LOCK_UPLOAD") && !strcmp(getenv("SOPHIA_TEST_LOCK_UPLOAD"), "1");
  uint8_t pixels[16];
  uint64_t deadline = now_ms() + 10000;
  unsigned round;
  assert(fd >= 0 && role_storage && relay_storage);
  assert(socket_path && report_path && strlen(socket_path) < sizeof address.sun_path);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, socket_path);
  assert(!connect(fd, (struct sockaddr *)&address, sizeof address));
  assert(!fcntl(fd, F_SETFD, FD_CLOEXEC));
  for (round = 0; round < 2; ++round) {
  /* Reauthentication invalidates the old root; it never reuses a verdict.
   * Retire both quiescent clients before reusing their storage and fds. */
  if (round) {
    assert(role.fid_count == 1 && relay.fid_count == 0);
    assert(!role.rx_used && !relay.rx_used);
    sophia_9p_close(&role);
    sophia_9p_close(&relay);
  }
  assert(!sophia_9p_init(&role, fd, 4096, 8, 16, role_storage, bytes));
  assert(!sophia_9p_init(&relay, STDIN_FILENO, 4096, 8, 16, relay_storage, bytes));
  assert(!sophia_la_init(&auth, &role, &relay, "lock", "lock-provider", now_ms()));
  while (!sophia_la_ready(&auth)) {
    struct pollfd fds[2] = {
      {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0},
      {STDIN_FILENO, POLLIN | (sophia_9p_wants_write(&relay) ? POLLOUT : 0), 0}
    };
    assert(now_ms() < deadline);
    assert(poll(fds, 2, 1) >= 0);
    assert(!sophia_la_service(&auth, now_ms(), 65536));
  }
  }
  offer.minimum_revision = offer.maximum_revision = 1;
  offer.capabilities = SOPHIA_LF_CAP_PRESENT;
  assert(!sophia_lc_init_attached(&client, &role, sophia_la_root(&auth), &offer));
  do {
    const struct sophia_lf_record *event;
    struct pollfd pfd = {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0};
    assert(now_ms() < deadline);
    assert(poll(&pfd, 1, 1) >= 0);
    assert(!sophia_lc_service(&client, 65536));
    if (!sophia_lc_event(&client, &event)) {
      uint64_t generation;
      const struct sophia_lf_lock *lock = sophia_lc_lock(&client, &generation);
      assert(event->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
      assert(lock && lock->phase == SOPHIA_LF_LOCKED && lock->lock_epoch == 3);
      assert(!sophia_lc_event_consume(&client));
      observed = 1;
    }
    assert(!sophia_lc_obligations(&client, &obligations));
  } while (!observed || obligations.acked < 3);
  assert(sophia_lc_epoch(&client) == (getenv("SOPHIA_TEST_LOCK_EPOCH") ?
    strtoull(getenv("SOPHIA_TEST_LOCK_EPOCH"), NULL, 10) : 7));
  assert(sophia_lc_welcome(&client)->granted_capabilities == SOPHIA_LF_CAP_PRESENT);
  if (upload) {
    struct sophia_lf_resource_begin begin = {0};
    int status;
    begin.transaction = 2;
    begin.resource.id = begin.resource.generation = 1;
    begin.width_px = begin.height_px = 2;
    do {
      assert(now_ms() < deadline);
      status = sophia_lc_upload_begin(&client, &begin);
      if (status == SOPHIA_9P_BUSY) assert(!sophia_lc_service(&client, 65536));
    } while (status == SOPHIA_9P_BUSY);
    assert(!status);
    observed = 0;
    while (!observed || !sophia_lc_upload_ready(&client)) {
      const struct sophia_lf_record *event;
      struct pollfd pfd = {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0};
      assert(now_ms() < deadline && poll(&pfd, 1, 1) >= 0);
      assert(!sophia_lc_service(&client, 65536));
      if (!sophia_lc_event(&client, &event)) {
        assert(event->header.kind == SOPHIA_LF_RESOURCE_STATUS && event->value.resource_status.status == SOPHIA_LF_ADMITTED);
        observed = 1;
        assert(!sophia_lc_event_consume(&client));
      }
    }
    memset(pixels, 0x5a, sizeof pixels);
    assert(!sophia_lc_upload_chunk(&client, pixels, 4));
    while (!sophia_lc_upload_ready(&client)) {
      struct pollfd pfd = {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0};
      assert(now_ms() < deadline && poll(&pfd, 1, 1) >= 0);
      assert(!sophia_lc_service(&client, 65536));
    }
    assert(!sophia_lc_upload_chunk(&client, pixels + 4, 12));
    assert(!sophia_lc_service(&client, 0));
    assert(!sophia_lc_obligations(&client, &obligations));
    assert(obligations.upload_pending && obligations.upload_writes == 1);
    report(report_path, "upload-waiting\n");
    for (;;) {
      char text[32] = {0};
      FILE *f = fopen(report_path, "r");
      struct timespec pause = {0, 1000000};
      assert(f && now_ms() < deadline);
      (void)fgets(text, sizeof text, f);
      assert(!fclose(f));
      if (!strcmp(text, "continue\n")) break;
      nanosleep(&pause, NULL);
    }
  } else report(report_path, "waiting\n");
  for (;;) {
    struct pollfd pfd = {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0};
    assert(now_ms() < deadline);
    assert(poll(&pfd, 1, 1) >= 0);
    if (sophia_lc_service(&client, 65536)) break;
  }
  assert(sophia_lc_state(&client) == SOPHIA_LC_STALE || sophia_lc_state(&client) == SOPHIA_LC_FAILED);
  {
    uint64_t id;
    enum sophia_lc_submission custody;
    assert(!sophia_lc_submission(&client, &id, &custody, NULL));
    assert(id == (upload ? 2u : 1u) && custody == SOPHIA_LC_SUBMISSION_CUSTODIED);
  }
  if (upload) {
    assert(!sophia_lc_upload_pending(&client) && !sophia_lc_upload_ready(&client));
    assert(!sophia_lc_obligations(&client, &obligations) && !obligations.upload_writes);
    assert(sophia_lc_upload_end(&client) && sophia_lc_upload_cancel(&client));
  }
  report(report_path, upload ? "upload-revoked\n" : "revoked\n");
  close(fd); close(STDIN_FILENO); free(role_storage); free(relay_storage);
  return 0;
}
