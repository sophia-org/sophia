#include "../sophia_broker_files.h"
#include "internal.h"
#include <string.h>
static int valid(const struct sophia_bf_row *r) {
  int no_grant = !r->grant_token && !r->grant_revocation_epoch &&
                 !r->grant_target_generation;
  int no_text = !r->label_present && !r->redacted && !r->label_length;
  if (r->icon_present > 1 || r->redacted > 1 || r->label_present > 1 ||
      (!r->icon_present && r->icon) || r->label_length > 128 ||
      (!r->label_present && r->label_length) ||
      (r->label_length &&
       (!r->label ||
        !sophia_bf_utf8((const unsigned char *)r->label, r->label_length))))
    return 0;
  switch (r->kind) {
  case SOPHIA_BF_PUBLISH_RULE:
    return !r->value && r->disclosure >= 1 && r->disclosure <= 3 &&
           r->trust >= 1 && r->trust <= 4 && !r->attention && no_text &&
           no_grant;
  case SOPHIA_BF_EMIT_DESCRIPTOR:
    return !r->value && !r->disclosure && r->trust >= 1 && r->trust <= 4 &&
           r->attention >= 1 && r->attention <= 3 && r->generation &&
           r->grant_token && r->grant_revocation_epoch &&
           r->grant_target_generation == r->generation &&
           (!r->redacted || r->label_present);
  case SOPHIA_BF_RETIRE_SURFACE:
  case SOPHIA_BF_REJECTED:
  case SOPHIA_BF_NO_CHANGE:
    return !r->disclosure && !r->trust && !r->attention && !r->icon_present &&
           no_text && no_grant && !r->generation &&
           (r->kind == SOPHIA_BF_RETIRE_SURFACE
                ? !r->value
                : !r->surface_index && !r->surface_generation &&
                      (r->kind == SOPHIA_BF_REJECTED
                           ? r->value >= 1 && r->value <= 5
                           : !r->value));
  default:
    return 0;
  }
}
static int correlated(const struct sophia_bf_request *q,
                      const struct sophia_bf_row *r, size_t n) {
  size_t i;
  if (!q || !r || !q->id || q->kind < 1 || q->kind > 5 || n < 1 || n > 2)
    return 0;
  for (i = 0; i < n; ++i) {
    if (!valid(r + i))
      return 0;
    if (r[i].kind <= SOPHIA_BF_RETIRE_SURFACE &&
        (r[i].surface_index != q->surface_index ||
         r[i].surface_generation != q->surface_generation))
      return 0;
  }
  if (r->kind == SOPHIA_BF_REJECTED)
    return n == 1 &&
           (r->value == 5 ||
            (q->kind == 1   ? r->value == 1 || r->value == 3
             : q->kind == 2 ? r->value == 1 || r->value == 2 || r->value == 4
                            : r->value == 1));
  switch (q->kind) {
  case SOPHIA_BF_SURFACE_ADMITTED:
  case SOPHIA_BF_SET_DISCLOSURE:
    return r->kind == SOPHIA_BF_PUBLISH_RULE &&
           (q->kind != SOPHIA_BF_SET_DISCLOSURE || r->disclosure == q->value) &&
           (n == 1 ||
            (r[1].kind == SOPHIA_BF_EMIT_DESCRIPTOR && !r[1].label_present &&
             !r[1].redacted &&
             (q->kind != SOPHIA_BF_SURFACE_ADMITTED || r->disclosure == 2)));
  case SOPHIA_BF_CANDIDATE_REDUCED:
    return n == 1 && (r->kind == SOPHIA_BF_NO_CHANGE ||
                      (r->kind == SOPHIA_BF_EMIT_DESCRIPTOR &&
                       r->generation == q->candidate_generation));
  case SOPHIA_BF_ATTENTION_CHANGED:
    return n == 1 &&
           (r->kind == SOPHIA_BF_NO_CHANGE ||
            (r->kind == SOPHIA_BF_EMIT_DESCRIPTOR && r->attention == q->value));
  case SOPHIA_BF_SURFACE_REMOVED:
    return n == 1 && r->kind == SOPHIA_BF_RETIRE_SURFACE;
  default:
    return 0;
  }
}
static void put(unsigned char *b, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    b[i] = (unsigned char)(v >> (8 * i));
}
int sophia_bf_encode_response(uint64_t epoch, uint64_t submission,
                              const struct sophia_bf_request *q,
                              const struct sophia_bf_row *rows, size_t n,
                              void *output, size_t capacity) {
  unsigned char bytes[448] = {0};
  size_t i;
  if (!epoch || !submission || !output || capacity < 448 ||
      !correlated(q, rows, n))
    return -1;
  put(bytes, 448, 4);
  put(bytes + 4, 1, 2);
  put(bytes + 6, 257, 2);
  put(bytes + 8, epoch, 8);
  put(bytes + 16, submission, 8);
  put(bytes + 32, q->id, 8);
  put(bytes + 40, q->kind, 2);
  put(bytes + 42, n, 2);
  for (i = 0; i < n; ++i) {
    const struct sophia_bf_row *r = rows + i;
    unsigned char *b = bytes + 48 + 200 * i;
    put(b, r->kind, 2);
    put(b + 2, r->value, 2);
    put(b + 8, r->surface_index, 4);
    put(b + 12, r->surface_generation, 4);
    put(b + 16, r->disclosure, 2);
    put(b + 18, r->trust, 2);
    put(b + 20, r->attention, 2);
    b[22] = (unsigned char)r->icon_present;
    b[23] = (unsigned char)r->redacted;
    put(b + 24, r->icon, 8);
    put(b + 32, r->generation, 8);
    put(b + 40, r->grant_token, 8);
    put(b + 48, r->grant_revocation_epoch, 8);
    put(b + 56, r->grant_target_generation, 8);
    b[64] = (unsigned char)r->label_present;
    put(b + 66, r->label_length, 2);
    if (r->label_length)
      memcpy(b + 72, r->label, r->label_length);
  }
  memcpy(output, bytes, sizeof bytes);
  return 0;
}
