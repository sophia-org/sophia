#include "frame_internal.h"
#include <string.h>
enum {
  WALK_INFO = 1,
  OPEN_INFO,
  INFO,
  WALK_FRAME,
  OPEN_FRAME,
  PIXELS,
  EOF_CHECK,
  RECHECK,
  CLUNK_FRAME,
  CLUNK_INFO,
  READY
};
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int wire_error(struct sophia_pfr *s, int e) {
  s->wire_error = e;
  return SOPHIA_PFR_WIRE;
}
static int drive(struct sophia_pfr *s) {
  int r = 0;
  size_t i;
  if (s->active || s->stage == READY)
    return 0;
  if (s->stage == PIXELS) {
    for (i = 0; i < 4; ++i) {
      struct sophia_pfr_read *slot = &s->reads[i];
      if (slot->active)
        continue;
      r = sophia_pr_reserve(&s->window, &slot->request);
      if (r < 0)
        return SOPHIA_PFR_INVALID;
      if (r) {
        r = sophia_9p_read(s->wire, s->frame_fid, slot->request.offset,
                           slot->request.count, &slot->handle);
        if (r)
          return wire_error(s, r);
        slot->active = 1;
      }
    }
    return 0;
  }
  switch (s->stage) {
  case WALK_INFO: {
    const char *n = "frame-info";
    r = sophia_9p_walk(s->wire, s->root, &n, 1, &s->pending, &s->info_fid);
    break;
  }
  case WALK_FRAME: {
    const char *n = "frame";
    r = sophia_9p_walk(s->wire, s->root, &n, 1, &s->pending, &s->frame_fid);
    break;
  }
  case OPEN_INFO:
    r = sophia_9p_lopen(s->wire, s->info_fid, 0, &s->pending);
    break;
  case OPEN_FRAME:
    r = sophia_9p_lopen(s->wire, s->frame_fid, 0, &s->pending);
    break;
  case INFO:
  case RECHECK:
    s->sent = (uint32_t)(65 - s->used);
    if (s->sent > s->wire->msize - 11)
      s->sent = s->wire->msize - 11;
    if (s->info_unit && s->sent > s->info_unit)
      s->sent = s->info_unit;
    r = sophia_9p_read(s->wire, s->info_fid, s->used, s->sent, &s->pending);
    break;
  case EOF_CHECK:
    r = sophia_9p_read(s->wire, s->frame_fid, s->length, 1, &s->pending);
    break;
  case CLUNK_FRAME:
    r = sophia_9p_clunk(s->wire, s->frame_fid, &s->pending);
    break;
  case CLUNK_INFO:
    r = sophia_9p_clunk(s->wire, s->info_fid, &s->pending);
    break;
  default:
    return SOPHIA_PFR_INVALID;
  }
  if (r)
    return wire_error(s, r);
  s->active = 1;
  return 0;
}
static int init(struct sophia_pfr *s, struct sophia_9p_client *w, uint32_t root,
                void *image, size_t capacity, uint64_t now, uint64_t deadline) {
  size_t bytes, i;
  if (!s || !image || !capacity)
    return SOPHIA_PFR_INVALID;
  if (deadline <= now)
    return SOPHIA_PFR_DEADLINE;
  if (!w || w->terminal || w->phase != 2 || w->fid_count != 1 ||
      w->fids[0] != root || w->rx_used || w->capacity < 4 ||
      w->capacity > SOPHIA_9P_REQUESTS || w->fid_limit < 3 || w->msize < 512 ||
      w->msize > 65536)
    return SOPHIA_PFR_INVALID;
  bytes = sophia_9p_storage_bytes(w->offered, w->capacity);
  if (!bytes || !disjoint(s, sizeof *s, w, sizeof *w) ||
      !disjoint(s, sizeof *s, w->storage, bytes) ||
      !disjoint(s, sizeof *s, image, capacity) ||
      !disjoint(image, capacity, w, sizeof *w) ||
      !disjoint(image, capacity, w->storage, bytes))
    return SOPHIA_PFR_INVALID;
  for (i = 0; i < 2u * w->capacity; ++i)
    if (w->slots[i].state)
      return SOPHIA_PFR_INVALID;
  if (sophia_pfr_prepare(s, w, root, image, capacity, now, deadline))
    return SOPHIA_PFR_INVALID;
  s->error = drive(s);
  return s->error;
}
int sophia_pfr_prepare(struct sophia_pfr *s, struct sophia_9p_client *w,
                       uint32_t root, void *image, size_t capacity,
                       uint64_t now, uint64_t deadline) {
  if (!image || !capacity)
    return SOPHIA_PFR_INVALID;
  if (deadline <= now)
    return SOPHIA_PFR_DEADLINE;
  memset(s, 0, sizeof *s);
  s->wire = w;
  s->root = root;
  s->pixels = image;
  s->capacity = capacity;
  s->now = now;
  s->deadline = deadline;
  s->stage = WALK_INFO;
  s->info_fid = s->frame_fid = SOPHIA_9P_NOFID;
  return 0;
}

int sophia_pfr_init(struct sophia_pfr *s, struct sophia_po *o, void *image,
                    size_t capacity, uint64_t now, uint64_t deadline) {
  int r;
  if (!s || !o || !sophia_po_ready(o) ||
      !disjoint(s, sizeof *s, o, sizeof *o) ||
      !disjoint(image, capacity, o, sizeof *o))
    return SOPHIA_PFR_INVALID;
  if (now < o->now)
    return SOPHIA_PFR_CLOCK;
  r = init(s, o->wire, o->root, image, capacity, now, deadline);
  if (!r || r == SOPHIA_PFR_WIRE)
    sophia_po_close(o);
  return r;
}
int sophia_pfr_restart(struct sophia_pfr *s, void *image, size_t capacity,
                       uint64_t now, uint64_t deadline) {
  if (!s || s->error || s->stage != READY)
    return SOPHIA_PFR_INVALID;
  if (now < s->now)
    return SOPHIA_PFR_CLOCK;
  return init(s, s->wire, s->root, image, capacity, now, deadline);
}
static int same(struct sophia_9p_handle a, struct sophia_9p_handle b) {
  return a.slot == b.slot && a.serial == b.serial;
}
static int accept(struct sophia_pfr *s, const struct sophia_9p_reply *r) {
  size_t i;
  if (r->type == 7) {
    s->remote_error = r->error;
    return SOPHIA_PFR_REMOTE;
  }
  if (s->stage == PIXELS) {
    for (i = 0; i < 4; ++i) {
      struct sophia_pfr_read *slot = &s->reads[i];
      uint64_t offset;
      if (!slot->active || !same(slot->handle, r->handle))
        continue;
      if (r->type != 117 || (r->count && !r->data) ||
          sophia_pr_accept(&s->window, slot->request.token, r->count, &offset))
        return SOPHIA_PFR_INVALID;
      memcpy(s->pixels + (size_t)offset, r->data, r->count);
      slot->active = 0;
      if (sophia_pr_payload_complete(&s->window))
        s->stage = EOF_CHECK;
      return 0;
    }
    return SOPHIA_PFR_INVALID;
  }
  if (!s->active || !same(s->pending, r->handle))
    return SOPHIA_PFR_INVALID;
  s->active = 0;
  switch (s->stage) {
  case WALK_INFO:
  case WALK_FRAME:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_PFR_INVALID;
    s->stage = s->stage == WALK_INFO ? OPEN_INFO : OPEN_FRAME;
    break;
  case OPEN_INFO:
    if (r->type != 13 || r->qid.type)
      return SOPHIA_PFR_INVALID;
    s->info_unit = r->iounit;
    s->stage = INFO;
    break;
  case OPEN_FRAME:
    if (r->type != 13 || r->qid.type ||
        sophia_pr_init(&s->window, s->length, s->wire->msize, r->iounit,
                       60 * 1024))
      return SOPHIA_PFR_INVALID;
    s->stage = PIXELS;
    break;
  case INFO:
  case RECHECK:
    if (r->type != 117 || r->count > s->sent || r->count > 64 - s->used ||
        (r->count && !r->data))
      return SOPHIA_PFR_INVALID;
    if (r->count) {
      memcpy((s->stage == INFO ? s->record : s->check) + s->used, r->data,
             r->count);
      s->used += r->count;
    } else {
      if (s->used != 64)
        return SOPHIA_PFR_INVALID;
      if (s->stage == RECHECK) {
        if (memcmp(s->record, s->check, 64))
          return SOPHIA_PFR_INVALID;
        s->stage = CLUNK_FRAME;
      } else {
        if (sophia_pf_decode_frame_info(s->record, 64, &s->info))
          return SOPHIA_PFR_INVALID;
        if (s->info.image_bytes > s->capacity)
          return SOPHIA_PFR_CAPACITY;
        s->length = (size_t)s->info.image_bytes;
        s->stage = WALK_FRAME;
      }
    }
    break;
  case EOF_CHECK:
    if (r->type != 117 || r->count)
      return SOPHIA_PFR_INVALID;
    s->used = 0;
    s->stage = RECHECK;
    break;
  case CLUNK_FRAME:
  case CLUNK_INFO:
    if (r->type != 121)
      return SOPHIA_PFR_INVALID;
    s->stage = s->stage == CLUNK_FRAME ? CLUNK_INFO : READY;
    break;
  default:
    return SOPHIA_PFR_INVALID;
  }
  return 0;
}
int sophia_pfr_matches(const struct sophia_pfr *s,
                       struct sophia_9p_handle tag) {
  size_t i;
  if (s->active && same(s->pending, tag))
    return 1;
  for (i = 0; i < 4; ++i)
    if (s->reads[i].active && same(s->reads[i].handle, tag))
      return 1;
  return 0;
}
int sophia_pfr_accept(struct sophia_pfr *s,
                      const struct sophia_9p_reply *reply) {
  if (!sophia_pfr_matches(s, reply->handle))
    return SOPHIA_PFR_INVALID;
  return accept(s, reply);
}
int sophia_pfr_step(struct sophia_pfr *s, uint64_t now) {
  if (now < s->now)
    return SOPHIA_PFR_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if (s->stage == READY)
    return 0;
  if (now >= s->deadline)
    return SOPHIA_PFR_DEADLINE;
  return drive(s);
}
int sophia_pfr_dispatch(struct sophia_pfr *s, uint64_t now, size_t budget) {
  unsigned i;
  int r;
  struct sophia_9p_reply reply;
  if (!s)
    return SOPHIA_PFR_INVALID;
  if (now < s->now)
    return SOPHIA_PFR_CLOCK;
  r = sophia_pfr_step(s, now);
  if (r)
    return s->error = r;
  if (s->stage == READY)
    return 0;
  r = sophia_9p_service(s->wire, budget);
  if (r < 0)
    return s->error = wire_error(s, r);
  for (i = 0; i < 32; ++i) {
    r = sophia_9p_peek(s->wire, &reply);
    if (r == SOPHIA_9P_AGAIN)
      return 0;
    if (r)
      return s->error = wire_error(s, r);
    r = sophia_pfr_accept(s, &reply);
    if (r)
      return s->error = r;
    r = sophia_9p_consume(s->wire, reply.handle);
    if (r)
      return s->error = wire_error(s, r);
    r = drive(s);
    if (r)
      return s->error = r;
    if (s->stage == READY)
      break;
  }
  return 0;
}
int sophia_pfr_wants_write(const struct sophia_pfr *s) {
  return s && !s->error && s->stage != READY && sophia_9p_wants_write(s->wire);
}
int sophia_pfr_result(const struct sophia_pfr *s,
                      struct sophia_pf_frame_info *info, const void **pixels,
                      size_t *length) {
  if (!s || !info || !pixels || !length || s->error || s->stage != READY)
    return 0;
  *info = s->info;
  *pixels = s->pixels;
  *length = s->length;
  return 1;
}
void sophia_pfr_close(struct sophia_pfr *s) {
  if (s && !s->error)
    s->error = SOPHIA_PFR_CLOSED;
}
uint64_t sophia_pfr_received(const struct sophia_pfr *s) {
  return s ? s->window.received : 0;
}
