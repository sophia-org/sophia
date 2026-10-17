#include "../sophia_portal_requester_startup.h"
#include <string.h>
enum { WALK = 1, OPEN, READ, CLUNK, READY };
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int wire_error(struct sophia_pqo *s, int r) {
  s->wire_error = r;
  return -7;
}
static int drive(struct sophia_pqo *s) {
  int r;
  const char *name;
  if (s->active || s->stage == READY)
    return 0;
  switch (s->stage) {
  case WALK:
    name = s->limits_file ? "limits" : "api";
    r = sophia_9p_walk(s->wire, s->root, &name, 1, &s->pending, &s->fid);
    break;
  case OPEN:
    r = sophia_9p_lopen(s->wire, s->fid, 0, &s->pending);
    break;
  case READ:
    s->sent = (uint32_t)((s->limits_file ? 80 : 128) + 1 - s->used);
    if (s->sent > s->wire->msize - 11)
      s->sent = s->wire->msize - 11;
    if (s->iounit && s->sent > s->iounit)
      s->sent = s->iounit;
    r = sophia_9p_read(s->wire, s->fid, s->used, s->sent, &s->pending);
    break;
  case CLUNK:
    r = sophia_9p_clunk(s->wire, s->fid, &s->pending);
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  if (r)
    return wire_error(s, r);
  s->active = 1;
  return 0;
}
int sophia_pqo_init(struct sophia_pqo *s, struct sophia_9p_client *wire,
                    uint32_t root, uint64_t now, uint64_t deadline) {
  size_t i, bytes;
  if (!s || !wire || wire->terminal || wire->phase != 2 ||
      wire->fid_count != 1 || wire->fids[0] != root ||
      root == SOPHIA_9P_NOFID || wire->rx_used || wire->msize < 512 ||
      wire->msize > 65536 || wire->capacity < 2 ||
      wire->capacity > SOPHIA_9P_REQUESTS || wire->fid_limit < 2)
    return SOPHIA_PS_INVALID;
  if (deadline <= now)
    return SOPHIA_PS_DEADLINE;
  bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!bytes || !disjoint(s, sizeof *s, wire, sizeof *wire) ||
      !disjoint(s, sizeof *s, wire->storage, bytes))
    return SOPHIA_PS_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i)
    if (wire->slots[i].state)
      return SOPHIA_PS_INVALID;
  memset(s, 0, sizeof *s);
  s->wire = wire;
  s->root = root;
  s->fid = SOPHIA_9P_NOFID;
  s->stage = WALK;
  s->now = now;
  s->deadline = deadline;
  return s->error = drive(s);
}
static int accept(struct sophia_pqo *s, const struct sophia_9p_reply *r) {
  uint64_t epoch;
  size_t maximum = s->limits_file ? 80 : 128;
  if (r->type == 7) {
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
    s->iounit = r->iounit;
    s->stage = READ;
    break;
  case READ:
    if (r->type != 117 || r->count > s->sent || r->count > maximum - s->used ||
        (r->count && !r->data))
      return SOPHIA_PS_INVALID;
    if (r->count) {
      memcpy(s->bytes + s->used, r->data, r->count);
      s->used += r->count;
    } else {
      if (s->limits_file) {
        if (sophia_pq_decode_limits(s->bytes, s->used, &epoch, &s->limits) ||
            epoch != s->epoch)
          return SOPHIA_PS_INVALID;
      } else if (sophia_pq_decode_api(s->bytes, s->used, &s->epoch))
        return SOPHIA_PS_INVALID;
      s->stage = CLUNK;
    }
    break;
  case CLUNK:
    if (r->type != 121)
      return SOPHIA_PS_INVALID;
    if (s->limits_file)
      s->stage = READY;
    else {
      s->limits_file = 1;
      s->used = 0;
      s->iounit = 0;
      s->stage = WALK;
    }
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  return 0;
}
int sophia_pqo_dispatch(struct sophia_pqo *s, uint64_t now, size_t budget) {
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
  return s->error = drive(s);
}
int sophia_pqo_ready(const struct sophia_pqo *s) {
  return s && !s->error && s->stage == READY;
}
int sophia_pqo_wants_write(const struct sophia_pqo *s) {
  return s && !s->error && s->stage != READY && sophia_9p_wants_write(s->wire);
}
void sophia_pqo_close(struct sophia_pqo *s) {
  if (s && !s->error)
    s->error = SOPHIA_PS_CLOSED;
}
