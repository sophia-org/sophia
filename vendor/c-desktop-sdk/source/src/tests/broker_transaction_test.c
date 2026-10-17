#include "sophia_broker_transaction.h"
#include <assert.h>
#include <string.h>
static void init(struct sophia_bt *t) {
  assert(!sophia_bt_negotiate(t, 7, 9, 2, 2, 0, 100, 20));
}
static int reply(struct sophia_bt *t, unsigned type, uint32_t count,
                 uint32_t unit, uint32_t error, uint64_t now) {
  struct sophia_9p_reply r = {0};
  r.type = (uint8_t)type;
  r.count = count;
  r.iounit = unit;
  r.error = error;
  return sophia_bt_reply(t, &r, now);
}
static void receipt(struct sophia_bt *t, uint64_t now) {
  struct sophia_bf_event e = {0};
  e.epoch = 7;
  e.sequence = 1;
  e.kind = SOPHIA_BF_SUBMITTED;
  e.submission = 9;
  e.candidate_kind = t->submission.kind;
  assert(!sophia_bt_receipt(t, &e, now));
}
static void prepare(struct sophia_bt *t) {
  struct sophia_bt_operation op;
  unsigned char staged[448];
  size_t used = 0;
  assert(!sophia_bt_operation(t, 1, 512, &op));
  assert(op.kind == SOPHIA_BT_WALK && !strcmp(op.name, "transaction"));
  assert(!sophia_bt_issued(t, 1, 512, 2));
  assert(!reply(t, 111, 1, 0, 0, 1));
  assert(!sophia_bt_operation(t, 1, 512, &op));
  assert(op.kind == SOPHIA_BT_OPEN && op.fid == 2 && op.flags == 2);
  assert(!sophia_bt_issued(t, 1, 512, SOPHIA_9P_NOFID));
  assert(!reply(t, 13, 0, 7, 0, 1));
  for (;;) {
    assert(!sophia_bt_operation(t, 1, 512, &op));
    if (op.kind != SOPHIA_BT_WRITE)
      break;
    assert(op.fid == 2 && op.offset == used && op.length <= 7);
    memcpy(staged + used, op.bytes, op.length);
    used += op.length;
    assert(!sophia_bt_issued(t, 1, 512, SOPHIA_9P_NOFID));
    assert(!reply(t, 119, (uint32_t)op.length, 0, 0, 1));
  }
  assert(used == t->submission.length &&
         !memcmp(staged, t->submission.bytes, used));
  assert(op.kind == SOPHIA_BT_WALK && !strcmp(op.name, "submit"));
  assert(!sophia_bt_issued(t, 1, 512, 3));
  assert(!reply(t, 111, 1, 0, 0, 1));
  assert(!sophia_bt_issued(t, 1, 512, SOPHIA_9P_NOFID));
  assert(!reply(t, 13, 0, 24, 0, 1));
  assert(!sophia_bt_operation(t, 1, 512, &op));
  assert(op.kind == SOPHIA_BT_WRITE && op.fid == 3 && !op.offset &&
         op.length == 24);
}
int main(void) {
  struct sophia_bt t;
  struct sophia_bt_operation op;
  uint32_t fid;
  unsigned fault;
  uint64_t deadline;
  init(&t);
  prepare(&t);
  assert(!sophia_bt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
  assert(!reply(&t, 7, 0, 0, 11, 3));
  assert(t.submission.custody == SOPHIA_BS_QUEUED);
  assert(!sophia_bt_operation(&t, 4, 512, &op) && op.kind == SOPHIA_BT_NONE);
  assert(!sophia_bt_retry(&t, 5));
  assert(!sophia_bt_issued(&t, 5, 512, SOPHIA_9P_NOFID));
  receipt(&t, 6);
  assert(sophia_bt_release(&t, 1, 7) == SOPHIA_BS_BUSY);
  assert(!reply(&t, 119, 24, 0, 0, 8));
  assert(sophia_bt_release(&t, 0, 9) == SOPHIA_BS_BUSY);
  assert(!sophia_bt_release(&t, 1, 10));
  for (fid = 3; fid >= 2; --fid) {
    assert(!sophia_bt_operation(&t, 11, 512, &op));
    assert(op.kind == SOPHIA_BT_CLUNK && op.fid == fid);
    assert(!sophia_bt_issued(&t, 11, 512, SOPHIA_9P_NOFID));
    assert(!reply(&t, 121, 0, 0, 0, 11));
  }
  assert(sophia_bt_done(&t) && t.submission.custody == SOPHIA_BS_SUBMITTED);
  assert(!sophia_bt_deadline(&t, &deadline));
  init(&t);
  prepare(&t);
  assert(sophia_bt_deadline(&t, &deadline) && deadline == 21);
  assert(sophia_bt_operation(&t, 21, 512, &op) == SOPHIA_BS_DEADLINE);
  assert(t.submission.custody == SOPHIA_BS_DROPPED_UNSENT);
  init(&t);
  prepare(&t);
  assert(!sophia_bt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
  sophia_bt_close(&t);
  assert(t.submission.custody == SOPHIA_BS_UNKNOWN);
  init(&t);
  prepare(&t);
  assert(!sophia_bt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
  receipt(&t, 3);
  sophia_bt_close(&t);
  assert(t.submission.custody == SOPHIA_BS_SUBMITTED);
  for (fault = 0; fault < 2; ++fault) {
    init(&t);
    prepare(&t);
    assert(!sophia_bt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
    assert(!reply(&t, 119, 24, 0, 0, 3));
    receipt(&t, 4);
    assert(!sophia_bt_release(&t, 1, 5));
    assert(!sophia_bt_issued(&t, 6, 512, SOPHIA_9P_NOFID));
    if (fault)
      assert(reply(&t, 7, 0, 0, 5, 7) == SOPHIA_BS_REMOTE);
    else
      assert(sophia_bt_operation(&t, 100, 512, &op) == SOPHIA_BS_DEADLINE);
    assert(!sophia_bt_done(&t) && t.submission.custody == SOPHIA_BS_SUBMITTED);
  }
  {
    struct sophia_bf_request request = {0};
    struct sophia_bf_row row = {0};
    request.id = 8;
    request.kind = SOPHIA_BF_SURFACE_REMOVED;
    request.surface_index = 2;
    request.surface_generation = 3;
    row.kind = SOPHIA_BF_RETIRE_SURFACE;
    row.surface_index = 2;
    row.surface_generation = 3;
    assert(!sophia_bt_response(&t, 7, 9, &request, &row, 1, 0, 100, 20));
    assert(t.submission.length == 448);
    prepare(&t);
    assert(!sophia_bt_issued(&t, 2, 512, SOPHIA_9P_NOFID));
    assert(!reply(&t, 119, 24, 0, 0, 3));
    receipt(&t, 4);
    assert(!sophia_bt_release(&t, 1, 5));
    for (fid = 3; fid >= 2; --fid) {
      assert(!sophia_bt_issued(&t, 6, 512, SOPHIA_9P_NOFID));
      assert(!reply(&t, 121, 0, 0, 0, 6));
    }
    assert(sophia_bt_done(&t));
  }
  return 0;
}
