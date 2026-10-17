#include "../sophia_admin_snapshot.h"
#include <string.h>

enum { WALK = 1, OPEN, READ, CLUNK, READY };
const char *sophia_as_name(enum sophia_as_file file) {
  switch (file) { case SOPHIA_AS_INFO: return "info"; case SOPHIA_AS_COMMANDS: return "commands";
    case SOPHIA_AS_STATUS: return "status"; default: return NULL; }
}
size_t sophia_as_maximum(enum sophia_as_file file) {
  if (file == SOPHIA_AS_COMMANDS) return SOPHIA_AF_CATALOG_MAX_BYTES;
  return file == SOPHIA_AS_INFO || file == SOPHIA_AS_STATUS ? SOPHIA_AF_LINE_MAX_BYTES : 0;
}
int sophia_as_init(struct sophia_as_fetch *s, enum sophia_as_file file,
                    void *storage, size_t capacity, uint64_t now, uint64_t deadline) {
  size_t maximum = sophia_as_maximum(file);
  if (!s || !storage || !maximum || capacity < maximum) return SOPHIA_AS_INVALID;
  if (deadline <= now) return SOPHIA_AS_DEADLINE;
  memset(s, 0, sizeof *s);
  s->file = file; s->bytes = storage; s->maximum = maximum;
  s->now = now; s->deadline = deadline; s->fid = SOPHIA_9P_NOFID; s->stage = WALK;
  return 0;
}
static int check(const struct sophia_as_fetch *s, uint64_t now) {
  if (!s) return SOPHIA_AS_INVALID;
  if (now < s->now) return SOPHIA_AS_CLOCK_REGRESSION;
  if (s->error) return s->error;
  if (s->stage != READY && now >= s->deadline) return SOPHIA_AS_DEADLINE;
  return 0;
}
int sophia_as_operation(const struct sophia_as_fetch *s, uint64_t now, uint32_t msize,
                         struct sophia_as_operation *out) {
  struct sophia_as_operation op = {0};
  int r = check(s, now);
  if (r) return r;
  if (!out) return SOPHIA_AS_INVALID;
  if (s->outstanding || s->stage == READY) return 0;
  op.file = s->file; op.fid = s->fid;
  switch (s->stage) {
  case WALK: op.kind = SOPHIA_AS_WALK; break;
  case OPEN: op.kind = SOPHIA_AS_OPEN; break;
  case READ:
    if (msize <= 11) return SOPHIA_AS_INVALID;
    op.kind = SOPHIA_AS_READ; op.offset = s->used;
    op.count = (uint32_t)(s->maximum + 1 - s->used);
    if (op.count > msize - 11) op.count = msize - 11;
    if (s->iounit && op.count > s->iounit) op.count = s->iounit;
    break;
  case CLUNK: op.kind = SOPHIA_AS_CLUNK; break;
  default: return SOPHIA_AS_INVALID;
  }
  *out = op;
  return 1;
}
int sophia_as_issued(struct sophia_as_fetch *s, uint64_t now, uint32_t msize, uint32_t fid) {
  struct sophia_as_operation op;
  int r = sophia_as_operation(s, now, msize, &op);
  if (r <= 0) return r ? r : SOPHIA_AS_BUSY;
  if ((op.kind == SOPHIA_AS_WALK) != (fid != SOPHIA_9P_NOFID)) return SOPHIA_AS_INVALID;
  if (op.kind == SOPHIA_AS_WALK) s->fid = fid;
  s->sent = op.count; s->now = now; s->outstanding = 1;
  return 0;
}
static int validate(struct sophia_as_fetch *s) {
  switch (s->file) {
  case SOPHIA_AS_INFO: { struct sophia_af_info v; return sophia_af_decode_info(s->bytes, s->used, &v); }
  case SOPHIA_AS_COMMANDS: { struct sophia_af_catalog v; return sophia_af_decode_catalog(s->bytes, s->used, &v); }
  case SOPHIA_AS_STATUS: { struct sophia_af_status v; return sophia_af_decode_status(s->bytes, s->used, &v); }
  default: return -1;
  }
}
static int accept(struct sophia_as_fetch *s, const struct sophia_9p_reply *r, uint64_t now) {
  int error = check(s, now);
  if (error) return error;
  if (!r || !s->outstanding) return SOPHIA_AS_INVALID;
  s->now = now; s->outstanding = 0;
  if (r->type == 7) { s->remote_error = r->error; return SOPHIA_AS_REMOTE; }
  switch (s->stage) {
  case WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type) return SOPHIA_AS_INVALID;
    s->stage = OPEN; break;
  case OPEN:
    if (r->type != 13 || r->qid.type) return SOPHIA_AS_INVALID;
    s->iounit = r->iounit; s->stage = READ; break;
  case READ:
    if (r->type != 117 || r->count > s->sent || r->count > s->maximum - s->used ||
        (r->count && !r->data)) return SOPHIA_AS_INVALID;
    if (r->count) { memcpy(s->bytes + s->used, r->data, r->count); s->used += r->count; }
    else { if (validate(s)) return SOPHIA_AS_INVALID; s->stage = CLUNK; }
    break;
  case CLUNK:
    if (r->type != 121) return SOPHIA_AS_INVALID;
    s->stage = READY; break;
  default: return SOPHIA_AS_INVALID;
  }
  return 0;
}
int sophia_as_reply(struct sophia_as_fetch *s, const struct sophia_9p_reply *r, uint64_t now) {
  int error;
  if (!s) return SOPHIA_AS_INVALID;
  if (now < s->now) return SOPHIA_AS_CLOCK_REGRESSION;
  error = accept(s, r, now);
  if (error && !s->error) s->error = error;
  return error;
}
int sophia_as_result(const struct sophia_as_fetch *s, const void **bytes, size_t *length) {
  if (!s || !bytes || !length || s->error || s->stage != READY) return 0;
  *bytes = s->bytes; *length = s->used;
  return 1;
}
void sophia_as_close(struct sophia_as_fetch *s) { if (s && !s->error) s->error = SOPHIA_AS_CLOSED; }
