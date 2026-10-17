#ifndef SOPHIA_BROKER_JOURNAL_H
#define SOPHIA_BROKER_JOURNAL_H
#include "sophia_broker_submission.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Private fixed storage. Event labels borrow it until consume/close/error;
 * do not move storage while a borrowed event is live. No socket ownership.
 * The local ack timer starts with observed bytes, not server publication. */
struct sophia_bj {
  uint64_t epoch, now, timeout, due, offset, decoded, consumed, acknowledged,
      ack_sequence;
  unsigned char bytes[512], ack[16];
  size_t start, end, head;
  unsigned has_due, has_ack;
  enum sophia_bs_error error;
  uint32_t remote_error;
};
int sophia_bj_init(struct sophia_bj *, uint64_t epoch, uint64_t now,
                   uint32_t timeout);
int sophia_bj_advance(struct sophia_bj *, uint64_t now);
uint32_t sophia_bj_read_count(const struct sophia_bj *);
int sophia_bj_receive(struct sophia_bj *, const void *, size_t, uint64_t now);
int sophia_bj_event(const struct sophia_bj *, struct sophia_bf_event *);
int sophia_bj_consume(struct sophia_bj *, uint64_t now);
int sophia_bj_begin_ack(struct sophia_bj *, uint64_t now);
int sophia_bj_ack_reply(struct sophia_bj *, uint32_t count, uint32_t error,
                        uint64_t now);
void sophia_bj_close(struct sophia_bj *);
#ifdef __cplusplus
}
#endif
#endif
