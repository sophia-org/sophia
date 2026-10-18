/* The lock image adapter over the scripted 9P lock peer and a real
 * SEQPACKET image endpoint: explicit r2 selection, discovery through the
 * client's wire, Hello, the ImageBegin submission, image journal routing,
 * channel failure and journal loss. */
#define _GNU_SOURCE
#include "lock_client_peer.h"
#include "../sophia_lock_image.h"
#undef NDEBUG
#include <assert.h>
#include <dirent.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/un.h>

#define GEN 22u
#define SEALS (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)
#define CAPS (SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS)

static unsigned fd_count(void) {
  DIR *d = opendir("/proc/self/fd");
  unsigned n = 0;
  assert(d);
  while (readdir(d))
    n++;
  closedir(d);
  return n;
}

/* ---- discovery files ----------------------------------------------------- */

struct set {
  char text[1024];
  uint8_t limits[SOPHIA_IC_LIMITS_BYTES], formats[SOPHIA_IC_FORMATS_HEADER_BYTES];
  size_t text_bytes, limits_bytes, formats_bytes;
};

static void make_set(struct set *s, uint32_t kinds) {
  struct sophia_ic_record r;
  struct sophia_ic_header h = {SOPHIA_IC_IMAGE_FORMATS, EPOCH, 0, 0};
  struct sophia_ic_formats f;
  memset(s, 0, sizeof(*s));
  s->text_bytes = (size_t)snprintf(
      s->text, sizeof(s->text),
      "sophia-image-channel version=1\nrole=2\nconnection_epoch=%u\n"
      "channel_generation=%u\nendpoint=image.sock\nsource_kinds=%u\n"
      "max_packet_bytes=200\nmax_fds=4\nmax_offers=4\nmax_packets_per_turn=8\n"
      "hello_timeout_ms=500\nvalidation_timeout_ms=100\n"
      "validation_chunk_bytes=65536\ncapture_interest_timeout_ms=200\n"
      "formats_generation=5\nlimits_generation=6\n",
      EPOCH, GEN, kinds);
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_IC_IMAGE_LIMITS;
  r.header.epoch = EPOCH;
  r.value.limits.channel_generation = GEN;
  r.value.limits.limits_generation = 6;
  r.value.limits.max_resource_bytes = 1u << 20;
  r.value.limits.max_sealed_bytes = 1u << 21;
  r.value.limits.max_offers = 4;
  r.value.limits.max_validation_jobs = 2;
  r.value.limits.max_outputs = 2;
  r.value.limits.max_rows_per_output = 1;
  r.value.limits.max_width = r.value.limits.max_height = 512;
  r.value.limits.validation_timeout_ms = 100;
  r.value.limits.capture_interest_timeout_ms = 200;
  r.value.limits.validation_chunk_bytes = 65536;
  r.value.limits.max_packets_per_turn = 8;
  assert(sophia_ic_record_encode(s->limits, sizeof(s->limits), &r,
                                 &s->limits_bytes) == SOPHIA_IC_OK);
  memset(&f, 0, sizeof(f));
  f.channel_generation = GEN;
  f.formats_generation = 5;
  f.limits_generation = 6;
  assert(sophia_ic_formats_encode(s->formats, sizeof(s->formats), &h, &f, NULL,
                                  &s->formats_bytes) == SOPHIA_IC_OK);
}

/* ---- rig ----------------------------------------------------------------- */

struct image_rig {
  struct rig *r;
  struct set set;
  struct sophia_lc_image img;
  uint8_t formats[256];
  char dir[64];
  int dirfd, listener, owner;
  uint64_t now;
};

static struct sophia_lf_negotiate image_offer(uint16_t min, uint16_t max,
                                              uint64_t caps) {
  struct sophia_lf_negotiate o = offer();
  o.minimum_revision = min;
  o.maximum_revision = max;
  o.capabilities = caps;
  return o;
}

static struct sophia_lc_image_config adapter_config(struct image_rig *t) {
  struct sophia_lc_image_config c;
  memset(&c, 0, sizeof(c));
  c.producer.bounds.max_resource_bytes = 1u << 20;
  c.producer.bounds.max_width = c.producer.bounds.max_height = 512;
  c.producer.bounds.max_outputs = 2;
  c.producer.bounds.transfer_slots = 4;
  c.producer.transfer_timeout_ms = 2000;
  c.producer.startup_deadline_ms = 1000 + 5000;
  c.endpoint_dir = t->dirfd;
  c.formats = t->formats;
  c.formats_capacity = sizeof(t->formats);
  return c;
}

static void listen_dir(struct image_rig *t) {
  struct sockaddr_un a;
  snprintf(t->dir, sizeof(t->dir), "/tmp/sophia-lc-image-XXXXXX");
  assert(mkdtemp(t->dir));
  t->dirfd = open(t->dir, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
  assert(t->dirfd >= 0);
  t->listener = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
  memset(&a, 0, sizeof(a));
  a.sun_family = AF_UNIX;
  snprintf(a.sun_path, sizeof(a.sun_path), "%s/image.sock", t->dir);
  assert(!bind(t->listener, (struct sockaddr *)&a, sizeof(a)) &&
         !listen(t->listener, 4));
  t->owner = -1;
}

/* A client offering o against a peer selecting up to revision, with the
 * adapter attached and the discovery files served. */
static void image_start(struct image_rig *t, uint16_t revision,
                        const struct sophia_lf_negotiate *o) {
  struct sophia_lc_image_config c;
  memset(t, 0, sizeof(*t));
  t->now = 1000;
  listen_dir(t);
  make_set(&t->set, SOPHIA_IC_KIND_BIT_SEALED);
  t->r = rig_new(o, NULL);
  t->r->peer.revision = revision;
  t->r->peer.image_data[0] = (const uint8_t *)t->set.text;
  t->r->peer.image_bytes[0] = t->set.text_bytes;
  t->r->peer.image_data[1] = t->set.limits;
  t->r->peer.image_bytes[1] = t->set.limits_bytes;
  t->r->peer.image_data[2] = t->set.formats;
  t->r->peer.image_bytes[2] = t->set.formats_bytes;
  c = adapter_config(t);
  assert(sophia_lc_image_init(&t->img, &c) == SOPHIA_IPR_OK);
  assert(sophia_lc_image_attach(&t->r->client, &t->img) == SOPHIA_IPR_OK);
  assert(sophia_lc_image_attach(&t->r->client, &t->img) == SOPHIA_IPR_ARGUMENT);
}

static int image_step(struct image_rig *t) {
  int status = sophia_lc_service(&t->r->client, 65536);
  pump(&t->r->peer);
  t->now++;
  assert(sophia_lc_image_dispatch(&t->img, sophia_lc_image_poll_events(&t->img),
                                  t->now) >= 0);
  return status;
}

static void image_close(struct image_rig *t) {
  struct sophia_ip *p = sophia_lc_image_producer(&t->img);
  struct sophia_ip_offer_info i;
  unsigned cursor = 0;
  if (p) {
    sophia_ip_close(p);
    while (sophia_ip_offer_next(p, &cursor, &i))
      assert(sophia_ip_forget(p, i.offer) == SOPHIA_IPR_OK);
  }
  rig_free(t->r);
  if (t->owner >= 0)
    close(t->owner);
  close(t->listener);
  unlinkat(t->dirfd, "image.sock", 0);
  close(t->dirfd);
  rmdir(t->dir);
}

static void peer_channel(struct peer *p, uint16_t kind, uint32_t value) {
  struct sophia_ic_record r;
  memset(&r, 0, sizeof(r));
  r.header.kind = kind;
  r.value.channel.channel_generation = GEN;
  r.value.channel.value = value;
  image_record(p, &r);
}

static void peer_status(struct peer *p, uint64_t offer, uint64_t resource,
                        uint16_t status, uint16_t reason) {
  struct sophia_lf_record r;
  memset(&r, 0, sizeof(r));
  r.value.resource_status.transaction = offer;
  r.value.resource_status.resource.id = resource;
  r.value.resource_status.resource.generation = 1;
  r.value.resource_status.status = status;
  r.value.resource_status.reason = reason;
  event(p, SOPHIA_LF_RESOURCE_STATUS, &r);
}

static void peer_released(struct peer *p, uint64_t offer, uint64_t resource) {
  struct sophia_ic_record r;
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_IC_SOURCE_RELEASED;
  r.value.released.offer = offer;
  r.value.released.resource_id = resource;
  r.value.released.resource_generation = 1;
  r.value.released.disposition = SOPHIA_IC_RETAINED;
  image_record(p, &r);
}

static ssize_t owner_receive(struct image_rig *t, uint8_t *b, size_t n, int *fd) {
  struct msghdr msg;
  struct iovec iov = {b, n};
  union {
    struct cmsghdr align;
    char bytes[CMSG_SPACE(4 * sizeof(int))];
  } control;
  struct cmsghdr *c;
  ssize_t got;
  if (t->owner < 0)
    t->owner = accept4(t->listener, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
  assert(t->owner >= 0);
  memset(&msg, 0, sizeof(msg));
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;
  msg.msg_control = control.bytes;
  msg.msg_controllen = sizeof(control.bytes);
  *fd = -1;
  got = recvmsg(t->owner, &msg, MSG_DONTWAIT | MSG_CMSG_CLOEXEC);
  for (c = CMSG_FIRSTHDR(&msg); got >= 0 && c; c = CMSG_NXTHDR(&msg, c))
    memcpy(fd, CMSG_DATA(c), sizeof(*fd));
  return got;
}

/* Consumes every presented event; image kinds never appear. Returns the
 * count and the kinds seen, up to n. */
static unsigned app_events(struct image_rig *t, uint16_t *kinds, unsigned n) {
  const struct sophia_lf_record *e;
  unsigned seen = 0;
  while (!sophia_lc_event(&t->r->client, &e)) {
    assert(e->header.kind < 256);
    if (seen < n)
      kinds[seen] = e->header.kind;
    seen++;
    assert(!sophia_lc_event_consume(&t->r->client));
  }
  return seen;
}

static void steps(struct image_rig *t, unsigned n) {
  uint16_t kinds[8];
  while (n--) {
    assert(!image_step(t));
    (void)app_events(t, kinds, 8);
  }
}

/* Steps and consumes until the client fails; returns its status. */
static int failure(struct image_rig *t) {
  const struct sophia_lf_record *e;
  unsigned i;
  int status = 0;
  for (i = 0; i < 32 && !status; i++) {
    status = image_step(t);
    while (!status && !sophia_lc_event(&t->r->client, &e))
      status = sophia_lc_event_consume(&t->r->client);
  }
  return status;
}

/* Until the client is READY. */
static void settle(struct image_rig *t) {
  unsigned i;
  for (i = 0; i < 256 && !sophia_lc_ready(&t->r->client); i++)
    steps(t, 1);
  assert(sophia_lc_ready(&t->r->client));
}

static int sealed(uint32_t w, uint32_t h) {
  void *pixels;
  size_t bytes;
  int fd;
  assert(sophia_ip_isb_create(w, h, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
  memset(pixels, 0x40, bytes);
  assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
  return fd;
}

static struct sophia_ic_metadata image(uint64_t resource) {
  struct sophia_ic_metadata m;
  memset(&m, 0, sizeof(m));
  m.resource_id = resource;
  m.resource_generation = 1;
  m.admission_epoch = 3;
  m.output = 7;
  m.allocation = 8;
  m.topology_generation = 9;
  m.source_kind = SOPHIA_IC_SOURCE_SEALED;
  m.source_format = SOPHIA_IC_SEALED_BGRA8_PREMULTIPLIED;
  m.width = m.height = 16;
  m.stride = 64;
  m.plane_count = m.descriptor_count = 1;
  m.logical_bytes = 16 * 16 * 4;
  return m;
}

/* Through r2 negotiation, discovery, connect, Hello and ChannelReady. */
static struct sophia_ip *image_ready(struct image_rig *t) {
  uint8_t b[256];
  unsigned i;
  int fd;
  struct sophia_ip *p;
  for (i = 0; i < 256 && sophia_lc_image_poll_fd(&t->img) < 0; i++)
    steps(t, 1);
  assert(sophia_lc_ready(&t->r->client));
  assert(sophia_lc_welcome(&t->r->client)->selected_revision == 2);
  p = sophia_lc_image_producer(&t->img);
  assert(p && sophia_ip_channel(p) >= SOPHIA_IPC_HELLO_QUEUED);
  for (i = 0; i < 8 && sophia_ip_channel(p) != SOPHIA_IPC_HELLO_SENT; i++)
    steps(t, 1);
  assert(sophia_ip_channel(p) == SOPHIA_IPC_HELLO_SENT);
  assert(t->r->peer.image_walks[0] == 1 && t->r->peer.image_walks[1] == 1 &&
         t->r->peer.image_walks[2] == 1);
  assert(owner_receive(t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES && fd < 0);
  peer_channel(&t->r->peer, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_SEALED);
  for (i = 0; i < 8 && sophia_ip_channel(p) != SOPHIA_IPC_READY; i++)
    steps(t, 1);
  assert(sophia_ip_channel(p) == SOPHIA_IPC_READY &&
         sophia_ip_path(p) == SOPHIA_IPP_DESCRIPTOR);
  return p;
}

static void begin_generation(struct image_rig *t, uint64_t resource,
                             uint64_t generation, uint64_t *offer) {
  struct sophia_ic_metadata m = image(resource);
  uint64_t id;
  enum sophia_lc_submission stage;
  unsigned i;
  int src = sealed(16, 16), r;
  m.resource_generation = generation;
  /* BUSY keeps src with the caller while the transaction recycles. */
  for (i = 0; i < 16 && (r = sophia_lc_image_begin(&t->img, &m, src, t->now,
                                                    offer)) == SOPHIA_IPR_BUSY;
       i++)
    steps(t, 1);
  assert(r == SOPHIA_IPR_OK);
  for (i = 0; i < 16; i++) {
    steps(t, 1);
    assert(!sophia_lc_submission(&t->r->client, &id, &stage, NULL));
    if (stage == SOPHIA_LC_SUBMISSION_CUSTODIED)
      break;
  }
  assert(stage == SOPHIA_LC_SUBMISSION_CUSTODIED);
  assert(t->r->peer.image_begin.header.kind == SOPHIA_IC_IMAGE_BEGIN &&
         t->r->peer.image_begin.header.submission == id);
  assert(sophia_ic_metadata_equal(&t->r->peer.image_begin.value.image, &m));
}

static void begin_custodied(struct image_rig *t, uint64_t resource,
                            uint64_t *offer) {
  begin_generation(t, resource, 1, offer);
}

/* ---- cases --------------------------------------------------------------- */

static void offers_and_attach(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o;
  struct sophia_lc_image_config c;
  struct sophia_lc_image other;
  struct rig *r;
  unsigned i, base = fd_count();
  /* Bit 2 needs maximum revision 2; bit 3 is never offered. */
  o = image_offer(1, 1, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  r = rig_storage(&o, NULL, 8, 0);
  assert(sophia_lc_init(&r->client, &r->wire, &o) == SOPHIA_9P_ARGUMENT);
  o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED |
                            SOPHIA_LF_CAP_IMAGE_DMABUF);
  assert(sophia_lc_init(&r->client, &r->wire, &o) == SOPHIA_9P_ARGUMENT);
  o = image_offer(1, 2, CAPS | 16);
  assert(sophia_lc_init(&r->client, &r->wire, &o) == SOPHIA_9P_ARGUMENT);
  rig_free(r);
  /* Attach needs an offer spanning 2 with bit 2. */
  memset(&t, 0, sizeof(t));
  listen_dir(&t);
  c = adapter_config(&t);
  assert(sophia_lc_image_init(&other, &c) == SOPHIA_IPR_OK);
  o = image_offer(1, 1, CAPS);
  r = rig_new(&o, NULL);
  assert(sophia_lc_image_attach(&r->client, &other) == SOPHIA_IPR_ARGUMENT);
  rig_free(r);
  o = image_offer(1, 2, CAPS);
  r = rig_new(&o, NULL);
  assert(sophia_lc_image_attach(&r->client, &other) == SOPHIA_IPR_ARGUMENT);
  rig_free(r);
  o = image_offer(3, 4, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  r = rig_new(&o, NULL);
  assert(sophia_lc_image_attach(&r->client, &other) == SOPHIA_IPR_ARGUMENT);
  rig_free(r);
  assert(sophia_lc_image_attach(NULL, &other) == SOPHIA_IPR_ARGUMENT);
  c.formats_capacity = SOPHIA_LC_IMAGE_FORMATS_MIN - 1;
  assert(sophia_lc_image_init(&other, &c) == SOPHIA_IPR_ARGUMENT);
  c = adapter_config(&t);
  c.endpoint_dir = -1;
  assert(sophia_lc_image_init(&other, &c) == SOPHIA_IPR_ARGUMENT);
  close(t.listener);
  unlinkat(t.dirfd, "image.sock", 0);
  close(t.dirfd);
  rmdir(t.dir);
  /* A second adapter is BUSY; after negotiation the first is too late. */
  o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  image_start(&t, 2, &o);
  c = adapter_config(&t);
  assert(sophia_lc_image_init(&other, &c) == SOPHIA_IPR_OK);
  assert(sophia_lc_image_attach(&t.r->client, &other) == SOPHIA_IPR_BUSY);
  for (i = 0; i < 128 && !sophia_lc_ready(&t.r->client); i++)
    steps(&t, 1);
  assert(sophia_lc_ready(&t.r->client));
  t.r->client.image = NULL;
  assert(sophia_lc_image_attach(&t.r->client, &other) == SOPHIA_IPR_BUSY);
  t.r->client.image = &t.img;
  assert(sophia_lc_image_producer(&other) == NULL &&
         sophia_lc_image_poll_fd(&other) < 0 &&
         sophia_lc_image_timeout(&other, 0) < 0);
  assert(sophia_lc_image_dispatch(&other, 0, 0) == -(int)SOPHIA_IPR_ARGUMENT);
  image_close(&t);
  assert(fd_count() == base);
}

/* The client checks the explicit selection against its offer before it
 * decodes anything at that revision. */
static void explicit_selection(void) {
  struct sophia_lf_negotiate o;
  struct rig *r;
  /* An r1 offer stays plain r1. */
  r = ready();
  assert(sophia_lc_welcome(&r->client)->selected_revision == 1);
  rig_free(r);
  /* An unattached client offering 1-2 accepts selection 2 with mask 3. */
  o = image_offer(1, 2, CAPS);
  r = rig_new(&o, NULL);
  r->peer.revision = 2;
  assert(next(r)->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  consume(r);
  assert(sophia_lc_welcome(&r->client)->selected_revision == 2 &&
         sophia_lc_welcome(&r->client)->granted_capabilities == CAPS);
  rig_free(r);
  /* A peer selecting 1 for that offer: plain r1. */
  r = rig_new(&o, NULL);
  assert(next(r)->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  assert(sophia_lc_welcome(&r->client)->selected_revision == 1);
  rig_free(r);
  /* A selection below the offered minimum is invalid. */
  o = image_offer(2, 2, CAPS);
  r = rig_new(&o, NULL);
  r->peer.revision = 2;
  r->peer.force_selected = 1;
  assert(spin(r) == SOPHIA_9P_INVALID && !sophia_lc_ready(&r->client));
  rig_free(r);
  /* Selection 2 against an r1 offer is never decoded. */
  o = image_offer(1, 1, CAPS);
  r = rig_new(&o, NULL);
  r->peer.revision = 2;
  r->peer.force_selected = 2;
  assert(spin(r) == SOPHIA_9P_INVALID && !sophia_lc_ready(&r->client));
  rig_free(r);
  /* A sealed grant the offer did not request is invalid. */
  o = image_offer(1, 2, CAPS);
  r = rig_new(&o, NULL);
  r->peer.revision = 2;
  r->peer.force_grant = CAPS | SOPHIA_LF_CAP_IMAGE_SEALED;
  assert(spin(r) == SOPHIA_9P_INVALID);
  rig_free(r);
  /* An unattached client granted sealed images fails on an image event. */
  o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  r = rig_new(&o, NULL);
  r->peer.revision = 2;
  assert(next(r)->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  consume(r);
  peer_channel(&r->peer, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_SEALED);
  assert(spin(r) == SOPHIA_9P_INVALID &&
         sophia_lc_state(&r->client) == SOPHIA_LC_FAILED);
  rig_free(r);
}

static void sealed_offer_end_to_end(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ic_record r;
  struct sophia_ip *p;
  struct sophia_ip_offer_info info;
  uint8_t b[256];
  uint16_t kinds[8];
  uint64_t offer = 0, acks;
  unsigned i, base = fd_count();
  int fd;
  image_start(&t, 2, &o);
  p = image_ready(&t);
  acks = t.r->peer.acked;
  begin_custodied(&t, 40, &offer);
  assert(offer == 1);
  /* Admission routes to the producer, which sends the source. */
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
  r.value.image = t.r->peer.image_begin.value.image;
  image_record(&t.r->peer, &r);
  for (i = 0; i < 8 && owner_receive(&t, b, sizeof(b), &fd) < 0; i++)
    steps(&t, 1);
  assert(fd >= 0 && (fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
  close(fd);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK &&
         info.state == SOPHIA_IPO_SENT);
  /* Status 1 is not final; Accepted reaches both producer and caller. */
  peer_status(&t.r->peer, offer, 40, SOPHIA_LF_ADMITTED, 0);
  peer_status(&t.r->peer, offer, 40, SOPHIA_LF_ACCEPTED, 0);
  peer_released(&t.r->peer, offer, 40);
  for (i = 0; i < 4; i++)
    assert(!image_step(&t));
  assert(app_events(&t, kinds, 8) == 2 &&
         kinds[0] == SOPHIA_LF_RESOURCE_STATUS &&
         kinds[1] == SOPHIA_LF_RESOURCE_STATUS);
  steps(&t, 4);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK);
  assert(info.state == SOPHIA_IPO_SETTLED && info.status == SOPHIA_IPS_ACCEPTED &&
         info.disposition == SOPHIA_IC_RETAINED);
  /* Image events were acknowledged without the caller. */
  assert(t.r->peer.acked == t.r->peer.sequence && t.r->peer.acked > acks);
  assert(sophia_ip_channel(p) == SOPHIA_IPC_READY);
  assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
  assert(sophia_lc_ready(&t.r->client));
  image_close(&t);
  assert(fd_count() == base);
}

static void status_routing(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ip *p;
  struct sophia_ip_offer_info info;
  uint64_t offer;
  unsigned base = fd_count();
  image_start(&t, 2, &o);
  p = image_ready(&t);
  begin_custodied(&t, 60, &offer);
  /* A different resource, or a different transaction, is not routed. */
  peer_status(&t.r->peer, offer, 61, SOPHIA_LF_RESOURCE_REJECTED, 9);
  peer_status(&t.r->peer, offer + 1, 60, SOPHIA_LF_RESOURCE_REJECTED, 9);
  steps(&t, 4);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK &&
         info.state == SOPHIA_IPO_BEGUN && !info.status);
  /* Rejected reason 9: unavailable, the channel closes cleanly. */
  peer_status(&t.r->peer, offer, 60, SOPHIA_LF_RESOURCE_REJECTED, 9);
  steps(&t, 4);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK &&
         info.state == SOPHIA_IPO_REFUSED && info.unavailable);
  assert(sophia_ip_cause(p) == SOPHIA_IPX_UNAVAILABLE &&
         sophia_ip_path(p) == SOPHIA_IPP_ERROR);
  image_close(&t);
  assert(fd_count() == base);
}

static void refused_submit_and_busy(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ic_metadata m = image(50);
  struct sophia_ip *p;
  struct sophia_ip_offer_info info;
  struct sophia_lf_record demand;
  uint64_t offer, id;
  enum sophia_lc_submission stage;
  unsigned i, base = fd_count();
  int src, r;
  image_start(&t, 2, &o);
  p = image_ready(&t);
  /* A refused submit journals nothing: the offer is refused. */
  t.r->peer.submit_error = 22;
  src = sealed(16, 16);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_OK);
  for (i = 0; i < 16; i++) {
    steps(&t, 1);
    assert(!sophia_lc_submission(&t.r->client, &id, &stage, NULL));
    if (stage == SOPHIA_LC_SUBMISSION_REFUSED)
      break;
  }
  assert(stage == SOPHIA_LC_SUBMISSION_REFUSED);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK &&
         info.state == SOPHIA_IPO_REFUSED);
  assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
  steps(&t, 4);
  /* Any submission in flight makes begin BUSY and keeps the fd. */
  memset(&demand, 0, sizeof(demand));
  demand.header.kind = SOPHIA_LF_FRAME_DEMAND;
  demand.value.frame_demand.transaction = 90;
  demand.value.frame_demand.lock_epoch = 1;
  demand.value.frame_demand.allocation = 41;
  demand.value.frame_demand.allocation_generation = 4;
  demand.value.frame_demand.demand = 1;
  for (i = 0; i < 16 && (r = sophia_lc_submit(&t.r->client, &demand)) ==
                            SOPHIA_9P_BUSY;
       i++)
    steps(&t, 1);
  assert(!r);
  src = sealed(16, 16);
  m = image(51);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_BUSY);
  assert(fcntl(src, F_GETFD) >= 0);
  close(src);
  /* Not READY: CLOSED before the producer starts. */
  image_close(&t);
  assert(fd_count() == base);
}

static void selection_one_is_unavailable(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ic_metadata m = image(70);
  struct sophia_ip *p;
  uint64_t offer;
  unsigned base = fd_count();
  int src;
  image_start(&t, 1, &o);
  src = sealed(16, 16);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_CLOSED);
  settle(&t);
  steps(&t, 4);
  assert(sophia_lc_ready(&t.r->client) &&
         sophia_lc_welcome(&t.r->client)->selected_revision == 1 &&
         sophia_lc_welcome(&t.r->client)->granted_capabilities == CAPS);
  p = sophia_lc_image_producer(&t.img);
  assert(p && sophia_ip_cause(p) == SOPHIA_IPX_UNAVAILABLE &&
         sophia_ip_path(p) == SOPHIA_IPP_ERROR);
  assert(!t.r->peer.image_walks[0]);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_CLOSED);
  close(src);
  /* An image event at selection 1 is invalid even with the adapter. */
  peer_channel(&t.r->peer, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_SEALED);
  assert(failure(&t) == SOPHIA_9P_INVALID);
  image_close(&t);
  /* Selection 2 without a sealed grant is unavailable too, and an image
   * event is still invalid. */
  image_start(&t, 2, &o);
  t.r->peer.force_grant = CAPS;
  settle(&t);
  steps(&t, 4);
  assert(sophia_lc_welcome(&t.r->client)->selected_revision == 2);
  p = sophia_lc_image_producer(&t.img);
  assert(p && sophia_ip_cause(p) == SOPHIA_IPX_UNAVAILABLE &&
         !t.r->peer.image_walks[0]);
  peer_channel(&t.r->peer, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_SEALED);
  assert(failure(&t) == SOPHIA_9P_INVALID);
  image_close(&t);
  /* The same closure remains an error path on another connection. */
  image_start(&t, 1, &o);
  settle(&t);
  steps(&t, 4);
  assert(sophia_ip_path(sophia_lc_image_producer(&t.img)) == SOPHIA_IPP_ERROR);
  image_close(&t);
  assert(fd_count() == base);
}

static void discovery_outcomes(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ip *p;
  unsigned base = fd_count();
  /* ENOENT on image-channel alone is absent. */
  image_start(&t, 2, &o);
  t.r->peer.image_error[0] = ENOENT;
  settle(&t);
  steps(&t, 48);
  p = sophia_lc_image_producer(&t.img);
  assert(p && sophia_ip_cause(p) == SOPHIA_IPX_ABSENT &&
         sophia_ip_path(p) == SOPHIA_IPP_ERROR && sophia_lc_ready(&t.r->client));
  image_close(&t);
  /* A missing companion or a denied channel file is a discovery error. */
  image_start(&t, 2, &o);
  t.r->peer.image_error[2] = ENOENT;
  settle(&t);
  steps(&t, 48);
  assert(sophia_ip_cause(sophia_lc_image_producer(&t.img)) == SOPHIA_IPX_DISCOVERY);
  image_close(&t);
  image_start(&t, 2, &o);
  t.r->peer.image_error[0] = EACCES;
  settle(&t);
  steps(&t, 48);
  assert(sophia_ip_cause(sophia_lc_image_producer(&t.img)) == SOPHIA_IPX_DISCOVERY);
  image_close(&t);
  /* EAGAIN backs off and a later pass completes. */
  image_start(&t, 2, &o);
  t.r->peer.image_error[1] = EAGAIN;
  settle(&t);
  {
    unsigned passes = 0, backed = 0, k;
    uint16_t kinds[8];
    for (k = 0; k < 64; k++) {
      uint8_t before;
      assert(!sophia_lc_service(&t.r->client, 65536));
      pump(&t.r->peer);
      (void)app_events(&t, kinds, 8);
      t.now++;
      before = t.img.file;
      assert(sophia_lc_image_dispatch(&t.img, 0, t.now) >= 0);
      /* A new pass is on the wire before the caller polls. */
      if (!before && t.img.file == 1) {
        passes++;
        assert(sophia_9p_wants_write(&t.r->wire));
      }
      if (!t.img.file && t.img.retry_at > t.now)
        backed++;
    }
    assert(passes >= 2 && passes < 16 && backed);
  }
  p = sophia_lc_image_producer(&t.img);
  assert(p && sophia_ip_channel(p) == SOPHIA_IPC_IDLE &&
         sophia_lc_image_timeout(&t.img, t.now) >= 0);
  t.r->peer.image_error[1] = 0;
  steps(&t, 300);
  assert(sophia_ip_channel(p) == SOPHIA_IPC_HELLO_SENT);
  image_close(&t);
  /* Formats storage larger than the budget reads no more than it. */
  image_start(&t, 2, &o);
  {
    static uint8_t wide[SOPHIA_IC_MAX_FORMATS_BYTES + 64],
        served[SOPHIA_IC_MAX_FORMATS_BYTES + 1];
    unsigned i;
    memset(wide, 0xa5, sizeof(wide));
    memcpy(served, t.set.formats, t.set.formats_bytes);
    t.img.config.formats = wide;
    t.img.config.formats_capacity = sizeof(wide);
    t.r->peer.image_data[2] = served;
    t.r->peer.image_bytes[2] = sizeof(served);
    for (i = 0; i < 400 && (!sophia_lc_image_producer(&t.img) ||
                            sophia_ip_cause(sophia_lc_image_producer(&t.img)) !=
                                SOPHIA_IPX_DISCOVERY);
         i++)
      steps(&t, 1);
    assert(sophia_ip_cause(sophia_lc_image_producer(&t.img)) ==
           SOPHIA_IPX_DISCOVERY);
    for (i = SOPHIA_IC_MAX_FORMATS_BYTES; i < sizeof(wide); i++)
      assert(wide[i] == 0xa5);
  }
  image_close(&t);
  assert(fd_count() == base);
}

static void channel_closed_and_contradictions(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ic_record r;
  struct sophia_ip *p;
  unsigned base = fd_count();
  /* ChannelClosed disables descriptors; the lock client stays READY. */
  image_start(&t, 2, &o);
  p = image_ready(&t);
  peer_channel(&t.r->peer, SOPHIA_IC_CHANNEL_CLOSED, 1);
  steps(&t, 4);
  assert(sophia_ip_channel(p) == SOPHIA_IPC_CLOSED && sophia_lc_ready(&t.r->client));
  image_close(&t);
  /* An image event of another epoch fails the client. */
  image_start(&t, 2, &o);
  image_ready(&t);
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_IC_CHANNEL_CLOSED;
  r.value.channel.channel_generation = GEN;
  r.value.channel.value = 1;
  image_record(&t.r->peer, &r);
  put(t.r->peer.journal + t.r->peer.journal_size - 16 - 24, EPOCH + 1, 8);
  assert(failure(&t) == SOPHIA_9P_INVALID);
  image_close(&t);
  /* An image event repeating a sequence fails the client. */
  image_start(&t, 2, &o);
  image_ready(&t);
  steps(&t, 4);
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_IC_CHANNEL_CLOSED;
  r.value.channel.channel_generation = GEN;
  r.value.channel.value = 1;
  image_record(&t.r->peer, &r);
  put(t.r->peer.journal + t.r->peer.journal_size - 16 - 8,
      t.r->peer.sequence - 1, 8);
  assert(failure(&t) == SOPHIA_9P_INVALID);
  image_close(&t);
  /* ImageBegin and the image objects are never journal events. */
  image_start(&t, 2, &o);
  image_ready(&t);
  {
    struct peer *q = &t.r->peer;
    size_t n;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_IMAGE_BEGIN;
    r.header.epoch = EPOCH;
    r.header.submission = 1;
    r.value.image = image(1);
    r.value.image.offer = 1;
    assert(sophia_ic_record_encode(q->journal + q->journal_size,
                                   sizeof(q->journal) - q->journal_size, &r,
                                   &n) == SOPHIA_IC_OK);
    /* As an event: no submission, the next sequence. */
    put(q->journal + q->journal_size + 16, 0, 8);
    put(q->journal + q->journal_size + 24, ++q->sequence, 8);
    q->journal_size += n;
  }
  assert(failure(&t) == SOPHIA_9P_INVALID);
  image_close(&t);
  assert(fd_count() == base);
}

/* A terminal client ends the journal. An ImageBegin never sent is refused;
 * one possibly sent is left to the producer as journal loss. */
static void journal_loss(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ic_metadata m = image(80);
  struct sophia_ip *p;
  struct sophia_ip_offer_info info;
  uint64_t offer, id;
  enum sophia_lc_submission stage;
  unsigned i, base = fd_count();
  int src;
  /* EAGAIN transferred nothing; the connection then dies unsent. */
  image_start(&t, 2, &o);
  p = image_ready(&t);
  t.r->peer.submit_again = 1;
  src = sealed(16, 16);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_OK);
  for (i = 0; i < 16; i++) {
    struct sophia_lc_obligations ob;
    steps(&t, 1);
    assert(!sophia_lc_obligations(&t.r->client, &ob));
    if (ob.waiting_for_retry)
      break;
  }
  assert(i < 16);
  assert(!sophia_lc_submission(&t.r->client, &id, &stage, NULL) &&
         stage == SOPHIA_LC_SUBMISSION_STAGED);
  shutdown(t.r->peer.fd, SHUT_RDWR);
  for (i = 0; i < 8 && !t.r->client.terminal; i++)
    (void)image_step(&t);
  assert(t.r->client.terminal);
  (void)image_step(&t);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK &&
         info.state == SOPHIA_IPO_REFUSED);
  assert(sophia_ip_cause(p) == SOPHIA_IPX_JOURNAL_LOST);
  image_close(&t);
  /* A submit the client queued but never saw answered: journal loss. */
  image_start(&t, 2, &o);
  p = image_ready(&t);
  src = sealed(16, 16);
  m = image(81);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_OK);
  for (i = 0; i < 8; i++) {
    assert(!sophia_lc_service(&t.r->client, 65536));
    assert(!sophia_lc_submission(&t.r->client, &id, &stage, NULL));
    /* Stop before the peer answers the submit. */
    if (stage == SOPHIA_LC_SUBMISSION_ISSUED)
      break;
    pump(&t.r->peer);
  }
  assert(stage == SOPHIA_LC_SUBMISSION_ISSUED);
  shutdown(t.r->peer.fd, SHUT_RDWR);
  for (i = 0; i < 8 && !t.r->client.terminal; i++) {
    (void)sophia_lc_service(&t.r->client, 65536);
    t.now++;
    (void)sophia_lc_image_dispatch(&t.img, 0, t.now);
  }
  assert(t.r->client.terminal);
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK &&
         info.state == SOPHIA_IPO_UNKNOWN);
  assert(sophia_ip_cause(p) == SOPHIA_IPX_JOURNAL_LOST);
  image_close(&t);
  assert(fd_count() == base);
}

static void peer_admitted(struct image_rig *t) {
  struct sophia_ic_record r;
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
  r.value.image = t->r->peer.image_begin.value.image;
  image_record(&t->r->peer, &r);
}

/* Steps until the latest submission leaves the client. */
static enum sophia_lc_submission submission_end(struct image_rig *t) {
  enum sophia_lc_submission stage;
  uint64_t id;
  unsigned i;
  for (i = 0; i < 16; i++) {
    steps(t, 1);
    assert(!sophia_lc_submission(&t->r->client, &id, &stage, NULL));
    if (stage == SOPHIA_LC_SUBMISSION_CUSTODIED ||
        stage == SOPHIA_LC_SUBMISSION_REFUSED)
      break;
  }
  return stage;
}

static struct sophia_ip_offer_info offer_info(struct sophia_ip *p, uint64_t offer) {
  struct sophia_ip_offer_info info;
  assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK);
  return info;
}

/* Cancels once the client's previous submission has fully left it. */
static int cancel_when_idle(struct image_rig *t, uint64_t offer) {
  unsigned i;
  int r = SOPHIA_IPR_BUSY;
  for (i = 0; i < 16 && (r = sophia_lc_image_cancel(&t->img, offer, t->now)) ==
                            SOPHIA_IPR_BUSY;
       i++)
    steps(t, 1);
  return r;
}

static void cancellation(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ic_metadata m = image(120);
  struct sophia_ip *p;
  struct sophia_ip_offer_info info;
  uint8_t b[256];
  uint64_t offer, second;
  unsigned i, held, base = fd_count();
  int src, fd;
  image_start(&t, 2, &o);
  assert(sophia_lc_image_cancel(&t.img, 1, t.now) == SOPHIA_IPR_CLOSED);
  p = image_ready(&t);
  assert(sophia_lc_image_cancel(NULL, 1, t.now) == SOPHIA_IPR_ARGUMENT);
  assert(sophia_lc_image_cancel(&t.img, 1, t.now) == SOPHIA_IPR_ARGUMENT);
  /* ImageBegin in flight: BUSY, and the offer is not marked. */
  src = sealed(16, 16);
  assert(sophia_lc_image_begin(&t.img, &m, src, t.now, &offer) == SOPHIA_IPR_OK);
  assert(sophia_lc_image_cancel(&t.img, offer, t.now) == SOPHIA_IPR_BUSY);
  assert(!offer_info(p, offer).cancel_requested);
  assert(submission_end(&t) == SOPHIA_LC_SUBMISSION_CUSTODIED);
  /* Before admission: one ResourceCancel naming the offer's transaction. */
  assert(cancel_when_idle(&t, offer) == SOPHIA_IPR_OK);
  /* The cancel is on the wire before the caller polls. */
  assert(sophia_9p_wants_write(&t.r->wire));
  assert(offer_info(p, offer).cancel_requested);
  assert(sophia_lc_image_cancel(&t.img, offer, t.now) == SOPHIA_IPR_BUSY);
  assert(submission_end(&t) == SOPHIA_LC_SUBMISSION_CUSTODIED);
  assert(t.r->peer.image_cancels == 1 &&
         t.r->peer.image_cancel.transaction == offer &&
         t.r->peer.image_cancel.resource.id == 120 &&
         t.r->peer.image_cancel.resource.generation == 1 &&
         !t.r->peer.image_cancel.total_bytes);
  assert(cancel_when_idle(&t, offer) == SOPHIA_IPR_REFUSED);
  /* Admitted afterward: no source; the fd is held until SourceReleased. */
  peer_admitted(&t);
  steps(&t, 8);
  assert(owner_receive(&t, b, sizeof(b), &fd) < 0 && errno == EAGAIN);
  assert(offer_info(p, offer).state == SOPHIA_IPO_ADMITTED);
  held = fd_count();
  peer_status(&t.r->peer, offer, 120, SOPHIA_LF_CANCELLED, 0);
  steps(&t, 4);
  info = offer_info(p, offer);
  assert(info.state == SOPHIA_IPO_ADMITTED && info.status == SOPHIA_IPS_CANCELLED);
  assert(fd_count() == held);
  peer_released(&t.r->peer, offer, 120);
  steps(&t, 4);
  assert(offer_info(p, offer).state == SOPHIA_IPO_SETTLED);
  assert(fd_count() == held - 1);
  assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
  /* Acceptance wins the race; the release still settles it. */
  begin_custodied(&t, 121, &second);
  peer_admitted(&t);
  for (i = 0; i < 8 && owner_receive(&t, b, sizeof(b), &fd) < 0; i++)
    steps(&t, 1);
  assert(fd >= 0);
  close(fd);
  assert(cancel_when_idle(&t, second) == SOPHIA_IPR_OK);
  assert(submission_end(&t) == SOPHIA_LC_SUBMISSION_CUSTODIED);
  peer_status(&t.r->peer, second, 121, SOPHIA_LF_ACCEPTED, 0);
  peer_released(&t.r->peer, second, 121);
  steps(&t, 6);
  info = offer_info(p, second);
  assert(info.state == SOPHIA_IPO_SETTLED && info.status == SOPHIA_IPS_ACCEPTED &&
         info.cancel_requested);
  assert(sophia_ip_forget(p, second) == SOPHIA_IPR_OK);
  /* A refused cancel keeps the mark and is not an ImageBegin refusal. */
  begin_custodied(&t, 122, &offer);
  t.r->peer.submit_error = 22;
  assert(cancel_when_idle(&t, offer) == SOPHIA_IPR_OK);
  assert(submission_end(&t) == SOPHIA_LC_SUBMISSION_REFUSED);
  assert(sophia_lc_ready(&t.r->client));
  info = offer_info(p, offer);
  assert(info.state == SOPHIA_IPO_BEGUN && info.cancel_requested);
  peer_admitted(&t);
  steps(&t, 8);
  assert(owner_receive(&t, b, sizeof(b), &fd) < 0 && errno == EAGAIN);
  assert(offer_info(p, offer).state == SOPHIA_IPO_ADMITTED);
  peer_released(&t.r->peer, offer, 122);
  steps(&t, 4);
  assert(offer_info(p, offer).state == SOPHIA_IPO_SETTLED);
  assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
  /* An owner-initiated Cancelled routes without a local mark. */
  begin_custodied(&t, 123, &offer);
  peer_status(&t.r->peer, offer, 123, SOPHIA_LF_CANCELLED, 0);
  steps(&t, 4);
  info = offer_info(p, offer);
  assert(info.state == SOPHIA_IPO_REFUSED && info.status == SOPHIA_IPS_CANCELLED &&
         !info.cancel_requested);
  assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
  assert(sophia_ip_channel(p) == SOPHIA_IPC_READY);
  image_close(&t);
  assert(fd_count() == base);
}


static void cancel_journal_loss(void) {
  struct image_rig t;
  struct sophia_lf_negotiate o = image_offer(1, 2, CAPS | SOPHIA_LF_CAP_IMAGE_SEALED);
  struct sophia_ip *p;
  uint64_t offer, id;
  enum sophia_lc_submission stage;
  unsigned i, base = fd_count();
  image_start(&t, 2, &o);
  p = image_ready(&t);
  begin_custodied(&t, 140, &offer);
  peer_admitted(&t);
  assert(cancel_when_idle(&t, offer) == SOPHIA_IPR_OK);
  for (i = 0; i < 8; i++) {
    assert(!sophia_lc_service(&t.r->client, 65536));
    assert(!sophia_lc_submission(&t.r->client, &id, &stage, NULL));
    if (stage == SOPHIA_LC_SUBMISSION_ISSUED)
      break;
    pump(&t.r->peer);
  }
  assert(stage == SOPHIA_LC_SUBMISSION_ISSUED);
  shutdown(t.r->peer.fd, SHUT_RDWR);
  for (i = 0; i < 8 && !t.r->client.terminal; i++) {
    (void)sophia_lc_service(&t.r->client, 65536);
    t.now++;
    (void)sophia_lc_image_dispatch(&t.img, 0, t.now);
  }
  assert(t.r->client.terminal);
  assert(offer_info(p, offer).state == SOPHIA_IPO_UNKNOWN);
  assert(sophia_ip_cause(p) == SOPHIA_IPX_JOURNAL_LOST);
  assert(sophia_lc_image_cancel(&t.img, offer, t.now) == SOPHIA_IPR_CLOSED);
  image_close(&t);
  assert(fd_count() == base);
}

int main(void) {
  offers_and_attach();
  explicit_selection();
  sealed_offer_end_to_end();
  status_routing();
  refused_submit_and_busy();
  selection_one_is_unavailable();
  discovery_outcomes();
  channel_closed_and_contradictions();
  journal_loss();
  cancellation();
  cancel_journal_loss();
  puts("lock_image_test ok");
  return 0;
}
