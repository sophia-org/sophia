#include "../sophia_portal_files.h"
int sophia_pf_encode_capture(struct sophia_pf_capture_request request,
                             void *bytes, size_t capacity) {
  unsigned char *p = bytes;
  uint64_t f[3] = {1, request.serial, request.output};
  size_t i, j;
  if (!p || capacity < SOPHIA_PF_CAPTURE_BYTES || !request.serial ||
      !request.output)
    return -1;
  for (i = 0; i < 3; ++i)
    for (j = 0; j < 8; ++j)
      p[i * 8 + j] = (unsigned char)(f[i] >> (8 * j));
  return 0;
}
int sophia_pf_decode_capture_status(const void *bytes, size_t length,
                                    struct sophia_pf_capture_status *out) {
  const unsigned char *p = bytes;
  uint64_t f[4] = {0};
  struct sophia_pf_capture_status value = {0};
  size_t i, j;
  if (!p || !out || length != SOPHIA_PF_CAPTURE_STATUS_BYTES)
    return -1;
  for (i = 0; i < 4; ++i)
    for (j = 0; j < 8; ++j)
      f[i] |= (uint64_t)p[i * 8 + j] << (8 * j);
  if (f[0] != 1 || f[3] > 7 || (f[3] == 0 ? (f[1] || f[2]) : (!f[1] || !f[2])))
    return -1;
  value.serial = f[1];
  value.output = f[2];
  value.state = (enum sophia_pf_capture_state)f[3];
  *out = value;
  return 0;
}
