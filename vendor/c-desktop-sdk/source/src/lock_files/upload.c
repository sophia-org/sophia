#include "client_internal.h"

static int upload_step(struct sophia_lc_client *, uint16_t);

int sophia_lc_upload_ready(const struct sophia_lc_client *c) {
  return c && !c->terminal && c->upload_stage == LC_UPLOAD_READY &&
         !c->upload_closing && !c->upload_cancel_requested;
}
int sophia_lc_upload_pending(const struct sophia_lc_client *c) {
  return c && !c->terminal && c->upload_stage != LC_UPLOAD_IDLE;
}
int sophia_lc_upload_window(struct sophia_lc_client *c, unsigned writes) {
  if (!c || !writes || writes > SOPHIA_LC_MAX_UPLOAD_WRITES ||
      c->wire->capacity < writes + 5)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (c->upload_stage)
    return SOPHIA_9P_BUSY;
  c->upload_window = (uint8_t)writes;
  return 0;
}
static unsigned writes_pending(const struct sophia_lc_client *c) {
  unsigned i, n = 0;
  for (i = 0; i < SOPHIA_LC_MAX_UPLOAD_WRITES; ++i)
    n += c->upload_writes[i].op.active != 0;
  return n;
}
/* Each slot owns exactly one wire handle. Requests are issued in cursor order;
 * replies may arrive in any order. Offset - sent is the chunk's fixed base. */
static int write_drive(struct sophia_lc_client *c) {
  unsigned i;
  for (i = 0; i < c->upload_window && c->upload_issued < c->upload_size; ++i) {
    struct sophia_lc_upload_write *w = &c->upload_writes[i];
    uint32_t n;
    int r;
    if (w->op.active)
      continue;
    n = lc_cap(c, c->upload_size - c->upload_issued, 23, c->upload_iounit);
    r = sophia_9p_write(c->wire, c->upload_fid,
                        c->upload_offset - c->upload_sent + c->upload_issued,
                        c->upload_data + c->upload_issued, n, &w->op.handle);
    if (r)
      return r == SOPHIA_9P_BUSY ? 0 : r;
    w->op.active = 1;
    w->count = n;
    c->upload_issued += n;
  }
  return 0;
}
int sophia_lc_internal_upload_write_reply(struct sophia_lc_client *c,
                                          const struct sophia_9p_reply *r) {
  unsigned i;
  for (i = 0; i < SOPHIA_LC_MAX_UPLOAD_WRITES; ++i) {
    struct sophia_lc_upload_write *w = &c->upload_writes[i];
    if (!lc_same(&w->op, r->handle))
      continue;
    w->op.active = 0;
    if (r->type == 7) {
      c->remote_error = r->error;
      c->upload_cancel_requested = 1;
    } else {
      if (r->type != 119 || !r->count || r->count > w->count)
        return SOPHIA_9P_INVALID;
      c->upload_sent += r->count;
      c->upload_offset += r->count;
      if (r->count != w->count) {
        if (c->upload_window == 1)
          c->upload_issued -= w->count - r->count;
        else {
          /* Later writes may already have reached the old cursor. Never
           * guess which bytes the peer retained or replay them. */
          c->upload_cancel_requested = 1;
        }
      }
    }
    if (!writes_pending(c) && c->upload_sent == c->upload_size &&
        !c->upload_cancel_requested && !c->upload_closing) {
      c->upload_data = NULL;
      c->upload_stage = LC_UPLOAD_READY;
    }
    return 0;
  }
  return SOPHIA_9P_INVALID;
}
int sophia_lc_upload_begin(struct sophia_lc_client *c,
                           const struct sophia_lf_resource_begin *v) {
  struct sophia_lf_record r;
  int status;
  if (!c || !v)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (!sophia_lc_ready(c) || c->upload_stage)
    return SOPHIA_9P_BUSY;
  if (v->slot >= c->limits.upload_slots || v->width_px > c->limits.max_width_px ||
      v->height_px > c->limits.max_height_px ||
      (uint64_t)v->width_px * v->height_px * 4 > c->limits.max_resource_bytes)
    return SOPHIA_9P_ARGUMENT;
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_LF_RESOURCE_BEGIN;
  r.value.resource_begin = *v;
  status = sophia_lc_internal_queue(c, &r);
  if (!status) {
    c->upload = *v;
    c->upload_stage = LC_UPLOAD_BEGUN;
    c->upload_offset = 0;
    c->upload_size = c->upload_sent = c->upload_issued = 0;
    c->upload_data = NULL;
    c->upload_closing = c->upload_cancel_requested = 0;
  }
  return status;
}
int sophia_lc_internal_upload_drive(struct sophia_lc_client *c) {
  char slot[2];
  const char *names[2] = {"upload", slot};
  int r;
  if (!c->upload_stage || c->upload_op.active)
    return 0;
  if (c->upload_closing) {
    if (writes_pending(c))
      return 0;
    c->upload_data = NULL;
    c->upload_size = c->upload_sent = c->upload_issued = 0;
    if (c->upload_fid == UINT32_MAX) {
      c->upload_stage = LC_UPLOAD_IDLE;
      c->upload_closing = 0;
      return 0;
    }
    c->upload_stage = LC_UPLOAD_CLUNK;
  }
  if (!c->upload_closing && c->upload_cancel_requested &&
      (c->upload_stage == LC_UPLOAD_READY || c->upload_stage == LC_UPLOAD_WRITE)) {
    int status;
    if (writes_pending(c))
      return 0;
    c->upload_data = NULL;
    c->upload_stage = LC_UPLOAD_READY;
    status = upload_step(c, SOPHIA_LF_RESOURCE_CANCEL);
    return status == SOPHIA_9P_BUSY ? 0 : status;
  }
  switch (c->upload_stage) {
  case LC_UPLOAD_WALK:
    slot[0] = (char)('0' + c->upload.slot);
    slot[1] = 0;
    r = sophia_9p_walk(c->wire, c->root, names, 2, &c->upload_op.handle,
                       &c->upload_fid);
    break;
  case LC_UPLOAD_OPEN:
    r = sophia_9p_lopen(c->wire, c->upload_fid, 1, &c->upload_op.handle);
    break;
  case LC_UPLOAD_WRITE:
    return write_drive(c);
  case LC_UPLOAD_CLUNK:
    r = sophia_9p_clunk(c->wire, c->upload_fid, &c->upload_op.handle);
    break;
  default:
    return 0;
  }
  return lc_started(&c->upload_op, r);
}
/* A slot error ends this upload, never the connection: the slot binding is
 * the server's, and the resource's status says what happened. */
int sophia_lc_internal_upload_reply(struct sophia_lc_client *c,
                                    const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    c->remote_error = r->error;
    if (c->upload_stage == LC_UPLOAD_WALK || c->upload_stage == LC_UPLOAD_CLUNK)
      c->upload_fid = UINT32_MAX;
    c->upload_closing = 1;
    return 0;
  }
  switch (c->upload_stage) {
  case LC_UPLOAD_WALK:
    if (r->type != 111 || r->count != 2)
      return SOPHIA_9P_INVALID;
    c->upload_stage = LC_UPLOAD_OPEN;
    return 0;
  case LC_UPLOAD_OPEN:
    if (r->type != 13)
      return SOPHIA_9P_INVALID;
    c->upload_iounit = r->iounit;
    c->upload_stage = LC_UPLOAD_READY;
    return 0;
  case LC_UPLOAD_CLUNK:
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    c->upload_fid = UINT32_MAX;
    c->upload_stage = LC_UPLOAD_IDLE;
    c->upload_closing = 0;
    return 0;
  default:
    return SOPHIA_9P_INVALID;
  }
}
void sophia_lc_internal_upload_status(
    struct sophia_lc_client *c, const struct sophia_lf_resource_status *v) {
  if (!c->upload_stage || v->resource.id != c->upload.resource.id ||
      v->resource.generation != c->upload.resource.generation)
    return;
  if (v->status == SOPHIA_LF_ADMITTED && c->upload_stage == LC_UPLOAD_BEGUN &&
      !c->upload_closing)
    c->upload_stage = LC_UPLOAD_WALK;
  else if (v->status != SOPHIA_LF_ADMITTED)
    c->upload_closing = 1;
}
void sophia_lc_internal_upload_refused(struct sophia_lc_client *c,
                                       uint16_t kind) {
  if ((kind == SOPHIA_LF_RESOURCE_BEGIN &&
       c->upload_stage == LC_UPLOAD_BEGUN) ||
      ((kind == SOPHIA_LF_RESOURCE_END || kind == SOPHIA_LF_RESOURCE_CANCEL) &&
       c->upload_stage == LC_UPLOAD_ENDING))
    c->upload_closing = 1;
}
int sophia_lc_upload_chunk(struct sophia_lc_client *c, const void *bytes,
                           size_t count) {
  if (!c || !bytes || !count)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (c->upload_stage != LC_UPLOAD_READY || c->upload_closing ||
      c->upload_cancel_requested)
    return SOPHIA_9P_BUSY;
  if (count > (uint64_t)c->upload.width_px * c->upload.height_px * 4 -
                  c->upload_offset)
    return SOPHIA_9P_ARGUMENT;
  c->upload_data = bytes;
  c->upload_size = count;
  c->upload_sent = c->upload_issued = 0;
  c->upload_stage = LC_UPLOAD_WRITE;
  return 0;
}
static int upload_step(struct sophia_lc_client *c, uint16_t kind) {
  struct sophia_lf_record r;
  int status;
  if (!c)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (c->upload_stage != LC_UPLOAD_READY || c->upload_closing)
    return SOPHIA_9P_BUSY;
  if (kind == SOPHIA_LF_RESOURCE_END && c->upload_cancel_requested)
    return SOPHIA_9P_BUSY;
  if (kind == SOPHIA_LF_RESOURCE_END &&
      c->upload_offset != (uint64_t)c->upload.width_px * c->upload.height_px * 4)
    return SOPHIA_9P_ARGUMENT;
  memset(&r, 0, sizeof(r));
  r.header.kind = kind;
  r.value.resource_step.transaction = c->upload.transaction;
  r.value.resource_step.resource = c->upload.resource;
  if (kind == SOPHIA_LF_RESOURCE_END)
    r.value.resource_step.total_bytes = c->upload_offset;
  status = sophia_lc_internal_queue(c, &r);
  if (!status)
    c->upload_stage = LC_UPLOAD_ENDING;
  return status;
}
int sophia_lc_upload_end(struct sophia_lc_client *c) {
  return upload_step(c, SOPHIA_LF_RESOURCE_END);
}
int sophia_lc_upload_cancel(struct sophia_lc_client *c) {
  if (!c)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (!c->upload_stage || c->upload_stage >= LC_UPLOAD_ENDING || c->upload_closing)
    return SOPHIA_9P_BUSY;
  c->upload_cancel_requested = 1;
  return 0;
}
