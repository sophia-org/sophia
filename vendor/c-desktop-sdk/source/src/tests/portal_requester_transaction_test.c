#include "sophia_portal_requester_transaction.h"
#include <assert.h>
#include <string.h>
static void init(struct sophia_pqt *t) {
  struct sophia_pq_candidate c = {0};
  c.kind = SOPHIA_PQ_NEGOTIATE;
  c.data.negotiate.minimum = c.data.negotiate.maximum = 1;
  assert(!sophia_pqt_init(t, 7, 9, &c, 0, 100, 20));
}
static int reply(struct sophia_pqt *t, unsigned type, uint32_t count,
                 uint32_t unit, uint32_t error, uint64_t now) {
  struct sophia_9p_reply r = {0};
  r.type = (uint8_t)type;
  r.count = count;
  r.iounit = unit;
  r.error = error;
  return sophia_pqt_reply(t, &r, now);
}
static void receipt(struct sophia_pqt *t, uint64_t now) {
  struct sophia_pq_event e = {0};
  e.epoch = 7;
  e.sequence = 1;
  e.kind = SOPHIA_PQ_SUBMITTED;
  e.data.submitted.submission = 9;
  e.data.submitted.candidate_kind = SOPHIA_PQ_NEGOTIATE;
  assert(!sophia_pqt_receipt(t, &e, now));
}
static void prepare(struct sophia_pqt *t) {
  struct sophia_pqt_operation op;
  unsigned char staged[344];
  size_t used = 0;
  assert(!sophia_pqt_operation(t, 1, 512, &op));
  assert(op.kind == SOPHIA_PQT_WALK && !strcmp(op.name, "transaction"));
  assert(!sophia_pqt_issued(t, 1, 512, 2));
  assert(!reply(t, 111, 1, 0, 0, 1));
  assert(!sophia_pqt_operation(t, 1, 512, &op));
  assert(op.kind == SOPHIA_PQT_OPEN && op.fid == 2 && op.flags == 2);
  assert(!sophia_pqt_issued(t, 1, 512, SOPHIA_9P_NOFID));
  assert(!reply(t, 13, 0, 7, 0, 1));
  for (;;) {
    assert(!sophia_pqt_operation(t, 1, 512, &op));
    if (op.kind != SOPHIA_PQT_WRITE)
      break;
    assert(op.fid == 2 && op.offset == used && op.length <= 7);
    memcpy(staged + used, op.bytes, op.length);
    used += op.length;
    assert(!sophia_pqt_issued(t, 1, 512, SOPHIA_9P_NOFID));
    assert(!reply(t, 119, (uint32_t)op.length, 0, 0, 1));
  }
  assert(used == 40 && !memcmp(staged, t->submission.bytes, used));
  assert(op.kind == SOPHIA_PQT_WALK && !strcmp(op.name, "submit"));
  assert(!sophia_pqt_issued(t, 1, 512, 3));
  assert(!reply(t, 111, 1, 0, 0, 1));
  assert(!sophia_pqt_issued(t, 1, 512, SOPHIA_9P_NOFID));
  assert(!reply(t, 13, 0, 24, 0, 1));
  assert(!sophia_pqt_operation(t, 1, 512, &op));
  assert(op.kind == SOPHIA_PQT_WRITE && op.fid == 3 && !op.offset &&
         op.length == 24);
}
int main(void) {
  struct sophia_pqt t;
  struct sophia_pqt_operation op;
  uint32_t fid;
  unsigned fault;
  uint64_t deadline;
  init(&t);
  prepare(&t);
  assert(!sophia_pqt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
  assert(!reply(&t, 7, 0, 0, 11, 3));
  assert(t.submission.custody == SOPHIA_PQS_QUEUED);
  assert(!sophia_pqt_operation(&t, 4, 512, &op) && op.kind == SOPHIA_PQT_NONE);
  assert(!sophia_pqt_retry(&t, 5));
  assert(!sophia_pqt_issued(&t, 5, 512, SOPHIA_9P_NOFID));
  receipt(&t, 6);
  assert(sophia_pqt_release(&t, 1, 7) == SOPHIA_PQS_BUSY);
  assert(!reply(&t, 119, 24, 0, 0, 8));
  assert(sophia_pqt_release(&t, 0, 9) == SOPHIA_PQS_BUSY);
  assert(!sophia_pqt_release(&t, 1, 10));
  for (fid = 3; fid >= 2; --fid) {
    assert(!sophia_pqt_operation(&t, 11, 512, &op));
    assert(op.kind == SOPHIA_PQT_CLUNK && op.fid == fid);
    assert(!sophia_pqt_issued(&t, 11, 512, SOPHIA_9P_NOFID));
    assert(!reply(&t, 121, 0, 0, 0, 11));
  }
  assert(sophia_pqt_done(&t) && t.submission.custody == SOPHIA_PQS_SUBMITTED);
  assert(!sophia_pqt_deadline(&t, &deadline));
  init(&t);
  prepare(&t);
  assert(sophia_pqt_deadline(&t, &deadline) && deadline == 21);
  assert(sophia_pqt_operation(&t, 21, 512, &op) == SOPHIA_PQS_DEADLINE);
  assert(t.submission.custody == SOPHIA_PQS_DROPPED_UNSENT);
  init(&t);
  prepare(&t);
  assert(!sophia_pqt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
  sophia_pqt_close(&t);
  assert(t.submission.custody == SOPHIA_PQS_UNKNOWN);
  init(&t);
  prepare(&t);
  assert(!sophia_pqt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
  receipt(&t, 3);
  sophia_pqt_close(&t);
  assert(t.submission.custody == SOPHIA_PQS_SUBMITTED);
  for (fault = 0; fault < 2; ++fault) {
    init(&t);
    prepare(&t);
    assert(!sophia_pqt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
    assert(!reply(&t, 119, 24, 0, 0, 3));
    receipt(&t, 4);
    assert(!sophia_pqt_release(&t, 1, 5));
    assert(!sophia_pqt_issued(&t, 6, 512, SOPHIA_9P_NOFID));
    if (fault)
      assert(reply(&t, 7, 0, 0, 5, 7) == SOPHIA_PQS_REMOTE);
    else
      assert(sophia_pqt_operation(&t, 100, 512, &op) == SOPHIA_PQS_DEADLINE);
    assert(!sophia_pqt_done(&t) &&
           t.submission.custody == SOPHIA_PQS_SUBMITTED);
  }
  return 0;
}
