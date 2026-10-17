#include "../sophia_portal_startup.h"
#include <string.h>

enum { INFO = 1, READY };
static int wire_error(struct sophia_po *s, int r) {
  s->wire_error = r;
  return SOPHIA_PO_WIRE_ERROR;
}
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int drive(struct sophia_po *s) {
  int r;
  if (s->active || s->stage == READY)
    return 0;
  switch (s->stage) {
  case INFO: {
    struct sophia_ps_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    r = sophia_ps_operation(&s->fetch, s->now, s->wire->msize, &op);
    if (r != 1)
      return r < 0 ? r : SOPHIA_PS_INVALID;
    switch (op.kind) {
    case SOPHIA_PS_WALK: {
      const char *name = sophia_ps_name(op.file);
      r = sophia_9p_walk(s->wire, s->root, &name, 1, &s->pending, &fid);
      break;
    }
    case SOPHIA_PS_OPEN:
      r = sophia_9p_lopen(s->wire, op.fid, 0, &s->pending);
      break;
    case SOPHIA_PS_READ:
      r = sophia_9p_read(s->wire, op.fid, op.offset, op.count, &s->pending);
      break;
    case SOPHIA_PS_CLUNK:
      r = sophia_9p_clunk(s->wire, op.fid, &s->pending);
      break;
    default:
      return SOPHIA_PS_INVALID;
    }
    if (r)
      return wire_error(s, r);
    r = sophia_ps_issued(&s->fetch, s->now, s->wire->msize, fid);
    if (r)
      return r;
    break;
  }
  default:
    return SOPHIA_PS_INVALID;
  }
  if (r)
    return wire_error(s, r);
  s->active = 1;
  return 0;
}
int sophia_po_init(struct sophia_po *s, struct sophia_9p_client *wire,
                   uint32_t root, uint64_t now, uint64_t deadline) {
  size_t i, wire_bytes;
  if (!s || !wire || wire->terminal || wire->phase != 2 ||
      wire->fid_count != 1 || wire->fids[0] != root ||
      root == SOPHIA_9P_NOFID || wire->rx_used || wire->msize < 512 ||
      wire->msize > 65536 || wire->capacity < 2 ||
      wire->capacity > SOPHIA_9P_REQUESTS || wire->fid_limit < 2)
    return SOPHIA_PS_INVALID;
  if (deadline <= now)
    return SOPHIA_PS_DEADLINE;
  wire_bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!wire_bytes || !disjoint(s, sizeof *s, wire, sizeof *wire) ||
      !disjoint(s, sizeof *s, wire->storage, wire_bytes))
    return SOPHIA_PS_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i)
    if (wire->slots[i].state)
      return SOPHIA_PS_INVALID;
  memset(s, 0, sizeof *s);
  s->wire = wire;
  s->stage = INFO;
  s->now = now;
  s->deadline = deadline;
  s->root = root;
  if (sophia_ps_init(&s->fetch, SOPHIA_PS_API, s->scratch, sizeof s->scratch,
                     now, deadline))
    return SOPHIA_PS_INVALID;
  s->error = drive(s);
  return s->error;
}
static int accept(struct sophia_po *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    s->remote_error = r->error;
    return SOPHIA_PS_REMOTE;
  }
  switch (s->stage) {
  case INFO: {
    const void *bytes;
    size_t length;
    int result = sophia_ps_reply(&s->fetch, r, s->now);
    if (result)
      return result;
    if (sophia_ps_result(&s->fetch, &bytes, &length)) {
      if (sophia_pf_decode_api(bytes, length, &s->info))
        return SOPHIA_PS_INVALID;
      s->stage = READY;
    }
    break;
  }
  default:
    return SOPHIA_PS_INVALID;
  }
  return 0;
}
int sophia_po_dispatch(struct sophia_po *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  int r;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (now < s->now)
    return SOPHIA_PS_CLOCK_REGRESSION;
  if (s->error)
    return s->error;
  s->now = now;
  if (s->stage == READY)
    return 0;
  if (now >= s->deadline)
    return s->error = SOPHIA_PS_DEADLINE;
  r = sophia_9p_service(s->wire, budget);
  if (r < 0)
    return s->error = wire_error(s, r);
  r = sophia_9p_peek(s->wire, &reply);
  if (r == SOPHIA_9P_AGAIN)
    return 0;
  if (r)
    return s->error = wire_error(s, r);
  if (!s->active || reply.handle.slot != s->pending.slot ||
      reply.handle.serial != s->pending.serial)
    return s->error = SOPHIA_PS_INVALID;
  r = accept(s, &reply);
  if (r)
    return s->error = r;
  r = sophia_9p_consume(s->wire, reply.handle);
  if (r)
    return s->error = wire_error(s, r);
  s->active = 0;
  r = drive(s);
  return r ? (s->error = r) : 0;
}
int sophia_po_ready(const struct sophia_po *s) {
  return s && !s->error && s->stage == READY;
}
int sophia_po_wants_write(const struct sophia_po *s) {
  return s && !s->error && s->stage != READY && sophia_9p_wants_write(s->wire);
}
void sophia_po_close(struct sophia_po *s) {
  if (s && !s->error)
    s->error = SOPHIA_PS_CLOSED;
}
