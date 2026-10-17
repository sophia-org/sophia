#include "../sophia_portal_snapshot.h"
#include <string.h>

enum { WALK = 1, OPEN, READ, CLUNK, READY };
const char *sophia_ps_name(enum sophia_ps_file file) {
  switch (file) {
  case SOPHIA_PS_API:
    return "api";
  case SOPHIA_PS_CAPTURE_STATUS:
    return "capture-status";
  case SOPHIA_PS_STATUS:
    return "status";
  case SOPHIA_PS_TRANSFER:
    return "transfer";
  default:
    return NULL;
  }
}
size_t sophia_ps_maximum(enum sophia_ps_file file) {
  if (file == SOPHIA_PS_TRANSFER)
    return SOPHIA_PS_TRANSFER_MAX_BYTES;
  if (file == SOPHIA_PS_CAPTURE_STATUS)
    return SOPHIA_PF_CAPTURE_STATUS_BYTES;
  return file == SOPHIA_PS_API      ? SOPHIA_PF_API_MAX_BYTES
         : file == SOPHIA_PS_STATUS ? SOPHIA_PF_STATUS_MAX_BYTES
                                    : 0;
}
int sophia_ps_init(struct sophia_ps_fetch *s, enum sophia_ps_file file,
                   void *storage, size_t capacity, uint64_t now,
                   uint64_t deadline) {
  size_t maximum = sophia_ps_maximum(file);
  if (!s || !storage || !maximum || capacity < maximum)
    return SOPHIA_PS_INVALID;
  if (deadline <= now)
    return SOPHIA_PS_DEADLINE;
  memset(s, 0, sizeof *s);
  s->file = file;
  s->bytes = storage;
  s->maximum = maximum;
  s->now = now;
  s->deadline = deadline;
  s->fid = SOPHIA_9P_NOFID;
  s->stage = WALK;
  return 0;
}
static int check(const struct sophia_ps_fetch *s, uint64_t now) {
  if (!s)
    return SOPHIA_PS_INVALID;
  if (now < s->now)
    return SOPHIA_PS_CLOCK_REGRESSION;
  if (s->error)
    return s->error;
  if (s->stage != READY && now >= s->deadline)
    return SOPHIA_PS_DEADLINE;
  return 0;
}
int sophia_ps_operation(const struct sophia_ps_fetch *s, uint64_t now,
                        uint32_t msize, struct sophia_ps_operation *out) {
  struct sophia_ps_operation op = {0};
  int r = check(s, now);
  if (r)
    return r;
  if (!out)
    return SOPHIA_PS_INVALID;
  if (s->outstanding || s->stage == READY)
    return 0;
  op.file = s->file;
  op.fid = s->fid;
  switch (s->stage) {
  case WALK:
    op.kind = SOPHIA_PS_WALK;
    break;
  case OPEN:
    op.kind = SOPHIA_PS_OPEN;
    break;
  case READ:
    if (msize <= 11)
      return SOPHIA_PS_INVALID;
    op.kind = SOPHIA_PS_READ;
    op.offset = s->used;
    op.count = (uint32_t)(s->maximum + 1 - s->used);
    if (op.count > msize - 11)
      op.count = msize - 11;
    if (s->iounit && op.count > s->iounit)
      op.count = s->iounit;
    break;
  case CLUNK:
    op.kind = SOPHIA_PS_CLUNK;
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  *out = op;
  return 1;
}
int sophia_ps_issued(struct sophia_ps_fetch *s, uint64_t now, uint32_t msize,
                     uint32_t fid) {
  struct sophia_ps_operation op;
  int r = sophia_ps_operation(s, now, msize, &op);
  if (r <= 0)
    return r ? r : SOPHIA_PS_BUSY;
  if ((op.kind == SOPHIA_PS_WALK) != (fid != SOPHIA_9P_NOFID))
    return SOPHIA_PS_INVALID;
  if (op.kind == SOPHIA_PS_WALK)
    s->fid = fid;
  s->sent = op.count;
  s->now = now;
  s->outstanding = 1;
  return 0;
}
static int validate(struct sophia_ps_fetch *s) {
  switch (s->file) {
  case SOPHIA_PS_TRANSFER:
    return 0;
  case SOPHIA_PS_API: {
    struct sophia_pf_api v;
    return sophia_pf_decode_api(s->bytes, s->used, &v);
  }
  case SOPHIA_PS_CAPTURE_STATUS: {
    struct sophia_pf_capture_status v;
    return sophia_pf_decode_capture_status(s->bytes, s->used, &v);
  }
  case SOPHIA_PS_STATUS: {
    struct sophia_pf_status v;
    return sophia_pf_decode_status(s->bytes, s->used, &v);
  }
  default:
    return -1;
  }
}
static int accept(struct sophia_ps_fetch *s, const struct sophia_9p_reply *r,
                  uint64_t now) {
  int error = check(s, now);
  if (error)
    return error;
  if (!r || !s->outstanding)
    return SOPHIA_PS_INVALID;
  s->now = now;
  s->outstanding = 0;
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
    if (r->type != 117 || r->count > s->sent ||
        r->count > s->maximum - s->used || (r->count && !r->data))
      return SOPHIA_PS_INVALID;
    if (r->count) {
      memcpy(s->bytes + s->used, r->data, r->count);
      s->used += r->count;
    } else {
      if (validate(s))
        return SOPHIA_PS_INVALID;
      s->stage = CLUNK;
    }
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
int sophia_ps_reply(struct sophia_ps_fetch *s, const struct sophia_9p_reply *r,
                    uint64_t now) {
  int error;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (now < s->now)
    return SOPHIA_PS_CLOCK_REGRESSION;
  error = accept(s, r, now);
  if (error && !s->error)
    s->error = error;
  return error;
}
int sophia_ps_result(const struct sophia_ps_fetch *s, const void **bytes,
                     size_t *length) {
  if (!s || !bytes || !length || s->error || s->stage != READY)
    return 0;
  *bytes = s->bytes;
  *length = s->used;
  return 1;
}
void sophia_ps_close(struct sophia_ps_fetch *s) {
  if (s && !s->error)
    s->error = SOPHIA_PS_CLOSED;
}
