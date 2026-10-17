#include "sophia_broker_journal.h"
#include <assert.h>
#include <string.h>
static void event(unsigned char *b, unsigned sequence) {
  memset(b, 0, 200);
  b[0] = 200;
  b[4] = 1;
  b[6] = 32;
  b[8] = 7;
  b[24] = (unsigned char)sequence;
  b[32] = 1;
  b[40] = 2;
  b[56] = 3;
  b[59] = 1;
  b[60] = 2;
  b[73] = 10;
}
int main(void) {
  struct sophia_bj j;
  struct sophia_bf_event v;
  unsigned char a[200], b[200], c[200], batch[500], ack[16];
  const char *pointer;
  unsigned fault;
  event(a, 1);
  event(b, 2);
  event(c, 3);
  assert(!sophia_bj_init(&j, 7, 0, 100));
  assert(!sophia_bj_receive(&j, a, 3, 1));
  assert(sophia_bj_event(&j, &v) == SOPHIA_BS_BUSY && j.due == 101);
  assert(!sophia_bj_receive(&j, a + 3, 197, 2));
  assert(!sophia_bj_event(&j, &v));
  assert(v.request.label_length == 2 && !memcmp(v.request.label, "\0\n", 2));
  pointer = v.request.label;
  assert(!sophia_bj_read_count(&j));
  assert(sophia_bj_receive(&j, b, 200, 3) == SOPHIA_BS_BUSY);
  assert(!sophia_bj_event(&j, &v) && v.request.label == pointer);
  assert(!sophia_bj_consume(&j, 3));
  assert(!sophia_bj_begin_ack(&j, 4));
  memcpy(ack, j.ack, 16);
  assert(!sophia_bj_receive(&j, b, 200, 5));
  assert(!sophia_bj_consume(&j, 6));
  assert(!memcmp(ack, j.ack, 16) && !j.acknowledged);
  assert(!sophia_bj_ack_reply(&j, 16, 0, 7) && j.acknowledged == 1 &&
         j.due == 107);
  assert(!sophia_bj_begin_ack(&j, 8));
  assert(!sophia_bj_ack_reply(&j, 16, 0, 9) && !j.has_due);
  assert(!sophia_bj_init(&j, 7, 0, 100));
  memcpy(batch, a, 200);
  memcpy(batch + 200, b, 200);
  memcpy(batch + 400, c, 100);
  assert(!sophia_bj_receive(&j, batch, 500, 1));
  assert(!sophia_bj_consume(&j, 2));
  assert(!sophia_bj_consume(&j, 3));
  assert(sophia_bj_event(&j, &v) == SOPHIA_BS_BUSY);
  assert(!sophia_bj_receive(&j, c + 100, 100, 4));
  assert(!sophia_bj_event(&j, &v) && v.sequence == 3 && j.offset == 600);
  for (fault = 0; fault < 4; ++fault) {
    event(a, 1);
    if (fault == 0)
      a[24] = 2;
    if (fault == 1)
      a[8] = 8;
    if (fault == 2)
      a[0] = 201;
    if (fault == 3)
      a[72] = 255;
    assert(!sophia_bj_init(&j, 7, 0, 100));
    assert(sophia_bj_receive(&j, a, 200, 1) == SOPHIA_BS_PROTOCOL);
    assert(sophia_bj_event(&j, &v) == SOPHIA_BS_PROTOCOL);
  }
  event(a, 1);
  assert(!sophia_bj_init(&j, 7, 0, 10));
  assert(!sophia_bj_receive(&j, a, 1, 1));
  assert(sophia_bj_advance(&j, 0) == SOPHIA_BS_CLOCK && !j.error);
  assert(!sophia_bj_receive(&j, a + 1, 199, 10));
  assert(sophia_bj_advance(&j, 11) == SOPHIA_BS_DEADLINE);
  for (fault = 0; fault < 2; ++fault) {
    assert(!sophia_bj_init(&j, 7, 0, 100));
    assert(!sophia_bj_receive(&j, a, 200, 1));
    assert(!sophia_bj_consume(&j, 2));
    assert(!sophia_bj_begin_ack(&j, 3));
    assert(sophia_bj_ack_reply(&j, fault ? 0 : 15, fault ? 5 : 0, 4) &&
           !j.acknowledged);
  }
  assert(!sophia_bj_init(&j, 7, UINT64_MAX - 1, 10));
  assert(sophia_bj_receive(&j, a, 200, UINT64_MAX - 1) == SOPHIA_BS_OVERFLOW &&
         !j.offset);
  return 0;
}
