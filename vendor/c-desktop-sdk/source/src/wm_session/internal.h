#ifndef SOPHIA_WM_SESSION_INTERNAL_H
#define SOPHIA_WM_SESSION_INTERNAL_H
#include "../sophia_wm_session.h"
#include "../wm_files/internal.h"
#include <poll.h>

#define WS_REQUESTS 8u
#define WS_FIDS 16u
#define WS_OUTCOMES 64u
/* Largest known API-1 event: Cycle prefix, 16 outputs, presentation action. */
#define WS_EVENT_BYTES (32u + 48u + 16u * 8u + 64u)
#define WS_JOURNAL_BYTES (64u * WS_EVENT_BYTES)
enum ws_tx_stage {
  WS_TX_IDLE,
  WS_TX_WALK,
  WS_TX_OPEN,
  WS_TX_WRITE,
  WS_TX_SUBMIT,
  WS_TX_WAIT,
  WS_TX_CLUNK
};
enum ws_object_stage {
  WS_OBJECT_IDLE,
  WS_OBJECT_WALK,
  WS_OBJECT_OPEN,
  WS_OBJECT_READ,
  WS_OBJECT_CLUNK,
  WS_OBJECT_READY
};
struct ws_operation {
  struct sophia_9p_handle handle;
  uint8_t active;
};
struct ws_ticket {
  uint64_t id;
  enum sophia_ws_custody custody;
  uint32_t error;
};
struct sophia_ws {
  struct sophia_9p_client wire;
  struct sophia_ws_config config;
  struct sophia_wf_limits limits;
  enum sophia_ws_state state;
  int terminal;
  uint32_t remote_error, root, boot_fid, events_fid, submit_fid, ack_fid,
      tx_fid, object_fid;
  uint32_t boot_iounit, events_iounit, tx_iounit, object_iounit;
  struct ws_operation boot_op, event_op, tx_op, ack_op, object_op;
  uint8_t bootstrap, have_limits, negotiated, event_ready, tx_replied,
      event_fault;
  uint64_t epoch, selected, now, pass, sequence, consumed, acked, ack_pending;
  uint64_t event_offset, fragment_deadline, bootstrap_deadline;
  uint64_t next_ticket, tx_ticket, tx_domain, next_domain, submitted_domain;
  uint64_t submitted_sequence, tx_deadline, assembly_deadline;
  uint64_t retry_at, retry_pass, retry_consumed, retry_acked;
  uint32_t retry_delay;
  enum ws_tx_stage tx_stage;
  enum ws_object_stage object_stage;
  uint64_t object_deadline, object_retry_at, object_transaction, object_scene;
  uint32_t object_retry_delay;
  uint8_t *tx, *journal, *object;
  size_t tx_size, tx_written, journal_used, journal_validated, object_used;
  unsigned journal_records;
  uint8_t boot_bytes[257];
  size_t boot_used;
  struct sophia_wf_record event, snapshot;
  struct ws_ticket tickets[WS_OUTCOMES];
};
static inline int ws_same(const struct ws_operation *op,
                          struct sophia_9p_handle h) {
  return op->active && op->handle.slot == h.slot &&
         op->handle.serial == h.serial;
}
static inline int ws_started(struct ws_operation *op, int result) {
  if (!result)
    op->active = 1;
  return result == SOPHIA_9P_BUSY ? 0 : result;
}
static inline uint64_t ws_add(uint64_t a, uint64_t b) {
  return a > UINT64_MAX - b ? UINT64_MAX : a + b;
}
static inline int ws_final(const struct sophia_ws *s) {
  return s->state >= SOPHIA_WS_CLOSED;
}
int ws_finish(struct sophia_ws *, int status);
int ws_drive(struct sophia_ws *);
int ws_boot_reply(struct sophia_ws *, const struct sophia_9p_reply *);
int ws_event_append(struct sophia_ws *, const struct sophia_9p_reply *);
int ws_events_validate(struct sophia_ws *);
int ws_event_head(struct sophia_ws *);
int ws_tx_reply(struct sophia_ws *, const struct sophia_9p_reply *);
int ws_tx_drive(struct sophia_ws *);
int ws_queue(struct sophia_ws *, const struct sophia_wf_record *, uint64_t,
             uint64_t *);
void ws_set(struct sophia_ws *, uint64_t, enum sophia_ws_custody, uint32_t);
int ws_object_drive(struct sophia_ws *);
int ws_object_reply(struct sophia_ws *, const struct sophia_9p_reply *);
#endif
