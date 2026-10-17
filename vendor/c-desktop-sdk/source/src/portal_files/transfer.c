#include "../sophia_portal_transfer.h"
#include <string.h>
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int wire_error(struct sophia_pt *s, int r) {
  s->wire_error = r;
  return SOPHIA_PO_WIRE_ERROR;
}
static int drive(struct sophia_pt *s) {
  struct sophia_ps_operation op;
  struct sophia_9p_client *w = s->opening->wire;
  uint32_t fid = SOPHIA_9P_NOFID;
  int r;
  if (s->active)
    return 0;
  r = sophia_ps_operation(&s->fetch, s->now, w->msize, &op);
  if (r <= 0)
    return r;
  switch (op.kind) {
  case SOPHIA_PS_WALK: {
    const char *name = sophia_ps_name(op.file);
    r = sophia_9p_walk(w, s->opening->root, &name, 1, &s->pending, &fid);
    break;
  }
  case SOPHIA_PS_OPEN:
    r = sophia_9p_lopen(w, op.fid, 0, &s->pending);
    break;
  case SOPHIA_PS_READ:
    r = sophia_9p_read(w, op.fid, op.offset, op.count, &s->pending);
    break;
  case SOPHIA_PS_CLUNK:
    r = sophia_9p_clunk(w, op.fid, &s->pending);
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  if (r)
    return wire_error(s, r);
  r = sophia_ps_issued(&s->fetch, s->now, w->msize, fid);
  if (!r)
    s->active = 1;
  return r;
}
int sophia_pt_init(struct sophia_pt *s, struct sophia_po *o, void *storage,
                   size_t capacity, uint64_t now, uint64_t deadline) {
  struct sophia_9p_client *w;
  size_t bytes, i;
  if (!s || !o || !storage || !sophia_po_ready(o) ||
      capacity < SOPHIA_PS_TRANSFER_MAX_BYTES)
    return SOPHIA_PS_INVALID;
  if (now < o->now)
    return SOPHIA_PS_CLOCK_REGRESSION;
  if (deadline <= now)
    return SOPHIA_PS_DEADLINE;
  w = o->wire;
  if (!w || w->terminal || w->phase != 2 || w->fid_count != 1 ||
      w->fids[0] != o->root || o->root == SOPHIA_9P_NOFID || w->rx_used ||
      w->capacity < 2 || w->capacity > SOPHIA_9P_REQUESTS || w->fid_limit < 2 ||
      w->msize < 512 || w->msize > 65536)
    return SOPHIA_PS_INVALID;
  bytes = sophia_9p_storage_bytes(w->offered, w->capacity);
  if (!bytes || !disjoint(s, sizeof *s, o, sizeof *o) ||
      !disjoint(s, sizeof *s, w, sizeof *w) ||
      !disjoint(s, sizeof *s, w->storage, bytes) ||
      !disjoint(s, sizeof *s, storage, capacity) ||
      !disjoint(storage, capacity, o, sizeof *o) ||
      !disjoint(storage, capacity, w, sizeof *w) ||
      !disjoint(storage, capacity, w->storage, bytes))
    return SOPHIA_PS_INVALID;
  for (i = 0; i < 2u * w->capacity; ++i)
    if (w->slots[i].state)
      return SOPHIA_PS_INVALID;
  memset(s, 0, sizeof *s);
  s->opening = o;
  s->now = now;
  s->deadline = deadline;
  s->error = sophia_ps_init(&s->fetch, SOPHIA_PS_TRANSFER, storage, capacity,
                            now, deadline);
  if (s->error)
    return s->error;
  sophia_po_close(o);
  return s->error = drive(s);
}
int sophia_pt_result(const struct sophia_pt *s, const void **bytes,
                     size_t *length) {
  return s && !s->error && sophia_ps_result(&s->fetch, bytes, length);
}
int sophia_pt_dispatch(struct sophia_pt *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  struct sophia_9p_client *w;
  const void *bytes;
  size_t length;
  int r;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (now < s->now)
    return SOPHIA_PS_CLOCK_REGRESSION;
  if (s->error)
    return s->error;
  s->now = now;
  if (sophia_pt_result(s, &bytes, &length))
    return 0;
  if (now >= s->deadline)
    return s->error = SOPHIA_PS_DEADLINE;
  w = s->opening->wire;
  r = sophia_9p_service(w, budget);
  if (r < 0)
    return s->error = wire_error(s, r);
  r = sophia_9p_peek(w, &reply);
  if (r == SOPHIA_9P_AGAIN)
    return 0;
  if (r)
    return s->error = wire_error(s, r);
  if (!s->active || reply.handle.slot != s->pending.slot ||
      reply.handle.serial != s->pending.serial)
    return s->error = SOPHIA_PS_INVALID;
  r = sophia_ps_reply(&s->fetch, &reply, now);
  if (r)
    return s->error = r;
  r = sophia_9p_consume(w, reply.handle);
  if (r)
    return s->error = wire_error(s, r);
  s->active = 0;
  return s->error = drive(s);
}
int sophia_pt_wants_write(const struct sophia_pt *s) {
  return s && !s->error && s->opening &&
         sophia_9p_wants_write(s->opening->wire);
}
int sophia_pt_finish(struct sophia_pt *s, const void **bytes, size_t *length) {
  if (!sophia_pt_result(s, bytes, length))
    return SOPHIA_PS_INVALID;
  s->opening->now = s->now;
  s->opening->error = 0;
  s->opening = NULL;
  s->error = SOPHIA_PS_CLOSED;
  return 0;
}
void sophia_pt_close(struct sophia_pt *s) {
  if (s && !s->error)
    s->error = SOPHIA_PS_CLOSED;
}
size_t sophia_pt_received(const struct sophia_pt *s) {
  return s ? s->fetch.used : 0;
}
