#include "sophia_broker_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static void put(unsigned char *b, uint32_t v) {
  unsigned i;
  for (i = 0; i < 4; ++i)
    b[i] = (unsigned char)(v >> (8 * i));
}
int main(void) {
  const char *epochs[] = {"1",  "18446744073709551615", "0", "01", "", "-1",
                          "+1", "18446744073709551616", "1 "};
  const uint32_t fields[][3] = {{36, 1, 1024},  {44, 4, 16},   {48, 352, 4096},
                                {52, 448, 512}, {56, 1, 5000}, {60, 1, 2000},
                                {64, 1, 12000}};
  const unsigned reserved[] = {4, 6, 16, 24, 32, 34, 42, 68};
  unsigned char good[72] = {0}, bad[72];
  struct sophia_bf_limits out, sentinel;
  size_t i, j;
  uint64_t epoch;
  char text[160];
  for (i = 0; i < sizeof epochs / sizeof *epochs; ++i) {
    int n = snprintf(text, sizeof text,
                     "sophia-broker-files version=1 role=metadata-broker "
                     "epoch=%s fd_transfer=none\n",
                     epochs[i]);
    epoch = 99;
    assert((sophia_bf_decode_api(text, (size_t)n, &epoch) == 0) == (i < 2));
    assert(epoch == (i == 0 ? 1 : i == 1 ? UINT64_MAX : 99));
    if (i < 2)
      for (j = 0; j < (size_t)n; ++j) {
        epoch = 99;
        assert(sophia_bf_decode_api(text, j, &epoch) && epoch == 99);
      }
  }
  good[0] = 72;
  good[4] = 1;
  good[6] = 1;
  good[8] = 7;
  good[32] = 2;
  good[40] = 128;
  good[42] = 2;
  for (i = 0; i < sizeof fields / sizeof *fields; ++i)
    put(good + fields[i][0], fields[i][2]);
  assert(!sophia_bf_decode_limits(good, 72, 7, &out) &&
         out.max_surfaces == 1024);
  memset(&sentinel, 0xa5, sizeof sentinel);
  for (i = 0; i < 72; ++i) {
    out = sentinel;
    assert(sophia_bf_decode_limits(good, i, 7, &out));
    assert(!memcmp(&out, &sentinel, sizeof out));
  }
  assert(sophia_bf_decode_limits(good, 72, 8, &out));
  assert(sophia_bf_decode_limits(good, 72, 0, &out));
  for (i = 0; i < sizeof reserved / sizeof *reserved; ++i) {
    memcpy(bad, good, 72);
    bad[reserved[i]] ^= 1;
    assert(sophia_bf_decode_limits(bad, 72, 7, &out));
  }
  for (i = 0; i < sizeof fields / sizeof *fields; ++i) {
    uint32_t values[] = {fields[i][1] - 1, fields[i][1], fields[i][2],
                         fields[i][2] + 1};
    for (j = 0; j < 4; ++j) {
      memcpy(bad, good, 72);
      put(bad + fields[i][0], values[j]);
      out = sentinel;
      assert((sophia_bf_decode_limits(bad, 72, 7, &out) == 0) ==
             (j == 1 || j == 2));
      if (j == 0 || j == 3)
        assert(!memcmp(&out, &sentinel, sizeof out));
    }
  }
  for (i = 0; i < 4; ++i) {
    const unsigned values[] = {0, 1, 128, 129};
    memcpy(bad, good, 72);
    bad[40] = (unsigned char)values[i];
    assert((sophia_bf_decode_limits(bad, 72, 7, &out) == 0) ==
           (i == 1 || i == 2));
  }
  return 0;
}
