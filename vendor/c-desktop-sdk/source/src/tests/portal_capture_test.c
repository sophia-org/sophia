#include "sophia_portal_files.h"
#include <assert.h>
#include <string.h>
int main(void) {
  const unsigned char expected[24] = {1,   0,   0,   0,   0,   0,   0,   0,
                                      8,   7,   6,   5,   4,   3,   2,   1,
                                      255, 255, 255, 255, 255, 255, 255, 255};
  struct sophia_pf_capture_request request = {UINT64_C(0x0102030405060708),
                                              UINT64_MAX};
  struct sophia_pf_capture_status out, prior;
  unsigned char bytes[33], invalid[33];
  size_t n, offset;
  unsigned state;
  memset(bytes, 0xa5, sizeof bytes);
  for (n = 0; n < 24; ++n) {
    assert(sophia_pf_encode_capture(request, bytes, n) == -1);
    for (offset = 0; offset < sizeof bytes; ++offset)
      assert(bytes[offset] == 0xa5);
  }
  assert(!sophia_pf_encode_capture(request, bytes, sizeof bytes));
  assert(!memcmp(bytes, expected, 24) && bytes[24] == 0xa5);
  request.serial = 0;
  assert(sophia_pf_encode_capture(request, bytes, sizeof bytes) == -1);
  assert(!memcmp(bytes, expected, 24));
  request.serial = UINT64_C(0x0102030405060708);
  request.output = 0;
  assert(sophia_pf_encode_capture(request, bytes, sizeof bytes) == -1);
  assert(!memcmp(bytes, expected, 24));
  for (state = 0; state < 8; ++state) {
    memset(bytes, 0, sizeof bytes);
    memcpy(bytes, expected, 24);
    if (!state)
      memset(bytes + 8, 0, 16);
    bytes[24] = (unsigned char)state;
    assert(!sophia_pf_decode_capture_status(bytes, 32, &out));
    assert((unsigned)out.state == state &&
           out.serial == (state ? request.serial : 0) &&
           out.output == (state ? UINT64_MAX : 0));
    memcpy(&prior, &out, sizeof out);
    for (n = 0; n <= 33; ++n) {
      if (n == 32)
        continue;
      assert(sophia_pf_decode_capture_status(bytes, n, &out) == -1);
      assert(!memcmp(&out, &prior, sizeof out));
    }
    for (offset = 8; offset <= 16; offset += 8) {
      memcpy(invalid, bytes, sizeof bytes);
      memset(invalid + offset, state ? 0 : 1, 8);
      assert(sophia_pf_decode_capture_status(invalid, 32, &out) == -1);
      assert(!memcmp(&out, &prior, sizeof out));
    }
    bytes[0] = 2;
    assert(sophia_pf_decode_capture_status(bytes, 32, &out) == -1);
    bytes[0] = 1;
    bytes[24] = 8;
    assert(sophia_pf_decode_capture_status(bytes, 32, &out) == -1);
    bytes[24] = (unsigned char)state;
    bytes[31] = 1;
    assert(sophia_pf_decode_capture_status(bytes, 32, &out) == -1);
    assert(!memcmp(&out, &prior, sizeof out));
  }
  return 0;
}
