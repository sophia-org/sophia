#include "../sophia_broker_files.h"
#include <string.h>
static uint64_t get(const unsigned char *b, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)b[i] << (8 * i);
  return v;
}
int sophia_bf_decode_api(const void *data, size_t n, uint64_t *out) {
  const unsigned char *b = data;
  const char prefix[] =
      "sophia-broker-files version=1 role=metadata-broker epoch=";
  const char suffix[] = " fd_transfer=none\n";
  size_t i, end;
  uint64_t epoch = 0;
  if (!b || !out || n < sizeof prefix + sizeof suffix - 1 ||
      memcmp(b, prefix, sizeof prefix - 1) ||
      memcmp(b + n - (sizeof suffix - 1), suffix, sizeof suffix - 1))
    return -1;
  i = sizeof prefix - 1;
  end = n - (sizeof suffix - 1);
  if (b[i] == '0')
    return -1;
  for (; i < end; ++i) {
    if (b[i] < '0' || b[i] > '9' || epoch > (UINT64_MAX - (b[i] - '0')) / 10)
      return -1;
    epoch = epoch * 10 + (b[i] - '0');
  }
  *out = epoch;
  return 0;
}
int sophia_bf_decode_limits(const void *data, size_t n, uint64_t epoch,
                            struct sophia_bf_limits *out) {
  const unsigned char *b = data;
  struct sophia_bf_limits v;
  if (!b || !out || n != 72 || !epoch || get(b, 4) != 72 ||
      get(b + 4, 2) != 1 || get(b + 6, 2) != 1 || get(b + 8, 8) != epoch ||
      get(b + 16, 8) || get(b + 24, 8) || get(b + 32, 2) != 2 ||
      get(b + 34, 2) || get(b + 42, 2) != 2 || get(b + 68, 4))
    return -1;
  memset(&v, 0, sizeof v);
  v.max_surfaces = (uint32_t)get(b + 36, 4);
  v.max_label_bytes = (uint16_t)get(b + 40, 2);
  v.journal_records = (uint32_t)get(b + 44, 4);
  v.journal_bytes = (uint32_t)get(b + 48, 4);
  v.staging_bytes = (uint32_t)get(b + 52, 4);
  v.response_timeout_ms = (uint32_t)get(b + 56, 4);
  v.ack_progress_timeout_ms = (uint32_t)get(b + 60, 4);
  v.assembly_timeout_ms = (uint32_t)get(b + 64, 4);
  if (!v.max_surfaces || v.max_surfaces > 1024 || !v.max_label_bytes ||
      v.max_label_bytes > 128 || v.journal_records < 4 ||
      v.journal_records > 16 || v.journal_bytes < 352 ||
      v.journal_bytes > 4096 || v.staging_bytes < 448 ||
      v.staging_bytes > 512 || !v.response_timeout_ms ||
      v.response_timeout_ms > 5000 || !v.ack_progress_timeout_ms ||
      v.ack_progress_timeout_ms > 2000 || !v.assembly_timeout_ms ||
      v.assembly_timeout_ms > 12000)
    return -1;
  *out = v;
  return 0;
}
