#include "../sophia_portal_requester_upload.h"
#include <string.h>
enum { WALK = 1, OPEN, WRITE, HELD, CLUNK, DONE };
static int fail(struct sophia_pqu *u, int e) {
  if (!u->error)
    u->error = e;
  return u->error;
}
void sophia_pqu_close(struct sophia_pqu *u) {
  if (u)
    fail(u, SOPHIA_PQS_CLOSED);
}
int sophia_pqu_held(const struct sophia_pqu *u) {
  return u && !u->error && u->stage == HELD;
}
int sophia_pqu_done(const struct sophia_pqu *u) {
  return u && !u->error && u->stage == DONE;
}
int sophia_pqu_init(struct sophia_pqu *u, const void *bytes, size_t length,
                    uint32_t maximum, uint64_t now, uint64_t deadline) {
  uintptr_t a = (uintptr_t)u, b = (uintptr_t)bytes;
  if (!u || (!bytes && length) || !maximum || maximum > 65536 ||
      length > maximum || a > UINTPTR_MAX - sizeof *u ||
      b > UINTPTR_MAX - length ||
      (length && !(a + sizeof *u <= b || b + length <= a)))
    return SOPHIA_PQS_INVALID;
  if (deadline <= now)
    return SOPHIA_PQS_DEADLINE;
  memset(u, 0, sizeof *u);
  u->bytes = bytes;
  u->length = length;
  u->now = now;
  u->deadline = deadline;
  u->stage = WALK;
  u->fid = SOPHIA_9P_NOFID;
  return 0;
}
static int advance(struct sophia_pqu *u, uint64_t now) {
  if (!u)
    return SOPHIA_PQS_INVALID;
  if (now < u->now)
    return SOPHIA_PQS_CLOCK;
  if (u->error)
    return u->error;
  u->now = now;
  if (u->stage != DONE && now >= u->deadline)
    return fail(u, SOPHIA_PQS_DEADLINE);
  return 0;
}
int sophia_pqu_operation(struct sophia_pqu *u, uint64_t now, uint32_t msize,
                         struct sophia_pqu_operation *out) {
  struct sophia_pqu_operation op = {0};
  int e;
  if (!out)
    return SOPHIA_PQS_INVALID;
  e = advance(u, now);
  if (e)
    return e;
  if (u->outstanding || u->stage == HELD || u->stage == DONE) {
    *out = op;
    return 0;
  }
  if (msize < 512)
    return fail(u, SOPHIA_PQS_INVALID);
  op.fid = u->fid;
  switch (u->stage) {
  case WALK:
    op.kind = SOPHIA_PQU_WALK;
    break;
  case OPEN:
    op.kind = SOPHIA_PQU_OPEN;
    break;
  case WRITE:
    op.kind = SOPHIA_PQU_WRITE;
    op.offset = u->written;
    op.bytes = u->bytes + u->written;
    op.length = u->length - u->written;
    if (op.length > msize - 23)
      op.length = msize - 23;
    if (u->iounit && op.length > u->iounit)
      op.length = u->iounit;
    break;
  case CLUNK:
    op.kind = SOPHIA_PQU_CLUNK;
    break;
  default:
    return fail(u, SOPHIA_PQS_PROTOCOL);
  }
  *out = op;
  return 0;
}
int sophia_pqu_issued(struct sophia_pqu *u, uint64_t now, uint32_t msize,
                      uint32_t fid) {
  struct sophia_pqu_operation op;
  int e = sophia_pqu_operation(u, now, msize, &op);
  if (e)
    return e;
  if (op.kind == SOPHIA_PQU_NONE)
    return SOPHIA_PQS_BUSY;
  if ((op.kind == SOPHIA_PQU_WALK) != (fid != SOPHIA_9P_NOFID))
    return SOPHIA_PQS_INVALID;
  u->sent = op.length;
  if (u->stage == WALK)
    u->fid = fid;
  u->outstanding = 1;
  return 0;
}
int sophia_pqu_release(struct sophia_pqu *u, uint64_t now) {
  int e = advance(u, now);
  if (e)
    return e;
  if (u->outstanding || u->stage != HELD)
    return SOPHIA_PQS_BUSY;
  u->stage = CLUNK;
  return 0;
}
int sophia_pqu_settle(struct sophia_pqu *u, uint64_t now) {
  int e = advance(u, now);
  if (e)
    return e;
  u->settling = 1;
  if (!u->outstanding && u->stage != DONE)
    u->stage = u->fid == SOPHIA_9P_NOFID ? DONE : CLUNK;
  return 0;
}
static int accept(struct sophia_pqu *u, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    u->remote_error = r->error;
    return SOPHIA_PQS_REMOTE;
  }
  switch (u->stage) {
  case WALK:
    if (r->type != 111 || r->count != 2 || !r->data || r->data[0] != 128 ||
        r->data[13] != 0)
      return SOPHIA_PQS_PROTOCOL;
    u->stage = OPEN;
    break;
  case OPEN:
    if (r->type != 13 || r->qid.type)
      return SOPHIA_PQS_PROTOCOL;
    u->iounit = r->iounit;
    u->stage = u->length ? WRITE : HELD;
    break;
  case WRITE:
    if (r->type != 119 || r->count != u->sent)
      return SOPHIA_PQS_PROTOCOL;
    u->written += u->sent;
    if (u->written == u->length)
      u->stage = HELD;
    break;
  case CLUNK:
    if (r->type != 121)
      return SOPHIA_PQS_PROTOCOL;
    u->stage = DONE;
    break;
  default:
    return SOPHIA_PQS_PROTOCOL;
  }
  return 0;
}
int sophia_pqu_reply(struct sophia_pqu *u, const struct sophia_9p_reply *r,
                     uint64_t now) {
  int e = advance(u, now);
  if (e)
    return e;
  if (!r)
    return SOPHIA_PQS_INVALID;
  if (!u->outstanding)
    return SOPHIA_PQS_BUSY;
  u->outstanding = 0;
  if (u->settling &&
      (u->stage == WALK || u->stage == OPEN || u->stage == WRITE) &&
      r->type == 7 && r->error == 116) {
    u->stage = u->stage == WALK ? DONE : CLUNK;
    e = 0;
  } else
    e = accept(u, r);
  if (!e && u->settling && u->stage != DONE)
    u->stage = CLUNK;
  return e ? fail(u, e) : 0;
}
