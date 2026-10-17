#ifndef SOPHIA_PORTAL_FRAME_INTERNAL_H
#define SOPHIA_PORTAL_FRAME_INTERNAL_H
#include "../sophia_portal_frame.h"
/* Internal scheduler interface. The owner validates root, capacity and disjoint
 * storage. prepare initializes state; step queues operations; accept routes a
 * matching reply. The caller consumes replies and owns all socket I/O. */
int sophia_pfr_prepare(struct sophia_pfr *, struct sophia_9p_client *, uint32_t,
                       void *, size_t, uint64_t, uint64_t);
int sophia_pfr_step(struct sophia_pfr *, uint64_t);
int sophia_pfr_matches(const struct sophia_pfr *, struct sophia_9p_handle);
int sophia_pfr_accept(struct sophia_pfr *, const struct sophia_9p_reply *);
#endif
