#include "sophia_admin_command.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static struct sophia_ac pending(enum sophia_af_owner owner, const char *name, int accept) {
  const char *wire = "v1 catalog 12 4\nv1 command policy 8:next tab\nv1 command session 6:logout\nv1 command session 14:reload-profile\nv1 command session 10:restart-wm\n";
  struct sophia_af_info info = {1,0,7,10,2,100};
  struct sophia_af_catalog catalog;
  struct sophia_af_command command;
  struct sophia_ac s;
  assert(!sophia_af_decode_catalog(wire, strlen(wire), &catalog));
  command.owner = owner; command.name = name; command.name_length = strlen(name);
  assert(!sophia_ac_init(&s, &info, &catalog, &command, 9, 0));
  if (accept) { assert(!sophia_ac_issued(&s, 0)); assert(!sophia_ac_write_reply(&s, 0, (uint32_t)s.length, 1)); }
  return s;
}
static int status(struct sophia_ac *s, const char *wire, uint64_t now) {
  return sophia_ac_status(s, wire, strlen(wire), now);
}
int main(void) {
  struct sophia_ac s = pending(SOPHIA_AF_POLICY, "next tab", 0);
  const char *wire = "v1 invoke 7 9 12 policy 8:next tab\n";
  const char *outcomes[] = {"committed", "completed", "unchanged", "rejected", "stale", "denied", "unavailable", "overloaded", "timed-out", "indeterminate"};
  const char *names[] = {"next tab", "logout", "reload-profile"};
  const char *bad[] = {"v1 ready 7\n", "v1 pending 8 9 12\n", "v1 pending 7 10 12\n", "v1 pending 7 9 13\n", "bad\n"};
  uint64_t time;
  size_t i, j;
  char result[128];
  assert(s.length == strlen(wire) && !memcmp(s.bytes, wire, s.length));
  assert(sophia_ac_start_status(&s, 0, &time) == SOPHIA_AC_BUSY);
  assert(!sophia_ac_issued(&s, 0) && sophia_ac_issued(&s, 0) == SOPHIA_AC_BUSY);
  assert(!sophia_ac_write_reply(&s, 0, (uint32_t)s.length, 1));
  assert(sophia_ac_next_wake(&s, &time) && time == 1);
  assert(!sophia_ac_start_status(&s, 1, &time) && time == 3);
  assert(sophia_ac_start_status(&s, 1, &time) == SOPHIA_AC_BUSY);
  assert(!status(&s, "v1 pending 7 9 12\n", 1));
  assert(sophia_ac_next_wake(&s, &time) && time == 2);
  assert(sophia_ac_start_status(&s, 1, &time) == SOPHIA_AC_BUSY);
  assert(!sophia_ac_start_status(&s, 2, &time) && time == 4);
  assert(!status(&s, "v1 outcome 7 9 12 committed 2:ok\n", 2));
  assert(s.custody == SOPHIA_AC_SETTLED && s.outcome == SOPHIA_AF_COMMITTED && s.detail_length == 2 && !memcmp(s.detail, "ok", 2));
  assert(!sophia_ac_next_wake(&s, &time) && !sophia_ac_invalidate_catalog(&s));
  sophia_ac_close(&s); assert(s.custody == SOPHIA_AC_SETTLED && s.outcome == SOPHIA_AF_COMMITTED);
  s = pending(SOPHIA_AF_POLICY, "next tab", 0); sophia_ac_close(&s); assert(s.custody == SOPHIA_AC_DROPPED_UNSENT);
  s = pending(SOPHIA_AF_POLICY, "next tab", 0); assert(!sophia_ac_issued(&s, 0)); sophia_ac_close(&s); assert(s.custody == SOPHIA_AC_UNKNOWN);
  s = pending(SOPHIA_AF_POLICY, "next tab", 1); sophia_ac_close(&s); assert(s.custody == SOPHIA_AC_UNKNOWN);
  s = pending(SOPHIA_AF_POLICY, "next tab", 0); assert(!sophia_ac_issued(&s, 0));
  assert(sophia_ac_write_reply(&s, 1, 11, 1) == SOPHIA_AC_REFUSAL && s.custody == SOPHIA_AC_REFUSED);
  sophia_ac_close(&s); assert(s.custody == SOPHIA_AC_REFUSED && s.remote_error == 11);
  s = pending(SOPHIA_AF_POLICY, "next tab", 0); assert(!sophia_ac_issued(&s, 0));
  assert(sophia_ac_write_reply(&s, 0, 1, 1) == SOPHIA_AC_PROTOCOL && s.custody == SOPHIA_AC_UNKNOWN);
  s = pending(SOPHIA_AF_POLICY, "next tab", 0);
  assert(sophia_ac_advance(&s, 12) == SOPHIA_AC_DEADLINE && s.custody == SOPHIA_AC_DROPPED_UNSENT);
  s = pending(SOPHIA_AF_POLICY, "next tab", 0); assert(!sophia_ac_issued(&s, 0));
  assert(sophia_ac_advance(&s, 2) == SOPHIA_AC_DEADLINE && s.custody == SOPHIA_AC_UNKNOWN);
  s = pending(SOPHIA_AF_POLICY, "next tab", 1); assert(!sophia_ac_start_status(&s, 1, &time));
  assert(status(&s, "v1 pending 7 9 12\n", 3) == SOPHIA_AC_DEADLINE);
  s = pending(SOPHIA_AF_POLICY, "next tab", 1);
  assert(sophia_ac_advance(&s, 0) == SOPHIA_AC_CLOCK_REGRESSION && !s.error);
  for (i = 1; i < 12; ++i) { assert(!sophia_ac_start_status(&s, i, &time)); assert(!status(&s, "v1 pending 7 9 12\n", i)); }
  assert(sophia_ac_start_status(&s, 12, &time) == SOPHIA_AC_DEADLINE);
  for (i = 0; i < 3; ++i) for (j = 0; j < 10; ++j) {
    enum sophia_af_owner owner = i ? SOPHIA_AF_SESSION : SOPHIA_AF_POLICY;
    int valid = j == 0 ? !i : j == 1 ? i != 0 : j == 2 ? i == 2 : 1;
    s = pending(owner, names[i], 1); assert(!sophia_ac_start_status(&s, 1, &time));
    (void)snprintf(result, sizeof result, "v1 outcome 7 9 12 %s 0:\n", outcomes[j]);
    assert((status(&s, result, 1) == 0) == valid);
    assert(s.custody == (valid ? SOPHIA_AC_SETTLED : SOPHIA_AC_UNKNOWN));
    if (valid) assert(!!sophia_ac_invalidate_catalog(&s) == (i != 0 || j == 4 || j == 5 || j == 9));
  }
  for (i = 0; i < sizeof bad / sizeof bad[0]; ++i) {
    s = pending(SOPHIA_AF_POLICY, "next tab", 1); assert(!sophia_ac_start_status(&s, 1, &time));
    assert(status(&s, bad[i], 1) == SOPHIA_AC_PROTOCOL && s.custody == SOPHIA_AC_UNKNOWN);
  }
  return 0;
}
