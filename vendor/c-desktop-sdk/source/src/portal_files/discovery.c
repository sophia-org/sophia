#include "../sophia_portal_files.h"
#include <string.h>
static int take(const unsigned char **p, size_t *n, const char *literal) {
  size_t length = strlen(literal);
  if (*n < length || memcmp(*p, literal, length))
    return -1;
  *p += length;
  *n -= length;
  return 0;
}
static int capability(const unsigned char **p, size_t *n,
                      enum sophia_pf_request_capability *out) {
  if (!take(p, n, "unavailable")) {
    *out = SOPHIA_PF_REQUEST_UNAVAILABLE;
    return 0;
  }
  if (!take(p, n, "output-capture-v1")) {
    *out = SOPHIA_PF_OUTPUT_CAPTURE_V1;
    return 0;
  }
  return -1;
}
int sophia_pf_decode_api(const void *bytes, size_t n,
                         struct sophia_pf_api *out) {
  const unsigned char *p = bytes;
  struct sophia_pf_api value = {0};
  if (!p || !out || n > SOPHIA_PF_API_MAX_BYTES ||
      take(&p, &n, "sophia-portal-client version=1 epoch=") || !n || *p < '1' ||
      *p > '9')
    return -1;
  while (n && *p >= '0' && *p <= '9') {
    unsigned digit = *p - '0';
    if (value.epoch > (UINT64_MAX - digit) / 10)
      return -1;
    value.epoch = value.epoch * 10 + digit;
    ++p;
    --n;
  }
  if (take(&p, &n, " request=") || capability(&p, &n, &value.request) ||
      take(&p, &n, " fd_transfer=none\n") || n)
    return -1;
  *out = value;
  return 0;
}
int sophia_pf_decode_status(const void *bytes, size_t n,
                            struct sophia_pf_status *out) {
  const unsigned char *p = bytes;
  struct sophia_pf_status value = {0};
  if (!p || !out || n > SOPHIA_PF_STATUS_MAX_BYTES ||
      take(&p, &n, "request=") || capability(&p, &n, &value.request) ||
      take(&p, &n, " transfer="))
    return -1;
  if (!take(&p, &n, "absent\n"))
    value.transfer = SOPHIA_PF_TRANSFER_ABSENT;
  else if (!take(&p, &n, "ready\n"))
    value.transfer = SOPHIA_PF_TRANSFER_READY;
  else if (!take(&p, &n, "revoked\n"))
    value.transfer = SOPHIA_PF_TRANSFER_REVOKED;
  else
    return -1;
  if (n)
    return -1;
  *out = value;
  return 0;
}
