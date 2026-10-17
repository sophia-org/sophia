#include "../sophia_broker_session.h"
#include <string.h>
enum { WALK_EVENTS = 1, OPEN_EVENTS, WALK_ACK, OPEN_ACK, READY };
static int br_fail(struct sophia_br *s, int e) {
  if (!s->error)
    s->error = e;
  sophia_bt_close(&s->transaction);
  sophia_bj_close(&s->journal);
  return s->error;
}
static int wire_error(struct sophia_br *s, int e) {
  s->wire_error = e;
  return SOPHIA_BR_WIRE;
}
void sophia_br_close(struct sophia_br *s) {
  if (s)
    br_fail(s, SOPHIA_BS_CLOSED);
}
int sophia_br_ready(const struct sophia_br *s) {
  return s && !s->error && !s->refused && s->established;
}
int sophia_br_transaction_ready(const struct sophia_br *s) {
  return sophia_br_ready(s) && sophia_bt_done(&s->transaction);
}
int sophia_br_retry_ready(const struct sophia_br *s) {
  return s && !s->error && sophia_bt_retry_ready(&s->transaction);
}
int sophia_br_deadline(const struct sophia_br *s, uint64_t *out) {
  uint64_t due = UINT64_MAX, transaction_due;
  unsigned has = 0;
  if (!s || !out || s->error)
    return 0;
  if (!sophia_br_ready(s)) {
    due = s->deadline;
    has = 1;
  }
  if (s->journal.has_due) {
    if (s->journal.due < due)
      due = s->journal.due;
    has = 1;
  }
  if (sophia_bt_deadline(&s->transaction, &transaction_due)) {
    if (transaction_due < due)
      due = transaction_due;
    has = 1;
  }
  if (s->refused) {
    if (s->refusal_due < due)
      due = s->refusal_due;
    has = 1;
  }
  if (s->has_request) {
    if (s->request_due < due)
      due = s->request_due;
    has = 1;
  }
  if (has)
    *out = due;
  return (int)has;
}
int sophia_br_wants_write(const struct sophia_br *s) {
  return s && !s->error && sophia_9p_wants_write(s->wire);
}
int sophia_br_event(const struct sophia_br *s, struct sophia_bf_event *out) {
  if (!s || !out)
    return SOPHIA_BS_INVALID;
  if (s->error)
    return s->error;
  return sophia_bj_event(&s->journal, out);
}
static int br_advance(struct sophia_br *s, uint64_t now) {
  uint64_t due;
  int e;
  if (!s)
    return SOPHIA_BS_INVALID;
  if (now < s->now)
    return SOPHIA_BS_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if (sophia_br_deadline(s, &due) && now >= due)
    return br_fail(s, SOPHIA_BS_DEADLINE);
  e = sophia_bj_advance(&s->journal, now);
  return e ? br_fail(s, e) : 0;
}
static int observe(struct sophia_br *s) {
  struct sophia_bf_event v;
  int e = sophia_bj_event(&s->journal, &v);
  if (e == SOPHIA_BS_BUSY)
    return 0;
  if (e)
    return e;
  if (v.sequence == s->observed)
    return 0;
  if (v.kind == SOPHIA_BF_SUBMITTED) {
    e = sophia_bt_receipt(&s->transaction, &v, s->now);
    if (e)
      return e;
  }
  e = sophia_bp_event(&s->ordering, &v, s->now);
  if (e)
    return e;
  switch (v.kind) {
  case SOPHIA_BF_SUBMITTED:
    if (v.candidate_kind == 257)
      s->has_request = 0;
    break;
  case SOPHIA_BF_NEGOTIATED:
    s->negotiated = v.sequence;
    break;
  case SOPHIA_BF_REFUSED:
    s->refused = v.sequence;
    s->refusal_due = s->ordering.refusal_due;
    break;
  case SOPHIA_BF_REQUEST:
    if (s->now > UINT64_MAX - s->limits.response_timeout_ms)
      return SOPHIA_BS_OVERFLOW;
    s->request = v.request;
    if (v.request.label_length)
      memcpy(s->label, v.request.label, v.request.label_length);
    s->request.label = NULL;
    s->has_request = 1;
    s->request_due = s->now + s->limits.response_timeout_ms;
    break;
  default:
    return SOPHIA_BS_PROTOCOL;
  }
  s->observed = v.sequence;
  return 0;
}
static int br_drive(struct sophia_br *s) {
  int e;
  const char *name;
  if (!s->established && !s->refused && s->negotiated &&
      s->journal.acknowledged >= s->negotiated &&
      sophia_bt_done(&s->transaction))
    s->established = 1;
  if (s->boot != READY) {
    if (s->boot_active)
      return 0;
    switch (s->boot) {
    case WALK_EVENTS:
      name = "events";
      e = sophia_9p_walk(s->wire, s->root, &name, 1, &s->boot_tag, &s->events);
      break;
    case OPEN_EVENTS:
      e = sophia_9p_lopen(s->wire, s->events, 0, &s->boot_tag);
      break;
    case WALK_ACK:
      name = "ack";
      e = sophia_9p_walk(s->wire, s->root, &name, 1, &s->boot_tag, &s->ack);
      break;
    case OPEN_ACK:
      e = sophia_9p_lopen(s->wire, s->ack, 1, &s->boot_tag);
      break;
    default:
      return SOPHIA_BS_PROTOCOL;
    }
    if (e)
      return wire_error(s, e);
    s->boot_active = 1;
    return 0;
  }
  if (!s->read_active && !s->refused && sophia_bj_read_count(&s->journal)) {
    uint32_t count = sophia_bj_read_count(&s->journal);
    if (count > s->wire->msize - 11)
      count = s->wire->msize - 11;
    if (s->events_iounit && count > s->events_iounit)
      count = s->events_iounit;
    e = sophia_9p_read(s->wire, s->events, s->journal.offset, count,
                       &s->read_tag);
    if (e)
      return wire_error(s, e);
    s->read_active = 1;
  }
  if (!s->releasing &&
      s->transaction.submission.custody == SOPHIA_BS_SUBMITTED) {
    e = sophia_bt_release(&s->transaction, s->journal.acknowledged, s->now);
    if (!e)
      s->releasing = 1;
    else if (e != SOPHIA_BS_BUSY)
      return e;
  }
  if (!s->transaction_active) {
    struct sophia_bt_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    e = sophia_bt_operation(&s->transaction, s->now, s->wire->msize, &op);
    if (e)
      return e;
    switch (op.kind) {
    case SOPHIA_BT_NONE:
      return 0;
    case SOPHIA_BT_WALK:
      name = op.name;
      e = sophia_9p_walk(s->wire, s->root, &name, 1, &s->transaction_tag, &fid);
      break;
    case SOPHIA_BT_OPEN:
      e = sophia_9p_lopen(s->wire, op.fid, op.flags, &s->transaction_tag);
      break;
    case SOPHIA_BT_WRITE:
      e = sophia_9p_write(s->wire, op.fid, op.offset, op.bytes, op.length,
                          &s->transaction_tag);
      break;
    case SOPHIA_BT_CLUNK:
      e = sophia_9p_clunk(s->wire, op.fid, &s->transaction_tag);
      break;
    default:
      return SOPHIA_BS_PROTOCOL;
    }
    if (e)
      return wire_error(s, e);
    e = sophia_bt_issued(&s->transaction, s->now, s->wire->msize, fid);
    if (e)
      return e;
    s->transaction_active = 1;
  }
  return 0;
}
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
int sophia_br_request(const struct sophia_br *s,
                      struct sophia_bf_request *out) {
  if (!s || !out)
    return SOPHIA_BS_INVALID;
  if (s->error)
    return s->error;
  if (!s->has_request || s->ordering.phase != SOPHIA_BP_READY)
    return SOPHIA_BS_BUSY;
  *out = s->request;
  out->label = out->label_present ? s->label : NULL;
  return 0;
}
int sophia_br_respond(struct sophia_br *s, uint64_t request,
                      const struct sophia_bf_row *rows, size_t count,
                      uint64_t now) {
  struct sophia_bt transaction;
  struct sophia_bf_request pending;
  int e = br_advance(s, now);
  if (e)
    return e;
  if (!sophia_br_transaction_ready(s) || s->ordering.phase != SOPHIA_BP_READY)
    return SOPHIA_BS_BUSY;
  e = sophia_br_request(s, &pending);
  if (e)
    return e;
  if (pending.id != request)
    return SOPHIA_BS_INVALID;
  if (s->last_submission == UINT64_MAX)
    return SOPHIA_BS_OVERFLOW;
  e = sophia_bt_response(&transaction, s->journal.epoch, s->last_submission + 1,
                         &pending, rows, count, now, s->request_due,
                         s->limits.assembly_timeout_ms);
  if (e)
    return e;
  e = sophia_bp_response(&s->ordering, request, s->last_submission + 1, now);
  if (e)
    return e;
  ++s->last_submission;
  transaction.retain_submit = 1;
  transaction.submit = s->transaction.submit;
  s->transaction = transaction;
  s->releasing = 0;
  e = br_drive(s);
  return e ? br_fail(s, e) : 0;
}
int sophia_br_init(struct sophia_br *s, struct sophia_bo *opening,
                   uint16_t minimum, uint16_t maximum, uint64_t deadline) {
  struct sophia_9p_client *wire;
  size_t bytes, i;
  int e;
  if (!s || !sophia_bo_ready(opening))
    return SOPHIA_BS_INVALID;
  wire = opening->wire;
  if (!wire || wire->terminal || wire->phase != 2 || wire->fid_count != 1 ||
      wire->fids[0] != opening->root || wire->rx_used || wire->capacity < 3 ||
      wire->capacity > SOPHIA_9P_REQUESTS || wire->fid_limit < 5 ||
      wire->msize < 512 || wire->msize > 65536)
    return SOPHIA_BS_INVALID;
  bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!bytes || !disjoint(s, sizeof *s, opening, sizeof *opening) ||
      !disjoint(s, sizeof *s, wire, sizeof *wire) ||
      !disjoint(s, sizeof *s, wire->storage, bytes))
    return SOPHIA_BS_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i)
    if (wire->slots[i].state)
      return SOPHIA_BS_INVALID;
  if (deadline <= opening->now)
    return SOPHIA_BS_DEADLINE;
  memset(s, 0, sizeof *s);
  s->wire = wire;
  s->root = opening->root;
  s->limits = opening->limits;
  s->now = opening->now;
  s->deadline = deadline;
  s->boot = WALK_EVENTS;
  s->last_submission = 1;
  s->events = s->ack = SOPHIA_9P_NOFID;
  e = sophia_bj_init(&s->journal, opening->epoch, s->now,
                     s->limits.ack_progress_timeout_ms);
  if (e)
    return br_fail(s, e);
  e = sophia_bp_init(&s->ordering, opening->epoch, &s->limits, 1, s->now);
  if (e)
    return br_fail(s, e);
  e = sophia_bt_negotiate(&s->transaction, opening->epoch, 1, minimum, maximum,
                          s->now, deadline, s->limits.assembly_timeout_ms);
  if (e)
    return br_fail(s, e);
  s->transaction.retain_submit = 1;
  e = br_drive(s);
  if (e)
    return br_fail(s, e);
  sophia_bo_close(opening);
  return 0;
}
int sophia_br_consume(struct sophia_br *s, uint64_t now) {
  int e = br_advance(s, now);
  if (e)
    return e;
  e = sophia_bj_consume(&s->journal, now);
  if (e)
    return e == SOPHIA_BS_BUSY ? e : br_fail(s, e);
  e = observe(s);
  if (!e)
    e = br_drive(s);
  return e ? br_fail(s, e) : 0;
}
int sophia_br_acknowledge(struct sophia_br *s, uint64_t now) {
  int e = br_advance(s, now);
  if (e)
    return e;
  e = sophia_bj_begin_ack(&s->journal, now);
  if (e)
    return e;
  e = sophia_9p_write(s->wire, s->ack, 0, s->journal.ack, 16, &s->ack_tag);
  if (e)
    return br_fail(s, wire_error(s, e));
  s->ack_active = 1;
  return 0;
}
int sophia_br_retry(struct sophia_br *s, uint64_t now) {
  int e = br_advance(s, now);
  if (e)
    return e;
  e = sophia_bt_retry(&s->transaction, now);
  if (e)
    return e;
  e = br_drive(s);
  return e ? br_fail(s, e) : 0;
}
static int same(struct sophia_9p_handle a, struct sophia_9p_handle b) {
  return a.slot == b.slot && a.serial == b.serial;
}
static int accept(struct sophia_br *s, const struct sophia_9p_reply *r) {
  int e;
  if (s->boot_active && same(s->boot_tag, r->handle)) {
    s->boot_active = 0;
    if (r->type == 7) {
      s->remote_error = r->error;
      return SOPHIA_BS_REMOTE;
    }
    switch (s->boot) {
    case WALK_EVENTS:
    case WALK_ACK:
      if (r->type != 111 || r->count != 1 || r->qid.type)
        return SOPHIA_BS_PROTOCOL;
      s->boot = s->boot == WALK_EVENTS ? OPEN_EVENTS : OPEN_ACK;
      break;
    case OPEN_EVENTS:
    case OPEN_ACK:
      if (r->type != 13 || r->qid.type)
        return SOPHIA_BS_PROTOCOL;
      if (s->boot == OPEN_EVENTS) {
        s->events_iounit = r->iounit;
        s->boot = WALK_ACK;
      } else {
        if (r->iounit && r->iounit < 16)
          return SOPHIA_BS_PROTOCOL;
        s->boot = READY;
      }
      break;
    default:
      return SOPHIA_BS_PROTOCOL;
    }
  } else if (s->read_active && same(s->read_tag, r->handle)) {
    s->read_active = 0;
    if (r->type == 7) {
      s->remote_error = r->error;
      return SOPHIA_BS_REMOTE;
    }
    if (r->type != 117)
      return SOPHIA_BS_PROTOCOL;
    return sophia_bj_receive(&s->journal, r->data, r->count, s->now);
  } else if (s->ack_active && same(s->ack_tag, r->handle)) {
    s->ack_active = 0;
    if (r->type != 119 && r->type != 7)
      return SOPHIA_BS_PROTOCOL;
    e = sophia_bj_ack_reply(&s->journal, r->count, r->type == 7 ? r->error : 0,
                            s->now);
    if (e) {
      s->remote_error = s->journal.remote_error;
      return e;
    }
    if (s->refused && s->journal.acknowledged >= s->refused)
      return SOPHIA_BR_REFUSED;
  } else if (s->transaction_active && same(s->transaction_tag, r->handle)) {
    s->transaction_active = 0;
    e = sophia_bt_reply(&s->transaction, r, s->now);
    if (e) {
      s->remote_error = s->transaction.remote_error;
      return e;
    }
  } else
    return SOPHIA_BS_PROTOCOL;
  return 0;
}
int sophia_br_dispatch(struct sophia_br *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  int e = br_advance(s, now);
  if (e)
    return e;
  e = br_drive(s);
  if (e)
    return br_fail(s, e);
  e = sophia_9p_service(s->wire, budget);
  if (e < 0)
    return br_fail(s, wire_error(s, e));
  e = sophia_9p_peek(s->wire, &reply);
  if (e == SOPHIA_9P_AGAIN)
    return 0;
  if (e)
    return br_fail(s, wire_error(s, e));
  e = accept(s, &reply);
  if (!e)
    e = observe(s);
  if (e)
    return br_fail(s, e);
  e = sophia_9p_consume(s->wire, reply.handle);
  if (e)
    return br_fail(s, wire_error(s, e));
  e = br_drive(s);
  return e ? br_fail(s, e) : 0;
}
