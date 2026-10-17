#include "../sophia_admin_command.h"
#include <string.h>

static uint64_t minimum(uint64_t a, uint64_t b) { return a < b ? a : b; }
static uint64_t frame_deadline(const struct sophia_ac *s, uint64_t now) {
  return now > UINT64_MAX - s->frame ? s->deadline : minimum(now + s->frame, s->deadline);
}
int sophia_ac_init(struct sophia_ac *s, const struct sophia_af_info *info,
                    const struct sophia_af_catalog *catalog, const struct sophia_af_command *command,
                    uint64_t request, uint64_t now) {
  struct sophia_ac v = {0};
  struct sophia_af_invocation invoke;
  uint64_t duration;
  if (!s || !info || !catalog || !command || !request || !info->connection || !catalog->generation ||
      !info->command_ms || info->command_ms > 10000 || !info->frame_ms || info->frame_ms > 2000 ||
      !sophia_af_catalog_contains(catalog, command)) return SOPHIA_AC_INVALID;
  duration = (uint64_t)info->command_ms + info->frame_ms;
  if (now > UINT64_MAX - duration) return SOPHIA_AC_INVALID;
  invoke.connection = info->connection; invoke.request = request; invoke.generation = catalog->generation;
  invoke.owner = command->owner; invoke.name = command->name; invoke.name_length = command->name_length;
  if (sophia_af_encode_invocation(&invoke, v.bytes, sizeof v.bytes, &v.length)) return SOPHIA_AC_INVALID;
  v.connection = invoke.connection; v.request = request; v.generation = invoke.generation;
  v.owner = command->owner; v.reload = command->name_length == 14 && !memcmp(command->name, "reload-profile", 14);
  v.frame = info->frame_ms; v.now = now; v.deadline = now + duration;
  v.operation_deadline = v.deadline; v.poll_at = now; v.custody = SOPHIA_AC_QUEUED;
  *s = v;
  return 0;
}
static int fail(struct sophia_ac *s, int error) {
  if (!s->error) {
    s->error = error;
    if (s->custody == SOPHIA_AC_QUEUED) s->custody = SOPHIA_AC_DROPPED_UNSENT;
    else if (s->custody == SOPHIA_AC_ISSUED || s->custody == SOPHIA_AC_ACCEPTED) s->custody = SOPHIA_AC_UNKNOWN;
    s->status_inflight = 0;
  }
  return s->error;
}
void sophia_ac_close(struct sophia_ac *s) { if (s) (void)fail(s, SOPHIA_AC_CLOSED); }
int sophia_ac_advance(struct sophia_ac *s, uint64_t now) {
  if (!s) return SOPHIA_AC_INVALID;
  if (now < s->now) return SOPHIA_AC_CLOCK_REGRESSION;
  if (s->error) return s->error;
  s->now = now;
  if (s->custody != SOPHIA_AC_SETTLED && (now >= s->deadline || now >= s->operation_deadline))
    return fail(s, SOPHIA_AC_DEADLINE);
  return 0;
}
int sophia_ac_issued(struct sophia_ac *s, uint64_t now) {
  int r = sophia_ac_advance(s, now);
  if (r) return r;
  if (s->custody != SOPHIA_AC_QUEUED) return SOPHIA_AC_BUSY;
  s->operation_deadline = frame_deadline(s, now); s->custody = SOPHIA_AC_ISSUED;
  return 0;
}
int sophia_ac_write_reply(struct sophia_ac *s, int remote, uint32_t value, uint64_t now) {
  int r = sophia_ac_advance(s, now);
  if (r) return r;
  if (s->custody != SOPHIA_AC_ISSUED) return fail(s, SOPHIA_AC_PROTOCOL);
  if (remote) {
    s->custody = SOPHIA_AC_REFUSED; s->remote_error = value;
    return fail(s, SOPHIA_AC_REFUSAL);
  }
  if (value != s->length) return fail(s, SOPHIA_AC_PROTOCOL);
  s->custody = SOPHIA_AC_ACCEPTED; s->poll_at = now; s->operation_deadline = s->deadline;
  return 0;
}
int sophia_ac_start_status(struct sophia_ac *s, uint64_t now, uint64_t *deadline) {
  int r;
  if (!deadline) return SOPHIA_AC_INVALID;
  r = sophia_ac_advance(s, now);
  if (r) return r;
  if (s->custody != SOPHIA_AC_ACCEPTED || s->status_inflight || now < s->poll_at) return SOPHIA_AC_BUSY;
  s->status_inflight = 1; s->operation_deadline = frame_deadline(s, now);
  *deadline = s->operation_deadline;
  return 0;
}
int sophia_ac_status(struct sophia_ac *s, const void *bytes, size_t length, uint64_t now) {
  struct sophia_af_status status;
  int r = sophia_ac_advance(s, now);
  if (r) return r;
  if (s->custody != SOPHIA_AC_ACCEPTED || !s->status_inflight ||
      sophia_af_decode_status(bytes, length, &status) || status.connection != s->connection ||
      status.request != s->request || status.generation != s->generation) return fail(s, SOPHIA_AC_PROTOCOL);
  if (status.kind == SOPHIA_AF_PENDING) {
    s->status_inflight = 0; s->operation_deadline = s->deadline;
    s->poll_at = now == UINT64_MAX ? now : now + 1;
    return 0;
  }
  if (status.kind != SOPHIA_AF_OUTCOME ||
      (status.outcome == SOPHIA_AF_COMMITTED && s->owner != SOPHIA_AF_POLICY) ||
      (status.outcome == SOPHIA_AF_COMPLETED && s->owner != SOPHIA_AF_SESSION) ||
      (status.outcome == SOPHIA_AF_UNCHANGED && (s->owner != SOPHIA_AF_SESSION || !s->reload)))
    return fail(s, SOPHIA_AC_PROTOCOL);
  memcpy(s->detail, status.detail, status.detail_length); s->detail_length = status.detail_length;
  s->outcome = status.outcome; s->custody = SOPHIA_AC_SETTLED; s->status_inflight = 0;
  return 0;
}
int sophia_ac_next_wake(const struct sophia_ac *s, uint64_t *time) {
  if (!s || !time || s->error || s->custody == SOPHIA_AC_SETTLED) return 0;
  *time = minimum(s->deadline, s->custody == SOPHIA_AC_ACCEPTED && !s->status_inflight ? s->poll_at : s->operation_deadline);
  return 1;
}
int sophia_ac_invalidate_catalog(const struct sophia_ac *s) {
  return s && s->outcome && (s->owner == SOPHIA_AF_SESSION || s->outcome == SOPHIA_AF_STALE ||
      s->outcome == SOPHIA_AF_DENIED || s->outcome == SOPHIA_AF_INDETERMINATE);
}
