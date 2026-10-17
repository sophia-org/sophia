#include "internal.h"
#include <string.h>

static int compare(const struct sophia_af_command *a, const struct sophia_af_command *b) {
  int order;
  size_t n;
  if (a->owner != b->owner) return a->owner < b->owner ? -1 : 1;
  n = a->name_length < b->name_length ? a->name_length : b->name_length;
  order = memcmp(a->name, b->name, n);
  if (order) return order;
  return a->name_length < b->name_length ? -1 : a->name_length != b->name_length;
}
static int named(const struct sophia_af_command *c, const char *text) {
  return c->name_length == strlen(text) && !memcmp(c->name, text, c->name_length);
}

int sophia_af_decode_catalog(const void *bytes, size_t length,
                             struct sophia_af_catalog *out) {
  const unsigned char *p = bytes;
  struct sophia_af_catalog v = {0};
  uint64_t count, name_length;
  size_t pos = 11, i, policies = 0;
  if (!p || !out || length < 12 || length > SOPHIA_AF_CATALOG_MAX_BYTES ||
      memcmp(p, "v1 catalog ", 11)) return -1;
  if (sophia_af_number(p, length, &pos, ' ', &v.generation) || !v.generation ||
      sophia_af_number(p, length, &pos, '\n', &count) || count > SOPHIA_AF_CATALOG_MAX_COMMANDS) return -1;
  v.count = (size_t)count;
  for (i = 0; i < v.count; ++i) {
    struct sophia_af_command *c = &v.rows[i];
    if (length - pos < 11 || memcmp(p + pos, "v1 command ", 11)) return -1;
    pos += 11;
    if (length - pos >= 7 && !memcmp(p + pos, "policy ", 7)) {
      c->owner = SOPHIA_AF_POLICY; pos += 7;
      if (++policies > 256) return -1;
    } else if (length - pos >= 8 && !memcmp(p + pos, "session ", 8)) {
      c->owner = SOPHIA_AF_SESSION; pos += 8;
    } else return -1;
    if (sophia_af_number(p, length, &pos, ':', &name_length) ||
        name_length > 128 || name_length >= length - pos) return -1;
    c->name = (const char *)p + pos; c->name_length = (size_t)name_length;
    if (!sophia_af_name_valid(c->name, c->name_length)) return -1;
    pos += c->name_length;
    if (p[pos++] != '\n') return -1;
    if (c->owner == SOPHIA_AF_SESSION && !named(c, "logout") &&
        !named(c, "reload-profile") && !named(c, "restart-wm")) return -1;
    if (i && compare(&v.rows[i-1], c) >= 0) return -1;
  }
  if (pos != length) return -1;
  *out = v;
  return 0;
}

int sophia_af_catalog_contains(const struct sophia_af_catalog *catalog,
                               const struct sophia_af_command *command) {
  size_t low = 0, high;
  if (!catalog || !command || catalog->count > SOPHIA_AF_CATALOG_MAX_COMMANDS ||
      !sophia_af_name_valid(command->name, command->name_length)) return 0;
  high = catalog->count;
  while (low < high) {
    size_t mid = low + (high - low) / 2;
    int order = compare(&catalog->rows[mid], command);
    if (!order) return 1;
    if (order < 0) low = mid + 1;
    else high = mid;
  }
  return 0;
}
