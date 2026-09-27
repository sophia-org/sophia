#ifndef SOPHIA_WM_SESSION_H
#define SOPHIA_WM_SESSION_H
#include "sophia_9p_client.h"
#include "sophia_wm_files.h"

/* One admitted WM attach over standard 9P2000.L. The session owns the fids,
 * offsets, partial writes, submission IDs, custody and cumulative acks. It
 * borrows the fd; the caller closes it after ending the session. No discovery,
 * IPC fallback, reconnect, replay, clock read, allocator or policy reducer.
 *
 * Allocate state_bytes() with malloc-compatible alignment and disjoint storage
 * from storage_bytes(). Both live until close. The opaque state lets language
 * bindings call this API without reproducing private layouts. Single thread. */
struct sophia_ws;
enum sophia_ws_state {
  SOPHIA_WS_NEGOTIATING,
  SOPHIA_WS_READY,
  SOPHIA_WS_CLOSED,
  SOPHIA_WS_STALE,
  SOPHIA_WS_FAILED
};
enum sophia_ws_custody {
  SOPHIA_WS_UNAVAILABLE,
  SOPHIA_WS_ADMITTED_LOCAL,
  SOPHIA_WS_ISSUED,
  SOPHIA_WS_SUBMITTED,
  SOPHIA_WS_REFUSED,
  SOPHIA_WS_DROPPED_UNSENT,
  SOPHIA_WS_UNKNOWN_DISCONNECTED
};
struct sophia_ws_config {
  uint32_t msize; /* 4096..65536. Eight bounded 9P requests. */
  struct sophia_wf_negotiate offer;
  uint64_t bootstrap_deadline_ms; /* Absolute CLOCK_MONOTONIC; > initial now. */
};
struct sophia_ws_obligations {
  uint64_t consumed, acked, submitted_sequence;
  uint64_t deadline_ms, retry_at_ms;
  uint8_t event_pending, snapshot_pending, snapshot_ready, waiting_for_ack;
};
size_t sophia_ws_state_bytes(void);
size_t sophia_ws_storage_bytes(uint32_t msize);
int sophia_ws_open_fd(struct sophia_ws *, int fd,
                      const struct sophia_ws_config *, void *storage,
                      size_t bytes, uint64_t now_ms);
/* READY means capability negotiation, not successful profile activation.
 * Policy phase, exact semantic correlations, checkpointing and settlement are
 * the caller's decisions under the WM contract. */
enum sophia_ws_state sophia_ws_state(const struct sophia_ws *);
uint64_t sophia_ws_epoch(const struct sophia_ws *);
uint64_t sophia_ws_capabilities(const struct sophia_ws *);
const struct sophia_wf_limits *sophia_ws_limits(const struct sophia_ws *);
int sophia_ws_poll_fd(const struct sophia_ws *);
short sophia_ws_poll_events(const struct sophia_ws *);
int sophia_ws_timeout(const struct sophia_ws *, uint64_t now_ms);
int sophia_ws_obligations(const struct sophia_ws *,
                          struct sophia_ws_obligations *);
uint32_t sophia_ws_remote_error(const struct sophia_ws *);
/* One bounded pass, independent of revents except POLLNVAL. now_ms must not
 * regress. EAGAIN retries keep identical bytes and submission ID, wait for a
 * later pass, then consumed/ack progress or backoff (4..256 ms). The fixed
 * caller deadline is never extended; candidate assembly is additionally bounded
 * by Limits. Idle event reads have no timer; incomplete records do.
 * Acks follow consumed events automatically. A queued candidate waits for the
 * reply to the previous Submitted ack before opening its transaction. */
int sophia_ws_dispatch(struct sophia_ws *, short revents, size_t byte_budget,
                       uint64_t now_ms);
/* One borrowed event, stable until consume/close. Negotiate/Submitted are owned
 * internally. Other events remain in original order and have no effect applied
 * by this library. Prompt consumption releases server retention. */
int sophia_ws_event(struct sophia_ws *, const struct sophia_wf_record **);
int sophia_ws_consume(struct sophia_ws *);
/* One immutable candidate at a time. BUSY makes no change; success copies all
 * bytes and returns a local ticket. SDK assigns envelope identity. Deadline is
 * absolute, supplied by the policy exchange owner. Outcomes for the last 64
 * tickets survive close; Submitted is final custody, not a semantic commit.
 * The codec does not replace the server's policy phase or scene validation. */
int sophia_ws_submit(struct sophia_ws *, const struct sophia_wf_record *,
                     uint64_t deadline_ms, uint64_t *ticket);
int sophia_ws_outcome(const struct sophia_ws *, uint64_t ticket,
                      enum sophia_ws_custody *, uint32_t *wire_error);
/* Optional monotonic domain allocator. Domain IDs do not alias submission
 * tickets. An externally supplied domain ID is also permitted when it exceeds
 * the last Submitted domain ID. Refusal never advances that watermark. */
int sophia_ws_next_transaction(struct sophia_ws *, uint64_t *transaction);
/* Fetch the immutable snapshot named by the CURRENT head Cycle. The SDK copies
 * its expected epoch/transaction/scene before the caller may consume the Cycle.
 * It checks the opened body's identity, complete length and EOF, then clunks
 * the pin. No partial snapshot escapes. One fetch/result at a time; release
 * invalidates its borrowed rows. EAGAIN before a pin uses bounded backoff. */
int sophia_ws_snapshot(struct sophia_ws *, uint64_t deadline_ms);
int sophia_ws_snapshot_result(struct sophia_ws *,
                              const struct sophia_wf_record **);
int sophia_ws_snapshot_release(struct sophia_ws *);
void sophia_ws_close(struct sophia_ws *);
#endif
