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
