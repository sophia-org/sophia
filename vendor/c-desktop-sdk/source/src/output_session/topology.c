#include "internal.h"

/* Fetch the object named by the head ObjectPublished. The export permits one
 * unacknowledged announcement, so topology always opens that object; the Qid
 * path is still checked on walk and open, and the decoded epochs must match.
 * The handle is clunked before the event is presented: at most one topology
 * handle is open, and an older pin never outlives its announcement here. */
int os_object_drive(struct sophia_os *s) {
  int r;
  if (s->object_op.active || s->object_stage == OS_OBJECT_IDLE ||
      s->object_stage == OS_OBJECT_READY || s->now < s->object_retry_at)
    return 0;
  if (s->object_stage == OS_OBJECT_WALK) {
    const char *name = "topology";
    r = sophia_9p_walk(&s->wire, s->root, &name, 1, &s->object_op.handle,
                       &s->object_fid);
  } else if (s->object_stage == OS_OBJECT_OPEN)
    r = sophia_9p_lopen(&s->wire, s->object_fid, 0, &s->object_op.handle);
  else if (s->object_stage == OS_OBJECT_READ) {
    size_t n = SOPHIA_OF_MAX_TOPOLOGY - s->object_used;
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
  return os_started(&s->object_op, r);
}
static int complete(struct sophia_os *s) {
  struct sophia_of_header h;
  if (sophia_of_topology_decode(s->object, s->object_used, &h, &s->topology)) {
    s->have_topology = 0;
    return SOPHIA_9P_INVALID;
  }
  if (h.epoch != s->epoch || s->topology.topology_epoch != s->object_epoch) {
    s->have_topology = 0;
    return SOPHIA_9P_INVALID;
  }
  s->have_topology = 1;
  return 0;
}
int os_object_reply(struct sophia_os *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    s->remote_error = r->error;
    /* Retry a withheld open within the announcement's deadline. */
    if ((s->object_stage == OS_OBJECT_OPEN ||
         s->object_stage == OS_OBJECT_WALK) &&
        r->error == 11) {
      if (s->object_stage == OS_OBJECT_OPEN)
        s->object_stage = OS_OBJECT_CLUNK; /* release, then walk again */
      s->object_retry_delay = os_backoff(s->object_retry_delay);
      s->object_retry_at = os_add(s->now, s->object_retry_delay);
      return 0;
    }
    return SOPHIA_9P_INVALID;
  }
  switch (s->object_stage) {
  case OS_OBJECT_WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type ||
        r->qid.path != s->object_qid)
      return SOPHIA_9P_INVALID;
    s->object_stage = OS_OBJECT_OPEN;
    break;
  case OS_OBJECT_OPEN:
    if (r->type != 13 || r->qid.type || r->qid.path != s->object_qid)
      return SOPHIA_9P_INVALID;
    s->object_iounit = r->iounit;
    s->object_retry_at = 0;
    s->object_stage = OS_OBJECT_READ;
    break;
  case OS_OBJECT_READ:
    if (r->type != 117 || r->count > SOPHIA_OF_MAX_TOPOLOGY - s->object_used)
      return SOPHIA_9P_INVALID;
    if (r->count) {
      size_t declared;
      memcpy(s->object + s->object_used, r->data, r->count);
      s->object_used += r->count;
      if (s->object_used < 4)
        return 0;
      declared = (size_t)of_get(s->object, 4);
      return declared < SOPHIA_OF_HEADER_BYTES ||
                     declared > SOPHIA_OF_MAX_TOPOLOGY ||
                     s->object_used > declared
                 ? SOPHIA_9P_INVALID
                 : 0;
    }
    if (complete(s))
      return SOPHIA_9P_INVALID;
    s->object_stage = OS_OBJECT_CLUNK;
    break;
  case OS_OBJECT_CLUNK:
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    if (s->object_retry_at) {
      /* A withheld open was released; walk again after the backoff. */
      s->object_stage = OS_OBJECT_WALK;
      break;
    }
    s->object_stage = OS_OBJECT_READY;
    s->object_deadline = 0;
    s->event_ready = 0;
    s->drive_pending = 1;
    return os_event_head(s);
  default:
    return SOPHIA_9P_INVALID;
  }
  return 0;
}
