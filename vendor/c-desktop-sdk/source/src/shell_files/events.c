#include "session_internal.h"

int sf_session_event_parse(struct sophia_sf_client *c)
{
    size_t n;
    struct sophia_sf_record *r = &c->event;
    int status;
    if (c->event_ready || c->event_used < 4)
        return 0;
    n = (size_t)sf_get(c->event_bytes, 4);
    if (n < 32 || n > sizeof(c->event_bytes))
        return SOPHIA_9P_INVALID;
    if (n > c->event_used)
        return 0;
    if (sophia_sf_decode(c->event_bytes, n, r) || r->header.epoch != c->epoch ||
        r->header.kind < 16 || r->header.kind >= 256 || r->header.sequence <= c->sequence)
        return SOPHIA_9P_INVALID;
    switch (r->header.kind) {
    case SOPHIA_SF_NEGOTIATED: {
        const struct sophia_sf_negotiated *v = &r->value.negotiated;
        if (c->negotiated || c->refused || v->selected_revision < c->offer.minimum_revision ||
            v->selected_revision > c->offer.maximum_revision ||
            (v->capabilities & c->offer.required_capabilities) != c->offer.required_capabilities)
            return SOPHIA_9P_INVALID;
        if (c->profile != SOPHIA_SF_BAR &&
            (v->selected_revision != (c->profile == SOPHIA_SF_LAUNCHER ? 7 : 8) ||
             v->capabilities != c->offer.required_capabilities))
            return SOPHIA_9P_INVALID;
        c->negotiated = 1;
        if (v->limits_published) {
            status = sophia_sf_client_object(c, SOPHIA_SF_LIMITS, 0, 0);
            if (status)
                return status;
        }
        break;
    }
    case SOPHIA_SF_REFUSED:
        c->refused = 1;
        break;
    case SOPHIA_SF_SUBMITTED:
        /* Custody cannot follow a definitive refusal of the same submission. */
        if (!c->submit_stage || c->submitted || c->submit_error ||
            r->value.submitted.submission_id != c->next_submission - 1 ||
            r->value.submitted.candidate_kind != sf_get(c->tx + 6, 2))
            return SOPHIA_9P_INVALID;
        c->submitted = 1;
        c->submitted_sequence = r->header.sequence;
        if (c->submit_replied)
            c->submit_stage = 4;
        break;
    case SOPHIA_SF_RESOURCE_STATUS:
        status = sf_session_upload_event(c, r);
        if (status)
            return status;
        break;
    default:
        break;
    }
    c->sequence = r->header.sequence;
    c->event_ready = 1;
    return 0;
}
int sophia_sf_client_event(struct sophia_sf_client *c, const struct sophia_sf_record **out)
{
    if (!c || !out)
        return SOPHIA_9P_ARGUMENT;
    if (!c->event_ready)
        return c->terminal ? c->terminal : SOPHIA_9P_AGAIN;
    *out = &c->event;
    return 0;
}
int sophia_sf_client_event_consume(struct sophia_sf_client *c)
{
    size_t n;
    int r;
    if (!c || !c->event_ready)
        return SOPHIA_9P_ARGUMENT;
    n = (size_t)sf_get(c->event_bytes, 4);
    c->consumed_sequence = c->event.header.sequence;
    c->event_used -= n;
    memmove(c->event_bytes, c->event_bytes + n, c->event_used);
    c->event_ready = 0;
    r = sf_session_event_parse(c);
    if (r)
        c->terminal = r;
    return r;
}
int sophia_sf_client_ack(struct sophia_sf_client *c)
{
    uint8_t b[16];
    int r;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (c->ack_op.active)
        return SOPHIA_9P_BUSY;
    if (c->consumed_sequence == c->acked_sequence)
        return 0;
    if (sophia_sf_ack_encode(b, c->epoch, c->consumed_sequence))
        return SOPHIA_9P_ARGUMENT;
    r = sophia_9p_write(c->wire, c->fids[3], 0, b, 16, &c->ack_op.handle);
    if (!r) {
        c->ack_op.active = 1;
        c->ack_pending = c->consumed_sequence;
    }
    return r;
}
