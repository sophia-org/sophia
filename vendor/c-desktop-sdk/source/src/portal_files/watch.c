#include "../sophia_portal_watch.h"
#include <string.h>
enum { WALK = 1, OPEN, READ, CLUNK, READY };
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int wire_error(struct sophia_pw *s, int r) {
  s->wire_error = r;
  return SOPHIA_PO_WIRE_ERROR;
}
static int drive(struct sophia_pw *s) {
  struct sophia_9p_client *w = s->opening->wire;
  int r;
  if (s->active || s->stage == READY)
    return 0;
  switch (s->stage) {
  case WALK: {
    const char *name = "watch";
    r = sophia_9p_walk(w, s->opening->root, &name, 1, &s->pending, &s->fid);
    break;
  }
  case OPEN:
    r = sophia_9p_lopen(w, s->fid, 0, &s->pending);
    break;
  case READ:
    r = sophia_9p_read(w, s->fid, 0, 1, &s->pending);
    break;
  case CLUNK:
    r = sophia_9p_clunk(w, s->fid, &s->pending);
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  if (r)
    return wire_error(s, r);
  s->active = 1;
  return 0;
}
int sophia_pw_init(struct sophia_pw *s, struct sophia_po *o, uint64_t now,
                   uint64_t deadline) {
  struct sophia_9p_client *w;
  size_t bytes, i;
  if (!s || !o || !sophia_po_ready(o))
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
      !disjoint(s, sizeof *s, w->storage, bytes))
    return SOPHIA_PS_INVALID;
  for (i = 0; i < 2u * w->capacity; ++i)
    if (w->slots[i].state)
      return SOPHIA_PS_INVALID;
  memset(s, 0, sizeof *s);
  s->opening = o;
  s->now = now;
  s->deadline = deadline;
  s->stage = WALK;
  s->fid = SOPHIA_9P_NOFID;
  sophia_po_close(o);
  return s->error = drive(s);
}
static int accept(struct sophia_pw *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    if (r->error == 116 && (s->stage == OPEN || s->stage == READ)) {
      s->stage = CLUNK;
      return 0;
    }
    s->remote_error = r->error;
    return SOPHIA_PS_REMOTE;
  }
  switch (s->stage) {
  case WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_PS_INVALID;
    s->stage = OPEN;
    break;
  case OPEN:
    if (r->type != 13 || r->qid.type)
      return SOPHIA_PS_INVALID;
    s->stage = READ;
    break;
  case CLUNK:
    if (r->type != 121)
      return SOPHIA_PS_INVALID;
    s->stage = READY;
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  return 0;
}
int sophia_pw_dispatch(struct sophia_pw *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  struct sophia_9p_client *w;
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
  r = accept(s, &reply);
  if (r)
    return s->error = r;
  r = sophia_9p_consume(w, reply.handle);
  if (r)
    return s->error = wire_error(s, r);
  s->active = 0;
  return s->error = drive(s);
}
int sophia_pw_waiting(const struct sophia_pw *s) {
  return s && !s->error && s->stage == READ;
}
int sophia_pw_revoked(const struct sophia_pw *s) {
  return s && !s->error && s->stage == READY;
}
int sophia_pw_wants_write(const struct sophia_pw *s) {
  return s && !s->error && s->stage != READY &&
         sophia_9p_wants_write(s->opening->wire);
}
int sophia_pw_finish(struct sophia_pw *s) {
  if (!sophia_pw_revoked(s))
    return SOPHIA_PS_INVALID;
  s->opening->now = s->now;
  s->opening->error = 0;
  s->opening = NULL;
  s->error = SOPHIA_PS_CLOSED;
  return 0;
}
void sophia_pw_close(struct sophia_pw *s) {
  if (s && !s->error)
    s->error = SOPHIA_PS_CLOSED;
}
