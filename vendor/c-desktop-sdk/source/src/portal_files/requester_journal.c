#include "../sophia_portal_requester_state.h"
#include <string.h>
static int fail(struct sophia_pqj_journal *j, enum sophia_pqs_error e) {
  if (!j->error)
    j->error = e;
  return j->error;
}
static uint32_t length(const unsigned char *b) {
  return (uint32_t)b[0] | (uint32_t)b[1] << 8 | (uint32_t)b[2] << 16 |
         (uint32_t)b[3] << 24;
}
int sophia_pqj_init(struct sophia_pqj_journal *j, uint64_t epoch, uint64_t now,
                    uint32_t timeout) {
  if (!j || !epoch || !timeout || timeout > 2000)
    return SOPHIA_PQS_INVALID;
  memset(j, 0, sizeof *j);
  j->epoch = epoch;
  j->now = now;
  j->timeout = timeout;
  return 0;
}
int sophia_pqj_advance(struct sophia_pqj_journal *j, uint64_t now) {
  if (!j)
    return SOPHIA_PQS_INVALID;
  if (now < j->now)
    return SOPHIA_PQS_CLOCK;
  if (j->error)
    return j->error;
  j->now = now;
  if (j->has_due && now >= j->due)
    return fail(j, SOPHIA_PQS_DEADLINE);
  return 0;
}
void sophia_pqj_close(struct sophia_pqj_journal *j) {
  if (j)
    fail(j, SOPHIA_PQS_CLOSED);
}
static int arm(struct sophia_pqj_journal *j) {
  if (j->now > UINT64_MAX - j->timeout)
    return fail(j, SOPHIA_PQS_OVERFLOW);
  j->due = j->now + j->timeout;
  j->has_due = 1;
  return 0;
}
uint32_t sophia_pqj_read_count(const struct sophia_pqj_journal *j) {
  return !j || j->error || j->has_head ? 0
                                       : (uint32_t)(512 - (j->end - j->start));
}
static int parse(struct sophia_pqj_journal *j) {
  const unsigned char *b = j->bytes + j->start;
  uint32_t n;
  struct sophia_pq_event v;
  if (j->end - j->start < 4)
    return 0;
  n = length(b);
  if (n < 40 || n > 88)
    return fail(j, SOPHIA_PQS_PROTOCOL);
  if (j->end - j->start < n)
    return 0;
  if (sophia_pq_decode_event(b, n, &v) || v.epoch != j->epoch ||
      j->decoded == UINT64_MAX || v.sequence != j->decoded + 1)
    return fail(j, SOPHIA_PQS_PROTOCOL);
  j->decoded = v.sequence;
  j->head = v;
  j->has_head = 1;
  return 0;
}
int sophia_pqj_receive(struct sophia_pqj_journal *j, const void *bytes,
                       size_t n, uint64_t now) {
  int e = sophia_pqj_advance(j, now);
  if (e)
    return e;
  if (j->has_head)
    return SOPHIA_PQS_BUSY;
  if (!bytes || !n || n > sophia_pqj_read_count(j))
    return fail(j, SOPHIA_PQS_PROTOCOL);
  if (n > UINT64_MAX - j->offset)
    return fail(j, SOPHIA_PQS_OVERFLOW);
  if (!j->has_due && (e = arm(j)))
    return e;
  if (j->end + n > sizeof j->bytes) {
    memmove(j->bytes, j->bytes + j->start, j->end - j->start);
    j->end -= j->start;
    j->start = 0;
  }
  memcpy(j->bytes + j->end, bytes, n);
  j->end += n;
  j->offset += n;
  return parse(j);
}
const struct sophia_pq_event *
sophia_pqj_event(const struct sophia_pqj_journal *j) {
  return j && !j->error && j->has_head ? &j->head : NULL;
}
int sophia_pqj_consume(struct sophia_pqj_journal *j, uint64_t now) {
  int e = sophia_pqj_advance(j, now);
  if (e)
    return e;
  if (!j->has_head)
    return SOPHIA_PQS_BUSY;
  j->consumed = j->head.sequence;
  j->has_head = 0;
  j->start += length(j->bytes + j->start);
  if (j->start == j->end)
    j->start = j->end = 0;
  return parse(j);
}
int sophia_pqj_begin_ack(struct sophia_pqj_journal *j, uint64_t now) {
  int e = sophia_pqj_advance(j, now);
  if (e)
    return e;
  if (j->has_ack || j->consumed == j->acknowledged)
    return SOPHIA_PQS_BUSY;
  if (sophia_pq_encode_ack(j->epoch, j->consumed, j->ack))
    return fail(j, SOPHIA_PQS_PROTOCOL);
  j->ack_sequence = j->consumed;
  j->has_ack = 1;
  return 0;
}
int sophia_pqj_ack_reply(struct sophia_pqj_journal *j, uint32_t count,
                         uint32_t error, uint64_t now) {
  int e = sophia_pqj_advance(j, now);
  if (e)
    return e;
  if (!j->has_ack)
    return SOPHIA_PQS_BUSY;
  if (error) {
    j->remote_error = error;
    return fail(j, SOPHIA_PQS_REMOTE);
  }
  if (count != 16)
    return fail(j, SOPHIA_PQS_PROTOCOL);
  j->acknowledged = j->ack_sequence;
  j->has_ack = 0;
  if (j->start != j->end || j->consumed > j->acknowledged)
    return arm(j);
  j->has_due = 0;
  return 0;
}
