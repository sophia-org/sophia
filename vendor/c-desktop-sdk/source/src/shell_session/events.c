#include "internal.h"

int sophia_ss_event(struct sophia_ss *s, const struct sophia_sf_record **out)
{
    if (!s || !out)
        return SOPHIA_9P_ARGUMENT;
    return sophia_sf_client_event(&s->files, out);
}
/* ObjectPublished becomes an ack obligation for its kind when consumed. A
 * newer announcement of that kind replaces the object owed but keeps the
 * earliest bound: nothing from the first unfetched announcement on may be
 * acknowledged until the newest one is fetched. When the announced object was
 * already fetched and verified (peek, fetch, then consume), it is discharged
 * with every older announcement it supersedes. */
int sophia_ss_consume(struct sophia_ss *s)
{
    const struct sophia_sf_record *e = NULL;
    uint64_t before;
    int r;
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    r = sophia_sf_client_event(&s->files, &e);
    if (r)
        return r == SOPHIA_9P_AGAIN ? SOPHIA_9P_ARGUMENT : r;
    before = s->last_consumed;
    if (e->header.kind == SOPHIA_SF_OBJECT_PUBLISHED &&
        e->value.object_published.object_kind >= SOPHIA_SF_LIMITS &&
        e->value.object_published.object_kind <= SOPHIA_SF_SHORTCUTS) {
        const struct sophia_sf_object_published *v = &e->value.object_published;
        struct sophia_ss_hold *h = &s->holds[v->object_kind - 1];
        const struct sophia_ss_hold *seen = &s->seen[v->object_kind - 1];
        if (seen->active && seen->generation == v->generation && seen->qid == v->qid)
            h->active = 0;
        else {
            if (!h->active)
                h->before = before;
            h->sequence = e->header.sequence;
            h->generation = v->generation;
            h->qid = v->qid;
            h->active = 1;
        }
    }
    ss_consumed(s, e->header.sequence);
    r = sophia_sf_client_event_consume(&s->files);
    return ss_final(s) ? r : ss_pump(s, 0);
}
uint64_t sophia_ss_ack_limit(const struct sophia_ss *s)
{
    uint64_t limit;
    unsigned i;
    if (!s)
        return 0;
    limit = s->last_consumed;
    for (i = 0; i < 7; i++)
        if (s->holds[i].active && s->holds[i].before < limit)
            limit = s->holds[i].before;
    return limit;
}
int sophia_ss_ack(struct sophia_ss *s)
{
    uint64_t limit;
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    if (s->files.ack_op.active)
        return SOPHIA_9P_BUSY;
    limit = sophia_ss_ack_limit(s);
    if (limit <= s->files.acked_sequence)
        return 0;
    return sophia_sf_client_ack_through(&s->files, limit);
}
int sophia_ss_object(struct sophia_ss *s, uint16_t kind, uint64_t generation, uint64_t qid)
{
    int r;
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    if (s->state != SOPHIA_SS_READY || s->object_requested)
        return SOPHIA_9P_BUSY;
    r = sophia_sf_client_object(&s->files, kind, generation, qid);
    if (r)
        return r;
    s->object_requested = 1;
    return ss_pump(s, 0);
}
static uint64_t generation_of(const struct sophia_sf_record *o)
{
    switch (o->header.kind) {
    case SOPHIA_SF_LIMITS:
        return o->value.limits.limits_generation;
    case SOPHIA_SF_OUTPUTS:
        return o->value.outputs.facts_generation;
    case SOPHIA_SF_CATALOG:
        return o->value.catalog.generation;
    case SOPHIA_SF_DESCRIPTORS:
        return o->value.descriptors.snapshot_generation;
    case SOPHIA_SF_TABS:
        return o->value.tabs.generation;
    case SOPHIA_SF_SHORTCUTS:
        return o->value.shortcuts.generation;
    default:
        return o->value.indicators.generation;
    }
}
int sophia_ss_object_result(struct sophia_ss *s, const struct sophia_sf_record **out)
{
    const struct sophia_sf_record *o = NULL;
    struct sophia_ss_hold *h;
    int r;
    if (!s || !out)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    if (!s->object_requested)
        return SOPHIA_9P_ARGUMENT;
    r = sophia_sf_client_object_result(&s->files, &o);
    if (r == SOPHIA_9P_BUSY)
        return r;
    s->object_requested = 0;
    if (r)
        return r;
    /* Reached only after full decode, epoch checks and the EOF probe; a
     * failed fetch returned above and leaves the verified record unchanged. */
    s->seen[o->header.kind - 1].generation = generation_of(o);
    s->seen[o->header.kind - 1].qid = s->files.object_qid;
    s->seen[o->header.kind - 1].active = 1;
    /* The hold names the latest consumed announcement, so a newer snapshot
     * releases it (and what it superseded) only when it is that one. */
    h = &s->holds[o->header.kind - 1];
    if (h->active && h->qid == s->files.object_qid && h->generation == generation_of(o))
        h->active = 0;
    *out = o;
    return 0;
}
