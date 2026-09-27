#include "internal.h"

static struct sophia_ns_resource *ns_resource(struct sophia_ns *n, uint64_t id, uint64_t generation)
{
    unsigned i;
    for (i = 0; i < SOPHIA_NS_RESOURCES; i++)
        if (n->resources[i].state != SOPHIA_NS_RESOURCE_FREE && n->resources[i].id == id &&
            n->resources[i].generation == generation)
            return &n->resources[i];
    return NULL;
}
static void ns_resource_free(struct sophia_ns *n, struct sophia_ns_resource *r)
{
    if (n->uploading && &n->resources[n->upload] == r)
        n->uploading = 0;
    memset(r, 0, sizeof(*r));
}
static int ns_allocation_result(struct sophia_ns *n, const struct sophia_sf_allocation_result *v)
{
    struct sophia_ns_allocation *a = &n->allocation;
    n->head.kind = SOPHIA_NS_EV_ALLOCATION;
    if (v->status == 4) {
        /* Session-initiated; names an allocation, not a request. */
        if (a->has_grant && v->allocation_id == a->granted.allocation_id &&
            v->allocation_generation == a->granted.allocation_generation) {
            a->has_grant = 0;
            a->orphaned = 0;
            n->head.current = 1;
        }
        return 0;
    }
    if (!a->pending || v->allocation_request_id != a->request_id)
        return -1;
    a->pending = 0;
    a->ticket = 0;
    n->head.current = 1;
    if (v->status == 1) {
        a->granted = *v;
        a->has_grant = 1;
        a->edge = a->request_edge;
        a->orphaned = !(n->open && a->opening == n->opening.opening);
    } else if (v->status == 3) {
        a->has_grant = 0;
        a->orphaned = 0;
    }
    /* status 2: an acquire leaves none; a rejected resize keeps the grant. */
    return 0;
}
static int ns_candidate_outcome(struct sophia_ns *n, const struct sophia_sf_candidate_outcome *v)
{
    struct sophia_ns_shown *p = &n->pending;
    n->head.kind = SOPHIA_NS_EV_CANDIDATE;
    if (!p->valid || v->transaction != p->transaction || v->candidate_generation != p->generation ||
        v->output_id != p->output_id || v->output_generation != p->output_generation)
        return -1;
    if (v->kind == 1) {
        p->prepared = 1;
        n->head.current = 1;
        return 0;
    }
    if (v->kind == 2 && n->open && p->opening == n->opening.opening) {
        /* Only the server's Presented makes a candidate the shown one. */
        n->shown = *p;
        n->shown.presentation_epoch = v->presentation_epoch;
        n->shown.ticket = 0;
        n->head.current = 1;
    } else if (n->open && p->opening == n->opening.opening)
        n->head.current = 1;
    memset(p, 0, sizeof(*p));
    return 0;
}
static int ns_permit(struct sophia_ns *n, const struct sophia_sf_frame_permit *v)
{
    struct sophia_ns_permit *p = &n->permit;
    n->head.kind = SOPHIA_NS_EV_PERMIT;
    if (!p->demand || v->demand_id != p->demand_id)
        return v->demand_id && v->demand_id == p->last_demand_id ? 0 : -1;
    n->head.current = 1;
    if (v->state == 1) {
        /* Advisory only: the server's TTL starts at its grant-time sample. */
        p->granted = 1;
        p->permit_id = v->permit_id;
        p->max_candidate_bytes = v->max_candidate_bytes;
        p->expires_ms =
            p->demand_ms > UINT64_MAX - v->ttl_ms ? UINT64_MAX : p->demand_ms + v->ttl_ms;
        return 0;
    }
    p->last_demand_id = p->demand_id;
    p->demand = 0;
    p->granted = 0;
    p->cancelling = 0;
    p->permit_id = 0;
    return 0;
}
int ns_apply_content(struct sophia_ns *n, const struct sophia_sf_record *e, uint64_t now_ms)
{
    struct sophia_ns_resource *r;
    (void)now_ms;
    switch (e->header.kind) {
    case SOPHIA_SF_ALLOCATION_RESULT:
        if (!ns_grant(n, e->value.allocation_result.grant_connection_epoch,
                      e->value.allocation_result.grant_content_epoch))
            return -1;
        return ns_allocation_result(n, &e->value.allocation_result);
    case SOPHIA_SF_RESOURCE_STATUS: {
        const struct sophia_sf_resource_status *v = &e->value.resource_status;
        if (!ns_grant(n, v->grant_connection_epoch, v->grant_content_epoch))
            return -1;
        n->head.kind = SOPHIA_NS_EV_RESOURCE;
        r = ns_resource(n, v->resource_id, v->resource_generation);
        if (!r)
            return 0;
        n->head.current = 1;
        if (v->status == 1)
            r->state = SOPHIA_NS_RESOURCE_ADMITTED;
        else if (v->status == 2)
            r->state = SOPHIA_NS_RESOURCE_ACCEPTED;
        else
            ns_resource_free(n, r);
        return 0;
    }
    case SOPHIA_SF_RESOURCE_RELEASED: {
        const struct sophia_sf_resource_released *v = &e->value.resource_released;
        if (!ns_grant(n, v->grant_connection_epoch, v->grant_content_epoch))
            return -1;
        n->head.kind = SOPHIA_NS_EV_RESOURCE_RELEASED;
        r = ns_resource(n, v->resource_id, v->resource_generation);
        if (r) {
            ns_resource_free(n, r);
            n->head.current = 1;
        }
        return 0;
    }
    case SOPHIA_SF_CANDIDATE_OUTCOME:
        if (!ns_grant(n, e->value.candidate_outcome.grant_connection_epoch,
                      e->value.candidate_outcome.grant_content_epoch))
            return -1;
        return ns_candidate_outcome(n, &e->value.candidate_outcome);
    case SOPHIA_SF_FRAME_PERMIT:
        if (!ns_grant(n, e->value.frame_permit.grant_connection_epoch,
                      e->value.frame_permit.grant_content_epoch))
            return -1;
        return ns_permit(n, &e->value.frame_permit);
    default:
        return ns_apply_action(n, e);
    }
}
void ns_service_content(struct sophia_ns *n, uint64_t now_ms)
{
    enum sophia_ss_outcome o;
    unsigned i;
    if (n->permit.granted && now_ms >= n->permit.expires_ms)
        n->permit.granted = 0; /* skipped; the demand ends with the server's record */
    o = ns_ticket(n, n->permit.ticket);
    if (ns_settled(o)) {
        if (ns_refused(o) && n->permit.demand && !n->permit.granted && !n->permit.permit_id) {
            n->permit.demand = 0;
            n->permit.cancelling = 0;
        }
        n->permit.ticket = 0;
    }
    o = ns_ticket(n, n->allocation.ticket);
    if (ns_settled(o)) {
        if (ns_refused(o))
            n->allocation.pending = 0;
        n->allocation.ticket = 0;
    }
    o = ns_ticket(n, n->pending.ticket);
    if (n->pending.valid && ns_settled(o)) {
        if (ns_refused(o)) {
            /* Never journaled: its permit is still the server's until it
             * ends, so a new demand waits for that FramePermit. */
            memset(&n->pending, 0, sizeof(n->pending));
            if (!n->permit.demand && n->permit.last_demand_id) {
                n->permit.demand = 1;
                n->permit.granted = 0;
                n->permit.demand_id = n->permit.last_demand_id;
            }
        } else
            n->pending.ticket = 0;
    }
    for (i = 0; i < SOPHIA_NS_RESOURCES; i++) {
        struct sophia_ns_resource *r = &n->resources[i];
        o = ns_ticket(n, r->ticket);
        if (r->state == SOPHIA_NS_RESOURCE_FREE || !ns_settled(o))
            continue;
        r->ticket = 0;
        if (!ns_refused(o))
            continue;
        if (r->state == SOPHIA_NS_RESOURCE_BEGUN)
            ns_resource_free(n, r);
        else if (r->state == SOPHIA_NS_RESOURCE_RETIRING)
            r->state = SOPHIA_NS_RESOURCE_ACCEPTED;
    }
}
static void ns_record(struct sophia_sf_record *r, uint16_t kind)
{
    memset(r, 0, sizeof(*r));
    r->header.kind = kind;
}
static int ns_allocation_request(struct sophia_ns *n, uint16_t operation, uint16_t edge,
                                 const struct sophia_ns_allocation_params *p, uint64_t now_ms,
                                 uint64_t *ticket)
{
    struct sophia_ns_allocation *a = &n->allocation;
    struct sophia_sf_record *r = &n->scratch;
    struct sophia_sf_native_allocation_request *v = &r->value.native_allocation_request;
    uint64_t t;
    int status;
    if (!n->next_request || n->next_request == UINT64_MAX || ns_peek_transaction(n, &t))
        return SOPHIA_9P_ARGUMENT;
    ns_record(r, SOPHIA_SF_NATIVE_ALLOCATION_REQUEST);
    v->transaction = t;
    v->grant_connection_epoch = n->limits.grant_connection_epoch;
    v->grant_content_epoch = n->limits.grant_content_epoch;
    v->opening = n->opening.opening;
    v->output_id = n->opening.output_id;
    v->output_generation = n->opening.output_generation;
    v->request_id = n->next_request;
    if (operation != 1) {
        v->prior_id = a->granted.allocation_id;
        v->prior_generation = a->granted.allocation_generation;
    }
    v->operation = operation;
    v->edge = edge;
    if (p) {
        v->desired_width = p->width;
        v->desired_height = p->height;
        v->margin_top = p->margin_top;
        v->margin_right = p->margin_right;
        v->margin_bottom = p->margin_bottom;
        v->margin_left = p->margin_left;
    }
    status = ns_submit(n, r, ticket);
    if (status)
        return status;
    n->next_transaction++;
    a->request_id = n->next_request++;
    a->ticket = *ticket;
    a->requested_ms = now_ms;
    a->opening = n->opening.opening;
    a->operation = operation;
    a->request_edge = edge;
    a->pending = 1;
    return 0;
}
int sophia_ns_allocate(struct sophia_ns *n, const struct sophia_ns_allocation_params *p,
                       uint64_t now_ms, uint64_t *ticket)
{
    const struct sophia_ns_allocation *a;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!p || !ticket || p->edge < 1 || p->edge > 4 || !p->width || !p->height)
        return SOPHIA_9P_ARGUMENT;
    a = &n->allocation;
    /* One request at a time; an allocation of a closed opening is not used
     * and blocks a new one until an invalidation (status 4) arrives, which
     * has no bounded arrival; a final session ends the wait. */
    if (!n->open || a->pending || a->orphaned)
        return SOPHIA_9P_INVALID;
    return ns_allocation_request(n, a->has_grant ? 2 : 1, p->edge, p, now_ms, ticket);
}
int sophia_ns_release(struct sophia_ns *n, uint64_t now_ms, uint64_t *ticket)
{
    const struct sophia_ns_allocation *a;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!ticket)
        return SOPHIA_9P_ARGUMENT;
    a = &n->allocation;
    if (!n->open || a->pending || !a->has_grant || a->orphaned || a->opening != n->opening.opening)
        return SOPHIA_9P_INVALID;
    return ns_allocation_request(n, 3, a->edge, NULL, now_ms, ticket);
}
int sophia_ns_upload_begin(struct sophia_ns *n, struct sophia_sf_resource_begin begin,
                           uint64_t *ticket)
{
    struct sophia_ns_resource *slot = NULL;
    unsigned i;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!ticket || !begin.resource_id || !begin.resource_generation)
        return SOPHIA_9P_ARGUMENT;
    if (n->uploading || ns_resource(n, begin.resource_id, begin.resource_generation))
        return SOPHIA_9P_INVALID;
    for (i = 0; i < SOPHIA_NS_RESOURCES && !slot; i++)
        if (n->resources[i].state == SOPHIA_NS_RESOURCE_FREE)
            slot = &n->resources[i];
    if (!slot)
        return SOPHIA_9P_BUSY;
    if (!n->next_transaction || n->next_transaction == UINT64_MAX)
        return SOPHIA_9P_ARGUMENT;
    begin.transaction = n->next_transaction;
    begin.grant_connection_epoch = n->limits.grant_connection_epoch;
    begin.grant_content_epoch = n->limits.grant_content_epoch;
    r = sophia_ss_upload_begin(n->ss, begin, ticket);
    if (r) {
        (void)ns_gate(n);
        return r;
    }
    slot->id = begin.resource_id;
    slot->generation = begin.resource_generation;
    slot->transaction = n->next_transaction++;
    slot->ticket = *ticket;
    slot->state = SOPHIA_NS_RESOURCE_BEGUN;
    n->upload = (unsigned)(slot - n->resources);
    n->uploading = 1;
    return 0;
}
static int ns_upload_finish(struct sophia_ns *n, int cancel, uint64_t *ticket)
{
    struct sophia_ns_resource *slot;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!ticket)
        return SOPHIA_9P_ARGUMENT;
    if (!n->uploading)
        return SOPHIA_9P_INVALID;
    slot = &n->resources[n->upload];
    if (!cancel && slot->state != SOPHIA_NS_RESOURCE_ADMITTED)
        return SOPHIA_9P_INVALID;
    if (!n->next_transaction || n->next_transaction == UINT64_MAX)
        return SOPHIA_9P_ARGUMENT;
    r = cancel ? sophia_ss_upload_cancel(n->ss, n->next_transaction, ticket)
               : sophia_ss_upload_end(n->ss, n->next_transaction, ticket);
    if (r) {
        (void)ns_gate(n);
        return r;
    }
    n->next_transaction++;
    /* The resource's status event settles it; the window closes here. */
    n->uploading = 0;
    return 0;
}
int sophia_ns_upload_end(struct sophia_ns *n, uint64_t *ticket)
{
    return ns_upload_finish(n, 0, ticket);
}
int sophia_ns_upload_cancel(struct sophia_ns *n, uint64_t *ticket)
{
    return ns_upload_finish(n, 1, ticket);
}
int sophia_ns_retire(struct sophia_ns *n, uint64_t id, uint64_t generation, uint64_t *ticket)
{
    struct sophia_ns_resource *slot;
    struct sophia_sf_record *rec;
    uint64_t t;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!ticket || !id || !generation)
        return SOPHIA_9P_ARGUMENT;
    slot = ns_resource(n, id, generation);
    if (!slot || slot->state != SOPHIA_NS_RESOURCE_ACCEPTED)
        return SOPHIA_9P_INVALID;
    if (ns_peek_transaction(n, &t))
        return SOPHIA_9P_ARGUMENT;
    rec = &n->scratch;
    ns_record(rec, SOPHIA_SF_RESOURCE_RETIRE);
    rec->value.resource_retire.transaction = t;
    rec->value.resource_retire.grant_connection_epoch = n->limits.grant_connection_epoch;
    rec->value.resource_retire.grant_content_epoch = n->limits.grant_content_epoch;
    rec->value.resource_retire.resource_id = id;
    rec->value.resource_retire.resource_generation = generation;
    r = ns_submit(n, rec, ticket);
    if (r)
        return r;
    n->next_transaction++;
    slot->state = SOPHIA_NS_RESOURCE_RETIRING;
    slot->ticket = *ticket;
    return 0;
}
int sophia_ns_demand(struct sophia_ns *n, uint16_t reason, uint64_t now_ms, uint64_t *ticket)
{
    const struct sophia_sf_allocation_result *g;
    struct sophia_sf_record *rec;
    uint64_t t;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!ticket || reason < 1 || reason > 3)
        return SOPHIA_9P_ARGUMENT;
    g = &n->allocation.granted;
    /* Never while a permit is unused (granted, or past its advisory bound
     * but not yet ended by FramePermit state 2/3) or a candidate is still
     * assembling: current Session treats that grant refusal as fatal. */
    if (!n->open || !n->allocation.has_grant || n->allocation.orphaned || n->permit.demand ||
        n->pending.valid)
        return SOPHIA_9P_INVALID;
    if (!n->next_demand || n->next_demand == UINT64_MAX || ns_peek_transaction(n, &t))
        return SOPHIA_9P_ARGUMENT;
    rec = &n->scratch;
    ns_record(rec, SOPHIA_SF_FRAME_DEMAND);
    rec->value.frame_demand.transaction = t;
    rec->value.frame_demand.grant_connection_epoch = n->limits.grant_connection_epoch;
    rec->value.frame_demand.grant_content_epoch = n->limits.grant_content_epoch;
    rec->value.frame_demand.output_id = n->opening.output_id;
    rec->value.frame_demand.output_generation = n->opening.output_generation;
    rec->value.frame_demand.allocation_id = g->allocation_id;
    rec->value.frame_demand.allocation_generation = g->allocation_generation;
    rec->value.frame_demand.demand_id = n->next_demand;
    rec->value.frame_demand.reason = reason;
    r = ns_submit(n, rec, ticket);
    if (r)
        return r;
    n->next_transaction++;
    n->permit.demand_id = n->next_demand++;
    n->permit.ticket = *ticket;
    n->permit.permit_id = 0;
    n->permit.demand_ms = now_ms;
    n->permit.expires_ms = 0;
    n->permit.demand = 1;
    n->permit.granted = 0;
    n->permit.cancelling = 0;
    return 0;
}
int sophia_ns_demand_cancel(struct sophia_ns *n, uint64_t now_ms, uint64_t *ticket)
{
    struct sophia_ns_permit *p;
    struct sophia_sf_record *rec;
    uint64_t t;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!ticket)
        return SOPHIA_9P_ARGUMENT;
    p = &n->permit;
    /* Only a granted permit safely inside its local bound names something
     * current; a standing demand may be granted concurrently. */
    if (!p->demand || !p->granted || p->cancelling || now_ms >= p->expires_ms ||
        p->expires_ms - now_ms <= n->config.permit_margin_ms)
        return SOPHIA_9P_INVALID;
    if (ns_peek_transaction(n, &t))
        return SOPHIA_9P_ARGUMENT;
    rec = &n->scratch;
    ns_record(rec, SOPHIA_SF_FRAME_DEMAND_CANCEL);
    rec->value.frame_demand_cancel.transaction = t;
    rec->value.frame_demand_cancel.grant_connection_epoch = n->limits.grant_connection_epoch;
    rec->value.frame_demand_cancel.grant_content_epoch = n->limits.grant_content_epoch;
    rec->value.frame_demand_cancel.output_id = n->opening.output_id;
    rec->value.frame_demand_cancel.output_generation = n->opening.output_generation;
    rec->value.frame_demand_cancel.demand_id = p->demand_id;
    rec->value.frame_demand_cancel.permit_id = p->permit_id;
    r = ns_submit(n, rec, ticket);
    if (r)
        return r;
    n->next_transaction++;
    p->granted = 0; /* never used after its cancel */
    p->cancelling = 1;
    return 0;
}
static int ns_placeable(struct sophia_ns *n, const struct sophia_sf_content_placement *p)
{
    const struct sophia_ns_resource *r = ns_resource(n, p->resource_id, p->resource_generation);
    return r && r->state == SOPHIA_NS_RESOURCE_ACCEPTED;
}
int sophia_ns_present(struct sophia_ns *n, const struct sophia_ns_scene *s, uint64_t now_ms,
                      uint64_t *ticket)
{
    const struct sophia_ns_allocation *a;
    const struct sophia_sf_allocation_result *g;
    struct sophia_sf_record *rec;
    struct sophia_sf_role_candidate *v;
    struct sophia_sf_candidate *c;
    struct sophia_ns_shown *p;
    size_t bytes;
    unsigned i;
    uint64_t t;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!s || !ticket || s->placement_count > 32 || s->row_count > SOPHIA_NS_ROWS)
        return SOPHIA_9P_ARGUMENT;
    a = &n->allocation;
    g = &a->granted;
    if (!ns_catalog_current(n) || !n->facts_installed || n->facts_installed != n->facts_announced ||
        !a->has_grant || a->pending || a->orphaned || a->opening != n->opening.opening ||
        g->output_id != n->opening.output_id ||
        g->output_generation != n->opening.output_generation || n->pending.valid)
        return SOPHIA_9P_INVALID;
    /* Never exceed a permit: granted, unconsumed, inside its advisory bound
     * (which cannot prove the server still holds it). */
    if (!n->permit.granted || n->permit.cancelling || now_ms >= n->permit.expires_ms ||
        n->permit.expires_ms - now_ms <= n->config.permit_margin_ms)
        return SOPHIA_9P_INVALID;
    for (i = 0; i < s->placement_count; i++)
        if (!ns_placeable(n, &s->placements[i]))
            return SOPHIA_9P_INVALID;
    if (!n->next_generation || n->next_generation == UINT64_MAX || ns_peek_transaction(n, &t))
        return SOPHIA_9P_ARGUMENT;
    rec = &n->scratch;
    ns_record(rec, SOPHIA_SF_NATIVE_CANDIDATE);
    v = &rec->value.role_candidate;
    c = &v->content;
    c->transaction = t;
    c->grant_connection_epoch = n->limits.grant_connection_epoch;
    c->grant_content_epoch = n->limits.grant_content_epoch;
    c->candidate_generation = n->next_generation;
    c->output_id = n->opening.output_id;
    c->output_generation = n->opening.output_generation;
    c->facts_generation = n->facts_installed;
    c->pacing_permit = n->permit.permit_id;
    c->interaction_generation = SOPHIA_NS_INTERACTION_GENERATION;
    c->surface_count = 1;
    c->placement_count = s->placement_count;
    c->target_count = s->row_count;
    c->surfaces[0].allocation_id = g->allocation_id;
    c->surfaces[0].allocation_generation = g->allocation_generation;
    c->surfaces[0].scale_generation = g->scale_generation;
    c->surfaces[0].role = 3;
    c->surfaces[0].edge = a->edge;
    c->surfaces[0].margin_top = g->margin_top;
    c->surfaces[0].margin_right = g->margin_right;
    c->surfaces[0].margin_bottom = g->margin_bottom;
    c->surfaces[0].margin_left = g->margin_left;
    c->surfaces[0].parent_surface_index = 65535;
    for (i = 0; i < s->placement_count; i++) {
        c->placements[i] = s->placements[i];
        c->placements[i].surface_index = 0;
    }
    for (i = 0; i < s->row_count; i++) {
        c->targets[i] = s->targets[i];
        c->targets[i].surface_index = 0;
        c->targets[i].action_kind = 2;
        c->targets[i].action_id = s->rows[i];
        v->rows[i] = s->rows[i];
    }
    v->opening = n->opening.opening;
    v->catalog_generation = n->opening.catalog_generation;
    v->state_revision = n->revision;
    v->selected = s->selected;
    v->row_count = s->row_count;
    if (sophia_sf_role_candidate_validate_value(SOPHIA_SF_NATIVE_CANDIDATE, v))
        return SOPHIA_9P_INVALID;
    bytes = sophia_ss_record_bytes(rec);
    if (!bytes || bytes > n->permit.max_candidate_bytes || bytes > n->limits.max_candidate_bytes)
        return SOPHIA_9P_INVALID;
    r = ns_submit(n, rec, ticket);
    if (r)
        return r;
    n->next_transaction++;
    p = &n->pending;
    memset(p, 0, sizeof(*p));
    p->transaction = t;
    p->ticket = *ticket;
    p->generation = n->next_generation++;
    p->submitted_ms = now_ms;
    p->opening = v->opening;
    p->catalog_generation = v->catalog_generation;
    p->state_revision = v->state_revision;
    p->interaction_generation = c->interaction_generation;
    p->allocation_id = g->allocation_id;
    p->allocation_generation = g->allocation_generation;
    p->output_id = c->output_id;
    p->output_generation = c->output_generation;
    p->selected = v->selected;
    p->row_count = v->row_count;
    memcpy(p->rows, v->rows, sizeof(p->rows));
    p->valid = 1;
    /* The permit is consumed by this candidate. */
    n->permit.last_demand_id = n->permit.demand_id;
    n->permit.demand = 0;
    n->permit.granted = 0;
    return 0;
}
