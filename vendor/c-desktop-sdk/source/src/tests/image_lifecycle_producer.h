/* Producer half of the test-only x6jp929h lifecycle fixture: an SDK client's
 * custody rules, plus shared test helpers. Scope as in image_lifecycle_fixture.h. */
#ifndef IMAGE_LIFECYCLE_PRODUCER_H
#define IMAGE_LIFECYCLE_PRODUCER_H

#include "image_lifecycle_fixture.h"

/* ---- Producer model (an SDK client's custody rules) ---------------------- */

enum { PC_NONE, PC_DISCOVERED, PC_HELLO_SENT, PC_READY, PC_CLOSED };
enum { P_FREE, P_BEGUN, P_ADMITTED, P_SENT, P_AMBIGUOUS, P_EXPIRED, P_SETTLED, P_UNKNOWN };
enum { D_READY, D_RETRY, D_DEADLINE, D_ABSENT, D_ERROR };
enum { PATH_DESCRIPTOR, PATH_BYTES, PATH_ERROR };

struct poffer {
  int state, fd, status;
  uint32_t disposition;
  unsigned char meta[META_BYTES];
  uint64_t deadline;
};

typedef ssize_t (*send_fn)(int, const void *, size_t, const int *, unsigned);

struct producer {
  uint64_t epoch, gen, next_id, startup_deadline, formats_gen, limits_gen, last_pass;
  uint32_t kinds;
  int channel, sock, fallback_allowed, want_writable, journal_alive, absent, read_pass;
  struct poffer offers[MAX_OFFERS];
  send_fn send;
  unsigned ignored_events, settles, duplicate_releases, sends;
};

FX void producer_init(struct producer *p, uint64_t startup_deadline, int fallback_allowed) {
  unsigned i;
  memset(p, 0, sizeof(*p));
  p->next_id = 1;
  p->sock = -1;
  p->startup_deadline = startup_deadline;
  p->fallback_allowed = fallback_allowed;
  p->journal_alive = 1;
  p->send = send_packet;
  for (i = 0; i < MAX_OFFERS; i++) p->offers[i].fd = -1;
}

typedef int (*read_set_fn)(void *context, struct objects *out);

/* At most one complete set per caller drive pass, the initial acquisition
 * included; a repeated call in the same pass reads nothing. Pass identity is
 * the caller's, distinct from time, which only bounds startup. A mixed
 * snapshot never yields D_READY. Only ENOENT is absence; every other failure
 * is an error, never a downgrade. */
FX int producer_discover(struct producer *p, read_set_fn read_set, void *context, uint64_t pass,
                         uint64_t now) {
  struct objects s;
  uint64_t v[15];
  if (now >= p->startup_deadline) return D_DEADLINE;
  if (p->read_pass && pass == p->last_pass) return D_RETRY;
  p->read_pass = 1;
  p->last_pass = pass;
  if (read_set(context, &s)) return D_ERROR;
  if (s.error == ENOENT) {
    p->absent = 1;
    return p->fallback_allowed ? D_ABSENT : D_ERROR;
  }
  if (s.error || discovery_parse(s.text, s.text_length, v)) return D_ERROR;
  if (get(s.limits, 0, 4) != 136 || get(s.limits, 6, 2) != 0x7005 ||
      get(s.formats, 0, 4) != 64 || get(s.formats, 6, 2) != 0x7006 ||
      get(s.limits, 8, 8) != v[1] || get(s.formats, 8, 8) != v[1])
    return D_ERROR;
  if (get(s.limits, 32, 8) != v[2] || get(s.formats, 32, 8) != v[2] ||
      get(s.limits, 40, 8) != v[14] || get(s.formats, 48, 8) != v[14] ||
      get(s.formats, 40, 8) != v[13])
    return D_RETRY; /* closed; the next pass may read one fresh set */
  p->epoch = v[1];
  p->gen = v[2];
  p->kinds = (uint32_t)v[4];
  p->formats_gen = v[13];
  p->limits_gen = v[14];
  p->channel = PC_DISCOVERED;
  return D_READY;
}

FX int producer_hello(struct producer *p, int sock) {
  unsigned char b[HELLO_BYTES];
  if (p->channel != PC_DISCOVERED) return -EINVAL;
  header_encode(b, HELLO_BYTES, KIND_HELLO, 0, p->epoch, p->gen, 0);
  p->sock = sock;
  if (p->send(sock, b, sizeof(b), 0, 0) != HELLO_BYTES) return -errno;
  p->channel = PC_HELLO_SENT;
  return 0;
}

FX struct poffer *producer_find(struct producer *p, uint64_t id) {
  unsigned i;
  for (i = 0; i < MAX_OFFERS; i++)
    if (p->offers[i].state != P_FREE && get(p->offers[i].meta, 0, 8) == id) return &p->offers[i];
  return 0;
}

FX void producer_event(struct producer *p, const struct event *e) {
  struct poffer *f;
  switch (e->kind) {
  case EV_READY:
    if (e->id == p->gen && p->channel == PC_HELLO_SENT) {
      p->channel = PC_READY;
      p->kinds = e->value;
    } else {
      p->ignored_events++;
    }
    return;
  case EV_CLOSED:
    if (e->id == p->gen && (p->channel == PC_HELLO_SENT || p->channel == PC_READY))
      p->channel = PC_CLOSED;
    else
      p->ignored_events++;
    return;
  case EV_ADMITTED:
    f = producer_find(p, e->id);
    if (f && f->state == P_BEGUN && !memcmp(f->meta, e->meta, META_BYTES)) f->state = P_ADMITTED;
    else p->ignored_events++;
    return;
  case EV_STATUS:
    f = producer_find(p, e->id);
    if (f) f->status = (int)e->value; /* never permission to reuse */
    return;
  case EV_RELEASED:
    f = producer_find(p, e->id);
    if (!f || f->state == P_SETTLED || f->state == P_UNKNOWN) {
      p->duplicate_releases++;
      return;
    }
    f->state = P_SETTLED;
    f->disposition = e->value;
    p->settles++;
    return;
  default:
    return;
  }
}

FX void producer_pump(struct producer *p, struct owner *o) {
  struct event e;
  while (owner_take(o, &e)) producer_event(p, &e);
}

/* Allocates a strictly increasing offer ID. Exhaustion requires reconnect. */
FX int producer_begin(struct producer *p, struct image *img, int fd, uint64_t now, uint64_t *id) {
  unsigned i;
  if (p->channel == PC_CLOSED || !p->journal_alive) return -ENODEV;
  if (p->channel != PC_READY) return -EAGAIN;
  if (!p->next_id) return -ECONNRESET;
  for (i = 0; i < MAX_OFFERS && p->offers[i].state != P_FREE; i++) {}
  if (i == MAX_OFFERS) return -EBUSY;
  img->offer = p->next_id++;
  meta_encode(p->offers[i].meta, img);
  p->offers[i].state = P_BEGUN;
  p->offers[i].fd = fd;
  p->offers[i].status = 0;
  p->offers[i].deadline = now + TRANSFER_MS;
  *id = img->offer;
  return 0;
}

/* The owner refused an ImageBegin: the slot is free, the ID is spent. */
FX void producer_refused(struct producer *p, uint64_t id) {
  struct poffer *f = producer_find(p, id);
  assert(f && f->state == P_BEGUN);
  memset(f, 0, sizeof(*f));
  f->fd = -1;
}

FX int producer_send(struct producer *p, uint64_t id, int indeterminate) {
  unsigned char b[SOURCE_BYTES];
  struct poffer *f = producer_find(p, id);
  ssize_t got;
  if (!f) return -ENOENT;
  if (f->state == P_SENT || f->state == P_AMBIGUOUS || f->state == P_SETTLED ||
      f->state == P_UNKNOWN || f->state == P_EXPIRED)
    return -EALREADY;
  if (f->state != P_ADMITTED) return -EINVAL;
  if (p->channel != PC_READY || !p->journal_alive) return -ENODEV; /* closed: never send */
  source_encode(b, p->epoch, p->gen, f->meta);
  got = p->send(p->sock, b, sizeof(b), &f->fd, 1);
  p->sends++;
  if (indeterminate) { /* caller lost the outcome: never replay or reuse */
    f->state = P_AMBIGUOUS;
    p->want_writable = 0;
    return -EIO;
  }
  if (got == SOURCE_BYTES) {
    f->state = P_SENT;
    p->want_writable = 0;
    return 0;
  }
  if (errno == EAGAIN || errno == EWOULDBLOCK) {
    p->want_writable = 1;
    return -EAGAIN;
  }
  return -errno;
}

/* Clockless accessors: interest in writability and the earliest deadline at
 * which an unsent admitted offer stops being sendable. */
FX int producer_wants_writable(const struct producer *p) { return p->want_writable; }
FX uint64_t producer_next_deadline(const struct producer *p) {
  uint64_t d = UINT64_MAX;
  unsigned i;
  for (i = 0; i < MAX_OFFERS; i++)
    if ((p->offers[i].state == P_ADMITTED || p->offers[i].state == P_BEGUN) &&
        p->offers[i].deadline < d)
      d = p->offers[i].deadline;
  return d;
}
FX void producer_tick(struct producer *p, uint64_t now) {
  unsigned i, unsent = 0;
  for (i = 0; i < MAX_OFFERS; i++) {
    if (p->offers[i].state == P_ADMITTED && now >= p->offers[i].deadline)
      p->offers[i].state = P_EXPIRED; /* awaits the owner's release */
    unsent += p->offers[i].state == P_ADMITTED;
  }
  if (!unsent) p->want_writable = 0;
}

FX int producer_may_reuse(struct producer *p, uint64_t id) {
  struct poffer *f = producer_find(p, id);
  return f && f->state == P_SETTLED;
}
FX void producer_forget(struct producer *p, uint64_t id) {
  struct poffer *f = producer_find(p, id);
  assert(f && f->state == P_SETTLED);
  memset(f, 0, sizeof(*f));
  f->fd = -1;
}

/* No journal: release can no longer be learned. Unsettled offers become
 * unknown custody; nothing is replayed or reused. */
FX void producer_journal_lost(struct producer *p) {
  unsigned i;
  p->journal_alive = 0;
  for (i = 0; i < MAX_OFFERS; i++)
    if (p->offers[i].state != P_FREE && p->offers[i].state != P_SETTLED)
      p->offers[i].state = P_UNKNOWN;
}
/* A channel socket hangup is not a completion signal for any offer. */
FX void producer_hangup(struct producer *p) {
  if (p->channel == PC_HELLO_SENT || p->channel == PC_READY) p->channel = PC_CLOSED;
}
FX int producer_path(const struct producer *p) {
  if (p->channel == PC_READY && p->journal_alive) return PATH_DESCRIPTOR;
  if (p->channel == PC_CLOSED || p->absent) return p->fallback_allowed ? PATH_BYTES : PATH_ERROR;
  return PATH_ERROR;
}

/* ---- Shared test helpers ------------------------------------------------- */

FX unsigned fd_count(void) {
  DIR *d = opendir("/proc/self/fd");
  unsigned n = 0;
  assert(d);
  while (readdir(d)) n++;
  closedir(d);
  return n;
}

/* A sealable image of w x h premultiplied BGRA pixels with the given seals. */
FX int sealed_image(uint32_t w, uint32_t h, int seals, unsigned char alpha) {
  static unsigned char row[4096 * 4];
  uint32_t i;
  int fd = memfd_create("t337-lifecycle", MFD_ALLOW_SEALING | MFD_CLOEXEC);
  assert(fd >= 0 && w <= 4096);
  for (i = 0; i < w * 4; i += 4) {
    row[i] = row[i + 1] = row[i + 2] = alpha / 2;
    row[i + 3] = alpha;
  }
  for (i = 0; i < h; i++) assert(write(fd, row, (size_t)w * 4) == (ssize_t)w * 4);
  if (seals) assert(fcntl(fd, F_ADD_SEALS, seals) == 0);
  return fd;
}

FX struct image image_for(const struct owner *o, uint64_t resource, uint32_t w, uint32_t h) {
  struct image i;
  memset(&i, 0, sizeof(i));
  i.resource = resource;
  i.generation = 1;
  i.grant = o->grant;
  i.output = 7;
  i.allocation = 8;
  i.topology = o->topology;
  i.width = w;
  i.height = h;
  return i;
}

struct live_owner {
  struct owner *owner;
  unsigned reads;
};
FX int read_live(void *context, struct objects *out) {
  struct live_owner *l = context;
  l->reads++;
  owner_open(l->owner, out);
  return 0;
}

/* Discovers, connects a SEQPACKET pair, sends Hello, joins and delivers
 * ChannelReady. Returns the producer's socket. */
FX int connect_ready(struct owner *o, struct producer *p) {
  int fds[2];
  struct live_owner l;
  l.owner = o;
  l.reads = 0;
  assert(producer_discover(p, read_live, &l, 1, 0) == D_READY);
  assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, fds) == 0);
  assert(producer_hello(p, fds[0]) == 0);
  assert(owner_accept(o, fds[1]) == A_JOINED);
  producer_pump(p, o);
  assert(p->channel == PC_READY);
  return fds[0];
}

/* Begins, admits and delivers ImageAdmitted for one image. */
FX uint64_t admit_size(struct owner *o, struct producer *p, uint64_t resource, int fd,
                      uint64_t now, uint32_t w, uint32_t h) {
  struct image img = image_for(o, resource, w, h);
  uint64_t id;
  struct poffer *f;
  assert(producer_begin(p, &img, fd, now, &id) == 0);
  f = producer_find(p, id);
  assert(owner_begin(o, f->meta, now) == B_ADMITTED);
  producer_pump(p, o);
  assert(f->state == P_ADMITTED);
  return id;
}
FX uint64_t admit(struct owner *o, struct producer *p, uint64_t resource, int fd, uint64_t now) {
  return admit_size(o, p, resource, fd, now, 64, 64);
}

FX unsigned count_events(const struct owner *o, int kind, uint64_t id) {
  unsigned i, n = 0;
  const struct journal *j = &o->journal;
  for (i = 0; i < j->count; i++) {
    const struct event *e = &j->events[(j->head + i) % 32];
    n += e->kind == kind && e->id == id;
  }
  return n;
}

#endif
