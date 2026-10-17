#include "sophia_portal_requester_upload.h"
#include <assert.h>
#include <string.h>
static int reply(struct sophia_pqu *u, unsigned type, uint32_t count,
                 uint32_t unit, uint32_t error, uint64_t now) {
  unsigned char qids[26] = {0};
  struct sophia_9p_reply r = {0};
  qids[0] = 128;
  r.type = (uint8_t)type;
  r.count = count;
  r.iounit = unit;
  r.error = error;
  r.data = qids;
  return sophia_pqu_reply(u, &r, now);
}
static void open_writer(struct sophia_pqu *u, uint32_t unit) {
  struct sophia_pqu_operation op;
  assert(!sophia_pqu_operation(u, 1, 512, &op) && op.kind == SOPHIA_PQU_WALK);
  assert(!sophia_pqu_issued(u, 1, 512, 4));
  assert(!reply(u, 111, 2, 0, 0, 1));
  assert(!sophia_pqu_issued(u, 1, 512, SOPHIA_9P_NOFID));
  assert(!reply(u, 13, 0, unit, 0, 1));
}
int main(void) {
  unsigned char bytes[65536];
  size_t sizes[] = {0, 1, 65536}, i, j, offset;
  uint32_t units[] = {0, 7, 1024};
  unsigned fault;
  struct sophia_pqu u;
  struct sophia_pqu_operation op;
  memset(bytes, 17, sizeof bytes);
  for (i = 0; i < 3; ++i)
    for (j = 0; j < 3; ++j) {
      assert(!sophia_pqu_init(&u, bytes, sizes[i], 65536, 0, 100));
      open_writer(&u, units[j]);
      offset = 0;
      while (!sophia_pqu_held(&u)) {
        size_t count;
        assert(!sophia_pqu_operation(&u, 1, 512, &op) &&
               op.kind == SOPHIA_PQU_WRITE);
        assert(op.fid == 4 && op.offset == offset &&
               op.bytes == bytes + offset);
        assert(op.length <= 489 && (!units[j] || op.length <= units[j]));
        count = op.length;
        assert(!sophia_pqu_issued(&u, 1, 512, SOPHIA_9P_NOFID));
        assert(!sophia_pqu_operation(&u, 1, 512, &op) &&
               op.kind == SOPHIA_PQU_NONE);
        assert(!reply(&u, 119, (uint32_t)count, 0, 0, 1));
        offset += count;
      }
      assert(u.written == sizes[i] && !sophia_pqu_done(&u));
      assert(!sophia_pqu_operation(&u, 2, 512, &op) &&
             op.kind == SOPHIA_PQU_NONE);
      assert(!sophia_pqu_release(&u, 2));
      assert(!sophia_pqu_operation(&u, 2, 512, &op) &&
             op.kind == SOPHIA_PQU_CLUNK && op.fid == 4);
      assert(!sophia_pqu_issued(&u, 2, 512, SOPHIA_9P_NOFID));
      assert(!reply(&u, 121, 0, 0, 0, 2));
      assert(sophia_pqu_done(&u) && u.bytes == bytes);
    }
  for (fault = 0; fault < 4; ++fault) {
    int e;
    assert(!sophia_pqu_init(&u, bytes, 9, 65536, 0, 100));
    open_writer(&u, 7);
    assert(!sophia_pqu_issued(&u, 1, 512, SOPHIA_9P_NOFID));
    if (fault == 0)
      e = reply(&u, 119, 6, 0, 0, 2);
    else if (fault == 1)
      e = reply(&u, 7, 0, 0, 116, 2);
    else if (fault == 2)
      e = reply(&u, 119, 7, 0, 0, 100);
    else {
      sophia_pqu_close(&u);
      e = reply(&u, 119, 7, 0, 0, 2);
    }
    assert(e && !u.written && !sophia_pqu_held(&u) && !sophia_pqu_done(&u));
    assert(sophia_pqu_operation(&u, 100, 512, &op));
  }
  assert(!sophia_pqu_init(&u, NULL, 0, 65536, 0, 100));
  assert(!sophia_pqu_issued(&u, 1, 512, 4));
  assert(reply(&u, 111, 1, 0, 0, 1) == SOPHIA_PQS_PROTOCOL);
  assert(!sophia_pqu_init(&u, NULL, 0, 65536, 0, 100));
  open_writer(&u, 0);
  assert(!sophia_pqu_release(&u, 2));
  assert(!sophia_pqu_issued(&u, 2, 512, SOPHIA_9P_NOFID));
  assert(reply(&u, 7, 0, 0, 5, 3) == SOPHIA_PQS_REMOTE && !sophia_pqu_done(&u));
  assert(sophia_pqu_init(&u, &u, 1, 65536, 0, 100) == SOPHIA_PQS_INVALID);
  for (fault = 0; fault < 2; ++fault) {
    assert(!sophia_pqu_init(&u, bytes, 32, 65536, 0, 100));
    open_writer(&u, 7);
    assert(!sophia_pqu_issued(&u, 1, 512, SOPHIA_9P_NOFID));
    assert(!sophia_pqu_settle(&u, 2));
    assert(!sophia_pqu_operation(&u, 2, 512, &op) &&
           op.kind == SOPHIA_PQU_NONE);
    assert(!reply(&u, fault ? 7 : 119, fault ? 0 : 7, 0, fault ? 116 : 0, 3));
    assert(u.written == (fault ? 0 : 7));
    assert(!sophia_pqu_operation(&u, 3, 512, &op) &&
           op.kind == SOPHIA_PQU_CLUNK);
    assert(!sophia_pqu_issued(&u, 3, 512, SOPHIA_9P_NOFID));
    assert(!reply(&u, 121, 0, 0, 0, 4));
    assert(sophia_pqu_done(&u));
  }
  assert(!sophia_pqu_init(&u, bytes, 32, 65536, 0, 100));
  assert(!sophia_pqu_settle(&u, 1));
  assert(sophia_pqu_done(&u));
  assert(!sophia_pqu_init(&u, bytes, 32, 65536, 0, 100));
  assert(!sophia_pqu_issued(&u, 1, 512, 4));
  assert(!sophia_pqu_settle(&u, 2));
  assert(!reply(&u, 7, 0, 0, 116, 3));
  assert(sophia_pqu_done(&u));
  return 0;
}
