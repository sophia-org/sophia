#ifndef SOPHIA_PORTAL_REQUEST_INTERNAL_H
#define SOPHIA_PORTAL_REQUEST_INTERNAL_H
#include "../sophia_portal_request.h"
/* Scheduler-only entry points. The owner validates the admitted root, storage
 * and capacity; it matches replies, consumes them and owns all socket I/O.
 * prepare initializes in place, step queues operations, accept routes one
 * reply. None of these functions polls, authenticates, or changes opening
 * ownership. */
int sophia_pcr_prepare(struct sophia_pcr *, struct sophia_9p_client *, uint32_t,
                       struct sophia_pf_capture_request, uint64_t, uint64_t,
                       uint64_t);
int sophia_pcr_step(struct sophia_pcr *, uint64_t);
int sophia_pcr_accept(struct sophia_pcr *, const struct sophia_9p_reply *);
#endif
