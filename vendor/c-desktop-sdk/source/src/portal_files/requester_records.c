#include "../sophia_portal_requester_files.h"
#include <string.h>
static uint64_t get(const unsigned char *b, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)b[i] << (i * 8);
  return v;
}
static void put(unsigned char *b, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    b[i] = (unsigned char)(v >> (i * 8));
}
static int zero(const unsigned char *b, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    if (b[i])
      return 0;
  return 1;
}
static int header(const unsigned char *b, size_t n, int event) {
  return b && n >= 32 && get(b, 4) == n && get(b + 4, 2) == 1 &&
         get(b + 8, 8) && !get(b + 16, 8) &&
         (event ? get(b + 24, 8) != 0 : get(b + 24, 8) == 0);
}
static int utf8(const char *text, size_t n) {
  const unsigned char *b = (const unsigned char *)text;
  size_t i = 0;
  if (n && !b)
    return 0;
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
int sophia_pq_decode_api(const void *data, size_t n, uint64_t *epoch) {
  const unsigned char *b = data;
  const char prefix[] =
      "sophia-portal-files version=1 role=portal-requester epoch=";
  const char suffix[] = " fd_transfer=none\n";
  size_t start = sizeof prefix - 1, tail = sizeof suffix - 1, i, end;
  uint64_t v = 0;
  if (!b || !epoch || n <= start + tail || n > start + tail + 20 ||
      memcmp(b, prefix, start) || memcmp(b + n - tail, suffix, tail) ||
      b[start] == '0')
    return -1;
  end = n - tail;
  for (i = start; i < end; ++i) {
    unsigned d = b[i] - '0';
    if (d > 9 || v > (UINT64_MAX - d) / 10)
      return -1;
    v = v * 10 + d;
  }
  *epoch = v;
  return 0;
}
int sophia_pq_decode_limits(const void *data, size_t n, uint64_t *epoch,
                            struct sophia_pq_limits *out) {
  const unsigned char *b = data;
  struct sophia_pq_limits v;
  uint64_t e;
  if (!out || !epoch || !header(b, n, 0) || n != 80 || get(b + 6, 2) != 1)
    return -1;
  e = get(b + 8, 8);
  b += 32;
  memset(&v, 0, sizeof v);
  v.max_transfers = (uint32_t)get(b + 4, 4);
  v.max_payload_bytes = (uint32_t)get(b + 8, 4);
  v.journal_records = (uint32_t)get(b + 16, 4);
  v.journal_bytes = (uint32_t)get(b + 20, 4);
  v.assembly_timeout_ms = (uint32_t)get(b + 24, 4);
  v.ack_progress_timeout_ms = (uint32_t)get(b + 28, 4);
  v.max_retained_transfers = (uint32_t)get(b + 36, 4);
  v.broker_generation = get(b + 40, 8);
  if (get(b, 2) != 1 || get(b + 2, 2) != 1 || get(b + 12, 2) != 255 ||
      get(b + 14, 2) || get(b + 32, 4) != 344 || !v.max_transfers ||
      v.max_transfers > 64 || !v.max_payload_bytes ||
      v.max_payload_bytes > 65536 || v.journal_records < 4 ||
      v.journal_records > 256 || v.journal_bytes < 280 ||
      v.journal_bytes > 32768 || !v.assembly_timeout_ms ||
      v.assembly_timeout_ms > 12000 || !v.ack_progress_timeout_ms ||
      v.ack_progress_timeout_ms > 2000 ||
      v.max_retained_transfers < v.max_transfers ||
      v.max_retained_transfers > 4096 || !v.broker_generation)
    return -1;
  *epoch = e;
  *out = v;
  return 0;
}
int sophia_pq_decode_event(const void *data, size_t n,
                           struct sophia_pq_event *out) {
  const unsigned char *b = data;
  struct sophia_pq_event v;
  unsigned kind;
  if (!out || !header(b, n, 1))
    return -1;
  memset(&v, 0, sizeof v);
  v.epoch = get(b + 8, 8);
  v.sequence = get(b + 24, 8);
  kind = (unsigned)get(b + 6, 2);
  b += 32;
  n -= 32;
  v.kind = (enum sophia_pq_kind)kind;
  switch (kind) {
  case 16:
    if (n != 16 || get(b, 2) != 1 || !zero(b + 2, 6) ||
        get(b + 8, 8) != v.epoch)
      return -1;
    break;
  case 17:
    if (n != 8 || get(b, 2) != 1 || !zero(b + 2, 6))
      return -1;
    break;
  case 18:
    if (n != 16 || !get(b, 8) || get(b + 8, 2) < 256 || get(b + 8, 2) > 260 ||
        !zero(b + 10, 6))
      return -1;
    v.data.submitted.submission = get(b, 8);
    v.data.submitted.candidate_kind = (enum sophia_pq_kind)get(b + 8, 2);
    break;
  case 32: {
    struct sophia_pq_grant *g = &v.data.decision.grant;
    if (n != 56)
      return -1;
    g->transfer = get(b, 8);
    if (b[8] == 1) {
      if (!zero(b + 9, 47))
        return -1;
      break;
    }
    if (b[8] != 2 || b[9] != 1 || get(b + 10, 2) < 1 || get(b + 10, 2) > 7 ||
        !zero(b + 12, 4))
      return -1;
    v.data.decision.allowed = 1;
    g->kind = (enum sophia_pq_transfer_kind)get(b + 10, 2);
    g->source = get(b + 16, 8);
    g->target = get(b + 24, 8);
    g->source_generation = get(b + 32, 8);
    g->broker_generation = get(b + 40, 8);
    g->deadline_msec = get(b + 48, 8);
    if (!g->transfer || !g->source || !g->target || g->source == g->target ||
        !g->source_generation || !g->broker_generation || !g->deadline_msec)
      return -1;
    break;
  }
  case 33:
    if (n != 16 || !get(b, 8) || b[8] < 2 || b[8] > 4 || !zero(b + 9, 7))
      return -1;
    v.data.outcome.transfer = get(b, 8);
    v.data.outcome.state = (enum sophia_pq_outcome)b[8];
    break;
  default:
    return -1;
  }
  *out = v;
  return 0;
}
int sophia_pq_encode_candidate(uint64_t epoch, uint64_t submission,
                               const struct sophia_pq_candidate *c, void *out,
                               size_t capacity, size_t *length) {
  unsigned char record[344] = {0}, *b = record + 32;
  size_t n;
  if (!epoch || !submission || !c || !out || !length)
    return -1;
  switch (c->kind) {
  case SOPHIA_PQ_NEGOTIATE:
  case SOPHIA_PQ_CANCEL:
    n = 40;
    break;
  case SOPHIA_PQ_REQUEST:
    n = 344;
    break;
  case SOPHIA_PQ_BEGIN:
    n = 56;
    break;
  case SOPHIA_PQ_END:
    n = 48;
    break;
  default:
    return -1;
  }
  if (capacity < n)
    return -1;
  put(record, n, 4);
  put(record + 4, 1, 2);
  put(record + 6, (unsigned)c->kind, 2);
  put(record + 8, epoch, 8);
  put(record + 16, submission, 8);
  switch (c->kind) {
  case SOPHIA_PQ_NEGOTIATE:
    put(b, c->data.negotiate.minimum, 2);
    put(b + 2, c->data.negotiate.maximum, 2);
    break;
  case SOPHIA_PQ_REQUEST: {
    const struct sophia_pq_request *r = &c->data.request;
    if (!r->source || !r->target || r->source == r->target || r->kind < 1 ||
        r->kind > 7 || r->mime_present > 1 || r->mime_length > 255 ||
        (!r->mime_present && r->mime_length) ||
        (r->mime_present && !utf8(r->mime, r->mime_length)))
      return -1;
    put(b, r->transfer, 8);
    put(b + 8, r->source, 8);
    put(b + 16, r->target, 8);
    put(b + 24, (unsigned)r->kind, 2);
    b[26] = (unsigned char)r->mime_present;
    put(b + 28, r->mime_length, 2);
    put(b + 32, r->byte_size, 8);
    put(b + 40, r->generation, 8);
    put(b + 48, r->deadline_msec, 8);
    if (r->mime_length)
      memcpy(b + 56, r->mime, r->mime_length);
    break;
  }
  case SOPHIA_PQ_BEGIN:
  case SOPHIA_PQ_END:
    if (!c->data.payload.transfer || c->data.payload.total_bytes > 65536)
      return -1;
    put(b, c->data.payload.transfer, 8);
    put(b + (c->kind == SOPHIA_PQ_BEGIN ? 16 : 8), c->data.payload.total_bytes,
        8);
    break;
  case SOPHIA_PQ_CANCEL:
    if (!c->data.cancel)
      return -1;
    put(b, c->data.cancel, 8);
    break;
  default:
    return -1;
  }
  memcpy(out, record, n);
  *length = n;
  return 0;
}
int sophia_pq_encode_submit(uint64_t epoch, uint64_t submission, uint32_t bytes,
                            unsigned char out[24]) {
  unsigned char b[24] = {0};
  if (!epoch || !submission || !out || bytes < 40 || bytes > 344)
    return -1;
  put(b, epoch, 8);
  put(b + 8, submission, 8);
  put(b + 16, bytes, 4);
  memcpy(out, b, 24);
  return 0;
}
int sophia_pq_encode_ack(uint64_t epoch, uint64_t sequence,
                         unsigned char out[16]) {
  unsigned char b[16];
  if (!epoch || !sequence || !out)
    return -1;
  put(b, epoch, 8);
  put(b + 8, sequence, 8);
  memcpy(out, b, 16);
  return 0;
}
