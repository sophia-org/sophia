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
    const struct sophia_sf_buffers buffers = {storage, capacity, NULL, 0};
    return sophia_sf_client_init_buffers(c, wire, offer, profile, &buffers);
}
static int disjoint(const void *left, size_t n, const void *right, size_t m)
{
    uintptr_t a = (uintptr_t)left, b = (uintptr_t)right;
    return !n || !m ||
           (a <= UINTPTR_MAX - n && b <= UINTPTR_MAX - m && (a + n <= b || b + m <= a));
}
static int external_buffer(const struct sophia_sf_client *c, const struct sophia_9p_client *wire,
                           const void *p, size_t n, size_t minimum)
{
    return ((!p) == (!n)) &&
           (!p || (n >= minimum && n <= SOPHIA_SF_MAX_RECORD &&
                   disjoint(p, n, c, sizeof(*c)) && disjoint(p, n, wire, sizeof(*wire)) &&
                   disjoint(p, n, wire->storage,
                            sophia_9p_storage_bytes(wire->offered, wire->capacity))));
}
int sophia_sf_client_init_buffers(struct sophia_sf_client *c, struct sophia_9p_client *wire,
                                  struct sophia_sf_negotiate offer, enum sophia_sf_profile profile,
                                  const struct sophia_sf_buffers *buffers)
{
    const struct sophia_sf_buffers v = buffers ? *buffers : (struct sophia_sf_buffers){0};
    if (!c || !wire || wire->phase || !offer.minimum_revision ||
        offer.minimum_revision > offer.maximum_revision || wire->capacity < 8 ||
        profile < SOPHIA_SF_BAR || profile > SOPHIA_SF_DESCRIPTOR ||
        !external_buffer(c, wire, v.objects, v.object_capacity, 296) ||
        !external_buffer(c, wire, v.transaction, v.transaction_capacity, 8192) ||
        !disjoint(v.objects, v.object_capacity, v.transaction, v.transaction_capacity))
        return SOPHIA_9P_ARGUMENT;
    if (profile == SOPHIA_SF_DESCRIPTOR && !sf_descriptor_offer(&offer))
        return SOPHIA_9P_ARGUMENT;
    if (profile == SOPHIA_SF_LAUNCHER || profile == SOPHIA_SF_DOCK) {
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
    c->object_storage = v.objects ? v.objects : c->object_bytes;
    c->object_capacity = v.objects ? v.object_capacity : sizeof(c->object_bytes);
    c->tx_storage = v.transaction ? v.transaction : c->tx;
    c->tx_capacity = v.transaction ? v.transaction_capacity : sizeof(c->tx);
    c->next_submission = 1;
    c->object_fid = c->upload_fid = UINT32_MAX;
    return sf_started(&c->boot_op, sophia_9p_version(wire, &c->boot_op.handle));
}
int sophia_sf_client_ready(const struct sophia_sf_client *c)
{
    if (!c || c->terminal || c->refused || !c->negotiated)
        return 0;
    if (c->profile == SOPHIA_SF_DESCRIPTOR)
        return c->bootstrap_custody_consumed && c->welcome_consumed &&
               (!(c->welcome.capabilities & (1u << 7)) || c->have_limits);
    return c->have_limits;
}
const struct sophia_sf_negotiated *sophia_sf_client_welcome(const struct sophia_sf_client *c)
{
    return c && c->negotiated ? &c->welcome : NULL;
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
    if (c->submit_stage && !c->submit_op.active && !(c->submit_stage == 2 && c->submit_wait)) {
        if (c->submit_stage == 1) {
            size_t n = c->tx_size - c->tx_offset, cap = c->wire->msize - 23u;
            if (c->iounit[1] && cap > c->iounit[1])
                cap = c->iounit[1];
            if (n > cap)
                n = cap;
            r = sophia_9p_write(c->wire, c->fids[1], c->tx_offset, c->tx_storage + c->tx_offset, n,
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
        else if (c->submit_stage == 6) {
            /* Clunk cannot release accepted custody. A caller may defer its
             * cumulative ack behind an object fetch or event consumption;
             * keep servicing those lanes without opening a busy transaction. */
            if (c->submitted && c->acked_sequence < c->submitted_sequence)
                r = SOPHIA_9P_BUSY;
            else
                r = sophia_9p_lopen(c->wire, c->fids[1], 2, &c->submit_op.handle);
        }
        else
            r = SOPHIA_9P_BUSY;
        r = sf_started(&c->submit_op, r);
        if (r)
            return r;
        if (c->submit_stage == 2 && c->submit_op.active)
            c->submit_sent = 1;
    }
    r = sf_session_object_drive(c);
    if (r)
        return r;
    return sf_session_upload_drive(c);
}
/* ESTALE on the events, submit or ack stream is kept distinct from a
 * protocol violation. Object retention and upload slots never set it. */
static int remote_failure(struct sophia_sf_client *c, uint32_t error, int stream)
{
    c->remote_error = error;
    if (stream && error == 116)
        c->stale = 1;
    return SOPHIA_9P_INVALID;
}
static int submission_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    if (r->type == 7 && c->submit_stage == 2) {
        /* EAGAIN transferred nothing: never re-driven in this service pass. */
        if (r->error == 11) {
            c->submit_sent = 0;
            c->submit_wait = 1;
            return 0;
        }
        /* EALREADY after observed custody is that custody; otherwise fatal. */
        if (r->error == 114 && c->submitted) {
            c->submit_replied = 1;
            c->submit_stage = 4;
            return 0;
        }
        /* Any other valid Rlerror, unknown errno included, is a definitive
         * refusal with no meaning beyond its errno: nothing was journaled,
         * and clunk discards the staging. */
        if (r->error != 114 && r->error != 116 && c->negotiated && !c->submitted) {
            uint64_t kind = sf_get(c->tx_storage + 6, 2);
            c->submit_sent = 0;
            c->submit_error = r->error;
            c->submit_stage = 4;
            /* A refused upload record leaves nothing for the writer to wait on. */
            if ((kind == SOPHIA_SF_RESOURCE_BEGIN && c->upload_stage == 1) ||
                ((kind == SOPHIA_SF_RESOURCE_END || kind == SOPHIA_SF_RESOURCE_CANCEL) &&
                 c->upload_stage == 6))
                c->upload_closing = 1;
            return 0;
        }
    }
    if (r->type == 7)
        return remote_failure(c, r->error, 1);
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
        if (r->type == 7)
            return remote_failure(c, r->error, 0);
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
        if (r->type == 7)
            return remote_failure(c, r->error, 1);
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
        if (r->type == 7)
            return remote_failure(c, r->error, 1);
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
static void submission_arm(struct sophia_sf_client *c, size_t n)
{
    c->next_submission++;
    c->tx_size = n;
    c->tx_offset = 0;
    c->submit_stage = 1;
    c->submitted = c->submit_replied = 0;
    c->submitted_sequence = 0;
    c->submit_wait = c->submit_sent = 0;
    c->submit_error = 0;
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
    status = sophia_sf_encode(c->tx_storage, c->tx_capacity, &r, &n);
    if (status)
        return status;
    submission_arm(c, n);
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
    if (!sf_session_candidate_allowed(c, r->header.kind))
        return SOPHIA_9P_ARGUMENT;
    return sf_session_queue(c, r);
}
int sophia_sf_client_submit_bytes(struct sophia_sf_client *c, const void *record, size_t bytes)
{
    const uint8_t *b = record;
    struct sophia_sf_record value;
    uint64_t kind;
    if (!c || !b || bytes < SOPHIA_SF_HEADER_BYTES || bytes > c->tx_capacity)
        return SOPHIA_9P_ARGUMENT;
    kind = sf_get(b + 6, 2);
    if (sf_get(b, 4) != bytes || sf_get(b + 4, 2) != 1 || kind <= SOPHIA_SF_NEGOTIATE ||
        sf_get(b + 16, 8) || sf_get(b + 24, 8))
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (!sophia_sf_client_ready(c) || c->submit_stage)
        return SOPHIA_9P_BUSY;
    if (sf_get(b + 8, 8) != c->epoch || c->next_submission == UINT64_MAX)
        return SOPHIA_9P_ARGUMENT;
    if (!sf_session_candidate_allowed(c, (uint16_t)kind))
        return SOPHIA_9P_ARGUMENT;
    /* tx is idle scratch until armed; a refused record changes no state. */
    memmove(c->tx_storage, b, bytes);
    sf_put(c->tx_storage + 16, c->next_submission, 8);
    if (sophia_sf_decode(c->tx_storage, bytes, &value))
        return SOPHIA_9P_INVALID;
    submission_arm(c, bytes);
    return 0;
}
int sophia_sf_client_submission(const struct sophia_sf_client *c, uint64_t *id,
                                enum sophia_sf_submission *stage)
{
    if (!c || !id || !stage)
        return SOPHIA_9P_ARGUMENT;
    *id = c->next_submission - 1;
    if (!*id)
        *stage = SOPHIA_SF_SUBMISSION_NONE;
    else if (c->submitted)
        *stage = SOPHIA_SF_SUBMISSION_CUSTODIED;
    else if (c->submit_error)
        *stage = SOPHIA_SF_SUBMISSION_REFUSED;
    else if (c->submit_stage == 1 || (c->submit_stage == 2 && !c->submit_sent))
        *stage = SOPHIA_SF_SUBMISSION_STAGED;
    else if (c->submit_stage == 2 || c->submit_stage == 3)
        *stage = SOPHIA_SF_SUBMISSION_ISSUED;
    else
        *stage = SOPHIA_SF_SUBMISSION_NONE;
    return 0;
}
int sophia_sf_client_submit_retry(struct sophia_sf_client *c)
{
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (c->submit_stage != 2 || !c->submit_wait)
        return SOPHIA_9P_ARGUMENT;
    c->submit_wait = 0;
    return 0;
}
int sophia_sf_client_ack_through(struct sophia_sf_client *c, uint64_t sequence)
{
    uint8_t b[16];
    int r;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (sequence > c->consumed_sequence)
        return SOPHIA_9P_ARGUMENT;
    if (c->ack_op.active)
        return SOPHIA_9P_BUSY;
    if (sequence <= c->acked_sequence)
        return 0;
    if (sophia_sf_ack_encode(b, c->epoch, sequence))
        return SOPHIA_9P_ARGUMENT;
    r = sophia_9p_write(c->wire, c->fids[3], 0, b, 16, &c->ack_op.handle);
    if (!r) {
        c->ack_op.active = 1;
        c->ack_pending = sequence;
    }
    return r;
}
