#ifndef SOPHIA_OUTPUT_SESSION_H
#define SOPHIA_OUTPUT_SESSION_H
#include "sophia_9p_client.h"
#include "sophia_output_files.h"

/* One admitted output-role attach over standard 9P2000.L. The session owns
 * fids, offsets, partial writes, submission IDs, file custody, topology
 * fetches and cumulative acks. It borrows the fd; the caller closes it after
 * ending the session. No discovery, IPC fallback, reconnect, clock read,
 * allocator or topology policy. Grants come only from the server.
 *
 * Allocate state_bytes() with malloc-compatible alignment and disjoint storage
 * from storage_bytes(). Both live until close. Single thread. */
struct sophia_os;
enum sophia_os_state {
  SOPHIA_OS_NEGOTIATING,
  SOPHIA_OS_READY,
  SOPHIA_OS_REFUSED, /* Negotiation refusal acknowledged; close the fd. */
  SOPHIA_OS_CLOSED,
  SOPHIA_OS_STALE,
  SOPHIA_OS_FAILED
};
enum sophia_os_custody {
  SOPHIA_OS_UNAVAILABLE,
  SOPHIA_OS_ADMITTED_LOCAL,
  SOPHIA_OS_ISSUED,
  SOPHIA_OS_SUBMITTED,
  SOPHIA_OS_REFUSED_SUBMIT,
  SOPHIA_OS_DROPPED_UNSENT,
  SOPHIA_OS_UNKNOWN_DISCONNECTED
};
struct sophia_os_config {
  uint32_t msize; /* 4096..65536. Eight bounded 9P requests. */
  /* Requested revisions and capabilities. Observe is required; READY means
   * the server granted a subset of this request that includes observe. */
  struct sophia_of_negotiate offer;
  uint64_t bootstrap_deadline_ms; /* Absolute CLOCK_MONOTONIC; > initial now. */
};
struct sophia_os_obligations {
  uint64_t consumed, acked, submitted_sequence;
  uint64_t deadline_ms, retry_at_ms;
  uint8_t event_pending, topology_pending, waiting_for_ack;
};
size_t sophia_os_state_bytes(void);
size_t sophia_os_storage_bytes(uint32_t msize);
int sophia_os_open_fd(struct sophia_os *, int fd,
                      const struct sophia_os_config *, void *storage,
                      size_t bytes, uint64_t now_ms);
enum sophia_os_state sophia_os_state(const struct sophia_os *);
uint64_t sophia_os_epoch(const struct sophia_os *);
uint64_t sophia_os_capabilities(const struct sophia_os *);
/* Nonzero after a Refused event: 1 unsupported revision, 2 observe required. */
uint16_t sophia_os_refusal(const struct sophia_os *);
const struct sophia_of_limits *sophia_os_limits(const struct sophia_os *);
int sophia_os_poll_fd(const struct sophia_os *);
short sophia_os_poll_events(const struct sophia_os *);
/* Poll with this timeout even when no fd event is ready. Successful submit and
 * consume request one immediate dispatch (timeout 0). Recompute after calls. */
int sophia_os_timeout(const struct sophia_os *, uint64_t now_ms);
int sophia_os_obligations(const struct sophia_os *,
                          struct sophia_os_obligations *);
uint32_t sophia_os_remote_error(const struct sophia_os *);
/* One bounded pass, independent of revents except POLLNVAL. now_ms must not
 * regress. EAGAIN submissions keep identical bytes and submission ID and
 * retry after ack progress or backoff (4..256 ms). Acks follow consumed events
 * automatically; the server's ack-progress deadline (Limits) still requires
 * prompt consumption. A queued candidate waits for the reply to the ack that
 * covers the previous Submitted, and for its transaction fid's clunk. */
int sophia_os_dispatch(struct sophia_os *, short revents, size_t byte_budget,
                       uint64_t now_ms);
/* One borrowed event, stable until consume/close. Negotiated, Refused and
 * Submitted are owned internally. An ObjectPublished event is presented only
 * after the SDK has read that exact object (matching Qid path, epoch and
 * topology epoch, complete through EOF) and released its handle, so the ack
 * that follows its consumption never precedes the full read. */
int sophia_os_event(struct sophia_os *, const struct sophia_of_record **);
int sophia_os_consume(struct sophia_os *);
/* The most recently fetched authoritative topology, or NULL before the first.
 * It changes only when a later ObjectPublished is presented; copy it to keep
 * it across that point. */
const struct sophia_of_topology *sophia_os_topology(const struct sophia_os *);
/* One immutable proposal at a time. BUSY makes no change; success copies the
 * encoded bytes and returns a local ticket. The SDK assigns the envelope
 * identity; the domain transaction is the caller's (see next_transaction).
 * Requires the configure grant. Deadline is absolute. Outcomes for the last
 * 64 tickets survive close; SUBMITTED is file custody, not a topology result:
 * that arrives as an Outcome event naming the domain transaction. */
int sophia_os_submit(struct sophia_os *, const struct sophia_of_proposal *,
                     uint64_t deadline_ms, uint64_t *ticket);
int sophia_os_outcome(const struct sophia_os *, uint64_t ticket,
                      enum sophia_os_custody *, uint32_t *wire_error);
/* Optional increasing domain allocator. The server refuses reuse of a domain
 * transaction within the epoch, including after a semantic rejection. */
int sophia_os_next_transaction(struct sophia_os *, uint64_t *transaction);
void sophia_os_close(struct sophia_os *);
#endif
