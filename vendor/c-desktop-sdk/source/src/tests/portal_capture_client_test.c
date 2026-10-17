#include "sophia_portal_capture.h"
#include <assert.h>
#include <string.h>

static struct sophia_pc pending(void) {
  struct sophia_pc s;
  struct sophia_pf_capture_request r = {9, 7};
  assert(!sophia_pc_init(&s, r, 0, 100));
  return s;
}
static struct sophia_pc accepted(void) {
  struct sophia_pc s = pending();
  assert(!sophia_pc_issued(&s, 0));
  assert(!sophia_pc_write_reply(&s, 0, 24, 1));
  return s;
}
static int status(struct sophia_pc *s, unsigned serial, unsigned output,
                  unsigned state, uint64_t now) {
  unsigned char b[32] = {1};
  b[8] = (unsigned char)serial;
  b[16] = (unsigned char)output;
  b[24] = (unsigned char)state;
  return sophia_pc_status(s, b, sizeof b, now);
}
int main(void) {
  const unsigned char request[24] = {1, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0,
                                     0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0};
  const uint32_t refusals[] = {11, 116, 13, 22},
                 counts[] = {0, 1, 23, 25, UINT32_MAX};
  struct sophia_pc s;
  uint64_t deadline, wake, now;
  unsigned outcome, state, i;
  for (outcome = 3; outcome <= 7; ++outcome) {
    s = pending();
    assert(!memcmp(s.bytes, request, sizeof request));
    assert(sophia_pc_start_status(&s, 0, &deadline) == SOPHIA_PC_BUSY);
    assert(!sophia_pc_issued(&s, 0));
    assert(sophia_pc_issued(&s, 0) == SOPHIA_PC_BUSY);
    assert(!sophia_pc_write_reply(&s, 0, 24, 1));
    assert(s.custody == SOPHIA_PC_ACCEPTED);
    for (state = 1; state <= 2; ++state) {
      assert(sophia_pc_next_wake(&s, &wake) && wake == state);
      assert(!sophia_pc_start_status(&s, state, &deadline) && deadline == 100);
      assert(sophia_pc_start_status(&s, state, &deadline) == SOPHIA_PC_BUSY);
      assert(!status(&s, 9, 7, state, state));
      assert(s.custody == SOPHIA_PC_ACCEPTED);
      assert(sophia_pc_start_status(&s, state, &deadline) == SOPHIA_PC_BUSY);
    }
    assert(!sophia_pc_start_status(&s, 3, &deadline));
    assert(!status(&s, 9, 7, outcome, 3));
    assert(s.custody == SOPHIA_PC_SETTLED && (unsigned)s.outcome == outcome);
    assert(!sophia_pc_next_wake(&s, &wake));
    assert(!sophia_pc_advance(&s, 1000));
    sophia_pc_close(&s);
    assert(s.custody == SOPHIA_PC_SETTLED && (unsigned)s.outcome == outcome);
  }
  for (i = 0; i < sizeof refusals / sizeof refusals[0]; ++i) {
    s = pending();
    assert(!sophia_pc_issued(&s, 0));
    assert(sophia_pc_write_reply(&s, 1, refusals[i], 1) == SOPHIA_PC_REFUSAL);
    assert(s.custody == SOPHIA_PC_REFUSED && s.remote_error == refusals[i]);
    sophia_pc_close(&s);
    assert(sophia_pc_issued(&s, 2) == SOPHIA_PC_REFUSAL);
  }
  for (i = 0; i < sizeof counts / sizeof counts[0]; ++i) {
    s = pending();
    assert(!sophia_pc_issued(&s, 0));
    assert(sophia_pc_write_reply(&s, 0, counts[i], 1) == SOPHIA_PC_PROTOCOL);
    assert(s.custody == SOPHIA_PC_UNKNOWN);
    assert(sophia_pc_issued(&s, 2) == SOPHIA_PC_PROTOCOL);
  }
  s = pending();
  sophia_pc_close(&s);
  assert(s.custody == SOPHIA_PC_DROPPED_UNSENT);
  s = pending();
  assert(!sophia_pc_issued(&s, 0));
  sophia_pc_close(&s);
  assert(s.custody == SOPHIA_PC_UNKNOWN);
  s = accepted();
  sophia_pc_close(&s);
  assert(s.custody == SOPHIA_PC_UNKNOWN);
  for (i = 0; i < 4; ++i) {
    s = accepted();
    assert(!sophia_pc_start_status(&s, 1, &deadline));
    assert(status(&s,
                  i == 0   ? 10
                  : i == 2 ? 0
                           : 9,
                  i == 1   ? 8
                  : i == 2 ? 0
                           : 7,
                  i == 2   ? 0
                  : i == 3 ? 8
                           : 3,
                  1) == SOPHIA_PC_PROTOCOL);
    assert(s.custody == SOPHIA_PC_UNKNOWN);
  }
  s = accepted();
  assert(!sophia_pc_start_status(&s, 1, &deadline));
  assert(sophia_pc_status(&s, request, sizeof request, 1) ==
         SOPHIA_PC_PROTOCOL);
  s = accepted();
  assert(sophia_pc_advance(&s, 0) == SOPHIA_PC_CLOCK_REGRESSION && !s.error);
  for (now = 1; now < 100; ++now) {
    assert(!sophia_pc_start_status(&s, now, &deadline) && deadline == 100);
    assert(status(&s, 9, 7, 2, now - 1) == SOPHIA_PC_CLOCK_REGRESSION);
    assert(!status(&s, 9, 7, 2, now));
  }
  assert(sophia_pc_start_status(&s, 100, &deadline) == SOPHIA_PC_DEADLINE);
  assert(s.custody == SOPHIA_PC_UNKNOWN);
  s = pending();
  assert(sophia_pc_issued(&s, 100) == SOPHIA_PC_DEADLINE);
  assert(s.custody == SOPHIA_PC_DROPPED_UNSENT);
  {
    struct sophia_pf_capture_request r = {1, 1};
    assert(!sophia_pc_init(&s, r, 0, 10000));
    assert(sophia_pc_init(&s, r, 0, 10001) == SOPHIA_PC_DEADLINE);
    assert(!sophia_pc_init(&s, r, UINT64_MAX - 1, UINT64_MAX));
    assert(sophia_pc_init(&s, r, 1, 1) == SOPHIA_PC_DEADLINE);
    r.serial = 0;
    assert(sophia_pc_init(&s, r, 0, 10) == SOPHIA_PC_INVALID);
  }
  return 0;
}
