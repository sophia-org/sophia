#include "../sophia_broker_files.h"
#include "internal.h"
#include <string.h>
static uint64_t get(const unsigned char *b, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)b[i] << (8 * i);
  return v;
}
static int zero(const unsigned char *b, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    if (b[i])
      return 0;
  return 1;
}
int sophia_bf_utf8(const unsigned char *b, size_t n) {
  size_t i = 0;
  while (i < n) {
    unsigned c = b[i++], need, code, minimum;
    if (c < 128)
      continue;
    if (c >= 0xc2 && c <= 0xdf) {
      need = 1;
      code = c & 31;
      minimum = 128;
    } else if (c >= 0xe0 && c <= 0xef) {
      need = 2;
      code = c & 15;
      minimum = 2048;
    } else if (c >= 0xf0 && c <= 0xf4) {
      need = 3;
      code = c & 7;
      minimum = 65536;
    } else
      return 0;
    if (need > n - i)
      return 0;
    while (need--) {
      c = b[i++];
      if ((c & 0xc0) != 0x80)
        return 0;
      code = (code << 6) | (c & 63);
    }
    if (code < minimum || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff))
      return 0;
  }
  return 1;
}
static int request(const unsigned char *b, struct sophia_bf_request *r) {
  unsigned kind = (unsigned)get(b + 8, 2), value = (unsigned)get(b + 24, 2);
  size_t length = (size_t)get(b + 28, 2);
  if (!get(b, 8) || kind < 1 || kind > 5 || !zero(b + 10, 6) ||
      (kind == 1   ? value < 1 || value > 2
       : kind == 4 ? value != 0
                   : value < 1 || value > 3) ||
      b[26] > 1 || b[27] > 1 || (b[26] && !b[27]) || get(b + 30, 2) ||
      length > 128 || (!b[27] && length) ||
      !zero(b + 40 + length, 128 - length) ||
      (kind != 2 && !zero(b + 26, 142)) || !sophia_bf_utf8(b + 40, length))
    return -1;
  r->id = get(b, 8);
  r->kind = (enum sophia_bf_request_kind)kind;
  r->surface_index = (uint32_t)get(b + 16, 4);
  r->surface_generation = (uint32_t)get(b + 20, 4);
  r->value = (uint16_t)value;
  r->redacted = b[26];
  r->label_present = b[27];
  r->label = b[27] ? (const char *)b + 40 : NULL;
  r->label_length = length;
  r->candidate_generation = get(b + 32, 8);
  return 0;
}
int sophia_bf_decode_event(const void *data, size_t n, uint64_t epoch,
                           struct sophia_bf_event *out) {
  const unsigned char *b = data;
  struct sophia_bf_event v;
  unsigned kind;
  if (!b || !out || n < 32 || !epoch || get(b, 4) != n || get(b + 4, 2) != 1 ||
      get(b + 8, 8) != epoch || get(b + 16, 8) || !get(b + 24, 8))
    return -1;
  memset(&v, 0, sizeof v);
  v.epoch = epoch;
  v.sequence = get(b + 24, 8);
  kind = (unsigned)get(b + 6, 2);
  v.kind = (enum sophia_bf_event_kind)kind;
  switch (kind) {
  case 16:
    if (n != 56 || get(b + 32, 2) != 2 || get(b + 34, 2) ||
        get(b + 36, 4) < 1 || get(b + 36, 4) > 1024 ||
        get(b + 40, 8) != epoch || get(b + 48, 2) < 1 || get(b + 48, 2) > 128 ||
        get(b + 50, 2) != 2 || get(b + 52, 4))
      return -1;
    v.max_surfaces = (uint32_t)get(b + 36, 4);
    v.max_label_bytes = (uint16_t)get(b + 48, 2);
    break;
  case 17:
    if (n != 40 || get(b + 32, 2) != 1 || !zero(b + 34, 6))
      return -1;
    break;
  case 18:
    if (n != 48 || !get(b + 32, 8) || get(b + 40, 2) < 256 ||
        get(b + 40, 2) > 257 || !zero(b + 42, 6))
      return -1;
    v.submission = get(b + 32, 8);
    v.candidate_kind = (uint16_t)get(b + 40, 2);
    break;
  case 32:
    if (n != 200 || request(b + 32, &v.request))
      return -1;
    break;
  default:
    return -1;
  }
  *out = v;
  return 0;
}
