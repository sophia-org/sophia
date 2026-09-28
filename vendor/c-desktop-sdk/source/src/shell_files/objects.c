#include "session_internal.h"

int sophia_sf_client_object(struct sophia_sf_client *c, uint16_t kind, uint64_t generation,
                            uint64_t qid)
{
    if (!c || kind < SOPHIA_SF_LIMITS || kind > SOPHIA_SF_SHORTCUTS)
        return SOPHIA_9P_ARGUMENT;
    if (c->profile == SOPHIA_SF_DESCRIPTOR) {
        if (!sf_descriptor_object_allowed(c, kind))
            return SOPHIA_9P_ARGUMENT;
    } else if (kind > SOPHIA_SF_INDICATORS ||
        (kind == SOPHIA_SF_CATALOG && c->profile == SOPHIA_SF_BAR) ||
        (kind == SOPHIA_SF_INDICATORS &&
         (c->profile != SOPHIA_SF_BAR || !(c->offer.required_capabilities & (1u << 9)))))
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (!c->negotiated || c->object_stage || c->object_ready)
        return SOPHIA_9P_BUSY;
    c->object_kind = kind;
    c->object_generation = generation;
    c->object_qid = qid;
    c->object_used = 0;
    c->object_probe = 0;
    c->object_stage = 1;
    c->object_status = 0;
    return 0;
}
int sf_session_object_drive(struct sophia_sf_client *c)
{
    static const char *const names[] = {"limits", "outputs", "catalog", "indicators",
                                       "descriptors", "tabs", "shortcuts"};
    const char *name;
    int r;
    uint32_t count = (uint32_t)(c->object_capacity - c->object_used);
    if (count > c->wire->msize - 11u)
        count = c->wire->msize - 11u;
    if (c->object_iounit && count > c->object_iounit)
        count = c->object_iounit;
    if (!c->object_stage || c->object_op.active)
        return 0;
    name = names[c->object_kind - 1];
    switch (c->object_stage) {
    case 1:
        r = sophia_9p_walk(c->wire, c->root, &name, 1, &c->object_op.handle, &c->object_fid);
        break;
    case 2:
        r = sophia_9p_lopen(c->wire, c->object_fid, 0, &c->object_op.handle);
        break;
    case 3:
        r = sophia_9p_read(c->wire, c->object_fid, c->object_used, c->object_probe ? 1 : count,
                           &c->object_op.handle);
        break;
    default:
        r = sophia_9p_clunk(c->wire, c->object_fid, &c->object_op.handle);
        break;
    }
    return sf_started(&c->object_op, r);
}
static int object_finish(struct sophia_sf_client *c, int status)
{
    c->object_status = status;
    c->object_stage = 4;
    return 0;
}
int sf_session_object_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    size_t n;
    uint64_t generation;
    if (c->object_stage == 4) {
        if (r->type != 121)
            return SOPHIA_9P_INVALID;
        c->object_stage = 0;
        c->object_fid = UINT32_MAX;
        c->object_ready = 1;
        if (!c->object_status && c->object_kind == SOPHIA_SF_LIMITS) {
            c->limits = c->object.value.limits;
            c->have_limits = 1;
        }
        return 0;
    }
    if (r->type == 7) {
        c->remote_error = r->error;
        if (c->object_stage == 1) {
            c->object_stage = 0;
            c->object_ready = 1;
            c->object_status = r->error == 11 ? SOPHIA_9P_AGAIN : SOPHIA_9P_INVALID;
            return 0;
        }
        return object_finish(c, r->error == 11 ? SOPHIA_9P_AGAIN : SOPHIA_9P_INVALID);
    }
    if (c->object_stage == 1) {
        if (r->type != 111 || r->count != 1)
            return SOPHIA_9P_INVALID;
        c->object_stage = 2;
        return 0;
    }
    if (c->object_stage == 2) {
        c->object_iounit = r->iounit;
        if (c->object_qid && c->object_qid != r->qid.path)
            return object_finish(c, SOPHIA_9P_AGAIN);
        /* Record the pinned qid so a result can be matched to its announcement. */
        c->object_qid = r->qid.path;
        c->object_stage = 3;
        return 0;
    }
    if (!c->object_probe) {
        if (r->type != 117 || !r->count || r->count > c->object_capacity - c->object_used)
            return object_finish(c, SOPHIA_9P_INVALID);
        memcpy(c->object_storage + c->object_used, r->data, r->count);
        c->object_used += r->count;
        if (c->object_used < 4)
            return 0;
        n = (size_t)sf_get(c->object_storage, 4);
        if (n < 32 || n > c->object_capacity || c->object_used > n ||
            (c->object_kind == SOPHIA_SF_INDICATORS && n > 32768) ||
            (c->object_kind == SOPHIA_SF_DESCRIPTORS && n > 4096) ||
            (c->object_kind == SOPHIA_SF_TABS && n > 1048576) ||
            (c->object_kind == SOPHIA_SF_SHORTCUTS && n > 131072))
            return object_finish(c, SOPHIA_9P_INVALID);
        if (c->object_used < n)
            return 0;
        /* A positive short read is not EOF. After the exact record, one
         * bounded probe must find EOF; any trailing byte refuses the object. */
        c->object_probe = 1;
        return 0;
    }
    if (r->type != 117 || r->count)
        return object_finish(c, SOPHIA_9P_INVALID);
    n = c->object_used;
    if (sophia_sf_decode(c->object_storage, n, &c->object) ||
        c->object.header.kind != c->object_kind || c->object.header.epoch != c->epoch)
        return object_finish(c, SOPHIA_9P_INVALID);
    if (c->object_kind == SOPHIA_SF_LIMITS) {
        if (c->object.value.limits.grant_connection_epoch != c->epoch)
            return object_finish(c, SOPHIA_9P_INVALID);
        generation = c->object.value.limits.limits_generation;
    } else if (c->object_kind == SOPHIA_SF_OUTPUTS) {
        if (c->object.value.outputs.grant_connection_epoch != c->epoch)
            return object_finish(c, SOPHIA_9P_INVALID);
        generation = c->object.value.outputs.facts_generation;
    } else if (c->object_kind == SOPHIA_SF_CATALOG) {
        if (c->object.value.catalog.connection_epoch != c->epoch ||
            (c->profile == SOPHIA_SF_DESCRIPTOR && c->object.value.catalog.identities_present))
            return object_finish(c, SOPHIA_9P_INVALID);
        generation = c->object.value.catalog.generation;
    } else if (c->object_kind == SOPHIA_SF_INDICATORS) {
        if (c->object.value.indicators.connection_epoch != c->epoch)
            return object_finish(c, SOPHIA_9P_INVALID);
        generation = c->object.value.indicators.generation;
    } else if (c->object_kind == SOPHIA_SF_DESCRIPTORS) {
        generation = c->object.value.descriptors.snapshot_generation;
    } else if (c->object_kind == SOPHIA_SF_TABS) {
        generation = c->object.value.tabs.generation;
    } else {
        generation = c->object.value.shortcuts.generation;
    }
    return object_finish(
        c, c->object_generation && generation != c->object_generation ? SOPHIA_9P_AGAIN : 0);
}
int sophia_sf_client_object_result(struct sophia_sf_client *c, const struct sophia_sf_record **out)
{
    if (!c || !out)
        return SOPHIA_9P_ARGUMENT;
    if (!c->object_ready)
        return SOPHIA_9P_BUSY;
    c->object_ready = 0;
    if (!c->object_status)
        *out = &c->object;
    return c->object_status;
}
