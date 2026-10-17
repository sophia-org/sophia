#include "sophia_admin_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  const char *names[] = {"committed", "completed", "unchanged", "rejected", "stale",
    "denied", "unavailable", "overloaded", "timed-out", "indeterminate"};
  const char *bad[] = {"v1 ready 0\n", "v1 ready 7 \n", "v1 pending 7 9 0\n",
    "v1 pending 7 9 12 \n", "v1 outcome 7 9 12 unknown 0:\n",
    "v1 outcome 7 9 12 committed 0\n", "v1 outcome 7 9 12 committed 00:\n",
    "v1 outcome 7 9 12 committed 1:\xc3\xa9\n",
    "v1 outcome 7 9 12 committed 2:\xc2\x85\n",
    "v1 outcome 7 9 12 committed 1:\t\n", "v1 pending 7 09 12\n",
    "v1 outcome 7 9 12 committed 2:\xc0\xaf\n",
    "v1 outcome 7 9 12 committed 3:\xed\xa0\x80\n",
    "v1 outcome 7 9 12 committed 4:\xf4\x90\x80\x80\n"};
  char wire[512], detail[258];
  struct sophia_af_status status;
  size_t i, end;
  const char *ready = "v1 ready 7\n", *pending = "v1 pending 7 9 12\n";
  assert(!sophia_af_decode_status(ready, strlen(ready), &status));
  assert(status.kind == SOPHIA_AF_READY && status.connection == 7 && !status.request && !status.detail);
  assert(!sophia_af_decode_status(pending, strlen(pending), &status));
  assert(status.kind == SOPHIA_AF_PENDING && status.request == 9 && status.generation == 12);
  for (i = 0; i < sizeof names / sizeof names[0]; ++i) {
    int n = snprintf(wire, sizeof wire, "v1 outcome 7 9 12 %s 3:\xc3\xa9!\n", names[i]);
    assert(n > 0 && !sophia_af_decode_status(wire, (size_t)n, &status));
    assert(status.kind == SOPHIA_AF_OUTCOME && status.outcome == (enum sophia_af_outcome)(i+1));
    assert(status.connection == 7 && status.request == 9 && status.generation == 12);
    assert(status.detail == wire + n - 4 && status.detail_length == 3);
    for (end = 0; end < (size_t)n; ++end) assert(sophia_af_decode_status(wire, end, &status) == -1);
  }
  for (i = 0; i < sizeof bad / sizeof bad[0]; ++i)
    assert(sophia_af_decode_status(bad[i], strlen(bad[i]), &status) == -1);
  assert(sophia_af_decode_status("v1 outcome 7 9 12 committed 1:\0\n",
      sizeof "v1 outcome 7 9 12 committed 1:\0\n" - 1, &status) == -1);
  for (i = 256; i <= 257; ++i) {
    int n;
    memset(detail, 'a', i); detail[i] = 0;
    n = snprintf(wire, sizeof wire, "v1 outcome 7 9 12 rejected %zu:%s\n", i, detail);
    assert((sophia_af_decode_status(wire, (size_t)n, &status) == 0) == (i == 256));
  }
  assert(!sophia_af_decode_status("v1 outcome 7 9 12 committed 0:\n",
      sizeof "v1 outcome 7 9 12 committed 0:\n" - 1, &status));
  assert(status.detail_length == 0);
  assert(sophia_af_decode_status(NULL, 0, &status) == -1);
  assert(sophia_af_decode_status(ready, strlen(ready), NULL) == -1);
  return 0;
}
