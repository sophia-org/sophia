#include "../sophia_broker_journal.h"
#include <string.h>
static int fail(struct sophia_bj *s, enum sophia_bs_error e) {
  if (!s->error)
    s->error = e;
  return s->error;
}
int sophia_bj_init(struct sophia_bj *s, uint64_t epoch, uint64_t now,
                   uint32_t timeout) {
  if (!s || !epoch || !timeout || timeout > 2000)
    return SOPHIA_BS_INVALID;
  memset(s, 0, sizeof *s);
  s->epoch = epoch;
  s->now = now;
  s->timeout = timeout;
  return 0;
}
int sophia_bj_advance(struct sophia_bj *s, uint64_t now) {
  if (!s)
    return SOPHIA_BS_INVALID;
  if (now < s->now)
    return SOPHIA_BS_CLOCK;
  if (s->error)
    return s->error;
  s->now = now;
  if (s->has_due && now >= s->due)
    return fail(s, SOPHIA_BS_DEADLINE);
  return 0;
}
static int arm(struct sophia_bj *s) {
  if (s->now > UINT64_MAX - s->timeout)
    return fail(s, SOPHIA_BS_OVERFLOW);
  s->due = s->now + s->timeout;
  s->has_due = 1;
  return 0;
}
uint32_t sophia_bj_read_count(const struct sophia_bj *s) {
  return !s || s->error || s->head ? 0 : (uint32_t)(512 - (s->end - s->start));
}
static int parse(struct sophia_bj *s) {
  const unsigned char *b = s->bytes + s->start;
  uint32_t n;
  struct sophia_bf_event event;
  if (s->end - s->start < 4)
    return 0;
  n = (uint32_t)b[0] | ((uint32_t)b[1] << 8) | ((uint32_t)b[2] << 16) |
      ((uint32_t)b[3] << 24);
  if (n != 40 && n != 48 && n != 56 && n != 200)
    return fail(s, SOPHIA_BS_PROTOCOL);
  if (s->end - s->start < n)
    return 0;
  if (sophia_bf_decode_event(b, n, s->epoch, &event) ||
      s->decoded == UINT64_MAX || event.sequence != s->decoded + 1)
    return fail(s, SOPHIA_BS_PROTOCOL);
  s->decoded = event.sequence;
  s->head = n;
  return 0;
}
int sophia_bj_receive(struct sophia_bj *s, const void *bytes, size_t n,
                      uint64_t now) {
  int e = sophia_bj_advance(s, now);
  if (e)
    return e;
  if (s->head)
    return SOPHIA_BS_BUSY;
  if (!bytes || !n || n > sophia_bj_read_count(s))
    return fail(s, SOPHIA_BS_PROTOCOL);
  if (n > UINT64_MAX - s->offset)
    return fail(s, SOPHIA_BS_OVERFLOW);
  if (!s->has_due && (e = arm(s)))
    return e;
  if (s->end + n > 512) {
    memmove(s->bytes, s->bytes + s->start, s->end - s->start);
    s->end -= s->start;
    s->start = 0;
  }
  memcpy(s->bytes + s->end, bytes, n);
  s->end += n;
  s->offset += n;
  return parse(s);
}
int sophia_bj_event(const struct sophia_bj *s, struct sophia_bf_event *out) {
  if (!s || !out)
    return SOPHIA_BS_INVALID;
  if (s->error)
    return s->error;
  if (!s->head)
    return SOPHIA_BS_BUSY;
  return sophia_bf_decode_event(s->bytes + s->start, s->head, s->epoch, out)
             ? SOPHIA_BS_PROTOCOL
             : 0;
}
int sophia_bj_consume(struct sophia_bj *s, uint64_t now) {
  int e = sophia_bj_advance(s, now);
  if (e)
    return e;
  if (!s->head)
    return SOPHIA_BS_BUSY;
  s->consumed = s->decoded;
  s->start += s->head;
  s->head = 0;
  if (s->start == s->end)
    s->start = s->end = 0;
  return parse(s);
}
int sophia_bj_begin_ack(struct sophia_bj *s, uint64_t now) {
  int e = sophia_bj_advance(s, now);
  if (e)
    return e;
  if (s->has_ack || s->consumed == s->acknowledged)
    return SOPHIA_BS_BUSY;
  if (sophia_bf_encode_ack(s->epoch, s->consumed, s->ack, 16))
    return fail(s, SOPHIA_BS_PROTOCOL);
  s->ack_sequence = s->consumed;
  s->has_ack = 1;
  return 0;
}
int sophia_bj_ack_reply(struct sophia_bj *s, uint32_t count, uint32_t error,
                        uint64_t now) {
  int e = sophia_bj_advance(s, now);
  if (e)
    return e;
  if (!s->has_ack)
    return SOPHIA_BS_BUSY;
  if (error) {
    s->remote_error = error;
    return fail(s, SOPHIA_BS_REMOTE);
  }
  if (count != 16)
    return fail(s, SOPHIA_BS_PROTOCOL);
  s->acknowledged = s->ack_sequence;
  s->has_ack = 0;
  if (s->start != s->end || s->consumed > s->acknowledged)
    return arm(s);
  s->has_due = 0;
  return 0;
}
void sophia_bj_close(struct sophia_bj *s) {
  if (s)
    fail(s, SOPHIA_BS_CLOSED);
}
