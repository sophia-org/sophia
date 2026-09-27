#include "internal.h"
#include <limits.h>
#include <poll.h>

static uint64_t ss_get(const uint8_t *p, size_t n)
{
    size_t i;
    uint64_t v = 0;
    for (i = 0; i < n; i++)
        v |= (uint64_t)p[i] << (i * 8);
    return v;
}
size_t sophia_ss_storage_bytes(uint32_t msize, size_t queue_bytes)
{
    size_t wire = sophia_9p_storage_bytes(msize, SOPHIA_SS_REQUESTS);
    if (!wire || queue_bytes < SOPHIA_SS_RECORD_BYTES ||
        queue_bytes > (size_t)SOPHIA_SS_SLOTS * SOPHIA_SS_RECORD_BYTES ||
        queue_bytes > SIZE_MAX - wire)
        return 0;
    return wire + queue_bytes;
}
int sophia_ss_open_fd(struct sophia_ss *s, int fd, const struct sophia_ss_config *config,
                      void *storage, size_t bytes)
{
    size_t needed, wire;
    uintptr_t a = (uintptr_t)s, b = (uintptr_t)storage;
    int r;
    if (!s || !config || !storage || !config->queue_slots || config->queue_slots > SOPHIA_SS_SLOTS)
        return SOPHIA_9P_ARGUMENT;
    needed = sophia_ss_storage_bytes(config->msize, config->queue_bytes);
    if (!needed || bytes < needed || a > UINTPTR_MAX - sizeof(*s) || b > UINTPTR_MAX - needed ||
        !(a + sizeof(*s) <= b || b + needed <= a))
        return SOPHIA_9P_ARGUMENT;
    wire = sophia_9p_storage_bytes(config->msize, SOPHIA_SS_REQUESTS);
    memset(s, 0, sizeof(*s));
    r = sophia_9p_init(&s->wire, fd, config->msize, SOPHIA_SS_REQUESTS, SOPHIA_SS_FIDS, storage,
                       wire);
    if (!r)
        r = sophia_sf_client_init_profile(&s->files, &s->wire, config->offer, config->profile,
                                          config->object_storage, config->object_capacity);
    if (r) {
        s->state = SOPHIA_SS_FAILED;
        return r;
    }
    s->queue = (uint8_t *)storage + wire;
    s->queue_capacity = config->queue_bytes;
    s->slots = config->queue_slots;
    s->next_ticket = 1;
    s->state = SOPHIA_SS_NEGOTIATING;
    return 0;
}
int ss_status(const struct sophia_ss *s)
{
    if (!ss_final(s))
        return 0;
    return s->state == SOPHIA_SS_CLOSED ? SOPHIA_9P_CLOSED : SOPHIA_9P_INVALID;
}
/* After an end, bytes already received may hold the in-flight Submitted
 * behind undelivered events. Walk them in order with the parser's own checks
 * and stop at the first incomplete record or one that fails them, or whose
 * validity depends on other client state: unvalidated bytes prove nothing.
 * Nothing is consumed or reordered. */
static int received_custody(const struct sophia_ss *s)
{
    const struct sophia_sf_client *c = &s->files;
    struct sophia_sf_record r;
    uint64_t sequence = c->sequence;
    size_t at = c->event_ready ? (size_t)ss_get(c->event_bytes, 4) : 0, n;
    while (at <= c->event_used && c->event_used - at >= 4) {
        n = (size_t)ss_get(c->event_bytes + at, 4);
        if (n < SOPHIA_SF_HEADER_BYTES || n > c->event_used - at ||
            sophia_sf_decode(c->event_bytes + at, n, &r) || r.header.epoch != c->epoch ||
            r.header.kind < 16 || r.header.kind >= 256 || r.header.sequence <= sequence ||
            r.header.kind == SOPHIA_SF_NEGOTIATED || r.header.kind == SOPHIA_SF_RESOURCE_STATUS)
            return 0;
        if (r.header.kind == SOPHIA_SF_SUBMITTED)
            return r.value.submitted.submission_id == s->flight_id &&
                   r.value.submitted.candidate_kind == ss_get(c->tx + 6, 2);
        sequence = r.header.sequence;
        at += n;
    }
    return 0;
}
/* Classify the in-flight record from observed progress, then drop the queue. */
static void finish(struct sophia_ss *s, enum sophia_ss_state state)
{
    enum sophia_sf_submission stage = SOPHIA_SF_SUBMISSION_ISSUED;
    enum sophia_ss_outcome outcome;
    uint64_t id = 0;
    uint16_t i;
    if (s->flight) {
        if (sophia_sf_client_submission(&s->files, &id, &stage) || id != s->flight_id)
            stage = SOPHIA_SF_SUBMISSION_ISSUED;
        if (stage == SOPHIA_SF_SUBMISSION_ISSUED && received_custody(s))
            stage = SOPHIA_SF_SUBMISSION_CUSTODIED;
        if (stage == SOPHIA_SF_SUBMISSION_CUSTODIED)
            outcome = SOPHIA_SS_SUBMITTED;
        else if (stage == SOPHIA_SF_SUBMISSION_REFUSED)
            outcome = SOPHIA_SS_REFUSED;
        else if (stage == SOPHIA_SF_SUBMISSION_STAGED)
            outcome = SOPHIA_SS_DROPPED_UNSENT;
        else
            outcome = SOPHIA_SS_UNKNOWN_DISCONNECTED;
        ss_set(s, s->flight_ticket, outcome,
               outcome == SOPHIA_SS_REFUSED ? s->files.submit_error : 0);
        s->flight = 0;
    }
    for (i = 0; i < s->queued; i++)
        ss_set(s, s->entries[i].ticket, SOPHIA_SS_DROPPED_UNSENT, 0);
    s->queued = 0;
    s->queue_used = 0;
    s->waiting = 0;
    s->state = state;
}
static void track(struct sophia_ss *s)
{
    enum sophia_sf_submission stage;
    uint64_t id;
    if (!s->flight || sophia_sf_client_submission(&s->files, &id, &stage) || id != s->flight_id ||
        (stage != SOPHIA_SF_SUBMISSION_CUSTODIED && stage != SOPHIA_SF_SUBMISSION_REFUSED))
        return;
    if (stage == SOPHIA_SF_SUBMISSION_CUSTODIED)
        ss_set(s, s->flight_ticket, SOPHIA_SS_SUBMITTED, 0);
    else
        ss_set(s, s->flight_ticket, SOPHIA_SS_REFUSED, s->files.submit_error);
    s->flight = s->waiting = 0;
    s->retry_delay = 0;
}
/* Session-owned events and results, EAGAIN deferral and ack progress. */
static int drain(struct sophia_ss *s)
{
    const struct sophia_sf_record *e = NULL;
    int r = 0;
    if (s->files.submit_wait && !s->waiting) {
        s->waiting = 1;
        s->progress = 0;
        s->retry_delay = !s->retry_delay       ? 4u
                         : s->retry_delay < 256u ? 2u * s->retry_delay
                                                 : 256u;
        s->retry_at = s->now_ms > UINT64_MAX - s->retry_delay ? UINT64_MAX
                                                               : s->now_ms + s->retry_delay;
    }
    if (s->files.acked_sequence > s->acked_seen) {
        s->acked_seen = s->files.acked_sequence;
        s->ack_clock = s->now_ms;
        s->progress = 1;
    }
    /* Negotiation's own Limits fetch; without limits the session cannot
     * become ready, so a failed fetch fails closed. */
    if (s->files.object_ready && !s->object_requested &&
        sophia_sf_client_object_result(&s->files, &e) && !s->files.have_limits)
        r = SOPHIA_9P_INVALID;
    while (!r && !sophia_sf_client_event(&s->files, &e) &&
           (e->header.kind == SOPHIA_SF_NEGOTIATED || e->header.kind == SOPHIA_SF_REFUSED ||
            e->header.kind == SOPHIA_SF_SUBMITTED)) {
        if (e->header.kind == SOPHIA_SF_REFUSED) {
            s->refused = 1;
            s->refused_reason = e->value.refused.reason;
            s->refused_denied = e->value.refused.denied_capabilities;
        }
        ss_consumed(s, e->header.sequence);
        r = sophia_sf_client_event_consume(&s->files);
    }
    track(s);
    return r;
}
static int settle(struct sophia_ss *s, int r)
{
    if (s->files.refused)
        finish(s, SOPHIA_SS_REFUSED_NEGOTIATION);
    else if (s->files.stale)
        finish(s, SOPHIA_SS_STALE);
    else if (r == SOPHIA_9P_CLOSED || r == SOPHIA_9P_IO)
        finish(s, SOPHIA_SS_CLOSED);
    else
        finish(s, SOPHIA_SS_FAILED);
    return ss_status(s);
}
int ss_pump(struct sophia_ss *s, size_t budget)
{
    unsigned pass;
    uint64_t consumed;
    int r, d;
    for (pass = 0; pass < 3; pass++) {
        consumed = s->last_consumed;
        if (ss_final(s))
            return ss_status(s);
        r = ss_handoff(s);
        if (!r)
            r = sophia_sf_client_service(&s->files, pass ? 0 : budget);
        /* Parsed custody is taken before any end is classified. */
        d = drain(s);
        if (!r)
            r = d;
        if (r || s->files.refused)
            return settle(s, r);
        if (s->state == SOPHIA_SS_NEGOTIATING && sophia_sf_client_ready(&s->files))
            s->state = SOPHIA_SS_READY;
        /* A slot freed or an event consumed in this pass needs its next
         * request queued now, so poll reports POLLOUT for it. */
        if (s->last_consumed == consumed &&
            (s->state != SOPHIA_SS_READY || s->flight || !s->queued))
            break;
    }
    return 0;
}
int sophia_ss_dispatch(struct sophia_ss *s, short revents, size_t budget, uint64_t now_ms)
{
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    /* A clock that runs backwards is refused, never taken as elapsed time. */
    if (now_ms < s->now_ms)
        return SOPHIA_9P_ARGUMENT;
    s->now_ms = now_ms;
    if (revents & POLLNVAL)
        return settle(s, SOPHIA_9P_IO);
    /* The pass that saw EAGAIN has ended; resume after progress or backoff. */
    if (s->waiting && (s->progress || now_ms >= s->retry_at)) {
        s->waiting = 0;
        (void)sophia_sf_client_submit_retry(&s->files);
    }
    return ss_pump(s, budget);
}
int sophia_ss_poll_fd(const struct sophia_ss *s) { return s ? s->wire.fd : -1; }
short sophia_ss_poll_events(const struct sophia_ss *s)
{
    if (!s || ss_final(s))
        return 0;
    return (short)(POLLIN | (sophia_9p_wants_write(&s->wire) ? POLLOUT : 0));
}
int sophia_ss_timeout(const struct sophia_ss *s, uint64_t now_ms)
{
    if (!s || ss_final(s) || !s->waiting)
        return -1;
    if (s->progress || now_ms >= s->retry_at)
        return 0;
    return s->retry_at - now_ms > INT_MAX ? INT_MAX : (int)(s->retry_at - now_ms);
}
enum sophia_ss_state sophia_ss_state(const struct sophia_ss *s)
{
    return s ? s->state : SOPHIA_SS_FAILED;
}
uint64_t sophia_ss_epoch(const struct sophia_ss *s)
{
    return s && s->files.negotiated ? s->files.epoch : 0;
}
const struct sophia_sf_limits *sophia_ss_limits(const struct sophia_ss *s)
{
    return s && s->files.have_limits ? &s->files.limits : NULL;
}
int sophia_ss_refusal(const struct sophia_ss *s, uint16_t *reason, uint64_t *denied)
{
    if (!s || !reason || !denied)
        return SOPHIA_9P_ARGUMENT;
    if (!s->refused)
        return SOPHIA_9P_AGAIN;
    *reason = s->refused_reason;
    *denied = s->refused_denied;
    return 0;
}
int sophia_ss_obligations(const struct sophia_ss *s, struct sophia_ss_obligations *o)
{
    unsigned i;
    if (!s || !o)
        return SOPHIA_9P_ARGUMENT;
    memset(o, 0, sizeof(*o));
    o->consumed = s->last_consumed;
    o->acked = s->files.acked_sequence;
    o->ack_limit = sophia_ss_ack_limit(s);
    if (o->consumed > o->acked)
        o->ack_due_ms = s->ack_clock > UINT64_MAX - SOPHIA_SS_ACK_PROGRESS_MS
                            ? UINT64_MAX
                            : s->ack_clock + SOPHIA_SS_ACK_PROGRESS_MS;
    for (i = 0; i < 4; i++)
        if (s->holds[i].active)
            o->objects |= (uint8_t)(1u << i);
    o->blocked = o->consumed > o->ack_limit;
    return 0;
}
void sophia_ss_close(struct sophia_ss *s)
{
    if (s && !ss_final(s))
        finish(s, SOPHIA_SS_CLOSED);
}
