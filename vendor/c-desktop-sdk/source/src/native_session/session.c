#include "internal.h"

void ns_fail(struct sophia_ns *n)
{
    n->state = SOPHIA_NS_FAILED;
    n->focused = 0;
    n->shown.valid = 0;
}
/* Submit one record through ss, which assigns its header; the transaction is
 * the caller's. BUSY and errors change nothing here. */
int ns_submit(struct sophia_ns *n, struct sophia_sf_record *r, uint64_t *ticket)
{
    int status;
    r->header.epoch = 0;
    r->header.submission = 0;
    r->header.sequence = 0;
    status = sophia_ss_submit(n->ss, r, 1, ticket);
    if (status < 0 && sophia_ss_state(n->ss) > SOPHIA_SS_READY)
        n->state = SOPHIA_NS_ENDED;
    return status;
}
int sophia_ns_init(struct sophia_ns *n, struct sophia_ss *ss, const struct sophia_ns_config *c)
{
    const struct sophia_sf_limits *l;
    if (!n || !ss || !c || !c->first_transaction)
        return SOPHIA_9P_ARGUMENT;
    if (sophia_ss_state(ss) > SOPHIA_SS_READY)
        return SOPHIA_9P_INVALID;
    l = sophia_ss_limits(ss);
    if (sophia_ss_state(ss) != SOPHIA_SS_READY || !l)
        return SOPHIA_9P_BUSY;
    memset(n, 0, sizeof(*n));
    n->ss = ss;
    n->state = SOPHIA_NS_LIVE;
    n->limits = *l;
    n->config = *c;
    n->next_transaction = c->first_transaction;
    n->next_request = 1;
    n->next_demand = 1;
    n->next_generation = 1;
    return 0;
}
enum sophia_ns_state sophia_ns_state(const struct sophia_ns *n)
{
    if (!n)
        return SOPHIA_NS_FAILED;
    if (n->state == SOPHIA_NS_LIVE && n->ss && sophia_ss_state(n->ss) > SOPHIA_SS_READY)
        return SOPHIA_NS_ENDED;
    return n->state;
}
int sophia_ns_service(struct sophia_ns *n, uint64_t now_ms)
{
    int r = ns_gate(n);
    if (r)
        return r;
    ns_acks_trim(n);
    ns_service_input(n);
    ns_service_content(n, now_ms);
    return ns_gate(n);
}
static void ns_object(struct sophia_ns *n, const struct sophia_sf_object_published *v)
{
    if (v->object_kind == SOPHIA_SF_CATALOG)
        n->catalog_announced = v->generation;
    else if (v->object_kind == SOPHIA_SF_OUTPUTS)
        n->facts_announced = v->generation;
}
int sophia_ns_next(struct sophia_ns *n, uint64_t now_ms, struct sophia_ns_event *out)
{
    const struct sophia_sf_record *e;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!out)
        return SOPHIA_9P_ARGUMENT;
    if (n->have_head) {
        *out = n->head;
        return 0;
    }
    r = sophia_ss_event(n->ss, &e);
    if (r) {
        (void)ns_gate(n);
        return r;
    }
    memset(&n->head, 0, sizeof(n->head));
    n->head.record = e;
    n->head_ms = now_ms;
    if (e->header.kind >= SOPHIA_SF_ALLOCATION_RESULT && e->header.kind <= SOPHIA_SF_ACTION)
        r = ns_apply_content(n, e, now_ms);
    else if (e->header.kind >= SOPHIA_SF_NATIVE_OPENING &&
             e->header.kind <= SOPHIA_SF_NATIVE_CLOSED)
        r = ns_apply_native(n, e, now_ms);
    else {
        if (e->header.kind == SOPHIA_SF_OBJECT_PUBLISHED)
            ns_object(n, &e->value.object_published);
        n->head.kind = SOPHIA_NS_EV_OTHER;
        r = 0;
    }
    if (r) {
        ns_fail(n);
        return SOPHIA_9P_INVALID;
    }
    n->have_head = 1;
    *out = n->head;
    return 0;
}
int sophia_ns_done(struct sophia_ns *n)
{
    int r = ns_gate(n);
    if (r)
        return r;
    if (!n->have_head || n->head.kind == SOPHIA_NS_EV_INPUT ||
        (n->head.kind == SOPHIA_NS_EV_ACTION && n->head.ack_owed))
        return SOPHIA_9P_ARGUMENT;
    r = sophia_ss_consume(n->ss);
    n->have_head = 0;
    (void)ns_gate(n);
    return r;
}
int sophia_ns_catalog(struct sophia_ns *n, uint64_t generation)
{
    int r = ns_gate(n);
    if (r)
        return r;
    if (!generation)
        return SOPHIA_9P_ARGUMENT;
    n->catalog_installed = generation;
    return 0;
}
int sophia_ns_facts(struct sophia_ns *n, uint64_t generation)
{
    int r = ns_gate(n);
    if (r)
        return r;
    if (!generation)
        return SOPHIA_9P_ARGUMENT;
    n->facts_installed = generation;
    return 0;
}
static void ns_earliest(uint64_t *next, uint64_t value)
{
    if (value && (!*next || value < *next))
        *next = value;
}
static uint64_t ns_add(uint64_t a, uint64_t b) { return a > UINT64_MAX - b ? UINT64_MAX : a + b; }
int sophia_ns_obligations(const struct sophia_ns *n, struct sophia_ns_obligations *o)
{
    const struct sophia_ns_allocation *a;
    uint64_t timeout;
    if (!n || !o)
        return SOPHIA_9P_ARGUMENT;
    memset(o, 0, sizeof(*o));
    a = &n->allocation;
    timeout = n->limits.action_ack_timeout_ms;
    if (n->have_head && n->head.kind == SOPHIA_NS_EV_INPUT)
        o->input_due_ms =
            ns_add(n->head.record->value.native_input.issued_mono_usec / 1000u, timeout);
    if (n->have_head && n->head.kind == SOPHIA_NS_EV_ACTION && n->head.ack_owed)
        o->action_due_ms = ns_add(n->head_ms, timeout);
    if (n->permit.granted)
        o->permit_expires_ms = n->permit.expires_ms;
    if (a->pending)
        o->allocation_due_ms = ns_add(a->requested_ms, n->limits.allocation_timeout_ms);
    if (n->pending.valid)
        o->candidate_due_ms =
            ns_add(n->pending.submitted_ms, (uint64_t)n->limits.candidate_timeout_ms +
                                                n->limits.preparation_timeout_ms +
                                                n->limits.presentation_timeout_ms);
    o->needs_catalog = n->open && !ns_catalog_current(n);
    o->needs_facts = n->open && (!n->facts_installed || n->facts_installed != n->facts_announced);
    o->needs_candidate = n->open && !o->needs_catalog && !o->needs_facts && a->has_grant &&
                         !a->orphaned && !n->pending.valid &&
                         (!n->shown.valid || n->shown.state_revision != n->revision ||
                          n->shown.allocation_id != a->granted.allocation_id ||
                          n->shown.allocation_generation != a->granted.allocation_generation);
    o->awaiting_invalidation = a->has_grant && a->orphaned;
    o->lost_acks = n->lost_acks;
    ns_earliest(&o->next_ms, o->input_due_ms);
    ns_earliest(&o->next_ms, o->action_due_ms);
    ns_earliest(&o->next_ms, o->permit_expires_ms);
    ns_earliest(&o->next_ms, o->allocation_due_ms);
    ns_earliest(&o->next_ms, o->candidate_due_ms);
    return 0;
}
const struct sophia_sf_native_opening *sophia_ns_opening(const struct sophia_ns *n)
{
    return sophia_ns_state(n) == SOPHIA_NS_LIVE && n->open ? &n->opening : NULL;
}
const struct sophia_sf_native_binding *sophia_ns_focus(const struct sophia_ns *n)
{
    return sophia_ns_state(n) == SOPHIA_NS_LIVE && n->focused ? &n->focus : NULL;
}
const struct sophia_ns_shown *sophia_ns_presented(const struct sophia_ns *n)
{
    return sophia_ns_state(n) == SOPHIA_NS_LIVE && n->shown.valid ? &n->shown : NULL;
}
const struct sophia_sf_allocation_result *sophia_ns_allocation(const struct sophia_ns *n)
{
    return sophia_ns_state(n) == SOPHIA_NS_LIVE && n->allocation.has_grant &&
                   !n->allocation.orphaned
               ? &n->allocation.granted
               : NULL;
}
uint64_t sophia_ns_revision(const struct sophia_ns *n) { return n && n->open ? n->revision : 0; }
