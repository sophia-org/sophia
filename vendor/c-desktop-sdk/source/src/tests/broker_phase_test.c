#include "sophia_broker_phase.h"
#include <assert.h>
int main(void) {
  struct sophia_bp s;
  struct sophia_bf_limits l = {1024, 128, 16, 4096, 512, 5000, 2000, 12000};
  struct sophia_bf_event e = {0};
  e.epoch = 7;
  e.sequence = 1;
  e.kind = SOPHIA_BF_NEGOTIATED;
  e.max_surfaces = 1024;
  e.max_label_bytes = 128;
  assert(!sophia_bp_init(&s, 7, &l, 1, 0));
  assert(sophia_bp_event(&s, &e, 1) == SOPHIA_BS_PROTOCOL);
  assert(!sophia_bp_init(&s, 7, &l, 1, 0));
  e.kind = SOPHIA_BF_SUBMITTED;
  e.submission = 1;
  e.candidate_kind = 256;
  assert(!sophia_bp_event(&s, &e, 1));
  e.kind = SOPHIA_BF_REFUSED;
  e.sequence = 2;
  assert(!sophia_bp_event(&s, &e, 2));
  assert(!sophia_bp_refusal_acked(&s, 1) && sophia_bp_refusal_acked(&s, 2));
  assert(s.refusal_due == 2002 &&
         sophia_bp_advance(&s, 2002) == SOPHIA_BS_DEADLINE);
  assert(!sophia_bp_init(&s, 7, &l, 1, 0));
  e.kind = SOPHIA_BF_SUBMITTED;
  e.sequence = 1;
  assert(!sophia_bp_event(&s, &e, 1));
  e.kind = SOPHIA_BF_NEGOTIATED;
  e.sequence = 2;
  assert(!sophia_bp_event(&s, &e, 2));
  e.kind = SOPHIA_BF_REQUEST;
  e.sequence = 3;
  e.request.id = 8;
  assert(!sophia_bp_event(&s, &e, 3));
  assert(sophia_bp_response(&s, 9, 2, 4) == SOPHIA_BS_BUSY);
  assert(!sophia_bp_response(&s, 8, 2, 4));
  e.kind = SOPHIA_BF_SUBMITTED;
  e.sequence = 4;
  e.submission = 2;
  e.candidate_kind = 257;
  assert(!sophia_bp_event(&s, &e, 5) && !s.pending);
  e.kind = SOPHIA_BF_REQUEST;
  e.sequence = 5;
  assert(sophia_bp_event(&s, &e, 6) == SOPHIA_BS_PROTOCOL);
  return 0;
}
