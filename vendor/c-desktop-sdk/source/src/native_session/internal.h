#ifndef SOPHIA_NS_INTERNAL_H
#define SOPHIA_NS_INTERNAL_H
#include "../sophia_shell_native_session.h"
#include <string.h>

static inline int ns_final_status(const struct sophia_ns *n)
{
    return n->state == SOPHIA_NS_ENDED && sophia_ss_state(n->ss) == SOPHIA_SS_CLOSED
               ? SOPHIA_9P_CLOSED
               : SOPHIA_9P_INVALID;
}
/* ARGUMENT for a NULL ns; the final status once the ns is not LIVE. */
static inline int ns_gate(struct sophia_ns *n)
{
    if (!n || !n->ss)
        return SOPHIA_9P_ARGUMENT;
    if (n->state == SOPHIA_NS_LIVE && sophia_ss_state(n->ss) > SOPHIA_SS_READY)
        n->state = SOPHIA_NS_ENDED;
    return n->state == SOPHIA_NS_LIVE ? 0 : ns_final_status(n);
}
static inline int ns_grant(const struct sophia_ns *n, uint64_t connection, uint64_t content)
{
    return connection == n->limits.grant_connection_epoch &&
           content == n->limits.grant_content_epoch;
}
static inline int ns_binding_equal(const struct sophia_sf_native_binding *a,
                                   const struct sophia_sf_native_binding *b)
{
    return a->transaction == b->transaction &&
           a->grant_connection_epoch == b->grant_connection_epoch &&
           a->grant_content_epoch == b->grant_content_epoch && a->opening == b->opening &&
           a->output_id == b->output_id && a->output_generation == b->output_generation &&
           a->allocation_id == b->allocation_id &&
           a->allocation_generation == b->allocation_generation &&
           a->catalog_generation == b->catalog_generation &&
           a->candidate_generation == b->candidate_generation &&
           a->presentation_epoch == b->presentation_epoch &&
           a->interaction_generation == b->interaction_generation &&
           a->state_revision == b->state_revision && a->focus_lease == b->focus_lease;
}
/* The binding identity without its carrying record's transaction. */
static inline int ns_binding_same(const struct sophia_sf_native_binding *a,
                                  const struct sophia_sf_native_binding *b)
{
    struct sophia_sf_native_binding x = *a;
    x.transaction = b->transaction;
    return ns_binding_equal(&x, b);
}
/* Opening installed catalog equals the latest announced one. */
static inline int ns_catalog_current(const struct sophia_ns *n)
{
    return n->open && n->opening.catalog_generation == n->catalog_installed &&
           n->opening.catalog_generation == n->catalog_announced;
}
/* The next transaction ID; the caller advances it only on admission. */
static inline int ns_peek_transaction(const struct sophia_ns *n, uint64_t *out)
{
    if (!n->next_transaction || n->next_transaction == UINT64_MAX)
        return SOPHIA_9P_ARGUMENT;
    *out = n->next_transaction;
    return 0;
}
/* A ticket's outcome; UNAVAILABLE (evicted, or no ticket) only stops
 * tracking it: the server's own event still decides the item. */
static inline enum sophia_ss_outcome ns_ticket(const struct sophia_ns *n, uint64_t ticket)
{
    enum sophia_ss_outcome o;
    if (!ticket || sophia_ss_outcome(n->ss, ticket, &o, NULL))
        return SOPHIA_SS_UNAVAILABLE;
    return o;
}
static inline int ns_refused(enum sophia_ss_outcome o)
{
    return o == SOPHIA_SS_REFUSED || o == SOPHIA_SS_DROPPED_UNSENT;
}
static inline int ns_settled(enum sophia_ss_outcome o)
{
    return o != SOPHIA_SS_ADMITTED_LOCAL && o != SOPHIA_SS_IN_FLIGHT;
}
void ns_fail(struct sophia_ns *);
int ns_submit(struct sophia_ns *, struct sophia_sf_record *, uint64_t *ticket);
/* Apply one event (content.c / input.c); fills the head. */
int ns_apply_content(struct sophia_ns *, const struct sophia_sf_record *, uint64_t now_ms);
int ns_apply_native(struct sophia_ns *, const struct sophia_sf_record *, uint64_t now_ms);
int ns_apply_action(struct sophia_ns *, const struct sophia_sf_record *);
void ns_service_content(struct sophia_ns *, uint64_t now_ms);
void ns_service_input(struct sophia_ns *);
/* Drop settled ack tickets; one refused or dropped unsent is counted lost. */
void ns_acks_trim(struct sophia_ns *);
#endif
