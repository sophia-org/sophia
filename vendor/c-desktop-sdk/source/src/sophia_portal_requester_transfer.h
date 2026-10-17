#ifndef SOPHIA_PORTAL_REQUESTER_TRANSFER_H
#define SOPHIA_PORTAL_REQUESTER_TRANSFER_H
#include "sophia_portal_requester_state.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_pqf_state {
  SOPHIA_PQF_PENDING,
  SOPHIA_PQF_DENIED,
  SOPHIA_PQF_ALLOWED,
  SOPHIA_PQF_END_ISSUED,
  SOPHIA_PQF_COMPLETED,
  SOPHIA_PQF_REVOKED,
  SOPHIA_PQF_EXPIRED
};
/* Passive per-request correlation. Private fixed storage, no owned pointers.
 * A matched grant alone promises neither upload support nor an executor. */
struct sophia_pqf {
  struct sophia_pq_grant expected;
  enum sophia_pqf_state state;
  unsigned closed;
};
int sophia_pqf_init(struct sophia_pqf *, const struct sophia_pq_request *,
                    uint64_t broker_generation);
/* Session verifies envelope/receipt first. Failure leaves state unchanged. */
int sophia_pqf_decision(struct sophia_pqf *, const struct sophia_pq_event *);
/* Call only after validated fully uploaded End enters the transport. */
int sophia_pqf_end_issued(struct sophia_pqf *);
int sophia_pqf_outcome(struct sophia_pqf *, uint64_t transfer,
                       enum sophia_pq_outcome);
/* Borrowed grant, unavailable after terminal outcome or disconnect. */
const struct sophia_pq_grant *sophia_pqf_grant(const struct sophia_pqf *);
/* Preserves the last observed state; disconnect is not a terminal outcome.
 * END_ISSUED after close means execution is uncertain. */
void sophia_pqf_close(struct sophia_pqf *);
#ifdef __cplusplus
}
#endif
#endif
