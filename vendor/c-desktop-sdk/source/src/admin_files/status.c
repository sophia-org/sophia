#include "../sophia_admin_files.h"
#include <string.h>

struct span { const unsigned char *p; size_t n; };
static int equal(struct span s, const char *v) {
  return s.n == strlen(v) && !memcmp(s.p, v, s.n);
}
static struct span take(struct span *rest, unsigned char separator) {
  struct span result = *rest;
  size_t i;
  for (i = 0; i < rest->n && rest->p[i] != separator; ++i) {}
  result.n = i;
  if (i < rest->n) ++i;
  rest->p += i; rest->n -= i;
  return result;
}
static int decimal(struct span s, uint64_t *out) {
  size_t i;
  uint64_t v = 0;
  if (!s.n || (s.n > 1 && s.p[0] == '0')) return -1;
  for (i = 0; i < s.n; ++i) {
    unsigned digit;
    if (s.p[i] < '0' || s.p[i] > '9') return -1;
    digit = s.p[i] - '0';
    if (v > (UINT64_MAX - digit) / 10) return -1;
    v = v * 10 + digit;
  }
  *out = v;
  return 0;
}
/* Same Unicode scalar/control boundary as Rust str + char::is_control. */
static int detail_valid(struct span s) {
  size_t i = 0, j, count;
  uint32_t value, minimum;
  while (i < s.n) {
    unsigned c = s.p[i++];
    if (c < 0x80) { value = c; count = 0; minimum = 0; }
    else if (c >= 0xc2 && c <= 0xdf) { value = c & 31; count = 1; minimum = 0x80; }
    else if (c >= 0xe0 && c <= 0xef) { value = c & 15; count = 2; minimum = 0x800; }
    else if (c >= 0xf0 && c <= 0xf4) { value = c & 7; count = 3; minimum = 0x10000; }
    else return 0;
    if (count > s.n - i) return 0;
    for (j = 0; j < count; ++j) {
      c = s.p[i++];
      if ((c & 0xc0) != 0x80) return 0;
      value = (value << 6) | (c & 63);
    }
    if (value < minimum || value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff) ||
        value < 32 || (value >= 127 && value <= 159)) return 0;
  }
  return 1;
}

int sophia_af_decode_status(const void *bytes, size_t length,
                            struct sophia_af_status *out) {
  const unsigned char *p = bytes;
  struct sophia_af_status v = {0};
  struct span rest, kind, outcome, count;
  static const char *names[] = {"committed", "completed", "unchanged", "rejected", "stale",
    "denied", "unavailable", "overloaded", "timed-out", "indeterminate"};
  uint64_t n;
  size_t i;
  if (!p || !out || !length || length > SOPHIA_AF_LINE_MAX_BYTES || p[length-1] != '\n') return -1;
  rest.p = p; rest.n = length - 1;
  if (!equal(take(&rest, ' '), "v1")) return -1;
  kind = take(&rest, ' ');
  if (equal(kind, "ready")) {
    if (decimal(rest, &v.connection) || !v.connection) return -1;
    v.kind = SOPHIA_AF_READY;
  } else {
    if (decimal(take(&rest, ' '), &v.connection) || !v.connection ||
        decimal(take(&rest, ' '), &v.request) || !v.request) return -1;
    if (equal(kind, "pending")) {
      if (decimal(rest, &v.generation) || !v.generation) return -1;
      v.kind = SOPHIA_AF_PENDING;
    } else {
      if (!equal(kind, "outcome") || decimal(take(&rest, ' '), &v.generation) || !v.generation) return -1;
      outcome = take(&rest, ' ');
      for (i = 0; i < sizeof names / sizeof names[0]; ++i) if (equal(outcome, names[i])) break;
      if (i == sizeof names / sizeof names[0]) return -1;
      v.kind = SOPHIA_AF_OUTCOME;
      v.outcome = (enum sophia_af_outcome)(i + 1);
      count = take(&rest, ':');
      /* take() must have consumed the colon even for an empty detail. */
      if (count.n == 0 || count.p + count.n >= p + length - 1 || count.p[count.n] != ':' ||
          decimal(count, &n) || n > 256 || n != rest.n || !detail_valid(rest)) return -1;
      v.detail = (const char *)rest.p; v.detail_length = rest.n;
    }
  }
  *out = v;
  return 0;
}
