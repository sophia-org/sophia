/* The scripted 9P2000.L lock peer and client rig shared by the lock client
 * tests; see lock_client_test.c for the custody rules it follows. */
#ifndef LOCK_CLIENT_PEER_H
#define LOCK_CLIENT_PEER_H
#include "sophia_lock_client.h"
#include "sophia_image_channel.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define EPOCH 7u
enum {
  F_ROOT = 1,
  F_API,
  F_LIMITS,
  F_LOCK_FILE,
  F_EVENTS,
  F_TX,
  F_SUBMIT,
  F_ACK,
  F_UPLOADS,
  F_UPLOAD, /* + slot */
  F_IMAGE = 32 /* + image file */
};
static const char *const image_names[] = {"image-channel", "image-limits",
                                          "image-formats"};
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
  uint16_t upload_tags[16];
  uint32_t upload_counts[16], upload_errors[16], upload_iounit;
  unsigned hold_upload, held_uploads, upload_short, upload_error, upload_ends;
  uint16_t held_tag;
  uint64_t held_sequence;
  uint32_t submit_error, extra_grant, error_after_custody;
  uint8_t last_bytes[SOPHIA_LF_MAX_CANDIDATE];
  size_t last_size;
  struct sophia_lf_resource_begin begin;
  /* Revision 2: the highest revision selected (0 means 1), an optional
   * forced selection and grant, the image discovery files and the last
   * ImageBegin. */
  uint16_t revision, force_selected;
  uint64_t force_grant;
  const uint8_t *image_data[3];
  size_t image_bytes[3];
  uint32_t image_error[3];
  unsigned image_walks[3], image_begins;
  struct sophia_ic_record image_begin;
  /* A ResourceCancel naming the last ImageBegin's resource, which the test
   * answers itself, and an upload End left unanswered. */
  unsigned image_cancels, hold_end;
  struct sophia_lf_resource_step image_cancel;
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
  assert(!sophia_lf_encode_revision(p->journal + p->journal_size,
                                    sizeof(p->journal) - p->journal_size,
                                    p->revision ? p->revision : 1, r, &n));
  p->journal_size += n;
}
/* Journals one image event at the next sequence. */
static inline void image_record(struct peer *p, struct sophia_ic_record *r) {
  size_t n;
  r->header.epoch = EPOCH;
  r->header.submission = 0;
  r->header.sequence = ++p->sequence;
  assert(sophia_ic_record_encode(p->journal + p->journal_size,
                                 sizeof(p->journal) - p->journal_size, r,
                                 &n) == SOPHIA_IC_OK);
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
  for (i = 0; i < 3; ++i)
    if (strlen(image_names[i]) == n && !memcmp(name, image_names[i], n))
      return F_IMAGE + i;
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
    uint16_t top = p->revision ? p->revision : 1;
    if (n->minimum_revision > top || n->maximum_revision < 1) {
      r.value.refusal = SOPHIA_LF_UNSUPPORTED_REVISION;
      event(p, SOPHIA_LF_REFUSED, &r);
      p->refused = p->sequence;
      return;
    }
    r.value.negotiated.granted_chords = n->chord_count;
    r.value.negotiated.selected_revision =
        n->maximum_revision < top ? n->maximum_revision : top;
    r.value.negotiated.granted_capabilities = n->capabilities | p->extra_grant;
    if (r.value.negotiated.selected_revision < 2)
      r.value.negotiated.granted_capabilities &= 3;
    if (p->force_selected)
      r.value.negotiated.selected_revision = p->force_selected;
    if (p->force_grant)
      r.value.negotiated.granted_capabilities = p->force_grant;
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
    ++p->upload_ends;
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
  int image = get(p->tx + 6, 2) == SOPHIA_IC_IMAGE_BEGIN;
  int image_cancel;
  assert(get(d, 8) == EPOCH && get(d + 16, 4) == p->tx_size && !get(d + 20, 4));
  memset(&c, 0, sizeof(c));
  if (image) {
    assert(sophia_ic_record_decode(p->tx, p->tx_size, &p->image_begin) ==
               SOPHIA_IC_OK &&
           p->image_begin.header.submission == id &&
           p->image_begin.header.epoch == EPOCH &&
           !p->image_begin.header.sequence);
    c.header.kind = SOPHIA_IC_IMAGE_BEGIN;
  } else
    assert(!sophia_lf_decode(p->tx, p->tx_size, &c) &&
           c.header.submission == id && c.header.epoch == EPOCH);
  image_cancel = c.header.kind == SOPHIA_LF_RESOURCE_CANCEL && p->image_begins &&
                 c.value.resource_step.resource.id ==
                     p->image_begin.value.image.resource_id;
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
      !image_cancel && c.value.resource_step.transaction != p->begin.transaction)
    p->submit_error = 22;
  if (p->submit_error) {
    error(p, tag, p->submit_error);
    p->submit_error = 0;
    return;
  }
  submitted(p, id, c.header.kind);
  if (p->error_after_custody) {
    events(p);
    error(p, tag, p->error_after_custody);
    return;
  }
  /* The test journals the ImageBegin's answers itself. */
  if (image)
    ++p->image_begins;
  else if (image_cancel) {
    ++p->image_cancels;
    p->image_cancel = c.value.resource_step;
  } else if (!(p->hold_end && c.header.kind == SOPHIA_LF_RESOURCE_END))
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
  if (file >= F_IMAGE) {
    ++p->image_walks[file - F_IMAGE];
    if (p->image_error[file - F_IMAGE]) {
      error(p, tag, p->image_error[file - F_IMAGE]);
      return;
    }
  }
  put(b, n, 2);
  if (n == 2) {
    const uint8_t *slot = q + 12 + length;
    assert(file == F_UPLOADS && get(slot, 2) == 1 && slot[2] >= '0' &&
           slot[2] <= '3');
    qid(b + 2, 0x80, F_UPLOADS);
    file = F_UPLOAD + (unsigned)(slot[2] - '0');
    qid(b + 15, 0, file);
  } else
    qid(b + 2, 0, file == F_LOCK_FILE ? p->qid : file);
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
    } else if (file >= F_UPLOAD && file < F_IMAGE) {
      assert(flags == 1);
      if (!p->upload_bound || file - F_UPLOAD != p->upload_slot) {
        error(p, tag, 116);
        return;
      }
      p->upload_fid = fid;
    } else
      assert(flags == (file == F_SUBMIT || file == F_ACK ? 1u : 0u));
    qid(b, 0, file == F_LOCK_FILE ? p->qid : file);
    if (file >= F_UPLOAD && file < F_IMAGE)
      put(b + 13, p->upload_iounit, 4);
    send_reply(p, 13, tag, b, 17);
  } else if (type == 116) {
    uint64_t offset = get(q + 4, 8);
    uint32_t n = (uint32_t)get(q + 12, 4);
    if (file >= F_IMAGE)
      data(p, tag, p->image_data[file - F_IMAGE], p->image_bytes[file - F_IMAGE],
           offset, n);
    else if (file == F_API)
      data(p, tag, (const uint8_t *)p->api, strlen(p->api), offset, n);
    else if (file == F_LIMITS)
      data(p, tag, p->limits, p->limits_size, offset, n);
    else if (file == F_LOCK_FILE) {
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
      uint32_t err = 0;
      assert(n <= p->upload_total - offset);
      if (p->upload_error) {
        err = p->upload_error;
        p->upload_error = 0;
      } else if (offset != p->upload_cursor) {
        err = 22; /* The cursor did not advance across a short/failed write. */
      } else {
        if (p->upload_short) {
          assert(n > 1);
          --n;
          p->upload_short = 0;
        }
        memcpy(p->pixels + offset, d, n);
        p->upload_cursor += n;
      }
      if (p->hold_upload) {
        unsigned at = p->held_uploads++;
        assert(at < 16);
        p->upload_tags[at] = tag;
        p->upload_counts[at] = n;
        p->upload_errors[at] = err;
      } else if (err)
        error(p, tag, err);
      else
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
static struct rig *rig_storage(const struct sophia_lf_negotiate *o,
                               const char *api, uint16_t capacity, int initialize) {
  struct rig *r = calloc(1, sizeof(*r));
  struct sophia_lf_record l;
  size_t bytes = sophia_9p_storage_bytes(8192, capacity);
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
  assert(!sophia_9p_init(&r->wire, r->client_fd, 8192, capacity, 32, r->storage,
                         bytes));
  if (initialize) assert(!sophia_lc_init(&r->client, &r->wire, o));
  return r;
}
static struct rig *rig_new_capacity(const struct sophia_lf_negotiate *o,
                                    const char *api, uint16_t capacity) {
  return rig_storage(o, api, capacity, 1);
}
static struct rig *rig_new(const struct sophia_lf_negotiate *o, const char *api) {
  return rig_new_capacity(o, api, 8);
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
#endif
