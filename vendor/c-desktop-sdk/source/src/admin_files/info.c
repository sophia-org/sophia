#include "../sophia_admin_files.h"
#include <string.h>

int sophia_af_decode_info(const void *bytes, size_t length,
                          struct sophia_af_info *out) {
  const unsigned char *p = bytes;
  uint64_t words[6];
  struct sophia_af_info value;
  size_t pos = 8, i;
  if (!p || !out || length < 9 || length > SOPHIA_AF_LINE_MAX_BYTES ||
      memcmp(p, "v1 info ", 8)) return -1;
  for (i = 0; i < 6; ++i) {
    size_t start = pos;
    uint64_t n = 0;
    while (pos < length && p[pos] >= '0' && p[pos] <= '9') {
      unsigned digit = p[pos++] - '0';
      if (n > (UINT64_MAX - digit) / 10) return -1;
      n = n * 10 + digit;
    }
    if (pos == start || (pos - start > 1 && p[start] == '0') ||
        pos == length || p[pos++] != (i == 5 ? '\n' : ' ')) return -1;
    words[i] = n;
  }
  if (pos != length || !(words[0] | words[1]) || !words[2]) return -1;
  for (i = 3; i < 6; ++i)
    if (!words[i] || words[i] > UINT32_MAX) return -1;
  value.session_low = words[0];
  value.session_high = words[1];
  value.connection = words[2];
  value.command_ms = (uint32_t)words[3];
  value.frame_ms = (uint32_t)words[4];
  value.idle_ms = (uint32_t)words[5];
  *out = value;
  return 0;
}
