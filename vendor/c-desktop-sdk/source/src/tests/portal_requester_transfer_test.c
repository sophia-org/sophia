#include "sophia_portal_requester_transfer.h"
#include <assert.h>
#include <string.h>
static struct sophia_pq_request request(void) {
  struct sophia_pq_request r = {0};
  r.transfer = 11;
  r.source = 2;
  r.target = 3;
  r.kind = SOPHIA_PQ_CLIPBOARD;
  r.mime_present = 1;
  r.mime = "text/plain";
  r.mime_length = 10;
  r.byte_size = 123;
  r.generation = 5;
  r.deadline_msec = 900;
  return r;
}
static struct sophia_pq_event decision(void) {
  struct sophia_pq_event v = {0};
  v.kind = SOPHIA_PQ_DECISION;
  v.data.decision.allowed = 1;
  v.data.decision.grant.transfer = 11;
  v.data.decision.grant.source = 2;
  v.data.decision.grant.target = 3;
  v.data.decision.grant.kind = SOPHIA_PQ_CLIPBOARD;
  v.data.decision.grant.source_generation = 5;
  v.data.decision.grant.broker_generation = 17;
  v.data.decision.grant.deadline_msec = 900;
  return v;
}
int main(void) {
  struct sophia_pq_request r = request();
  struct sophia_pq_event v;
  struct sophia_pqf t;
  unsigned field, end;
  enum sophia_pq_outcome outcome;
  for (field = 0; field < 7; ++field) {
    assert(!sophia_pqf_init(&t, &r, 17));
    v = decision();
    switch (field) {
    case 0:
      ++v.data.decision.grant.transfer;
      break;
    case 1:
      ++v.data.decision.grant.source;
      break;
    case 2:
      ++v.data.decision.grant.target;
      break;
    case 3:
      v.data.decision.grant.kind = SOPHIA_PQ_SCREEN_CAPTURE;
      break;
    case 4:
      ++v.data.decision.grant.source_generation;
      break;
    case 5:
      ++v.data.decision.grant.broker_generation;
      break;
    default:
      ++v.data.decision.grant.deadline_msec;
      break;
    }
    assert(sophia_pqf_decision(&t, &v) == SOPHIA_PQS_PROTOCOL);
    assert(t.state == SOPHIA_PQF_PENDING && !sophia_pqf_grant(&t));
    v = decision();
    assert(!sophia_pqf_decision(&t, &v));
    assert(sophia_pqf_grant(&t));
    assert(sophia_pqf_decision(&t, &v) == SOPHIA_PQS_PROTOCOL &&
           t.state == SOPHIA_PQF_ALLOWED);
  }
  r.transfer = r.generation = r.deadline_msec = 0;
  assert(!sophia_pqf_init(&t, &r, 17));
  memset(&v, 0, sizeof v);
  v.kind = SOPHIA_PQ_DECISION;
  assert(!sophia_pqf_decision(&t, &v));
  assert(t.state == SOPHIA_PQF_DENIED);
  sophia_pqf_close(&t);
  assert(t.closed && t.state == SOPHIA_PQF_DENIED && !sophia_pqf_grant(&t));
  r = request();
  for (end = 0; end < 2; ++end) {
    assert(!sophia_pqf_init(&t, &r, 17));
    v = decision();
    assert(!sophia_pqf_decision(&t, &v));
    if (end)
      assert(!sophia_pqf_end_issued(&t));
    sophia_pqf_close(&t);
    assert(!sophia_pqf_grant(&t));
    assert(t.state == (end ? SOPHIA_PQF_END_ISSUED : SOPHIA_PQF_ALLOWED));
    assert(sophia_pqf_outcome(&t, 11, SOPHIA_PQ_COMPLETED) ==
           SOPHIA_PQS_CLOSED);
  }
  for (outcome = SOPHIA_PQ_COMPLETED; outcome <= SOPHIA_PQ_EXPIRED;
       outcome = (enum sophia_pq_outcome)(outcome + 1)) {
    assert(!sophia_pqf_init(&t, &r, 17));
    v = decision();
    assert(!sophia_pqf_decision(&t, &v));
    assert(sophia_pqf_outcome(&t, 11, SOPHIA_PQ_COMPLETED) ==
           SOPHIA_PQS_PROTOCOL);
    assert(sophia_pqf_outcome(&t, 12, SOPHIA_PQ_REVOKED) ==
           SOPHIA_PQS_PROTOCOL);
    if (outcome == SOPHIA_PQ_COMPLETED) {
      assert(!sophia_pqf_end_issued(&t));
      assert(sophia_pqf_end_issued(&t) == SOPHIA_PQS_BUSY);
    }
    assert(!sophia_pqf_outcome(&t, 11, outcome));
    assert(!sophia_pqf_grant(&t));
    assert(t.state == (outcome == SOPHIA_PQ_COMPLETED ? SOPHIA_PQF_COMPLETED
                       : outcome == SOPHIA_PQ_REVOKED ? SOPHIA_PQF_REVOKED
                                                      : SOPHIA_PQF_EXPIRED));
    assert(sophia_pqf_outcome(&t, 11, outcome) == SOPHIA_PQS_PROTOCOL);
    assert(sophia_pqf_end_issued(&t) == SOPHIA_PQS_BUSY);
  }
  return 0;
}
