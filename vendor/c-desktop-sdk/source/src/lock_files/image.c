/* Lock r2 seams for the image adapter: image event routing, ImageBegin
 * custody and whole-file reads of the image discovery files. With no adapter
 * attached every path here is inert, and an image event is invalid. */
#include "client_internal.h"
#include "../sophia_lock_image.h"

void sophia_lc_internal_image_record(struct sophia_lc_image *,
                                     const struct sophia_ic_record *);
void sophia_lc_internal_image_resource(struct sophia_lc_image *,
                                       const struct sophia_lf_resource_status *);
void sophia_lc_internal_image_refused(struct sophia_lc_image *);

static int image_r2(const struct sophia_lc_client *c) {
  return c->image && c->negotiated &&
         c->welcome.selected_revision == SOPHIA_LF_REVISION_IMAGE_CHANNEL &&
         (c->welcome.granted_capabilities & SOPHIA_LF_CAP_IMAGE_SEALED);
}
int sophia_lc_internal_image_kind(uint16_t kind) {
  return kind >= SOPHIA_IC_IMAGE_ADMITTED && kind <= SOPHIA_IC_CHANNEL_CLOSED;
}
int sophia_lc_internal_image_event(struct sophia_lc_client *c, const uint8_t *b,
                                   size_t n) {
  struct sophia_ic_record v;
  if (!image_r2(c) || sophia_ic_record_decode(b, n, &v) != SOPHIA_IC_OK ||
      v.header.kind != lc_get(b + 6, 2) || v.header.epoch != c->epoch)
    return SOPHIA_9P_INVALID;
  sophia_lc_internal_image_record(c->image, &v);
  return 0;
}
void sophia_lc_internal_image_status(struct sophia_lc_client *c,
                                     const struct sophia_lf_resource_status *v) {
  if (image_r2(c))
    sophia_lc_internal_image_resource(c->image, v);
}
/* Custody (Submitted) or an authoritative refusal of the latest
 * submission, when it is the ImageBegin. */
void sophia_lc_internal_image_settled(struct sophia_lc_client *c, int refused) {
  if (!c->image_submission || c->image_submission != c->next_submission - 1)
    return;
  c->image_submission = 0;
  if (refused && c->image)
    sophia_lc_internal_image_refused(c->image);
}

int sophia_lc_internal_file_read(struct sophia_lc_client *c, const char *name,
                                 void *buf, size_t cap) {
  if (!name || !buf || !cap)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (!c->negotiated || c->file_stage || c->file_ready)
    return SOPHIA_9P_BUSY;
  c->file_name = name;
  c->file_buf = buf;
  c->file_cap = cap;
  c->file_used = 0;
  c->file_status = 0;
  c->file_error = 0;
  c->file_stage = 1;
  return 0;
}
int sophia_lc_internal_file_result(struct sophia_lc_client *c, size_t *bytes,
                                   uint32_t *error) {
  if (!c->file_ready)
    return SOPHIA_9P_BUSY;
  c->file_ready = 0;
  *bytes = c->file_used;
  *error = c->file_error;
  return c->file_status;
}
int sophia_lc_internal_file_drive(struct sophia_lc_client *c) {
  size_t room = c->file_cap - c->file_used;
  uint32_t count = c->wire->msize - 11u;
  int r;
  if (!c->file_stage || c->file_op.active)
    return 0;
  /* At capacity, one more byte proves the file is not larger. Bound the
   * room before narrowing it, so a large buffer cannot wrap the count. */
  if (!room)
    count = 1;
  else if (room < count)
    count = (uint32_t)room;
  if (c->file_iounit && count > c->file_iounit)
    count = c->file_iounit;
  switch (c->file_stage) {
  case 1:
    r = sophia_9p_walk(c->wire, c->root, &c->file_name, 1, &c->file_op.handle,
                       &c->file_fid);
    break;
  case 2:
    r = sophia_9p_lopen(c->wire, c->file_fid, 0, &c->file_op.handle);
    break;
  case 3:
    r = sophia_9p_read(c->wire, c->file_fid, c->file_used, count,
                       &c->file_op.handle);
    break;
  default:
    r = sophia_9p_clunk(c->wire, c->file_fid, &c->file_op.handle);
    break;
  }
  return lc_started(&c->file_op, r);
}
static int file_finish(struct sophia_lc_client *c, int status, uint32_t error) {
  c->file_status = status;
  c->file_error = error;
  c->file_stage = 4;
  return 0;
}
int sophia_lc_internal_file_reply(struct sophia_lc_client *c,
                                  const struct sophia_9p_reply *r) {
  if (c->file_stage == 4) {
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    c->file_stage = 0;
    c->file_fid = UINT32_MAX;
    c->file_ready = 1;
    return 0;
  }
  if (r->type == 7) {
    int status = r->error == 11 ? SOPHIA_9P_AGAIN : SOPHIA_9P_INVALID;
    if (c->file_stage == 1) {
      /* No fid was created; nothing to clunk. */
      c->file_stage = 0;
      c->file_fid = UINT32_MAX;
      c->file_ready = 1;
      c->file_status = status;
      c->file_error = r->error;
      return 0;
    }
    return file_finish(c, status, r->error);
  }
  if (c->file_stage == 1) {
    if (r->type != 111 || r->count != 1)
      return SOPHIA_9P_INVALID;
    c->file_stage = 2;
    return 0;
  }
  if (c->file_stage == 2) {
    if (r->type != 13)
      return SOPHIA_9P_INVALID;
    c->file_iounit = r->iounit;
    c->file_stage = 3;
    return 0;
  }
  if (r->type != 117)
    return file_finish(c, SOPHIA_9P_INVALID, 0);
  if (!r->count)
    return file_finish(c, 0, 0);
  if (r->count > c->file_cap - c->file_used)
    return file_finish(c, SOPHIA_9P_INVALID, 0);
  memcpy(c->file_buf + c->file_used, r->data, r->count);
  c->file_used += r->count;
  return 0;
}
