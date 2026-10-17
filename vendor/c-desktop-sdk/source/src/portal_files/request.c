#include "request_internal.h"
#include <string.h>
enum { WALK = 1, OPEN, WRITE, CLUNK, WAIT, STATUS, READY };
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int fail(struct sophia_pcr *s, int error) {
  if (!s->error)
    s->error = error;
  sophia_pc_close(&s->capture);
  return s->error;
}
static int wire_error(struct sophia_pcr *s, int r) {
  s->wire_error = r;
  return SOPHIA_PCR_WIRE;
}
static int snapshot_error(struct sophia_pcr *s, int r) {
  s->snapshot_error = r;
  return SOPHIA_PCR_SNAPSHOT;
}
static int drive(struct sophia_pcr *s) {
  int r;
  if (s->active || s->stage == READY)
    return 0;
  if (s->stage == WAIT) {
    uint64_t wake, deadline;
    if (!sophia_pc_next_wake(&s->capture, &wake))
      return SOPHIA_PCR_INVALID;
    if (wake > s->now)
      return 0;
    if (sophia_pc_start_status(&s->capture, s->now, &deadline))
      return SOPHIA_PCR_CAPTURE;
    r = sophia_ps_init(&s->fetch, SOPHIA_PS_CAPTURE_STATUS, s->scratch,
                       sizeof s->scratch, s->now, deadline);
    if (r)
      return snapshot_error(s, r);
    s->stage = STATUS;
  }
  switch (s->stage) {
  case WALK: {
    const char *name = "capture";
    r = sophia_9p_walk(s->wire, s->root, &name, 1, &s->pending, &s->fid);
    break;
  }
  case OPEN:
    r = sophia_9p_lopen(s->wire, s->fid, 1, &s->pending);
    break;
  case WRITE:
    r = sophia_9p_write(s->wire, s->fid, 0, s->capture.bytes,
                        SOPHIA_PF_CAPTURE_BYTES, &s->pending);
    if (!r && sophia_pc_issued(&s->capture, s->now))
      return SOPHIA_PCR_CAPTURE;
    break;
  case CLUNK:
    r = sophia_9p_clunk(s->wire, s->fid, &s->pending);
    break;
  case STATUS: {
    struct sophia_ps_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    r = sophia_ps_operation(&s->fetch, s->now, s->wire->msize, &op);
    if (r != 1)
      return r < 0 ? snapshot_error(s, r) : SOPHIA_PCR_INVALID;
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
      return SOPHIA_PCR_INVALID;
    }
    if (r)
      return wire_error(s, r);
    r = sophia_ps_issued(&s->fetch, s->now, s->wire->msize, fid);
    if (r)
      return snapshot_error(s, r);
    break;
  }
  default:
    return SOPHIA_PCR_INVALID;
  }
  if (r)
    return wire_error(s, r);
  s->active = 1;
  return 0;
}
int sophia_pcr_prepare(struct sophia_pcr *s, struct sophia_9p_client *w,
                       uint32_t root, struct sophia_pf_capture_request request,
                       uint64_t previous_serial, uint64_t now,
                       uint64_t deadline) {
  struct sophia_pc capture;
  if (request.serial <= previous_serial)
    return SOPHIA_PCR_INVALID;
  if (sophia_pc_init(&capture, request, now, deadline))
    return SOPHIA_PCR_CAPTURE;
  memset(s, 0, sizeof *s);
  s->wire = w;
  s->root = root;
  s->capture = capture;
  s->serial = request.serial;
  s->now = now;
  s->deadline = deadline;
  s->fid = SOPHIA_9P_NOFID;
  s->stage = WALK;
  return 0;
}
int sophia_pcr_init(struct sophia_pcr *s, struct sophia_po *o,
                    struct sophia_pf_capture_request request,
                    uint64_t previous_serial, uint64_t now, uint64_t deadline) {
  struct sophia_9p_client *w;
  size_t bytes, i;
  int r;
  if (!s || !o || !sophia_po_ready(o))
    return SOPHIA_PCR_INVALID;
  if (now < o->now)
    return SOPHIA_PCR_CLOCK;
  if (o->info.request != SOPHIA_PF_OUTPUT_CAPTURE_V1)
    return SOPHIA_PCR_UNSUPPORTED;
  w = o->wire;
  if (!w || w->terminal || w->phase != 2 || w->fid_count != 1 ||
      o->root == SOPHIA_9P_NOFID || w->fids[0] != o->root || w->rx_used ||
      w->capacity < 2 || w->capacity > SOPHIA_9P_REQUESTS || w->fid_limit < 2 ||
      w->msize < 512 || w->msize > 65536)
    return SOPHIA_PCR_INVALID;
  bytes = sophia_9p_storage_bytes(w->offered, w->capacity);
  if (!bytes || !disjoint(s, sizeof *s, o, sizeof *o) ||
      !disjoint(s, sizeof *s, w, sizeof *w) ||
      !disjoint(s, sizeof *s, w->storage, bytes))
    return SOPHIA_PCR_INVALID;
  for (i = 0; i < 2u * w->capacity; ++i)
    if (w->slots[i].state)
      return SOPHIA_PCR_INVALID;
  r = sophia_pcr_prepare(s, w, o->root, request, previous_serial, now,
                         deadline);
  if (r)
    return r;
  s->opening = o;
  sophia_po_close(o);
  r = drive(s);
  return r ? fail(s, r) : 0;
}
static int accept(struct sophia_pcr *s, const struct sophia_9p_reply *r) {
  if (s->stage == WRITE) {
    if (r->type != 119 && r->type != 7)
      return SOPHIA_PCR_INVALID;
    if (sophia_pc_write_reply(&s->capture, r->type == 7,
                              r->type == 7 ? r->error : r->count, s->now))
      return SOPHIA_PCR_CAPTURE;
    s->stage = CLUNK;
    return 0;
  }
  if (r->type == 7) {
    s->remote_error = r->error;
    return SOPHIA_PCR_REMOTE;
  }
  if (s->stage == STATUS) {
    const void *bytes;
    size_t length;
    int result = sophia_ps_reply(&s->fetch, r, s->now);
    if (result)
      return snapshot_error(s, result);
    if (sophia_ps_result(&s->fetch, &bytes, &length)) {
      if (sophia_pc_status(&s->capture, bytes, length, s->now))
        return SOPHIA_PCR_CAPTURE;
      s->stage = s->capture.custody == SOPHIA_PC_SETTLED ? READY : WAIT;
    }
    return 0;
  }
  switch (s->stage) {
  case WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_PCR_INVALID;
    s->stage = OPEN;
    break;
  case OPEN:
    if (r->type != 13 || r->qid.type || (r->iounit && r->iounit < 24))
      return SOPHIA_PCR_INVALID;
    s->stage = WRITE;
    break;
  case CLUNK:
    if (r->type != 121)
      return SOPHIA_PCR_INVALID;
    s->stage = WAIT;
    break;
  default:
    return SOPHIA_PCR_INVALID;
  }
  return 0;
}
int sophia_pcr_step(struct sophia_pcr *s, uint64_t now) {
  if (now < s->now)
    return SOPHIA_PCR_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if (s->stage == READY)
    return 0;
  if (sophia_pc_advance(&s->capture, now))
    return SOPHIA_PCR_CAPTURE;
  return drive(s);
}
int sophia_pcr_accept(struct sophia_pcr *s,
                      const struct sophia_9p_reply *reply) {
  int r;
  if (!s->active || reply->handle.slot != s->pending.slot ||
      reply->handle.serial != s->pending.serial)
    return SOPHIA_PCR_INVALID;
  r = accept(s, reply);
  if (!r)
    s->active = 0;
  return r;
}
int sophia_pcr_dispatch(struct sophia_pcr *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  unsigned rounds;
  int r;
  if (!s)
    return SOPHIA_PCR_INVALID;
  if (now < s->now)
    return SOPHIA_PCR_CLOCK;
  r = sophia_pcr_step(s, now);
  if (r)
    return fail(s, r);
  if (s->stage == READY)
    return 0;
  r = sophia_9p_service(s->wire, budget);
  if (r < 0)
    return fail(s, wire_error(s, r));
  for (rounds = 0; rounds < 32; ++rounds) {
    r = sophia_9p_peek(s->wire, &reply);
    if (r == SOPHIA_9P_AGAIN)
      return 0;
    if (r)
      return fail(s, wire_error(s, r));
    r = sophia_pcr_accept(s, &reply);
    if (r)
      return fail(s, r);
    r = sophia_9p_consume(s->wire, reply.handle);
    if (r)
      return fail(s, wire_error(s, r));
    r = drive(s);
    if (r)
      return fail(s, r);
    if (s->stage == READY)
      return 0;
  }
  return 0;
}
int sophia_pcr_result(const struct sophia_pcr *s,
                      enum sophia_pf_capture_state *outcome) {
  if (!s || s->error || s->stage != READY || !outcome)
    return 0;
  *outcome = s->capture.outcome;
  return 1;
}
int sophia_pcr_finish(struct sophia_pcr *s,
                      enum sophia_pf_capture_state *outcome, uint64_t *serial) {
  if (!serial || !sophia_pcr_result(s, outcome))
    return SOPHIA_PCR_INVALID;
  *serial = s->serial;
  s->opening->error = 0;
  s->opening->now = s->now;
  s->opening = NULL;
  s->wire = NULL;
  s->error = SOPHIA_PCR_CLOSED;
  return 0;
}
int sophia_pcr_next_wake(const struct sophia_pcr *s, uint64_t *time) {
  if (!s || s->error || s->stage == READY || !time)
    return 0;
  if (s->stage == WAIT)
    return sophia_pc_next_wake(&s->capture, time);
  *time = s->deadline;
  return 1;
}
int sophia_pcr_wants_write(const struct sophia_pcr *s) {
  return s && !s->error && s->stage != READY && sophia_9p_wants_write(s->wire);
}
void sophia_pcr_close(struct sophia_pcr *s) {
  if (s)
    (void)fail(s, SOPHIA_PCR_CLOSED);
}
