#include "../sophia_broker_phase.h"
static int fail(struct sophia_bp *s, int e) {
  if (!s->error)
    s->error = e;
  return s->error;
}
int sophia_bp_init(struct sophia_bp *s, uint64_t epoch,
                   const struct sophia_bf_limits *limits, uint64_t id,
                   uint64_t now) {
  struct sophia_bp v = {0};
  if (!s || !limits || !epoch || !id)
    return SOPHIA_BS_INVALID;
  v.limits = *limits;
  v.epoch = epoch;
  v.expected = id;
  v.candidate_kind = 256;
  v.now = now;
  *s = v;
  return 0;
}
int sophia_bp_advance(struct sophia_bp *s, uint64_t now) {
  if (!s)
    return SOPHIA_BS_INVALID;
  if (now < s->now)
    return SOPHIA_BS_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if (s->refusal_sequence && now >= s->refusal_due)
    return fail(s, SOPHIA_BS_DEADLINE);
  return 0;
}
int sophia_bp_response(struct sophia_bp *s, uint64_t request, uint64_t id,
                       uint64_t now) {
  int e = sophia_bp_advance(s, now);
  if (e)
    return e;
  if (!id)
    return SOPHIA_BS_INVALID;
  if (s->phase != SOPHIA_BP_READY || !s->pending || s->pending != request ||
      s->expected)
    return SOPHIA_BS_BUSY;
  s->expected = id;
  s->candidate_kind = 257;
  s->phase = SOPHIA_BP_RESPONDING;
  return 0;
}
int sophia_bp_event(struct sophia_bp *s, const struct sophia_bf_event *v,
                    uint64_t now) {
  int e = sophia_bp_advance(s, now);
  if (e)
    return e;
  if (!v)
    return SOPHIA_BS_INVALID;
  if (v->epoch != s->epoch || !v->sequence)
    return fail(s, SOPHIA_BS_PROTOCOL);
  switch (v->kind) {
  case SOPHIA_BF_SUBMITTED:
    if (!s->expected || s->expected != v->submission ||
        s->candidate_kind != v->candidate_kind)
      return fail(s, SOPHIA_BS_PROTOCOL);
    s->expected = 0;
    if (s->phase == SOPHIA_BP_NEGOTIATING)
      s->handshake_receipt = 1;
    else if (s->phase == SOPHIA_BP_RESPONDING) {
      s->pending = 0;
      s->phase = SOPHIA_BP_READY;
    } else
      return fail(s, SOPHIA_BS_PROTOCOL);
    break;
  case SOPHIA_BF_NEGOTIATED:
    if (s->phase != SOPHIA_BP_NEGOTIATING || !s->handshake_receipt ||
        v->max_surfaces != s->limits.max_surfaces ||
        v->max_label_bytes != s->limits.max_label_bytes)
      return fail(s, SOPHIA_BS_PROTOCOL);
    s->phase = SOPHIA_BP_READY;
    break;
  case SOPHIA_BF_REFUSED:
    if (s->phase != SOPHIA_BP_NEGOTIATING || !s->handshake_receipt)
      return fail(s, SOPHIA_BS_PROTOCOL);
    if (now > UINT64_MAX - s->limits.ack_progress_timeout_ms)
      return fail(s, SOPHIA_BS_OVERFLOW);
    s->phase = SOPHIA_BP_REFUSED;
    s->refusal_sequence = v->sequence;
    s->refusal_due = now + s->limits.ack_progress_timeout_ms;
    break;
  case SOPHIA_BF_REQUEST:
    if (s->phase != SOPHIA_BP_READY || s->pending ||
        v->request.id <= s->last_request)
      return fail(s, SOPHIA_BS_PROTOCOL);
    s->last_request = s->pending = v->request.id;
    break;
  default:
    return fail(s, SOPHIA_BS_PROTOCOL);
  }
  return 0;
}
int sophia_bp_refusal_acked(const struct sophia_bp *s, uint64_t ack) {
  return s && s->refusal_sequence && ack >= s->refusal_sequence;
}
