#include "internal.h"

int sophia_ws_snapshot(struct sophia_ws *s, uint64_t deadline) {
  if (!s || deadline <= s->now)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  if (s->object_stage != WS_OBJECT_IDLE)
    return SOPHIA_9P_BUSY;
  if (!s->event_ready || s->event.header.kind != SOPHIA_WF_CYCLE)
    return SOPHIA_9P_ARGUMENT;
  s->object_transaction = s->event.value.cycle.snapshot_transaction;
  s->object_scene = s->event.value.cycle.scene_generation;
  s->object_deadline = deadline;
  s->object_used = 0;
  s->object_retry_at = 0;
  s->object_retry_delay = 0;
  s->object_stage = WS_OBJECT_WALK;
  s->drive_pending = 1;
  return 0;
}
int ws_object_drive(struct sophia_ws *s) {
  int r;
  if (s->object_op.active || s->object_stage == WS_OBJECT_IDLE ||
      s->object_stage == WS_OBJECT_READY || s->now < s->object_retry_at)
    return 0;
  if (s->object_stage == WS_OBJECT_WALK) {
    const char *name = "snapshot";
    r = sophia_9p_walk(&s->wire, s->root, &name, 1, &s->object_op.handle,
                       &s->object_fid);
  } else if (s->object_stage == WS_OBJECT_OPEN)
    r = sophia_9p_lopen(&s->wire, s->object_fid, 0, &s->object_op.handle);
  else if (s->object_stage == WS_OBJECT_READ) {
    size_t n = SOPHIA_WF_MAX_RECORD - s->object_used;
    if (!n)
      n = 1; /* EOF probe at the cap, never appended. */
    if (n > s->wire.msize - 11u)
      n = s->wire.msize - 11u;
    if (s->object_iounit && n > s->object_iounit)
      n = s->object_iounit;
    r = sophia_9p_read(&s->wire, s->object_fid, s->object_used, (uint32_t)n,
                       &s->object_op.handle);
  } else
    r = sophia_9p_clunk(&s->wire, s->object_fid, &s->object_op.handle);
  return ws_started(&s->object_op, r);
}
int ws_object_reply(struct sophia_ws *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    s->remote_error = r->error;
    if (s->object_stage == WS_OBJECT_OPEN && r->error == 11) {
      s->object_retry_delay =
          s->object_retry_delay ? s->object_retry_delay * 2 : 4;
      if (s->object_retry_delay > 256)
        s->object_retry_delay = 256;
      s->object_retry_at = ws_add(s->now, s->object_retry_delay);
      return 0;
    }
    return SOPHIA_9P_INVALID;
  }
  if (s->object_stage == WS_OBJECT_WALK) {
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_9P_INVALID;
    s->object_stage = WS_OBJECT_OPEN;
  } else if (s->object_stage == WS_OBJECT_OPEN) {
    if (r->type != 13 || r->qid.type)
      return SOPHIA_9P_INVALID;
    s->object_iounit = r->iounit;
    s->object_retry_at = 0;
    s->object_stage = WS_OBJECT_READ;
  } else if (s->object_stage == WS_OBJECT_READ) {
    if (r->type != 117 || r->count > SOPHIA_WF_MAX_RECORD - s->object_used)
      return SOPHIA_9P_INVALID;
    if (r->count) {
      size_t declared;
      memcpy(s->object + s->object_used, r->data, r->count);
      s->object_used += r->count;
      if (s->object_used < 4)
        return 0;
      declared = (size_t)wf_get(s->object, 4);
      return declared < 32 || declared > SOPHIA_WF_MAX_RECORD ||
                     s->object_used > declared
                 ? SOPHIA_9P_INVALID
                 : 0;
    }
    if (sophia_wf_decode(s->object, s->object_used, s->selected,
                         &s->snapshot) ||
        s->snapshot.header.kind != SOPHIA_WF_SNAPSHOT ||
        s->snapshot.header.epoch != s->epoch ||
        s->snapshot.value.snapshot.transaction != s->object_transaction ||
        s->snapshot.value.snapshot.scene_generation != s->object_scene)
      return SOPHIA_9P_INVALID;
    s->object_stage = WS_OBJECT_CLUNK;
  } else if (s->object_stage == WS_OBJECT_CLUNK) {
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    s->object_stage = WS_OBJECT_READY;
    s->object_deadline = 0;
  } else
    return SOPHIA_9P_INVALID;
  return 0;
}
int sophia_ws_snapshot_result(struct sophia_ws *s,
                              const struct sophia_wf_record **out) {
  if (!s || !out)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  if (s->object_stage != WS_OBJECT_READY)
    return SOPHIA_9P_AGAIN;
  *out = &s->snapshot;
  return 0;
}
int sophia_ws_snapshot_release(struct sophia_ws *s) {
  if (!s || s->object_stage != WS_OBJECT_READY)
    return SOPHIA_9P_ARGUMENT;
  s->object_stage = WS_OBJECT_IDLE;
  return 0;
}
