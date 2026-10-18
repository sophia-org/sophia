/* Independent SDK peer for Sophia's device-free protected launch fixture. */
#define _POSIX_C_SOURCE 200809L
#include "../sophia_launch_auth.h"
#include "../sophia_lock_client.h"
#include "../sophia_lock_image.h"
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
  int upload = getenv("SOPHIA_TEST_LOCK_IMAGE") && !strcmp(getenv("SOPHIA_TEST_LOCK_IMAGE"), "1");
  struct sophia_lc_image image;
  struct sophia_lc_image_config image_config = {0};
  uint8_t formats[65536];
  int image_dir = -1;
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
  offer.minimum_revision = offer.maximum_revision = upload ? 2 : 1;
  offer.capabilities = SOPHIA_LF_CAP_PRESENT | (upload ? SOPHIA_LF_CAP_IMAGE_SEALED : 0);
  assert(!sophia_lc_init_attached(&client, &role, sophia_la_root(&auth), &offer));
  if (upload) {
    char directory[sizeof address.sun_path], *slash;
    strcpy(directory, socket_path);
    slash = strrchr(directory, '/');
    assert(slash); slash[slash == directory ? 1 : 0] = 0;
    image_dir = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    assert(image_dir >= 0);
    image_config.endpoint_dir = image_dir;
    image_config.formats = formats;
    image_config.formats_capacity = sizeof formats;
    image_config.producer.bounds.max_resource_bytes = 256;
    image_config.producer.bounds.max_width = image_config.producer.bounds.max_height = 8;
    image_config.producer.bounds.max_outputs = image_config.producer.bounds.transfer_slots = 1;
    image_config.producer.transfer_timeout_ms = 5000;
    image_config.producer.startup_deadline_ms = deadline;
    assert(!sophia_lc_image_init(&image, &image_config));
    assert(!sophia_lc_image_attach(&client, &image));
  }
  do {
    const struct sophia_lf_record *event;
    struct pollfd pfd = {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0};
    assert(now_ms() < deadline);
    assert(poll(&pfd, 1, 1) >= 0);
    assert(!sophia_lc_service(&client, 65536));
    if (upload) assert(sophia_lc_image_dispatch(&image, 0, now_ms()) >= 0);
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
  assert(sophia_lc_welcome(&client)->granted_capabilities == offer.capabilities);
  if (upload) {
    struct sophia_ic_metadata metadata = {0};
    int source, status;
    void *pixels;
    size_t size;
    uint64_t offer_id;
    assert(!sophia_ip_isb_create(2, 2, &source, &pixels, &size));
    memset(pixels, 0x5a, size);
    assert(!sophia_ip_isb_finish(source, pixels, size));
    metadata.resource_id = metadata.resource_generation = 1;
    metadata.admission_epoch = 3;
    metadata.output = metadata.allocation = metadata.topology_generation = 1;
    metadata.source_kind = metadata.source_format = 1;
    metadata.width = metadata.height = 2;
    metadata.stride = 8;
    metadata.plane_count = metadata.descriptor_count = 1;
    metadata.logical_bytes = 16;
    do {
      assert(now_ms() < deadline);
      status = sophia_lc_image_begin(&image, &metadata, source, now_ms(), &offer_id);
      if (status == SOPHIA_IPR_BUSY || status == SOPHIA_IPR_AGAIN) {
        struct timespec pause = {0, 1000000};
        assert(!sophia_lc_service(&client, 65536));
        assert(sophia_lc_image_dispatch(&image, 0, now_ms()) >= 0);
        nanosleep(&pause, NULL);
      }
    } while (status == SOPHIA_IPR_BUSY || status == SOPHIA_IPR_AGAIN);
    assert(!status);
    /* Producer custody precedes any role submission: revoke while ImageBegin
     * remains staged, before a source can cross the side channel. */
    assert(!sophia_lc_service(&client, 0));
    report(report_path, "image-waiting\n");
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
    if (sophia_lc_service(&client, 65536)) {
      if (upload) (void)sophia_lc_image_dispatch(&image, 0, now_ms());
      break;
    }
    if (upload) assert(sophia_lc_image_dispatch(&image, 0, now_ms()) >= 0);
  }
  assert(sophia_lc_state(&client) == SOPHIA_LC_STALE || sophia_lc_state(&client) == SOPHIA_LC_FAILED);
  {
    uint64_t id;
    enum sophia_lc_submission custody;
    assert(!sophia_lc_submission(&client, &id, &custody, NULL));
    assert(id == (upload ? 2u : 1u));
    assert(custody == (upload ? SOPHIA_LC_SUBMISSION_STAGED : SOPHIA_LC_SUBMISSION_CUSTODIED));
  }
  if (upload) {
    sophia_ip_close(&image.producer);
    close(image_dir);
  }
  report(report_path, upload ? "image-revoked\n" : "revoked\n");
  close(fd); close(STDIN_FILENO); free(role_storage); free(relay_storage);
  return 0;
}
