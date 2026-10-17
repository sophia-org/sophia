#include "../sophia_broker_submission.h"
#include <string.h>
static int finish(struct sophia_bs_submission *out,
                  struct sophia_bs_submission *s, uint64_t epoch, uint64_t id,
                  size_t length, uint16_t kind, uint64_t now,
                  uint64_t deadline) {
  if (sophia_bf_encode_submit(epoch, id, (uint32_t)length, s->control, 24))
    return SOPHIA_BS_INVALID;
  s->epoch = epoch;
  s->id = id;
  s->length = length;
  s->kind = kind;
  s->now = now;
  s->deadline = deadline;
  *out = *s;
  return 0;
}
int sophia_bs_negotiate(struct sophia_bs_submission *out, uint64_t epoch,
                        uint64_t id, uint16_t minimum, uint16_t maximum,
                        uint64_t now, uint64_t deadline) {
  struct sophia_bs_submission s;
  if (!out)
    return SOPHIA_BS_INVALID;
  if (deadline <= now)
    return SOPHIA_BS_DEADLINE;
  memset(&s, 0, sizeof s);
  if (sophia_bf_encode_negotiate(epoch, id, minimum, maximum, s.bytes, 448))
    return SOPHIA_BS_INVALID;
  return finish(out, &s, epoch, id, 40, 256, now, deadline);
}
int sophia_bs_response(struct sophia_bs_submission *out, uint64_t epoch,
                       uint64_t id, const struct sophia_bf_request *q,
                       const struct sophia_bf_row *rows, size_t n, uint64_t now,
                       uint64_t deadline) {
  struct sophia_bs_submission s;
  if (!out)
    return SOPHIA_BS_INVALID;
  if (deadline <= now)
    return SOPHIA_BS_DEADLINE;
  memset(&s, 0, sizeof s);
  if (sophia_bf_encode_response(epoch, id, q, rows, n, s.bytes, 448))
    return SOPHIA_BS_INVALID;
  return finish(out, &s, epoch, id, 448, 257, now, deadline);
}
static int fail(struct sophia_bs_submission *s, enum sophia_bs_error e) {
  if (!s->error) {
    s->error = e;
    if (s->custody == SOPHIA_BS_QUEUED)
      s->custody = SOPHIA_BS_DROPPED_UNSENT;
    else if (s->custody == SOPHIA_BS_ISSUED ||
             s->custody == SOPHIA_BS_WRITE_ACCEPTED)
      s->custody = SOPHIA_BS_UNKNOWN;
  }
  return s->error;
}
int sophia_bs_advance(struct sophia_bs_submission *s, uint64_t now) {
  if (!s)
    return SOPHIA_BS_INVALID;
  if (now < s->now)
    return SOPHIA_BS_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if ((s->custody != SOPHIA_BS_SUBMITTED || s->inflight) && now >= s->deadline)
    return fail(s, SOPHIA_BS_DEADLINE);
  return 0;
}
int sophia_bs_issued(struct sophia_bs_submission *s, uint64_t now) {
  int e = sophia_bs_advance(s, now);
  if (e)
    return e;
  if (s->custody != SOPHIA_BS_QUEUED || s->inflight)
    return SOPHIA_BS_BUSY;
  s->custody = SOPHIA_BS_ISSUED;
  s->inflight = 1;
  return 0;
}
int sophia_bs_reply(struct sophia_bs_submission *s, uint32_t count,
                    uint32_t error, uint64_t now) {
  int e = sophia_bs_advance(s, now);
  if (e)
    return e;
  if (!s->inflight)
    return SOPHIA_BS_BUSY;
  s->inflight = 0;
  if (!error && count == 24) {
    if (!s->receipt)
      s->custody = SOPHIA_BS_WRITE_ACCEPTED;
    return 0;
  }
  if (error && !s->receipt) {
    if (error == 11) {
      s->custody = SOPHIA_BS_QUEUED;
      return 0;
    }
    s->custody = SOPHIA_BS_REFUSED;
    s->remote_error = error;
    return fail(s, SOPHIA_BS_REMOTE);
  }
  return fail(s, SOPHIA_BS_PROTOCOL);
}
int sophia_bs_receipt(struct sophia_bs_submission *s,
                      const struct sophia_bf_event *v, uint64_t now) {
  int e = sophia_bs_advance(s, now);
  if (e)
    return e;
  if (!v)
    return SOPHIA_BS_INVALID;
  if (v->epoch != s->epoch || !v->sequence || s->receipt ||
      (s->custody != SOPHIA_BS_ISSUED &&
       s->custody != SOPHIA_BS_WRITE_ACCEPTED) ||
      v->kind != SOPHIA_BF_SUBMITTED || v->submission != s->id ||
      v->candidate_kind != s->kind)
    return fail(s, SOPHIA_BS_PROTOCOL);
  s->receipt = v->sequence;
  s->custody = SOPHIA_BS_SUBMITTED;
  return 0;
}
int sophia_bs_releasable(const struct sophia_bs_submission *s, uint64_t ack) {
  return s && !s->error && !s->inflight && s->receipt && ack >= s->receipt;
}
void sophia_bs_close(struct sophia_bs_submission *s) {
  if (s)
    fail(s, SOPHIA_BS_CLOSED);
}
