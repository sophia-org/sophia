/* The pinned C SDK's lock provider client (sophia_lock_client.h) against
 * Sophia's production lock export. Unlike lock_files_peer.c, every record and
 * custody rule here is the SDK's own: this driver uses only its public API.
 * lock_client_c_sdk.rs plays Session and checks what reached it. */
#include "sophia_lock_client.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

/* Each wait is bounded: 1000 passes of at most 5 ms, well inside the test's
 * deadline, so a failure names its step. */
#define PASSES 1000u

static struct sophia_9p_client wire;
static struct sophia_lc_client client;
static int fd;

static void fail(const char *step) {
  fprintf(stderr,
          "lock_client_c_sdk failed step=%s state=%d remote=%u refusal=%u\n",
          step, (int)sophia_lc_state(&client), sophia_lc_remote_error(&client),
          sophia_lc_refusal(&client));
  assert(!"lock client step failed");
}

static void turn(const char *step) {
  struct pollfd p;
  p.fd = fd;
  p.events = POLLIN;
  p.revents = 0;
  if (sophia_9p_wants_write(&wire))
    p.events |= POLLOUT;
  (void)poll(&p, 1, 5);
  /* Any nonzero result is terminal and latches. */
  if (sophia_lc_service(&client, 65536) ||
      sophia_lc_state(&client) == SOPHIA_LC_STALE ||
      sophia_lc_state(&client) == SOPHIA_LC_FAILED ||
      sophia_lc_state(&client) == SOPHIA_LC_REFUSED)
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

static void until(int (*done)(const struct sophia_lc_client *), int want,
                  const char *step) {
  unsigned i;
  for (i = 0; i < PASSES && done(&client) != want; ++i)
    turn(step);
  if (done(&client) != want)
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
  struct sophia_lf_resource_begin begin;
  struct sophia_lf_record r;
  const struct sophia_lf_record *e;
  const struct sophia_lf_lock *lock;
  uint64_t generation, permit;
  uint8_t pixels[16];
  char admitted;
  unsigned i;
  assert(argc == 2 && strlen(argv[1]) < sizeof address.sun_path);
  /* Session admits this process by pidfd before it connects. */
  assert(read(STDIN_FILENO, &admitted, 1) == 1 && admitted == 'G');
  memset(&address, 0, sizeof address);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, argv[1]);
  fd = socket(AF_UNIX, SOCK_STREAM, 0);
  assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof address));
  assert(!fcntl(fd, F_SETFL, O_NONBLOCK));
  assert(!sophia_9p_init(&wire, fd, 8192, 8, 32, storage, sizeof storage));
  memset(&offer, 0, sizeof offer);
  offer.minimum_revision = offer.maximum_revision = 1;
  offer.capabilities = SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS;
  offer.chord_count = 1;
  offer.chords[0].keysym = 0x62;
  offer.chords[0].modifiers = SOPHIA_LF_MOD_ALT;
  assert(!sophia_lc_init(&client, &wire, &offer));

  e = await(SOPHIA_LF_OBJECT_PUBLISHED, "first lock object");
  lock = sophia_lc_lock(&client, &generation);
  if (!lock || lock->phase != SOPHIA_LF_LOCKED || lock->lock_epoch != 3 ||
      lock->allocation_count != 1 || lock->allocations[0].pixel_width != 2 ||
      sophia_lc_limits(&client)->upload_slots != 1 ||
      sophia_lc_welcome(&client)->granted_chords != 1)
    fail("first lock object contents");
  consume("first lock object");

  /* A 2x2 image in two chunks; End carries the Begin's transaction. */
  memset(&begin, 0, sizeof begin);
  begin.transaction = 1;
  begin.resource.id = 1;
  begin.resource.generation = 1;
  begin.width_px = begin.height_px = 2;
  if (sophia_lc_upload_begin(&client, &begin))
    fail("upload begin");
  e = await(SOPHIA_LF_RESOURCE_STATUS, "admitted");
  if (e->value.resource_status.status != SOPHIA_LF_ADMITTED)
    fail("admitted status");
  consume("admitted");
  memset(pixels, 0x7f, sizeof pixels);
  until(sophia_lc_upload_ready, 1, "first chunk ready");
  if (sophia_lc_upload_chunk(&client, pixels, 6))
    fail("first chunk");
  until(sophia_lc_upload_ready, 1, "second chunk ready");
  if (sophia_lc_upload_chunk(&client, pixels + 6, 10))
    fail("second chunk");
  until(sophia_lc_upload_ready, 1, "end ready");
  if (sophia_lc_upload_end(&client))
    fail("upload end");
  e = await(SOPHIA_LF_RESOURCE_STATUS, "accepted");
  if (e->value.resource_status.status != SOPHIA_LF_ACCEPTED)
    fail("accepted status");
  consume("accepted");
  until(sophia_lc_upload_pending, 0, "upload settled");

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
  r.value.candidate.resource = begin.resource;
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
  return 0;
}
