#include "sophia_broker_files.h"
#include <assert.h>
#include <string.h>
int main(void) {
  unsigned char out[48], expected[40], sentinel[48];
  const uint16_t ranges[][2] = {{2, 2}, {0, 0}, {3, 1}, {0, 65535}};
  const uint32_t lengths[] = {40, 41, 448};
  unsigned i;
  size_t n;
  memset(sentinel, 0xa5, sizeof sentinel);
  for (i = 0; i < 4; ++i) {
    memcpy(out, sentinel, sizeof out);
    memset(expected, 0, sizeof expected);
    expected[0] = 40;
    expected[4] = 1;
    expected[7] = 1;
    expected[8] = 7;
    expected[16] = 9;
    expected[32] = (unsigned char)ranges[i][0];
    expected[33] = (unsigned char)(ranges[i][0] >> 8);
    expected[34] = (unsigned char)ranges[i][1];
    expected[35] = (unsigned char)(ranges[i][1] >> 8);
    assert(!sophia_bf_encode_negotiate(7, 9, ranges[i][0], ranges[i][1], out,
                                       sizeof out));
    assert(!memcmp(out, expected, 40) && !memcmp(out + 40, sentinel + 40, 8));
  }
  for (i = 0; i < 3; ++i) {
    memcpy(out, sentinel, sizeof out);
    memset(expected, 0, sizeof expected);
    expected[0] = 7;
    expected[8] = 9;
    expected[16] = (unsigned char)lengths[i];
    expected[17] = (unsigned char)(lengths[i] >> 8);
    assert(!sophia_bf_encode_submit(7, 9, lengths[i], out, sizeof out));
    assert(!memcmp(out, expected, 24) && !memcmp(out + 24, sentinel + 24, 24));
  }
  memcpy(out, sentinel, sizeof out);
  memset(expected, 0, sizeof expected);
  memset(expected, 255, 8);
  expected[8] = 1;
  assert(!sophia_bf_encode_ack(UINT64_MAX, 1, out, sizeof out));
  assert(!memcmp(out, expected, 16) && !memcmp(out + 16, sentinel + 16, 32));
  memcpy(out, sentinel, sizeof out);
  for (i = 0; i < 2; ++i) {
    assert(sophia_bf_encode_negotiate(i, 1 - i, 2, 2, out, sizeof out));
    assert(sophia_bf_encode_submit(i, 1 - i, 40, out, sizeof out));
    assert(sophia_bf_encode_ack(i, 1 - i, out, sizeof out));
  }
  for (n = 0; n < 40; ++n)
    assert(sophia_bf_encode_negotiate(1, 1, 2, 2, out, n));
  for (n = 0; n < 24; ++n)
    assert(sophia_bf_encode_submit(1, 1, 40, out, n));
  for (n = 0; n < 16; ++n)
    assert(sophia_bf_encode_ack(1, 1, out, n));
  assert(sophia_bf_encode_submit(1, 1, 39, out, sizeof out));
  assert(sophia_bf_encode_submit(1, 1, 449, out, sizeof out));
  assert(!memcmp(out, sentinel, sizeof out));
  assert(sophia_bf_encode_negotiate(1, 1, 2, 2, NULL, 40));
  assert(sophia_bf_encode_submit(1, 1, 40, NULL, 24));
  assert(sophia_bf_encode_ack(1, 1, NULL, 16));
  return 0;
}
