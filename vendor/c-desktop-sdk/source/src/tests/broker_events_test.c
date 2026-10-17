#include "sophia_broker_files.h"
#include <assert.h>
#include <string.h>
static void record(unsigned char *b, unsigned kind, size_t n) {
  memset(b, 0, n);
  b[0] = (unsigned char)n;
  b[4] = 1;
  b[6] = (unsigned char)kind;
  b[8] = 7;
  b[24] = 1;
}
int main(void) {
  unsigned char b[200], bad[200];
  struct sophia_bf_event e, sentinel;
  unsigned kind;
  size_t n, i;
  memset(&sentinel, 0xa5, sizeof sentinel);
  for (kind = 1; kind <= 5; ++kind) {
    record(b, 32, 200);
    b[32] = 1;
    b[40] = (unsigned char)kind;
    b[56] = kind == 4 ? 0 : 1;
    assert(!sophia_bf_decode_event(b, 200, 7, &e));
    assert(!e.request.surface_index && !e.request.surface_generation &&
           !e.request.candidate_generation && !e.request.label_present);
    for (n = 0; n < 200; ++n) {
      e = sentinel;
      assert(sophia_bf_decode_event(b, n, 7, &e));
      assert(!memcmp(&e, &sentinel, sizeof e));
    }
    if (kind != 2) {
      b[59] = 1;
      assert(sophia_bf_decode_event(b, 200, 7, &e));
    }
  }
  record(b, 32, 200);
  b[32] = 1;
  b[40] = 2;
  b[56] = 3;
  b[59] = 1;
  {
    const unsigned char labels[][4] = {
        {0}, {0, 10}, {0xc3, 0xa9}, {0xf0, 0x9f, 0x98, 0x80}};
    const size_t lengths[] = {0, 2, 2, 4};
    for (i = 0; i < 4; ++i) {
      memset(b + 72, 0, 128);
      memcpy(b + 72, labels[i], lengths[i]);
      b[60] = (unsigned char)lengths[i];
      assert(!sophia_bf_decode_event(b, 200, 7, &e));
      assert(e.request.label_present &&
             e.request.label == (const char *)b + 72 &&
             e.request.label_length == lengths[i]);
    }
  }
  b[60] = 128;
  memset(b + 72, 'x', 128);
  assert(!sophia_bf_decode_event(b, 200, 7, &e));
  {
    const unsigned offsets[] = {16, 42, 44, 62, 58, 59, 60, 72};
    const unsigned values[] = {1, 1, 1, 1, 2, 2, 129, 255};
    for (i = 0; i < 8; ++i) {
      memcpy(bad, b, 200);
      bad[offsets[i]] = (unsigned char)values[i];
      e = sentinel;
      assert(sophia_bf_decode_event(bad, 200, 7, &e));
      assert(!memcmp(&e, &sentinel, sizeof e));
    }
  }
  b[59] = 0;
  assert(sophia_bf_decode_event(b, 200, 7, &e));
  b[60] = 0;
  memset(b + 72, 0, 128);
  b[58] = 1;
  assert(sophia_bf_decode_event(b, 200, 7, &e));
  for (kind = 16; kind <= 18; ++kind) {
    size_t length = kind == 16 ? 56 : kind == 17 ? 40 : 48;
    record(b, kind, length);
    b[32] = 1;
    if (kind == 16) {
      b[32] = 2;
      b[36] = 1;
      b[40] = 7;
      b[48] = 128;
      b[50] = 2;
    }
    if (kind == 18)
      b[41] = 1;
    assert(!sophia_bf_decode_event(b, length, 7, &e));
    assert(sophia_bf_decode_event(b, length, 8, &e));
    for (n = 0; n < length; ++n)
      assert(sophia_bf_decode_event(b, n, 7, &e));
    b[length - 1] = 1;
    assert(sophia_bf_decode_event(b, length, 7, &e));
  }
  return 0;
}
