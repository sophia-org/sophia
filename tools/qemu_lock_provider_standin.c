/* A generic lock provider for the session-lock-provider QEMU scenario, on the
 * vendored C SDK's public lock client only. It shows nothing worth seeing:
 * its modes load Session's lock service, so the scenario can show that an
 * accepted verdict unlocks without waiting for a provider.
 *
 *   baseline  services the connection: reads and acknowledges every event.
 *   flood     the same, plus continuous submissions, one in flight at a
 *             time: 1x1 sealed-image begin/cancel/retire churn and FrameDemands
 *             while the current lock object grants allocations. EAGAIN is retried.
 *   stall     services the connection until it has read and acknowledged
 *             a Locked lock object, then never services it again (no
 *             reads, acks or submits) while keeping it open. Session
 *             revokes a provider whose events go unacknowledged for its
 *             ack timeout, so the stall starts with nothing outstanding;
 *             the first event after it (the first typed key) starts that
 *             clock.
 *
 * Session starts it with SOPHIA_LOCK_9P_SOCKET; the mode is the first word of
 * the file named by SOPHIA_LOCK_CONFIG. Once a second it reports its counters
 * on stderr. It makes no timing claims; the scenario's verifier does. */
#define _GNU_SOURCE
#include "qemu_lock_provider_service.h"
#include "sophia_lock_client.h"
#include "sophia_lock_image.h"
#include "sophia_launch_auth.h"
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

enum mode { BASELINE, FLOOD, STALL };

static struct sophia_9p_client wire;
static struct sophia_lc_client client;
static int fd = -1;
static struct sophia_lc_image image;
static uint8_t formats[65536];
static uint64_t image_offer;
static int source_fd = -1, retiring, demand_due;
static struct sophia_lf_resource image_resource;
static enum mode mode;
static int stall_armed, stalled;
static uint64_t events, submitted, custodied, again, permits, last_custodied;
/* The last service pass, reported with any failure. */
static struct standin_service pass;

static const char *mode_name(void) {
  return mode == FLOOD ? "flood" : mode == STALL ? "stall" : "baseline";
}

static uint64_t now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * 1000u + (uint64_t)t.tv_nsec / 1000000u;
}

static void report(const char *state) {
  fprintf(stderr,
          "sophia_qemu_lock_provider schema=1 mode=%s state=%s events=%llu "
          "submitted=%llu custodied=%llu again=%llu permits=%llu\n",
          mode_name(), state, (unsigned long long)events,
          (unsigned long long)submitted, (unsigned long long)custodied,
          (unsigned long long)again, (unsigned long long)permits);
}

static void die(const char *step) {
  char line[256];
  standin_failure(line, sizeof line, mode_name(), step, &client, &pass);
  fputs(line, stderr);
  exit(1);
}

static enum mode read_mode(void) {
  const char *path = getenv("SOPHIA_LOCK_CONFIG");
  char word[16] = {0};
  FILE *f = path ? fopen(path, "r") : NULL;
  if (!f || fscanf(f, "%15s", word) != 1)
    die("mode");
  fclose(f);
  if (!strcmp(word, "baseline"))
    return BASELINE;
  if (!strcmp(word, "flood"))
    return FLOOD;
  if (!strcmp(word, "stall"))
    return STALL;
  die("mode");
  return BASELINE;
}

/* One wait for the socket (or `timeout_ms`), then one service pass. */
static void turn(int timeout_ms) {
  struct pollfd p[2] = {{fd, POLLIN, 0},
    {sophia_lc_image_poll_fd(&image), sophia_lc_image_poll_events(&image), 0}};
  int64_t image_timeout = sophia_lc_image_timeout(&image, now_ms());
  if (image_timeout >= 0 && image_timeout < timeout_ms) timeout_ms = (int)image_timeout;
  if (sophia_9p_wants_write(&wire)) p[0].events |= POLLOUT;
  (void)poll(p, 2, timeout_ms);
  if (standin_service(&client, &wire, 65536, &pass))
    die("service");
  if (sophia_lc_image_dispatch(&image, p[1].revents, now_ms()) < 0) die("image service");
  switch (sophia_lc_state(&client)) {
  case SOPHIA_LC_STALE:
  case SOPHIA_LC_FAILED:
  case SOPHIA_LC_REFUSED:
    die("connection");
  default:
    break;
  }
}

/* Reads and acknowledges every presented event. */
static void drain(void) {
  const struct sophia_lf_record *e;
  while (!sophia_lc_event(&client, &e)) {
    ++events;
    if (e->header.kind == SOPHIA_LF_FRAME_PERMIT)
      ++permits;
    if (e->header.kind == SOPHIA_LF_RESOURCE_RELEASED && retiring &&
        e->value.resource_released.resource.id == image_resource.id &&
        e->value.resource_released.resource.generation == image_resource.generation) {
      retiring = 0;
      demand_due = 1;
    }
    if (sophia_lc_event_consume(&client))
      die("consume");
    if (mode == STALL && !stalled) {
      uint64_t generation;
      const struct sophia_lf_lock *lock = sophia_lc_lock(&client, &generation);
      if (lock && lock->phase == SOPHIA_LF_LOCKED)
        stall_armed = 1;
    }
  }
}

/* One submission at a time. Sealed offers need a live allocation; cancellation
 * may lose to acceptance, which must be retired through ResourceReleased.
 * This replaces the historical byte workload, not its timing evidence. */
static void flood(uint64_t *transaction, uint64_t *next_demand, unsigned *at,
                  uint64_t *resource) {
  uint64_t generation, id;
  enum sophia_lc_submission stage;
  const struct sophia_lf_lock *lock = sophia_lc_lock(&client, &generation);
  struct sophia_lf_record r;
  if (sophia_lc_submission(&client, &id, &stage, NULL))
    die("submission");
  if (stage == SOPHIA_LC_SUBMISSION_CUSTODIED && id != last_custodied) {
    last_custodied = id;
    ++custodied;
  }
  if (stage == SOPHIA_LC_SUBMISSION_STAGED) {
    if (!sophia_lc_submit_retry(&client))
      ++again;
    return;
  }
  if (retiring) return;
  if (image_offer) {
    struct sophia_ip_offer_info info;
    if (sophia_ip_offer(&image.producer, image_offer, &info)) die("image offer");
    if (info.state == SOPHIA_IPO_SETTLED || info.state == SOPHIA_IPO_REFUSED) {
      if (info.status == SOPHIA_IPS_ACCEPTED) {
        memset(&r, 0, sizeof r);
        r.header.kind = SOPHIA_LF_RESOURCE_RETIRE;
        r.value.resource_step.transaction = *transaction;
        r.value.resource_step.resource = image_resource;
        if (sophia_lc_submit(&client, &r)) return;
        ++*transaction; ++submitted; retiring = 1;
      } else demand_due = 1;
      if (sophia_ip_forget(&image.producer, image_offer)) die("image forget");
      image_offer = 0;
    } else if (!info.cancel_requested) {
      int result = sophia_lc_image_cancel(&image, image_offer, now_ms());
      if (!result) ++submitted;
      else if (result != SOPHIA_IPR_BUSY && result != SOPHIA_IPR_REFUSED) die("image cancel");
    }
    return;
  }
  if (!lock || !lock->allocation_count) return;
  if (!demand_due) {
    struct sophia_ic_metadata metadata = {0};
    int result;
    if (source_fd < 0) {
      void *pixels;
      size_t bytes;
      if (sophia_ip_isb_create(1, 1, &source_fd, &pixels, &bytes)) die("image create");
      memset(pixels, 0, bytes);
      if (sophia_ip_isb_finish(source_fd, pixels, bytes)) die("image seal");
    }
    metadata.resource_id = *resource + 1; metadata.resource_generation = 1;
    metadata.admission_epoch = lock->lock_epoch;
    metadata.output = lock->allocations[0].output;
    metadata.allocation = lock->allocations[0].allocation;
    metadata.topology_generation = lock->topology_generation;
    metadata.source_kind = metadata.source_format = 1;
    metadata.width = metadata.height = 1; metadata.stride = 4;
    metadata.plane_count = metadata.descriptor_count = 1; metadata.logical_bytes = 4;
    result = sophia_lc_image_begin(&image, &metadata, source_fd, now_ms(), &image_offer);
    if (!result) {
      source_fd = -1;
      image_resource.id = ++*resource; image_resource.generation = 1;
      ++submitted;
    } else if (result != SOPHIA_IPR_BUSY && result != SOPHIA_IPR_AGAIN) die("image begin");
    return;
  }
  *at = (*at + 1) % lock->allocation_count;
  memset(&r, 0, sizeof r);
  r.header.kind = SOPHIA_LF_FRAME_DEMAND;
  r.value.frame_demand.transaction = *transaction;
  r.value.frame_demand.lock_epoch = lock->lock_epoch;
  r.value.frame_demand.allocation = lock->allocations[*at].allocation;
  r.value.frame_demand.allocation_generation =
      lock->allocations[*at].allocation_generation;
  r.value.frame_demand.demand = (*next_demand)++;
  if (!sophia_lc_submit(&client, &r)) {
    demand_due = 0;
    ++submitted;
    ++*transaction;
  }
}

int main(void) {
  size_t storage_bytes = sophia_9p_storage_bytes(65536, 16);
  uint8_t *storage = storage_bytes ? malloc(storage_bytes) : NULL;
  const char *path = getenv("SOPHIA_LOCK_9P_SOCKET");
  struct sockaddr_un address;
  struct sophia_lf_negotiate offer;
  struct sophia_9p_client relay;
  struct sophia_la_client auth;
  struct sophia_lc_image_config image_config = {0};
  void *relay_storage = storage_bytes ? malloc(storage_bytes) : NULL;
  char directory[sizeof address.sun_path], *slash;
  uint64_t auth_deadline;
  int image_dir;
  uint64_t transaction = 1, next_demand = 1, reported = 0, resource = 0;
  unsigned at = 0;
  mode = read_mode();
  if (!path || strlen(path) >= sizeof address.sun_path)
    die("socket path");
  memset(&address, 0, sizeof address);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, path);
  fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (fd < 0 || connect(fd, (struct sockaddr *)&address, sizeof address) ||
      fcntl(fd, F_SETFL, O_NONBLOCK))
    die("connect");
  if (!storage || sophia_9p_init(&wire, fd, 65536, 16, 32, storage, storage_bytes))
    die("wire");
  memset(&offer, 0, sizeof offer);
  if (!relay_storage || sophia_9p_init(&relay, STDIN_FILENO, 65536, 16, 32, relay_storage, storage_bytes)) die("auth relay");
  auth_deadline = now_ms() + 10000;
  if (sophia_la_init(&auth, &wire, &relay, "lock", "lock-provider", now_ms())) die("auth begin");
  while (!sophia_la_ready(&auth)) {
    struct pollfd peers[2] = {{fd, POLLIN | (sophia_9p_wants_write(&wire) ? POLLOUT : 0), 0},
      {STDIN_FILENO, POLLIN | (sophia_9p_wants_write(&relay) ? POLLOUT : 0), 0}};
    if (now_ms() >= auth_deadline || poll(peers, 2, 5) < 0 || sophia_la_service(&auth, now_ms(), 65536)) die("auth service");
  }
  offer.minimum_revision = offer.maximum_revision = 2;
  offer.capabilities = SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_IMAGE_SEALED;
  if (sophia_lc_init_attached(&client, &wire, sophia_la_root(&auth), &offer)) die("client");
  strcpy(directory, path); slash = strrchr(directory, '/');
  if (!slash) die("image directory");
  slash[slash == directory ? 1 : 0] = 0;
  image_dir = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
  if (image_dir < 0) die("image directory");
  image_config.endpoint_dir = image_dir; image_config.formats = formats;
  image_config.formats_capacity = sizeof formats;
  image_config.producer.bounds.max_resource_bytes = 4;
  image_config.producer.bounds.max_width = image_config.producer.bounds.max_height = 1;
  image_config.producer.bounds.max_outputs = 16;
  image_config.producer.bounds.transfer_slots = 1;
  image_config.producer.transfer_timeout_ms = 5000;
  image_config.producer.startup_deadline_ms = now_ms() + 10000;
  if (sophia_lc_image_init(&image, &image_config) || sophia_lc_image_attach(&client, &image)) die("image init");
  /* Every mode negotiates and reads the first lock object. */
  while (!events) {
    turn(100);
    drain();
  }
  report("negotiated");
  for (;;) {
    uint64_t now = now_ms();
    if (stall_armed && !stalled) {
      /* Stop only once the server has the Locked object's acknowledgement:
       * the SDK sets `acked` when the ack write's reply arrives (its client
       * struct is exposed for allocation; this only reads it). Unsent bytes
       * alone could miss an ack still queued inside the SDK. */
      unsigned i;
      for (i = 0; i < 200 && (client.acked != client.consumed ||
                              client.ack_op.active || sophia_9p_wants_write(&wire));
           ++i) {
        turn(5);
        drain();
      }
      if (client.acked != client.consumed || client.ack_op.active ||
          sophia_9p_wants_write(&wire))
        die("stall_flush");
      stalled = 1;
      reported = 0;
    }
    if (now - reported >= 1000 || (stalled && !reported)) {
      reported = now;
      report(stalled ? "stalled" : "serving");
    }
    if (stalled) {
      /* Hold the connection without touching it. */
      struct timespec pause = {.tv_sec = 0, .tv_nsec = 100000000};
      nanosleep(&pause, NULL);
      continue;
    }
    turn(mode == FLOOD ? 0 : 100);
    drain();
    if (mode == FLOOD)
      flood(&transaction, &next_demand, &at, &resource);
  }
}
