/* Native launcher session over a scripted stand-in for the sophia_ss API.
 * The shared peer serves only the bar profile, so these tests define every
 * sophia_ss_* symbol the ns uses (no shell_session member is linked) and run
 * each record through the real shell file codec in both directions. */
#include "../sophia_shell_native_session.h"
#include "shell_files_vectors.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

#define EPOCH 17u
#define LOG 64u
struct fake {
    enum sophia_ss_state state;
    int final_status, have_limits, submit_busy, reserve_busy, reserved;
    struct sophia_sf_limits limits;
    struct sophia_sf_record events[32];
    uint8_t bytes[32][1024];
    unsigned pushed, head;
    uint64_t sequence, next_ticket, serial;
    uint16_t reserved_slots;
    size_t reserved_bytes;
    enum sophia_ss_outcome outcomes[256];
    struct sophia_sf_record log[LOG];
    uint64_t log_ticket[LOG];
    unsigned logged, uploads;
};
static struct fake F;
static struct sophia_ss SS;

static size_t fake_encode(const struct sophia_sf_record *in)
{
    static uint8_t b[8192];
    struct sophia_sf_record r = *in;
    size_t n;
    r.header.epoch = EPOCH;
    r.header.submission = 1;
    r.header.sequence = 0;
    return sophia_sf_encode(b, sizeof(b), &r, &n) ? 0 : n;
}
static uint64_t fake_log(const struct sophia_sf_record *r)
{
    assert(F.logged < LOG && F.next_ticket < 256);
    F.log[F.logged] = *r;
    F.log_ticket[F.logged++] = F.next_ticket;
    F.outcomes[F.next_ticket] = SOPHIA_SS_ADMITTED_LOCAL;
    return F.next_ticket++;
}
enum sophia_ss_state sophia_ss_state(const struct sophia_ss *s)
{
    (void)s;
    return F.state;
}
const struct sophia_sf_limits *sophia_ss_limits(const struct sophia_ss *s)
{
    (void)s;
    return F.have_limits ? &F.limits : NULL;
}
size_t sophia_ss_record_bytes(const struct sophia_sf_record *r) { return r ? fake_encode(r) : 0; }
int sophia_ss_submit(struct sophia_ss *s, const struct sophia_sf_record *records, size_t count,
                     uint64_t *first)
{
    size_t i;
    (void)s;
    if (F.state > SOPHIA_SS_READY)
        return F.final_status;
    if (F.submit_busy || F.reserved)
        return SOPHIA_9P_BUSY;
    for (i = 0; i < count; i++)
        if (!fake_encode(&records[i]))
            return SOPHIA_9P_INVALID;
    *first = F.next_ticket;
    for (i = 0; i < count; i++)
        (void)fake_log(&records[i]);
    return 0;
}
int sophia_ss_reserve(struct sophia_ss *s, uint16_t slots, size_t bytes,
                      struct sophia_ss_reservation *out)
{
    (void)s;
    if (F.state > SOPHIA_SS_READY)
        return F.final_status;
    if (F.reserve_busy || F.reserved)
        return SOPHIA_9P_BUSY;
    F.reserved = 1;
    F.reserved_slots = slots;
    F.reserved_bytes = bytes;
    out->serial = ++F.serial;
    out->slots = slots;
    out->bytes = bytes;
    return 0;
}
int sophia_ss_commit(struct sophia_ss *s, struct sophia_ss_reservation *v,
                     const struct sophia_sf_record *records, size_t count, uint64_t *first)
{
    size_t i, bytes = 0;
    (void)s;
    assert(F.reserved && v->serial == F.serial);
    if (F.state > SOPHIA_SS_READY) {
        F.reserved = 0;
        return F.final_status;
    }
    for (i = 0; i < count; i++) {
        size_t n = fake_encode(&records[i]);
        if (!n)
            return SOPHIA_9P_INVALID;
        bytes += n;
    }
    if (count > F.reserved_slots || bytes > F.reserved_bytes)
        return SOPHIA_9P_ARGUMENT;
    *first = F.next_ticket;
    for (i = 0; i < count; i++)
        (void)fake_log(&records[i]);
    F.reserved = 0;
    memset(v, 0, sizeof(*v));
    return 0;
}
int sophia_ss_cancel(struct sophia_ss *s, struct sophia_ss_reservation *v)
{
    (void)s;
    if (!F.reserved || v->serial != F.serial)
        return SOPHIA_9P_ARGUMENT;
    F.reserved = 0;
    return 0;
}
int sophia_ss_outcome(const struct sophia_ss *s, uint64_t ticket, enum sophia_ss_outcome *out,
                      uint32_t *error)
{
    (void)s;
    if (!ticket || ticket >= F.next_ticket)
        return SOPHIA_9P_ARGUMENT;
    *out = F.outcomes[ticket];
    if (error)
        *error = 0;
    return 0;
}
int sophia_ss_event(struct sophia_ss *s, const struct sophia_sf_record **out)
{
    (void)s;
    if (F.state > SOPHIA_SS_READY)
        return F.final_status;
    if (F.head == F.pushed)
        return SOPHIA_9P_AGAIN;
    *out = &F.events[F.head];
    return 0;
}
int sophia_ss_consume(struct sophia_ss *s)
{
    (void)s;
    assert(F.head < F.pushed);
    F.head++;
    return 0;
}
static int fake_upload(uint64_t *ticket)
{
    struct sophia_sf_record r;
    if (F.state > SOPHIA_SS_READY)
        return F.final_status;
    if (F.reserved)
        return SOPHIA_9P_BUSY;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_RESOURCE_BEGIN;
    F.uploads++;
    *ticket = fake_log(&r);
    return 0;
}
int sophia_ss_upload_begin(struct sophia_ss *s, struct sophia_sf_resource_begin v, uint64_t *t)
{
    (void)s;
    assert(v.transaction && v.grant_connection_epoch == F.limits.grant_connection_epoch);
    return fake_upload(t);
}
int sophia_ss_upload_end(struct sophia_ss *s, uint64_t transaction, uint64_t *t)
{
    (void)s;
    assert(transaction);
    return fake_upload(t);
}
int sophia_ss_upload_cancel(struct sophia_ss *s, uint64_t transaction, uint64_t *t)
{
    (void)s;
    assert(transaction);
    return fake_upload(t);
}

/* Journal one event: encoded and decoded by the library, so every scripted
 * record satisfies the contract's value rules; text borrows its slot. */
static void push(struct sophia_sf_record r)
{
    size_t n;
    assert(F.pushed < 32);
    r.header.epoch = EPOCH;
    r.header.submission = 0;
    r.header.sequence = ++F.sequence;
    assert(!sophia_sf_encode(F.bytes[F.pushed], sizeof(F.bytes[0]), &r, &n));
    assert(!sophia_sf_decode(F.bytes[F.pushed], n, &F.events[F.pushed]));
    F.pushed++;
}
static const struct sophia_sf_record *last(uint16_t kind)
{
    unsigned i = F.logged;
    while (i--)
        if (F.log[i].header.kind == kind)
            return &F.log[i];
    return NULL;
}

#define OPENING 7u
#define OUTPUT 11u
#define OUTPUT_GEN 12u
#define CATALOG 5u
#define FACTS 3u
#define ALLOC 21u
#define ALLOC_GEN 22u
#define EPOCH_P 9u
#define RES 31u
#define RES_GEN 32u
static const struct sophia_ns_config CONFIG = {100, 10};
static uint64_t conn(void) { return F.limits.grant_connection_epoch; }
static uint64_t content(void) { return F.limits.grant_content_epoch; }

static void reset(void)
{
    struct sophia_sf_record r;
    memset(&F, 0, sizeof(F));
    assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &r));
    F.limits = r.value.limits;
    /* The vector carries minimum bounds; grant realistic ones. */
    F.limits.max_candidate_bytes = 8192;
    F.limits.action_ack_timeout_ms = 1000;
    F.have_limits = 1;
    F.state = SOPHIA_SS_READY;
    F.final_status = SOPHIA_9P_CLOSED;
    F.next_ticket = 1;
}
static void published(uint16_t kind, uint64_t generation)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_OBJECT_PUBLISHED;
    r.value.object_published.object_kind = kind;
    r.value.object_published.generation = generation;
    r.value.object_published.qid = 0x40 + generation;
    push(r);
}
static void opening(uint64_t id)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_OPENING;
    r.value.native_opening = (struct sophia_sf_native_opening){60,     conn(),     content(), id,
                                                               OUTPUT, OUTPUT_GEN, CATALOG,   1};
    push(r);
}
static void allocation_result(uint64_t request)
{
    struct sophia_sf_record r;
    struct sophia_sf_allocation_result *v = &r.value.allocation_result;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_ALLOCATION_RESULT;
    v->transaction = 61;
    v->grant_connection_epoch = conn();
    v->grant_content_epoch = content();
    v->allocation_request_id = request;
    v->status = 1;
    v->output_id = OUTPUT;
    v->output_generation = OUTPUT_GEN;
    v->allocation_id = ALLOC;
    v->allocation_generation = ALLOC_GEN;
    v->scale_generation = 4;
    v->logical_width = v->pixel_width = 400;
    v->logical_height = v->pixel_height = 300;
    v->scale_numerator = v->scale_denominator = 1;
    push(r);
}
static void invalidated(void)
{
    struct sophia_sf_record r;
    struct sophia_sf_allocation_result *v = &r.value.allocation_result;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_ALLOCATION_RESULT;
    v->transaction = 68;
    v->grant_connection_epoch = conn();
    v->grant_content_epoch = content();
    v->status = 4;
    v->reason = 12;
    v->output_id = OUTPUT;
    v->output_generation = OUTPUT_GEN;
    v->allocation_id = ALLOC;
    v->allocation_generation = ALLOC_GEN;
    push(r);
}
static void resource_status(uint16_t status)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_RESOURCE_STATUS;
    r.value.resource_status =
        (struct sophia_sf_resource_status){62, conn(), content(), RES, RES_GEN, status, 0, 0, 0};
    push(r);
}
static void permit(uint64_t demand, uint16_t state, uint64_t id, uint32_t ttl)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_FRAME_PERMIT;
    r.value.frame_permit = (struct sophia_sf_frame_permit){63,
                                                           conn(),
                                                           content(),
                                                           OUTPUT,
                                                           OUTPUT_GEN,
                                                           demand,
                                                           id,
                                                           state,
                                                           (uint16_t)(state == 1 ? 0 : 6),
                                                           state == 1 ? ttl : 0,
                                                           state == 1 ? 8192u : 0};
    push(r);
}
static void outcome(const struct sophia_sf_record *candidate, uint16_t kind)
{
    const struct sophia_sf_candidate *c = &candidate->value.role_candidate.content;
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_CANDIDATE_OUTCOME;
    r.value.candidate_outcome = (struct sophia_sf_candidate_outcome){c->transaction,
                                                                     conn(),
                                                                     content(),
                                                                     c->candidate_generation,
                                                                     OUTPUT,
                                                                     OUTPUT_GEN,
                                                                     kind,
                                                                     (uint16_t)(kind > 2 ? 1 : 0),
                                                                     kind == 2 ? EPOCH_P : 0,
                                                                     0,
                                                                     0};
    push(r);
}
static struct sophia_sf_native_binding binding(uint64_t candidate, uint64_t revision,
                                               uint64_t lease)
{
    struct sophia_sf_native_binding b = {64,         conn(), content(), OPENING, OUTPUT,
                                         OUTPUT_GEN, ALLOC,  ALLOC_GEN, CATALOG, candidate,
                                         EPOCH_P,    1,      revision,  lease};
    return b;
}
static void focus(struct sophia_sf_native_binding b)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_FOCUS;
    r.value.native_focus = b;
    push(r);
}
static void input(struct sophia_sf_native_binding b, uint64_t event, uint64_t revision,
                  uint16_t kind, const char *text)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_INPUT;
    r.value.native_input.binding = b;
    r.value.native_input.event_id = event;
    r.value.native_input.state_revision = revision;
    r.value.native_input.issued_mono_usec = 5000000;
    r.value.native_input.kind = kind;
    r.value.native_input.text.data = (const uint8_t *)text;
    r.value.native_input.text.size = (uint16_t)strlen(text);
    push(r);
}
static void closed(uint64_t id)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_CLOSED;
    r.value.native_closed = (struct sophia_sf_native_closed){65, conn(), content(), id, 12};
    push(r);
}
static int next(struct sophia_ns *n, struct sophia_ns_event *e, enum sophia_ns_event_kind kind)
{
    int r = sophia_ns_next(n, 1000, e);
    assert(!r && e->kind == kind);
    return 0;
}
static void finish(struct sophia_ns *n, enum sophia_ns_event_kind kind)
{
    struct sophia_ns_event e;
    (void)next(n, &e, kind);
    assert(!sophia_ns_done(n));
}
static struct sophia_ns_scene scene(void)
{
    struct sophia_ns_scene s;
    memset(&s, 0, sizeof(s));
    s.placement_count = 1;
    s.placements[0].resource_id = RES;
    s.placements[0].resource_generation = RES_GEN;
    s.row_count = 1;
    s.rows[0] = 5;
    s.selected = 5;
    s.targets[0].target_id = 41;
    s.targets[0].target_generation = 42;
    s.targets[0].bounds_width = 10;
    s.targets[0].bounds_height = 10;
    return s;
}
/* Opening, catalog and facts installed, allocation granted, resource
 * accepted, a permit granted at local time 1000 with ttl 100. */
static void ready(struct sophia_ns *n)
{
    struct sophia_ns_allocation_params p = {1, 400, 300, 0, 0, 0, 0};
    struct sophia_sf_resource_begin begin;
    uint64_t t;
    reset();
    assert(!sophia_ns_init(n, &SS, &CONFIG));
    published(SOPHIA_SF_CATALOG, CATALOG);
    published(SOPHIA_SF_OUTPUTS, FACTS);
    opening(OPENING);
    finish(n, SOPHIA_NS_EV_OTHER);
    finish(n, SOPHIA_NS_EV_OTHER);
    finish(n, SOPHIA_NS_EV_OPENING);
    assert(!sophia_ns_catalog(n, CATALOG) && !sophia_ns_facts(n, FACTS));
    assert(!sophia_ns_allocate(n, &p, 1000, &t));
    assert(last(SOPHIA_SF_NATIVE_ALLOCATION_REQUEST)->value.native_allocation_request.operation ==
           1);
    allocation_result(
        last(SOPHIA_SF_NATIVE_ALLOCATION_REQUEST)->value.native_allocation_request.request_id);
    finish(n, SOPHIA_NS_EV_ALLOCATION);
    assert(sophia_ns_allocation(n) && sophia_ns_allocation(n)->allocation_id == ALLOC);
    memset(&begin, 0, sizeof(begin));
    begin.resource_id = RES;
    begin.resource_generation = RES_GEN;
    assert(!sophia_ns_upload_begin(n, begin, &t));
    resource_status(1);
    finish(n, SOPHIA_NS_EV_RESOURCE);
    assert(!sophia_ns_upload_end(n, &t));
    resource_status(2);
    finish(n, SOPHIA_NS_EV_RESOURCE);
    assert(!sophia_ns_demand(n, 1, 1000, &t));
    permit(last(SOPHIA_SF_FRAME_DEMAND)->value.frame_demand.demand_id, 1, 77, 100);
    finish(n, SOPHIA_NS_EV_PERMIT);
}
/* ready() plus a presented candidate and its exact held focus at revision 1. */
static const struct sophia_sf_record *shown(struct sophia_ns *n)
{
    struct sophia_ns_scene s = scene();
    struct sophia_ns_event e;
    const struct sophia_sf_record *c;
    uint64_t t;
    ready(n);
    assert(!sophia_ns_present(n, &s, 1050, &t));
    c = last(SOPHIA_SF_NATIVE_CANDIDATE);
    outcome(c, 2);
    finish(n, SOPHIA_NS_EV_CANDIDATE);
    focus(binding(c->value.role_candidate.content.candidate_generation, 1, 1));
    (void)next(n, &e, SOPHIA_NS_EV_FOCUS);
    assert(e.held && !sophia_ns_done(n) && sophia_ns_focus(n));
    return c;
}

static void test_init(void)
{
    struct sophia_ns n;
    reset();
    F.state = SOPHIA_SS_NEGOTIATING;
    assert(sophia_ns_init(&n, &SS, &CONFIG) == SOPHIA_9P_BUSY);
    F.state = SOPHIA_SS_READY;
    F.have_limits = 0;
    assert(sophia_ns_init(&n, &SS, &CONFIG) == SOPHIA_9P_BUSY);
    F.have_limits = 1;
    assert(!sophia_ns_init(&n, &SS, &CONFIG));
    assert(sophia_ns_next(&n, 1000, &(struct sophia_ns_event){0}) == SOPHIA_9P_AGAIN);
}
/* A newer catalog announcement disarms the opening's candidates until the
 * opening's own generation is again installed and latest. */
static void test_catalog_invalidation(void)
{
    struct sophia_ns n;
    struct sophia_ns_obligations o;
    struct sophia_ns_scene s = scene();
    uint64_t t;
    ready(&n);
    assert(!sophia_ns_obligations(&n, &o) && !o.needs_catalog && !o.needs_facts);
    published(SOPHIA_SF_CATALOG, CATALOG + 1);
    finish(&n, SOPHIA_NS_EV_OTHER);
    assert(!sophia_ns_obligations(&n, &o) && o.needs_catalog);
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_INVALID);
    assert(!last(SOPHIA_SF_NATIVE_CANDIDATE));
    published(SOPHIA_SF_OUTPUTS, FACTS + 1);
    finish(&n, SOPHIA_NS_EV_OTHER);
    assert(!sophia_ns_obligations(&n, &o) && o.needs_facts);
    /* A second opening while one is open cannot be correlated. */
    opening(OPENING + 1);
    assert(sophia_ns_next(&n, 1000, &(struct sophia_ns_event){0}) == SOPHIA_9P_INVALID);
    assert(sophia_ns_state(&n) == SOPHIA_NS_FAILED);
}
/* Allocation, upload and permit strictly precede a candidate; the candidate
 * names the permit and consumes it; nothing is shown before Presented. */
static void test_pacing(void)
{
    struct sophia_ns n;
    struct sophia_ns_allocation_params p = {1, 400, 300, 0, 0, 0, 0};
    struct sophia_ns_scene s = scene();
    struct sophia_ns_obligations o;
    const struct sophia_sf_record *c;
    uint64_t t;
    reset();
    assert(!sophia_ns_init(&n, &SS, &CONFIG));
    assert(sophia_ns_allocate(&n, &p, 1000, &t) == SOPHIA_9P_INVALID); /* no opening */
    ready(&n);
    assert(sophia_ns_demand(&n, 1, 1000, &t) == SOPHIA_9P_INVALID); /* one demand */
    assert(sophia_ns_allocate(&n, &p, 1000, &t) == SOPHIA_9P_OK);   /* resize */
    assert(last(SOPHIA_SF_NATIVE_ALLOCATION_REQUEST)->value.native_allocation_request.operation ==
           2);
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_INVALID); /* request pending */
    allocation_result(
        last(SOPHIA_SF_NATIVE_ALLOCATION_REQUEST)->value.native_allocation_request.request_id);
    finish(&n, SOPHIA_NS_EV_ALLOCATION);
    s.placements[0].resource_generation = RES_GEN + 1;
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_INVALID); /* not accepted */
    s = scene();
    assert(sophia_ns_present(&n, &s, 1095, &t) == SOPHIA_9P_INVALID); /* inside margin */
    F.submit_busy = 1;
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_BUSY);
    F.submit_busy = 0;
    assert(!sophia_ns_present(&n, &s, 1050, &t));
    c = last(SOPHIA_SF_NATIVE_CANDIDATE);
    assert(c->value.role_candidate.content.pacing_permit == 77);
    assert(c->value.role_candidate.content.facts_generation == FACTS);
    assert(c->value.role_candidate.content.surfaces[0].allocation_id == ALLOC);
    assert(c->value.role_candidate.state_revision == 1 &&
           c->value.role_candidate.opening == OPENING);
    assert(c->value.role_candidate.content.targets[0].action_id == 5);
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_INVALID); /* permit consumed */
    assert(sophia_ns_demand(&n, 1, 1060, &t) == SOPHIA_9P_INVALID);   /* candidate assembling */
    assert(!sophia_ns_obligations(&n, &o) && !o.permit_expires_ms && o.candidate_due_ms);
    outcome(c, 1);
    finish(&n, SOPHIA_NS_EV_CANDIDATE);
    assert(!sophia_ns_presented(&n)); /* Prepared only */
    assert(sophia_ns_demand(&n, 1, 1060, &t) == SOPHIA_9P_INVALID);
    focus(binding(c->value.role_candidate.content.candidate_generation, 1, 1));
    {
        struct sophia_ns_event e;
        (void)next(&n, &e, SOPHIA_NS_EV_FOCUS);
        assert(!e.held && !sophia_ns_done(&n) && !sophia_ns_focus(&n));
    }
    outcome(c, 2);
    finish(&n, SOPHIA_NS_EV_CANDIDATE);
    assert(sophia_ns_presented(&n) && sophia_ns_presented(&n)->presentation_epoch == EPOCH_P);
    assert(!sophia_ns_obligations(&n, &o) && !o.needs_candidate);
    /* A late expiry of the consumed permit's demand changes nothing. */
    permit(1, 2, 77, 0);
    finish(&n, SOPHIA_NS_EV_PERMIT);
    /* An outcome for no candidate of ours cannot be correlated. */
    outcome(c, 3);
    assert(sophia_ns_next(&n, 1000, &(struct sophia_ns_event){0}) == SOPHIA_9P_INVALID);
}
/* A local permit bound expires; the demand stays until the server ends it. */
static void test_permit_expiry(void)
{
    struct sophia_ns n;
    struct sophia_ns_scene s = scene();
    struct sophia_ns_obligations o;
    uint64_t t;
    ready(&n);
    assert(!sophia_ns_obligations(&n, &o) && o.permit_expires_ms == 1100 && o.next_ms == 1100);
    assert(!sophia_ns_service(&n, 1100));
    assert(!sophia_ns_obligations(&n, &o) && !o.permit_expires_ms);
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_INVALID);
    assert(sophia_ns_demand_cancel(&n, 1050, &t) == SOPHIA_9P_INVALID);
    assert(sophia_ns_demand(&n, 1, 1100, &t) == SOPHIA_9P_INVALID);
    permit(1, 2, 77, 0);
    finish(&n, SOPHIA_NS_EV_PERMIT);
    assert(!sophia_ns_demand(&n, 1, 1200, &t));
    assert(last(SOPHIA_SF_FRAME_DEMAND)->value.frame_demand.demand_id == 2);
    /* A demand refused at submit ends locally. */
    F.outcomes[t] = SOPHIA_SS_REFUSED;
    assert(!sophia_ns_service(&n, 1200));
    assert(!sophia_ns_demand(&n, 1, 1200, &t));
    permit(3, 1, 78, 100);
    finish(&n, SOPHIA_NS_EV_PERMIT);
    assert(!sophia_ns_demand_cancel(&n, 1250, &t));
    assert(last(SOPHIA_SF_FRAME_DEMAND_CANCEL)->value.frame_demand_cancel.permit_id == 78);
    assert(sophia_ns_present(&n, &s, 1250, &t) == SOPHIA_9P_INVALID);
    /* A candidate refused at submit returns its permit to the server's end. */
    permit(3, 3, 78, 0);
    finish(&n, SOPHIA_NS_EV_PERMIT);
    assert(!sophia_ns_demand(&n, 1, 1300, &t));
    permit(4, 1, 79, 100);
    finish(&n, SOPHIA_NS_EV_PERMIT);
    assert(!sophia_ns_present(&n, &s, 1310, &t));
    F.outcomes[t] = SOPHIA_SS_REFUSED;
    assert(!sophia_ns_service(&n, 1320));
    assert(!sophia_ns_presented(&n) && sophia_ns_demand(&n, 1, 1320, &t) == SOPHIA_9P_INVALID);
    permit(4, 2, 79, 0);
    finish(&n, SOPHIA_NS_EV_PERMIT);
    assert(!sophia_ns_demand(&n, 1, 1400, &t));
}
/* Focus is held only for the exact presented generation. */
static void test_focus(void)
{
    struct sophia_ns n;
    struct sophia_ns_event e;
    struct sophia_sf_record r;
    const struct sophia_sf_record *c = shown(&n);
    uint64_t g = c->value.role_candidate.content.candidate_generation;
    struct sophia_sf_native_binding b = binding(g, 1, 1);
    /* Revocation of the held focus clears it. */
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_FOCUS_REVOKED;
    r.value.native_focus_revoked.binding = b;
    r.value.native_focus_revoked.binding.transaction = 66;
    r.value.native_focus_revoked.reason = 1;
    push(r);
    (void)next(&n, &e, SOPHIA_NS_EV_FOCUS_REVOKED);
    assert(e.current && !sophia_ns_done(&n) && !sophia_ns_focus(&n));
    /* Wrong generation, epoch or revision: never held. */
    focus(binding(g + 1, 1, 2));
    (void)next(&n, &e, SOPHIA_NS_EV_FOCUS);
    assert(!e.held && !sophia_ns_done(&n));
    b = binding(g, 1, 3);
    b.presentation_epoch = EPOCH_P + 1;
    focus(b);
    (void)next(&n, &e, SOPHIA_NS_EV_FOCUS);
    assert(!e.held && !sophia_ns_done(&n));
    focus(binding(g, 2, 4));
    (void)next(&n, &e, SOPHIA_NS_EV_FOCUS);
    assert(!e.held && !sophia_ns_done(&n) && !sophia_ns_focus(&n));
    focus(binding(g, 1, 5));
    (void)next(&n, &e, SOPHIA_NS_EV_FOCUS);
    assert(e.held && !sophia_ns_done(&n) && sophia_ns_focus(&n)->focus_lease == 5);
}
static unsigned edits;
static int edit(void *user, const struct sophia_sf_native_input *v)
{
    (void)user;
    assert(v->kind == 1 && v->text.size == 2 && !memcmp(v->text.data, "ab", 2));
    edits++;
    return 1;
}
/* The edit happens only after ack room is reserved, never again on BUSY or
 * redelivery, and its ack is admitted exactly once. */
static void test_input(void)
{
    struct sophia_ns n;
    struct sophia_ns_event e;
    struct sophia_ns_obligations o;
    const struct sophia_sf_record *c = shown(&n);
    uint64_t g = c->value.role_candidate.content.candidate_generation, t;
    unsigned before;
    input(binding(g, 1, 1), 10, 2, 1, "ab");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(e.editable && !e.can_activate && sophia_ns_revision(&n) == 2);
    assert(sophia_ns_done(&n) == SOPHIA_9P_ARGUMENT);
    assert(!sophia_ns_obligations(&n, &o) &&
           o.input_due_ms == 5000 + F.limits.action_ack_timeout_ms);
    edits = 0;
    F.reserve_busy = 1;
    assert(sophia_ns_input_apply(&n, 0, edit, NULL, &t) == SOPHIA_9P_BUSY && !edits);
    F.reserve_busy = 0;
    /* Redelivered head: same event, not reapplied. */
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(e.editable && sophia_ns_revision(&n) == 2);
    /* Explicit path: reserve, cancel before any edit, reserve again. */
    assert(!sophia_ns_input_reserve(&n, 0));
    assert(sophia_ns_input_reserve(&n, 0) == SOPHIA_9P_ARGUMENT);
    assert(!sophia_ns_input_cancel(&n) && !F.reserved);
    assert(sophia_ns_input_reserve(&n, 1) == SOPHIA_9P_INVALID); /* not an Accept */
    before = F.logged;
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t) && edits == 1);
    assert(F.logged == before + 1 && F.log[before].header.kind == SOPHIA_SF_NATIVE_INPUT_ACK);
    assert(F.log[before].value.native_input_ack.event_id == 10);
    assert(F.log[before].value.native_input_ack.disposition == 1);
    assert(F.log[before].value.native_input_ack.state_revision == 2);
    assert(F.log[before].value.native_input_ack.binding.state_revision == 1);
    assert(F.head == F.pushed);
    assert(sophia_ns_input_apply(&n, 0, edit, NULL, &t) == SOPHIA_9P_ARGUMENT && edits == 1);
    assert(sophia_ns_input_commit(&n, 1, &t) == SOPHIA_9P_ARGUMENT);
    assert(!sophia_ns_obligations(&n, &o) && o.needs_candidate && !o.input_due_ms);
    /* A refused ack is lost, not resent. */
    F.outcomes[t] = SOPHIA_SS_REFUSED;
    assert(!sophia_ns_service(&n, 1100));
    assert(!sophia_ns_obligations(&n, &o) && o.lost_acks == 1);
    /* An input bound to no held focus is never editable: acked Stale. */
    input(binding(g + 1, 1, 9), 11, 3, 1, "ab");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!e.editable);
    before = F.logged;
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t) && edits == 1);
    assert(F.log[before].value.native_input_ack.disposition == 2);
}
/* Keyboard activation: NativeActivate precedes the Accept's ack in one
 * commit; the outcome must echo it; Admitted ends activation. */
static void test_activation(void)
{
    struct sophia_ns n;
    struct sophia_ns_event e;
    struct sophia_sf_record r;
    const struct sophia_sf_record *c = shown(&n);
    uint64_t g = c->value.role_candidate.content.candidate_generation, t;
    unsigned before;
    input(binding(g, 1, 1), 20, 1, 17, "");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(e.can_activate && !e.editable);
    before = F.logged;
    assert(!sophia_ns_input_apply(&n, 1, edit, NULL, &t));
    assert(F.logged == before + 2);
    assert(F.log[before].header.kind == SOPHIA_SF_NATIVE_ACTIVATE);
    assert(F.log[before].value.native_activate.cause == 1);
    assert(F.log[before].value.native_activate.slot == 5);
    assert(F.log[before].value.native_activate.event_id == 20);
    assert(F.log[before + 1].header.kind == SOPHIA_SF_NATIVE_INPUT_ACK);
    assert(F.log[before + 1].value.native_input_ack.disposition == 1);
    /* A second Accept cannot activate while one is pending. */
    input(binding(g, 1, 1), 21, 1, 17, "");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!e.can_activate);
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t));
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_ACTIVATION_OUTCOME;
    r.value.native_activation_outcome.activation = F.log[before].value.native_activate;
    r.value.native_activation_outcome.status = 1;
    push(r);
    (void)next(&n, &e, SOPHIA_NS_EV_ACTIVATION);
    assert(e.current && !sophia_ns_done(&n));
    assert(!sophia_ns_focus(&n)); /* Admitted disarms focus at once */
    input(binding(g, 1, 1), 22, 1, 17, "");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!e.can_activate && !e.editable); /* this opening was admitted */
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t));
    /* A matching NativeFocus after Admitted does not re-arm focus, and query
     * input stays non-editable until a new opening. */
    focus(binding(g, 1, 2));
    (void)next(&n, &e, SOPHIA_NS_EV_FOCUS);
    assert(!e.held && !sophia_ns_done(&n) && !sophia_ns_focus(&n));
    input(binding(g, 1, 2), 23, 2, 1, "ab");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!e.editable);
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t));
    assert(last(SOPHIA_SF_NATIVE_INPUT_ACK)->value.native_input_ack.disposition == 2);
    closed(OPENING);
    finish(&n, SOPHIA_NS_EV_CLOSED);
    opening(OPENING + 1);
    finish(&n, SOPHIA_NS_EV_OPENING);
    input(binding(g, 1, 2), 24, 2, 1, "ab"); /* old binding: still not editable */
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!e.editable && !e.current);
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t));
    /* An outcome echoing nothing pending cannot be correlated. */
    push(r);
    assert(sophia_ns_next(&n, 1000, &e) == SOPHIA_9P_INVALID);
}
static void action(uint64_t generation, uint64_t event, uint16_t kind)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_ACTION;
    r.value.action =
        (struct sophia_sf_action){67,         conn(),     content(), OUTPUT,
                                  OUTPUT_GEN, generation, EPOCH_P,   1,
                                  ALLOC,      ALLOC_GEN,  41,        42,
                                  5,          event,      kind,      (uint16_t)(kind == 3 ? 1 : 0)};
    push(r);
}
/* Action acknowledgement is explicit, BUSY-safe and consumes the event; a
 * pointer activation is sent only on request, before the ack, atomically. */
static void test_action(void)
{
    struct sophia_ns n;
    struct sophia_ns_event e;
    const struct sophia_sf_record *c = shown(&n);
    uint64_t g = c->value.role_candidate.content.candidate_generation, t;
    unsigned before;
    action(g, 30, 1);
    (void)next(&n, &e, SOPHIA_NS_EV_ACTION);
    assert(e.current && e.ack_owed && e.can_activate && sophia_ns_done(&n) == SOPHIA_9P_ARGUMENT);
    F.submit_busy = 1;
    assert(sophia_ns_action_ack(&n, 1, 1, &t) == SOPHIA_9P_BUSY && F.head < F.pushed);
    F.submit_busy = 0;
    before = F.logged;
    assert(!sophia_ns_action_ack(&n, 2, 0, &t) && F.head == F.pushed && F.logged == before + 1);
    assert(last(SOPHIA_SF_ACTION_ACK)->value.action_ack.event_id == 30);
    assert(last(SOPHIA_SF_ACTION_ACK)->value.action_ack.disposition == 2);
    assert(!last(SOPHIA_SF_NATIVE_ACTIVATE)); /* nothing unless requested */
    action(g, 31, 1);
    (void)next(&n, &e, SOPHIA_NS_EV_ACTION);
    before = F.logged;
    assert(!sophia_ns_action_ack(&n, 1, 1, &t) && F.logged == before + 2);
    assert(F.log[before].header.kind == SOPHIA_SF_NATIVE_ACTIVATE);
    assert(F.log[before].value.native_activate.cause == 2);
    assert(F.log[before].value.native_activate.slot == 5);
    assert(F.log[before].value.native_activate.event_id == 31);
    assert(F.log[before].value.native_activate.binding.focus_lease == 1);
    assert(F.log[before + 1].header.kind == SOPHIA_SF_ACTION_ACK);
    /* One activation at a time. */
    action(g, 32, 1);
    (void)next(&n, &e, SOPHIA_NS_EV_ACTION);
    assert(!e.can_activate && sophia_ns_action_ack(&n, 1, 1, &t) == SOPHIA_9P_INVALID);
    assert(!sophia_ns_action_ack(&n, 1, 0, &t));
    /* Kind 3 is a cancellation: no ack is owed or sendable. */
    action(g, 33, 3);
    (void)next(&n, &e, SOPHIA_NS_EV_ACTION);
    assert(!e.ack_owed && sophia_ns_action_ack(&n, 1, 0, &t) == SOPHIA_9P_ARGUMENT);
    before = F.logged;
    assert(!sophia_ns_done(&n) && F.logged == before);
}
/* Closed ends the opening: no focus, nothing presented, allocation unusable. */
static void test_close(void)
{
    struct sophia_ns n;
    struct sophia_ns_event e;
    struct sophia_ns_allocation_params p = {1, 400, 300, 0, 0, 0, 0};
    const struct sophia_sf_record *c = shown(&n);
    uint64_t t;
    input(binding(c->value.role_candidate.content.candidate_generation, 1, 1), 40, 2, 1, "ab");
    closed(OPENING);
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t));
    finish(&n, SOPHIA_NS_EV_CLOSED);
    assert(!sophia_ns_opening(&n) && !sophia_ns_focus(&n) && !sophia_ns_presented(&n));
    assert(!sophia_ns_allocation(&n) && !sophia_ns_revision(&n));
    assert(sophia_ns_allocate(&n, &p, 1500, &t) == SOPHIA_9P_INVALID);
    /* A late input of the closed opening is still acknowledged, not edited. */
    input(binding(c->value.role_candidate.content.candidate_generation, 1, 1), 41, 3, 1, "ab");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!e.current && !e.editable);
    assert(!sophia_ns_input_apply(&n, 0, edit, NULL, &t));
    /* A new opening; the closed opening's allocation still blocks a second
     * one until the Session invalidates it (status 4, reason 12). */
    opening(OPENING + 1);
    finish(&n, SOPHIA_NS_EV_OPENING);
    assert(sophia_ns_revision(&n) == 1);
    assert(sophia_ns_allocate(&n, &p, 1500, &t) == SOPHIA_9P_INVALID);
    {
        struct sophia_ns_obligations o;
        assert(!sophia_ns_obligations(&n, &o) && o.awaiting_invalidation);
    }
    invalidated();
    (void)next(&n, &e, SOPHIA_NS_EV_ALLOCATION);
    assert(e.current && !sophia_ns_done(&n));
    assert(!sophia_ns_allocate(&n, &p, 1500, &t));
    assert(last(SOPHIA_SF_NATIVE_ALLOCATION_REQUEST)->value.native_allocation_request.operation ==
           1);
    assert(last(SOPHIA_SF_NATIVE_ALLOCATION_REQUEST)->value.native_allocation_request.opening ==
           OPENING + 1);
    /* Reopening requires a larger opening ID. */
    closed(OPENING + 1);
    finish(&n, SOPHIA_NS_EV_CLOSED);
    opening(OPENING + 1);
    assert(sophia_ns_next(&n, 1000, &e) == SOPHIA_9P_INVALID);
}
/* A final ss ends the ns: nothing is presented or focused afterwards, a held
 * reservation commits nothing, and no call replays. */
static void test_terminal(void)
{
    struct sophia_ns n;
    struct sophia_ns_event e;
    struct sophia_ns_scene s = scene();
    const struct sophia_sf_record *c = shown(&n);
    uint64_t t;
    unsigned before;
    input(binding(c->value.role_candidate.content.candidate_generation, 1, 1), 50, 2, 1, "ab");
    (void)next(&n, &e, SOPHIA_NS_EV_INPUT);
    assert(!sophia_ns_input_reserve(&n, 0));
    F.outcomes[F.next_ticket - 1] = SOPHIA_SS_UNKNOWN_DISCONNECTED;
    F.state = SOPHIA_SS_CLOSED;
    before = F.logged;
    assert(sophia_ns_input_commit(&n, 1, &t) == SOPHIA_9P_CLOSED && F.logged == before);
    assert(!F.reserved);
    assert(sophia_ns_state(&n) == SOPHIA_NS_ENDED);
    assert(sophia_ns_service(&n, 2000) == SOPHIA_9P_CLOSED);
    assert(!sophia_ns_presented(&n) && !sophia_ns_focus(&n) && !sophia_ns_opening(&n));
    assert(sophia_ns_present(&n, &s, 1050, &t) == SOPHIA_9P_CLOSED);
    assert(sophia_ns_next(&n, 2000, &e) == SOPHIA_9P_CLOSED);
    assert(sophia_ns_input_reserve(&n, 0) == SOPHIA_9P_CLOSED);
}
int main(void)
{
    test_init();
    test_catalog_invalidation();
    test_pacing();
    test_permit_expiry();
    test_focus();
    test_input();
    test_activation();
    test_action();
    test_close();
    test_terminal();
    puts("native_session_test: ok");
    return 0;
}
