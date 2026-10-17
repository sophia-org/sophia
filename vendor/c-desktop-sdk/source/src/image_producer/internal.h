#ifndef SOPHIA_IMAGE_PRODUCER_INTERNAL_H
#define SOPHIA_IMAGE_PRODUCER_INTERNAL_H
#include "../image_channel/internal.h"
#include "../sophia_image_producer.h"

/* Accepts now_ms if it does not go backwards. */
int ip_clock(struct sophia_ip *, uint64_t now_ms);
/* Closes the channel once with cause; later causes do not replace it, except
 * that a fatal cause replaces a clean one. Closes the socket. */
void ip_fail(struct sophia_ip *, enum sophia_ip_cause);
struct sophia_ip_slot *ip_slot(struct sophia_ip *, uint64_t offer);
void ip_free_slot(struct sophia_ip_slot *);
/* Marks passed deadlines; returns nonzero if anything changed. */
int ip_expire(struct sophia_ip *, uint64_t now_ms);
#endif
