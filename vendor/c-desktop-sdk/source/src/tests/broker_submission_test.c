#include "sophia_broker_submission.h"
#include <assert.h>
#include <string.h>
static void init(struct sophia_bs_submission *s) {
  assert(!sophia_bs_negotiate(s, 7, 9, 2, 2, 0, 100));
}
int main(void) {
  struct sophia_bs_submission s;
  struct sophia_bf_event receipt = {0}, r;
  unsigned first, fault;
  unsigned char bytes[448], control[24];
  receipt.epoch = 7;
  receipt.sequence = 3;
  receipt.kind = SOPHIA_BF_SUBMITTED;
  receipt.submission = 9;
  receipt.candidate_kind = 256;
  for (first = 0; first < 2; ++first) {
    init(&s);
    memcpy(bytes, s.bytes, 448);
    memcpy(control, s.control, 24);
    assert(!sophia_bs_issued(&s, 1));
    assert(!sophia_bs_reply(&s, 0, 11, 2));
    assert(s.custody == SOPHIA_BS_QUEUED && !sophia_bs_advance(&s, 3));
    assert(!memcmp(bytes, s.bytes, 448) && !memcmp(control, s.control, 24));
    assert(!sophia_bs_issued(&s, 4));
    if (first) {
      assert(!sophia_bs_receipt(&s, &receipt, 5));
      assert(!sophia_bs_releasable(&s, 3));
    }
    assert(!sophia_bs_reply(&s, 24, 0, 6));
    if (!first) {
      assert(s.custody == SOPHIA_BS_WRITE_ACCEPTED);
      assert(!sophia_bs_receipt(&s, &receipt, 7));
    }
    assert(!sophia_bs_releasable(&s, 2) && sophia_bs_releasable(&s, 3));
    assert(!sophia_bs_advance(&s, 100));
    sophia_bs_close(&s);
    assert(s.custody == SOPHIA_BS_SUBMITTED);
  }
  init(&s);
  sophia_bs_close(&s);
  assert(s.custody == SOPHIA_BS_DROPPED_UNSENT);
  for (fault = 0; fault < 3; ++fault) {
    const uint32_t counts[] = {0, 23, 25};
    init(&s);
    assert(!sophia_bs_issued(&s, 1));
    assert(sophia_bs_reply(&s, counts[fault], 0, 2) == SOPHIA_BS_PROTOCOL &&
           s.custody == SOPHIA_BS_UNKNOWN);
  }
  init(&s);
  assert(!sophia_bs_issued(&s, 1));
  assert(sophia_bs_reply(&s, 0, 116, 2) == SOPHIA_BS_REMOTE &&
         s.remote_error == 116 && s.custody == SOPHIA_BS_REFUSED);
  init(&s);
  assert(!sophia_bs_issued(&s, 1));
  assert(!sophia_bs_receipt(&s, &receipt, 2));
  sophia_bs_close(&s);
  assert(s.custody == SOPHIA_BS_SUBMITTED && !sophia_bs_releasable(&s, 3));
  for (fault = 0; fault < 4; ++fault) {
    init(&s);
    assert(!sophia_bs_issued(&s, 1));
    r = receipt;
    if (fault == 0)
      r.epoch = 8;
    if (fault == 1)
      r.sequence = 0;
    if (fault == 2)
      r.submission = 10;
    if (fault == 3)
      r.candidate_kind = 257;
    assert(sophia_bs_receipt(&s, &r, 2) == SOPHIA_BS_PROTOCOL &&
           s.custody == SOPHIA_BS_UNKNOWN);
  }
  init(&s);
  assert(!sophia_bs_advance(&s, 5));
  assert(sophia_bs_advance(&s, 4) == SOPHIA_BS_CLOCK && !s.error);
  assert(!sophia_bs_issued(&s, 6));
  assert(!sophia_bs_reply(&s, 0, 11, 99));
  assert(sophia_bs_issued(&s, 100) == SOPHIA_BS_DEADLINE &&
         s.custody == SOPHIA_BS_DROPPED_UNSENT);
  init(&s);
  assert(!sophia_bs_issued(&s, 1));
  assert(sophia_bs_advance(&s, 100) == SOPHIA_BS_DEADLINE &&
         s.custody == SOPHIA_BS_UNKNOWN);
  {
    struct sophia_bf_request q = {0};
    struct sophia_bf_row row = {0};
    q.id = 1;
    q.kind = SOPHIA_BF_CANDIDATE_REDUCED;
    q.value = 1;
    row.kind = SOPHIA_BF_NO_CHANGE;
    assert(!sophia_bs_response(&s, 7, 9, &q, &row, 1, 0, 100));
    q.id = 2;
    row.kind = SOPHIA_BF_REJECTED;
    assert(s.length == 448 && s.bytes[32] == 1 && s.bytes[48] == 5 &&
           s.control[16] == 192 && s.control[17] == 1);
  }
  return 0;
}
