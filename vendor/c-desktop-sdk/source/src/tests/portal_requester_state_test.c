#include "../sophia_portal_requester_state.h"
#include <assert.h>
#include <string.h>
static void put(unsigned char *b, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    b[i] = (unsigned char)(v >> (8 * i));
}
static void wire(unsigned char b[48], uint64_t sequence) {
  memset(b, 0, 48);
  put(b, 48, 4);
  put(b + 4, 1, 2);
  put(b + 6, 18, 2);
  put(b + 8, 7, 8);
  put(b + 24, sequence, 8);
  put(b + 32, 9, 8);
  put(b + 40, 256, 2);
}
static void init(struct sophia_pqs_submission *s) {
  struct sophia_pq_candidate c = {0};
  c.kind = SOPHIA_PQ_NEGOTIATE;
  c.data.negotiate.minimum = c.data.negotiate.maximum = 1;
  assert(!sophia_pqs_init(s, 7, 9, &c, 0, 100));
}
int main(void) {
  struct sophia_pqs_submission s;
  struct sophia_pqj_journal j;
  struct sophia_pq_event event;
  unsigned char bytes[96], saved[344], ack[16];
  int first;
  size_t i;
  wire(bytes, 1);
  wire(bytes + 48, 2);
  assert(!sophia_pq_decode_event(bytes, 48, &event));
  for (first = 0; first < 2; ++first) {
    init(&s);
    assert(!sophia_pqs_issued(&s, 1));
    if (first)
      assert(!sophia_pqs_receipt(&s, &event, 2));
    assert(!sophia_pqs_reply(&s, 24, 0, 3));
    if (!first)
      assert(!sophia_pqs_receipt(&s, &event, 4));
    assert(s.custody == SOPHIA_PQS_SUBMITTED);
    assert(!sophia_pqs_releasable(&s, 0));
    assert(sophia_pqs_releasable(&s, 1));
    assert(!sophia_pqs_advance(&s, 1000));
    sophia_pqs_close(&s);
    assert(s.custody == SOPHIA_PQS_SUBMITTED);
    assert(!sophia_pqs_releasable(&s, 1));
  }
  init(&s);
  memcpy(saved, s.bytes, sizeof saved);
  assert(!sophia_pqs_issued(&s, 1));
  assert(!sophia_pqs_reply(&s, 0, 11, 2));
  assert(s.custody == SOPHIA_PQS_QUEUED && s.deadline == 100);
  assert(!memcmp(saved, s.bytes, sizeof saved));
  assert(sophia_pqs_reply(&s, 24, 0, 3) == SOPHIA_PQS_BUSY);
  assert(!sophia_pqs_issued(&s, 4));
  sophia_pqs_close(&s);
  assert(s.custody == SOPHIA_PQS_UNKNOWN);
  init(&s);
  assert(sophia_pqs_advance(&s, 100) == SOPHIA_PQS_DEADLINE);
  assert(s.custody == SOPHIA_PQS_DROPPED_UNSENT);
  init(&s);
  assert(!sophia_pqs_issued(&s, 1));
  assert(sophia_pqs_reply(&s, 23, 0, 2) == SOPHIA_PQS_PROTOCOL);
  assert(s.custody == SOPHIA_PQS_UNKNOWN);
  init(&s);
  assert(!sophia_pqs_issued(&s, 1));
  event.epoch = 8;
  assert(sophia_pqs_receipt(&s, &event, 2) == SOPHIA_PQS_PROTOCOL);
  event.epoch = 7;
  init(&s);
  assert(!sophia_pqs_issued(&s, 1));
  assert(!sophia_pqs_receipt(&s, &event, 2));
  assert(sophia_pqs_reply(&s, 0, 11, 3) == SOPHIA_PQS_PROTOCOL);
  assert(s.custody == SOPHIA_PQS_SUBMITTED);

  assert(!sophia_pqj_init(&j, 7, 0, 100));
  for (i = 0; i < 45; i += 3)
    assert(!sophia_pqj_receive(&j, bytes + i, 3, 1));
  assert(!sophia_pqj_receive(&j, bytes + 45, 2, 1));
  assert(!sophia_pqj_event(&j));
  assert(!sophia_pqj_receive(&j, bytes + 47, 49, 2));
  assert(sophia_pqj_event(&j)->sequence == 1);
  assert(!sophia_pqj_read_count(&j));
  assert(sophia_pqj_receive(&j, bytes, 48, 3) == SOPHIA_PQS_BUSY);
  assert(!sophia_pqj_consume(&j, 4));
  assert(!sophia_pqj_begin_ack(&j, 5));
  memcpy(ack, j.ack, 16);
  assert(sophia_pqj_event(&j)->sequence == 2);
  assert(!sophia_pqj_consume(&j, 6));
  assert(!memcmp(ack, j.ack, 16) && !j.acknowledged);
  assert(!sophia_pqj_ack_reply(&j, 16, 0, 7));
  assert(j.acknowledged == 1 && j.due == 107);
  assert(!sophia_pqj_begin_ack(&j, 8));
  assert(!sophia_pqj_ack_reply(&j, 16, 0, 9));
  assert(j.acknowledged == 2 && !j.has_due && j.offset == 96);

  assert(!sophia_pqj_init(&j, 7, 0, 100));
  assert(!sophia_pqj_receive(&j, bytes, 4, 1));
  assert(!sophia_pqj_receive(&j, bytes + 4, 44, 80));
  assert(!sophia_pqj_consume(&j, 90));
  assert(!sophia_pqj_begin_ack(&j, 99));
  assert(j.due == 101);
  assert(sophia_pqj_ack_reply(&j, 16, 0, 101) == SOPHIA_PQS_DEADLINE);
  assert(!j.acknowledged);
  assert(!sophia_pqj_init(&j, 7, 5, 100));
  assert(sophia_pqj_receive(&j, bytes, 48, 4) == SOPHIA_PQS_CLOCK && !j.error);
  assert(!sophia_pqj_receive(&j, bytes, 48, 5));
  assert(!sophia_pqj_consume(&j, 6));
  assert(!sophia_pqj_begin_ack(&j, 7));
  assert(sophia_pqj_ack_reply(&j, 15, 0, 8) == SOPHIA_PQS_PROTOCOL &&
         !j.acknowledged);
  assert(!sophia_pqj_init(&j, 7, 0, 100));
  assert(sophia_pqj_receive(&j, bytes + 48, 48, 1) == SOPHIA_PQS_PROTOCOL);
  assert(!sophia_pqj_event(&j));
  assert(!sophia_pqj_init(&j, 7, 0, 100));
  assert(sophia_pqj_receive(&j, bytes, 0, 1) == SOPHIA_PQS_PROTOCOL);
  return 0;
}
