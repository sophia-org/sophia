/* Test-only lifecycle and custody fixture for proposed Sophia x6jp929h.
 *
 * Two independent state machines exchange real SEQPACKET packets carrying real
 * SCM_RIGHTS references to real sealable memfds. The owner model represents
 * Sophia for state transitions only. It is not an authenticated production
 * endpoint: its credential check compares SO_PEERCRED with this test process,
 * and nothing here proves supervised launch custody. Journal events are
 * structured values; their record bytes are covered by
 * descriptor_draft_codec_test. "GPU work" is a fake completion signal that
 * tests transitions only and proves nothing about real GPU reads.
 *
 * Both models are clockless. Callers supply the time in milliseconds and the
 * socket readiness; accessors expose deadlines and writable interest. */
#ifndef IMAGE_LIFECYCLE_FIXTURE_H
#define IMAGE_LIFECYCLE_FIXTURE_H

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

#define FX static inline

enum { HELLO_BYTES = 40, SOURCE_BYTES = 200, META_BYTES = 96 };
enum { KIND_HELLO = 1, KIND_SOURCE = 2, ROLE_SHELL = 1, ROLE_LOCK = 2, MAX_OFFERS = 4 };

/* The role every packet, discovery object and refusal uses in this run. */
static uint16_t fixture_role = ROLE_SHELL;
enum { CHUNK = 65536, MAX_IMAGE_BYTES = 4 << 20, TRANSFER_MS = 2000 };
enum { SEALED_BACKING_BYTES = 16 << 20, BACKING_SLOTS = 8 };
#define REQUIRED_SEALS (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)

/* Journal vocabulary. Dispositions and close reasons are the draft's values;
 * status values are fixture labels for ResourceStatus's existing meanings. */
enum { EV_READY = 1, EV_CLOSED, EV_ADMITTED, EV_STATUS, EV_RELEASED, EV_RES_RELEASED };
enum { ST_ACCEPTED = 1, ST_REJECTED, ST_CANCELLED };
/* Proposed image_channel_unavailable: shell ResourceStatus reason 13 at r9,
 * lock reason 9 at r2. */
FX uint32_t channel_unavailable_reason(void) { return fixture_role == ROLE_SHELL ? 13 : 9; }
enum { DISP_RETAINED = 1, DISP_CAPTURED, DISP_BEFORE_USE, DISP_CANCELLED, DISP_DRAINED };
enum { CLOSE_PEER = 1, CLOSE_PROTOCOL, CLOSE_HELLO_TIMEOUT, CLOSE_REVOKED };

FX void put16(unsigned char *b, size_t at, uint16_t v) { b[at] = v & 255; b[at + 1] = v >> 8; }
FX void put32(unsigned char *b, size_t at, uint32_t v) {
  unsigned i;
  for (i = 0; i < 4; i++) b[at + i] = (unsigned char)(v >> (8 * i));
}
FX void put64(unsigned char *b, size_t at, uint64_t v) {
  unsigned i;
  for (i = 0; i < 8; i++) b[at + i] = (unsigned char)(v >> (8 * i));
}
FX uint64_t get(const unsigned char *b, size_t at, unsigned width) {
  uint64_t v = 0;
  while (width--) v = (v << 8) | b[at + width];
  return v;
}

/* ---- Metadata and packets ------------------------------------------------ */

struct image {
  uint64_t offer, resource, generation, grant, output, allocation, topology;
  uint32_t width, height;
};

FX void meta_encode(unsigned char m[META_BYTES], const struct image *i) {
  memset(m, 0, META_BYTES);
  put64(m, 0, i->offer);
  put64(m, 8, i->resource);
  put64(m, 16, i->generation);
  put64(m, 24, i->grant);
  put64(m, 32, i->output);
  put64(m, 40, i->allocation);
  put64(m, 48, i->topology);
  put32(m, 56, 1);
  put32(m, 60, 1);
  put32(m, 64, i->width);
  put32(m, 68, i->height);
  put32(m, 72, i->width * 4);
  put16(m, 76, 1);
  put16(m, 78, 1);
  put64(m, 80, (uint64_t)i->width * i->height * 4);
}

FX void header_encode(unsigned char *p, uint32_t total, uint16_t kind, uint16_t fds,
                      uint64_t epoch, uint64_t gen, uint64_t offer) {
  memset(p, 0, total);
  put32(p, 0, total);
  put16(p, 4, 1);
  put16(p, 6, kind);
  put16(p, 8, fixture_role);
  put16(p, 10, fds);
  put64(p, 16, epoch);
  put64(p, 24, gen);
  put64(p, 32, offer);
}

FX void source_encode(unsigned char p[SOURCE_BYTES], uint64_t epoch, uint64_t gen,
                      const unsigned char meta[META_BYTES]) {
  header_encode(p, SOURCE_BYTES, KIND_SOURCE, 1, epoch, gen, get(meta, 0, 8));
  memcpy(p + 40, meta, META_BYTES);
  put32(p, 136 + 8, (uint32_t)get(meta, 72, 4)); /* row 0: offset 0, fd_index 0 */
}

FX ssize_t send_packet(int socket, const void *bytes, size_t length, const int *fds,
                       unsigned count) {
  union { struct cmsghdr align; unsigned char b[CMSG_SPACE(8 * sizeof(int))]; } c;
  struct iovec io;
  struct msghdr m;
  struct cmsghdr *h;
  assert(count <= 8);
  memset(&m, 0, sizeof(m));
  memset(&c, 0, sizeof(c));
  io.iov_base = (void *)bytes;
  io.iov_len = length;
  m.msg_iov = &io;
  m.msg_iovlen = 1;
  if (count) {
    m.msg_control = c.b;
    m.msg_controllen = CMSG_SPACE(count * sizeof(int));
    h = CMSG_FIRSTHDR(&m);
    h->cmsg_level = SOL_SOCKET;
    h->cmsg_type = SCM_RIGHTS;
    h->cmsg_len = CMSG_LEN(count * sizeof(int));
    memcpy(CMSG_DATA(h), fds, count * sizeof(int));
  }
  return sendmsg(socket, &m, MSG_NOSIGNAL | MSG_DONTWAIT);
}

/* Receives one packet with close-on-exec references and room for one more
 * reference than any valid packet carries. Returns bytes, or -1 with errno;
 * *bad reports truncation or an unexpected ancillary type. */
FX ssize_t receive_packet(int socket, unsigned char *bytes, size_t capacity, int fds[5],
                          unsigned *count, int *bad) {
  union { struct cmsghdr align; unsigned char b[CMSG_SPACE(5 * sizeof(int))]; } c;
  struct iovec io;
  struct msghdr m;
  struct cmsghdr *h;
  ssize_t got;
  memset(&m, 0, sizeof(m));
  io.iov_base = bytes;
  io.iov_len = capacity;
  m.msg_iov = &io;
  m.msg_iovlen = 1;
  m.msg_control = c.b;
  m.msg_controllen = sizeof(c.b);
  *count = 0;
  *bad = 0;
  got = recvmsg(socket, &m, MSG_CMSG_CLOEXEC | MSG_DONTWAIT);
  if (got < 0) return got;
  for (h = CMSG_FIRSTHDR(&m); h; h = CMSG_NXTHDR(&m, h)) {
    size_t n;
    if (h->cmsg_level != SOL_SOCKET || h->cmsg_type != SCM_RIGHTS || h->cmsg_len < CMSG_LEN(0)) {
      *bad = 1;
      continue;
    }
    size_t j;
    n = (h->cmsg_len - CMSG_LEN(0)) / sizeof(int);
    for (j = 0; j < n; j++) {
      int fd;
      memcpy(&fd, CMSG_DATA(h) + j * sizeof(int), sizeof(int));
      if (*count < 5) {
        fds[(*count)++] = fd;
      } else { /* never keep more than the bound: close and refuse */
        close(fd);
        *bad = 1;
      }
    }
  }
  if (m.msg_flags & (MSG_CTRUNC | MSG_TRUNC)) *bad = 1;
  return got;
}

FX void close_all(const int *fds, unsigned count) {
  while (count--) close(fds[count]);
}

/* ---- Discovery objects --------------------------------------------------- */

struct objects {
  int error; /* 0, or the errno an open/read returned */
  char text[2048];
  size_t text_length;
  unsigned char limits[136];
  unsigned char formats[64];
};

static const char *const DISCOVERY_KEYS[15] = {
    "role", "connection_epoch", "channel_generation", "endpoint", "source_kinds",
    "max_packet_bytes", "max_fds", "max_offers", "max_packets_per_turn",
    "hello_timeout_ms", "validation_timeout_ms", "validation_chunk_bytes",
    "capture_interest_timeout_ms", "formats_generation", "limits_generation"};

FX void record_header(unsigned char *b, uint32_t total, uint16_t kind, uint64_t epoch) {
  memset(b, 0, total);
  put32(b, 0, total);
  put16(b, 4, 1);
  put16(b, 6, kind);
  put64(b, 8, epoch);
}

/* Strict version-1 discovery parse: fixed order, canonical decimals, v1
 * constants. Returns 0 and fills values (endpoint slot left zero) or -1. */
FX int discovery_parse(const char *text, size_t length, uint64_t v[15]) {
  const char *p = text, *end = text + length;
  const char *first = "sophia-image-channel version=1\n";
  unsigned k;
  if (length > 2048 || length < strlen(first) || memcmp(p, first, strlen(first))) return -1;
  p += strlen(first);
  for (k = 0; k < 15; k++) {
    size_t key = strlen(DISCOVERY_KEYS[k]);
    const char *value, *eol;
    if ((size_t)(end - p) < key + 2 || memcmp(p, DISCOVERY_KEYS[k], key) || p[key] != '=')
      return -1;
    value = p + key + 1;
    eol = memchr(value, '\n', (size_t)(end - value));
    if (!eol || eol == value) return -1;
    v[k] = 0;
    if (k == 3) {
      const char *q;
      if (eol - value > 64 || (eol - value == 1 && value[0] == '.') ||
          (eol - value == 2 && value[0] == '.' && value[1] == '.'))
        return -1;
      for (q = value; q < eol; q++)
        if (!((*q >= 'a' && *q <= 'z') || (*q >= 'A' && *q <= 'Z') ||
              (*q >= '0' && *q <= '9') || *q == '_' || *q == '.' || *q == '-'))
          return -1;
    } else {
      const char *q;
      if (eol - value > 20 || (value[0] == '0' && eol - value > 1)) return -1;
      for (q = value; q < eol; q++) {
        if (*q < '0' || *q > '9' || v[k] > (UINT64_MAX - (uint64_t)(*q - '0')) / 10) return -1;
        v[k] = v[k] * 10 + (uint64_t)(*q - '0');
      }
    }
    p = eol + 1;
  }
  if (p != end || v[0] != fixture_role || !v[1] || !v[2] || v[5] != SOURCE_BYTES || v[6] != 4 ||
      !v[7] || v[7] > MAX_OFFERS || v[8] != 1 || !v[4] || v[4] > 3)
    return -1;
  return 0;
}

/* ---- Owner model (represents Sophia; not a production endpoint) ---------- */

enum { CH_NONE, CH_LIVE, CH_DISABLED };
enum { O_FREE, O_WAITING, O_RECEIVED, O_VALIDATING, O_GPU };

struct event {
  int kind;
  uint64_t id;       /* offer ID, channel generation or resource ID */
  uint32_t value;    /* status, disposition, close reason or source-kind mask */
  uint32_t reason;   /* status reason */
  unsigned char meta[META_BYTES];
};

struct journal {
  struct event events[32];
  unsigned head, count, capacity, reserved, emitted;
  int alive;
};

struct offer {
  int state, fd, status_sent, quarantined;
  unsigned char meta[META_BYTES];
  const unsigned char *map;
  size_t size, done;
  uint64_t deadline, charged; /* sealed backing bytes reserved at admission */
};

struct backing {
  uint64_t resource;
  const unsigned char *map;
  size_t size;
  unsigned leases;
  int retired, live, release_pending; /* retired, unleased, awaiting journal room */
};

struct owner {
  uint64_t epoch, gen, grant, topology, formats_gen, limits_gen, mark;
  uint32_t kinds;
  int channel, sock, epoch_live;
  pid_t assignee;
  struct offer offers[MAX_OFFERS];
  struct backing backings[BACKING_SLOTS];
  struct journal journal;
  uint64_t backing_charge; /* reserved offers plus live backings */
  unsigned backing_slots_reserved, stale_closed, above_mark, faults, byte_submits;
};

FX void owner_init(struct owner *o, uint64_t epoch, uint64_t gen, unsigned capacity) {
  unsigned i;
  memset(o, 0, sizeof(*o));
  o->epoch = epoch;
  o->gen = gen;
  o->grant = 6;
  o->topology = 9;
  o->formats_gen = 9;
  o->limits_gen = 1;
  o->kinds = 1;
  o->sock = -1;
  o->epoch_live = 1;
  o->assignee = getpid();
  o->journal.capacity = capacity;
  o->journal.alive = 1;
  for (i = 0; i < MAX_OFFERS; i++) o->offers[i].fd = -1;
}

/* Ordinary events need room outside every reservation; terminal events spend
 * a slot reserved at admission. */
FX int journal_room(const struct journal *j, unsigned ordinary, unsigned reserve) {
  return j->alive && j->count + j->reserved + ordinary + reserve <= j->capacity;
}
FX void journal_push(struct journal *j, const struct event *e, int reserved) {
  assert(j->alive && j->count < 32);
  if (reserved) {
    assert(j->reserved > 0);
    j->reserved--;
  } else {
    assert(j->count + j->reserved < j->capacity);
  }
  j->events[(j->head + j->count) % 32] = *e;
  j->count++;
  j->emitted++;
}
FX void emit(struct owner *o, int kind, uint64_t id, uint32_t value, uint32_t reason,
             int reserved) {
  struct event e;
  memset(&e, 0, sizeof(e));
  e.kind = kind;
  e.id = id;
  e.value = value;
  e.reason = reason;
  if (!o->journal.alive) return;
  journal_push(&o->journal, &e, reserved);
}
FX void owner_settle_pending(struct owner *o);
/* The client reads and acknowledges one event, freeing its slot; a deferred
 * ResourceReleased may then take it. */
FX int owner_take(struct owner *o, struct event *e) {
  struct journal *j = &o->journal;
  if (!j->count) return 0;
  *e = j->events[j->head];
  j->head = (j->head + 1) % 32;
  j->count--;
  owner_settle_pending(o);
  return 1;
}

FX struct offer *owner_find(struct owner *o, uint64_t id) {
  unsigned i;
  for (i = 0; i < MAX_OFFERS; i++)
    if (o->offers[i].state != O_FREE && get(o->offers[i].meta, 0, 8) == id) return &o->offers[i];
  return 0;
}
FX unsigned owner_live(const struct owner *o) {
  unsigned i, n = 0;
  for (i = 0; i < MAX_OFFERS; i++) n += o->offers[i].state != O_FREE;
  return n;
}

FX unsigned owner_backings_live(const struct owner *o) {
  unsigned i, n = 0;
  for (i = 0; i < BACKING_SLOTS; i++) n += o->backings[i].live;
  return n;
}

/* Returns an unaccepted offer's backing reservation; an accepted offer has
 * already moved its charge into the backing, so this is a no-op for it. */
FX void owner_uncharge(struct owner *o, struct offer *f) {
  if (!f->charged) return;
  o->backing_charge -= f->charged;
  o->backing_slots_reserved--;
  f->charged = 0;
}

FX void offer_drop(struct offer *f) {
  if (f->map) munmap((void *)f->map, f->size);
  if (f->fd >= 0) close(f->fd);
  memset(f, 0, sizeof(*f));
  f->fd = -1;
}

/* One terminal ResourceStatus (unless already sent) and exactly one
 * SourceReleased, both from slots reserved at admission. */
FX void owner_terminal(struct owner *o, struct offer *f, uint32_t status, uint32_t disposition) {
  uint64_t id = get(f->meta, 0, 8);
  if (o->journal.alive) {
    if (!f->status_sent) emit(o, EV_STATUS, id, status, 0, 1);
    emit(o, EV_RELEASED, id, disposition, 0, 1);
  }
  owner_uncharge(o, f);
  offer_drop(f);
}

FX int owner_cancel(struct owner *o, uint64_t id);
/* Channel loss withdraws interest in every live offer. Each still ends by its
 * own custody: an unsent or synchronously held source is released at once,
 * submitted work only after proven safe cleanup (disposition 5). */
FX void owner_channel_close(struct owner *o, uint32_t reason) {
  unsigned i;
  if (o->channel != CH_LIVE) return;
  close(o->sock);
  o->sock = -1;
  o->channel = CH_DISABLED;
  if (o->journal.alive) emit(o, EV_CLOSED, o->gen, reason, 0, 1);
  for (i = 0; i < MAX_OFFERS; i++)
    if (o->offers[i].state != O_FREE) owner_cancel(o, get(o->offers[i].meta, 0, 8));
}

FX size_t owner_discovery(const struct owner *o, char *out, size_t capacity) {
  uint64_t v[15];
  size_t n;
  unsigned k;
  v[0] = fixture_role; v[1] = o->epoch; v[2] = o->gen; v[4] = o->kinds; v[5] = SOURCE_BYTES;
  v[6] = 4; v[7] = MAX_OFFERS; v[8] = 1; v[9] = 250; v[10] = 2000; v[11] = CHUNK; v[12] = 500;
  v[13] = o->formats_gen; v[14] = o->limits_gen;
  n = (size_t)snprintf(out, capacity, "sophia-image-channel version=1\n");
  for (k = 0; k < 15; k++)
    n += (size_t)(k == 3 ? snprintf(out + n, capacity - n, "%s=image.sock\n", DISCOVERY_KEYS[k])
                         : snprintf(out + n, capacity - n, "%s=%llu\n", DISCOVERY_KEYS[k],
                                    (unsigned long long)v[k]));
  assert(n < capacity);
  return n;
}

/* Opening a fid snapshots all three objects; reads of that fid never change. */
FX void owner_open(const struct owner *o, struct objects *s) {
  memset(s, 0, sizeof(*s));
  s->text_length = owner_discovery(o, s->text, sizeof(s->text));
  record_header(s->limits, 136, 0x7005, o->epoch);
  put64(s->limits, 32, o->gen);
  put64(s->limits, 40, o->limits_gen);
  record_header(s->formats, 64, 0x7006, o->epoch);
  put64(s->formats, 32, o->gen);
  put64(s->formats, 40, o->formats_gen);
  put64(s->formats, 48, o->limits_gen);
}

enum { A_JOINED, A_REJECTED, A_WAIT };
/* Takes ownership of an incoming channel socket. Rejection closes only that
 * socket and emits nothing: it cannot close or replace a live channel. */
FX int owner_accept(struct owner *o, int sock) {
  unsigned char b[HELLO_BYTES + 1];
  int fds[5], bad;
  unsigned n;
  ssize_t got;
  struct ucred cred;
  socklen_t length = sizeof(cred);
  if (o->channel != CH_NONE || !o->epoch_live) goto reject;
  if (getsockopt(sock, SOL_SOCKET, SO_PEERCRED, &cred, &length) || cred.pid != o->assignee)
    goto reject;
  got = receive_packet(sock, b, sizeof(b), fds, &n, &bad);
  if (got < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) return A_WAIT;
  close_all(fds, n);
  if (got != HELLO_BYTES || n || bad || get(b, 0, 4) != HELLO_BYTES || get(b, 4, 2) != 1 ||
      get(b, 6, 2) != KIND_HELLO || get(b, 8, 2) != fixture_role || get(b, 10, 2) ||
      get(b, 12, 4) || get(b, 16, 8) != o->epoch || get(b, 24, 8) != o->gen || get(b, 32, 8))
    goto reject;
  if (!journal_room(&o->journal, 1, 1)) goto reject;
  o->journal.reserved++; /* ChannelClosed */
  o->sock = sock;
  o->channel = CH_LIVE;
  emit(o, EV_READY, o->gen, o->kinds, 0, 0);
  return A_JOINED;
reject:
  close(sock);
  return A_REJECTED;
}

enum { B_ADMITTED, B_UNAVAILABLE, B_ORDER, B_CAPACITY, B_INVALID, B_DEAD };
FX int owner_begin(struct owner *o, const unsigned char meta[META_BYTES], uint64_t now) {
  uint64_t id = get(meta, 0, 8), w = get(meta, 64, 4), h = get(meta, 68, 4);
  unsigned i;
  struct event e;
  if (!o->epoch_live) return B_DEAD;
  if (o->channel != CH_LIVE) {
    if (journal_room(&o->journal, 1, 0))
      emit(o, EV_STATUS, id, ST_REJECTED, channel_unavailable_reason(), 0);
    return B_UNAVAILABLE;
  }
  if (!id || !get(meta, 8, 8) || !get(meta, 16, 8) || get(meta, 24, 8) != o->grant ||
      !get(meta, 32, 8) || !get(meta, 40, 8) || get(meta, 48, 8) != o->topology ||
      get(meta, 56, 4) != 1 || get(meta, 60, 4) != 1 || !w || !h || w > 4096 || h > 4096 ||
      get(meta, 72, 4) != w * 4 || get(meta, 76, 2) != 1 || get(meta, 78, 2) != 1 ||
      get(meta, 80, 8) != w * h * 4 || w * h * 4 > MAX_IMAGE_BYTES || get(meta, 88, 8))
    return B_INVALID;
  if (id <= o->mark) return B_ORDER; /* the mark advances only on admission */
  for (i = 0; i < MAX_OFFERS && o->offers[i].state != O_FREE; i++) {}
  if (i == MAX_OFFERS || !journal_room(&o->journal, 1, 2) ||
      o->backing_charge + w * h * 4 > SEALED_BACKING_BYTES ||
      owner_backings_live(o) + o->backing_slots_reserved >= BACKING_SLOTS)
    return B_CAPACITY;
  o->mark = id;
  o->backing_charge += w * h * 4; /* sealed backing bytes and a backing slot */
  o->backing_slots_reserved++;
  o->offers[i].charged = w * h * 4;
  o->journal.reserved += 2; /* final ResourceStatus and SourceReleased */
  o->offers[i].state = O_WAITING;
  o->offers[i].fd = -1;
  memcpy(o->offers[i].meta, meta, META_BYTES);
  o->offers[i].deadline = now + TRANSFER_MS;
  memset(&e, 0, sizeof(e));
  e.kind = EV_ADMITTED;
  e.id = id;
  memcpy(e.meta, meta, META_BYTES);
  journal_push(&o->journal, &e, 0);
  return B_ADMITTED;
}

enum { RX_NONE, RX_HANGUP, RX_STALE, RX_FAULT, RX_RECEIVED };
FX int owner_fault(struct owner *o, const int *fds, unsigned n) {
  close_all(fds, n);
  o->faults++;
  owner_channel_close(o, CLOSE_PROTOCOL);
  return RX_FAULT;
}
/* Receives at most one packet (max_packets_per_turn = 1). */
FX int owner_receive(struct owner *o) {
  unsigned char b[SOURCE_BYTES + 1];
  int fds[5], bad;
  unsigned n, row;
  ssize_t got;
  uint64_t id;
  struct offer *f;
  if (o->channel != CH_LIVE) return RX_NONE;
  got = receive_packet(o->sock, b, sizeof(b), fds, &n, &bad);
  if (got < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) return RX_NONE;
  if (got <= 0) {
    close_all(fds, n);
    owner_channel_close(o, CLOSE_PEER);
    return RX_HANGUP;
  }
  if (bad || got < 40 || get(b, 0, 4) != (uint64_t)got || get(b, 4, 2) != 1 ||
      get(b, 8, 2) != fixture_role || get(b, 12, 4) || get(b, 16, 8) != o->epoch ||
      get(b, 24, 8) != o->gen || get(b, 6, 2) != KIND_SOURCE || got != SOURCE_BYTES)
    return owner_fault(o, fds, n);
  id = get(b, 32, 8);
  /* Packet shape and identity first, so a stale ID cannot hide a malformed
   * packet: counts, sealed plane row and unused rows. */
  if (!id || get(b, 40, 8) != id || get(b, 10, 2) != 1 || n != 1 || get(b, 40 + 76, 2) != 1 ||
      get(b, 40 + 78, 2) != 1 || get(b, 136, 8) || get(b, 144, 4) != get(b, 40 + 72, 4) ||
      get(b, 148, 4))
    return owner_fault(o, fds, n);
  for (row = 1; row < 4; row++) {
    unsigned k;
    for (k = 0; k < 16; k++)
      if (b[136 + 16 * row + k]) return owner_fault(o, fds, n);
  }
  if (id > o->mark) {
    o->above_mark++;
    return owner_fault(o, fds, n);
  }
  f = owner_find(o, id);
  if (!f) { /* expired, released or skipped: drain without event or fault */
    close_all(fds, n);
    o->stale_closed++;
    return RX_STALE;
  }
  if (f->state != O_WAITING) return owner_fault(o, fds, n); /* second Source */
  if (memcmp(b + 40, f->meta, META_BYTES)) return owner_fault(o, fds, n); /* not as admitted */
  f->fd = fds[0];
  f->state = O_RECEIVED;
  return RX_RECEIVED;
}

FX int premultiplied(const unsigned char *p, size_t n) {
  size_t i;
  for (i = 0; i + 4 <= n; i += 4)
    if (p[i] > p[i + 3] || p[i + 1] > p[i + 3] || p[i + 2] > p[i + 3]) return 0;
  return 1;
}

FX struct backing *owner_backing(struct owner *o, uint64_t resource) {
  unsigned i;
  for (i = 0; i < BACKING_SLOTS; i++)
    if (o->backings[i].live && o->backings[i].resource == resource) return &o->backings[i];
  return 0;
}

/* One validation unit: the seal/size check and read-only mapping for a
 * received source, or one 64 KiB pixel chunk for a mapped one. Units run
 * synchronously, so a cancellation between calls may unmap at once. That is
 * not evidence about cancelling an in-flight worker, which must hold the
 * mapping until the worker returns. */
FX void owner_validate(struct owner *o, struct offer *f) {
  struct stat st;
  int seals;
  if (f->state == O_RECEIVED) {
    seals = fcntl(f->fd, F_GET_SEALS);
    if (seals < 0 || (seals & REQUIRED_SEALS) != REQUIRED_SEALS || fstat(f->fd, &st) ||
        (uint64_t)st.st_size != get(f->meta, 80, 8)) {
      owner_terminal(o, f, ST_REJECTED, DISP_BEFORE_USE);
      return;
    }
    f->size = (size_t)st.st_size;
    f->map = mmap(0, f->size, PROT_READ, MAP_SHARED, f->fd, 0);
    if (f->map == MAP_FAILED) {
      f->map = 0;
      owner_terminal(o, f, ST_REJECTED, DISP_BEFORE_USE);
      return;
    }
    f->state = O_VALIDATING;
    return;
  }
  if (f->state == O_VALIDATING) {
    size_t n = f->size - f->done < CHUNK ? f->size - f->done : CHUNK;
    if (!premultiplied(f->map + f->done, n)) {
      owner_terminal(o, f, ST_REJECTED, DISP_BEFORE_USE);
      return;
    }
    f->done += n;
    if (f->done == f->size) {
      unsigned i;
      for (i = 0; i < BACKING_SLOTS && o->backings[i].live; i++) {}
      assert(i < BACKING_SLOTS); /* reserved at admission */
      o->backing_slots_reserved--;
      f->charged = 0; /* the charge moves into the backing, not duplicated */
      o->backings[i].live = 1;
      o->backings[i].resource = get(f->meta, 8, 8);
      o->backings[i].map = f->map; /* the mapping moves into immutable backing */
      o->backings[i].size = f->size;
      f->map = 0;
      owner_terminal(o, f, ST_ACCEPTED, DISP_RETAINED);
    }
  }
}
FX unsigned owner_work(struct owner *o, unsigned units) {
  unsigned i, done = 0;
  for (i = 0; i < MAX_OFFERS && done < units; i++)
    while (done < units && (o->offers[i].state == O_RECEIVED || o->offers[i].state == O_VALIDATING)) {
      owner_validate(o, &o->offers[i]);
      done++;
    }
  return done;
}

FX int owner_cancel(struct owner *o, uint64_t id) {
  struct offer *f = owner_find(o, id);
  if (!f) return ENOENT;
  if (f->state == O_GPU) { /* status now; release waits for safe cleanup */
    if (!f->status_sent && o->journal.alive) emit(o, EV_STATUS, id, ST_CANCELLED, 0, 1);
    f->status_sent = 1;
    return 0;
  }
  owner_terminal(o, f, ST_CANCELLED, DISP_CANCELLED);
  return 0;
}

/* Clockless expiry: the caller supplies now. GPU-held work never expires
 * into a release; it can only be timed out into quarantine. */
FX void owner_tick(struct owner *o, uint64_t now) {
  unsigned i;
  for (i = 0; i < MAX_OFFERS; i++) {
    struct offer *f = &o->offers[i];
    if (f->state != O_FREE && f->state != O_GPU && now >= f->deadline)
      owner_terminal(o, f, ST_REJECTED, DISP_BEFORE_USE);
  }
}

/* Fake GPU custody: models a capture job holding the received reference. */
FX void owner_gpu_submit(struct owner *o, uint64_t id) {
  struct offer *f = owner_find(o, id);
  assert(f && (f->state == O_RECEIVED || f->state == O_VALIDATING));
  f->state = O_GPU;
}
FX void owner_gpu_timeout(struct owner *o, uint64_t id) {
  struct offer *f = owner_find(o, id);
  assert(f && f->state == O_GPU);
  if (!f->status_sent && o->journal.alive) emit(o, EV_STATUS, id, ST_REJECTED, 0, 1);
  f->status_sent = 1;
  f->quarantined = 1; /* keeps its offer slot until cleanup is safe */
}
FX void owner_gpu_complete(struct owner *o, uint64_t id) {
  struct offer *f = owner_find(o, id);
  assert(f && f->state == O_GPU);
  if (!o->journal.alive) {
    owner_uncharge(o, f);
    offer_drop(f);
    return;
  }
  if (f->status_sent) owner_terminal(o, f, ST_CANCELLED, DISP_DRAINED);
  else owner_terminal(o, f, ST_ACCEPTED, DISP_CAPTURED);
}

FX int owner_byte_submit(struct owner *o) {
  if (!o->epoch_live) return -1;
  o->byte_submits++;
  return 0;
}

FX void owner_lease(struct owner *o, uint64_t resource) {
  struct backing *b = owner_backing(o, resource);
  assert(b && !b->retired);
  b->leases++;
}
/* The last consumer is gone. On a live journal without room, the backing
 * keeps its slot and charge until ResourceReleased can be recorded; it is
 * never dropped silently. A dead journal records nothing. */
FX void backing_settle(struct owner *o, struct backing *b) {
  if (!b->retired || b->leases) return;
  if (o->journal.alive && !journal_room(&o->journal, 1, 0)) {
    b->release_pending = 1;
    return;
  }
  if (o->journal.alive) emit(o, EV_RES_RELEASED, b->resource, 0, 0, 0);
  munmap((void *)b->map, b->size);
  o->backing_charge -= b->size;
  memset(b, 0, sizeof(*b));
}
FX void owner_settle_pending(struct owner *o) {
  unsigned i;
  for (i = 0; i < BACKING_SLOTS; i++)
    if (o->backings[i].live && o->backings[i].release_pending) backing_settle(o, &o->backings[i]);
}
FX void owner_unlease(struct owner *o, uint64_t resource) {
  struct backing *b = owner_backing(o, resource);
  assert(b && b->leases);
  b->leases--;
  backing_settle(o, b);
}
FX void owner_retire(struct owner *o, uint64_t resource) {
  struct backing *b = owner_backing(o, resource);
  assert(b);
  b->retired = 1;
  backing_settle(o, b);
}

/* 9P epoch end: no journal remains, so no event is emitted or invented. */
FX void owner_end_epoch(struct owner *o) {
  unsigned i;
  o->epoch_live = 0;
  o->journal.alive = 0;
  o->journal.reserved = 0;
  if (o->sock >= 0) close(o->sock);
  o->sock = -1;
  o->channel = CH_DISABLED;
  for (i = 0; i < MAX_OFFERS; i++)
    if (o->offers[i].state != O_FREE && o->offers[i].state != O_GPU) {
      owner_uncharge(o, &o->offers[i]);
      offer_drop(&o->offers[i]);
    }
  /* Every backing retires; one still leased by a consumer lives until its
   * last lease ends, and none produces an event on the dead journal. */
  for (i = 0; i < BACKING_SLOTS; i++)
    if (o->backings[i].live) {
      o->backings[i].retired = 1;
      backing_settle(o, &o->backings[i]);
    }
}

#endif
