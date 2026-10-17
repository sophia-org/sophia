#include "sophia_broker_files.h"
#include <assert.h>
#include <string.h>
static struct sophia_bf_row descriptor(void) {
  struct sophia_bf_row r = {0};
  r.kind = SOPHIA_BF_EMIT_DESCRIPTOR;
  r.surface_index = 3;
  r.surface_generation = 4;
  r.trust = 1;
  r.attention = 2;
  r.generation = 8;
  r.grant_token = 12;
  r.grant_revocation_epoch = 1;
  r.grant_target_generation = 8;
  return r;
}
int main(void) {
  struct sophia_bf_request q = {0};
  struct sophia_bf_row rows[2] = {{0}}, rule;
  unsigned char out[448], sentinel[448];
  unsigned kind, reason, fault;
  size_t i;
  q.id = 9;
  q.surface_index = 3;
  q.surface_generation = 4;
  q.value = 2;
  q.candidate_generation = 8;
  rows[0].kind = SOPHIA_BF_PUBLISH_RULE;
  rows[0].surface_index = 3;
  rows[0].surface_generation = 4;
  rows[0].disclosure = 2;
  rows[0].trust = 1;
  rows[0].icon_present = 1;
  rows[0].generation = 7;
  rule = rows[0];
  memset(sentinel, 0xa5, sizeof sentinel);
  for (kind = 1; kind <= 5; kind += 4) {
    q.kind = (enum sophia_bf_request_kind)kind;
    rows[0] = rule;
    rows[1] = descriptor();
    assert(!sophia_bf_encode_response(7, 2, &q, rows, 2, out, sizeof out));
    assert(!memcmp(out, "\xc0\x01\x00\x00\x01\x00\x01\x01", 8));
    assert(out[32] == 9 && out[40] == kind && out[42] == 2 && out[48] == 1 &&
           out[248] == 2 && out[70] == 1);
    for (fault = 0; fault < 4; ++fault) {
      rows[1] = descriptor();
      if (fault == 0)
        rows[1].grant_token = 0;
      if (fault == 1)
        rows[1].surface_generation = 5;
      if (fault == 2)
        rows[1].label_present = 1;
      if (fault == 3)
        rows[1].grant_target_generation = 9;
      memcpy(out, sentinel, sizeof out);
      assert(sophia_bf_encode_response(7, 2, &q, rows, 2, out, sizeof out));
      assert(!memcmp(out, sentinel, sizeof out));
    }
    rows[0] = descriptor();
    rows[1] = rule;
    assert(sophia_bf_encode_response(7, 2, &q, rows, 2, out, sizeof out));
  }
  for (kind = 1; kind <= 5; ++kind) {
    q.kind = (enum sophia_bf_request_kind)kind;
    for (reason = 1; reason <= 5; ++reason) {
      int valid = reason == 5 ||
                  (kind == 1   ? reason == 1 || reason == 3
                   : kind == 2 ? reason == 1 || reason == 2 || reason == 4
                               : reason == 1);
      memset(rows, 0, sizeof rows);
      rows[0].kind = SOPHIA_BF_REJECTED;
      rows[0].value = (uint16_t)reason;
      assert((sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out) ==
              0) == valid);
    }
  }
  q.kind = SOPHIA_BF_CANDIDATE_REDUCED;
  for (fault = 0; fault < 4; ++fault) {
    const char *labels[] = {NULL, "", "\0\n", "\xc3\xa9"};
    const size_t lengths[] = {0, 0, 2, 2};
    rows[0] = descriptor();
    rows[0].label_present = fault != 0;
    rows[0].redacted = fault != 0;
    rows[0].label = labels[fault];
    rows[0].label_length = lengths[fault];
    assert(!sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out));
    assert(out[112] == (fault != 0));
    for (i = 248; i < 448; ++i)
      assert(!out[i]);
  }
  rows[0] = descriptor();
  rows[0].label_present = 1;
  rows[0].label = "\xff";
  rows[0].label_length = 1;
  assert(sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out));
  rows[0] = descriptor();
  rows[0].generation = 9;
  rows[0].grant_target_generation = 9;
  assert(sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out));
  q.kind = SOPHIA_BF_ATTENTION_CHANGED;
  rows[0] = descriptor();
  rows[0].attention = 1;
  assert(sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out));
  q.kind = SOPHIA_BF_SURFACE_REMOVED;
  memset(rows, 0, sizeof rows);
  rows[0].kind = SOPHIA_BF_RETIRE_SURFACE;
  rows[0].surface_index = 3;
  rows[0].surface_generation = 4;
  assert(!sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out));
  for (kind = 2; kind <= 3; ++kind) {
    q.kind = (enum sophia_bf_request_kind)kind;
    memset(rows, 0, sizeof rows);
    rows[0].kind = SOPHIA_BF_NO_CHANGE;
    assert(!sophia_bf_encode_response(7, 2, &q, rows, 1, out, sizeof out));
  }
  memcpy(out, sentinel, sizeof out);
  assert(sophia_bf_encode_response(0, 2, &q, rows, 1, out, sizeof out));
  assert(sophia_bf_encode_response(7, 0, &q, rows, 1, out, sizeof out));
  assert(sophia_bf_encode_response(7, 2, &q, rows, 1, out, 447));
  assert(!memcmp(out, sentinel, sizeof out));
  return 0;
}
