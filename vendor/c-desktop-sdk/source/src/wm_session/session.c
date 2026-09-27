#include "internal.h"

size_t sophia_ws_state_bytes(void) { return sizeof(struct sophia_ws); }
size_t sophia_ws_storage_bytes(uint32_t msize) {
  size_t wire = sophia_9p_storage_bytes(msize, WS_REQUESTS);
  size_t extra = 2u * (size_t)SOPHIA_WF_MAX_RECORD + WS_JOURNAL_BYTES;
  return msize > 65536 || !wire || wire > SIZE_MAX - extra ? 0 : wire + extra;
}
int sophia_ws_open_fd(struct sophia_ws *s, int fd,
                      const struct sophia_ws_config *config, void *storage,
                      size_t bytes, uint64_t now) {
  size_t needed, wire;
  struct sophia_ws_config copied;
  uintptr_t a = (uintptr_t)s, b = (uintptr_t)storage;
  int r;
  if (!s || !config || !storage || fd < 0 ||
      config->bootstrap_deadline_ms <= now ||
      (config->offer.required & config->offer.optional))
    return SOPHIA_9P_ARGUMENT;
  needed = sophia_ws_storage_bytes(config->msize);
  if (!needed || bytes < needed || a > UINTPTR_MAX - sizeof(*s) ||
      b > UINTPTR_MAX - needed || !(a + sizeof(*s) <= b || b + needed <= a))
    return SOPHIA_9P_ARGUMENT;
  wire = sophia_9p_storage_bytes(config->msize, WS_REQUESTS);
  copied = *config;
  memset(s, 0, sizeof(*s));
  s->config = copied;
  s->state = SOPHIA_WS_NEGOTIATING;
  s->bootstrap_deadline = copied.bootstrap_deadline_ms;
  s->now = now;
  s->next_ticket = s->next_domain = 1;
  s->tx = (uint8_t *)storage + wire;
  s->object = s->tx + SOPHIA_WF_MAX_RECORD;
  s->journal = s->object + SOPHIA_WF_MAX_RECORD;
  r = sophia_9p_init(&s->wire, fd, copied.msize, WS_REQUESTS, WS_FIDS, storage,
                     wire);
  if (!r)
    r = ws_drive(s);
  return r ? ws_finish(s, r) : 0;
}
int ws_finish(struct sophia_ws *s, int status) {
  if (ws_final(s))
    return s->terminal;
  if (s->tx_ticket) {
    const struct ws_ticket *t = &s->tickets[s->tx_ticket % WS_OUTCOMES];
    if (t->id == s->tx_ticket && t->custody < SOPHIA_WS_SUBMITTED)
      ws_set(s, s->tx_ticket,
             t->custody == SOPHIA_WS_ISSUED ? SOPHIA_WS_UNKNOWN_DISCONNECTED
                                            : SOPHIA_WS_DROPPED_UNSENT,
             0);
  }
  s->terminal = status < 0 ? status : SOPHIA_9P_INVALID;
  s->state = status == SOPHIA_9P_CLOSED ? SOPHIA_WS_CLOSED
             : s->remote_error == 116   ? SOPHIA_WS_STALE
                                        : SOPHIA_WS_FAILED;
  s->event_ready = 0;
  s->object_stage = WS_OBJECT_IDLE;
  return s->terminal;
}
void sophia_ws_close(struct sophia_ws *s) {
  if (s)
    (void)ws_finish(s, SOPHIA_9P_CLOSED);
}
enum sophia_ws_state sophia_ws_state(const struct sophia_ws *s) {
  return s ? s->state : SOPHIA_WS_FAILED;
}
uint64_t sophia_ws_epoch(const struct sophia_ws *s) { return s ? s->epoch : 0; }
uint64_t sophia_ws_capabilities(const struct sophia_ws *s) {
  return s ? s->selected : 0;
}
const struct sophia_wf_limits *sophia_ws_limits(const struct sophia_ws *s) {
  return s && s->have_limits ? &s->limits : NULL;
}
uint32_t sophia_ws_remote_error(const struct sophia_ws *s) {
  return s ? s->remote_error : 0;
}
int sophia_ws_poll_fd(const struct sophia_ws *s) { return s ? s->wire.fd : -1; }
short sophia_ws_poll_events(const struct sophia_ws *s) {
  return !s || ws_final(s)
             ? 0
             : (short)(POLLIN |
                       (sophia_9p_wants_write(&s->wire) ? POLLOUT : 0));
}
static uint64_t earlier(uint64_t a, uint64_t b) {
  return !a || (b && b < a) ? b : a;
}
static uint64_t deadline(const struct sophia_ws *s) {
  uint64_t d = earlier(s->bootstrap_deadline, s->tx_deadline);
  d = earlier(d, s->assembly_deadline);
  d = earlier(d, s->object_deadline);
  return earlier(d, s->fragment_deadline);
}
int sophia_ws_timeout(const struct sophia_ws *s, uint64_t now) {
  uint64_t d;
  if (!s || ws_final(s))
    return -1;
  d = earlier(deadline(s), earlier(s->retry_at, s->object_retry_at));
  if (!d)
    return -1;
  if (d <= now)
    return 0;
  return d - now > INT_MAX ? INT_MAX : (int)(d - now);
}
int sophia_ws_obligations(const struct sophia_ws *s,
                          struct sophia_ws_obligations *out) {
  struct sophia_ws_obligations v;
  if (!s || !out)
    return SOPHIA_9P_ARGUMENT;
  memset(&v, 0, sizeof(v));
  v.consumed = s->consumed;
  v.acked = s->acked;
  v.submitted_sequence = s->submitted_sequence;
  if (!ws_final(s)) {
    v.deadline_ms = deadline(s);
    v.retry_at_ms = earlier(s->retry_at, s->object_retry_at);
    v.event_pending = s->event_ready;
    v.snapshot_pending =
        s->object_stage > WS_OBJECT_IDLE && s->object_stage < WS_OBJECT_READY;
    v.snapshot_ready = s->object_stage == WS_OBJECT_READY;
    v.waiting_for_ack =
        s->tx_stage == WS_TX_WALK && s->acked < s->submitted_sequence;
  }
  *out = v;
  return 0;
}
/* All completed replies in this bounded drain are handled before a submit
 * reply is judged. Its scalar value is copied; Rread storage is copied before
 * the generic pipeline consumes the reply. No borrowed reply outlives consume.
 */
static int drain(struct sophia_ws *s) {
  struct sophia_9p_reply r, submit;
  int status = 0, have_submit = 0, end = 0, result;
  unsigned visit;
  for (visit = 0; visit < WS_REQUESTS; ++visit) {
    result = sophia_9p_peek(&s->wire, &r);
    if (result == SOPHIA_9P_AGAIN)
      break;
    if (result) {
      end = result;
      break;
    }
    if (ws_same(&s->tx_op, r.handle)) {
      s->tx_op.active = 0;
      submit = r;
      submit.data = NULL;
      have_submit = 1;
      result = 0;
    } else if (ws_same(&s->event_op, r.handle)) {
      s->event_op.active = 0;
      if (r.type == 7) {
        s->remote_error = r.error;
        result = SOPHIA_9P_INVALID;
      } else if (!r.count)
        result = SOPHIA_9P_CLOSED;
      else
        result = ws_event_append(s, &r);
    } else if (ws_same(&s->boot_op, r.handle)) {
      s->boot_op.active = 0;
      result = ws_boot_reply(s, &r);
    } else if (ws_same(&s->object_op, r.handle)) {
      s->object_op.active = 0;
      result = ws_object_reply(s, &r);
    } else if (ws_same(&s->ack_op, r.handle)) {
      s->ack_op.active = 0;
      if (r.type == 7) {
        s->remote_error = r.error;
        result = SOPHIA_9P_INVALID;
      } else if (r.type != 119 || r.count != 16)
        result = SOPHIA_9P_INVALID;
      else {
        s->acked = s->ack_pending;
        result = 0;
      }
    } else
      result = SOPHIA_9P_INVALID;
    if (result && !status)
      status = result;
    result = sophia_9p_consume(&s->wire, r.handle);
    if (result) {
      end = result;
      break;
    }
  }
  result = ws_events_validate(s);
  if (result)
    return result;
  if (have_submit) {
    result = ws_tx_reply(s, &submit);
    if (result && !status)
      status = result;
  }
  if (status)
    return status;
  if (end)
    return end;
  return ws_event_head(s);
}
int sophia_ws_dispatch(struct sophia_ws *s, short revents, size_t budget,
                       uint64_t now) {
  int r;
  uint64_t due;
  if (!s || !budget || now < s->now || s->pass == UINT64_MAX)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  s->now = now;
  ++s->pass;
  if (revents & POLLNVAL)
    return ws_finish(s, SOPHIA_9P_CLOSED);
  due = deadline(s);
  if (due && due <= now) {
    /* Do not send queued bytes after their owning deadline. Completed
     * replies already held by the pipeline may still establish custody. */
    (void)drain(s);
    return ws_finish(s, SOPHIA_9P_CLOSED);
  }
  r = sophia_9p_service(&s->wire, budget);
  /* Clean EOF preserves the generic pipeline's final validated replies. */
  if (!r || r == SOPHIA_9P_CLOSED) {
    int drained = drain(s);
    if (drained)
      r = drained;
  }
  if (r)
    return ws_finish(s, r);
  if (s->negotiated && s->tx_stage == WS_TX_IDLE &&
      s->acked >= s->submitted_sequence)
    s->bootstrap_deadline = 0;
  due = deadline(s);
  if (due && due <= now)
    return ws_finish(s, SOPHIA_9P_CLOSED);
  r = ws_drive(s);
  return r ? ws_finish(s, r) : 0;
}
