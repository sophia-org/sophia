#include "session_internal.h"

static const char *const names[] = {"events", "transaction", "submit", "ack"};
int sophia_sf_client_init(struct sophia_sf_client *c, struct sophia_9p_client *wire,
                          struct sophia_sf_negotiate offer)
{
    return sophia_sf_client_init_profile(c, wire, offer, SOPHIA_SF_BAR, NULL, 0);
}
int sophia_sf_client_init_profile(struct sophia_sf_client *c, struct sophia_9p_client *wire,
                                  struct sophia_sf_negotiate offer, enum sophia_sf_profile profile,
                                  void *storage, size_t capacity)
{
    if (!c || !wire || wire->phase || !offer.minimum_revision ||
        offer.minimum_revision > offer.maximum_revision || wire->capacity < 8 ||
        profile < SOPHIA_SF_BAR || profile > SOPHIA_SF_DOCK || ((!storage) != (!capacity)) ||
        (storage && (capacity < 296 || capacity > SOPHIA_SF_MAX_RECORD)))
        return SOPHIA_9P_ARGUMENT;
    if (profile != SOPHIA_SF_BAR) {
        unsigned revision = profile == SOPHIA_SF_LAUNCHER ? 7 : 8;
        uint64_t mask = profile == SOPHIA_SF_LAUNCHER ? 0x9a0 : 0x11a2;
        if (offer.minimum_revision > revision || offer.maximum_revision < revision ||
            offer.required_capabilities != mask)
            return SOPHIA_9P_ARGUMENT;
    }
    memset(c, 0, sizeof(*c));
    c->wire = wire;
    c->offer = offer;
    c->profile = profile;
    c->object_storage = storage ? storage : c->object_bytes;
    c->object_capacity = storage ? capacity : sizeof(c->object_bytes);
    c->next_submission = 1;
    c->object_fid = c->upload_fid = UINT32_MAX;
    return sf_started(&c->boot_op, sophia_9p_version(wire, &c->boot_op.handle));
}
int sophia_sf_client_ready(const struct sophia_sf_client *c)
{
    return c && !c->terminal && !c->refused && c->negotiated && c->have_limits;
}
static int bootstrap_drive(struct sophia_sf_client *c)
{
    int r;
    unsigned index;
    if (c->boot_op.active || c->bootstrap >= 14)
        return 0;
    if (c->bootstrap == 1)
        r = sophia_9p_attach(c->wire, "", "", &c->boot_op.handle, &c->root);
    else if (c->bootstrap == 2) {
        const char *api = "api";
        r = sophia_9p_walk(c->wire, c->root, &api, 1, &c->boot_op.handle, &c->api_fid);
    } else if (c->bootstrap == 3)
        r = sophia_9p_lopen(c->wire, c->api_fid, 0, &c->boot_op.handle);
    else if (c->bootstrap == 4) {
        uint32_t count = 257u - (uint32_t)c->api_used;
        if (c->api_iounit && count > c->api_iounit)
            count = c->api_iounit;
        r = sophia_9p_read(c->wire, c->api_fid, c->api_used, count, &c->boot_op.handle);
    } else if (c->bootstrap == 5)
        r = sophia_9p_clunk(c->wire, c->api_fid, &c->boot_op.handle);
    else {
        index = (c->bootstrap - 6) / 2;
        if (!(c->bootstrap % 2))
            r = sophia_9p_walk(c->wire, c->root, &names[index], 1, &c->boot_op.handle,
                               &c->fids[index]);
        else
            r = sophia_9p_lopen(c->wire, c->fids[index], index == 0 ? 0 : (index == 1 ? 2 : 1),
                                &c->boot_op.handle);
    }
    return sf_started(&c->boot_op, r);
}
int sf_session_drive(struct sophia_sf_client *c)
{
    int r = bootstrap_drive(c);
    if (r || c->bootstrap < 14)
        return r;
    if (!c->event_ready && !c->event_op.active) {
        uint32_t count = (uint32_t)(sizeof(c->event_bytes) - c->event_used);
        if (c->iounit[0] && count > c->iounit[0])
            count = c->iounit[0];
        r = sophia_9p_read(c->wire, c->fids[0], c->event_offset, count, &c->event_op.handle);
        r = sf_started(&c->event_op, r);
        if (r)
            return r;
    }
    if (c->submit_stage && !c->submit_op.active) {
        if (c->submit_stage == 1) {
            size_t n = c->tx_size - c->tx_offset, cap = c->wire->msize - 23u;
            if (c->iounit[1] && cap > c->iounit[1])
                cap = c->iounit[1];
            if (n > cap)
                n = cap;
            r = sophia_9p_write(c->wire, c->fids[1], c->tx_offset, c->tx + c->tx_offset, n,
                                &c->submit_op.handle);
        } else if (c->submit_stage == 2) {
            uint8_t b[24];
            if (sophia_sf_submit_encode(b, c->epoch, c->next_submission - 1, (uint32_t)c->tx_size))
                return -1;
            r = sophia_9p_write(c->wire, c->fids[2], 0, b, 24, &c->submit_op.handle);
        } else if (c->submit_stage == 4)
            r = sophia_9p_clunk(c->wire, c->fids[1], &c->submit_op.handle);
        else if (c->submit_stage == 5)
            r = sophia_9p_walk(c->wire, c->root, &names[1], 1, &c->submit_op.handle, &c->fids[1]);
        else if (c->submit_stage == 6)
            r = sophia_9p_lopen(c->wire, c->fids[1], 2, &c->submit_op.handle);
        else
            r = SOPHIA_9P_BUSY;
        r = sf_started(&c->submit_op, r);
        if (r)
            return r;
    }
    r = sf_session_object_drive(c);
    if (r)
        return r;
    return sf_session_upload_drive(c);
}
static int submission_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    if (r->type == 7) {
        if (r->error == 11 && c->submit_stage == 2)
            return 0;
        c->remote_error = r->error;
        return SOPHIA_9P_INVALID;
    }
    if (c->submit_stage >= 4) {
        if (c->submit_stage == 5 && r->count != 1)
            return SOPHIA_9P_INVALID;
        if (c->submit_stage == 6)
            c->iounit[1] = r->iounit;
        c->submit_stage = c->submit_stage == 6 ? 0 : (uint8_t)(c->submit_stage + 1);
        return 0;
    }
    if (r->type != 119 || !r->count)
        return SOPHIA_9P_INVALID;
    if (c->submit_stage == 1) {
        c->tx_offset += r->count;
        if (c->tx_offset == c->tx_size)
            c->submit_stage = 2;
    } else {
        if (r->count != 24)
            return SOPHIA_9P_INVALID;
        c->submit_replied = 1;
        c->submit_stage = c->submitted ? 4 : 3;
    }
    return 0;
}
static int receive(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    if (sf_same(&c->boot_op, r->handle)) {
        c->boot_op.active = 0;
        if (r->type == 7) {
            c->remote_error = r->error;
            return SOPHIA_9P_INVALID;
        }
        if (r->type == 111 && r->count != 1)
            return SOPHIA_9P_INVALID;
        if (c->bootstrap == 4) {
            if (r->count > 256u - c->api_used)
                return SOPHIA_9P_INVALID;
            if (r->count) {
                memcpy(c->event_bytes + c->api_used, r->data, r->count);
                c->api_used += r->count;
                return 0;
            }
            if (sf_api_epoch(c->event_bytes, c->api_used, &c->epoch) ||
                sf_api_profile(c->event_bytes, c->api_used, c->profile))
                return SOPHIA_9P_INVALID;
        }
        if (r->type == 13 && c->bootstrap == 3)
            c->api_iounit = r->iounit;
        else if (r->type == 13) {
            unsigned index = (c->bootstrap - 6) / 2;
            if (r->iounit && ((index == 2 && r->iounit < 24) || (index == 3 && r->iounit < 16)))
                return SOPHIA_9P_INVALID;
            c->iounit[index] = r->iounit;
        }
        c->bootstrap++;
        if (c->bootstrap == 14) {
            struct sophia_sf_record record = {0};
            record.header.kind = SOPHIA_SF_NEGOTIATE;
            record.value.negotiate = c->offer;
            return sf_session_queue(c, &record);
        }
        return 0;
    }
    if (sf_same(&c->submit_op, r->handle)) {
        c->submit_op.active = 0;
        return submission_reply(c, r);
    }
    if (sf_same(&c->event_op, r->handle)) {
        c->event_op.active = 0;
        if (r->type != 117 || !r->count || r->count > sizeof(c->event_bytes) - c->event_used ||
            r->count > UINT64_MAX - c->event_offset)
            return SOPHIA_9P_INVALID;
        memcpy(c->event_bytes + c->event_used, r->data, r->count);
        c->event_used += r->count;
        c->event_offset += r->count;
        return sf_session_event_parse(c);
    }
    if (sf_same(&c->ack_op, r->handle)) {
        c->ack_op.active = 0;
        if (r->type != 119 || r->count != 16)
            return SOPHIA_9P_INVALID;
        c->acked_sequence = c->ack_pending;
        return 0;
    }
    if (sf_same(&c->object_op, r->handle)) {
        c->object_op.active = 0;
        return sf_session_object_reply(c, r);
    }
    if (sf_same(&c->upload_op, r->handle)) {
        c->upload_op.active = 0;
        return sf_session_upload_reply(c, r);
    }
    return SOPHIA_9P_INVALID;
}
int sophia_sf_client_service(struct sophia_sf_client *c, size_t budget)
{
    struct sophia_9p_reply reply;
    int r;
    unsigned i;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    r = sf_session_drive(c);
    if (r)
        return c->terminal = r;
    r = sophia_9p_service(c->wire, budget);
    if (r)
        return c->terminal = r;
    for (i = 0; i < 64; i++) {
        r = sophia_9p_peek(c->wire, &reply);
        if (r == SOPHIA_9P_AGAIN)
            break;
        if (r)
            return c->terminal = r;
        r = receive(c, &reply);
        if (r)
            return c->terminal = r;
        r = sophia_9p_consume(c->wire, reply.handle);
        if (r)
            return c->terminal = r;
    }
    r = sf_session_drive(c);
    if (r)
        c->terminal = r;
    return r;
}
int sf_session_queue(struct sophia_sf_client *c, const struct sophia_sf_record *value)
{
    struct sophia_sf_record r;
    size_t n;
    int status;
    if (c->submit_stage)
        return SOPHIA_9P_BUSY;
    if (c->next_submission == UINT64_MAX || value->header.kind < 256)
        return SOPHIA_9P_ARGUMENT;
    r = *value;
    r.header.epoch = c->epoch;
    r.header.submission = c->next_submission;
    r.header.sequence = 0;
    status = sophia_sf_encode(c->tx, sizeof(c->tx), &r, &n);
    if (status)
        return status;
    c->next_submission++;
    c->tx_size = n;
    c->tx_offset = 0;
    c->submit_stage = 1;
    c->submitted = c->submit_replied = 0;
    return 0;
}
int sophia_sf_client_submit(struct sophia_sf_client *c, const struct sophia_sf_record *r)
{
    if (!c || !r || r->header.kind == SOPHIA_SF_NEGOTIATE)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (!sophia_sf_client_ready(c))
        return SOPHIA_9P_BUSY;
    return sf_session_queue(c, r);
}
