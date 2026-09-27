#include "internal.h"

/* The held focus binds exactly the shown candidate at the current revision. */
static int ns_focus_matches(const struct sophia_ns *n, const struct sophia_sf_native_binding *v)
{
    const struct sophia_ns_shown *s = &n->shown;
    /* After Admitted no focus is armed again for this opening. */
    return n->open && !n->launched && s->valid && s->opening == n->opening.opening &&
           v->opening == n->opening.opening && v->output_id == s->output_id &&
           v->output_generation == s->output_generation && v->allocation_id == s->allocation_id &&
           v->allocation_generation == s->allocation_generation &&
           v->catalog_generation == s->catalog_generation &&
           v->catalog_generation == n->opening.catalog_generation &&
           v->candidate_generation == s->generation &&
           v->presentation_epoch == s->presentation_epoch &&
           v->interaction_generation == s->interaction_generation &&
           v->state_revision == s->state_revision && v->state_revision == n->revision;
}
static int ns_binding_grant(const struct sophia_ns *n, const struct sophia_sf_native_binding *b)
{
    return ns_grant(n, b->grant_connection_epoch, b->grant_content_epoch);
}
static void ns_input(struct sophia_ns *n, const struct sophia_sf_native_input *v)
{
    int bound;
    n->head.kind = SOPHIA_NS_EV_INPUT;
    n->head.current = n->open && v->binding.opening == n->opening.opening;
    bound = n->head.current && n->focused && ns_binding_same(&v->binding, &n->focus);
    if (v->kind != 17) {
        /* The server issued this revision whether or not the UI edits. */
        if (n->head.current && v->state_revision > n->revision) {
            n->revision = v->state_revision;
            n->head.editable = (uint8_t)bound;
        }
        return;
    }
    n->head.can_activate =
        bound && v->state_revision == n->revision && n->focus.state_revision == n->revision &&
        n->shown.valid && n->shown.generation == n->focus.candidate_generation &&
        n->shown.presentation_epoch == n->focus.presentation_epoch && n->shown.selected &&
        ns_catalog_current(n) && !n->activation.active && !n->launched;
}
static int ns_activation_outcome(struct sophia_ns *n,
                                 const struct sophia_sf_native_activation_outcome *v)
{
    const struct sophia_sf_native_activate *a = &v->activation, *mine = &n->activation.value;
    n->head.kind = SOPHIA_NS_EV_ACTIVATION;
    if (!n->activation.active || !ns_binding_equal(&a->binding, &mine->binding) ||
        a->event_id != mine->event_id || a->state_revision != mine->state_revision ||
        a->cause != mine->cause || a->slot != mine->slot)
        return -1;
    n->activation.active = 0;
    n->activation.ticket = 0;
    n->head.current = n->open && a->binding.opening == n->opening.opening;
    /* Admission is a queue slot only. It disarms focus at once; the opening
     * activates no more and awaits its NativeClosed. */
    if (v->status == 1 && n->head.current) {
        n->launched = 1;
        n->focused = 0;
    }
    return 0;
}
int ns_apply_native(struct sophia_ns *n, const struct sophia_sf_record *e, uint64_t now_ms)
{
    (void)now_ms;
    switch (e->header.kind) {
    case SOPHIA_SF_NATIVE_OPENING: {
        const struct sophia_sf_native_opening *v = &e->value.native_opening;
        n->head.kind = SOPHIA_NS_EV_OPENING;
        /* Reopening requires a larger opening ID. */
        if (!ns_grant(n, v->grant_connection_epoch, v->grant_content_epoch) || n->open ||
            v->opening <= n->last_opening)
            return -1;
        n->opening = *v;
        n->last_opening = v->opening;
        n->open = 1;
        n->revision = v->state_revision;
        n->focused = 0;
        n->launched = 0;
        n->closed_reason = 0;
        memset(&n->shown, 0, sizeof(n->shown));
        n->head.current = 1;
        return 0;
    }
    case SOPHIA_SF_NATIVE_FOCUS: {
        const struct sophia_sf_native_binding *v = &e->value.native_focus;
        n->head.kind = SOPHIA_NS_EV_FOCUS;
        if (!ns_binding_grant(n, v))
            return -1;
        if (!n->open || v->opening != n->opening.opening)
            return 0;
        n->head.current = 1;
        /* A focus that is not provably the shown candidate's is not held,
         * and it supersedes any focus held before. */
        n->focused = (uint8_t)ns_focus_matches(n, v);
        n->head.held = n->focused;
        if (n->focused)
            n->focus = *v;
        return 0;
    }
    case SOPHIA_SF_NATIVE_FOCUS_REVOKED: {
        const struct sophia_sf_native_binding *v = &e->value.native_focus_revoked.binding;
        n->head.kind = SOPHIA_NS_EV_FOCUS_REVOKED;
        if (!ns_binding_grant(n, v))
            return -1;
        if (n->focused && ns_binding_same(v, &n->focus)) {
            n->focused = 0;
            n->head.current = 1;
        }
        return 0;
    }
    case SOPHIA_SF_NATIVE_INPUT:
        if (!ns_binding_grant(n, &e->value.native_input.binding))
            return -1;
        ns_input(n, &e->value.native_input);
        return 0;
    case SOPHIA_SF_NATIVE_ACTIVATION_OUTCOME:
        if (!ns_binding_grant(n, &e->value.native_activation_outcome.activation.binding))
            return -1;
        return ns_activation_outcome(n, &e->value.native_activation_outcome);
    default: {
        const struct sophia_sf_native_closed *v = &e->value.native_closed;
        n->head.kind = SOPHIA_NS_EV_CLOSED;
        if (!ns_grant(n, v->grant_connection_epoch, v->grant_content_epoch) || !n->open ||
            v->opening != n->opening.opening)
            return -1;
        n->open = 0;
        n->focused = 0;
        n->closed_reason = v->reason;
        memset(&n->shown, 0, sizeof(n->shown));
        if (n->allocation.has_grant && n->allocation.opening == v->opening)
            n->allocation.orphaned = 1; /* stop using it; see header */
        n->head.current = 1;
        return 0;
    }
    }
}
/* A pointer activation names a displayed row of the shown candidate under a
 * held focus bound to it at the current revision. */
static int ns_pointer_slot(const struct sophia_ns *n, const struct sophia_sf_action *v)
{
    unsigned i;
    if (!n->focused || n->launched || n->activation.active || !ns_catalog_current(n) ||
        n->focus.state_revision != n->revision ||
        n->focus.candidate_generation != n->shown.generation ||
        n->focus.presentation_epoch != n->shown.presentation_epoch || !v->action_id ||
        v->action_id > 4096)
        return 0;
    for (i = 0; i < n->shown.row_count; i++)
        if (n->shown.rows[i] == v->action_id)
            return 1;
    return 0;
}
int ns_apply_action(struct sophia_ns *n, const struct sophia_sf_record *e)
{
    const struct sophia_sf_action *v = &e->value.action;
    const struct sophia_ns_shown *s = &n->shown;
    n->head.kind = SOPHIA_NS_EV_ACTION;
    if (!ns_grant(n, v->grant_connection_epoch, v->grant_content_epoch))
        return -1;
    n->head.current = s->valid && v->output_id == s->output_id &&
                      v->output_generation == s->output_generation &&
                      v->candidate_generation == s->generation &&
                      v->presentation_epoch == s->presentation_epoch &&
                      v->interaction_generation == s->interaction_generation &&
                      v->allocation_id == s->allocation_id &&
                      v->allocation_generation == s->allocation_generation;
    /* Kind 3 is a cancellation and owes nothing. */
    n->head.ack_owed = v->kind == 1 || v->kind == 2;
    n->head.can_activate = v->kind == 1 && n->head.current && ns_pointer_slot(n, v);
    return 0;
}
void ns_service_input(struct sophia_ns *n)
{
    enum sophia_ss_outcome o = ns_ticket(n, n->activation.ticket);
    if (!n->activation.active || !ns_settled(o))
        return;
    if (ns_refused(o))
        n->activation.active = 0;
    n->activation.ticket = 0;
}
void ns_acks_trim(struct sophia_ns *n)
{
    unsigned i = 0;
    while (i < n->ack_count) {
        enum sophia_ss_outcome o = ns_ticket(n, n->acks[i]);
        if (!ns_settled(o)) {
            i++;
            continue;
        }
        if (ns_refused(o) && n->lost_acks < UINT8_MAX)
            n->lost_acks++;
        n->acks[i] = n->acks[--n->ack_count];
    }
}
int sophia_ns_input_reserve(struct sophia_ns *n, int activate)
{
    const struct sophia_sf_native_input *in;
    struct sophia_ns_response *resp;
    size_t bytes = 0, one;
    unsigned i, count;
    uint64_t t;
    int r = ns_gate(n);
    if (r)
        return r;
    resp = &n->response;
    if (!n->have_head || n->head.kind != SOPHIA_NS_EV_INPUT || resp->active)
        return SOPHIA_9P_ARGUMENT;
    if (activate && !n->head.can_activate)
        return SOPHIA_9P_INVALID;
    ns_acks_trim(n);
    if (n->ack_count >= SOPHIA_NS_ACKS)
        return SOPHIA_9P_BUSY;
    count = activate ? 2u : 1u;
    if (ns_peek_transaction(n, &t) || t > UINT64_MAX - count)
        return SOPHIA_9P_ARGUMENT;
    in = &n->head.record->value.native_input;
    memset(resp->records, 0, sizeof(resp->records));
    i = 0;
    if (activate) {
        struct sophia_sf_native_activate *a = &resp->records[0].value.native_activate;
        resp->records[0].header.kind = SOPHIA_SF_NATIVE_ACTIVATE;
        a->binding = in->binding;
        a->binding.transaction = t;
        a->event_id = in->event_id;
        a->state_revision = in->state_revision;
        a->cause = 1;
        a->slot = n->shown.selected;
        i = 1;
    }
    resp->records[i].header.kind = SOPHIA_SF_NATIVE_INPUT_ACK;
    resp->records[i].value.native_input_ack.binding = in->binding;
    resp->records[i].value.native_input_ack.binding.transaction = t + i;
    resp->records[i].value.native_input_ack.event_id = in->event_id;
    resp->records[i].value.native_input_ack.state_revision = in->state_revision;
    resp->records[i].value.native_input_ack.disposition = 1;
    for (i = 0; i < count; i++) {
        one = sophia_ss_record_bytes(&resp->records[i]);
        if (!one)
            return SOPHIA_9P_INVALID;
        bytes += one;
    }
    r = sophia_ss_reserve(n->ss, (uint16_t)count, bytes, &resp->reservation);
    if (r) {
        (void)ns_gate(n);
        return r;
    }
    n->next_transaction += count;
    resp->count = (uint8_t)count;
    resp->activate = (uint8_t)(activate != 0);
    resp->active = 1;
    return 0;
}
int sophia_ns_input_commit(struct sophia_ns *n, uint16_t disposition, uint64_t *first_ticket)
{
    struct sophia_ns_response *resp;
    uint64_t first;
    int r;
    if (!n || !n->ss || !first_ticket || disposition < 1 || disposition > 2)
        return SOPHIA_9P_ARGUMENT;
    resp = &n->response;
    if (!resp->active)
        return SOPHIA_9P_ARGUMENT;
    resp->records[resp->count - 1].value.native_input_ack.disposition = disposition;
    /* Cannot fail for capacity; a final ss releases the reservation. */
    r = sophia_ss_commit(n->ss, &resp->reservation, resp->records, resp->count, &first);
    if (r) {
        if (sophia_ss_state(n->ss) > SOPHIA_SS_READY) {
            resp->active = 0;
            (void)ns_gate(n);
        }
        return r;
    }
    if (resp->activate) {
        n->activation.value = resp->records[0].value.native_activate;
        n->activation.transaction = n->activation.value.binding.transaction;
        n->activation.ticket = first;
        n->activation.active = 1;
    }
    n->acks[n->ack_count++] = first + resp->count - 1;
    resp->active = 0;
    *first_ticket = first;
    n->have_head = 0;
    (void)sophia_ss_consume(n->ss);
    (void)ns_gate(n);
    return 0;
}
int sophia_ns_input_cancel(struct sophia_ns *n)
{
    int r;
    if (!n || !n->ss || !n->response.active)
        return SOPHIA_9P_ARGUMENT;
    r = sophia_ss_cancel(n->ss, &n->response.reservation);
    n->response.active = 0;
    return r;
}
int sophia_ns_input_apply(struct sophia_ns *n, int activate, sophia_ns_edit edit, void *user,
                          uint64_t *first_ticket)
{
    int applied = 0, r;
    if (!first_ticket)
        return SOPHIA_9P_ARGUMENT;
    r = sophia_ns_input_reserve(n, activate);
    if (r)
        return r;
    /* Past this point the edit runs at most once: the ack is already owned. */
    if (n->head.editable && edit)
        applied = edit(user, &n->head.record->value.native_input) != 0;
    return sophia_ns_input_commit(n, (applied || n->response.activate) ? 1 : 2, first_ticket);
}
int sophia_ns_action_ack(struct sophia_ns *n, uint16_t disposition, int activate,
                         uint64_t *first_ticket)
{
    const struct sophia_sf_action *v;
    struct sophia_sf_record *recs;
    struct sophia_sf_action_ack *a;
    unsigned count;
    uint64_t t, first;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!first_ticket || disposition < 1 || disposition > 2 || !n->have_head ||
        n->head.kind != SOPHIA_NS_EV_ACTION || !n->head.ack_owed || n->response.active)
        return SOPHIA_9P_ARGUMENT;
    if (activate && !n->head.can_activate)
        return SOPHIA_9P_INVALID;
    ns_acks_trim(n);
    if (n->ack_count >= SOPHIA_NS_ACKS)
        return SOPHIA_9P_BUSY;
    count = activate ? 2u : 1u;
    if (ns_peek_transaction(n, &t) || t > UINT64_MAX - count)
        return SOPHIA_9P_ARGUMENT;
    v = &n->head.record->value.action;
    /* The idle response group is scratch: no input reservation can be held
     * while an ACTION is the head. */
    recs = n->response.records;
    memset(recs, 0, 2 * sizeof(*recs));
    if (activate) {
        struct sophia_sf_native_activate *x = &recs[0].value.native_activate;
        recs[0].header.kind = SOPHIA_SF_NATIVE_ACTIVATE;
        x->binding = n->focus;
        x->binding.transaction = t;
        x->event_id = v->event_id;
        x->state_revision = n->focus.state_revision;
        x->cause = 2;
        x->slot = (uint16_t)v->action_id;
    }
    recs[count - 1].header.kind = SOPHIA_SF_ACTION_ACK;
    a = &recs[count - 1].value.action_ack;
    a->transaction = t + count - 1;
    a->grant_connection_epoch = v->grant_connection_epoch;
    a->grant_content_epoch = v->grant_content_epoch;
    a->output_id = v->output_id;
    a->output_generation = v->output_generation;
    a->candidate_generation = v->candidate_generation;
    a->presentation_epoch = v->presentation_epoch;
    a->interaction_generation = v->interaction_generation;
    a->allocation_id = v->allocation_id;
    a->allocation_generation = v->allocation_generation;
    a->target_id = v->target_id;
    a->target_generation = v->target_generation;
    a->action_id = v->action_id;
    a->event_id = v->event_id;
    a->disposition = disposition;
    /* All or none: BUSY leaves the event and every state unchanged. */
    r = sophia_ss_submit(n->ss, recs, count, &first);
    if (r) {
        (void)ns_gate(n);
        return r;
    }
    n->next_transaction += count;
    if (activate) {
        n->activation.value = recs[0].value.native_activate;
        n->activation.transaction = t;
        n->activation.ticket = first;
        n->activation.active = 1;
    }
    n->acks[n->ack_count++] = first + count - 1;
    *first_ticket = first;
    n->have_head = 0;
    (void)sophia_ss_consume(n->ss);
    (void)ns_gate(n);
    return 0;
}
