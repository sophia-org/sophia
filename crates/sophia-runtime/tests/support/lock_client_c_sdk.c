/* The pinned C SDK's lock provider client (sophia_lock_client.h) against
 * Sophia's production lock export. Unlike lock_files_peer.c, every record and
 * custody rule here is the SDK's own: this driver uses only its public API.
 * lock_client_c_sdk.rs plays Session and checks what reached it. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_lock_client.h"
#include "sophia_lock_image.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <time.h>

/* Each wait is bounded: 1000 passes of at most 5 ms, well inside the test's
 * deadline, so a failure names its step. */
#define PASSES 1000u

static struct sophia_9p_client wire;
static struct sophia_lc_client client;
static int fd;
static struct sophia_lc_image image;
static uint64_t now_ms(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}

static void auth_reply(struct sophia_9p_handle request, uint8_t type) {
  uint64_t deadline = now_ms() + 5000;
  for (;;) {
    struct sophia_9p_reply reply;
    struct pollfd p = {fd, POLLIN, 0};
    int result;
    assert(now_ms() < deadline);
    if (sophia_9p_wants_write(&wire)) p.events |= POLLOUT;
    assert(poll(&p, 1, 2) >= 0);
    assert(!sophia_9p_service(&wire, 65536));
    result = sophia_9p_peek(&wire, &reply);
    if (result == SOPHIA_9P_AGAIN) continue;
    assert(!result && reply.handle.serial == request.serial && reply.type == type && !reply.error);
    assert(!sophia_9p_consume(&wire, request));
    return;
  }
}

static void fail(const char *step) {
  fprintf(stderr,
          "lock_client_c_sdk failed step=%s state=%d remote=%u refusal=%u\n",
          step, (int)sophia_lc_state(&client), sophia_lc_remote_error(&client),
          sophia_lc_refusal(&client));
  assert(!"lock client step failed");
}

static void turn(const char *step) {
  struct pollfd p[2] = {{fd, POLLIN, 0},
    {sophia_lc_image_poll_fd(&image), sophia_lc_image_poll_events(&image), 0}};
  if (sophia_9p_wants_write(&wire))
    p[0].events |= POLLOUT;
  assert(poll(p, 2, 5) >= 0);
  /* Any nonzero result is terminal and latches. */
  if (sophia_lc_service(&client, 65536) ||
      sophia_lc_state(&client) == SOPHIA_LC_STALE ||
      sophia_lc_state(&client) == SOPHIA_LC_FAILED ||
      sophia_lc_state(&client) == SOPHIA_LC_REFUSED)
    fail(step);
  if (sophia_lc_image_dispatch(&image, p[1].revents, now_ms()) < 0)
    fail(step);
}

static const struct sophia_lf_record *await(uint16_t kind, const char *step) {
  const struct sophia_lf_record *e;
  unsigned i;
  for (i = 0; i < PASSES; ++i) {
    if (!sophia_lc_event(&client, &e)) {
      if (e->header.kind != kind)
        fail(step);
      return e;
    }
    turn(step);
  }
  fail(step);
  return NULL;
}

static void consume(const char *step) {
  if (sophia_lc_event_consume(&client))
    fail(step);
}

static void submit(const struct sophia_lf_record *r, const char *step) {
  unsigned i;
  int status = SOPHIA_9P_BUSY;
  for (i = 0; i < PASSES && status == SOPHIA_9P_BUSY; ++i) {
    status = sophia_lc_submit(&client, r);
    if (status == SOPHIA_9P_BUSY)
      turn(step);
  }
  if (status)
    fail(step);
}

int main(int argc, char **argv) {
  struct sockaddr_un address;
  static uint8_t storage[1u << 20];
  struct sophia_lf_negotiate offer;
  struct sophia_lc_image_config config = {0};
  struct sophia_ic_metadata metadata = {0};
  struct sophia_ip_offer_info info;
  struct sophia_9p_handle request;
  uint32_t afid, root;
  uint8_t formats[65536];
  char directory[sizeof address.sun_path], *slash;
  int image_dir, source, result;
  void *pixels;
  size_t size;
  uint64_t offer_id;
  struct sophia_lf_record r;
  const struct sophia_lf_record *e;
  const struct sophia_lf_lock *lock;
  uint64_t generation, permit;
  char admitted;
  unsigned i;
  assert(argc == 2 && strlen(argv[1]) < sizeof address.sun_path);
  /* Session admits this process by pidfd before it connects. */
  if (getenv("SOPHIA_TEST_READY_FILE")) {
    uint64_t deadline = now_ms() + 5000;
    for (;;) {
      FILE *gate = fopen(getenv("SOPHIA_TEST_READY_FILE"), "r");
      int ready = gate ? fgetc(gate) : EOF;
      struct timespec pause = {0, 1000000};
      if (gate) fclose(gate);
      if (ready == 'G') break;
      assert(now_ms() < deadline);
      nanosleep(&pause, NULL);
    }
  } else assert(read(STDIN_FILENO, &admitted, 1) == 1 && admitted == 'G');
  memset(&address, 0, sizeof address);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, argv[1]);
  fd = socket(AF_UNIX, SOCK_STREAM, 0);
  assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof address));
  assert(!fcntl(fd, F_SETFL, O_NONBLOCK));
  assert(!sophia_9p_init(&wire, fd, 8192, 8, 32, storage, sizeof storage));
  memset(&offer, 0, sizeof offer);
  offer.minimum_revision = offer.maximum_revision = 2;
  offer.capabilities = SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS | SOPHIA_LF_CAP_IMAGE_SEALED;
  offer.chord_count = 1;
  offer.chords[0].keysym = 0x62;
  offer.chords[0].modifiers = SOPHIA_LF_MOD_ALT;
  assert(!sophia_9p_version(&wire, &request)); auth_reply(request, 101);
  assert(!sophia_9p_auth(&wire, "", "", &request, &afid)); auth_reply(request, 103);
  assert(!sophia_9p_attach_with_afid(&wire, afid, "", "", &request, &root)); auth_reply(request, 105);
  assert(!sophia_9p_clunk(&wire, afid, &request)); auth_reply(request, 121);
  assert(!sophia_lc_init_attached(&client, &wire, root, &offer));
  strcpy(directory, argv[1]); slash = strrchr(directory, '/');
  assert(slash); slash[slash == directory ? 1 : 0] = 0;
  image_dir = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
  assert(image_dir >= 0);
  config.endpoint_dir = image_dir;
  config.formats = formats; config.formats_capacity = sizeof formats;
  config.producer.bounds.max_resource_bytes = 64;
  config.producer.bounds.max_width = config.producer.bounds.max_height = 4;
  config.producer.bounds.max_outputs = config.producer.bounds.transfer_slots = 1;
  config.producer.transfer_timeout_ms = 5000;
  config.producer.startup_deadline_ms = now_ms() + 5000;
  assert(!sophia_lc_image_init(&image, &config));
  assert(!sophia_lc_image_attach(&client, &image));

  e = await(SOPHIA_LF_OBJECT_PUBLISHED, "first lock object");
  lock = sophia_lc_lock(&client, &generation);
  if (!lock || lock->phase != SOPHIA_LF_LOCKED || lock->lock_epoch != 3 ||
      lock->allocation_count != 1 || lock->allocations[0].pixel_width != 2 ||
      sophia_lc_limits(&client)->upload_slots != 1 ||
      sophia_lc_welcome(&client)->granted_chords != 1)
    fail("first lock object contents");
  consume("first lock object");

  assert(!sophia_ip_isb_create(2, 2, &source, &pixels, &size));
  memset(pixels, 0x7f, size);
  assert(!sophia_ip_isb_finish(source, pixels, size));
  metadata.resource_id = metadata.resource_generation = 1;
  metadata.admission_epoch = 3;
  metadata.output = metadata.allocation = metadata.topology_generation = 1;
  metadata.source_kind = metadata.source_format = 1;
  metadata.width = metadata.height = 2; metadata.stride = 8;
  metadata.plane_count = metadata.descriptor_count = 1;
  metadata.logical_bytes = 16;
  for (i = 0; i < PASSES; ++i) {
    result = sophia_lc_image_begin(&image, &metadata, source, now_ms(), &offer_id);
    if (!result) break;
    if (result != SOPHIA_IPR_BUSY && result != SOPHIA_IPR_AGAIN) fail("image begin");
    turn("image readiness");
  }
  if (i == PASSES) fail("image begin timeout");
  e = await(SOPHIA_LF_RESOURCE_STATUS, "accepted");
  if (e->value.resource_status.status != SOPHIA_LF_ACCEPTED)
    fail("accepted status");
  consume("accepted");
  for (i = 0; i < PASSES; ++i) {
    assert(!sophia_ip_offer(&image.producer, offer_id, &info));
    if (info.state == SOPHIA_IPO_SETTLED) break;
    turn("image settlement");
  }
  if (i == PASSES || info.status != SOPHIA_IPS_ACCEPTED || info.disposition != 1)
    fail("image settlement");
  assert(!sophia_ip_forget(&image.producer, offer_id));

  memset(&r, 0, sizeof r);
  r.header.kind = SOPHIA_LF_FRAME_DEMAND;
  r.value.frame_demand.transaction = 3;
  r.value.frame_demand.lock_epoch = 3;
  r.value.frame_demand.allocation = 1;
  r.value.frame_demand.allocation_generation = 1;
  r.value.frame_demand.demand = 1;
  submit(&r, "demand");
  e = await(SOPHIA_LF_FRAME_PERMIT, "permit");
  permit = e->value.frame_permit.pacing_permit;
  consume("permit");

  memset(&r, 0, sizeof r);
  r.header.kind = SOPHIA_LF_CANDIDATE;
  r.value.candidate.transaction = 4;
  r.value.candidate.lock_epoch = 3;
  r.value.candidate.output = 1;
  r.value.candidate.output_generation = 1;
  r.value.candidate.allocation = 1;
  r.value.candidate.allocation_generation = 1;
  r.value.candidate.candidate_generation = 1;
  r.value.candidate.pacing_permit = permit;
  r.value.candidate.resource.id = r.value.candidate.resource.generation = 1;
  submit(&r, "candidate");
  e = await(SOPHIA_LF_CANDIDATE_OUTCOME, "outcome");
  if (e->value.candidate_outcome.status != SOPHIA_LF_PRESENTED)
    fail("presented outcome");
  consume("outcome");

  e = await(SOPHIA_LF_ENTRY, "entry");
  if (e->value.entry.entry != SOPHIA_LF_INSERT || e->value.entry.empty_after)
    fail("entry contents");
  consume("entry");
  e = await(SOPHIA_LF_CHORD, "chord");
  if (e->value.chord.chord != 0)
    fail("chord contents");
  consume("chord");
  e = await(SOPHIA_LF_OBJECT_PUBLISHED, "unlocking object");
  lock = sophia_lc_lock(&client, &generation);
  if (!lock || lock->phase != SOPHIA_LF_UNLOCKING || lock->allocation_count)
    fail("unlocking object contents");
  consume("unlocking object");
  /* Give the final acknowledgement a chance to leave before exiting. */
  for (i = 0; i < 50; ++i)
    turn("final acknowledgement");
  puts("lock_client_c_sdk status=done");
  sophia_ip_close(&image.producer);
  close(image_dir);
  sophia_9p_close(&wire);
  close(fd);
  return 0;
}
