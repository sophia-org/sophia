#include "../sophia_portal_capture.h"

int sophia_pc_init(struct sophia_pc *s,
                   struct sophia_pf_capture_request request, uint64_t now,
                   uint64_t deadline) {
  struct sophia_pc v = {0};
  if (!s)
    return SOPHIA_PC_INVALID;
  if (deadline <= now || deadline - now > 10000)
    return SOPHIA_PC_DEADLINE;
  if (sophia_pf_encode_capture(request, v.bytes, sizeof v.bytes))
    return SOPHIA_PC_INVALID;
  v.request = request;
  v.now = now;
  v.deadline = deadline;
  v.poll_at = now;
  v.custody = SOPHIA_PC_QUEUED;
  *s = v;
  return 0;
}
static int fail(struct sophia_pc *s, int error) {
  if (!s->error) {
    s->error = error;
    if (s->custody == SOPHIA_PC_QUEUED)
      s->custody = SOPHIA_PC_DROPPED_UNSENT;
    else if (s->custody == SOPHIA_PC_ISSUED || s->custody == SOPHIA_PC_ACCEPTED)
      s->custody = SOPHIA_PC_UNKNOWN;
    s->status_inflight = 0;
  }
  return s->error;
}
void sophia_pc_close(struct sophia_pc *s) {
  if (s)
    (void)fail(s, SOPHIA_PC_CLOSED);
}
int sophia_pc_advance(struct sophia_pc *s, uint64_t now) {
  if (!s)
    return SOPHIA_PC_INVALID;
  if (now < s->now)
    return SOPHIA_PC_CLOCK_REGRESSION;
  if (s->error)
    return s->error;
  s->now = now;
  if (s->custody != SOPHIA_PC_SETTLED && now >= s->deadline)
    return fail(s, SOPHIA_PC_DEADLINE);
  return 0;
}
int sophia_pc_issued(struct sophia_pc *s, uint64_t now) {
  int r = sophia_pc_advance(s, now);
  if (r)
    return r;
  if (s->custody != SOPHIA_PC_QUEUED)
    return SOPHIA_PC_BUSY;
  s->custody = SOPHIA_PC_ISSUED;
  return 0;
}
int sophia_pc_write_reply(struct sophia_pc *s, int remote, uint32_t value,
                          uint64_t now) {
  int r = sophia_pc_advance(s, now);
  if (r)
    return r;
  if (s->custody != SOPHIA_PC_ISSUED)
    return fail(s, SOPHIA_PC_PROTOCOL);
  if (remote) {
    s->custody = SOPHIA_PC_REFUSED;
    s->remote_error = value;
    return fail(s, SOPHIA_PC_REFUSAL);
  }
  if (value != SOPHIA_PF_CAPTURE_BYTES)
    return fail(s, SOPHIA_PC_PROTOCOL);
  s->custody = SOPHIA_PC_ACCEPTED;
  s->poll_at = now;
  return 0;
}
int sophia_pc_start_status(struct sophia_pc *s, uint64_t now,
                           uint64_t *deadline) {
  int r;
  if (!deadline)
    return SOPHIA_PC_INVALID;
  r = sophia_pc_advance(s, now);
  if (r)
    return r;
  if (s->custody != SOPHIA_PC_ACCEPTED || s->status_inflight ||
      now < s->poll_at)
    return SOPHIA_PC_BUSY;
  s->status_inflight = 1;
  *deadline = s->deadline;
  return 0;
}
int sophia_pc_status(struct sophia_pc *s, const void *bytes, size_t length,
                     uint64_t now) {
  struct sophia_pf_capture_status status;
  int r = sophia_pc_advance(s, now);
  if (r)
    return r;
  if (s->custody != SOPHIA_PC_ACCEPTED || !s->status_inflight ||
      sophia_pf_decode_capture_status(bytes, length, &status) ||
      status.serial != s->request.serial || status.output != s->request.output)
    return fail(s, SOPHIA_PC_PROTOCOL);
  s->status_inflight = 0;
  if (status.state == SOPHIA_PF_QUEUED || status.state == SOPHIA_PF_CAPTURING)
    s->poll_at = now == UINT64_MAX ? now : now + 1;
  else if (status.state == SOPHIA_PF_IDLE)
    return fail(s, SOPHIA_PC_PROTOCOL);
  else {
    s->outcome = status.state;
    s->custody = SOPHIA_PC_SETTLED;
  }
  return 0;
}
int sophia_pc_next_wake(const struct sophia_pc *s, uint64_t *time) {
  if (!s || !time || s->error || s->custody == SOPHIA_PC_SETTLED)
    return 0;
  *time = s->deadline;
  if (s->custody == SOPHIA_PC_ACCEPTED && !s->status_inflight &&
      s->poll_at < *time)
    *time = s->poll_at;
  return 1;
}
