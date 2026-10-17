#include "../sophia_admin_startup.h"
#include <string.h>

enum { ATTACH = 1, INFO, WALK_CTL, OPEN_CTL, READY };
static int wire_error(struct sophia_ao *s, int r) {
  s->wire_error = r;
  return SOPHIA_AO_WIRE_ERROR;
}
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
      (av + an <= bv || bv + bn <= av);
}
static int drive(struct sophia_ao *s) {
  int r;
  if (s->active || s->stage == READY) return 0;
  switch (s->stage) {
  case ATTACH: r = sophia_9p_attach(s->wire, "", "", &s->pending, &s->root); break;
  case INFO: {
    struct sophia_as_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    r = sophia_as_operation(&s->fetch, s->now, s->wire->msize, &op);
    if (r != 1) return r < 0 ? r : SOPHIA_AS_INVALID;
    switch (op.kind) {
    case SOPHIA_AS_WALK: {
      const char *name = sophia_as_name(op.file);
      r = sophia_9p_walk(s->wire, s->root, &name, 1, &s->pending, &fid); break;
    }
    case SOPHIA_AS_OPEN: r = sophia_9p_lopen(s->wire, op.fid, 0, &s->pending); break;
    case SOPHIA_AS_READ: r = sophia_9p_read(s->wire, op.fid, op.offset, op.count, &s->pending); break;
    case SOPHIA_AS_CLUNK: r = sophia_9p_clunk(s->wire, op.fid, &s->pending); break;
    default: return SOPHIA_AS_INVALID;
    }
    if (r) return wire_error(s, r);
    r = sophia_as_issued(&s->fetch, s->now, s->wire->msize, fid);
    if (r) return r;
    break;
  }
  case WALK_CTL: { const char *name = "ctl";
    r = sophia_9p_walk(s->wire, s->root, &name, 1, &s->pending, &s->ctl); break; }
  case OPEN_CTL: r = sophia_9p_lopen(s->wire, s->ctl, 1, &s->pending); break;
  default: return SOPHIA_AS_INVALID;
  }
  if (r) return wire_error(s, r);
  s->active = 1;
  return 0;
}
int sophia_ao_init(struct sophia_ao *s, struct sophia_9p_client *wire, uint64_t now, uint64_t deadline) {
  size_t i, wire_bytes;
  if (!s || !wire || wire->terminal || wire->phase != 2 || wire->fid_count || wire->rx_used ||
      wire->msize < 4096 || wire->msize > 65536 || wire->capacity < 2 ||
      wire->capacity > SOPHIA_9P_REQUESTS || wire->fid_limit < 4) return SOPHIA_AS_INVALID;
  if (deadline <= now) return SOPHIA_AS_DEADLINE;
  wire_bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!wire_bytes || !disjoint(s, sizeof *s, wire, sizeof *wire) ||
      !disjoint(s, sizeof *s, wire->storage, wire_bytes)) return SOPHIA_AS_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i) if (wire->slots[i].state) return SOPHIA_AS_INVALID;
  memset(s, 0, sizeof *s);
  s->wire = wire; s->stage = ATTACH; s->now = now; s->deadline = deadline;
  s->root = s->ctl = SOPHIA_9P_NOFID;
  if (sophia_as_init(&s->fetch, SOPHIA_AS_INFO, s->scratch, sizeof s->scratch, now, deadline)) return SOPHIA_AS_INVALID;
  s->error = drive(s);
  return s->error;
}
static int accept(struct sophia_ao *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) { s->remote_error = r->error; return SOPHIA_AS_REMOTE; }
  switch (s->stage) {
  case ATTACH:
    if (r->type != 105 || r->qid.type != 0x80) return SOPHIA_AS_INVALID;
    s->stage = INFO; break;
  case INFO: {
    const void *bytes;
    size_t length;
    int result = sophia_as_reply(&s->fetch, r, s->now);
    if (result) return result;
    if (sophia_as_result(&s->fetch, &bytes, &length)) {
      if (sophia_af_decode_info(bytes, length, &s->info) || s->info.command_ms > 10000 ||
          s->info.frame_ms > 2000 || s->info.idle_ms > 60000) return SOPHIA_AS_INVALID;
      s->stage = WALK_CTL;
    }
    break;
  }
  case WALK_CTL:
    if (r->type != 111 || r->count != 1 || r->qid.type) return SOPHIA_AS_INVALID;
    s->stage = OPEN_CTL; break;
  case OPEN_CTL:
    if (r->type != 13 || r->qid.type || (r->iounit && r->iounit < 512)) return SOPHIA_AS_INVALID;
    s->stage = READY; break;
  default: return SOPHIA_AS_INVALID;
  }
  return 0;
}
int sophia_ao_dispatch(struct sophia_ao *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  int r;
  if (!s) return SOPHIA_AS_INVALID;
  if (now < s->now) return SOPHIA_AS_CLOCK_REGRESSION;
  if (s->error) return s->error;
  s->now = now;
  if (s->stage == READY) return 0;
  if (now >= s->deadline) return s->error = SOPHIA_AS_DEADLINE;
  r = sophia_9p_service(s->wire, budget);
  if (r < 0) return s->error = wire_error(s, r);
  r = sophia_9p_peek(s->wire, &reply);
  if (r == SOPHIA_9P_AGAIN) return 0;
  if (r) return s->error = wire_error(s, r);
  if (!s->active || reply.handle.slot != s->pending.slot || reply.handle.serial != s->pending.serial)
    return s->error = SOPHIA_AS_INVALID;
  r = accept(s, &reply);
  if (r) return s->error = r;
  r = sophia_9p_consume(s->wire, reply.handle);
  if (r) return s->error = wire_error(s, r);
  s->active = 0;
  r = drive(s);
  return r ? (s->error = r) : 0;
}
int sophia_ao_ready(const struct sophia_ao *s) { return s && !s->error && s->stage == READY; }
int sophia_ao_wants_write(const struct sophia_ao *s) {
  return s && !s->error && s->stage != READY && sophia_9p_wants_write(s->wire);
}
void sophia_ao_close(struct sophia_ao *s) { if (s && !s->error) s->error = SOPHIA_AS_CLOSED; }
