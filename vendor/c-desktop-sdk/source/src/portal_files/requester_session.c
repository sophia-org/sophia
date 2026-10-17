#include "requester_session_internal.h"
#include <string.h>
enum { WALK_EVENTS = 1, OPEN_EVENTS, WALK_ACK, OPEN_ACK, READY };
int pqr_fail(struct sophia_pqr *s, int e) {
  size_t i;
  if (!s->error)
    s->error = e;
  sophia_pqt_close(&s->transaction);
  sophia_pqj_close(&s->journal);
  if (s->upload_present)
    sophia_pqu_close(&s->upload);
  for (i = 0; i < 65; ++i)
    if (s->transfers[i].occupied)
      sophia_pqf_close(&s->transfers[i].transfer);
  return s->error;
}
static int wire_error(struct sophia_pqr *s, int e) {
  s->wire_error = e;
  return SOPHIA_PQR_WIRE;
}
void sophia_pqr_close(struct sophia_pqr *s) {
  if (s)
    pqr_fail(s, SOPHIA_PQS_CLOSED);
}
int sophia_pqr_ready(const struct sophia_pqr *s) {
  return s && !s->error && !s->refused && s->established;
}
int sophia_pqr_transaction_ready(const struct sophia_pqr *s) {
  return sophia_pqr_ready(s) && sophia_pqt_done(&s->transaction) &&
         !s->pending_request;
}
int sophia_pqr_deadline(const struct sophia_pqr *s, uint64_t *out) {
  uint64_t due = UINT64_MAX, transaction_due;
  unsigned has = 0;
  if (!s || !out || s->error)
    return 0;
  if (!sophia_pqr_ready(s)) {
    due = s->deadline;
    has = 1;
  }
  if (s->journal.has_due) {
    if (s->journal.due < due)
      due = s->journal.due;
    has = 1;
  }
  if (sophia_pqt_deadline(&s->transaction, &transaction_due)) {
    if (transaction_due < due)
      due = transaction_due;
    has = 1;
  }
  if (s->refused) {
    if (s->refusal_due < due)
      due = s->refusal_due;
    has = 1;
  }
  if (s->upload_present && !sophia_pqu_done(&s->upload) && !s->upload.error) {
    if (s->upload.deadline < due)
      due = s->upload.deadline;
    has = 1;
  }
  if (has)
    *out = due;
  return (int)has;
}
int sophia_pqr_wants_write(const struct sophia_pqr *s) {
  return s && !s->error && sophia_9p_wants_write(s->wire);
}
const struct sophia_pq_event *sophia_pqr_event(const struct sophia_pqr *s) {
  return s && !s->error ? sophia_pqj_event(&s->journal) : NULL;
}
int pqr_advance(struct sophia_pqr *s, uint64_t now) {
  uint64_t due;
  int e;
  if (!s)
    return SOPHIA_PQS_INVALID;
  if (now < s->now)
    return SOPHIA_PQS_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if (sophia_pqr_deadline(s, &due) && now >= due)
    return pqr_fail(s, SOPHIA_PQS_DEADLINE);
  e = sophia_pqj_advance(&s->journal, now);
  return e ? pqr_fail(s, e) : 0;
}
static int observe(struct sophia_pqr *s) {
  const struct sophia_pq_event *v = sophia_pqj_event(&s->journal);
  int e;
  if (!v || v->sequence == s->observed)
    return 0;
  switch (v->kind) {
  case SOPHIA_PQ_SUBMITTED:
    e = sophia_pqt_receipt(&s->transaction, v, s->now);
    if (e)
      return e;
    if (s->command == SOPHIA_PQ_BEGIN) {
      if (!s->upload_present)
        return SOPHIA_PQS_PROTOCOL;
      if (s->upload_phase == PQR_UPLOAD_BEGIN)
        s->upload_phase = PQR_UPLOAD_WRITING;
    }
    break;
  case SOPHIA_PQ_NEGOTIATED:
  case SOPHIA_PQ_REFUSED:
    if (s->transaction.submission.custody != SOPHIA_PQS_SUBMITTED ||
        s->negotiated || s->refused)
      return SOPHIA_PQS_PROTOCOL;
    if (v->kind == SOPHIA_PQ_NEGOTIATED)
      s->negotiated = v->sequence;
    else {
      if (s->now > UINT64_MAX - s->limits.ack_progress_timeout_ms)
        return SOPHIA_PQS_OVERFLOW;
      s->refused = v->sequence;
      s->refusal_due = s->now + s->limits.ack_progress_timeout_ms;
    }
    break;
  case SOPHIA_PQ_DECISION: {
    struct sophia_pqr_tracked *tracked;
    size_t i;
    if (!s->established ||
        s->transaction.submission.custody != SOPHIA_PQS_SUBMITTED ||
        !s->pending_request)
      return SOPHIA_PQS_PROTOCOL;
    if (v->data.decision.allowed) {
      for (i = 0; i < 65; ++i)
        if (s->transfers[i].occupied &&
            s->transfers[i].transfer.expected.transfer ==
                v->data.decision.grant.transfer &&
            sophia_pqf_grant(&s->transfers[i].transfer))
          return SOPHIA_PQS_PROTOCOL;
    }
    tracked = &s->transfers[s->pending_request - 1];
    e = sophia_pqf_decision(&tracked->transfer, v);
    if (e)
      return e;
    if (tracked->transfer.state == SOPHIA_PQF_DENIED)
      tracked->terminal_sequence = v->sequence;
    s->pending_request = 0;
    break;
  }
  case SOPHIA_PQ_OUTCOME: {
    size_t i;
    for (i = 0; i < 65; ++i)
      if (s->transfers[i].occupied &&
          s->transfers[i].transfer.expected.transfer ==
              v->data.outcome.transfer &&
          sophia_pqf_grant(&s->transfers[i].transfer))
        break;
    if (i == 65)
      return SOPHIA_PQS_PROTOCOL;
    e = sophia_pqf_outcome(&s->transfers[i].transfer, v->data.outcome.transfer,
                           v->data.outcome.state);
    if (e)
      return e;
    s->transfers[i].terminal_sequence = v->sequence;
    if (s->upload_present && s->upload_id.epoch == s->transfers[i].id.epoch &&
        s->upload_id.submission == s->transfers[i].id.submission) {
      e = sophia_pqu_settle(&s->upload, s->now);
      if (e)
        return e;
      s->upload_phase = PQR_UPLOAD_SETTLING;
    }
    break;
  }
  default:
    return SOPHIA_PQS_PROTOCOL;
  }
  s->observed = v->sequence;
  return 0;
}
int pqr_drive(struct sophia_pqr *s) {
  int e;
  const char *name;
  if (!s->established && !s->refused && s->negotiated &&
      s->journal.acknowledged >= s->negotiated &&
      sophia_pqt_done(&s->transaction))
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
      return SOPHIA_PQS_PROTOCOL;
    }
    if (e)
      return wire_error(s, e);
    s->boot_active = 1;
    return 0;
  }
  if (!s->read_active && !s->refused && sophia_pqj_read_count(&s->journal)) {
    uint32_t count = sophia_pqj_read_count(&s->journal);
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
      s->transaction.submission.custody == SOPHIA_PQS_SUBMITTED) {
    e = sophia_pqt_release(&s->transaction, s->journal.acknowledged, s->now);
    if (!e)
      s->releasing = 1;
    else if (e != SOPHIA_PQS_BUSY)
      return e;
  }
  if (!s->transaction_active) {
    struct sophia_pqt_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    int end_submit;
    e = sophia_pqt_operation(&s->transaction, s->now, s->wire->msize, &op);
    if (e)
      return e;
    end_submit = s->command == SOPHIA_PQ_END && op.kind == SOPHIA_PQT_WRITE &&
                 op.fid == s->transaction.submit;
    switch (op.kind) {
    case SOPHIA_PQT_NONE:
      return pqr_drive_upload(s);
    case SOPHIA_PQT_WALK:
      name = op.name;
      e = sophia_9p_walk(s->wire, s->root, &name, 1, &s->transaction_tag, &fid);
      break;
    case SOPHIA_PQT_OPEN:
      e = sophia_9p_lopen(s->wire, op.fid, op.flags, &s->transaction_tag);
      break;
    case SOPHIA_PQT_WRITE:
      e = sophia_9p_write(s->wire, op.fid, op.offset, op.bytes, op.length,
                          &s->transaction_tag);
      break;
    case SOPHIA_PQT_CLUNK:
      e = sophia_9p_clunk(s->wire, op.fid, &s->transaction_tag);
      break;
    default:
      return SOPHIA_PQS_PROTOCOL;
    }
    if (e)
      return wire_error(s, e);
    e = sophia_pqt_issued(&s->transaction, s->now, s->wire->msize, fid);
    if (e)
      return e;
    if (end_submit) {
      size_t slot = pqr_transfer_slot(s, s->upload_id);
      if (slot == 65)
        return SOPHIA_PQS_PROTOCOL;
      if (s->transfers[slot].transfer.state == SOPHIA_PQF_ALLOWED) {
        e = sophia_pqf_end_issued(&s->transfers[slot].transfer);
        if (e)
          return e;
      }
    }
    s->transaction_active = 1;
  }
  return pqr_drive_upload(s);
}
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
int sophia_pqr_request(struct sophia_pqr *s,
                       const struct sophia_pq_request *request, uint64_t now,
                       uint64_t deadline, struct sophia_pqr_request_id *out) {
  struct sophia_pqt transaction;
  struct sophia_pqf transfer;
  struct sophia_pq_candidate candidate = {0};
  struct sophia_pqr_request_id id;
  size_t slot;
  int e;
  if (!s || !request || !out || !disjoint(s, sizeof *s, out, sizeof *out))
    return SOPHIA_PQS_INVALID;
  e = pqr_advance(s, now);
  if (e)
    return e;
  if (!sophia_pqr_ready(s) || !sophia_pqt_done(&s->transaction) ||
      s->pending_request)
    return SOPHIA_PQS_BUSY;
  for (slot = 0; slot < 65; ++slot)
    if (!s->transfers[slot].occupied)
      break;
  if (slot == 65)
    return SOPHIA_PQS_BUSY;
  e = sophia_pqf_init(&transfer, request, s->limits.broker_generation);
  if (e)
    return e;
  if (s->last_submission == UINT64_MAX)
    return SOPHIA_PQS_OVERFLOW;
  id.epoch = s->journal.epoch;
  id.issuer = s;
  id.submission = s->last_submission + 1;
  candidate.kind = SOPHIA_PQ_REQUEST;
  candidate.data.request = *request;
  e = sophia_pqt_init(&transaction, id.epoch, id.submission, &candidate, now,
                      deadline, s->limits.assembly_timeout_ms);
  if (e)
    return e;
  s->transfers[slot].id = id;
  s->transfers[slot].transfer = transfer;
  s->transfers[slot].terminal_sequence = 0;
  s->transfers[slot].occupied = 1;
  s->transfers[slot].cancel_pending = 0;
  s->transfers[slot].begun = 0;
  s->pending_request = (unsigned)slot + 1;
  s->last_submission = id.submission;
  s->transaction = transaction;
  s->command = SOPHIA_PQ_REQUEST;
  s->releasing = 0;
  e = pqr_drive(s);
  if (e)
    return pqr_fail(s, e);
  *out = id;
  return 0;
}
size_t pqr_transfer_slot(const struct sophia_pqr *s,
                         struct sophia_pqr_request_id id) {
  size_t i;
  if (!s || id.issuer != s)
    return 65;
  for (i = 0; i < 65; ++i)
    if (s->transfers[i].occupied && s->transfers[i].id.epoch == id.epoch &&
        s->transfers[i].id.submission == id.submission)
      break;
  return i;
}
const struct sophia_pqf *sophia_pqr_transfer(const struct sophia_pqr *s,
                                             struct sophia_pqr_request_id id) {
  size_t slot = pqr_transfer_slot(s, id);
  return slot == 65 ? NULL : &s->transfers[slot].transfer;
}
int sophia_pqr_release_transfer(struct sophia_pqr *s,
                                struct sophia_pqr_request_id id, uint64_t now) {
  size_t slot;
  int e = pqr_advance(s, now);
  if (e)
    return e;
  if (s->upload_present && id.issuer == s && id.epoch == s->upload_id.epoch &&
      id.submission == s->upload_id.submission)
    return SOPHIA_PQS_BUSY;
  slot = pqr_transfer_slot(s, id);
  if (slot == 65)
    return SOPHIA_PQS_INVALID;
  if (!s->transfers[slot].terminal_sequence ||
      s->journal.acknowledged < s->transfers[slot].terminal_sequence)
    return SOPHIA_PQS_BUSY;
  s->transfers[slot].occupied = 0;
  return 0;
}
int sophia_pqr_cancel(struct sophia_pqr *s, struct sophia_pqr_request_id id,
                      uint64_t now, uint64_t deadline) {
  struct sophia_pqt transaction;
  struct sophia_pq_candidate candidate = {0};
  size_t slot;
  int e = pqr_advance(s, now);
  if (e)
    return e;
  if (!sophia_pqr_ready(s) || !sophia_pqt_done(&s->transaction) ||
      s->pending_request)
    return SOPHIA_PQS_BUSY;
  slot = pqr_transfer_slot(s, id);
  if (slot == 65)
    return SOPHIA_PQS_INVALID;
  if (s->transfers[slot].cancel_pending ||
      s->transfers[slot].transfer.state != SOPHIA_PQF_ALLOWED)
    return SOPHIA_PQS_BUSY;
  if (s->last_submission == UINT64_MAX)
    return SOPHIA_PQS_OVERFLOW;
  candidate.kind = SOPHIA_PQ_CANCEL;
  candidate.data.cancel = s->transfers[slot].transfer.expected.transfer;
  e = sophia_pqt_init(&transaction, s->journal.epoch, s->last_submission + 1,
                      &candidate, now, deadline, s->limits.assembly_timeout_ms);
  if (e)
    return e;
  s->transfers[slot].cancel_pending = 1;
  ++s->last_submission;
  s->transaction = transaction;
  s->command = SOPHIA_PQ_CANCEL;
  s->releasing = 0;
  e = pqr_drive(s);
  return e ? pqr_fail(s, e) : 0;
}
int sophia_pqr_init(struct sophia_pqr *s, struct sophia_pqo *opening,
                    uint16_t minimum, uint16_t maximum, uint64_t deadline) {
  struct sophia_9p_client *wire;
  struct sophia_pq_candidate candidate = {0};
  size_t bytes, i;
  int e;
  if (!s || !sophia_pqo_ready(opening))
    return SOPHIA_PQS_INVALID;
  wire = opening->wire;
  if (!wire || wire->terminal || wire->phase != 2 || wire->fid_count != 1 ||
      wire->fids[0] != opening->root || wire->rx_used || wire->capacity < 3 ||
      wire->capacity > SOPHIA_9P_REQUESTS || wire->fid_limit < 5 ||
      wire->msize < 512 || wire->msize > 65536)
    return SOPHIA_PQS_INVALID;
  bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!bytes || !disjoint(s, sizeof *s, opening, sizeof *opening) ||
      !disjoint(s, sizeof *s, wire, sizeof *wire) ||
      !disjoint(s, sizeof *s, wire->storage, bytes))
    return SOPHIA_PQS_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i)
    if (wire->slots[i].state)
      return SOPHIA_PQS_INVALID;
  if (deadline <= opening->now)
    return SOPHIA_PQS_DEADLINE;
  memset(s, 0, sizeof *s);
  s->wire = wire;
  s->root = opening->root;
  s->limits = opening->limits;
  s->now = opening->now;
  s->deadline = deadline;
  s->boot = WALK_EVENTS;
  s->last_submission = 1;
  s->command = SOPHIA_PQ_NEGOTIATE;
  s->events = s->ack = SOPHIA_9P_NOFID;
  e = sophia_pqj_init(&s->journal, opening->epoch, s->now,
                      s->limits.ack_progress_timeout_ms);
  if (e)
    return pqr_fail(s, e);
  candidate.kind = SOPHIA_PQ_NEGOTIATE;
  candidate.data.negotiate.minimum = minimum;
  candidate.data.negotiate.maximum = maximum;
  e = sophia_pqt_init(&s->transaction, opening->epoch, 1, &candidate, s->now,
                      deadline, s->limits.assembly_timeout_ms);
  if (e)
    return pqr_fail(s, e);
  e = pqr_drive(s);
  if (e)
    return pqr_fail(s, e);
  sophia_pqo_close(opening);
  return 0;
}
int sophia_pqr_consume(struct sophia_pqr *s, uint64_t now) {
  int e = pqr_advance(s, now);
  if (e)
    return e;
  e = sophia_pqj_consume(&s->journal, now);
  if (e)
    return e == SOPHIA_PQS_BUSY ? e : pqr_fail(s, e);
  e = observe(s);
  if (!e)
    e = pqr_drive(s);
  return e ? pqr_fail(s, e) : 0;
}
int sophia_pqr_acknowledge(struct sophia_pqr *s, uint64_t now) {
  int e = pqr_advance(s, now);
  if (e)
    return e;
  e = sophia_pqj_begin_ack(&s->journal, now);
  if (e)
    return e;
  e = sophia_9p_write(s->wire, s->ack, 0, s->journal.ack, 16, &s->ack_tag);
  if (e)
    return pqr_fail(s, wire_error(s, e));
  s->ack_active = 1;
  return 0;
}
int sophia_pqr_retry(struct sophia_pqr *s, uint64_t now) {
  int e = pqr_advance(s, now);
  if (e)
    return e;
  e = sophia_pqt_retry(&s->transaction, now);
  if (e)
    return e;
  e = pqr_drive(s);
  return e ? pqr_fail(s, e) : 0;
}
static int same(struct sophia_9p_handle a, struct sophia_9p_handle b) {
  return a.slot == b.slot && a.serial == b.serial;
}
static int accept(struct sophia_pqr *s, const struct sophia_9p_reply *r) {
  int e;
  if (s->boot_active && same(s->boot_tag, r->handle)) {
    s->boot_active = 0;
    if (r->type == 7) {
      s->remote_error = r->error;
      return SOPHIA_PQS_REMOTE;
    }
    switch (s->boot) {
    case WALK_EVENTS:
    case WALK_ACK:
      if (r->type != 111 || r->count != 1 || r->qid.type)
        return SOPHIA_PQS_PROTOCOL;
      s->boot = s->boot == WALK_EVENTS ? OPEN_EVENTS : OPEN_ACK;
      break;
    case OPEN_EVENTS:
    case OPEN_ACK:
      if (r->type != 13 || r->qid.type)
        return SOPHIA_PQS_PROTOCOL;
      if (s->boot == OPEN_EVENTS) {
        s->events_iounit = r->iounit;
        s->boot = WALK_ACK;
      } else {
        if (r->iounit && r->iounit < 16)
          return SOPHIA_PQS_PROTOCOL;
        s->boot = READY;
      }
      break;
    default:
      return SOPHIA_PQS_PROTOCOL;
    }
  } else if (s->read_active && same(s->read_tag, r->handle)) {
    s->read_active = 0;
    if (r->type == 7) {
      s->remote_error = r->error;
      return SOPHIA_PQS_REMOTE;
    }
    if (r->type != 117)
      return SOPHIA_PQS_PROTOCOL;
    return sophia_pqj_receive(&s->journal, r->data, r->count, s->now);
  } else if (s->ack_active && same(s->ack_tag, r->handle)) {
    s->ack_active = 0;
    if (r->type != 119 && r->type != 7)
      return SOPHIA_PQS_PROTOCOL;
    e = sophia_pqj_ack_reply(&s->journal, r->count, r->type == 7 ? r->error : 0,
                             s->now);
    if (e) {
      s->remote_error = s->journal.remote_error;
      return e;
    }
    if (s->refused && s->journal.acknowledged >= s->refused)
      return SOPHIA_PQR_REFUSED;
  } else if (s->upload_active && same(s->upload_tag, r->handle)) {
    s->upload_active = 0;
    e = sophia_pqu_reply(&s->upload, r, s->now);
    if (e) {
      s->remote_error = s->upload.remote_error;
      return e;
    }
  } else if (s->transaction_active && same(s->transaction_tag, r->handle)) {
    s->transaction_active = 0;
    e = sophia_pqt_reply(&s->transaction, r, s->now);
    if (e) {
      s->remote_error = s->transaction.remote_error;
      return e;
    }
  } else
    return SOPHIA_PQS_PROTOCOL;
  return 0;
}
int sophia_pqr_dispatch(struct sophia_pqr *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  int e = pqr_advance(s, now);
  if (e)
    return e;
  e = pqr_drive(s);
  if (e)
    return pqr_fail(s, e);
  e = sophia_9p_service(s->wire, budget);
  if (e < 0)
    return pqr_fail(s, wire_error(s, e));
  e = sophia_9p_peek(s->wire, &reply);
  if (e == SOPHIA_9P_AGAIN)
    return 0;
  if (e)
    return pqr_fail(s, wire_error(s, e));
  e = accept(s, &reply);
  if (!e)
    e = observe(s);
  if (e)
    return pqr_fail(s, e);
  e = sophia_9p_consume(s->wire, reply.handle);
  if (e)
    return pqr_fail(s, wire_error(s, e));
  e = pqr_drive(s);
  return e ? pqr_fail(s, e) : 0;
}
