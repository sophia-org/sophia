#include "sophia_portal_requester_files.h"
#include <assert.h>
#include <string.h>
static void put(unsigned char *b, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    b[i] = (unsigned char)(v >> (i * 8));
}
static void record(unsigned char *b, unsigned kind, size_t n,
                   uint64_t submission, uint64_t sequence) {
  memset(b, 0, n);
  put(b, n, 4);
  put(b + 4, 1, 2);
  put(b + 6, kind, 2);
  put(b + 8, 7, 8);
  put(b + 16, submission, 8);
  put(b + 24, sequence, 8);
}
static void candidates(void) {
  unsigned char out[345], expected[344], before[345], control[24],
      bad_utf8[] = {0xc0, 0x80};
  char mime[256];
  struct sophia_pq_candidate c;
  size_t length = 999, i, m;
  unsigned kind;
  memset(out, 0xa5, sizeof out);
  memset(&c, 0, sizeof c);
  c.kind = SOPHIA_PQ_NEGOTIATE;
  c.data.negotiate.minimum = 2;
  assert(!sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
         length == 40);
  record(expected, 256, 40, 9, 0);
  expected[32] = 2;
  assert(!memcmp(out, expected, 40) && out[40] == 0xa5);
  for (i = 0; i < 254; i += 2) {
    mime[i] = (char)0xc3;
    mime[i + 1] = (char)0xa9;
  }
  mime[254] = 'x';
  mime[255] = 'x';
  for (kind = 1; kind <= 7; ++kind)
    for (m = 0; m < 4; ++m) {
      memset(&c, 0, sizeof c);
      c.kind = SOPHIA_PQ_REQUEST;
      c.data.request.source = 2;
      c.data.request.target = 3;
      c.data.request.kind = (enum sophia_pq_transfer_kind)kind;
      c.data.request.byte_size = UINT64_MAX;
      c.data.request.mime_present = m != 0;
      c.data.request.mime = m == 2 ? "text/plain" : mime;
      c.data.request.mime_length = m == 3 ? 255 : m == 2 ? 10 : 0;
      assert(!sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
             length == 344);
      record(expected, 257, 344, 9, 0);
      put(expected + 40, 2, 8);
      put(expected + 48, 3, 8);
      put(expected + 56, kind, 2);
      expected[58] = (unsigned char)(m != 0);
      put(expected + 60, c.data.request.mime_length, 2);
      put(expected + 64, UINT64_MAX, 8);
      memcpy(expected + 88, c.data.request.mime, c.data.request.mime_length);
      assert(!memcmp(out, expected, 344) && out[344] == 0xa5);
    }
  memcpy(before, out, sizeof out);
  c.data.request.mime_length = 256;
  length = 999;
  assert(sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
         length == 999 && !memcmp(out, before, sizeof out));
  c.data.request.mime = (const char *)bad_utf8;
  c.data.request.mime_length = 2;
  assert(sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
         !memcmp(out, before, sizeof out));
  {
    const unsigned char invalid[][4] = {{0xed, 0xa0, 0x80, 0},
                                        {0xf4, 0x90, 0x80, 0x80},
                                        {0xe2, 0x82, 0, 0},
                                        {0xff, 0, 0, 0}};
    const size_t sizes[] = {3, 4, 2, 1};
    for (i = 0; i < 4; ++i) {
      c.data.request.mime = (const char *)invalid[i];
      c.data.request.mime_length = sizes[i];
      assert(sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
             !memcmp(out, before, sizeof out));
    }
  }
  c.data.request.mime_length = 0;
  c.data.request.target = 2;
  assert(sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
         !memcmp(out, before, sizeof out));
  for (kind = 258; kind <= 260; ++kind) {
    size_t n = kind == 258 ? 56 : kind == 259 ? 48 : 40;
    memset(&c, 0, sizeof c);
    c.kind = (enum sophia_pq_kind)kind;
    if (kind == 260)
      c.data.cancel = 5;
    else {
      c.data.payload.transfer = 5;
      c.data.payload.total_bytes = 65536;
    }
    assert(!sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
           length == n);
    record(expected, kind, n, 9, 0);
    put(expected + 32, 5, 8);
    if (kind != 260)
      put(expected + (kind == 258 ? 48 : 40), 65536, 8);
    assert(!memcmp(out, expected, n));
    memcpy(before, out, sizeof out);
    length = 999;
    assert(sophia_pq_encode_candidate(0, 9, &c, out, sizeof out, &length) &&
           length == 999 && !memcmp(out, before, sizeof out));
    assert(sophia_pq_encode_candidate(7, 0, &c, out, sizeof out, &length) &&
           !memcmp(out, before, sizeof out));
    assert(sophia_pq_encode_candidate(7, 9, &c, out, n - 1, &length) &&
           !memcmp(out, before, sizeof out));
    if (kind == 260)
      c.data.cancel = 0;
    else
      c.data.payload.transfer = 0;
    assert(sophia_pq_encode_candidate(7, 9, &c, out, sizeof out, &length) &&
           !memcmp(out, before, sizeof out));
  }
  memset(expected, 0, 24);
  put(expected, 7, 8);
  put(expected + 8, 9, 8);
  put(expected + 16, 344, 4);
  assert(!sophia_pq_encode_submit(7, 9, 344, control) &&
         !memcmp(control, expected, 24));
  assert(sophia_pq_encode_submit(7, 9, 345, control) &&
         !memcmp(control, expected, 24));
  put(expected + 8, 11, 8);
  assert(!sophia_pq_encode_ack(7, 11, control) &&
         !memcmp(control, expected, 16));
  assert(sophia_pq_encode_ack(7, 0, control) && !memcmp(control, expected, 16));
}
static void limits(void) {
  unsigned char b[80], bad[80];
  uint64_t epoch;
  struct sophia_pq_limits out, before;
  const char *api = "sophia-portal-files version=1 role=portal-requester "
                    "epoch=18446744073709551615 fd_transfer=none\n";
  const unsigned offsets[] = {36, 40, 48, 52, 56, 60, 64, 68};
  const uint32_t values[] = {64, 65536, 256, 32768, 12000, 2000, 344, 4096};
  const uint32_t invalid[] = {65, 0, 3, 279, 12001, 2001, 345, 63};
  size_t i;
  assert(!sophia_pq_decode_api(api, strlen(api), &epoch) &&
         epoch == UINT64_MAX);
  assert(sophia_pq_decode_api(api, strlen(api) - 1, &epoch) &&
         epoch == UINT64_MAX);
  {
    const char *bad_api[] = {
        "sophia-portal-files version=1 role=portal-requester epoch=0 "
        "fd_transfer=none\n",
        "sophia-portal-files version=1 role=portal-requester epoch=01 "
        "fd_transfer=none\n",
        "sophia-portal-files version=1 role=portal-requester "
        "epoch=18446744073709551616 fd_transfer=none\n",
        "sophia-portal-files version=1 role=portal-requester epoch=-1 "
        "fd_transfer=none\n"};
    for (i = 0; i < 4; ++i)
      assert(sophia_pq_decode_api(bad_api[i], strlen(bad_api[i]), &epoch) &&
             epoch == UINT64_MAX);
  }
  record(b, 1, 80, 0, 0);
  b[32] = 1;
  b[34] = 1;
  b[44] = 255;
  put(b + 72, 17, 8);
  for (i = 0; i < 8; ++i)
    put(b + offsets[i], values[i], 4);
  assert(!sophia_pq_decode_limits(b, 80, &epoch, &out) && epoch == 7 &&
         out.broker_generation == 17);
  before = out;
  for (i = 0; i < 80; ++i) {
    epoch = 99;
    assert(sophia_pq_decode_limits(b, i, &epoch, &out) && epoch == 99 &&
           !memcmp(&out, &before, sizeof out));
  }
  for (i = 0; i < 8; ++i) {
    memcpy(bad, b, 80);
    put(bad + offsets[i], invalid[i], 4);
    assert(sophia_pq_decode_limits(bad, 80, &epoch, &out) &&
           !memcmp(&out, &before, sizeof out));
  }
}
static void check_event(unsigned char *b, size_t n, unsigned kind) {
  struct sophia_pq_event out, before;
  unsigned char bad[89];
  size_t i;
  assert(!sophia_pq_decode_event(b, n, &out) &&
         out.kind == (enum sophia_pq_kind)kind && out.epoch == 7 &&
         out.sequence == 1);
  before = out;
  for (i = 0; i < n; ++i)
    assert(sophia_pq_decode_event(b, i, &out) &&
           !memcmp(&out, &before, sizeof out));
  memcpy(bad, b, n);
  bad[n] = 0;
  assert(sophia_pq_decode_event(bad, n + 1, &out) &&
         !memcmp(&out, &before, sizeof out));
  for (i = 8; i <= 24; i += 8) {
    memcpy(bad, b, n);
    put(bad + i, i == 16 ? 1 : 0, 8);
    assert(sophia_pq_decode_event(bad, n, &out) &&
           !memcmp(&out, &before, sizeof out));
  }
}
static void events(void) {
  unsigned char b[88], bad[88];
  struct sophia_pq_event out;
  size_t i;
  unsigned state;
  record(b, 16, 48, 0, 1);
  b[32] = 1;
  put(b + 40, 7, 8);
  check_event(b, 48, 16);
  record(b, 17, 40, 0, 1);
  b[32] = 1;
  check_event(b, 40, 17);
  record(b, 18, 48, 0, 1);
  put(b + 32, 9, 8);
  put(b + 40, 257, 2);
  check_event(b, 48, 18);
  record(b, 32, 88, 0, 1);
  b[40] = 1;
  check_event(b, 88, 32);
  assert(!sophia_pq_decode_event(b, 88, &out) && !out.data.decision.allowed &&
         !out.data.decision.grant.transfer);
  for (i = 41; i < 88; ++i) {
    memcpy(bad, b, 88);
    bad[i] = 1;
    assert(sophia_pq_decode_event(bad, 88, &out));
  }
  b[40] = 2;
  b[41] = 1;
  b[42] = 1;
  put(b + 32, 5, 8);
  put(b + 48, 2, 8);
  put(b + 56, 3, 8);
  put(b + 64, 9, 8);
  put(b + 72, 17, 8);
  put(b + 80, 100, 8);
  check_event(b, 88, 32);
  assert(!sophia_pq_decode_event(b, 88, &out) && out.data.decision.allowed &&
         out.data.decision.grant.broker_generation == 17);
  for (i = 48; i < 88; i += 8) {
    memcpy(bad, b, 88);
    put(bad + i, 0, 8);
    assert(sophia_pq_decode_event(bad, 88, &out));
  }
  for (i = 44; i < 48; ++i) {
    memcpy(bad, b, 88);
    bad[i] = 1;
    assert(sophia_pq_decode_event(bad, 88, &out));
  }
  for (state = 2; state <= 4; ++state) {
    record(b, 33, 48, 0, 1);
    put(b + 32, 5, 8);
    b[40] = (unsigned char)state;
    check_event(b, 48, 33);
    assert(!sophia_pq_decode_event(b, 48, &out) &&
           out.data.outcome.state == (enum sophia_pq_outcome)state);
  }
}
int main(void) {
  candidates();
  limits();
  events();
  return 0;
}
