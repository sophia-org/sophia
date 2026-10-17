#include "sophia_admin_snapshot.h"
#include <assert.h>
#include <string.h>

static int reply(struct sophia_as_fetch *s, unsigned type, const void *bytes, uint32_t n, uint64_t now) {
  struct sophia_as_operation op;
  struct sophia_9p_reply r = {0};
  int result = sophia_as_operation(s, now, 4096, &op);
  if (result < 0) return result;
  assert(result == 1);
  assert(!sophia_as_issued(s, now, 4096, op.kind == SOPHIA_AS_WALK ? 4 : SOPHIA_9P_NOFID));
  r.type = (uint8_t)type; r.count = n; r.iounit = n; r.error = n; r.data = bytes;
  return sophia_as_reply(s, &r, now);
}
static void open_file(struct sophia_as_fetch *s, uint32_t iounit) {
  assert(!reply(s, 111, NULL, 1, 1));
  assert(!reply(s, 13, NULL, iounit, 1));
}
int main(void) {
  const char *records[] = {"v1 info 0 1 7 10000 2000 60000\n",
    "v1 catalog 1 1\nv1 command policy 8:next tab\n", "v1 pending 7 9 12\n"};
  unsigned char storage[SOPHIA_AF_CATALOG_MAX_BYTES], over[512];
  struct sophia_as_fetch fetch;
  struct sophia_as_operation op;
  struct sophia_9p_reply r = {0};
  const void *result;
  size_t length, i, offset;
  for (i = 0; i < 3; ++i) {
    assert(!sophia_as_init(&fetch, (enum sophia_as_file)(i+1), storage, sizeof storage, 0, 100));
    open_file(&fetch, 3);
    for (offset = 0; offset < strlen(records[i]);) {
      size_t n = strlen(records[i]) - offset;
      if (n > 3) n = 3;
      assert(sophia_as_operation(&fetch, 1, 4096, &op) == 1);
      assert(op.kind == SOPHIA_AS_READ && op.fid == 4 && op.offset == offset && op.count == 3);
      assert(!reply(&fetch, 117, records[i]+offset, (uint32_t)n, 1));
      assert(!sophia_as_result(&fetch, &result, &length)); offset += n;
    }
    assert(!reply(&fetch, 117, NULL, 0, 1));
    assert(!sophia_as_result(&fetch, &result, &length));
    assert(sophia_as_operation(&fetch, 1, 4096, &op) == 1 && op.kind == SOPHIA_AS_CLUNK);
    assert(!reply(&fetch, 121, NULL, 0, 1));
    assert(sophia_as_result(&fetch, &result, &length) == 1);
    assert(result == storage && length == strlen(records[i]) && !memcmp(result, records[i], length));
    assert(sophia_as_operation(&fetch, 1000, 4096, &op) == 0);
  }
  assert(!sophia_as_init(&fetch, SOPHIA_AS_STATUS, storage, sizeof storage, 0, 100));
  open_file(&fetch, 0);
  assert(sophia_as_operation(&fetch, 1, 20, &op) == 1 && op.count == 9);
  memset(over, 'a', sizeof over);
  assert(!reply(&fetch, 117, over, sizeof over, 1));
  assert(sophia_as_operation(&fetch, 1, 4096, &op) == 1 && op.count == 1 && op.offset == 512);
  assert(reply(&fetch, 117, over, 1, 1) == SOPHIA_AS_INVALID);
  assert(sophia_as_operation(&fetch, 2, 4096, &op) == SOPHIA_AS_INVALID);
  assert(!sophia_as_init(&fetch, SOPHIA_AS_INFO, storage, sizeof storage, 0, 100));
  open_file(&fetch, 0);
  assert(reply(&fetch, 117, NULL, 0, 1) == SOPHIA_AS_INVALID);
  assert(!sophia_as_init(&fetch, SOPHIA_AS_STATUS, storage, sizeof storage, 0, 100));
  open_file(&fetch, 0);
  assert(!reply(&fetch, 117, "v1 ready 7\n", 11, 1));
  assert(!reply(&fetch, 117, NULL, 0, 1));
  assert(reply(&fetch, 7, NULL, 5, 1) == SOPHIA_AS_REMOTE && fetch.remote_error == 5);
  assert(!sophia_as_result(&fetch, &result, &length));
  assert(!sophia_as_init(&fetch, SOPHIA_AS_INFO, storage, sizeof storage, 10, 20));
  assert(sophia_as_operation(&fetch, 9, 4096, &op) == SOPHIA_AS_CLOCK_REGRESSION);
  assert(!sophia_as_issued(&fetch, 10, 4096, 4));
  assert(sophia_as_operation(&fetch, 10, 4096, &op) == 0);
  r.type = 111; r.count = 1;
  assert(sophia_as_reply(&fetch, &r, 9) == SOPHIA_AS_CLOCK_REGRESSION && !fetch.error);
  assert(sophia_as_reply(&fetch, &r, 20) == SOPHIA_AS_DEADLINE);
  sophia_as_close(&fetch);
  assert(fetch.error == SOPHIA_AS_DEADLINE);
  assert(!sophia_as_init(&fetch, SOPHIA_AS_INFO, storage, sizeof storage, 0, 100));
  assert(reply(&fetch, 7, NULL, 11, 1) == SOPHIA_AS_REMOTE);
  assert(sophia_as_operation(&fetch, 2, 4096, &op) == SOPHIA_AS_REMOTE);
  assert(!sophia_as_init(&fetch, SOPHIA_AS_INFO, storage, sizeof storage, 0, 100));
  open_file(&fetch, 1);
  assert(reply(&fetch, 117, over, 2, 1) == SOPHIA_AS_INVALID);
  return 0;
}
