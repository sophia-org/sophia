#ifndef SOPHIA_PORTAL_REQUESTER_SESSION_H
#define SOPHIA_PORTAL_REQUESTER_SESSION_H
#include "sophia_portal_requester_startup.h"
#include "sophia_portal_requester_transaction.h"
#include "sophia_portal_requester_transfer.h"
#include "sophia_portal_requester_upload.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_PQR_WIRE 9
#define SOPHIA_PQR_REFUSED 10
struct sophia_pqr_request_id {
  const struct sophia_pqr *issuer;
  uint64_t epoch, submission;
};
struct sophia_pqr_tracked {
  struct sophia_pqr_request_id id;
  struct sophia_pqf transfer;
  uint64_t terminal_sequence;
  unsigned occupied;
  unsigned cancel_pending;
  unsigned begun;
};
/* Developing handshake session. Fixed, immovable caller storage; borrows wire
 * exclusively. The caller keeps the authority channel alive and closes the role
 * fd on terminal error. Fields are private implementation state. */
struct sophia_pqr {
  struct sophia_9p_client *wire;
  struct sophia_pq_limits limits;
  struct sophia_pqj_journal journal;
  struct sophia_pqt transaction;
  struct sophia_9p_handle boot_tag, read_tag, ack_tag, transaction_tag;
  uint32_t root, events, ack, events_iounit, remote_error;
  uint64_t now, deadline, observed, negotiated, refused, refusal_due;
  unsigned boot, boot_active, read_active, ack_active, transaction_active,
      releasing;
  int error, wire_error;
  unsigned established, pending_request;
  uint64_t last_submission;
  struct sophia_pqr_tracked transfers[65];
  enum sophia_pq_kind command;
  struct sophia_pqu upload;
  struct sophia_pqr_request_id upload_id;
  struct sophia_9p_handle upload_tag;
  unsigned upload_present, upload_active, upload_phase;
};
/* Consumes a ready opening on success. No attach/reconnect or ambient
 * admission. Opens events/ack and negotiates with submission ID one on this new
 * epoch. */
int sophia_pqr_init(struct sophia_pqr *, struct sophia_pqo *, uint16_t minimum,
                    uint16_t maximum, uint64_t deadline);
int sophia_pqr_dispatch(struct sophia_pqr *, uint64_t now, size_t budget);
int sophia_pqr_ready(const struct sophia_pqr *);
int sophia_pqr_transaction_ready(const struct sophia_pqr *);
const struct sophia_pq_event *sophia_pqr_event(const struct sophia_pqr *);
int sophia_pqr_consume(struct sophia_pqr *, uint64_t now);
int sophia_pqr_acknowledge(struct sophia_pqr *, uint64_t now);
int sophia_pqr_retry(struct sophia_pqr *, uint64_t now);
int sophia_pqr_deadline(const struct sophia_pqr *, uint64_t *);
int sophia_pqr_wants_write(const struct sophia_pqr *);
void sophia_pqr_close(struct sophia_pqr *);
/* Fixed storage holds 64 grants plus one pending/denied request. Handles are
 * connection-scoped observations, not authority. Release acknowledged terminal
 * results to reclaim storage. No request is replayed on reconnect. */
int sophia_pqr_request(struct sophia_pqr *, const struct sophia_pq_request *,
                       uint64_t now, uint64_t deadline,
                       struct sophia_pqr_request_id *);
const struct sophia_pqf *sophia_pqr_transfer(const struct sophia_pqr *,
                                             struct sophia_pqr_request_id);
int sophia_pqr_release_transfer(struct sophia_pqr *,
                                struct sophia_pqr_request_id, uint64_t now);
/* Retains Allowed until the terminal event; cannot cancel after End issuance.
 * Session storage must not be moved/reinitialized while its handles are live.
 */
int sophia_pqr_cancel(struct sophia_pqr *, struct sophia_pqr_request_id,
                      uint64_t now, uint64_t deadline);
/* One immutable borrowed payload, disjoint from session and transport storage.
 * Requires four request slots/six fids. Remains borrowed until release_upload,
 * or role fd closure after failure. Server selects/validates the executor. */
int sophia_pqr_begin(struct sophia_pqr *, struct sophia_pqr_request_id,
                     const void *, size_t, uint64_t now, uint64_t deadline);
int sophia_pqr_upload_written(const struct sophia_pqr *,
                              struct sophia_pqr_request_id, size_t *);
int sophia_pqr_upload_ready(const struct sophia_pqr *,
                            struct sophia_pqr_request_id);
int sophia_pqr_end(struct sophia_pqr *, struct sophia_pqr_request_id,
                   uint64_t now, uint64_t deadline);
/* Releases local buffer borrow after terminal settlement/writer cleanup, or
 * after failure (caller must close role fd first). Outcome remains separately
 * retained until acknowledged and released. */
int sophia_pqr_release_upload(struct sophia_pqr *,
                              struct sophia_pqr_request_id);
/* Custody remains in transaction.submission.custody after terminal failure.
 * Readiness requires acknowledged negotiation and successful transaction
 * clunks. Consume does not send an ack. Dispatch performs one bounded service
 * call and routes one reply; call again while the transport reports runnable
 * work. */
#ifdef __cplusplus
}
#endif
#endif
