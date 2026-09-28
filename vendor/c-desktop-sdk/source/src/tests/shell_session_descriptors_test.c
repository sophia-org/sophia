#include "shell_session_peer.h"

static uint8_t object_scratch[1048576], object_wire[8][430000], row_scratch[430000];
static struct sophia_ss_config config(uint16_t revision, uint64_t caps)
{
    struct sophia_ss_config c = {{1, revision, caps}, SOPHIA_SF_DESCRIPTOR,  4096, 4, 8192,
                                 object_scratch,      sizeof(object_scratch)};
    return c;
}
static void start(struct rig *r, uint16_t revision, uint64_t caps)
{
    struct sophia_ss_config c = config(revision, caps);
    rig_start(r, &c);
    r->p.descriptor = 1;
}
static void ready(struct rig *r, uint16_t revision, uint64_t caps)
{
    start(r, revision, caps);
    rig_settle(r);
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_READY);
    rig_ack(r);
    assert(r->s.files.welcome.capabilities == (caps | 2u));
    assert(r->s.files.have_limits == !!(caps & (1u << 7)));
}
static struct sophia_sf_record candidate(uint64_t generation)
{
    struct sophia_sf_record r = {
        {SOPHIA_SF_DESCRIPTOR_CANDIDATE, PEER_EPOCH, 0, 0},
        .value.descriptor_candidate = {1, PEER_EPOCH, 7, generation, 1, 0, 0, 0, 0, 0, {{0}}}};
    return r;
}
static void snapshot(struct rig *r, unsigned kind, uint64_t generation, uint64_t qid)
{
    struct sophia_sf_record v = {0};
    v.header = (struct sophia_sf_header){(uint16_t)kind, PEER_EPOCH, 0, 0};
    size_t n;
    if (kind == SOPHIA_SF_DESCRIPTORS) {
        struct sophia_sf_descriptor_entry e = {1, 0, 0,          0, 0, 2,        3,
                                               4, 5, PEER_EPOCH, 1, 2, {NULL, 0}};
        assert(!sophia_sf_descriptor_entry_encode(row_scratch, &e));
        v.value.descriptors = (struct sophia_sf_descriptors){1, PEER_EPOCH, generation,  1,  1, 4,
                                                             5, 1,          row_scratch, 196};
    } else if (kind == SOPHIA_SF_TABS) {
        /* A real multi-read object, beyond both inline and single 9P buffers. */
        struct sophia_sf_tab_group g = {1, 1, 1, 1, 128};
        struct sophia_sf_descriptor_entry e = {1, 0, 0,          0, 0, 2,        3,
                                               4, 5, PEER_EPOCH, 1, 2, {NULL, 0}};
        assert(!sophia_sf_tab_group_encode(row_scratch, &g));
        for (unsigned i = 0; i < 128; i++) {
            e.slot = e.action_target_slot = (uint16_t)(i + 1);
            assert(!sophia_sf_descriptor_entry_encode(row_scratch + 24 + i * 196, &e));
        }
        v.value.tabs =
            (struct sophia_sf_tabs){1, PEER_EPOCH, generation, 1, 128, row_scratch, 24 + 128 * 196};
    } else if (kind == SOPHIA_SF_SHORTCUTS) {
        struct sophia_sf_shortcut_entry e = {
            1,         0,        0, {(const uint8_t *)"M-1", 3}, {(const uint8_t *)"focus", 5},
            {NULL, 0}, {NULL, 0}};
        for (unsigned i = 0; i < 64; i++) {
            e.slot = (uint16_t)(i + 1);
            assert(!sophia_sf_shortcut_entry_encode(row_scratch + i * 408, &e));
        }
        v.value.shortcuts =
            (struct sophia_sf_shortcuts){1, PEER_EPOCH, generation, 64, row_scratch, 64 * 408};
    } else if (kind == SOPHIA_SF_CATALOG) {
        v.value.catalog = (struct sophia_sf_catalog){1, PEER_EPOCH, generation, 0, 0, NULL, 0};
    } else {
        assert(kind == SOPHIA_SF_INDICATORS);
        v.value.indicators =
            (struct sophia_sf_indicators){1, PEER_EPOCH, generation, 0, 0, 0, 0, NULL, 0};
    }
    assert(!sophia_sf_encode(object_wire[kind], sizeof(object_wire[kind]), &v, &n));
    r->p.object_data[kind] = object_wire[kind];
    r->p.object_bytes[kind] = n;
    r->p.object_qids[kind] = qid;
}
static void consume_publication(struct rig *r, unsigned kind, uint64_t generation, uint64_t qid)
{
    const struct sophia_sf_record *e;
    peer_published(&r->p, (uint16_t)kind, generation, qid);
    rig_run(r, 8);
    assert(!sophia_ss_event(&r->s, &e) && e->header.kind == SOPHIA_SF_OBJECT_PUBLISHED &&
           e->value.object_published.object_kind == kind);
    assert(!sophia_ss_consume(&r->s));
}
static int fetch(struct rig *r, uint16_t kind, uint64_t generation, uint64_t qid)
{
    const struct sophia_sf_record *o = NULL;
    int status = SOPHIA_9P_BUSY;
    assert(!sophia_ss_object(&r->s, kind, generation, qid));
    for (unsigned i = 0; i < 2048 && status == SOPHIA_9P_BUSY; i++) {
        rig_step(r);
        status = sophia_ss_object_result(&r->s, &o);
    }
    assert(status != SOPHIA_9P_BUSY);
    if (!status)
        assert(o && o->header.kind == kind && o->header.epoch == PEER_EPOCH);
    return status;
}
static void readiness(void)
{
    struct rig r;
    start(&r, 1, 1);
    r.p.hold_bootstrap_submitted = 1;
    rig_run(&r, 128);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_NEGOTIATING && r.s.files.welcome_consumed &&
           !r.s.files.bootstrap_custody_consumed && !r.s.files.have_limits);
    peer_submitted(&r.p, 1, SOPHIA_SF_NEGOTIATE);
    rig_run(&r, 8);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    assert(r.s.files.welcome.capabilities == 3 && !sophia_ss_limits(&r.s));
    assert(sophia_ss_welcome(&r.s)->selected_revision == 1);
    for (unsigned kind = 1; kind < 8; kind++)
        assert(!r.p.object_walks[kind]);
    rig_close(&r);
    start(&r, 8, 129);
    r.p.object_error = 13;
    rig_run(&r, 128);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && !sophia_ss_limits(&r.s));
    rig_close(&r);
    ready(&r, 8, 1 | (1u << 7) | (1u << 8));
    assert(r.p.object_walks[SOPHIA_SF_LIMITS] == 1 && sophia_ss_limits(&r.s));
    struct sophia_sf_record v = rig_request(1);
    uint64_t ticket;
    assert(!sophia_ss_submit(&r.s, &v, 1, &ticket));
    assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
    ready(&r, UINT16_MAX, 1);
    assert(r.s.files.welcome.selected_revision == 8);
    rig_close(&r);
}
static void bad_offers(void)
{
    const struct sophia_sf_negotiate bad[] = {{0, 1, 1},
                                              {2, 1, 1},
                                              {9, 9, 1},
                                              {1, 8, 0},
                                              {1, 8, 2},
                                              {1, 1, 5},
                                              {1, 2, 9},
                                              {1, 3, 33},
                                              {1, 4, 129},
                                              {1, 5, 513},
                                              {1, 8, 17},
                                              {1, 8, 65},
                                              {1, 8, 257},
                                              {1, 8, 1025},
                                              {1, 8, 2049},
                                              {1, 8, 4097},
                                              {1, 8, UINT64_C(0x8000000000000001)}};
    for (unsigned i = 0; i < sizeof(bad) / sizeof(bad[0]); i++) {
        struct rig r;
        struct sophia_ss_config c = config(8, 1);
        size_t bytes = sophia_ss_storage_bytes(c.msize, c.queue_bytes);
        memset(&r, 0, sizeof(r));
        assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, r.fd));
        r.storage = malloc(bytes);
        assert(r.storage);
        r.p.fd = r.fd[1];
        c.offer = bad[i];
        assert(sophia_ss_open_fd(&r.s, r.fd[0], &c, r.storage, bytes) == SOPHIA_9P_ARGUMENT);
        peer_pump(&r.p);
        assert(!r.p.fid_count && !r.p.submits && !sophia_9p_wants_write(&r.s.wire));
        rig_close(&r);
    }
    const struct {
        uint16_t rev;
        uint64_t caps;
    } good[] = {{1, 1},  {1, 3},   {2, 5},   {3, 9},   {3, 25},   {4, 33},
                {4, 97}, {5, 129}, {5, 385}, {6, 513}, {6, 1537}, {8, 0x7ff}};
    for (unsigned i = 0; i < sizeof(good) / sizeof(good[0]); i++) {
        struct rig r;
        ready(&r, good[i].rev, good[i].caps);
        rig_close(&r);
    }
}
static void wrong_role_and_welcome(void)
{
    struct rig r;
    struct sophia_ss_config c = config(8, 1);
    rig_start(&r, &c); /* Bar api cannot authorize descriptor. */
    rig_run(&r, 128);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && !r.p.submits);
    rig_close(&r);
    rig_start(&r, &rig_config);
    r.p.descriptor = 1;
    rig_run(&r, 128);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && !r.p.submits);
    rig_close(&r);
    rig_ready(&r, &rig_config);
    struct sophia_sf_record descriptor = candidate(1);
    uint64_t ticket = 1234;
    assert(sophia_ss_submit(&r.s, &descriptor, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(ticket == 1234 && r.s.next_ticket == 1 && r.p.submits == 1);
    rig_close(&r);
    for (unsigned fault = 0; fault < 6; fault++) {
        struct sophia_sf_negotiated v = {8, PEER_EPOCH, 3, 16, 128, 16, 0};
        start(&r, 8, fault == 5 ? 129 : 1);
        switch (fault) {
        case 0:
            v.selected_revision = 7;
            break;
        case 1:
            v.capabilities = 1;
            break;
        case 2:
            v.capabilities = 7;
            break;
        case 3:
            r.p.wrong_welcome_epoch = 1;
            break;
        case 4:
            v.limits_published = 1;
            break;
        case 5:
            v.capabilities = 131;
            break;
        }
        r.p.welcome_override = &v;
        rig_run(&r, 128);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && !r.p.object_walks[1]);
        rig_close(&r);
    }
}
static void selected_feeds_and_holds(void)
{
    struct rig r;
    struct sophia_ss_obligations o;
    struct sophia_sf_record records[2] = {candidate(1), candidate(2)};
    uint64_t ticket;
    ready(&r, 8, 1 | 4 | 8);
    r.p.object_read_max = 127;
    for (unsigned kind = 5; kind <= 7; kind++) {
        snapshot(&r, kind, 7, 100 + kind);
        consume_publication(&r, kind, 7, 100 + kind);
    }
    assert(!sophia_ss_obligations(&r.s, &o) && o.objects == 0x70 && o.ack_limit == 2);
    assert(!sophia_ss_submit(&r.s, records, 2, &ticket));
    rig_run(&r, 16);
    assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED &&
           rig_outcome(&r, ticket + 1) == SOPHIA_SS_ADMITTED_LOCAL && r.p.submits == 2);
    for (unsigned kind = 5; kind <= 7; kind++) {
        assert(!sophia_ss_object(&r.s, (uint16_t)kind, 7, 100 + kind));
        uint64_t prior_limit = sophia_ss_ack_limit(&r.s);
        const struct sophia_sf_record *v = NULL;
        int status = SOPHIA_9P_BUSY;
        for (unsigned i = 0; i < 2048 && status == SOPHIA_9P_BUSY; i++) {
            rig_step(&r);
            assert(sophia_ss_ack_limit(&r.s) == prior_limit);
            status = sophia_ss_object_result(&r.s, &v);
        }
        assert(!status && v && v->header.kind == kind && r.p.object_eofs[kind]);
        assert(r.p.acked == 2 && r.p.submits == 2);
    }
    assert(!sophia_ss_obligations(&r.s, &o) && !o.objects && !o.blocked);
    rig_ack(&r);
    assert(rig_until(&r, ticket + 1, 32) == SOPHIA_SS_SUBMITTED && !r.p.transaction_busy);
    rig_close(&r);
}
static void refused_and_superseded_fetches(void)
{
    struct rig r;
    ready(&r, 8, 1 | 4 | 8);
    snapshot(&r, SOPHIA_SF_SHORTCUTS, 7, 100);
    consume_publication(&r, SOPHIA_SF_SHORTCUTS, 7, 100);
    r.p.object_qids[SOPHIA_SF_SHORTCUTS] = 101;
    assert(fetch(&r, SOPHIA_SF_SHORTCUTS, 7, 100) == SOPHIA_9P_AGAIN);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    r.p.object_qids[SOPHIA_SF_SHORTCUTS] = 100;
    assert(fetch(&r, SOPHIA_SF_SHORTCUTS, 8, 100) == SOPHIA_9P_AGAIN);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    r.p.object_trailing = 1;
    assert(fetch(&r, SOPHIA_SF_SHORTCUTS, 7, 100) == SOPHIA_9P_INVALID);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    r.p.object_trailing = 0;
    object_wire[SOPHIA_SF_SHORTCUTS][40]++; /* Wrong connection in body. */
    assert(fetch(&r, SOPHIA_SF_SHORTCUTS, 7, 100) == SOPHIA_9P_INVALID);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    snapshot(&r, SOPHIA_SF_SHORTCUTS, 8, 101);
    consume_publication(&r, SOPHIA_SF_SHORTCUTS, 8, 101);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    assert(!fetch(&r, SOPHIA_SF_SHORTCUTS, 8, 101));
    rig_ack(&r);
    assert(r.p.acked == 4);
    /* Fetch-before-consume also releases exactly its matching announcement. */
    snapshot(&r, SOPHIA_SF_DESCRIPTORS, 9, 102);
    peer_published(&r.p, SOPHIA_SF_DESCRIPTORS, 9, 102);
    rig_run(&r, 8);
    assert(!fetch(&r, SOPHIA_SF_DESCRIPTORS, 9, 102));
    assert(!sophia_ss_consume(&r.s));
    rig_ack(&r);
    assert(r.p.acked == 5);
    rig_close(&r);
}
static void disclosure(void)
{
    struct rig r;
    struct sophia_sf_record v;
    uint64_t ticket;
    ready(&r, 8, 1);
    for (unsigned kind = 1; kind <= 7; kind++) {
        if (kind == SOPHIA_SF_DESCRIPTORS)
            continue;
        assert(sophia_ss_object(&r.s, (uint16_t)kind, 1, 1) == SOPHIA_9P_ARGUMENT);
        assert(!r.p.object_walks[kind]);
    }
    v = rig_request(1);
    assert(sophia_sf_client_submit(&r.s.files, &v) == SOPHIA_9P_ARGUMENT);
    ticket = 1234;
    assert(sophia_ss_submit(&r.s, &v, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(ticket == 1234 && !r.s.queued && r.s.next_ticket == 1);
    struct sophia_sf_resource_begin resource = {0};
    assert(sophia_ss_upload_begin(&r.s, resource, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(r.p.submits == 1 && r.s.files.next_submission == 2);
    v = candidate(1);
    v.header.kind = SOPHIA_SF_REFERENCE_CANDIDATE;
    /* Unselected record family is rejected before encoding/admission. */
    assert(sophia_ss_submit(&r.s, &v, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(r.s.next_ticket == 1 && !r.s.queued);
    rig_close(&r);
    for (unsigned kind = 1; kind <= 7; kind++) {
        if (kind == SOPHIA_SF_DESCRIPTORS)
            continue;
        ready(&r, 8, 1);
        peer_published(&r.p, (uint16_t)kind, 1, 100);
        rig_run(&r, 8);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && r.p.acked == 2);
        rig_close(&r);
    }
    /* Catalog-only and indicators without content are valid metadata roles. */
    ready(&r, 8, 1 | (1u << 5) | (1u << 9));
    for (unsigned kind = SOPHIA_SF_CATALOG; kind <= SOPHIA_SF_INDICATORS; kind++) {
        snapshot(&r, kind, 7, 100 + kind);
        consume_publication(&r, kind, 7, 100 + kind);
        if (kind == SOPHIA_SF_CATALOG) {
            object_wire[kind][58] = 1; /* Persistent identities are a different role. */
            assert(fetch(&r, (uint16_t)kind, 7, 100 + kind) == SOPHIA_9P_INVALID);
            assert(sophia_ss_ack_limit(&r.s) == 2);
            object_wire[kind][58] = 0;
        }
        assert(!fetch(&r, (uint16_t)kind, 7, 100 + kind));
        rig_ack(&r);
    }
    assert(!r.s.files.have_limits && !r.p.object_walks[1] && !r.p.object_walks[2]);
    rig_close(&r);
}
static void optional_events(void)
{
    const uint64_t needed[] = {1 | 8 | 16, 1 | 32 | 64, 1 | 512 | 1024};
    for (unsigned family = 0; family < 3; family++) {
        for (int granted = 0; granted <= 1; granted++) {
            struct rig r;
            struct sophia_sf_record v = {0};
            const struct sophia_sf_record *event;
            ready(&r, 8, granted ? needed[family] : 1);
            if (family == 0) {
                v.header.kind = SOPHIA_SF_REFERENCE_REQUEST;
                v.value.reference_request =
                    (struct sophia_sf_reference_request){1, PEER_EPOCH, 2, 3, 4, 5, 0, 1};
            } else if (family == 1) {
                v.header.kind = SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST;
                v.value.descriptor_launcher_request =
                    (struct sophia_sf_descriptor_launcher_request){
                        {1, PEER_EPOCH, 2, 3, 4, 5, 0, 1}, {(const uint8_t *)"query", 5}};
            } else {
                v.header.kind = SOPHIA_SF_INDICATOR_ACTIVATION_OUTCOME;
                v.value.indicator_activation_outcome =
                    (struct sophia_sf_indicator_activation_outcome){1, PEER_EPOCH, 2, 3, 1, 0};
            }
            peer_record(&r.p, &v);
            rig_run(&r, 8);
            if (granted) {
                assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
                assert(!sophia_ss_event(&r.s, &event) && event->header.kind == v.header.kind);
                assert(!sophia_ss_consume(&r.s));
                rig_ack(&r);
                assert(r.p.acked == 3);
            } else {
                assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && r.s.last_consumed == 2 &&
                       r.p.acked == 2);
            }
            rig_close(&r);
        }
    }
    struct rig r;
    ready(&r, 8, 1);
    peer_app_event(&r.p); /* No content event without a content grant. */
    rig_run(&r, 8);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && r.p.acked == 2);
    rig_close(&r);
}
static void custody_cannot_cross_undisclosed_events(void)
{
    for (int undisclosed = 0; undisclosed <= 1; undisclosed++) {
        struct rig r;
        struct sophia_sf_record v = candidate(1);
        struct sophia_sf_record event = {{SOPHIA_SF_DESCRIPTOR_OUTCOME, PEER_EPOCH, 0, 0},
                                         .value.descriptor_outcome = {1, PEER_EPOCH, 1, 0, 1}};
        uint64_t ticket;
        ready(&r, 8, 1);
        r.p.policy = P_SILENT;
        assert(!sophia_ss_submit(&r.s, &v, 1, &ticket));
        rig_run(&r, 16);
        assert(r.p.submits == 2 && rig_outcome(&r, ticket) == SOPHIA_SS_IN_FLIGHT);
        peer_record(&r.p, &event);
        if (undisclosed)
            peer_published(&r.p, SOPHIA_SF_TABS, 7, 100);
        peer_submitted(&r.p, 2, SOPHIA_SF_DESCRIPTOR_CANDIDATE);
        rig_run(&r, 8);
        assert(r.s.files.event_ready && r.s.files.event_used > 68 &&
               rig_outcome(&r, ticket) == SOPHIA_SS_IN_FLIGHT);
        peer_close(&r.p);
        rig_run(&r, 8);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
        assert(rig_outcome(&r, ticket) ==
               (undisclosed ? SOPHIA_SS_UNKNOWN_DISCONNECTED : SOPHIA_SS_SUBMITTED));
        rig_close(&r);
    }
}
int main(void)
{
    readiness();
    bad_offers();
    wrong_role_and_welcome();
    selected_feeds_and_holds();
    refused_and_superseded_fetches();
    disclosure();
    optional_events();
    custody_cannot_cross_undisclosed_events();
    puts("descriptor session: explicit role, metadata/combined readiness, disclosure and fetch/ack "
         "holds passed");
    return 0;
}
