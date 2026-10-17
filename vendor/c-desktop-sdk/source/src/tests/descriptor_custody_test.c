/* Proposed x6jp929h custody cases: seals, ancillary ownership, expiry,
 * cancellation, channel/journal/epoch loss, capacity and sealed backing.
 * Fixture scope is stated in image_lifecycle_fixture.h: real SEQPACKET,
 * SCM_RIGHTS and memfd seals; modelled owner, journal and fake GPU custody. */
#include "image_lifecycle_producer.h"

#ifndef F_SEAL_FUTURE_WRITE
#define F_SEAL_FUTURE_WRITE 0x0010
#endif

static void start(struct owner *o, struct producer *p, int *sock, unsigned capacity) {
  owner_init(o, 1, 2, capacity);
  producer_init(p, 100000, 1);
  *sock = connect_ready(o, p);
}

static ssize_t raw_source(int sock, const unsigned char meta[META_BYTES], const int *fds,
                          unsigned count) {
  unsigned char b[SOURCE_BYTES];
  source_encode(b, 1, 2, meta);
  return send_packet(sock, b, sizeof(b), fds, count);
}

static unsigned released(const struct producer *p) { return p->settles; }

static void kernel_seals(void) {
  int sets[6] = {F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL,
                 F_SEAL_WRITE | F_SEAL_SHRINK | F_SEAL_SEAL,
                 F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SEAL,
                 F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK,
                 F_SEAL_FUTURE_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL,
                 REQUIRED_SEALS};
  unsigned i;
  for (i = 0; i < 6; i++) {
    struct owner o;
    struct producer p;
    int sock, fd = sealed_image(64, i == 5 ? 63 : 64, sets[i], 255); /* last: wrong size */
    uint64_t id;
    start(&o, &p, &sock, 64);
    id = admit(&o, &p, 40, fd, 0);
    assert(producer_send(&p, id, 0) == 0);
    assert(owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 1)) {}
    producer_pump(&p, &o);
    assert(producer_find(&p, id)->status == ST_REJECTED);
    assert(producer_find(&p, id)->disposition == DISP_BEFORE_USE && released(&p) == 1);
    assert(!o.backings[0].live);
    close(sock);
    close(fd);
    owner_end_epoch(&o);
  }
  { /* Sealed but not premultiplied: one pixel whose colour exceeds alpha. */
    struct owner o;
    struct producer p;
    int sock, fd = memfd_create("t337-unpremultiplied", MFD_ALLOW_SEALING | MFD_CLOEXEC);
    unsigned char good[4] = {10, 10, 10, 255}, bad[4] = {255, 0, 0, 0};
    unsigned k;
    uint64_t id;
    assert(fd >= 0);
    for (k = 0; k < 64 * 64; k++) assert(write(fd, k == 64 * 64 - 1 ? bad : good, 4) == 4);
    assert(fcntl(fd, F_ADD_SEALS, REQUIRED_SEALS) == 0);
    start(&o, &p, &sock, 64);
    id = admit(&o, &p, 40, fd, 0);
    assert(producer_send(&p, id, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 1)) {}
    producer_pump(&p, &o);
    assert(producer_find(&p, id)->status == ST_REJECTED && !o.backings[0].live);
    close(sock);
    close(fd);
    owner_end_epoch(&o);
  }
  { /* Producer side: a shared writable mapping blocks F_SEAL_WRITE, and the
       receiver sees only the missing seal, never the producer's mapping. */
    struct owner o;
    struct producer p;
    int sock, fd = sealed_image(64, 64, 0, 255);
    void *w = mmap(0, 64 * 64 * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    uint64_t id;
    assert(w != MAP_FAILED);
    assert(fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE) == -1 && errno == EBUSY);
    start(&o, &p, &sock, 64);
    id = admit(&o, &p, 40, fd, 0);
    assert(producer_send(&p, id, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 1)) {}
    producer_pump(&p, &o);
    assert(producer_find(&p, id)->status == ST_REJECTED);
    munmap(w, 64 * 64 * 4);
    close(sock);
    close(fd);
    owner_end_epoch(&o);
  }
}

static void ancillary_ownership(void) {
  unsigned before = fd_count(), i;
  for (i = 0; i < 5; i++) { /* missing, extra, truncated, malformed, changed */
    struct owner o;
    struct producer p;
    int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255), many[8];
    unsigned char b[SOURCE_BYTES];
    uint64_t id;
    unsigned k;
    start(&o, &p, &sock, 64);
    id = admit(&o, &p, 40, fd, 0);
    for (k = 0; k < 8; k++) many[k] = fd;
    source_encode(b, 1, 2, producer_find(&p, id)->meta);
    if (i == 3) put16(b, 4, 2);
    if (i == 4) put64(b, 40 + 16, 2); /* metadata differs from the admitted record */
    assert(send_packet(sock, b, sizeof(b), many, i == 0 ? 0 : i == 1 ? 2 : i == 2 ? 8 : 1) ==
           SOURCE_BYTES);
    assert(owner_receive(&o) == RX_FAULT && o.channel == CH_DISABLED);
    assert(o.offers[0].state == O_FREE); /* the waiting offer is cancelled pre-use */
    producer_pump(&p, &o);
    assert(producer_find(&p, id)->disposition == DISP_CANCELLED && p.channel == PC_CLOSED);
    close(sock);
    close(fd);
    owner_end_epoch(&o);
    assert(fd_count() == before);
  }
  { /* Valid: close-on-exec receipt, and the mapping outlives both closes. */
    struct owner o;
    struct producer p;
    int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
    uint64_t id;
    start(&o, &p, &sock, 64);
    id = admit(&o, &p, 40, fd, 0);
    assert(producer_send(&p, id, 0) == 0);
    close(fd); /* the producer drops its reference after sending */
    assert(owner_receive(&o) == RX_RECEIVED);
    assert(fcntl(o.offers[0].fd, F_GETFD) & FD_CLOEXEC);
    while (owner_work(&o, 1)) {}
    assert(o.backings[0].live && o.backings[0].map[3] == 255);
    producer_pump(&p, &o);
    assert(producer_find(&p, id)->status == ST_ACCEPTED);
    close(sock);
    owner_end_epoch(&o);
    assert(fd_count() == before);
  }
}

static void expiry_race(void) {
  struct owner o;
  struct producer p;
  struct image img;
  unsigned char meta[META_BYTES];
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  uint64_t id;
  unsigned before;
  start(&o, &p, &sock, 64);
  before = fd_count();
  /* Released before arrival: drained and closed, one release only. */
  id = admit(&o, &p, 40, fd, 0);
  assert(producer_send(&p, id, 0) == 0);
  owner_tick(&o, TRANSFER_MS);
  producer_pump(&p, &o);
  assert(producer_find(&p, id)->disposition == DISP_BEFORE_USE && released(&p) == 1);
  assert(owner_receive(&o) == RX_STALE && o.stale_closed == 1 && o.channel == CH_LIVE);
  assert(o.journal.count == 0 && fd_count() == before);
  producer_forget(&p, id);
  /* A skipped lower ID, never admitted, is discarded by the mark. */
  p.next_id = 9;
  id = admit(&o, &p, 41, fd, 0);
  img = image_for(&o, 42, 64, 64);
  img.offer = 5;
  meta_encode(meta, &img);
  assert(raw_source(sock, meta, &fd, 1) == SOURCE_BYTES);
  assert(owner_receive(&o) == RX_STALE && o.stale_closed == 2 && o.channel == CH_LIVE);
  assert(o.journal.count == 0 && fd_count() == before);
  /* Shape precedes the stale lookup: a stale ID carrying two references
   * is a packet-shape fault, not a silent drain. */
  {
    struct owner q;
    struct producer r;
    int qs, two[2];
    uint64_t live;
    start(&q, &r, &qs, 64);
    live = admit(&q, &r, 41, fd, 0);
    q.offers[0].deadline = 0;
    owner_tick(&q, 0); /* the offer expires: its ID is now stale */
    two[0] = two[1] = fd;
    assert(raw_source(qs, producer_find(&r, live)->meta, two, 2) == SOURCE_BYTES);
    assert(owner_receive(&q) == RX_FAULT && q.stale_closed == 0 && q.channel == CH_DISABLED);
    close(qs);
    owner_end_epoch(&q);
  }
  /* Above the mark: unadmitted, so the channel faults. */
  img.offer = id + 1;
  meta_encode(meta, &img);
  assert(raw_source(sock, meta, &fd, 1) == SOURCE_BYTES);
  assert(owner_receive(&o) == RX_FAULT && o.above_mark == 1);
  assert(count_events(&o, EV_CLOSED, 2) == 1);
  producer_pump(&p, &o);
  assert(producer_find(&p, id)->disposition == DISP_CANCELLED && released(&p) == 2);
  assert(p.duplicate_releases == 0);
  assert(fd_count() == before - 1); /* the fault closed only the owner's channel end */
  close(sock);
  close(fd);
  owner_end_epoch(&o);
}

static void cancellation(void) {
  unsigned phase;
  for (phase = 0; phase < 3; phase++) { /* before receipt, after, during validation */
    struct owner o;
    struct producer p;
    int sock, fd = sealed_image(256, 256, REQUIRED_SEALS, 255);
    struct image img;
    uint64_t id;
    unsigned before;
    start(&o, &p, &sock, 64);
    before = fd_count();
    img = image_for(&o, 40, 256, 256);
    assert(producer_begin(&p, &img, fd, 0, &id) == 0);
    assert(owner_begin(&o, producer_find(&p, id)->meta, 0) == B_ADMITTED);
    producer_pump(&p, &o);
    assert(producer_send(&p, id, 0) == 0);
    if (phase > 0) assert(owner_receive(&o) == RX_RECEIVED);
    if (phase > 1) {
      assert(owner_work(&o, 2) == 2);
      assert(o.offers[0].state == O_VALIDATING && o.offers[0].done == CHUNK);
    }
    assert(!producer_may_reuse(&p, id)); /* no early reuse */
    assert(owner_cancel(&o, id) == 0 && owner_cancel(&o, id) == ENOENT);
    assert(count_events(&o, EV_STATUS, id) == 1 && count_events(&o, EV_RELEASED, id) == 1);
    assert(o.journal.reserved == 1); /* only ChannelClosed remains reserved */
    producer_pump(&p, &o);
    assert(producer_find(&p, id)->status == ST_CANCELLED && producer_may_reuse(&p, id));
    assert(producer_find(&p, id)->disposition == DISP_CANCELLED);
    if (phase == 0) { /* the fenced offer's late packet releases nothing */
      assert(owner_receive(&o) == RX_STALE && o.journal.count == 0);
    }
    assert(fd_count() == before && !o.backings[0].live);
    close(sock);
    close(fd);
    owner_end_epoch(&o);
  }
}

static void channel_loss(void) {
  unsigned fallback;
  for (fallback = 0; fallback < 2; fallback++) {
    struct owner o;
    struct producer p;
    int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255), held;
    uint64_t waiting, received, gpu;
    struct image img;
    start(&o, &p, &sock, 64);
    p.fallback_allowed = (int)fallback;
    waiting = admit(&o, &p, 40, fd, 0);
    received = admit(&o, &p, 41, fd, 0);
    gpu = admit(&o, &p, 42, fd, 0);
    assert(producer_send(&p, received, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    assert(producer_send(&p, gpu, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    owner_gpu_submit(&o, gpu);
    held = owner_find(&o, gpu)->fd;
    close(sock); /* the producer's channel end goes away */
    assert(owner_receive(&o) == RX_HANGUP && o.channel == CH_DISABLED);
    /* Interest in every offer is withdrawn; each releases by its custody. */
    assert(count_events(&o, EV_CLOSED, 2) == 1);
    assert(count_events(&o, EV_RELEASED, waiting) == 1);
    assert(count_events(&o, EV_RELEASED, received) == 1);
    assert(count_events(&o, EV_STATUS, gpu) == 1 && count_events(&o, EV_RELEASED, gpu) == 0);
    assert(fcntl(held, F_GETFD) >= 0); /* submitted work keeps its reference */
    { /* ChannelClosed arrives first; an admitted offer awaiting its release
         is never sent after it. */
      struct event e;
      unsigned sends = p.sends;
      uint64_t unsent;
      assert(owner_take(&o, &e) && e.kind == EV_CLOSED);
      producer_event(&p, &e);
      img = image_for(&o, 43, 64, 64);
      assert(producer_begin(&p, &img, fd, 0, &unsent) == -ENODEV);
      assert(producer_find(&p, waiting)->state == P_ADMITTED);
      assert(producer_send(&p, waiting, 0) == -ENODEV && p.sends == sends);
    }
    producer_pump(&p, &o);
    assert(producer_find(&p, waiting)->disposition == DISP_CANCELLED);
    assert(producer_find(&p, received)->status == ST_CANCELLED);
    assert(producer_find(&p, received)->disposition == DISP_CANCELLED);
    assert(!producer_may_reuse(&p, gpu));
    owner_gpu_complete(&o, gpu); /* fake proof of safe cleanup */
    assert(count_events(&o, EV_RELEASED, gpu) == 1 && count_events(&o, EV_STATUS, gpu) == 0);
    producer_pump(&p, &o);
    assert(producer_find(&p, gpu)->disposition == DISP_DRAINED && producer_may_reuse(&p, gpu));
    assert(p.channel == PC_CLOSED);
    assert(producer_path(&p) == (fallback ? PATH_BYTES : PATH_ERROR));
    img = image_for(&o, 44, 64, 64);
    img.offer = 99;
    {
      unsigned char meta[META_BYTES];
      meta_encode(meta, &img);
      assert(owner_begin(&o, meta, 0) == B_UNAVAILABLE);
    }
    assert(o.journal.count == 1); /* draft reasons: shell 13, lock 9 */
    assert(o.journal.events[o.journal.head].reason == (fixture_role == ROLE_SHELL ? 13u : 9u));
    assert(owner_byte_submit(&o) == 0 && o.byte_submits == 1);
    close(fd);
    owner_end_epoch(&o);
  }
}

static void journal_loss(void) {
  struct owner o;
  struct producer p;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  uint64_t id;
  char byte;
  start(&o, &p, &sock, 64);
  /* A channel close is not a release: the received offer stays unsettled
   * until its own journal release. */
  id = admit(&o, &p, 40, fd, 0);
  assert(producer_send(&p, id, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
  owner_channel_close(&o, CLOSE_REVOKED);
  assert(recv(sock, &byte, 1, MSG_DONTWAIT) == 0);
  producer_hangup(&p);
  assert(producer_find(&p, id)->state == P_SENT && !producer_may_reuse(&p, id));
  /* The journal dies before the release is delivered. */
  while (owner_work(&o, 1)) {}
  owner_end_epoch(&o);
  producer_journal_lost(&p);
  assert(producer_find(&p, id)->state == P_UNKNOWN && !producer_may_reuse(&p, id));
  assert(producer_send(&p, id, 0) == -EALREADY);
  assert(producer_begin(&p, 0, fd, 0, &id) == -ENODEV);
  assert(p.settles == 0);
  close(sock);
  close(fd);
}

static void epoch_end(void) {
  struct owner o;
  struct producer p;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255), pipe_fds[2], held;
  uint64_t queued, gpu, plain;
  unsigned emitted, before;
  char byte;
  start(&o, &p, &sock, 64);
  before = fd_count();
  /* Two accepted backings, one still leased by a consumer. */
  for (plain = 30; plain < 32; plain++) {
    queued = admit(&o, &p, plain, fd, 0);
    assert(producer_send(&p, queued, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 4)) {}
    producer_pump(&p, &o);
    producer_forget(&p, queued);
  }
  owner_lease(&o, 30);
  gpu = admit(&o, &p, 40, fd, 0);
  plain = admit(&o, &p, 41, fd, 0);
  queued = admit(&o, &p, 42, fd, 0);
  assert(producer_send(&p, gpu, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
  assert(producer_send(&p, plain, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
  owner_gpu_submit(&o, gpu);
  held = owner_find(&o, gpu)->fd;
  /* A queued, never-received reference: a pipe's only write end. */
  assert(pipe2(pipe_fds, O_CLOEXEC | O_NONBLOCK) == 0);
  assert(raw_source(sock, producer_find(&p, queued)->meta, &pipe_fds[1], 1) == SOURCE_BYTES);
  close(pipe_fds[1]);
  assert(read(pipe_fds[0], &byte, 1) == -1 && errno == EAGAIN);
  producer_pump(&p, &o);
  emitted = o.journal.emitted;
  owner_end_epoch(&o);
  assert(read(pipe_fds[0], &byte, 1) == 0); /* socket teardown released it */
  assert(owner_begin(&o, producer_find(&p, queued)->meta, 0) == B_DEAD);
  assert(fcntl(held, F_GETFD) >= 0); /* GPU-held reference survives */
  owner_gpu_complete(&o, gpu); /* fake proof of safe cleanup */
  assert(fcntl(held, F_GETFD) == -1 && errno == EBADF);
  assert(o.journal.emitted == emitted && owner_live(&o) == 0); /* nothing invented */
  /* Epoch end retired every backing: the unleased one is gone, the leased
   * one lives until its last lease, and neither emits on the dead journal. */
  assert(!owner_backing(&o, 31) && owner_backing(&o, 30));
  owner_unlease(&o, 30);
  assert(!owner_backing(&o, 30) && o.backing_charge == 0 && o.journal.emitted == emitted);
  producer_journal_lost(&p);
  assert(!producer_may_reuse(&p, gpu) && !producer_may_reuse(&p, plain));
  close(pipe_fds[0]);
  close(sock);
  assert(fd_count() == before - 2);
  close(fd);
}

static void capacity(void) {
  struct owner o;
  struct producer p;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  uint64_t ids[MAX_OFFERS], quarantined, extra;
  struct image img;
  unsigned i;
  /* Terminal capacity is reserved before admission: a journal of seven
   * pending slots admits two offers, not four. */
  start(&o, &p, &sock, 7);
  ids[0] = admit(&o, &p, 40, fd, 0);
  ids[1] = admit(&o, &p, 41, fd, 0);
  img = image_for(&o, 42, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &extra) == 0);
  assert(owner_begin(&o, producer_find(&p, extra)->meta, 0) == B_CAPACITY);
  assert(o.mark == ids[1] && owner_live(&o) == 2 && o.journal.reserved == 5);
  close(sock);
  owner_end_epoch(&o);
  /* Four offers fill the slots; a quarantined capture keeps its slot. */
  start(&o, &p, &sock, 64);
  for (i = 0; i < MAX_OFFERS; i++) ids[i] = admit(&o, &p, 40 + i, fd, 0);
  img = image_for(&o, 50, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &extra) == -EBUSY);
  img.offer = ids[3] + 1;
  {
    unsigned char meta[META_BYTES];
    meta_encode(meta, &img);
    assert(owner_begin(&o, meta, 0) == B_CAPACITY && o.mark == ids[3]);
  }
  quarantined = ids[0];
  assert(producer_send(&p, quarantined, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
  owner_gpu_submit(&o, quarantined);
  owner_gpu_timeout(&o, quarantined);
  for (i = 1; i < MAX_OFFERS; i++) assert(owner_cancel(&o, ids[i]) == 0);
  producer_pump(&p, &o);
  assert(producer_find(&p, quarantined)->status == ST_REJECTED);
  assert(!producer_may_reuse(&p, quarantined)); /* rejection is not release */
  for (i = 1; i < MAX_OFFERS; i++) producer_forget(&p, ids[i]);
  for (i = 1; i < MAX_OFFERS; i++) ids[i] = admit(&o, &p, 60 + i, fd, 0);
  img = image_for(&o, 70, 64, 64);
  img.offer = ids[3] + 1;
  {
    unsigned char meta[META_BYTES];
    meta_encode(meta, &img);
    assert(owner_begin(&o, meta, 0) == B_CAPACITY);
    owner_gpu_complete(&o, quarantined);
    assert(count_events(&o, EV_STATUS, quarantined) == 0);
    assert(count_events(&o, EV_RELEASED, quarantined) == 1);
    producer_pump(&p, &o);
    assert(producer_find(&p, quarantined)->disposition == DISP_DRAINED);
    producer_forget(&p, quarantined);
    assert(owner_begin(&o, meta, 0) == B_ADMITTED);
  }
  close(sock);
  close(fd);
  owner_end_epoch(&o);
  /* Sealed backing bytes are reserved at admission and move into the backing
   * on acceptance: four 4 MiB images fill the 16 MiB budget. */
  start(&o, &p, &sock, 64);
  fd = sealed_image(1024, 1024, REQUIRED_SEALS, 255);
  for (i = 0; i < MAX_OFFERS; i++) {
    ids[i] = admit_size(&o, &p, 80 + i, fd, 0, 1024, 1024);
    assert(producer_send(&p, ids[i], 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 64)) {}
    producer_pump(&p, &o);
    producer_forget(&p, ids[i]);
  }
  assert(owner_backings_live(&o) == MAX_OFFERS && o.backing_charge == SEALED_BACKING_BYTES);
  img = image_for(&o, 90, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &extra) == 0);
  assert(owner_begin(&o, producer_find(&p, extra)->meta, 0) == B_CAPACITY);
  producer_refused(&p, extra);
  owner_retire(&o, 80);
  assert(o.backing_charge == SEALED_BACKING_BYTES - (4 << 20));
  extra = admit(&o, &p, 90, fd, 0);
  assert(owner_cancel(&o, extra) == 0 && o.backing_charge == SEALED_BACKING_BYTES - (4 << 20));
  close(sock);
  close(fd);
  owner_end_epoch(&o);
  assert(o.backing_charge == 0 && owner_backings_live(&o) == 0);
  /* Backing slots are reserved too: eight retained images, then refusal. */
  start(&o, &p, &sock, 64);
  fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  for (i = 0; i < BACKING_SLOTS; i++) {
    extra = admit(&o, &p, 100 + i, fd, 0);
    assert(producer_send(&p, extra, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 4)) {}
    producer_pump(&p, &o);
    producer_forget(&p, extra);
  }
  assert(owner_backings_live(&o) == BACKING_SLOTS);
  img = image_for(&o, 120, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &extra) == 0);
  assert(owner_begin(&o, producer_find(&p, extra)->meta, 0) == B_CAPACITY);
  close(sock);
  close(fd);
  owner_end_epoch(&o);
}

static void sealed_backing(void) {
  struct owner o;
  struct producer p;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  uint64_t id, second;
  unsigned char meta[META_BYTES];
  start(&o, &p, &sock, 64);
  id = admit(&o, &p, 40, fd, 0);
  memcpy(meta, producer_find(&p, id)->meta, META_BYTES);
  assert(producer_send(&p, id, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
  while (owner_work(&o, 1)) {}
  producer_pump(&p, &o);
  assert(producer_find(&p, id)->disposition == DISP_RETAINED);
  /* Release does not make the source writable. */
  assert((fcntl(fd, F_GET_SEALS) & REQUIRED_SEALS) == REQUIRED_SEALS);
  assert(write(fd, "x", 1) == -1 && errno == EPERM);
  assert(ftruncate(fd, 0) == -1 && errno == EPERM);
  /* Retirement waits for the last consumer lease. */
  owner_lease(&o, 40);
  owner_lease(&o, 40);
  owner_retire(&o, 40);
  assert(count_events(&o, EV_RES_RELEASED, 40) == 0);
  /* The offer ended at SourceReleased even though the backing lives: a late
   * Source for it drains by the mark, without fault or second release. */
  assert(raw_source(sock, meta, &fd, 1) == SOURCE_BYTES);
  assert(owner_receive(&o) == RX_STALE && o.channel == CH_LIVE && o.journal.count == 0);
  owner_unlease(&o, 40);
  assert(count_events(&o, EV_RES_RELEASED, 40) == 0);
  owner_unlease(&o, 40);
  assert(count_events(&o, EV_RES_RELEASED, 40) == 1 && !owner_backing(&o, 40));
  producer_pump(&p, &o);
  assert(p.settles == 1 && p.duplicate_releases == 0);
  { /* A duplicated SourceReleased delivery settles nothing a second time. */
    struct event again;
    uint64_t other = admit(&o, &p, 42, fd, 0);
    memset(&again, 0, sizeof(again));
    again.kind = EV_RELEASED;
    again.id = id;
    again.value = DISP_CANCELLED;
    producer_event(&p, &again);
    assert(p.settles == 1 && p.duplicate_releases == 1);
    assert(producer_find(&p, id)->disposition == DISP_RETAINED);
    assert(!producer_may_reuse(&p, other));
    assert(owner_cancel(&o, other) == 0);
    producer_pump(&p, &o);
    assert(p.settles == 2 && producer_may_reuse(&p, other));
  }
  { /* A full live journal defers ResourceReleased; it is recorded exactly
       once when room returns, and the backing keeps its charge until then. */
    uint64_t r = admit(&o, &p, 50, fd, 0);
    struct event e;
    unsigned seen = 0;
    assert(producer_send(&p, r, 0) == 0 && owner_receive(&o) == RX_RECEIVED);
    while (owner_work(&o, 4)) {}
    o.journal.capacity = o.journal.count + o.journal.reserved; /* no free slot */
    owner_retire(&o, 50);
    assert(count_events(&o, EV_RES_RELEASED, 50) == 0);
    assert(owner_backing(&o, 50) && o.backing_charge == 64 * 64 * 4);
    while (owner_take(&o, &e)) {
      seen += e.kind == EV_RES_RELEASED && e.id == 50;
      producer_event(&p, &e);
    }
    assert(seen == 1 && !owner_backing(&o, 50) && o.backing_charge == 0);
    o.journal.capacity = 64;
    producer_forget(&p, r);
  }
  /* A second Source while the offer is still live is a protocol fault. */
  second = admit(&o, &p, 41, fd, 0);
  assert(raw_source(sock, producer_find(&p, second)->meta, &fd, 1) == SOURCE_BYTES);
  assert(raw_source(sock, producer_find(&p, second)->meta, &fd, 1) == SOURCE_BYTES);
  assert(owner_receive(&o) == RX_RECEIVED && owner_receive(&o) == RX_FAULT);
  /* The duplicate never replaced the first source; the fault withdrew the
   * offer, which released once by its own custody. */
  assert(count_events(&o, EV_STATUS, second) == 1 && count_events(&o, EV_RELEASED, second) == 1);
  assert(owner_live(&o) == 0);
  close(sock);
  close(fd);
  owner_end_epoch(&o);
}

int main(void) {
  unsigned before = fd_count();
  for (fixture_role = ROLE_SHELL; fixture_role <= ROLE_LOCK; fixture_role++) {
    kernel_seals();
    ancillary_ownership();
    expiry_race();
    cancellation();
    channel_loss();
    journal_loss();
    epoch_end();
    capacity();
    sealed_backing();
    assert(fd_count() == before);
  }
  puts("descriptor custody fixture: 9 cases passed for shell and lock roles");
  return 0;
}
