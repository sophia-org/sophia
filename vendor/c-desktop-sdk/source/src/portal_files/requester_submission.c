#include "../sophia_portal_requester_state.h"
#include <string.h>
static int fail(struct sophia_pqs_submission *s, enum sophia_pqs_error e) {
  if (!s->error) {
    s->error = e;
    if (s->custody == SOPHIA_PQS_QUEUED)
      s->custody = SOPHIA_PQS_DROPPED_UNSENT;
    else if (s->custody == SOPHIA_PQS_ISSUED ||
             s->custody == SOPHIA_PQS_WRITE_ACCEPTED)
      s->custody = SOPHIA_PQS_UNKNOWN;
  }
  return s->error;
}
int sophia_pqs_init(struct sophia_pqs_submission *s, uint64_t epoch,
                    uint64_t id, const struct sophia_pq_candidate *c,
                    uint64_t now, uint64_t deadline) {
  struct sophia_pqs_submission v;
  if (!s || !c)
    return SOPHIA_PQS_INVALID;
  if (deadline <= now)
    return SOPHIA_PQS_DEADLINE;
  memset(&v, 0, sizeof v);
  if (sophia_pq_encode_candidate(epoch, id, c, v.bytes, sizeof v.bytes,
                                 &v.length) ||
      sophia_pq_encode_submit(epoch, id, (uint32_t)v.length, v.control))
    return SOPHIA_PQS_INVALID;
  v.epoch = epoch;
  v.id = id;
  v.kind = c->kind;
  v.now = now;
  v.deadline = deadline;
  *s = v;
  return 0;
}
int sophia_pqs_advance(struct sophia_pqs_submission *s, uint64_t now) {
  if (!s)
    return SOPHIA_PQS_INVALID;
  if (now < s->now)
    return SOPHIA_PQS_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if ((s->custody != SOPHIA_PQS_SUBMITTED || s->inflight) && now >= s->deadline)
    return fail(s, SOPHIA_PQS_DEADLINE);
  return 0;
}
void sophia_pqs_close(struct sophia_pqs_submission *s) {
  if (s)
    fail(s, SOPHIA_PQS_CLOSED);
}
int sophia_pqs_issued(struct sophia_pqs_submission *s, uint64_t now) {
  int e = sophia_pqs_advance(s, now);
  if (e)
    return e;
  if (s->custody != SOPHIA_PQS_QUEUED || s->inflight)
    return SOPHIA_PQS_BUSY;
  s->custody = SOPHIA_PQS_ISSUED;
  s->inflight = 1;
  return 0;
}
int sophia_pqs_reply(struct sophia_pqs_submission *s, uint32_t count,
                     uint32_t error, uint64_t now) {
  int e = sophia_pqs_advance(s, now);
  if (e)
    return e;
  if (!s->inflight)
    return SOPHIA_PQS_BUSY;
  s->inflight = 0;
  if (!error && count == 24) {
    if (!s->receipt)
      s->custody = SOPHIA_PQS_WRITE_ACCEPTED;
    return 0;
  }
  if (error && !s->receipt) {
    if (error == 11) {
      s->custody = SOPHIA_PQS_QUEUED;
      return 0;
    }
    s->custody = SOPHIA_PQS_REFUSED;
    s->remote_error = error;
    return fail(s, SOPHIA_PQS_REMOTE);
  }
  return fail(s, SOPHIA_PQS_PROTOCOL);
}
int sophia_pqs_receipt(struct sophia_pqs_submission *s,
                       const struct sophia_pq_event *v, uint64_t now) {
  int e = sophia_pqs_advance(s, now);
  if (e)
    return e;
  if (!v)
    return SOPHIA_PQS_INVALID;
  if (v->epoch != s->epoch || !v->sequence || s->receipt ||
      (s->custody != SOPHIA_PQS_ISSUED &&
       s->custody != SOPHIA_PQS_WRITE_ACCEPTED) ||
      v->kind != SOPHIA_PQ_SUBMITTED || v->data.submitted.submission != s->id ||
      v->data.submitted.candidate_kind != s->kind)
    return fail(s, SOPHIA_PQS_PROTOCOL);
  s->receipt = v->sequence;
  s->custody = SOPHIA_PQS_SUBMITTED;
  return 0;
}
int sophia_pqs_releasable(const struct sophia_pqs_submission *s,
                          uint64_t acknowledged) {
  return s && !s->error && !s->inflight && s->receipt &&
         acknowledged >= s->receipt;
}
