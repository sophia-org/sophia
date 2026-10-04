#include "client_internal.h"

static void drop_head(struct sophia_lc_client *c) {
  size_t n = (size_t)lc_get(c->events, 4);
  c->consumed = c->event.header.sequence;
  c->event_used -= n;
  memmove(c->events, c->events + n, c->event_used);
}
static int negotiated(struct sophia_lc_client *c,
                      const struct sophia_lf_negotiated *v) {
  /* Session grants every requested chord or refuses the negotiation. */
  if (c->negotiated || c->refusal ||
      (v->granted_capabilities & ~c->offer.capabilities) ||
      v->granted_chords != c->offer.chord_count ||
      (v->granted_chords && !(v->granted_capabilities & SOPHIA_LF_CAP_CHORDS)))
    return SOPHIA_9P_INVALID;
  c->welcome = *v;
  c->negotiated = 1;
  return 0;
}
static int submitted(struct sophia_lc_client *c,
                     const struct sophia_lf_submitted *v, uint64_t sequence) {
  /* Custody cannot follow a definitive refusal of the same submission. */
  if (!c->submit_stage || c->submitted || c->submit_error ||
      v->submission != c->next_submission - 1 ||
      v->kind != lc_get(c->tx + 6, 2))
    return SOPHIA_9P_INVALID;
  c->submitted = 1;
  c->submitted_sequence = sequence;
  if (c->submit_replied)
    c->submit_stage = LC_TX_CLUNK;
  return 0;
}
/* Process events at the head in journal order. Negotiation and custody are
 * consumed here; an ObjectPublished holds the head until its object is read;
 * anything else is presented. Nothing behind a presented head is decoded. */
int sophia_lc_internal_parse(struct sophia_lc_client *c) {
  struct sophia_lf_record *r = &c->event;
  while (!c->event_ready && !c->object_stage && c->event_used >= 4) {
    size_t n = (size_t)lc_get(c->events, 4);
    int internal = 0;
    if (n < SOPHIA_LF_HEADER_BYTES || n > sizeof(c->events))
      return SOPHIA_9P_INVALID;
    if (n > c->event_used)
      return 0;
    if (sophia_lf_decode(c->events, n, r) || r->header.epoch != c->epoch ||
        r->header.kind >= 256 || r->header.sequence <= c->sequence ||
        c->refusal)
      return SOPHIA_9P_INVALID;
    if (!c->negotiated && r->header.kind != SOPHIA_LF_SUBMITTED &&
        r->header.kind != SOPHIA_LF_NEGOTIATED &&
        r->header.kind != SOPHIA_LF_REFUSED)
      return SOPHIA_9P_INVALID;
    switch (r->header.kind) {
    case SOPHIA_LF_NEGOTIATED:
      if (negotiated(c, &r->value.negotiated))
        return SOPHIA_9P_INVALID;
      internal = 1;
      break;
    case SOPHIA_LF_REFUSED:
      if (c->negotiated)
        return SOPHIA_9P_INVALID;
      c->refusal = r->value.refusal;
      internal = 1;
      break;
    case SOPHIA_LF_SUBMITTED:
      if (submitted(c, &r->value.submitted, r->header.sequence))
        return SOPHIA_9P_INVALID;
      internal = 1;
      break;
    case SOPHIA_LF_OBJECT_PUBLISHED:
      if (r->value.published.object_generation <= c->lock_generation ||
          !r->value.published.qid_path)
        return SOPHIA_9P_INVALID;
      c->object_generation = r->value.published.object_generation;
      c->object_qid = r->value.published.qid_path;
      c->object_used = 0;
      c->object_probe = 0;
      c->object_stage = LC_OBJECT_WALK;
      break;
    case SOPHIA_LF_RESOURCE_STATUS:
      sophia_lc_internal_upload_status(c, &r->value.resource_status);
      break;
    case SOPHIA_LF_CHORD:
      if (r->value.chord.chord >= c->welcome.granted_chords)
        return SOPHIA_9P_INVALID;
      break;
    default:
      break;
    }
    c->sequence = r->header.sequence;
    if (internal)
      drop_head(c);
    else if (!c->object_stage)
      c->event_ready = 1;
  }
  return 0;
}
int sophia_lc_event(struct sophia_lc_client *c,
                    const struct sophia_lf_record **out) {
  if (!c || !out)
    return SOPHIA_9P_ARGUMENT;
  if (!c->event_ready)
    return c->terminal ? c->terminal : SOPHIA_9P_AGAIN;
  *out = &c->event;
  return 0;
}
int sophia_lc_event_consume(struct sophia_lc_client *c) {
  int r;
  if (!c || !c->event_ready)
    return SOPHIA_9P_ARGUMENT;
  c->event_ready = 0;
  drop_head(c);
  if ((r = sophia_lc_internal_parse(c)))
    c->terminal = r;
  return r;
}
const struct sophia_lf_lock *sophia_lc_lock(const struct sophia_lc_client *c,
                                            uint64_t *generation) {
  if (!c || !c->have_lock)
    return NULL;
  if (generation)
    *generation = c->lock_generation;
  return &c->lock;
}

/* The lock file always names the newest object; a later qid than the one
 * announced means a newer publication, announced later in the journal. */
int sophia_lc_internal_object_drive(struct sophia_lc_client *c) {
  const char *name = "lock";
  int r;
  if (!c->object_stage || c->object_op.active)
    return 0;
  switch (c->object_stage) {
  case LC_OBJECT_WALK:
    r = sophia_9p_walk(c->wire, c->root, &name, 1, &c->object_op.handle,
                       &c->object_fid);
    break;
  case LC_OBJECT_OPEN:
    r = sophia_9p_lopen(c->wire, c->object_fid, 0, &c->object_op.handle);
    break;
  case LC_OBJECT_READ:
    r = sophia_9p_read(c->wire, c->object_fid, c->object_used,
                       c->object_probe
                           ? 1
                           : lc_cap(c, sizeof(c->object) - c->object_used, 11,
                                    c->object_iounit),
                       &c->object_op.handle);
    break;
  default:
    r = sophia_9p_clunk(c->wire, c->object_fid, &c->object_op.handle);
    break;
  }
  return lc_started(&c->object_op, r);
}
static int pinned(struct sophia_lc_client *c, uint64_t path) {
  if (path < c->object_qid)
    return SOPHIA_9P_INVALID;
  if (path > c->object_qid)
    c->object_probe = 2; /* superseded: release the fid unread */
  return 0;
}
int sophia_lc_internal_object_reply(struct sophia_lc_client *c,
                                    const struct sophia_9p_reply *r) {
  struct sophia_lf_record object;
  size_t n;
  if (r->type == 7) {
    if (c->object_stage == LC_OBJECT_WALK)
      c->object_fid = UINT32_MAX;
    return lc_remote(c, r->error);
  }
  switch (c->object_stage) {
  case LC_OBJECT_WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type ||
        pinned(c, r->qid.path))
      return SOPHIA_9P_INVALID;
    c->object_stage = c->object_probe == 2 ? LC_OBJECT_CLUNK : LC_OBJECT_OPEN;
    return 0;
  case LC_OBJECT_OPEN:
    if (r->type != 13 || r->qid.type || pinned(c, r->qid.path))
      return SOPHIA_9P_INVALID;
    c->object_iounit = r->iounit;
    c->object_stage = c->object_probe == 2 ? LC_OBJECT_CLUNK : LC_OBJECT_READ;
    return 0;
  case LC_OBJECT_READ:
    if (r->type != 117)
      return SOPHIA_9P_INVALID;
    if (!c->object_probe) {
      if (!r->count || r->count > sizeof(c->object) - c->object_used)
        return SOPHIA_9P_INVALID;
      memcpy(c->object + c->object_used, r->data, r->count);
      c->object_used += r->count;
      if (c->object_used < 4)
        return 0;
      n = (size_t)lc_get(c->object, 4);
      if (n < SOPHIA_LF_HEADER_BYTES || n > sizeof(c->object) ||
          c->object_used > n)
        return SOPHIA_9P_INVALID;
      /* A short read is not EOF: after the exact record, probe for EOF. */
      if (c->object_used == n)
        c->object_probe = 1;
      return 0;
    }
    if (r->count || sophia_lf_decode(c->object, c->object_used, &object) ||
        object.header.kind != SOPHIA_LF_LOCK || object.header.epoch != c->epoch)
      return SOPHIA_9P_INVALID;
    c->lock = object.value.lock;
    c->lock_generation = c->object_generation;
    c->have_lock = 1;
    c->object_stage = LC_OBJECT_CLUNK;
    return 0;
  case LC_OBJECT_CLUNK:
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    c->object_fid = UINT32_MAX;
    c->object_stage = LC_OBJECT_IDLE;
    /* The handle is released before the announcement is presented. */
    if (c->object_probe == 2)
      drop_head(c);
    else
      c->event_ready = 1;
    c->object_probe = 0;
    return sophia_lc_internal_parse(c);
  default:
    return SOPHIA_9P_INVALID;
  }
}

int sophia_lc_upload_ready(const struct sophia_lc_client *c) {
  return c && !c->terminal && c->upload_stage == LC_UPLOAD_READY &&
         !c->upload_closing;
}
int sophia_lc_upload_pending(const struct sophia_lc_client *c) {
  return c && !c->terminal && c->upload_stage != LC_UPLOAD_IDLE;
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
    c->upload_size = c->upload_sent = 0;
    c->upload_data = NULL;
    c->upload_closing = 0;
  }
  return status;
}
int sophia_lc_internal_upload_drive(struct sophia_lc_client *c) {
  char slot[2];
  const char *names[2] = {"upload", slot};
  size_t n;
  int r;
  if (!c->upload_stage || c->upload_op.active)
    return 0;
  if (c->upload_closing) {
    c->upload_data = NULL;
    c->upload_size = c->upload_sent = 0;
    if (c->upload_fid == UINT32_MAX) {
      c->upload_stage = LC_UPLOAD_IDLE;
      c->upload_closing = 0;
      return 0;
    }
    c->upload_stage = LC_UPLOAD_CLUNK;
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
    n = lc_cap(c, c->upload_size - c->upload_sent, 23, c->upload_iounit);
    r = sophia_9p_write(c->wire, c->upload_fid, c->upload_offset,
                        c->upload_data + c->upload_sent, n,
                        &c->upload_op.handle);
    break;
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
  case LC_UPLOAD_WRITE:
    if (r->type != 119 || !r->count || r->count > c->upload_size - c->upload_sent)
      return SOPHIA_9P_INVALID;
    c->upload_sent += r->count;
    c->upload_offset += r->count;
    if (c->upload_sent == c->upload_size) {
      c->upload_data = NULL;
      c->upload_stage = LC_UPLOAD_READY;
    }
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
  if (c->upload_stage != LC_UPLOAD_READY || c->upload_closing)
    return SOPHIA_9P_BUSY;
  if (count > (uint64_t)c->upload.width_px * c->upload.height_px * 4 -
                  c->upload_offset)
    return SOPHIA_9P_ARGUMENT;
  c->upload_data = bytes;
  c->upload_size = count;
  c->upload_sent = 0;
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
  return upload_step(c, SOPHIA_LF_RESOURCE_CANCEL);
}
