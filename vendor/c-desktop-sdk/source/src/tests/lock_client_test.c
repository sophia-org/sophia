/* Lock client against a scripted 9P2000.L peer, not a production Session.
 * The peer follows the export's custody rules (spec/sophia-lock-files.md):
 * Negotiate journals Submitted, Negotiated and the lock's ObjectPublished;
 * the lock file always names the newest object; an upload writer is bound to
 * an admitted slot and writes at its cursor; a refused submit journals
 * nothing; the final ack of a refusal revokes the connection. */
#include "sophia_lock_client.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

/* Consumer link control: an application may define these generic names. The
 * library's internals must not export them, or this test would fail to link
 * (multiple definition) or call one of them below and abort. */
int lc_queue(void) { abort(); }
int lc_parse(void) { abort(); }
int lc_object_drive(void) { abort(); }
int lc_object_reply(void) { abort(); }
int lc_upload_drive(void) { abort(); }
int lc_upload_reply(void) { abort(); }
void lc_upload_status(void) { abort(); }
void lc_upload_refused(void) { abort(); }

#define EPOCH 7u
enum {
  F_ROOT = 1,
  F_API,
  F_LIMITS,
  F_LOCK,
  F_EVENTS,
  F_TX,
  F_SUBMIT,
  F_ACK,
  F_UPLOADS,
  F_UPLOAD /* + slot */
};
static const char *const names[] = {"api",         "limits", "lock", "events",
                                    "transaction", "submit", "ack",  "upload"};

struct peer {
  int fd;
  uint8_t input[16384], journal[16384], tx[SOPHIA_LF_MAX_CANDIDATE];
  uint8_t limits[128], lock[1024], pixels[4096];
  const char *api;
  size_t used, journal_size, tx_size, limits_size, lock_size;
  unsigned files[256], attaches, submits, acks, lock_reads;
  uint32_t tx_fid, event_count, upload_fid;
  uint64_t sequence, acked, event_offset, qid, generation, refused;
  uint64_t last_submit_id, last_bytes_id, upload_cursor, upload_total;
  uint64_t accepted_submitted;
  uint16_t event_tag, upload_slot;
  unsigned events_pending, negotiated, upload_bound;
  /* Faults. */
  unsigned submit_again, reject_begin, stale_events, hold_acks;
  uint16_t held_tag;
  uint64_t held_sequence;
  uint32_t submit_error, extra_grant;
  uint8_t last_bytes[SOPHIA_LF_MAX_CANDIDATE];
  size_t last_size;
  struct sophia_lf_resource_begin begin;
};
struct rig {
  struct peer peer;
  struct sophia_9p_client wire;
  struct sophia_lc_client client;
  void *storage;
  int client_fd;
};

static uint64_t get(const uint8_t *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (8 * i);
  return v;
}
static void put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (8 * i));
}
static void send_reply(struct peer *p, unsigned kind, uint16_t tag,
                       const void *data, size_t n) {
  static uint8_t b[70000];
  size_t at = 0;
  assert(n + 7 <= sizeof(b));
  put(b, n + 7, 4);
  b[4] = (uint8_t)kind;
  put(b + 5, tag, 2);
  if (n)
    memcpy(b + 7, data, n);
  while (at < n + 7) {
    ssize_t amount = send(p->fd, b + at, n + 7 - at, MSG_NOSIGNAL);
    assert(amount > 0);
    at += (size_t)amount;
  }
}
static void error(struct peer *p, uint16_t tag, uint32_t value) {
  uint8_t b[4];
  put(b, value, 4);
  send_reply(p, 7, tag, b, 4);
}
static void count(struct peer *p, uint16_t tag, uint32_t value) {
  uint8_t b[4];
  put(b, value, 4);
  send_reply(p, 119, tag, b, 4);
}
static void data(struct peer *p, uint16_t tag, const uint8_t *bytes,
                 size_t size, uint64_t offset, uint32_t n) {
  static uint8_t b[70000];
  size_t amount = offset < size ? size - (size_t)offset : 0;
  if (amount > n)
    amount = n;
  put(b, amount, 4);
  if (amount)
    memcpy(b + 4, bytes + offset, amount);
  send_reply(p, 117, tag, b, amount + 4);
}
static void qid(uint8_t *b, uint8_t type, uint64_t path) {
  b[0] = type;
  put(b + 1, 0, 4);
  put(b + 5, path, 8);
}
static void record(struct peer *p, struct sophia_lf_record *r) {
  size_t n;
  r->header.epoch = EPOCH;
  r->header.submission = 0;
  r->header.sequence = ++p->sequence;
  assert(!sophia_lf_encode(p->journal + p->journal_size,
                           sizeof(p->journal) - p->journal_size, r, &n));
  p->journal_size += n;
}
static struct sophia_lf_allocation allocation(uint64_t output) {
  struct sophia_lf_allocation a;
  a.output = output;
  a.output_generation = 3;
  a.allocation = 40 + output;
  a.allocation_generation = 4;
  a.pixel_width = 4;
  a.pixel_height = 2;
  a.scale_numerator = 1;
  a.scale_denominator = 1;
  return a;
}
/* Replace the lock object under a fresh qid and announce it. */
static void announce(struct peer *);
static void publish(struct peer *p, uint16_t phase, uint64_t lock_epoch,
                    uint16_t outputs) {
  struct sophia_lf_record r;
  uint16_t i;
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_LF_LOCK;
  r.header.epoch = EPOCH;
  r.value.lock.lock_epoch = lock_epoch;
  r.value.lock.topology_generation = 9;
  r.value.lock.phase = phase;
  r.value.lock.allocation_count = outputs;
  for (i = 0; i < outputs; ++i)
    r.value.lock.allocations[i] = allocation(i + 1u);
  assert(!sophia_lf_encode(p->lock, sizeof(p->lock), &r, &p->lock_size));
  p->qid = p->qid ? p->qid + 1 : 90;
  ++p->generation;
  if (p->negotiated)
    announce(p);
}
/* Announce the current object; Negotiate announces the one it found. */
static void announce(struct peer *p) {
  struct sophia_lf_record e;
  memset(&e, 0, sizeof(e));
  e.header.kind = SOPHIA_LF_OBJECT_PUBLISHED;
  e.value.published.object_generation = p->generation;
  e.value.published.qid_path = p->qid;
  record(p, &e);
}
static void event(struct peer *p, uint16_t kind, const struct sophia_lf_record *v) {
  struct sophia_lf_record r = *v;
  r.header.kind = kind;
  record(p, &r);
}
static void events(struct peer *p) {
  if (!p->events_pending || p->event_offset == p->journal_size)
    return;
  assert(p->event_offset < p->journal_size);
  data(p, p->event_tag, p->journal, p->journal_size, p->event_offset,
       p->event_count);
  p->events_pending = 0;
}
static unsigned file_name(const uint8_t *name, size_t n) {
  unsigned i;
  for (i = 0; i < sizeof(names) / sizeof(names[0]); ++i)
    if (strlen(names[i]) == n && !memcmp(name, names[i], n))
      return i + F_API;
  assert(!"unexpected file");
  return 0;
}
static void submitted(struct peer *p, uint64_t id, uint16_t kind) {
  struct sophia_lf_record r;
  memset(&r, 0, sizeof(r));
  r.value.submitted.submission = id;
  r.value.submitted.kind = kind;
  event(p, SOPHIA_LF_SUBMITTED, &r);
  p->accepted_submitted = p->sequence;
}
static void resource_status(struct peer *p, uint64_t transaction,
                            uint16_t status, uint16_t reason) {
  struct sophia_lf_record r;
  memset(&r, 0, sizeof(r));
  r.value.resource_status.transaction = transaction;
  r.value.resource_status.resource = p->begin.resource;
  r.value.resource_status.status = status;
  r.value.resource_status.reason = reason;
  r.value.resource_status.admitted_bytes =
      status == SOPHIA_LF_ADMITTED ? p->upload_total : 0;
  event(p, SOPHIA_LF_RESOURCE_STATUS, &r);
}
static void accept_candidate(struct peer *p, const struct sophia_lf_record *c) {
  struct sophia_lf_record r;
  memset(&r, 0, sizeof(r));
  switch (c->header.kind) {
  case SOPHIA_LF_NEGOTIATE: {
    const struct sophia_lf_negotiate *n = &c->value.negotiate;
    if (n->minimum_revision > 1 || n->maximum_revision < 1) {
      r.value.refusal = SOPHIA_LF_UNSUPPORTED_REVISION;
      event(p, SOPHIA_LF_REFUSED, &r);
      p->refused = p->sequence;
      return;
    }
    r.value.negotiated.granted_chords = n->chord_count;
    r.value.negotiated.granted_capabilities = n->capabilities | p->extra_grant;
    event(p, SOPHIA_LF_NEGOTIATED, &r);
    p->negotiated = 1;
    announce(p);
    return;
  }
  case SOPHIA_LF_RESOURCE_BEGIN:
    p->begin = c->value.resource_begin;
    p->upload_total = (uint64_t)p->begin.width_px * p->begin.height_px * 4;
    p->upload_cursor = 0;
    if (p->reject_begin) {
      resource_status(p, p->begin.transaction, SOPHIA_LF_RESOURCE_REJECTED,
                      SOPHIA_LF_REASON_BUDGET);
      return;
    }
    p->upload_bound = 1;
    p->upload_slot = p->begin.slot;
    resource_status(p, p->begin.transaction, SOPHIA_LF_ADMITTED, 0);
    return;
  case SOPHIA_LF_RESOURCE_END:
    p->upload_bound = 0;
    if (p->upload_cursor == p->upload_total &&
        c->value.resource_step.total_bytes == p->upload_total)
      resource_status(p, c->value.resource_step.transaction,
                      SOPHIA_LF_ACCEPTED, 0);
    else
      resource_status(p, c->value.resource_step.transaction,
                      SOPHIA_LF_RESOURCE_REJECTED, SOPHIA_LF_REASON_SIZE_MISMATCH);
    return;
  case SOPHIA_LF_RESOURCE_CANCEL:
    p->upload_bound = 0;
    resource_status(p, c->value.resource_step.transaction, SOPHIA_LF_CANCELLED,
                    0);
    return;
  case SOPHIA_LF_RESOURCE_RETIRE:
    r.value.resource_released.transaction = c->value.resource_step.transaction;
    r.value.resource_released.resource = c->value.resource_step.resource;
    event(p, SOPHIA_LF_RESOURCE_RELEASED, &r);
    return;
  case SOPHIA_LF_FRAME_DEMAND: {
    const struct sophia_lf_frame_demand *d = &c->value.frame_demand;
    r.value.frame_permit.lock_epoch = d->lock_epoch;
    r.value.frame_permit.allocation = d->allocation;
    r.value.frame_permit.allocation_generation = d->allocation_generation;
    r.value.frame_permit.demand = d->demand;
    r.value.frame_permit.pacing_permit = 77;
    r.value.frame_permit.expires_after_ms = 100;
    event(p, SOPHIA_LF_FRAME_PERMIT, &r);
    return;
  }
  case SOPHIA_LF_CANDIDATE: {
    const struct sophia_lf_candidate *v = &c->value.candidate;
    r.value.candidate_outcome.transaction = v->transaction;
    r.value.candidate_outcome.lock_epoch = v->lock_epoch;
    r.value.candidate_outcome.output = v->output;
    r.value.candidate_outcome.allocation = v->allocation;
    r.value.candidate_outcome.candidate_generation = v->candidate_generation;
    r.value.candidate_outcome.status = SOPHIA_LF_PRESENTED;
    event(p, SOPHIA_LF_CANDIDATE_OUTCOME, &r);
    return;
  }
  default:
    assert(!"unexpected candidate");
  }
}
static void submit(struct peer *p, uint16_t tag, const uint8_t *d) {
  struct sophia_lf_record c;
  uint64_t id = get(d + 8, 8);
  assert(get(d, 8) == EPOCH && get(d + 16, 4) == p->tx_size && !get(d + 20, 4));
  assert(!sophia_lf_decode(p->tx, p->tx_size, &c) &&
         c.header.submission == id && c.header.epoch == EPOCH);
  ++p->submits;
  if (id == p->last_bytes_id) /* a retry repeats the exact bytes */
    assert(p->tx_size == p->last_size &&
           !memcmp(p->tx, p->last_bytes, p->last_size));
  else
    assert(id > p->last_submit_id);
  p->last_bytes_id = id;
  p->last_size = p->tx_size;
  memcpy(p->last_bytes, p->tx, p->tx_size);
  if (p->submit_again) {
    --p->submit_again;
    error(p, tag, 11);
    return;
  }
  p->last_submit_id = id;
  /* An upload is one transaction: End and Cancel carry the begin's. */
  if ((c.header.kind == SOPHIA_LF_RESOURCE_END ||
       c.header.kind == SOPHIA_LF_RESOURCE_CANCEL) &&
      c.value.resource_step.transaction != p->begin.transaction)
    p->submit_error = 22;
  if (p->submit_error) {
    error(p, tag, p->submit_error);
    p->submit_error = 0;
    return;
  }
  submitted(p, id, c.header.kind);
  accept_candidate(p, &c);
  count(p, tag, SOPHIA_LF_SUBMIT_BYTES);
}
static void walk(struct peer *p, uint16_t tag, uint32_t fid, const uint8_t *q) {
  uint8_t b[64] = {0};
  uint32_t target = (uint32_t)get(q + 4, 4);
  unsigned n = (unsigned)get(q + 8, 2), file;
  size_t length = (size_t)get(q + 10, 2);
  assert(p->files[fid] == F_ROOT && target < 256 && (n == 1 || n == 2));
  file = file_name(q + 12, length);
  put(b, n, 2);
  if (n == 2) {
    const uint8_t *slot = q + 12 + length;
    assert(file == F_UPLOADS && get(slot, 2) == 1 && slot[2] >= '0' &&
           slot[2] <= '3');
    qid(b + 2, 0x80, F_UPLOADS);
    file = F_UPLOAD + (unsigned)(slot[2] - '0');
    qid(b + 15, 0, file);
  } else
    qid(b + 2, 0, file == F_LOCK ? p->qid : file);
  p->files[target] = file;
  send_reply(p, 111, tag, b, 2 + 13u * n);
}
static void request(struct peer *p, const uint8_t *m) {
  uint8_t type = m[4], b[64] = {0};
  uint16_t tag = (uint16_t)get(m + 5, 2);
  const uint8_t *q = m + 7;
  uint32_t fid = (uint32_t)get(q, 4);
  unsigned file;
  if (type == 100) {
    put(b, get(q, 4), 4);
    put(b + 4, 8, 2);
    memcpy(b + 6, "9P2000.L", 8);
    send_reply(p, 101, tag, b, 14);
    return;
  }
  assert(fid < 256);
  file = p->files[fid];
  if (type == 104) {
    assert(!p->attaches++); /* one admitted attach per epoch */
    p->files[fid] = F_ROOT;
    qid(b, 0x80, 1);
    send_reply(p, 105, tag, b, 13);
  } else if (type == 110)
    walk(p, tag, fid, q);
  else if (type == 12) {
    uint32_t flags = (uint32_t)get(q + 4, 4);
    if (file == F_TX) {
      assert(flags == 2);
      /* Custody is held until its Submitted is acknowledged. */
      assert(!p->tx_fid && p->acked >= p->accepted_submitted);
      p->tx_fid = fid;
      p->tx_size = 0;
    } else if (file >= F_UPLOAD) {
      assert(flags == 1);
      if (!p->upload_bound || file - F_UPLOAD != p->upload_slot) {
        error(p, tag, 116);
        return;
      }
      p->upload_fid = fid;
    } else
      assert(flags == (file == F_SUBMIT || file == F_ACK ? 1u : 0u));
    qid(b, 0, file == F_LOCK ? p->qid : file);
    send_reply(p, 13, tag, b, 17);
  } else if (type == 116) {
    uint64_t offset = get(q + 4, 8);
    uint32_t n = (uint32_t)get(q + 12, 4);
    if (file == F_API)
      data(p, tag, (const uint8_t *)p->api, strlen(p->api), offset, n);
    else if (file == F_LIMITS)
      data(p, tag, p->limits, p->limits_size, offset, n);
    else if (file == F_LOCK) {
      ++p->lock_reads;
      data(p, tag, p->lock, p->lock_size, offset, n);
    } else {
      assert(file == F_EVENTS && !p->events_pending);
      if (p->stale_events) {
        error(p, tag, 116);
        return;
      }
      p->events_pending = 1;
      p->event_tag = tag;
      p->event_offset = offset;
      p->event_count = n;
    }
  } else if (type == 118) {
    uint64_t offset = get(q + 4, 8);
    uint32_t n = (uint32_t)get(q + 12, 4);
    const uint8_t *d = q + 16;
    if (file == F_TX) {
      assert(p->tx_fid == fid && offset == p->tx_size &&
             n <= sizeof(p->tx) - p->tx_size);
      memcpy(p->tx + p->tx_size, d, n);
      p->tx_size += n;
      count(p, tag, n);
    } else if (file >= F_UPLOAD) {
      assert(p->upload_fid == fid && p->upload_bound);
      assert(offset == p->upload_cursor && n <= p->upload_total - offset);
      memcpy(p->pixels + offset, d, n);
      p->upload_cursor += n;
      count(p, tag, n);
    } else if (file == F_ACK) {
      uint64_t sequence = get(d + 8, 8);
      assert(!offset && n == 16 && get(d, 8) == EPOCH);
      assert(sequence > p->acked && sequence <= p->sequence);
      ++p->acks;
      if (p->hold_acks) { /* answered by release_ack */
        assert(!p->held_sequence);
        p->held_tag = tag;
        p->held_sequence = sequence;
        return;
      }
      p->acked = sequence;
      count(p, tag, 16);
      if (p->refused && sequence >= p->refused) {
        p->stale_events = 1;
        if (p->events_pending) {
          error(p, p->event_tag, 116);
          p->events_pending = 0;
        }
      }
    } else {
      assert(file == F_SUBMIT && !offset && n == 24);
      submit(p, tag, d);
    }
  } else if (type == 120) {
    if (file == F_TX && p->tx_fid == fid)
      p->tx_fid = 0;
    if (file >= F_UPLOAD && p->upload_fid == fid)
      p->upload_fid = 0;
    p->files[fid] = 0;
    send_reply(p, 121, tag, NULL, 0);
  } else
    assert(!"unexpected 9P request");
}
static void pump(struct peer *p) {
  for (;;) {
    ssize_t n = recv(p->fd, p->input + p->used, sizeof(p->input) - p->used,
                     MSG_DONTWAIT);
    if (n < 0) {
      assert(errno == EAGAIN || errno == EWOULDBLOCK);
      break;
    }
    if (!n)
      break;
    p->used += (size_t)n;
    while (p->used >= 4 && get(p->input, 4) <= p->used) {
      size_t bytes = (size_t)get(p->input, 4);
      request(p, p->input);
      p->used -= bytes;
      memmove(p->input, p->input + bytes, p->used);
    }
  }
  events(p);
}

static const struct sophia_lf_limits limits = {16, 2, 8, 4, 2560, 1440,
                                               2560u * 1440u * 4u, 128, 32768,
                                               2000, 2000};
static struct sophia_lf_negotiate offer(void) {
  struct sophia_lf_negotiate o;
  memset(&o, 0, sizeof(o));
  o.minimum_revision = o.maximum_revision = 1;
  o.capabilities = SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS;
  o.chord_count = 2;
  o.chords[0].keysym = 0x62;
  o.chords[0].modifiers = SOPHIA_LF_MOD_ALT;
  o.chords[1].keysym = 0xff1b;
  o.chords[1].modifiers = SOPHIA_LF_MOD_SUPER | SOPHIA_LF_MOD_SHIFT;
  return o;
}
static struct rig *rig_new(const struct sophia_lf_negotiate *o,
                           const char *api) {
  struct rig *r = calloc(1, sizeof(*r));
  struct sophia_lf_record l;
  size_t bytes = sophia_9p_storage_bytes(8192, 8);
  int sockets[2];
  assert(r && !socketpair(AF_UNIX, SOCK_STREAM, 0, sockets));
  r->client_fd = sockets[0];
  r->peer.fd = sockets[1];
  r->peer.api = api ? api : "sophia-lock-files version=1 epoch=7\n";
  memset(&l, 0, sizeof(l));
  l.header.kind = SOPHIA_LF_LIMITS;
  l.header.epoch = EPOCH;
  l.value.limits = limits;
  assert(!sophia_lf_encode(r->peer.limits, sizeof(r->peer.limits), &l,
                           &r->peer.limits_size));
  publish(&r->peer, SOPHIA_LF_UNLOCKED, 0, 0);
  r->storage = malloc(bytes);
  assert(r->storage);
  assert(!sophia_9p_init(&r->wire, r->client_fd, 8192, 8, 32, r->storage,
                         bytes));
  assert(!sophia_lc_init(&r->client, &r->wire, o));
  return r;
}
static void rig_free(struct rig *r) {
  close(r->client_fd);
  close(r->peer.fd);
  free(r->storage);
  free(r);
}
/* Service both sides until the client stops changing or fails. */
static int spin(struct rig *r) {
  unsigned i;
  int status = 0;
  for (i = 0; i < 64 && !status; ++i) {
    status = sophia_lc_service(&r->client, 65536);
    pump(&r->peer);
  }
  return status;
}
static const struct sophia_lf_record *next(struct rig *r) {
  const struct sophia_lf_record *e = NULL;
  assert(!spin(r));
  assert(!sophia_lc_event(&r->client, &e));
  return e;
}
static void consume(struct rig *r) {
  assert(!sophia_lc_event_consume(&r->client));
  assert(!spin(r));
}
static struct rig *ready(void) {
  struct sophia_lf_negotiate o = offer();
  struct rig *r = rig_new(&o, NULL);
  const struct sophia_lf_record *e = next(r);
  uint64_t generation;
  assert(sophia_lc_ready(&r->client) && sophia_lc_epoch(&r->client) == EPOCH);
  assert(!memcmp(sophia_lc_limits(&r->client), &limits, sizeof(limits)));
  assert(sophia_lc_welcome(&r->client)->granted_chords == 2);
  /* Submitted and Negotiated are internal: the lock's announcement is first,
   * presented only after the object was read. */
  assert(e->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  assert(e->value.published.object_generation == 1 && r->peer.lock_reads);
  assert(sophia_lc_lock(&r->client, &generation)->phase == SOPHIA_LF_UNLOCKED &&
         generation == 1);
  consume(r);
  assert(r->peer.acked == r->peer.sequence);
  return r;
}

static void bootstrap_and_lock(void) {
  struct rig *r = ready();
  const struct sophia_lf_record *e;
  const struct sophia_lf_lock *lock;
  uint64_t generation;
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 2);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  lock = sophia_lc_lock(&r->client, &generation);
  assert(generation == 2 && lock->lock_epoch == 5 && lock->allocation_count == 2);
  assert(lock->allocations[1].allocation == 42);
  /* Not acknowledged until consumed. */
  assert(r->peer.acked < r->peer.sequence);
  consume(r);
  assert(r->peer.acked == r->peer.sequence);
  rig_free(r);
}

/* An announcement whose object was already replaced is consumed unread. */
static void superseded_announcement(void) {
  struct rig *r = ready();
  const struct sophia_lf_record *e;
  uint64_t generation;
  unsigned reads = r->peer.lock_reads;
  publish(&r->peer, SOPHIA_LF_LOCKING, 5, 1);
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_OBJECT_PUBLISHED &&
         e->value.published.object_generation == 3);
  assert(sophia_lc_lock(&r->client, &generation)->phase == SOPHIA_LF_LOCKED &&
         generation == 3);
  assert(r->peer.lock_reads == reads + 2); /* one record read, one EOF probe */
  consume(r);
  rig_free(r);
}

static void entry_and_chord(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  const struct sophia_lf_record *e;
  memset(&v, 0, sizeof(v));
  v.value.entry.lock_epoch = 5;
  v.value.entry.entry = SOPHIA_LF_INSERT;
  event(&r->peer, SOPHIA_LF_ENTRY, &v);
  memset(&v, 0, sizeof(v));
  v.value.chord.lock_epoch = 5;
  v.value.chord.chord = 1;
  event(&r->peer, SOPHIA_LF_CHORD, &v);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_ENTRY && e->value.entry.entry == SOPHIA_LF_INSERT);
  consume(r);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_CHORD && e->value.chord.chord == 1);
  consume(r);
  /* A chord ID beyond the grant is a protocol violation. */
  v.value.chord.chord = 2;
  event(&r->peer, SOPHIA_LF_CHORD, &v);
  assert(spin(r) == SOPHIA_9P_INVALID &&
         sophia_lc_state(&r->client) == SOPHIA_LC_FAILED);
  rig_free(r);
}

static void upload_and_present(void) {
  struct rig *r = ready();
  struct sophia_lf_resource_begin begin;
  struct sophia_lf_record v;
  const struct sophia_lf_record *e;
  uint8_t pixels[32];
  unsigned i;
  for (i = 0; i < sizeof(pixels); ++i)
    pixels[i] = (uint8_t)(i * 7 + 1);
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
  next(r);
  consume(r);
  memset(&begin, 0, sizeof(begin));
  begin.transaction = 30;
  begin.resource.id = 21;
  begin.resource.generation = 2;
  begin.width_px = 4;
  begin.height_px = 2;
  begin.slot = 2; /* Limits grant two slots */
  assert(sophia_lc_upload_begin(&r->client, &begin) == SOPHIA_9P_ARGUMENT);
  begin.slot = 1;
  assert(!sophia_lc_upload_begin(&r->client, &begin));
  assert(sophia_lc_upload_begin(&r->client, &begin) == SOPHIA_9P_BUSY);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_RESOURCE_STATUS &&
         e->value.resource_status.status == SOPHIA_LF_ADMITTED);
  consume(r);
  assert(sophia_lc_upload_ready(&r->client));
  assert(sophia_lc_upload_end(&r->client) == SOPHIA_9P_ARGUMENT);
  assert(!sophia_lc_upload_chunk(&r->client, pixels, 12));
  assert(!sophia_lc_upload_ready(&r->client));
  assert(!spin(r) && sophia_lc_upload_ready(&r->client));
  assert(sophia_lc_upload_chunk(&r->client, pixels + 12, 21) == SOPHIA_9P_ARGUMENT);
  assert(!sophia_lc_upload_chunk(&r->client, pixels + 12, 20));
  assert(!spin(r));
  assert(!sophia_lc_upload_end(&r->client));
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_RESOURCE_STATUS &&
         e->value.resource_status.status == SOPHIA_LF_ACCEPTED);
  assert(!memcmp(r->peer.pixels, pixels, sizeof(pixels)));
  consume(r);
  assert(!sophia_lc_upload_pending(&r->client) && !r->peer.upload_fid);

  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_FRAME_DEMAND;
  v.value.frame_demand.transaction = 32;
  v.value.frame_demand.lock_epoch = 5;
  v.value.frame_demand.allocation = 41;
  v.value.frame_demand.allocation_generation = 4;
  v.value.frame_demand.demand = 3;
  assert(!sophia_lc_submit(&r->client, &v));
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_FRAME_PERMIT &&
         e->value.frame_permit.pacing_permit == 77);
  consume(r);
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_CANDIDATE;
  v.value.candidate.transaction = 33;
  v.value.candidate.lock_epoch = 5;
  v.value.candidate.output = 1;
  v.value.candidate.output_generation = 3;
  v.value.candidate.allocation = 41;
  v.value.candidate.allocation_generation = 4;
  v.value.candidate.candidate_generation = 1;
  v.value.candidate.pacing_permit = 77;
  v.value.candidate.resource = begin.resource;
  assert(!sophia_lc_submit(&r->client, &v));
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_CANDIDATE_OUTCOME &&
         e->value.candidate_outcome.status == SOPHIA_LF_PRESENTED);
  consume(r);
  /* Begin, End and Negotiate go through their own calls. */
  v.header.kind = SOPHIA_LF_RESOURCE_BEGIN;
  assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_ARGUMENT);
  rig_free(r);
}

static void rejected_and_refused_uploads(void) {
  struct rig *r = ready();
  struct sophia_lf_resource_begin begin;
  const struct sophia_lf_record *e;
  enum sophia_lc_submission stage;
  uint64_t id;
  uint32_t err;
  memset(&begin, 0, sizeof(begin));
  begin.transaction = 30;
  begin.resource.id = 21;
  begin.resource.generation = 2;
  begin.width_px = 4;
  begin.height_px = 2;
  r->peer.reject_begin = 1;
  assert(!sophia_lc_upload_begin(&r->client, &begin));
  e = next(r);
  assert(e->value.resource_status.status == SOPHIA_LF_RESOURCE_REJECTED);
  assert(!sophia_lc_upload_pending(&r->client));
  consume(r);
  /* A begin the server refuses outright journals nothing. */
  r->peer.reject_begin = 0;
  r->peer.submit_error = 22;
  begin.resource.generation = 3;
  assert(!sophia_lc_upload_begin(&r->client, &begin));
  assert(!spin(r) && !sophia_lc_upload_pending(&r->client));
  assert(!sophia_lc_submission(&r->client, &id, &stage, &err));
  assert(stage == SOPHIA_LC_SUBMISSION_REFUSED && err == 22);
  /* The connection lives on: the next upload is admitted. */
  begin.resource.generation = 4;
  assert(!sophia_lc_upload_begin(&r->client, &begin));
  e = next(r);
  assert(e->value.resource_status.status == SOPHIA_LF_ADMITTED);
  consume(r);
  assert(sophia_lc_upload_ready(&r->client));
  assert(!sophia_lc_upload_cancel(&r->client));
  e = next(r);
  assert(e->value.resource_status.status == SOPHIA_LF_CANCELLED);
  consume(r);
  assert(!sophia_lc_upload_pending(&r->client));
  rig_free(r);
}

static void again_then_retry(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  enum sophia_lc_submission stage;
  uint64_t id;
  unsigned submits;
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_RESOURCE_RETIRE;
  v.value.resource_step.transaction = 40;
  v.value.resource_step.resource.id = 21;
  v.value.resource_step.resource.generation = 2;
  r->peer.submit_again = 1;
  assert(sophia_lc_submit_retry(&r->client) == SOPHIA_9P_ARGUMENT);
  assert(!sophia_lc_submit(&r->client, &v));
  assert(!spin(r));
  submits = r->peer.submits;
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(id == 2 && stage == SOPHIA_LC_SUBMISSION_STAGED);
  assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_BUSY);
  assert(!spin(r) && r->peer.submits == submits); /* waits for the caller */
  assert(!sophia_lc_submit_retry(&r->client));
  assert(next(r)->header.kind == SOPHIA_LF_RESOURCE_RELEASED);
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(id == 2 && stage == SOPHIA_LC_SUBMISSION_CUSTODIED);
  consume(r);
  rig_free(r);
}

static void release_ack(struct peer *p) {
  assert(p->held_sequence);
  p->hold_acks = 0;
  p->acked = p->held_sequence;
  p->held_sequence = 0;
  count(p, p->held_tag, 16);
}
/* The transaction reopens only once the ack covering its Submitted was
 * answered, even when an earlier ack is still in flight. */
static void custody_waits_for_ack(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  enum sophia_lc_submission stage;
  uint64_t id;
  r->peer.hold_acks = 1;
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
  next(r);
  consume(r);
  assert(r->peer.held_sequence && r->peer.acked < r->peer.held_sequence);
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_RESOURCE_RETIRE;
  v.value.resource_step.transaction = 40;
  v.value.resource_step.resource.id = 21;
  v.value.resource_step.resource.generation = 2;
  assert(!sophia_lc_submit(&r->client, &v));
  assert(next(r)->header.kind == SOPHIA_LF_RESOURCE_RELEASED);
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(stage == SOPHIA_LC_SUBMISSION_CUSTODIED && !r->peer.tx_fid);
  assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_BUSY);
  release_ack(&r->peer);
  assert(!spin(r) && r->peer.tx_fid && r->peer.acked == r->peer.accepted_submitted);
  consume(r);
  rig_free(r);
}

static void refused_negotiation(void) {
  struct sophia_lf_negotiate o = offer();
  struct rig *r;
  const struct sophia_lf_record *e;
  o.minimum_revision = o.maximum_revision = 2;
  r = rig_new(&o, NULL);
  assert(spin(r) == SOPHIA_9P_INVALID);
  assert(sophia_lc_state(&r->client) == SOPHIA_LC_REFUSED);
  assert(sophia_lc_refusal(&r->client) == SOPHIA_LF_UNSUPPORTED_REVISION);
  assert(r->peer.acked == r->peer.refused); /* the refusal was acknowledged */
  assert(sophia_lc_event(&r->client, &e) == SOPHIA_9P_INVALID);
  assert(!sophia_lc_limits(&r->client));
  rig_free(r);
}

static void stale_connection(void) {
  struct rig *r = ready();
  r->peer.stale_events = 1;
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1); /* wakes the parked read */
  assert(spin(r) == SOPHIA_9P_INVALID);
  assert(sophia_lc_state(&r->client) == SOPHIA_LC_STALE &&
         sophia_lc_remote_error(&r->client) == 116);
  rig_free(r);
}

static void refuses_bad_peers(void) {
  struct sophia_lf_negotiate o = offer();
  struct rig *r = rig_new(&o, "sophia-lock-files version=1 epoch=7\nx");
  assert(spin(r) == SOPHIA_9P_INVALID &&
         sophia_lc_state(&r->client) == SOPHIA_LC_FAILED);
  rig_free(r);
  r = rig_new(&o, "sophia-lock-files version=1 epoch=07\n");
  assert(spin(r) == SOPHIA_9P_INVALID);
  rig_free(r);
  /* A grant beyond the request. */
  o.capabilities = SOPHIA_LF_CAP_PRESENT;
  o.chord_count = 0;
  r = rig_new(&o, NULL);
  r->peer.extra_grant = SOPHIA_LF_CAP_CHORDS;
  assert(spin(r) == SOPHIA_9P_INVALID);
  rig_free(r);
}

static void refuses_bad_offers(void) {
  struct sophia_lf_negotiate o = offer();
  struct sophia_lc_client c;
  struct sophia_9p_client wire;
  size_t bytes = sophia_9p_storage_bytes(4096, 8);
  void *storage = malloc(bytes);
  int sockets[2];
  assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, sockets));
  assert(!sophia_9p_init(&wire, sockets[0], 4096, 8, 32, storage, bytes));
  o.capabilities = SOPHIA_LF_CAP_CHORDS; /* present is required */
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer();
  o.capabilities = SOPHIA_LF_CAP_PRESENT; /* chords need their capability */
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer();
  o.chords[0].modifiers = 0;
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer();
  o.minimum_revision = 2;
  o.maximum_revision = 1;
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer(); /* the control: the same wire takes a valid offer */
  assert(!sophia_lc_init(&c, &wire, &o));
  close(sockets[0]);
  close(sockets[1]);
  free(storage);
}

int main(void) {
  bootstrap_and_lock();
  superseded_announcement();
  entry_and_chord();
  upload_and_present();
  rejected_and_refused_uploads();
  again_then_retry();
  custody_waits_for_ack();
  refused_negotiation();
  stale_connection();
  refuses_bad_peers();
  refuses_bad_offers();
  puts("lock_client_test ok");
  return 0;
}
