#include "sophia_portal_snapshot.h"
#include <assert.h>
#include <string.h>

static int reply(struct sophia_ps_fetch *s, unsigned type, const void *bytes,
                 uint32_t n, uint64_t now) {
  struct sophia_ps_operation op;
  struct sophia_9p_reply r = {0};
  int result = sophia_ps_operation(s, now, 4096, &op);
  if (result < 0)
    return result;
  assert(result == 1);
  assert(!sophia_ps_issued(s, now, 4096,
                           op.kind == SOPHIA_PS_WALK ? 4 : SOPHIA_9P_NOFID));
  r.type = (uint8_t)type;
  r.count = n;
  r.iounit = n;
  r.error = n;
  r.data = bytes;
  return sophia_ps_reply(s, &r, now);
}
static void open_file(struct sophia_ps_fetch *s, uint32_t iounit) {
  assert(!reply(s, 111, NULL, 1, 1));
  assert(!reply(s, 13, NULL, iounit, 1));
}
int main(void) {
  const char *records[] = {"sophia-portal-client version=1 epoch=1 "
                           "request=unavailable fd_transfer=none\n",
                           "\1", "request=unavailable transfer=ready\n"};
  const unsigned char idle[32] = {1};
  const size_t lengths[] = {strlen(records[0]), 32, strlen(records[2])};
  unsigned char storage[SOPHIA_PF_API_MAX_BYTES], over[43];
  records[1] = (const char *)idle;
  struct sophia_ps_fetch fetch;
  struct sophia_ps_operation op;
  struct sophia_9p_reply r = {0};
  const void *result;
  size_t length, i, offset;
  for (i = 0; i < 3; ++i) {
    assert(!sophia_ps_init(&fetch, (enum sophia_ps_file)(i + 1), storage,
                           sizeof storage, 0, 100));
    open_file(&fetch, 3);
    for (offset = 0; offset < lengths[i];) {
      size_t n = lengths[i] - offset;
      if (n > 3)
        n = 3;
      assert(sophia_ps_operation(&fetch, 1, 4096, &op) == 1);
      assert(op.kind == SOPHIA_PS_READ && op.fid == 4 && op.offset == offset &&
             op.count == 3);
      assert(!reply(&fetch, 117, records[i] + offset, (uint32_t)n, 1));
      assert(!sophia_ps_result(&fetch, &result, &length));
      offset += n;
    }
    assert(!reply(&fetch, 117, NULL, 0, 1));
    assert(!sophia_ps_result(&fetch, &result, &length));
    assert(sophia_ps_operation(&fetch, 1, 4096, &op) == 1 &&
           op.kind == SOPHIA_PS_CLUNK);
    assert(!reply(&fetch, 121, NULL, 0, 1));
    assert(sophia_ps_result(&fetch, &result, &length) == 1);
    assert(result == storage && length == lengths[i] &&
           !memcmp(result, records[i], length));
    assert(sophia_ps_operation(&fetch, 1000, 4096, &op) == 0);
  }
  assert(!sophia_ps_init(&fetch, SOPHIA_PS_STATUS, storage, sizeof storage, 0,
                         100));
  open_file(&fetch, 0);
  assert(sophia_ps_operation(&fetch, 1, 20, &op) == 1 && op.count == 9);
  memset(over, 'a', sizeof over);
  assert(!reply(&fetch, 117, over, sizeof over, 1));
  assert(sophia_ps_operation(&fetch, 1, 4096, &op) == 1 && op.count == 1 &&
         op.offset == 43);
  assert(reply(&fetch, 117, over, 1, 1) == SOPHIA_PS_INVALID);
  assert(sophia_ps_operation(&fetch, 2, 4096, &op) == SOPHIA_PS_INVALID);
  assert(
      !sophia_ps_init(&fetch, SOPHIA_PS_API, storage, sizeof storage, 0, 100));
  open_file(&fetch, 0);
  assert(reply(&fetch, 117, NULL, 0, 1) == SOPHIA_PS_INVALID);
  assert(!sophia_ps_init(&fetch, SOPHIA_PS_STATUS, storage, sizeof storage, 0,
                         100));
  open_file(&fetch, 0);
  assert(!reply(&fetch, 117, "request=unavailable transfer=ready\n",
                sizeof("request=unavailable transfer=ready\n") - 1, 1));
  assert(!reply(&fetch, 117, NULL, 0, 1));
  assert(reply(&fetch, 7, NULL, 5, 1) == SOPHIA_PS_REMOTE &&
         fetch.remote_error == 5);
  assert(!sophia_ps_result(&fetch, &result, &length));
  assert(
      !sophia_ps_init(&fetch, SOPHIA_PS_API, storage, sizeof storage, 10, 20));
  assert(sophia_ps_operation(&fetch, 9, 4096, &op) ==
         SOPHIA_PS_CLOCK_REGRESSION);
  assert(!sophia_ps_issued(&fetch, 10, 4096, 4));
  assert(sophia_ps_operation(&fetch, 10, 4096, &op) == 0);
  r.type = 111;
  r.count = 1;
  assert(sophia_ps_reply(&fetch, &r, 9) == SOPHIA_PS_CLOCK_REGRESSION &&
         !fetch.error);
  assert(sophia_ps_reply(&fetch, &r, 20) == SOPHIA_PS_DEADLINE);
  sophia_ps_close(&fetch);
  assert(fetch.error == SOPHIA_PS_DEADLINE);
  assert(
      !sophia_ps_init(&fetch, SOPHIA_PS_API, storage, sizeof storage, 0, 100));
  assert(reply(&fetch, 7, NULL, 11, 1) == SOPHIA_PS_REMOTE);
  assert(sophia_ps_operation(&fetch, 2, 4096, &op) == SOPHIA_PS_REMOTE);
  assert(
      !sophia_ps_init(&fetch, SOPHIA_PS_API, storage, sizeof storage, 0, 100));
  open_file(&fetch, 1);
  assert(reply(&fetch, 117, over, 2, 1) == SOPHIA_PS_INVALID);
  return 0;
}
