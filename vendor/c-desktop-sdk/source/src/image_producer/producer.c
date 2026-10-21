#include "internal.h"
#include <errno.h>
#include <unistd.h>

static int clean(enum sophia_ip_cause c)
{
    return c == SOPHIA_IPX_ABSENT || c == SOPHIA_IPX_UNAVAILABLE || c == SOPHIA_IPX_EXHAUSTED;
}

int ip_clock(struct sophia_ip *p, uint64_t now_ms)
{
    if (now_ms < p->last_now)
        return 0;
    p->last_now = now_ms;
    return 1;
}

void ip_fail(struct sophia_ip *p, enum sophia_ip_cause cause)
{
    if (p->fd >= 0) {
        close(p->fd);
        p->fd = -1;
    }
    if (p->channel != SOPHIA_IPC_CLOSED || (clean((enum sophia_ip_cause)p->cause) && !clean(cause)))
        p->cause = (uint8_t)cause;
    p->channel = SOPHIA_IPC_CLOSED;
}

struct sophia_ip_slot *ip_slot(struct sophia_ip *p, uint64_t offer)
{
    unsigned i;
    if (!offer)
        return NULL;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++)
        if (p->slots[i].state != SOPHIA_IPO_FREE && p->slots[i].metadata.offer == offer)
            return &p->slots[i];
    return NULL;
}

void ip_free_slot(struct sophia_ip_slot *s)
{
    if (s->fd >= 0)
        close(s->fd);
    memset(s, 0, sizeof *s);
    s->fd = -1;
}

int sophia_ip_init(struct sophia_ip *p, const struct sophia_ip_config *c, uint64_t now_ms)
{
    unsigned i;
    if (!p || !c || !c->connection_epoch || !c->transfer_timeout_ms || now_ms >= c->startup_deadline_ms ||
        !c->granted_kinds)
        return SOPHIA_IPR_ARGUMENT;
    /* Shell r10 is exactly the owned ring; r9 and lock r2 subsets of 1|2. */
    if (c->role == SOPHIA_IC_ROLE_SHELL && c->revision == 10
            ? c->granted_kinds != SOPHIA_IC_KIND_BIT_OWNED_RING
            : (c->granted_kinds & ~3u) != 0)
        return SOPHIA_IPR_ARGUMENT;
    if (c->role == SOPHIA_IC_ROLE_SHELL ? c->revision != 9 && c->revision != 10
        : c->role == SOPHIA_IC_ROLE_LOCK ? c->revision != 2
                                         : 1)
        return SOPHIA_IPR_ARGUMENT;
    memset(p, 0, sizeof *p);
    p->config = *c;
    p->last_now = now_ms;
    p->next_offer = 1;
    p->fd = -1;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++)
        p->slots[i].fd = -1;
    return SOPHIA_IPR_OK;
}

/* The kind this producer's sources use: owned ring at shell r10, else sealed. */
static uint32_t source_kind_bit(const struct sophia_ip *p)
{
    return p->config.revision == 10 ? SOPHIA_IC_KIND_BIT_OWNED_RING : SOPHIA_IC_KIND_BIT_SEALED;
}

static int same_generations(const struct sophia_ic_discovery *d, const struct sophia_ic_record *l,
                            const struct sophia_ic_record *f)
{
    return l->value.limits.channel_generation == d->channel_generation &&
           f->value.formats.channel_generation == d->channel_generation &&
           l->value.limits.limits_generation == d->limits_generation &&
           f->value.formats.limits_generation == d->limits_generation &&
           f->value.formats.formats_generation == d->formats_generation;
}

int sophia_ip_discover(struct sophia_ip *p, const void *text, size_t text_bytes,
                       const void *limits, size_t limits_bytes, const void *formats,
                       size_t formats_bytes, int read_errno, uint64_t pass, uint64_t now_ms)
{
    struct sophia_ic_discovery d;
    struct sophia_ic_record l, f;
    if (!p || !ip_clock(p, now_ms))
        return SOPHIA_IPR_ARGUMENT;
    if (p->channel != SOPHIA_IPC_IDLE)
        return p->channel == SOPHIA_IPC_CLOSED ? SOPHIA_IPR_CLOSED : SOPHIA_IPR_REFUSED;
    if (now_ms >= p->config.startup_deadline_ms) {
        ip_fail(p, SOPHIA_IPX_TIMEOUT);
        return SOPHIA_IPR_CLOSED;
    }
    if (p->have_pass && pass == p->last_pass)
        return SOPHIA_IPR_AGAIN;
    p->have_pass = 1;
    p->last_pass = pass;
    if (read_errno) {
        ip_fail(p, read_errno == ENOENT ? SOPHIA_IPX_ABSENT : SOPHIA_IPX_DISCOVERY);
        return SOPHIA_IPR_CLOSED;
    }
    if (!text || !limits || !formats ||
        sophia_ic_discovery_decode(text, text_bytes, &d) != SOPHIA_IC_OK ||
        sophia_ic_record_decode(limits, limits_bytes, &l) != SOPHIA_IC_OK ||
        sophia_ic_record_decode(formats, formats_bytes, &f) != SOPHIA_IC_OK ||
        l.header.kind != SOPHIA_IC_IMAGE_LIMITS || f.header.kind != SOPHIA_IC_IMAGE_FORMATS ||
        d.role != p->config.role || d.connection_epoch != p->config.connection_epoch ||
        l.header.epoch != d.connection_epoch || f.header.epoch != d.connection_epoch) {
        ip_fail(p, SOPHIA_IPX_DISCOVERY);
        return SOPHIA_IPR_CLOSED;
    }
    if (!same_generations(&d, &l, &f))
        return SOPHIA_IPR_AGAIN;
    if (sophia_ic_discovery_coherent(&d, &l, &f, &p->config.bounds, &p->expect) != SOPHIA_IC_OK) {
        ip_fail(p, SOPHIA_IPX_DISCOVERY);
        return SOPHIA_IPR_CLOSED;
    }
    /* Discovery may withhold a negotiated kind (availability) but never
     * names one negotiation did not grant. Version 1 producers send sealed
     * sources only; shell r10 producers send owned rings only. */
    if (p->expect.source_kinds & ~p->config.granted_kinds) {
        ip_fail(p, SOPHIA_IPX_DISCOVERY);
        return SOPHIA_IPR_CLOSED;
    }
    if (!(p->expect.source_kinds & source_kind_bit(p))) {
        ip_fail(p, SOPHIA_IPX_UNAVAILABLE);
        return SOPHIA_IPR_CLOSED;
    }
    p->hello_timeout_ms = d.hello_timeout_ms;
    p->max_offers = d.max_offers;
    p->channel = SOPHIA_IPC_DISCOVERED;
    return SOPHIA_IPR_OK;
}

static int fault(struct sophia_ip *p)
{
    ip_fail(p, SOPHIA_IPX_FAULT);
    return SOPHIA_IPR_CLOSED;
}

static int channel_record(struct sophia_ip *p, const struct sophia_ic_record *r)
{
    const struct sophia_ic_channel_state *c = &r->value.channel;
    if (p->channel == SOPHIA_IPC_IDLE || p->channel == SOPHIA_IPC_DISCOVERED ||
        c->channel_generation != p->expect.channel_generation)
        return fault(p);
    if (r->header.kind == SOPHIA_IC_CHANNEL_READY) {
        /* Ready may trail a local closure; it never reopens the channel. */
        if (p->channel == SOPHIA_IPC_CLOSED)
            return SOPHIA_IPR_OK;
        /* Before Hello is sent, a second Ready, or a grant beyond discovery. */
        if (p->channel != SOPHIA_IPC_HELLO_SENT || (c->value & ~p->expect.source_kinds))
            return fault(p);
        p->expect.source_kinds = c->value;
        if (!(c->value & source_kind_bit(p))) {
            ip_fail(p, SOPHIA_IPX_UNAVAILABLE);
            return SOPHIA_IPR_CLOSED;
        }
        p->channel = SOPHIA_IPC_READY;
        return SOPHIA_IPR_OK;
    }
    ip_fail(p, c->value == SOPHIA_IC_CLOSED_PEER || c->value == SOPHIA_IC_CLOSED_REVOKED
                   ? SOPHIA_IPX_UNAVAILABLE
                   : SOPHIA_IPX_FAULT);
    return SOPHIA_IPR_OK;
}

static int offer_record(struct sophia_ip *p, const struct sophia_ic_record *r)
{
    uint64_t offer = r->header.kind == SOPHIA_IC_IMAGE_ADMITTED ? r->value.image.offer
                                                                : r->value.released.offer;
    struct sophia_ip_slot *s = ip_slot(p, offer);
    if (!s) {
        /* Above every issued ID it was never begun; at or below, the offer
         * was already forgotten after its definitive end. */
        if (offer >= p->next_offer)
            return fault(p);
        p->ignored++;
        return SOPHIA_IPR_OK;
    }
    if (r->header.kind == SOPHIA_IC_IMAGE_ADMITTED) {
        if (s->state != SOPHIA_IPO_BEGUN || !sophia_ic_metadata_equal(&s->metadata, &r->value.image))
            return fault(p);
        s->state = s->deadline_passed ? SOPHIA_IPO_EXPIRED : SOPHIA_IPO_ADMITTED;
        return SOPHIA_IPR_OK;
    }
    if (r->value.released.resource_id != s->metadata.resource_id ||
        r->value.released.resource_generation != s->metadata.resource_generation)
        return fault(p);
    switch (s->state) {
    case SOPHIA_IPO_ADMITTED:
    case SOPHIA_IPO_SENT:
    case SOPHIA_IPO_AMBIGUOUS:
    case SOPHIA_IPO_EXPIRED:
        break;
    default: /* never admitted, refused, or already released */
        return fault(p);
    }
    if (s->fd >= 0) {
        close(s->fd);
        s->fd = -1;
    }
    s->state = SOPHIA_IPO_SETTLED;
    s->disposition = r->value.released.disposition;
    return SOPHIA_IPR_OK;
}

int sophia_ip_record(struct sophia_ip *p, const struct sophia_ic_record *r, uint64_t now_ms)
{
    if (!p || !r || !ip_clock(p, now_ms))
        return SOPHIA_IPR_ARGUMENT;
    if (r->header.epoch != p->config.connection_epoch) {
        p->ignored++;
        return SOPHIA_IPR_OK;
    }
    switch (r->header.kind) {
    case SOPHIA_IC_CHANNEL_READY:
    case SOPHIA_IC_CHANNEL_CLOSED:
        return channel_record(p, r);
    case SOPHIA_IC_IMAGE_ADMITTED:
    case SOPHIA_IC_SOURCE_RELEASED:
        return offer_record(p, r);
    default: /* ImageBegin, limits and formats are not journal events */
        return SOPHIA_IPR_ARGUMENT;
    }
}

static struct sophia_ip_slot *by_resource(struct sophia_ip *p, uint64_t id, uint64_t generation)
{
    unsigned i;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++) {
        struct sophia_ip_slot *s = &p->slots[i];
        if (s->state != SOPHIA_IPO_FREE && s->state != SOPHIA_IPO_REFUSED &&
            s->state != SOPHIA_IPO_SETTLED && s->state != SOPHIA_IPO_UNKNOWN &&
            s->metadata.resource_id == id && s->metadata.resource_generation == generation)
            return s;
    }
    return NULL;
}

int sophia_ip_resource_status(struct sophia_ip *p, uint64_t resource_id, uint64_t generation,
                              enum sophia_ip_status status, int unavailable, uint64_t now_ms)
{
    struct sophia_ip_slot *s;
    if (!p || !ip_clock(p, now_ms) || status < SOPHIA_IPS_ACCEPTED ||
        status > SOPHIA_IPS_CANCELLED || (unavailable && status != SOPHIA_IPS_REJECTED))
        return SOPHIA_IPR_ARGUMENT;
    s = by_resource(p, resource_id, generation);
    if (!s)
        return SOPHIA_IPR_OK;
    /* One final status per offer. Accepted needs a source that was sent. */
    if (s->status || (status == SOPHIA_IPS_ACCEPTED && s->state != SOPHIA_IPO_SENT &&
                      s->state != SOPHIA_IPO_AMBIGUOUS))
        return fault(p);
    s->status = (uint8_t)status;
    s->unavailable = (uint8_t)(unavailable != 0);
    if (s->state == SOPHIA_IPO_BEGUN) {
        /* Definitive refusal before admission: no source ever left. */
        if (s->fd >= 0) {
            close(s->fd);
            s->fd = -1;
        }
        s->state = SOPHIA_IPO_REFUSED;
    }
    if (unavailable)
        ip_fail(p, SOPHIA_IPX_UNAVAILABLE);
    return SOPHIA_IPR_OK;
}

int sophia_ip_begin_refused(struct sophia_ip *p, uint64_t offer, uint64_t now_ms)
{
    struct sophia_ip_slot *s;
    if (!p || !ip_clock(p, now_ms))
        return SOPHIA_IPR_ARGUMENT;
    s = ip_slot(p, offer);
    if (!s || s->state != SOPHIA_IPO_BEGUN)
        return SOPHIA_IPR_REFUSED;
    if (s->fd >= 0) {
        close(s->fd);
        s->fd = -1;
    }
    s->state = SOPHIA_IPO_REFUSED;
    return SOPHIA_IPR_OK;
}

void sophia_ip_journal_lost(struct sophia_ip *p)
{
    unsigned i;
    if (!p)
        return;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++) {
        struct sophia_ip_slot *s = &p->slots[i];
        if (s->state != SOPHIA_IPO_FREE && s->state != SOPHIA_IPO_REFUSED &&
            s->state != SOPHIA_IPO_SETTLED)
            s->state = SOPHIA_IPO_UNKNOWN;
    }
    ip_fail(p, SOPHIA_IPX_JOURNAL_LOST);
}

void sophia_ip_close(struct sophia_ip *p)
{
    if (p)
        ip_fail(p, SOPHIA_IPX_CALLER);
}

enum sophia_ip_channel sophia_ip_channel(const struct sophia_ip *p)
{
    return p ? (enum sophia_ip_channel)p->channel : SOPHIA_IPC_CLOSED;
}

enum sophia_ip_cause sophia_ip_cause(const struct sophia_ip *p)
{
    return p ? (enum sophia_ip_cause)p->cause : SOPHIA_IPX_NONE;
}

enum sophia_ip_path sophia_ip_path(const struct sophia_ip *p)
{
    if (!p)
        return SOPHIA_IPP_ERROR;
    if (p->channel == SOPHIA_IPC_READY)
        return SOPHIA_IPP_DESCRIPTOR;
    if (p->channel != SOPHIA_IPC_CLOSED)
        return SOPHIA_IPP_WAIT;
    return SOPHIA_IPP_ERROR;
}
