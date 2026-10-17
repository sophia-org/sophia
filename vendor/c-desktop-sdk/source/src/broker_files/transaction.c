#include "../sophia_broker_transaction.h"
#include <string.h>
enum {
  WALK_TRANSACTION = 1,
  OPEN_TRANSACTION,
  STAGING,
  WALK_SUBMIT,
  OPEN_SUBMIT,
  SUBMIT,
  WAIT,
  CLUNK_SUBMIT,
  CLUNK_TRANSACTION,
  DONE
};
static int fail(struct sophia_bt *t, int e) {
  if (!t->error)
    t->error = (enum sophia_bs_error)e;
  sophia_bs_close(&t->submission);
  return t->error;
}
void sophia_bt_close(struct sophia_bt *t) {
  if (t)
    fail(t, SOPHIA_BS_CLOSED);
}
int sophia_bt_done(const struct sophia_bt *t) {
  return t && !t->error && t->stage == DONE;
}
int sophia_bt_retry_ready(const struct sophia_bt *t) {
  return t && !t->error && t->stage == WAIT && !t->outstanding &&
         t->submission.custody == SOPHIA_BS_QUEUED;
}
int sophia_bt_deadline(const struct sophia_bt *t, uint64_t *out) {
  if (!t || !out || t->error || t->stage == DONE)
    return 0;
  *out = t->has_assembly_due && t->assembly_due < t->deadline ? t->assembly_due
                                                              : t->deadline;
  return 1;
}
static void initialize(struct sophia_bt *t, uint64_t now, uint64_t deadline,
                       uint32_t timeout) {
  t->stage = WALK_TRANSACTION;
  t->transaction = t->submit = SOPHIA_9P_NOFID;
  t->now = now;
  t->deadline = deadline;
  t->assembly_timeout = timeout;
}
int sophia_bt_negotiate(struct sophia_bt *t, uint64_t epoch, uint64_t id,
                        uint16_t minimum, uint16_t maximum, uint64_t now,
                        uint64_t deadline, uint32_t timeout) {
  struct sophia_bt v = {0};
  int e;
  if (!t || !timeout || timeout > 12000)
    return SOPHIA_BS_INVALID;
  e = sophia_bs_negotiate(&v.submission, epoch, id, minimum, maximum, now,
                          deadline);
  if (e)
    return e;
  initialize(&v, now, deadline, timeout);
  *t = v;
  return 0;
}
int sophia_bt_response(struct sophia_bt *t, uint64_t epoch, uint64_t id,
                       const struct sophia_bf_request *request,
                       const struct sophia_bf_row *rows, size_t count,
                       uint64_t now, uint64_t deadline, uint32_t timeout) {
  struct sophia_bt v = {0};
  int e;
  if (!t || !timeout || timeout > 12000)
    return SOPHIA_BS_INVALID;
  e = sophia_bs_response(&v.submission, epoch, id, request, rows, count, now,
                         deadline);
  if (e)
    return e;
  initialize(&v, now, deadline, timeout);
  *t = v;
  return 0;
}
static int advance(struct sophia_bt *t, uint64_t now) {
  int e;
  if (!t)
    return SOPHIA_BS_INVALID;
  if (now < t->now)
    return SOPHIA_BS_CLOCK;
  if (t->error)
    return t->error;
  t->now = now;
  if (t->stage != DONE && now >= t->deadline)
    return fail(t, SOPHIA_BS_DEADLINE);
  e = sophia_bs_advance(&t->submission, now);
  if (e)
    return fail(t, e);
  if (t->has_assembly_due && now >= t->assembly_due)
    return fail(t, SOPHIA_BS_DEADLINE);
  return 0;
}
int sophia_bt_operation(struct sophia_bt *t, uint64_t now, uint32_t msize,
                        struct sophia_bt_operation *out) {
  struct sophia_bt_operation op = {0};
  int e;
  if (!out)
    return SOPHIA_BS_INVALID;
  e = advance(t, now);
  if (e)
    return e;
  if (t->outstanding || t->stage == WAIT || t->stage == DONE) {
    *out = op;
    return 0;
  }
  if (msize < 512)
    return fail(t, SOPHIA_BS_INVALID);
  switch (t->stage) {
  case WALK_TRANSACTION:
  case WALK_SUBMIT:
    op.kind = SOPHIA_BT_WALK;
    op.name = t->stage == WALK_TRANSACTION ? "transaction" : "submit";
    break;
  case OPEN_TRANSACTION:
  case OPEN_SUBMIT:
    op.kind = SOPHIA_BT_OPEN;
    op.fid = t->stage == OPEN_TRANSACTION ? t->transaction : t->submit;
    op.flags = t->stage == OPEN_TRANSACTION ? 2 : 1;
    break;
  case STAGING:
    op.kind = SOPHIA_BT_WRITE;
    op.fid = t->transaction;
    op.offset = t->offset;
    op.bytes = t->submission.bytes + t->offset;
    op.length = t->submission.length - t->offset;
    if (op.length > msize - 23)
      op.length = msize - 23;
    if (t->iounit && op.length > t->iounit)
      op.length = t->iounit;
    break;
  case SUBMIT:
    op.kind = SOPHIA_BT_WRITE;
    op.fid = t->submit;
    op.bytes = t->submission.control;
    op.length = 24;
    break;
  case CLUNK_SUBMIT:
  case CLUNK_TRANSACTION:
    op.kind = SOPHIA_BT_CLUNK;
    op.fid = t->stage == CLUNK_SUBMIT ? t->submit : t->transaction;
    break;
  default:
    return fail(t, SOPHIA_BS_PROTOCOL);
  }
  *out = op;
  return 0;
}
int sophia_bt_issued(struct sophia_bt *t, uint64_t now, uint32_t msize,
                     uint32_t fid) {
  struct sophia_bt_operation op;
  int e = sophia_bt_operation(t, now, msize, &op);
  if (e)
    return e;
  if (op.kind == SOPHIA_BT_NONE)
    return SOPHIA_BS_BUSY;
  if ((op.kind == SOPHIA_BT_WALK) != (fid != SOPHIA_9P_NOFID))
    return SOPHIA_BS_INVALID;
  t->sent = op.length;
  switch (t->stage) {
  case WALK_TRANSACTION:
    t->transaction = fid;
    break;
  case WALK_SUBMIT:
    t->submit = fid;
    break;
  case STAGING:
    if (!t->has_assembly_due) {
      if (now > UINT64_MAX - t->assembly_timeout)
        return fail(t, SOPHIA_BS_OVERFLOW);
      t->assembly_due = now + t->assembly_timeout;
      t->has_assembly_due = 1;
    }
    break;
  case SUBMIT:
    e = sophia_bs_issued(&t->submission, now);
    if (e)
      return e;
    break;
  default:
    break;
  }
  t->outstanding = 1;
  return 0;
}
int sophia_bt_receipt(struct sophia_bt *t, const struct sophia_bf_event *v,
                      uint64_t now) {
  int e = advance(t, now);
  if (e)
    return e;
  e = sophia_bs_receipt(&t->submission, v, now);
  if (e)
    return fail(t, e);
  t->has_assembly_due = 0;
  return 0;
}
int sophia_bt_retry(struct sophia_bt *t, uint64_t now) {
  int e = advance(t, now);
  if (e)
    return e;
  if (!sophia_bt_retry_ready(t))
    return SOPHIA_BS_BUSY;
  t->stage = SUBMIT;
  return 0;
}
int sophia_bt_release(struct sophia_bt *t, uint64_t acknowledged,
                      uint64_t now) {
  int e = advance(t, now);
  if (e)
    return e;
  if (t->stage != WAIT || t->outstanding ||
      !sophia_bs_releasable(&t->submission, acknowledged))
    return SOPHIA_BS_BUSY;
  /* Integrated sessions own submit for the connection lifetime. The assembly
   * remains transaction-owned and is released only after receipt acknowledgement. */
  t->stage = t->retain_submit ? CLUNK_TRANSACTION : CLUNK_SUBMIT;
  return 0;
}
static int accept(struct sophia_bt *t, const struct sophia_9p_reply *r,
                  uint64_t now) {
  int e;
  if (t->stage == SUBMIT) {
    if (r->type != 119 && r->type != 7)
      return SOPHIA_BS_PROTOCOL;
    e = sophia_bs_reply(&t->submission, r->count, r->type == 7 ? r->error : 0,
                        now);
    if (e) {
      t->remote_error = t->submission.remote_error;
      return e;
    }
    if (t->submission.custody != SOPHIA_BS_QUEUED)
      t->has_assembly_due = 0;
    t->stage = WAIT;
    return 0;
  }
  if (r->type == 7) {
    t->remote_error = r->error;
    return SOPHIA_BS_REMOTE;
  }
  switch (t->stage) {
  case WALK_TRANSACTION:
  case WALK_SUBMIT:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_BS_PROTOCOL;
    t->stage = t->stage == WALK_TRANSACTION ? OPEN_TRANSACTION : OPEN_SUBMIT;
    break;
  case OPEN_TRANSACTION:
  case OPEN_SUBMIT:
    if (r->type != 13 || r->qid.type ||
        (t->stage == OPEN_SUBMIT && r->iounit && r->iounit < 24))
      return SOPHIA_BS_PROTOCOL;
    t->iounit = r->iounit;
    t->stage = t->stage == OPEN_TRANSACTION ? STAGING : SUBMIT;
    break;
  case STAGING:
    if (r->type != 119 || r->count != t->sent)
      return SOPHIA_BS_PROTOCOL;
    t->offset += t->sent;
    if (t->offset == t->submission.length)
      t->stage = t->submit == SOPHIA_9P_NOFID ? WALK_SUBMIT : SUBMIT;
    break;
  case CLUNK_SUBMIT:
  case CLUNK_TRANSACTION:
    if (r->type != 121)
      return SOPHIA_BS_PROTOCOL;
    t->stage = t->stage == CLUNK_SUBMIT ? CLUNK_TRANSACTION : DONE;
    break;
  default:
    return SOPHIA_BS_PROTOCOL;
  }
  return 0;
}
int sophia_bt_reply(struct sophia_bt *t, const struct sophia_9p_reply *r,
                    uint64_t now) {
  int e = advance(t, now);
  if (e)
    return e;
  if (!r)
    return SOPHIA_BS_INVALID;
  if (!t->outstanding)
    return SOPHIA_BS_BUSY;
  t->outstanding = 0;
  e = accept(t, r, now);
  return e ? fail(t, e) : 0;
}
