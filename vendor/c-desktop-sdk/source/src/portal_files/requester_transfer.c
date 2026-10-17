#include "../sophia_portal_requester_transfer.h"
#include <string.h>
int sophia_pqf_init(struct sophia_pqf *t, const struct sophia_pq_request *r,
                    uint64_t broker_generation) {
  struct sophia_pqf v;
  struct sophia_pq_candidate c = {0};
  unsigned char scratch[344];
  size_t length;
  if (!t || !r || !broker_generation)
    return SOPHIA_PQS_INVALID;
  c.kind = SOPHIA_PQ_REQUEST;
  c.data.request = *r;
  if (sophia_pq_encode_candidate(1, 1, &c, scratch, sizeof scratch, &length))
    return SOPHIA_PQS_INVALID;
  memset(&v, 0, sizeof v);
  v.expected.transfer = r->transfer;
  v.expected.kind = r->kind;
  v.expected.source = r->source;
  v.expected.target = r->target;
  v.expected.source_generation = r->generation;
  v.expected.broker_generation = broker_generation;
  v.expected.deadline_msec = r->deadline_msec;
  *t = v;
  return 0;
}
static int equal(const struct sophia_pq_grant *a,
                 const struct sophia_pq_grant *b) {
  return a->transfer == b->transfer && a->kind == b->kind &&
         a->source == b->source && a->target == b->target &&
         a->source_generation == b->source_generation &&
         a->broker_generation == b->broker_generation &&
         a->deadline_msec == b->deadline_msec;
}
int sophia_pqf_decision(struct sophia_pqf *t, const struct sophia_pq_event *v) {
  if (!t || !v)
    return SOPHIA_PQS_INVALID;
  if (t->closed)
    return SOPHIA_PQS_CLOSED;
  if (t->state != SOPHIA_PQF_PENDING || v->kind != SOPHIA_PQ_DECISION)
    return SOPHIA_PQS_PROTOCOL;
  if (!v->data.decision.allowed &&
      v->data.decision.grant.transfer == t->expected.transfer) {
    t->state = SOPHIA_PQF_DENIED;
    return 0;
  }
  if (v->data.decision.allowed == 1 &&
      equal(&v->data.decision.grant, &t->expected) && t->expected.transfer &&
      t->expected.source_generation && t->expected.deadline_msec) {
    t->state = SOPHIA_PQF_ALLOWED;
    return 0;
  }
  return SOPHIA_PQS_PROTOCOL;
}
int sophia_pqf_end_issued(struct sophia_pqf *t) {
  if (!t)
    return SOPHIA_PQS_INVALID;
  if (t->closed)
    return SOPHIA_PQS_CLOSED;
  if (t->state != SOPHIA_PQF_ALLOWED)
    return SOPHIA_PQS_BUSY;
  t->state = SOPHIA_PQF_END_ISSUED;
  return 0;
}
int sophia_pqf_outcome(struct sophia_pqf *t, uint64_t id,
                       enum sophia_pq_outcome outcome) {
  enum sophia_pqf_state next;
  if (!t)
    return SOPHIA_PQS_INVALID;
  if (t->closed)
    return SOPHIA_PQS_CLOSED;
  if (id != t->expected.transfer ||
      (t->state != SOPHIA_PQF_ALLOWED && t->state != SOPHIA_PQF_END_ISSUED))
    return SOPHIA_PQS_PROTOCOL;
  switch (outcome) {
  case SOPHIA_PQ_COMPLETED:
    if (t->state != SOPHIA_PQF_END_ISSUED)
      return SOPHIA_PQS_PROTOCOL;
    next = SOPHIA_PQF_COMPLETED;
    break;
  case SOPHIA_PQ_REVOKED:
    next = SOPHIA_PQF_REVOKED;
    break;
  case SOPHIA_PQ_EXPIRED:
    next = SOPHIA_PQF_EXPIRED;
    break;
  default:
    return SOPHIA_PQS_PROTOCOL;
  }
  t->state = next;
  return 0;
}
const struct sophia_pq_grant *sophia_pqf_grant(const struct sophia_pqf *t) {
  return t && !t->closed &&
                 (t->state == SOPHIA_PQF_ALLOWED ||
                  t->state == SOPHIA_PQF_END_ISSUED)
             ? &t->expected
             : NULL;
}
void sophia_pqf_close(struct sophia_pqf *t) {
  if (t)
    t->closed = 1;
}
