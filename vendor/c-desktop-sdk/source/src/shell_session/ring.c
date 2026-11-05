/* Shell r10 owned ring: one registration per connection, slot offers
 * through owned-ring-v1, D4E1 events and store statuses from the journal. */
#include "internal.h"
#include "../shell_files/session_internal.h"
#include "../sophia_shell_image.h"
#include "../sophia_shell_frame_stream.h"
#include "../sophia_shell_frames.h"
#include "../image_producer/internal.h"
#include <errno.h>

enum { CONTROL_NONE, CONTROL_REGISTER, CONTROL_OFFER };

static int live(const struct sophia_ss_ring_slot *s)
{
    return s->state == SOPHIA_SS_SLOT_OFFERING || s->state == SOPHIA_SS_SLOT_OFFERED;
}

static int final(const struct sophia_ss_image_ring *r)
{
    return r->state == SOPHIA_SS_RING_REFUSED || r->state == SOPHIA_SS_RING_FAILED;
}

/* A contradiction or lost custody: the ring ends, live slots are never
 * released to the caller and are never written again. */
static void fail(struct sophia_ss_image_ring *r)
{
    unsigned i;
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; i++)
        if (live(&r->slots[i]))
            r->slots[i].state = SOPHIA_SS_SLOT_UNKNOWN;
    if (r->state != SOPHIA_SS_RING_REFUSED)
        r->state = SOPHIA_SS_RING_FAILED;
    r->control = CONTROL_NONE;
}

int ss_ring_mode(const struct sophia_ss_image *img)
{
    return img && img->session && sf_session_image_r10(&img->session->files);
}

/* The DMA-BUF opt-in selects the DMA producer lifecycle on this
 * connection: no ring is registered, driven or routed then, so ring
 * and DMA offer ids are never live in two producers at once. Both
 * protocol bits are still negotiated either way. */
int ss_ring_active(const struct sophia_ss_image *img)
{
    return ss_ring_mode(img) && !img->config.dma_buf;
}

static void back_off(struct sophia_ss_image_ring *r, uint64_t now)
{
    r->retry_delay = !r->retry_delay ? 4u : r->retry_delay < 256u ? 2u * r->retry_delay : 256u;
    r->retry_at = now > UINT64_MAX - r->retry_delay ? UINT64_MAX : now + r->retry_delay;
}

static int same_grant(const struct sophia_ss_image *img, uint64_t connection, uint64_t content)
{
    const struct sophia_or_identity *id = &img->ring.registration.identity;
    return connection == id->connection_epoch && content == id->content_epoch;
}

static void start(struct sophia_ss_image *img)
{
    struct sophia_ss_image_ring *r = &img->ring;
    const struct sophia_sf_limits *limits = sophia_ss_limits(img->session);
    uint64_t capacity = img->config.ring_capacity;
    uint32_t slots = img->config.ring_slots ? img->config.ring_slots : 1u;
    if (!capacity) {
        capacity = img->producer.expect.max_resource_bytes;
        if (capacity > SOPHIA_OR_MAX_CAPACITY)
            capacity = SOPHIA_OR_MAX_CAPACITY;
        capacity &= ~(uint64_t)3;
    }
    r->state = SOPHIA_SS_RING_REGISTERING;
    if (!limits || capacity > img->producer.expect.max_resource_bytes ||
        sophia_ip_ring_create(&r->memory, slots, capacity) != SOPHIA_IPR_OK) {
        fail(r);
        return;
    }
    r->registration.identity.connection_epoch = img->session->files.epoch;
    r->registration.identity.content_epoch = limits->grant_content_epoch;
    r->registration.identity.channel_generation = img->producer.expect.channel_generation;
    r->registration.identity.ring = 1;
    r->registration.identity.ring_generation = 1;
    r->registration.slots = slots;
    r->registration.capacity = capacity;
    if (sophia_or_register_encode(r->control_bytes, &r->registration) != SOPHIA_IC_OK) {
        fail(r);
        return;
    }
    r->control_used = SOPHIA_OR_REGISTER_BYTES;
    r->control = CONTROL_REGISTER;
    r->next_offer = 1;
}

/* The outcome of the active control write. An event may have proven the
 * write's acceptance first; a later refusal of it is a contradiction. */
static void control_result(struct sophia_ss_image *img, uint64_t now)
{
    struct sophia_ss_image_ring *r = &img->ring;
    struct sophia_ss_ring_slot *slot = NULL;
    uint32_t error = 0;
    int status;
    if (!r->control_written)
        return;
    status = sf_session_control_result(&img->session->files, &error);
    if (status == SOPHIA_9P_BUSY)
        return;
    r->control_written = 0;
    if (r->control >= CONTROL_OFFER)
        slot = &r->slots[r->control - CONTROL_OFFER];
    if (status == SOPHIA_9P_AGAIN) {
        back_off(r, now);
        return;
    }
    r->retry_delay = 0;
    if (!status) {
        if (slot && slot->state == SOPHIA_SS_SLOT_OFFERING)
            slot->state = SOPHIA_SS_SLOT_OFFERED;
    } else if (r->control == CONTROL_REGISTER) {
        if (r->state != SOPHIA_SS_RING_REGISTERING) {
            fail(r);
            return;
        }
        r->control_error = error;
        r->state = SOPHIA_SS_RING_REFUSED;
    } else if (slot && slot->state == SOPHIA_SS_SLOT_OFFERING && !slot->status) {
        /* Definitive: nothing was journaled; id and generation stay spent. */
        slot->control_error = error;
        slot->state = SOPHIA_SS_SLOT_REFUSED;
    } else {
        fail(r);
        return;
    }
    r->control = CONTROL_NONE;
}

static void send_source(struct sophia_ss_image *img)
{
    struct sophia_ss_image_ring *r = &img->ring;
    uint8_t b[SOPHIA_OR_SOURCE_BYTES];
    int sent;
    if (r->state != SOPHIA_SS_RING_ADMITTED || r->source_sent || img->producer.fd < 0)
        return;
    if (sophia_or_source_encode(b, &r->registration) != SOPHIA_IC_OK) {
        fail(r);
        return;
    }
    sent = ip_send_packet(img->producer.fd, b, sizeof b, r->memory.fd);
    if (!sent)
        return;
    /* Sent or not, the descriptor is never sent again. */
    r->source_sent = 1;
    if (sent < 0) {
        ip_fail(&img->producer, errno == EPIPE || errno == ECONNRESET ? SOPHIA_IPX_UNAVAILABLE
                                                                      : SOPHIA_IPX_FAULT);
        fail(r);
    }
}

static void refused_source(struct sophia_ss_image_ring *r, struct sophia_ss_ring_slot *s)
{
    if (!live(s) || s->status || s->frame_admitted) fail(r);
    else s->state = SOPHIA_SS_SLOT_REFUSED;
}
void ss_ring_frame_refused(struct sophia_ss_image *img, const struct sophia_sf_record *record)
{
    const struct sophia_sft_refused *v = &record->value.frame.refused;
    unsigned i;
    if (!ss_ring_active(img) || v->request_kind != SOPHIA_SFT_COMMIT) return;
    for (i = 0; i < img->ring.registration.slots; i++) {
        struct sophia_ss_ring_slot *s = &img->ring.slots[i];
        const struct sophia_or_offer *o = &s->offer;
        if (!s->frame_ticket || s->frame_transaction != v->transaction) continue;
        if (o->identity.connection_epoch != v->connection_epoch || o->identity.content_epoch != v->content_epoch ||
            o->output_id != v->output_id || o->output_generation != v->output_generation ||
            s->frame_stream != v->stream_id || s->frame_generation != v->candidate_generation) fail(&img->ring);
        else refused_source(&img->ring, s);
        return;
    }
}
void ss_ring_frame_outcome(struct sophia_ss_image *img, const struct sophia_sf_record *record)
{
    const struct sophia_sft_outcome *v = &record->value.frame.outcome;
    const struct sophia_sf_candidate_outcome *c = &v->candidate;
    unsigned i;
    if (!ss_ring_active(img)) return;
    for (i = 0; i < img->ring.registration.slots; i++) {
        struct sophia_ss_ring_slot *s = &img->ring.slots[i];
        const struct sophia_or_offer *o = &s->offer;
        if (!s->frame_ticket || s->frame_transaction != c->transaction) continue;
        if (o->identity.connection_epoch != c->grant_connection_epoch || o->identity.content_epoch != c->grant_content_epoch ||
            o->output_id != c->output_id || o->output_generation != c->output_generation ||
            s->frame_generation != c->candidate_generation || s->frame_stream != v->stream_id ||
            o->allocation_id != v->allocation_id || o->allocation_generation != v->allocation_generation ||
            o->topology_generation != v->facts_generation || s->state == SOPHIA_SS_SLOT_REFUSED) fail(&img->ring);
        else s->frame_admitted = 1;
        return;
    }
}

static void frame_custody(struct sophia_ss_image *img)
{
    unsigned i;
    for (i = 0; i < img->ring.registration.slots; i++) {
        struct sophia_ss_ring_slot *s = &img->ring.slots[i];
        enum sophia_ss_outcome outcome;
        uint32_t error;
        if (!s->frame_ticket || !live(s)) continue;
        if (sophia_ss_outcome(img->session, s->frame_ticket, &outcome, &error)) { fail(&img->ring); return; }
        if (outcome == SOPHIA_SS_REFUSED || outcome == SOPHIA_SS_DROPPED_UNSENT) refused_source(&img->ring, s);
        else if (outcome == SOPHIA_SS_UNKNOWN_DISCONNECTED || outcome == SOPHIA_SS_UNAVAILABLE) fail(&img->ring);
    }
}

void ss_ring_drive(struct sophia_ss_image *img, uint64_t now)
{
    struct sophia_ss_image_ring *r = &img->ring;
    unsigned i;
    if (final(r))
        return;
    frame_custody(img);
    control_result(img, now);
    if (final(r))
        return;
    if (sophia_ip_channel(&img->producer) == SOPHIA_IPC_CLOSED) {
        fail(r);
        return;
    }
    if (r->state == SOPHIA_SS_RING_IDLE) {
        if (sophia_ip_channel(&img->producer) != SOPHIA_IPC_READY)
            return;
        start(img);
        if (final(r))
            return;
    }
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; i++)
        if (live(&r->slots[i]) && now >= r->slots[i].deadline_ms)
            r->slots[i].deadline_passed = 1;
    send_source(img);
    if (r->control && !r->control_written && now >= r->retry_at && !ss_final(img->session)) {
        int status = sf_session_control_write(&img->session->files, r->control_bytes,
                                              r->control_used);
        if (!status)
            r->control_written = 1;
        else if (status != SOPHIA_9P_BUSY)
            fail(r);
    }
}

void ss_ring_lost(struct sophia_ss_image *img)
{
    fail(&img->ring);
}

static int exact(const struct sophia_ss_ring_slot *s, const struct sophia_or_event *e)
{
    return live(s) && s->offer.offer == e->offer && s->offer.slot_generation == e->slot_generation &&
           s->offer.resource_id == e->resource_id &&
           s->offer.resource_generation == e->resource_generation;
}

enum { CANCELLED_REASON = 11 };

static int evidence(const struct sophia_ss_ring_slot *s, uint16_t reason)
{
    if (!reason)
        return s->accepted;
    return reason == CANCELLED_REASON ? s->cancelled_seen : s->rejected;
}

void ss_ring_event(struct sophia_ss_image *img, const uint8_t *b, size_t n)
{
    struct sophia_ss_image_ring *r = &img->ring;
    struct sophia_ss_ring_slot *s;
    struct sophia_or_event e;
    if (final(r))
        return;
    if (sophia_or_event_decode(b, n, &e) != SOPHIA_IC_OK || r->state == SOPHIA_SS_RING_IDLE ||
        !sophia_or_identity_equal(&e.identity, &r->registration.identity)) {
        fail(r);
        return;
    }
    switch (e.kind) {
    case SOPHIA_OR_RING_ADMITTED:
        if (r->state != SOPHIA_SS_RING_REGISTERING)
            break;
        r->state = SOPHIA_SS_RING_ADMITTED;
        send_source(img);
        return;
    case SOPHIA_OR_RING_READY:
        if (r->state != SOPHIA_SS_RING_ADMITTED || !r->source_sent)
            break;
        r->state = SOPHIA_SS_RING_READY;
        return;
    case SOPHIA_OR_RING_REFUSED:
        if (r->state != SOPHIA_SS_RING_REGISTERING && r->state != SOPHIA_SS_RING_ADMITTED)
            break;
        r->reason = e.reason;
        r->state = SOPHIA_SS_RING_REFUSED;
        return;
    case SOPHIA_OR_SLOT_RELEASED:
        if (r->state != SOPHIA_SS_RING_READY || e.slot >= r->registration.slots)
            break;
        s = &r->slots[e.slot];
        /* Release follows the final store status, never precedes it, so its
         * reason must match a status kind already seen: Accepted for 0,
         * Cancelled for 11, Rejected otherwise. Order, transaction and failed
         * control replies never pick the outcome. */
        if (!exact(s, &e) || !evidence(s, e.reason))
            break;
        s->status = (uint8_t)(!e.reason                        ? SOPHIA_IPS_ACCEPTED
                              : e.reason == CANCELLED_REASON ? SOPHIA_IPS_CANCELLED
                                                             : SOPHIA_IPS_REJECTED);
        s->state = SOPHIA_SS_SLOT_RELEASED;
        s->reason = e.reason;
        s->changed = e.changed_generation;
        return;
    }
    fail(r);
}

/* Terminal statuses for a live offer, matched by grant and resource since a
 * cancellation may carry its own transaction. Failed ResourceRetire/Cancel
 * replies share the record, so every one is kept as evidence and none is a
 * contradiction; the exact SlotReleased decides. */
/* Returns 1 when the status names a live ring slot. Anything else is
 * not ring evidence: in mixed mode it may belong to a DMA-BUF producer
 * offer, and the caller routes it there instead. */
int ss_ring_status(struct sophia_ss_image *img, const struct sophia_sf_record *e)
{
    struct sophia_ss_image_ring *r = &img->ring;
    const struct sophia_sf_resource_status *v = &e->value.resource_status;
    unsigned i;
    if (final(r) || v->status < 2 || v->status > 4 ||
        !same_grant(img, v->grant_connection_epoch, v->grant_content_epoch))
        return 0;
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; i++) {
        struct sophia_ss_ring_slot *s = &r->slots[i];
        if (live(s) && s->offer.resource_id == v->resource_id &&
            s->offer.resource_generation == v->resource_generation) {
            if (v->status == 2)
                s->accepted = 1;
            else if (v->status == 4)
                s->cancelled_seen = 1;
            else
                s->rejected = 1;
            s->status = (uint8_t)(v->status == 2 ? SOPHIA_IPS_ACCEPTED
                                  : v->status == 3 ? SOPHIA_IPS_REJECTED
                                                   : SOPHIA_IPS_CANCELLED);
            return 1;
        }
    }
    return 0;
}

int ss_ring_poll_out(const struct sophia_ss_image *img)
{
    return img->ring.state == SOPHIA_SS_RING_ADMITTED && !img->ring.source_sent;
}

int64_t ss_ring_timeout(const struct sophia_ss_image *img, uint64_t now)
{
    const struct sophia_ss_image_ring *r = &img->ring;
    uint64_t due = UINT64_MAX;
    unsigned i;
    if (final(r))
        return -1;
    if (r->control && !r->control_written)
        due = r->retry_at;
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; i++)
        if (live(&r->slots[i]) && !r->slots[i].deadline_passed && r->slots[i].deadline_ms < due)
            due = r->slots[i].deadline_ms;
    if (due == UINT64_MAX)
        return -1;
    return due <= now ? 0 : due - now > INT64_MAX ? INT64_MAX : (int64_t)(due - now);
}

enum sophia_ss_ring_state sophia_ss_image_ring_state(const struct sophia_ss_image *img,
                                                     uint16_t *reason)
{
    if (reason)
        *reason = img ? img->ring.reason : 0;
    return img ? (enum sophia_ss_ring_state)img->ring.state : SOPHIA_SS_RING_FAILED;
}

uint64_t sophia_ss_image_ring_capacity(const struct sophia_ss_image *img, uint32_t *slots)
{
    if (slots)
        *slots = img && img->ring.memory.base ? img->ring.memory.slots : 0;
    return img && img->ring.memory.base ? img->ring.memory.capacity : 0;
}

void *sophia_ss_image_ring_pixels(struct sophia_ss_image *img, uint32_t slot)
{
    struct sophia_ss_image_ring *r;
    if (!ss_ring_active(img))
        return NULL;
    r = &img->ring;
    if (r->state != SOPHIA_SS_RING_READY || !r->memory.base || slot >= r->memory.slots ||
        r->slots[slot].state != SOPHIA_SS_SLOT_FREE)
        return NULL;
    return r->memory.base + (size_t)slot * r->memory.extent + SOPHIA_OR_SLOT_HEADER_BYTES;
}

static int prepare_offer(struct sophia_ss_image *img, uint32_t slot,
                         const struct sophia_ss_ring_image *m, struct sophia_or_offer *out,
                         uint8_t encoded[SOPHIA_OR_OFFER_BYTES])
{
    struct sophia_ss_image_ring *r;
    struct sophia_or_offer o;
    unsigned i;
    if (!ss_ring_active(img) || !m || !out)
        return SOPHIA_IPR_ARGUMENT;
    r = &img->ring;
    if (ss_final(img->session) || final(r))
        return SOPHIA_IPR_CLOSED;
    if (img->session->state != SOPHIA_SS_READY || r->state != SOPHIA_SS_RING_READY || r->control)
        return SOPHIA_IPR_AGAIN;
    if (slot >= r->registration.slots || r->slots[slot].state != SOPHIA_SS_SLOT_FREE ||
        !m->width || !m->height || m->width > img->producer.expect.max_width ||
        m->height > img->producer.expect.max_height ||
        (uint64_t)m->width * m->height > r->registration.capacity / 4)
        return SOPHIA_IPR_REFUSED;
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; i++)
        if (live(&r->slots[i]) && r->slots[i].offer.resource_id == m->resource_id &&
            r->slots[i].offer.resource_generation == m->resource_generation)
            return SOPHIA_IPR_REFUSED;
    if (r->next_offer == UINT64_MAX || r->generation[slot] > UINT64_MAX - 4)
        return SOPHIA_IPR_EXHAUSTED;
    memset(&o, 0, sizeof o);
    o.identity = r->registration.identity;
    o.offer = r->next_offer;
    o.slot = slot;
    o.slot_generation = r->generation[slot] + 2;
    o.resource_id = m->resource_id;
    o.resource_generation = m->resource_generation;
    o.allocation_id = m->allocation_id;
    o.allocation_generation = m->allocation_generation;
    o.output_id = m->output_id;
    o.output_generation = m->output_generation;
    o.topology_generation = m->topology_generation;
    o.width = m->width;
    o.height = m->height;
    if (sophia_or_offer_encode(encoded, &o) != SOPHIA_IC_OK)
        return SOPHIA_IPR_REFUSED;
    *out = o;
    return SOPHIA_IPR_OK;
}
static void publish_offer(struct sophia_ss_image *img, const struct sophia_or_offer *o, uint64_t now_ms)
{
    struct sophia_ss_image_ring *r = &img->ring;
    struct sophia_ss_ring_slot *s;
    uint32_t slot = o->slot;
    uint8_t *word;
    unsigned i;
    /* The generation word precedes the Offer; the slot is then untouched. */
    word = r->memory.base + (size_t)slot * r->memory.extent;
    for (i = 0; i < 8; i++)
        word[i] = (uint8_t)(o->slot_generation >> (8 * i));
    r->next_offer++;
    r->generation[slot] = o->slot_generation;
    s = &r->slots[slot];
    memset(s, 0, sizeof *s);
    s->offer = *o;
    s->state = SOPHIA_SS_SLOT_OFFERING;
    s->deadline_ms = now_ms + img->producer.config.transfer_timeout_ms;
    if (s->deadline_ms < now_ms)
        s->deadline_ms = UINT64_MAX;
}
static int legacy_offer(struct sophia_ss_image *img, uint32_t slot,
                        const struct sophia_ss_ring_image *m, uint64_t now_ms, uint64_t *offer)
{
    struct sophia_or_offer o;
    int status;
    status = prepare_offer(img, slot, m, &o, img->ring.control_bytes);
    if (status) return status;
    publish_offer(img, &o, now_ms);
    img->ring.control_used = SOPHIA_OR_OFFER_BYTES;
    img->ring.control = (uint8_t)(CONTROL_OFFER + slot);
    img->ring.retry_at = 0;
    *offer = o.offer;
    (void)ss_pump(img->session, 0);
    return SOPHIA_IPR_OK;
}

int sophia_ss_image_ring_offer(struct sophia_ss_image *img, uint32_t slot,
                               const struct sophia_ss_ring_image *m, uint64_t now_ms, uint64_t *offer)
{
    if (!img || !img->session || !offer || sf_session_frame(&img->session->files)) return SOPHIA_IPR_ARGUMENT;
    return legacy_offer(img, slot, m, now_ms, offer);
}

int sophia_ss_frame_legacy_ring_offer(struct sophia_ss_image *img,
                                     const struct sophia_ss_frame_stream *f, uint32_t slot,
                                     const struct sophia_ss_ring_image *m, uint64_t now_ms,
                                     uint64_t *offer)
{
    const struct sophia_sft_key *k;
    unsigned i;
    int status;
    if (!img || !img->session || !m || !offer || !sophia_ss_frame_drained(f) ||
        !sf_session_frame(&img->session->files)) return SOPHIA_IPR_ARGUMENT;
    k = &f->key;
    if (!k->connection_epoch || !same_grant(img, k->connection_epoch, k->content_epoch) ||
        m->output_id != k->output_id || m->output_generation != k->output_generation ||
        m->allocation_id != k->allocation_id || m->allocation_generation != k->allocation_generation ||
        m->topology_generation != k->facts_generation || now_ms < img->session->now_ms ||
        now_ms < img->producer.last_now) return SOPHIA_IPR_ARGUMENT;
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; ++i) {
        const struct sophia_ss_ring_slot *s = &img->ring.slots[i];
        if (!s->frame_transaction || s->offer.output_id != k->output_id ||
            s->offer.output_generation != k->output_generation) continue;
        if (s->state == SOPHIA_SS_SLOT_UNKNOWN) return SOPHIA_IPR_CLOSED;
        if (live(s)) return SOPHIA_IPR_BUSY;
    }
    status = legacy_offer(img, slot, m, now_ms, offer);
    if (!status) img->producer.last_now = now_ms;
    return status;
}

int sophia_ss_frame_submit_ring(struct sophia_ss_image *img, struct sophia_ss_frame_stream *f,
                               uint32_t slot, const struct sophia_ss_ring_image *m,
                               struct sophia_sf_record *record, uint64_t now_ms,
                               uint64_t *ticket, uint64_t *offer)
{
    struct sophia_ss *session;
    struct sophia_or_offer o, saved_image;
    struct sophia_sf_header saved_header;
    struct sophia_ss_ring_slot *source;
    uint8_t bytes[8192], encoded_offer[SOPHIA_OR_OFFER_BYTES];
    size_t n;
    int status;
    if (!img || !(session = img->session) || !record || !ticket || !offer ||
        !sf_session_frame(&session->files) || img->config.ring_slots != 2 ||
        now_ms < session->now_ms || now_ms < img->producer.last_now) return SOPHIA_9P_ARGUMENT;
    if (ss_final(session)) return ss_status(session);
    status = ss_frame_check(f, record);
    if (status) return status;
    if (record->value.frame.commit.has_image) return SOPHIA_9P_ARGUMENT;
    status = prepare_offer(img, slot, m, &o, encoded_offer);
    if (status) return status == SOPHIA_IPR_AGAIN || status == SOPHIA_IPR_BUSY ? SOPHIA_9P_BUSY : SOPHIA_9P_ARGUMENT;
    saved_header = record->header;
    saved_image = record->value.frame.commit.image;
    record->header.epoch = session->files.epoch;
    record->header.submission = 1;
    record->header.sequence = 0;
    record->value.frame.commit.image = o;
    record->value.frame.commit.has_image = 1;
    status = sophia_sf_encode_revision(bytes, sizeof bytes, SOPHIA_SFT_REVISION, record, &n);
    record->header = saved_header;
    record->value.frame.commit.image = saved_image;
    record->value.frame.commit.has_image = 0;
    if (status || !session->files.have_limits || n > session->files.limits.max_candidate_bytes)
        return SOPHIA_9P_ARGUMENT;
    status = ss_admit_encoded(session, bytes, n, ticket);
    if (status) return status;
    /* No I/O or fallible step between queue admission and publication. */
    publish_offer(img, &o, now_ms);
    source = &img->ring.slots[slot];
    source->frame_ticket = *ticket;
    source->frame_transaction = record->value.frame.commit.candidate.transaction;
    source->frame_generation = record->value.frame.commit.candidate.candidate_generation;
    source->frame_stream = f->key.stream_id;
    ss_frame_submitted(f, record);
    img->producer.last_now = now_ms;
    *offer = o.offer;
    (void)ss_pump(session, 0);
    return 0;
}

static struct sophia_ss_ring_slot *by_offer(const struct sophia_ss_image *img, uint64_t offer)
{
    unsigned i;
    if (!offer)
        return NULL;
    for (i = 0; i < SOPHIA_OR_MAX_SLOTS; i++)
        if (img->ring.slots[i].state != SOPHIA_SS_SLOT_FREE &&
            img->ring.slots[i].offer.offer == offer)
            return (struct sophia_ss_ring_slot *)&img->ring.slots[i];
    return NULL;
}

static void info_of(const struct sophia_ss_ring_slot *s, struct sophia_ss_ring_offer_info *out)
{
    memset(out, 0, sizeof *out);
    out->offer = s->offer;
    out->state = (enum sophia_ss_ring_slot_state)s->state;
    /* Accepted precedence is pending evidence only; once released the status
     * is the outcome the release selected. */
    out->status = (enum sophia_ip_status)(live(s) && s->accepted ? SOPHIA_IPS_ACCEPTED
                                                                 : s->status);
    out->control_error = s->control_error;
    out->reason = s->reason;
    out->changed_generation = s->changed;
    out->cancel_requested = s->cancelled;
    out->deadline_passed = s->deadline_passed;
    out->adoptable = s->state == SOPHIA_SS_SLOT_RELEASED && !s->reason && !s->changed;
}

int sophia_ss_image_ring_info(const struct sophia_ss_image *img, uint64_t offer,
                              struct sophia_ss_ring_offer_info *out)
{
    const struct sophia_ss_ring_slot *s;
    if (!img || !out || !ss_ring_active(img))
        return SOPHIA_IPR_ARGUMENT;
    s = by_offer(img, offer);
    if (!s)
        return SOPHIA_IPR_ARGUMENT;
    info_of(s, out);
    return SOPHIA_IPR_OK;
}

int sophia_ss_image_ring_next(const struct sophia_ss_image *img, unsigned *cursor,
                              struct sophia_ss_ring_offer_info *out)
{
    if (!img || !cursor || !out)
        return 0;
    while (*cursor < SOPHIA_OR_MAX_SLOTS) {
        const struct sophia_ss_ring_slot *s = &img->ring.slots[(*cursor)++];
        if (s->state != SOPHIA_SS_SLOT_FREE) {
            info_of(s, out);
            return 1;
        }
    }
    return 0;
}

int sophia_ss_image_ring_cancel(struct sophia_ss_image *img, uint64_t offer, uint64_t now_ms,
                                uint64_t *ticket)
{
    struct sophia_ss *s;
    struct sophia_ss_ring_slot *slot;
    struct sophia_sf_record record;
    uint8_t bytes[SOPHIA_SS_IMAGE_BEGIN_BYTES];
    size_t n;
    (void)now_ms;
    if (!ss_ring_active(img) || !ticket)
        return SOPHIA_IPR_ARGUMENT;
    s = img->session;
    if (ss_final(s))
        return SOPHIA_IPR_CLOSED;
    if (s->state != SOPHIA_SS_READY)
        return SOPHIA_IPR_AGAIN;
    slot = by_offer(img, offer);
    if (!slot)
        return SOPHIA_IPR_ARGUMENT;
    if (!live(slot) || slot->status || slot->cancelled || final(&img->ring))
        return SOPHIA_IPR_REFUSED;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_SF_RESOURCE_CANCEL;
    record.header.epoch = s->files.epoch;
    record.header.submission = 1;
    record.value.resource_cancel.transaction = offer;
    record.value.resource_cancel.grant_connection_epoch = slot->offer.identity.connection_epoch;
    record.value.resource_cancel.grant_content_epoch = slot->offer.identity.content_epoch;
    record.value.resource_cancel.resource_id = slot->offer.resource_id;
    record.value.resource_cancel.resource_generation = slot->offer.resource_generation;
    if (sophia_sf_encode(bytes, sizeof(bytes), &record, &n))
        return SOPHIA_IPR_REFUSED;
    if (!ss_room(s, n))
        return SOPHIA_IPR_BUSY;
    if (ss_admit_encoded(s, bytes, n, ticket))
        return SOPHIA_IPR_REFUSED; /* unreachable after the room check */
    /* The ticket outcome is the caller's; it never changes slot custody. */
    slot->cancelled = 1;
    (void)ss_pump(s, 0);
    return SOPHIA_IPR_OK;
}

int sophia_ss_image_ring_forget(struct sophia_ss_image *img, uint64_t offer)
{
    struct sophia_ss_ring_slot *s;
    if (!ss_ring_active(img))
        return SOPHIA_IPR_ARGUMENT;
    s = by_offer(img, offer);
    if (!s)
        return SOPHIA_IPR_ARGUMENT;
    if (live(s))
        return SOPHIA_IPR_REFUSED;
    memset(s, 0, sizeof *s);
    return SOPHIA_IPR_OK;
}

void sophia_ss_image_close(struct sophia_ss_image *img)
{
    if (!img)
        return;
    if (img->started)
        sophia_ip_close(&img->producer);
    /* Sophia's queued readers keep their own reference; ours just ends. */
    if (!final(&img->ring))
        fail(&img->ring);
    sophia_ip_ring_close(&img->ring.memory);
}
