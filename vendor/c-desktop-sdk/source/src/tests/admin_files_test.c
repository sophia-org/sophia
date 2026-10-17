#include "sophia_admin_files.h"
#include <assert.h>
#include <string.h>

int main(void) {
  const char *valid = "v1 info 0 1 7 10000 2000 60000\n";
  const char *maximum = "v1 info 18446744073709551615 0 18446744073709551615 4294967295 1 4294967295\n";
  const char *bad[] = {
    "v1 info 0 0 7 1 1 1\n", "v1 info 1 0 0 1 1 1\n",
    "v1 info 1 0 1 0 1 1\n", "v1 info 1 0 1 1 0 1\n",
    "v1 info 1 0 1 1 1 0\n", "v1 info 1 0 1 4294967296 1 1\n",
    "v1 info 18446744073709551616 0 1 1 1 1\n",
    "v1 info 01 0 1 1 1 1\n", "v1 info +1 0 1 1 1 1\n",
    "v1 info 1 0 1 1 1 1 \n", "v1 info 1  0 1 1 1 1\n",
    "v1 info 1 0 1 1 1 1\r\n", "v1 info 1 0 1 1 1 1\n\n",
    "v2 info 1 0 1 1 1 1\n", "v1 info \xef\xbc\x91 0 1 1 1 1\n",
    "v1 info \xff 0 1 1 1 1\n"
  };
  struct sophia_af_info info, saved;
  unsigned char oversized[513];
  size_t i;
  memset(&info, 0, sizeof info);
  assert(!sophia_af_decode_info(valid, strlen(valid), &info));
  assert(info.session_low == 0 && info.session_high == 1 && info.connection == 7);
  assert(info.command_ms == 10000 && info.frame_ms == 2000 && info.idle_ms == 60000);
  assert(!sophia_af_decode_info(maximum, strlen(maximum), &info));
  assert(info.session_low == UINT64_MAX && info.connection == UINT64_MAX);
  assert(info.command_ms == UINT32_MAX && info.idle_ms == UINT32_MAX);
  saved = info;
  for (i = 0; i < strlen(valid); ++i)
    assert(sophia_af_decode_info(valid, i, &info) == -1);
  for (i = 0; i < sizeof bad / sizeof bad[0]; ++i)
    assert(sophia_af_decode_info(bad[i], strlen(bad[i]), &info) == -1);
  memset(oversized, '1', sizeof oversized);
  assert(sophia_af_decode_info(oversized, sizeof oversized, &info) == -1);
  assert(sophia_af_decode_info("v1 info 1 0 1 1 1 1\0\n",
      sizeof "v1 info 1 0 1 1 1 1\0\n" - 1, &info) == -1);
  assert(sophia_af_decode_info(NULL, 0, &info) == -1);
  assert(sophia_af_decode_info(valid, strlen(valid), NULL) == -1);
  assert(info.session_low == saved.session_low && info.session_high == saved.session_high);
  assert(info.connection == saved.connection && info.command_ms == saved.command_ms);
  assert(info.frame_ms == saved.frame_ms && info.idle_ms == saved.idle_ms);
  return 0;
}
