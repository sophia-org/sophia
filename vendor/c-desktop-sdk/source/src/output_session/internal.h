#ifndef SOPHIA_OUTPUT_SESSION_INTERNAL_H
#define SOPHIA_OUTPUT_SESSION_INTERNAL_H
#include "../output_files/internal.h"
#include "../sophia_output_session.h"
#include <poll.h>

#define OS_REQUESTS 8u
#define OS_FIDS 16u
#define OS_OUTCOMES 64u
#define OS_MAX_EVENTS 64u
/* Every API-1 event body is at most 24 bytes. */
#define OS_EVENT_BYTES (32u + 24u)
#define OS_JOURNAL_BYTES (OS_MAX_EVENTS * OS_EVENT_BYTES)
enum os_tx_stage {
  OS_TX_IDLE,
  OS_TX_WALK,
  OS_TX_OPEN,
  OS_TX_WRITE,
  OS_TX_SUBMIT,
  OS_TX_WAIT,
  OS_TX_CLUNK
};
enum os_object_stage {
  OS_OBJECT_IDLE,
  OS_OBJECT_WALK,
  OS_OBJECT_OPEN,
  OS_OBJECT_READ,
  OS_OBJECT_CLUNK,
  OS_OBJECT_READY
};
struct os_operation {
  struct sophia_9p_handle handle;
  uint8_t active;
};
struct os_ticket {
  uint64_t id;
  enum sophia_os_custody custody;
  uint32_t error;
};
struct sophia_os {
  struct sophia_9p_client wire;
  struct sophia_os_config config;
  struct sophia_of_limits limits;
  enum sophia_os_state state;
  int terminal;
  uint32_t remote_error, root, boot_fid, events_fid, submit_fid, ack_fid,
      tx_fid, object_fid;
  uint32_t boot_iounit, events_iounit, tx_iounit, object_iounit;
  struct os_operation boot_op, event_op, tx_op, ack_op, object_op;
  uint8_t bootstrap, have_limits, negotiated, event_ready, tx_replied,
      drive_pending, have_topology;
  uint16_t refusal;
  uint64_t epoch, selected, now, pass, sequence, consumed, acked, ack_pending;
  uint64_t refused_sequence, event_offset, fragment_deadline;
  uint64_t bootstrap_deadline;
  uint64_t next_ticket, tx_ticket, tx_domain, next_domain;
  uint64_t submitted_sequence, tx_deadline, assembly_deadline;
  uint64_t retry_at, retry_pass, retry_consumed, retry_acked;
  uint32_t retry_delay;
  enum os_tx_stage tx_stage;
  enum os_object_stage object_stage;
  /* The announcement being fetched: exact Qid path and topology epoch. */
  uint64_t object_qid, object_epoch, object_deadline, object_retry_at;
  uint32_t object_retry_delay;
  uint8_t *tx, *journal, *object;
  size_t tx_size, tx_written, journal_used, journal_validated, object_used;
  unsigned journal_records;
  uint8_t boot_bytes[128];
  size_t boot_used;
  struct sophia_of_record event;
  struct os_ticket tickets[OS_OUTCOMES];
  struct sophia_of_topology topology;
};
static inline int os_same(const struct os_operation *op,
                          struct sophia_9p_handle h) {
  return op->active && op->handle.slot == h.slot &&
         op->handle.serial == h.serial;
}
static inline int os_started(struct os_operation *op, int result) {
  if (!result)
    op->active = 1;
  return result == SOPHIA_9P_BUSY ? 0 : result;
}
static inline uint64_t os_add(uint64_t a, uint64_t b) {
  return a > UINT64_MAX - b ? UINT64_MAX : a + b;
}
static inline int os_final(const struct sophia_os *s) {
  return s->state >= SOPHIA_OS_REFUSED;
}
static inline uint32_t os_backoff(uint32_t delay) {
  delay = delay ? delay * 2 : 4;
  return delay > 256 ? 256 : delay;
}
int os_finish(struct sophia_os *, int status);
int os_drive(struct sophia_os *);
int os_boot_reply(struct sophia_os *, const struct sophia_9p_reply *);
int os_event_append(struct sophia_os *, const struct sophia_9p_reply *);
int os_events_validate(struct sophia_os *);
int os_event_head(struct sophia_os *);
int os_tx_reply(struct sophia_os *, const struct sophia_9p_reply *);
int os_tx_drive(struct sophia_os *);
int os_queue(struct sophia_os *, const struct sophia_of_record *, uint64_t,
             uint64_t *);
void os_set(struct sophia_os *, uint64_t, enum sophia_os_custody, uint32_t);
int os_object_drive(struct sophia_os *);
int os_object_reply(struct sophia_os *, const struct sophia_9p_reply *);
#endif
