#ifndef SOPHIA_BROKER_SESSION_H
#define SOPHIA_BROKER_SESSION_H
#include "sophia_broker_journal.h"
#include "sophia_broker_phase.h"
#include "sophia_broker_startup.h"
#include "sophia_broker_transaction.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_BR_WIRE 9
#define SOPHIA_BR_REFUSED 10
/* Fixed immovable caller storage, exclusive borrowed wire. Keep the admission
 * authority channel alive; close the role fd on terminal error. */
struct sophia_br {
  struct sophia_9p_client *wire;
  struct sophia_bf_limits limits;
  struct sophia_bj journal;
  struct sophia_bt transaction;
  struct sophia_bp ordering;
  struct sophia_bf_request request;
  char label[128];
  unsigned has_request;
  uint64_t request_due;
  struct sophia_9p_handle boot_tag, read_tag, ack_tag, transaction_tag;
  uint32_t root, events, ack, events_iounit, remote_error;
  uint64_t now, deadline, observed, negotiated, refused, refusal_due,
      last_submission;
  unsigned boot, boot_active, read_active, ack_active, transaction_active,
      releasing, established;
  int error, wire_error;
};
/* Takes exclusive use from a ready opening on success. */
int sophia_br_init(struct sophia_br *, struct sophia_bo *, uint16_t, uint16_t,
                   uint64_t);
int sophia_br_dispatch(struct sophia_br *, uint64_t now, size_t budget);
int sophia_br_ready(const struct sophia_br *);
int sophia_br_transaction_ready(const struct sophia_br *);
/* Pure query: a retryable submit refusal is waiting for caller retry. */
int sophia_br_retry_ready(const struct sophia_br *);
/* Returned label borrows session storage until response receipt or failure.
 * Journal consumption does not invalidate the retained request. */
int sophia_br_request(const struct sophia_br *, struct sophia_bf_request *);
int sophia_br_respond(struct sophia_br *, uint64_t request,
                      const struct sophia_bf_row *, size_t, uint64_t now);
int sophia_br_event(const struct sophia_br *, struct sophia_bf_event *);
int sophia_br_consume(struct sophia_br *, uint64_t now);
int sophia_br_acknowledge(struct sophia_br *, uint64_t now);
int sophia_br_retry(struct sophia_br *, uint64_t now);
int sophia_br_deadline(const struct sophia_br *, uint64_t *);
int sophia_br_wants_write(const struct sophia_br *);
void sophia_br_close(struct sophia_br *);
#ifdef __cplusplus
}
#endif
#endif
