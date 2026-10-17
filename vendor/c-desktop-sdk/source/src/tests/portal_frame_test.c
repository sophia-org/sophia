#include "sophia_portal_files.h"
#include <assert.h>
#include <string.h>
static void record(unsigned char *b, const uint64_t *f) {
  size_t i, j;
  for (i = 0; i < 8; ++i)
    for (j = 0; j < 8; ++j)
      b[i * 8 + j] = (unsigned char)(f[i] >> (j * 8));
}
int main(void) {
  uint64_t f[8] = {1, UINT64_MAX, 2, 3, 4096, 4096, 16384, 1};
  const uint64_t invalid[][2] = {{0, 2},
                                 {1, 0},
                                 {2, 0},
                                 {3, 0},
                                 {4, 0},
                                 {5, 0},
                                 {4, UINT64_C(4294967296)},
                                 {5, UINT64_C(4294967296)},
                                 {5, 4097},
                                 {6, 16385},
                                 {7, 2}};
  unsigned char b[65] = {0};
  struct sophia_pf_frame_info out, prior;
  size_t i;
  record(b, f);
  assert(!sophia_pf_decode_frame_info(b, 64, &out));
  assert(out.output == UINT64_MAX && out.output_generation == 2 &&
         out.presentation_generation == 3);
  assert(out.width == 4096 && out.height == 4096 && out.stride == 16384 &&
         out.image_bytes == SOPHIA_PF_IMAGE_MAX_BYTES);
  memcpy(&prior, &out, sizeof out);
  for (i = 0; i <= 65; ++i) {
    if (i == 64)
      continue;
    assert(sophia_pf_decode_frame_info(b, i, &out) == -1);
    assert(!memcmp(&prior, &out, sizeof out));
  }
  for (i = 0; i < sizeof invalid / sizeof invalid[0]; ++i) {
    uint64_t old = f[invalid[i][0]];
    f[invalid[i][0]] = invalid[i][1];
    record(b, f);
    assert(sophia_pf_decode_frame_info(b, 64, &out) == -1);
    assert(!memcmp(&prior, &out, sizeof out));
    f[invalid[i][0]] = old;
  }
  f[4] = f[5] = UINT32_MAX;
  f[6] = (uint64_t)UINT32_MAX * 4;
  record(b, f);
  assert(sophia_pf_decode_frame_info(b, 64, &out) == -1);
  f[4] = f[5] = 1;
  f[6] = 4;
  record(b, f);
  assert(!sophia_pf_decode_frame_info(b, 64, &out) && out.image_bytes == 4);
  assert(sophia_pf_decode_frame_info(NULL, 64, &out) == -1);
  assert(sophia_pf_decode_frame_info(b, 64, NULL) == -1);
  return 0;
}
