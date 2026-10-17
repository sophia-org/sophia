#include "internal.h"
#include <inttypes.h>
#include <stdio.h>
#include <string.h>

int sophia_af_name_valid(const char *name, size_t n) {
  size_t i;
  if (!name || !n || n > 128 || name[0] == ' ' || name[n-1] == ' ') return 0;
  for (i = 0; i < n; ++i) {
    unsigned char c = (unsigned char)name[i];
    if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
          (c >= '0' && c <= '9') || c == ' ' || c == '-' || c == '_' || c == '.')) return 0;
  }
  return 1;
}

int sophia_af_number(const unsigned char *p, size_t length, size_t *pos,
                  unsigned char delimiter, uint64_t *out) {
  size_t start = *pos;
  uint64_t n = 0;
  while (*pos < length && p[*pos] >= '0' && p[*pos] <= '9') {
    unsigned digit = p[(*pos)++] - '0';
    if (n > (UINT64_MAX - digit) / 10) return -1;
    n = n * 10 + digit;
  }
  if (*pos == start || (*pos - start > 1 && p[start] == '0') ||
      *pos == length || p[(*pos)++] != delimiter) return -1;
  *out = n;
  return 0;
}

int sophia_af_decode_invocation(const void *bytes, size_t length,
                                struct sophia_af_invocation *out) {
  const unsigned char *p = bytes;
  struct sophia_af_invocation v;
  size_t pos = 10;
  uint64_t count;
  if (!p || !out || length < 11 || length > SOPHIA_AF_LINE_MAX_BYTES ||
      memcmp(p, "v1 invoke ", 10) || p[length-1] != '\n') return -1;
  if (sophia_af_number(p, length, &pos, ' ', &v.connection) ||
      sophia_af_number(p, length, &pos, ' ', &v.request) ||
      sophia_af_number(p, length, &pos, ' ', &v.generation)) return -1;
  if (length - pos >= 7 && !memcmp(p + pos, "policy ", 7)) {
    v.owner = SOPHIA_AF_POLICY; pos += 7;
  } else if (length - pos >= 8 && !memcmp(p + pos, "session ", 8)) {
    v.owner = SOPHIA_AF_SESSION; pos += 8;
  } else return -1;
  if (sophia_af_number(p, length, &pos, ':', &count) || pos >= length ||
      count != length - pos - 1 || !v.connection || !v.request || !v.generation) return -1;
  v.name = (const char *)p + pos;
  v.name_length = (size_t)count;
  if (!sophia_af_name_valid(v.name, v.name_length)) return -1;
  *out = v;
  return 0;
}

int sophia_af_encode_invocation(const struct sophia_af_invocation *v,
                                void *bytes, size_t capacity, size_t *written) {
  char buffer[SOPHIA_AF_LINE_MAX_BYTES];
  int n;
  const char *owner;
  if (!v || !bytes || !written || !v->connection || !v->request || !v->generation ||
      !sophia_af_name_valid(v->name, v->name_length)) return -1;
  if (v->owner == SOPHIA_AF_POLICY) owner = "policy";
  else if (v->owner == SOPHIA_AF_SESSION) owner = "session";
  else return -1;
  n = snprintf(buffer, sizeof buffer, "v1 invoke %" PRIu64 " %" PRIu64 " %" PRIu64 " %s %zu:%.*s\n",
      v->connection, v->request, v->generation, owner, v->name_length, (int)v->name_length, v->name);
  if (n < 0 || (size_t)n >= sizeof buffer || (size_t)n > capacity) return -1;
  memcpy(bytes, buffer, (size_t)n);
  *written = (size_t)n;
  return 0;
}
