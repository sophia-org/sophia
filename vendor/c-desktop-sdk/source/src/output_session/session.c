#include "internal.h"

size_t sophia_os_state_bytes(void) { return sizeof(struct sophia_os); }
size_t sophia_os_storage_bytes(uint32_t msize) {
  size_t wire = sophia_9p_storage_bytes(msize, OS_REQUESTS);
  size_t extra = (size_t)SOPHIA_OF_MAX_CANDIDATE + SOPHIA_OF_MAX_TOPOLOGY +
                 OS_JOURNAL_BYTES;
  return msize > 65536 || !wire || wire > SIZE_MAX - extra ? 0 : wire + extra;
}
int sophia_os_open_fd(struct sophia_os *s, int fd,
                      const struct sophia_os_config *config, void *storage,
                      size_t bytes, uint64_t now) {
  size_t needed, wire;
  struct sophia_os_config copied;
  uintptr_t a = (uintptr_t)s, b = (uintptr_t)storage;
  int r;
  if (!s || !config || !storage || fd < 0 ||
      config->bootstrap_deadline_ms <= now ||
      !(config->offer.capabilities & SOPHIA_OF_CAP_OBSERVE))
    return SOPHIA_9P_ARGUMENT;
  needed = sophia_os_storage_bytes(config->msize);
  if (!needed || bytes < needed || a > UINTPTR_MAX - sizeof(*s) ||
      b > UINTPTR_MAX - needed || !(a + sizeof(*s) <= b || b + needed <= a))
    return SOPHIA_9P_ARGUMENT;
  wire = sophia_9p_storage_bytes(config->msize, OS_REQUESTS);
  copied = *config;
  memset(s, 0, sizeof(*s));
  s->config = copied;
  s->state = SOPHIA_OS_NEGOTIATING;
  s->bootstrap_deadline = copied.bootstrap_deadline_ms;
  s->now = now;
  s->next_ticket = s->next_domain = 1;
  s->tx = (uint8_t *)storage + wire;
  s->object = s->tx + SOPHIA_OF_MAX_CANDIDATE;
  s->journal = s->object + SOPHIA_OF_MAX_TOPOLOGY;
  r = sophia_9p_init(&s->wire, fd, copied.msize, OS_REQUESTS, OS_FIDS, storage,
                     wire);
  if (!r)
    r = os_drive(s);
  return r ? os_finish(s, r) : 0;
}
int os_finish(struct sophia_os *s, int status) {
  if (os_final(s))
    return s->terminal;
  if (s->tx_ticket) {
    const struct os_ticket *t = &s->tickets[s->tx_ticket % OS_OUTCOMES];
    if (t->id == s->tx_ticket && t->custody < SOPHIA_OS_SUBMITTED)
      os_set(s, s->tx_ticket,
             t->custody == SOPHIA_OS_ISSUED ? SOPHIA_OS_UNKNOWN_DISCONNECTED
                                            : SOPHIA_OS_DROPPED_UNSENT,
             0);
  }
  s->terminal = status < 0 ? status : SOPHIA_9P_INVALID;
  s->state = status == SOPHIA_9P_CLOSED ? SOPHIA_OS_CLOSED
             : s->remote_error == 116   ? SOPHIA_OS_STALE
                                        : SOPHIA_OS_FAILED;
  s->event_ready = 0;
  s->object_stage = OS_OBJECT_IDLE;
  return s->terminal;
}
void sophia_os_close(struct sophia_os *s) {
  if (s)
    (void)os_finish(s, SOPHIA_9P_CLOSED);
}
enum sophia_os_state sophia_os_state(const struct sophia_os *s) {
  return s ? s->state : SOPHIA_OS_FAILED;
}
uint64_t sophia_os_epoch(const struct sophia_os *s) { return s ? s->epoch : 0; }
uint64_t sophia_os_capabilities(const struct sophia_os *s) {
  return s ? s->selected : 0;
}
uint16_t sophia_os_refusal(const struct sophia_os *s) {
  return s ? s->refusal : 0;
}
const struct sophia_of_limits *sophia_os_limits(const struct sophia_os *s) {
  return s && s->have_limits ? &s->limits : NULL;
}
const struct sophia_of_topology *sophia_os_topology(const struct sophia_os *s) {
  return s && s->have_topology ? &s->topology : NULL;
}
uint32_t sophia_os_remote_error(const struct sophia_os *s) {
  return s ? s->remote_error : 0;
}
int sophia_os_poll_fd(const struct sophia_os *s) { return s ? s->wire.fd : -1; }
short sophia_os_poll_events(const struct sophia_os *s) {
  return !s || os_final(s)
             ? 0
             : (short)(POLLIN |
                       (sophia_9p_wants_write(&s->wire) ? POLLOUT : 0));
}
static uint64_t earlier(uint64_t a, uint64_t b) {
  return !a || (b && b < a) ? b : a;
}
static uint64_t deadline(const struct sophia_os *s) {
  uint64_t d = earlier(s->bootstrap_deadline, s->tx_deadline);
  d = earlier(d, s->assembly_deadline);
  d = earlier(d, s->object_deadline);
  return earlier(d, s->fragment_deadline);
}
int sophia_os_timeout(const struct sophia_os *s, uint64_t now) {
  uint64_t d;
  if (!s || os_final(s))
    return -1;
  if (s->drive_pending)
    return 0;
  d = earlier(deadline(s), earlier(s->retry_at, s->object_retry_at));
  if (!d)
    return -1;
  if (d <= now)
    return 0;
  return d - now > INT_MAX ? INT_MAX : (int)(d - now);
}
int sophia_os_obligations(const struct sophia_os *s,
                          struct sophia_os_obligations *out) {
  struct sophia_os_obligations v;
  if (!s || !out)
    return SOPHIA_9P_ARGUMENT;
  memset(&v, 0, sizeof(v));
  v.consumed = s->consumed;
  v.acked = s->acked;
  v.submitted_sequence = s->submitted_sequence;
  if (!os_final(s)) {
    v.deadline_ms = deadline(s);
    v.retry_at_ms = earlier(s->retry_at, s->object_retry_at);
    v.event_pending = s->event_ready;
    v.topology_pending =
        s->object_stage > OS_OBJECT_IDLE && s->object_stage < OS_OBJECT_READY;
    v.waiting_for_ack =
        s->tx_stage == OS_TX_WALK && s->acked < s->submitted_sequence;
  }
  *out = v;
  return 0;
}
static int ack_reply(struct sophia_os *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    s->remote_error = r->error;
    return SOPHIA_9P_INVALID;
  }
  if (r->type != 119 || r->count != SOPHIA_OF_ACK_BYTES)
    return SOPHIA_9P_INVALID;
  s->acked = s->ack_pending;
  return 0;
}
/* All completed replies in this bounded drain are handled before a submit
 * reply is judged, so a Submitted event in the batch wins either ordering.
 * Scalars are copied; no borrowed reply outlives consume. */
static int drain(struct sophia_os *s) {
  struct sophia_9p_reply r, submit;
  int status = 0, have_submit = 0, end = 0, result;
  unsigned visit;
  for (visit = 0; visit < OS_REQUESTS; ++visit) {
    result = sophia_9p_peek(&s->wire, &r);
    if (result == SOPHIA_9P_AGAIN)
      break;
    if (result) {
      end = result;
      break;
    }
    if (os_same(&s->tx_op, r.handle)) {
      s->tx_op.active = 0;
      submit = r;
      submit.data = NULL;
      have_submit = 1;
      result = 0;
    } else if (os_same(&s->event_op, r.handle)) {
      s->event_op.active = 0;
      if (r.type == 7) {
        s->remote_error = r.error;
        result = SOPHIA_9P_INVALID;
      } else if (!r.count)
        result = SOPHIA_9P_CLOSED;
      else
        result = os_event_append(s, &r);
    } else if (os_same(&s->boot_op, r.handle)) {
      s->boot_op.active = 0;
      result = os_boot_reply(s, &r);
    } else if (os_same(&s->object_op, r.handle)) {
      s->object_op.active = 0;
      result = os_object_reply(s, &r);
    } else if (os_same(&s->ack_op, r.handle)) {
      s->ack_op.active = 0;
      result = ack_reply(s, &r);
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
  result = os_events_validate(s);
  if (result)
    return result;
  if (have_submit) {
    result = os_tx_reply(s, &submit);
    if (result && !status)
      status = result;
  }
  if (status)
    return status;
  if (end)
    return end;
  return os_event_head(s);
}
int sophia_os_dispatch(struct sophia_os *s, short revents, size_t budget,
                       uint64_t now) {
  int r;
  uint64_t due;
  if (!s || !budget || now < s->now || s->pass == UINT64_MAX)
    return SOPHIA_9P_ARGUMENT;
  if (os_final(s))
    return s->terminal;
  s->now = now;
  ++s->pass;
  if (revents & POLLNVAL)
    return os_finish(s, SOPHIA_9P_CLOSED);
  due = deadline(s);
  if (due && due <= now) {
    /* Queued bytes are not sent after their owning deadline. Completed
     * replies already held by the pipeline may still establish custody. */
    (void)drain(s);
    return os_finish(s, SOPHIA_9P_CLOSED);
  }
  r = sophia_9p_service(&s->wire, budget);
  if (!r || r == SOPHIA_9P_CLOSED) {
    int drained = drain(s);
    if (drained)
      r = drained;
  }
  /* A refusal is terminal once its acknowledgement is confirmed. The export
   * then revokes the attach, which may fail a pending read in the same pass;
   * that revocation is the expected end, not a protocol failure. */
  if (s->refused_sequence && s->acked >= s->refused_sequence) {
    s->state = SOPHIA_OS_REFUSED;
    s->terminal = SOPHIA_9P_CLOSED;
    s->event_ready = 0;
    return s->terminal;
  }
  if (r)
    return os_finish(s, r);
  if (s->negotiated && s->tx_stage == OS_TX_IDLE &&
      s->acked >= s->submitted_sequence)
    s->bootstrap_deadline = 0;
  due = deadline(s);
  if (due && due <= now)
    return os_finish(s, SOPHIA_9P_CLOSED);
  s->drive_pending = 0;
  r = os_drive(s);
  return r ? os_finish(s, r) : 0;
}
