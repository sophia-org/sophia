#include "../sophia_portal_files.h"
int sophia_pf_decode_frame_info(const void *bytes, size_t length,
                                struct sophia_pf_frame_info *out) {
  const unsigned char *p = bytes;
  uint64_t f[8] = {0};
  struct sophia_pf_frame_info value = {0};
  size_t i, j;
  if (!p || !out || length != SOPHIA_PF_FRAME_INFO_BYTES)
    return -1;
  for (i = 0; i < 8; ++i)
    for (j = 0; j < 8; ++j)
      f[i] |= (uint64_t)p[i * 8 + j] << (j * 8);
  if (f[0] != 1 || f[7] != 1 || !f[1] || !f[2] || !f[3] || !f[4] || !f[5] ||
      f[4] > UINT32_MAX || f[5] > UINT32_MAX || f[6] != f[4] * 4 ||
      f[5] > SOPHIA_PF_IMAGE_MAX_BYTES / f[6])
    return -1;
  value.output = f[1];
  value.output_generation = f[2];
  value.presentation_generation = f[3];
  value.width = (uint32_t)f[4];
  value.height = (uint32_t)f[5];
  value.stride = f[6];
  value.image_bytes = f[6] * f[5];
  *out = value;
  return 0;
}
