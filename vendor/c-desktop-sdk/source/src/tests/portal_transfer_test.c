#include "sophia_portal_snapshot.h"
#include <assert.h>
#include <string.h>

static int reply(struct sophia_ps_fetch *s, unsigned type, const void *data,
                 uint32_t n) {
  struct sophia_ps_operation op;
  struct sophia_9p_reply r = {0};
  assert(sophia_ps_operation(s, 1, 4096, &op) == 1);
  assert(!sophia_ps_issued(s, 1, 4096,
                           op.kind == SOPHIA_PS_WALK ? 4 : SOPHIA_9P_NOFID));
  r.type = (uint8_t)type;
  r.data = data;
  r.count = n;
  r.iounit = n;
  r.error = n;
  return sophia_ps_reply(s, &r, 1);
}
static void open_file(struct sophia_ps_fetch *s, unsigned char *storage,
                      size_t capacity, uint32_t unit) {
  struct sophia_ps_operation op;
  assert(!sophia_ps_init(s, SOPHIA_PS_TRANSFER, storage, capacity, 0, 100));
  assert(sophia_ps_operation(s, 1, 4096, &op) == 1 &&
         op.kind == SOPHIA_PS_WALK);
  assert(!strcmp(sophia_ps_name(op.file), "transfer"));
  assert(!reply(s, 111, NULL, 1));
  assert(!reply(s, 13, NULL, unit));
}
int main(void) {
  unsigned char storage[SOPHIA_PS_TRANSFER_MAX_BYTES + 8],
      payload[SOPHIA_PS_TRANSFER_MAX_BYTES];
  const size_t lengths[] = {0, 1, SOPHIA_PS_TRANSFER_MAX_BYTES};
  struct sophia_ps_fetch s;
  struct sophia_ps_operation op;
  const void *result;
  size_t length, i, j, offset, n;
  for (i = 0; i < sizeof payload; ++i)
    payload[i] = (unsigned char)i;
  for (i = 0; i < sizeof lengths / sizeof lengths[0]; ++i) {
    memset(storage, 0xa5, sizeof storage);
    open_file(&s, storage, sizeof storage, 33);
    for (offset = 0; offset < lengths[i]; offset += n) {
      n = lengths[i] - offset;
      if (n > 17)
        n = 17;
      assert(sophia_ps_operation(&s, 1, 4096, &op) == 1 &&
             op.kind == SOPHIA_PS_READ && op.offset == offset &&
             op.count >= n && op.count <= 33);
      assert(!reply(&s, 117, payload + offset, (uint32_t)n));
      assert(!sophia_ps_result(&s, &result, &length));
    }
    if (lengths[i] == sizeof payload)
      assert(sophia_ps_operation(&s, 1, 4096, &op) == 1 && op.count == 1 &&
             op.offset == sizeof payload);
    assert(!reply(&s, 117, NULL, 0));
    assert(!sophia_ps_result(&s, &result, &length));
    assert(!reply(&s, 121, NULL, 0));
    assert(sophia_ps_result(&s, &result, &length) && result == storage &&
           length == lengths[i]);
    assert(!memcmp(result, payload, length));
    for (j = length; j < sizeof storage; ++j)
      assert(storage[j] == 0xa5);
  }
  assert(sophia_ps_init(&s, SOPHIA_PS_TRANSFER, storage, sizeof payload - 1, 0,
                        100) == SOPHIA_PS_INVALID);
  for (i = 0; i < 3; ++i) {
    open_file(&s, storage, sizeof storage, 0);
    if (!i) {
      for (offset = 0; offset < sizeof payload; offset += n) {
        n = sizeof payload - offset;
        if (n > 4000)
          n = 4000;
        assert(!reply(&s, 117, payload + offset, (uint32_t)n));
      }
      assert(reply(&s, 117, payload, 1) == SOPHIA_PS_INVALID);
    } else {
      assert(!reply(&s, 117, payload, 3));
      if (i == 2)
        assert(!reply(&s, 117, NULL, 0));
      assert(reply(&s, 7, NULL, 116) == SOPHIA_PS_REMOTE &&
             s.remote_error == 116);
    }
    assert(!sophia_ps_result(&s, &result, &length));
    assert(sophia_ps_operation(&s, 2, 4096, &op) < 0);
  }
  return 0;
}
