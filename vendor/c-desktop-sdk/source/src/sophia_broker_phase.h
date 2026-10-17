#ifndef SOPHIA_BROKER_PHASE_H
#define SOPHIA_BROKER_PHASE_H
#include "sophia_broker_submission.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_bp_phase {
  SOPHIA_BP_NEGOTIATING,
  SOPHIA_BP_READY,
  SOPHIA_BP_RESPONDING,
  SOPHIA_BP_REFUSED
};
/* Private fixed state. Journal checks sequences; transaction checks custody.
 * Refusal timer starts on observation, never on a partial acknowledgement.
 * Server remains responsible for the publication deadline. */
struct sophia_bp {
  struct sophia_bf_limits limits;
  uint64_t epoch, expected, last_request, pending, refusal_sequence,
      refusal_due, now;
  uint16_t candidate_kind;
  unsigned handshake_receipt;
  enum sophia_bp_phase phase;
  int error;
};
int sophia_bp_init(struct sophia_bp *, uint64_t,
                   const struct sophia_bf_limits *, uint64_t, uint64_t);
int sophia_bp_advance(struct sophia_bp *, uint64_t);
int sophia_bp_response(struct sophia_bp *, uint64_t request,
                       uint64_t submission, uint64_t now);
/* Call after journal and transaction validation. */
int sophia_bp_event(struct sophia_bp *, const struct sophia_bf_event *,
                    uint64_t);
int sophia_bp_refusal_acked(const struct sophia_bp *, uint64_t);
#ifdef __cplusplus
}
#endif
#endif
