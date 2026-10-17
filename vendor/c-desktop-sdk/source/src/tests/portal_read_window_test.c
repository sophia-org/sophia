#include "sophia_portal_read_window.h"
#include <assert.h>
int main(void) {
  struct sophia_pr_window w;
  struct sophia_pr_request requests[4], extra;
  unsigned char seen[1003] = {0};
  uint64_t offset;
  size_t n, i, j;
  assert(!sophia_pr_init(&w, 1003, 512, 256, 60000));
  while (!sophia_pr_payload_complete(&w)) {
    n = 0;
    while (n < 4 && sophia_pr_reserve(&w, &requests[n]) == 1)
      ++n;
    assert(n && !sophia_pr_reserve(&w, &extra));
    while (n) {
      struct sophia_pr_request r = requests[--n];
      size_t length = r.count > 1 ? r.count / 2 : 1;
      assert(r.count <= 256 &&
             !sophia_pr_accept(&w, r.token, length, &offset) &&
             offset == r.offset);
      for (j = 0; j < length; ++j) {
        assert(!seen[offset + j]);
        seen[offset + j] = 1;
      }
    }
  }
  for (i = 0; i < sizeof seen; ++i)
    assert(seen[i]);
  assert(w.received == 1003 && !sophia_pr_reserve(&w, &extra));
  assert(sophia_pr_init(&w, 0, 512, 0, 1) == SOPHIA_PR_INVALID);
  assert(sophia_pr_init(&w, 67108865, 512, 0, 1) == SOPHIA_PR_INVALID);
  assert(sophia_pr_init(&w, 1, 511, 0, 1) == SOPHIA_PR_INVALID);
  assert(sophia_pr_init(&w, 1, 65537, 0, 1) == SOPHIA_PR_INVALID);
  assert(sophia_pr_init(&w, 1, 512, 0, 0) == SOPHIA_PR_INVALID);
  for (i = 0; i < 2; ++i) {
    assert(!sophia_pr_init(&w, 1000, 512, 0, UINT32_MAX));
    assert(sophia_pr_reserve(&w, &extra) == 1 && extra.count == 501);
    offset = 99;
    assert(sophia_pr_accept(&w, extra.token, i ? 502 : 0, &offset) ==
               SOPHIA_PR_INVALID &&
           offset == 99);
    assert(sophia_pr_reserve(&w, &extra) == SOPHIA_PR_INVALID &&
           !sophia_pr_payload_complete(&w));
  }
  assert(!sophia_pr_init(&w, 1, 512, 0, 1));
  assert(sophia_pr_reserve(&w, &extra) == 1);
  assert(!sophia_pr_accept(&w, extra.token, 1, &offset));
  assert(sophia_pr_accept(&w, extra.token, 1, &offset) == SOPHIA_PR_INVALID &&
         !sophia_pr_payload_complete(&w));
  assert(!sophia_pr_init(&w, 67108864, 65536, 0, 65536));
  while (sophia_pr_reserve(&w, &extra) == 1) {
    assert(extra.count <= 65525 &&
           !sophia_pr_accept(&w, extra.token, extra.count, &offset));
  }
  assert(sophia_pr_payload_complete(&w));
  sophia_pr_close(&w);
  assert(sophia_pr_reserve(&w, &extra) == SOPHIA_PR_CLOSED);
  return 0;
}
