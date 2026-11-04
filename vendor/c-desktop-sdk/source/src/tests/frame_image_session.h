/* Independent SFT/source join through the existing 9P and image-channel rig. */
#include "../sophia_shell_frame_stream.h"
#include "../sophia_shell_frames.h"

static struct sophia_sf_record image_frame_record(void)
{
    FILE *file = fopen("spec/golden/sophia-frame-transactions-v1.tsv", "r");
    char line[17000];
    struct sophia_sft_record v;
    struct sophia_sf_record r = {0};
    int found = 0;
    assert(file);
    while (fgets(line, sizeof line, file)) {
        uint8_t bytes[8192];
        char *hex;
        size_t n, i;
        if (strncmp(line, "commit-copy\t", 12)) continue;
        hex = strchr(line, '\t'); assert(hex);
        hex = strchr(hex + 1, '\t'); assert(hex); ++hex;
        n = strcspn(hex, "\n") / 2; assert(n <= sizeof bytes);
        for (i = 0; i < n; ++i) {
            unsigned b;
            assert(sscanf(hex + 2*i, "%2x", &b) == 1); bytes[i] = (uint8_t)b;
        }
        assert(!sophia_sft_decode(bytes, n, SOPHIA_SFT_REQUEST, 13, 8192, &v));
        found = 1; break;
    }
    fclose(file); assert(found);
    v.value.commit.candidate.transaction = 21;
    v.value.commit.candidate.grant_connection_epoch = PEER_EPOCH;
    v.value.commit.candidate.grant_content_epoch = 3;
    v.value.commit.candidate.candidate_generation = 1;
    v.value.commit.candidate.placements[0].resource_generation = 1;
    v.value.commit.has_image = v.value.commit.has_retire = 0;
    r.header.kind = SOPHIA_SFT_COMMIT; r.value.frame = v.value;
    return r;
}
static struct sophia_ss_ring_image frame_image(const struct sophia_sf_record *r)
{
    const struct sophia_sf_candidate *c = &r->value.frame.commit.candidate;
    struct sophia_ss_ring_image m = ring_image(c->placements[0].resource_id, 2, 1);
    m.resource_generation = c->placements[0].resource_generation;
    m.allocation_id = c->surfaces[0].allocation_id;
    m.allocation_generation = c->surfaces[0].allocation_generation;
    m.output_id = c->output_id; m.output_generation = c->output_generation;
    m.topology_generation = c->facts_generation;
    return m;
}
static int frame_image_deliver(struct image_rig *t, struct sophia_ss_frame_stream *f, struct sophia_sf_record r)
{
    const struct sophia_sf_record *event;
    int status;
    peer_record(&t->r.p, &r); ring_steps(t, 8);
    assert(!sophia_ss_event(&t->r.s, &event));
    status = sophia_ss_frame_observe(f, event);
    assert(!sophia_ss_consume(&t->r.s)); rig_ack(&t->r);
    return status;
}
static int frame_image_ready(struct image_rig *t, struct sophia_ss_frame_stream *f, const struct sophia_sf_record *r)
{
    const struct sophia_sf_candidate *c = &r->value.frame.commit.candidate;
    struct sophia_sf_record opened = {0};
    uint64_t ticket;
    int fd;
    ring_start_revision(t, SOPHIA_SF_BAR, 2, 0, 13);
    fd = ring_ready(t); ack_all(t);
    opened.header.kind = SOPHIA_SFT_OPENED;
    opened.value.frame.stream.transaction = 10;
    opened.value.frame.stream.ttl_ms = 100;
    opened.value.frame.stream.key = (struct sophia_sft_key){PEER_EPOCH, 3, c->output_id, c->output_generation,
        c->surfaces[0].allocation_id, c->surfaces[0].allocation_generation, c->facts_generation, c->pacing_permit};
    assert(!sophia_ss_frame_open(&t->r.s, f, 10, &opened.value.frame.stream.key, &ticket));
    assert(settle_ticket(t, ticket) == SOPHIA_SS_SUBMITTED);
    assert(!frame_image_deliver(t, f, opened));
    return fd;
}
static struct sophia_sf_record frame_image_refused(const struct sophia_ss_frame_stream *f, uint64_t generation)
{
    struct sophia_sf_record r = {0};
    struct sophia_sft_refused *v = &r.value.frame.refused;
    r.header.kind = SOPHIA_SFT_REFUSED;
    v->transaction = 21; v->connection_epoch = f->key.connection_epoch; v->content_epoch = f->key.content_epoch;
    v->output_id = f->key.output_id; v->output_generation = f->key.output_generation;
    v->stream_id = f->key.stream_id; v->candidate_generation = generation;
    v->request_kind = SOPHIA_SFT_COMMIT; v->reason = 1;
    return r;
}
static struct sophia_sf_record frame_image_outcome(const struct sophia_ss_frame_stream *f, uint16_t kind)
{
    struct sophia_sf_record r = {0};
    struct sophia_sft_outcome *v = &r.value.frame.outcome;
    r.header.kind = SOPHIA_SFT_OUTCOME;
    v->candidate = (struct sophia_sf_candidate_outcome){21, f->key.connection_epoch, f->key.content_epoch, 1,
        f->key.output_id, f->key.output_generation, kind, 0, kind == 2, 0, 0};
    v->stream_id = f->key.stream_id; v->allocation_id = f->key.allocation_id;
    v->allocation_generation = f->key.allocation_generation; v->facts_generation = f->key.facts_generation;
    v->credit_expires_msec = kind == 2 ? 100 : 0;
    return r;
}
static void frame_image_session(void)
{
    unsigned test;
    for (test = 0; test < 5; test++) {
        struct image_rig t;
        struct sophia_ss_frame_stream f = {0};
        struct sophia_sf_record r = image_frame_record(), before;
        struct sophia_ss_ring_image m = frame_image(&r);
        uint64_t ticket = 0, offer = 0;
        int fd = frame_image_ready(&t, &f, &r);
        before = r;
        memset(sophia_ss_image_ring_pixels(&t.img, 0), 7, 8);
        if (test == 0) {
            struct sophia_ss_reservation reserved;
            assert(!sophia_ss_reserve(&t.r.s, t.r.s.slots, t.r.s.queue_capacity, &reserved));
            assert(sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &offer) == SOPHIA_9P_BUSY);
            assert(!memcmp(&r, &before, sizeof r));
            assert(sophia_ss_frame_has_credit(&f) && ticket == 0 && offer == 0);
            assert(t.img.ring.generation[0] == 0 && t.img.ring.next_offer == 1);
            assert(!sophia_ss_cancel(&t.r.s, &reserved));
        }
        assert(!sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &offer));
        assert(offer == 1 && !memcmp(&r, &before, sizeof r) && !sophia_ss_frame_has_credit(&f));
        assert(settle_ticket(&t, ticket) == SOPHIA_SS_SUBMITTED);
        assert(t.r.p.ring_writes == 1); /* Registration only; no legacy Offer. */
        assert(ring_info(&t, offer).offer.slot_generation == 2);
        assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
        if (test == 0) {
            assert(!frame_image_deliver(&t, &f, frame_image_refused(&f, 1)));
            assert(sophia_ss_frame_has_credit(&f) && ring_info(&t, offer).state == SOPHIA_SS_SLOT_REFUSED);
            assert(!sophia_ss_image_ring_forget(&t.img, offer));
            r.value.frame.commit.candidate.transaction = 22;
            r.value.frame.commit.candidate.candidate_generation = 2;
            assert(!sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &offer));
            assert(offer == 2 && ring_info(&t, offer).offer.slot_generation == 4);
        } else if (test == 1) {
            assert(!frame_image_deliver(&t, &f, frame_image_outcome(&f, 1)));
            assert(!sophia_ss_frame_has_credit(&f));
            assert(!frame_image_deliver(&t, &f, frame_image_outcome(&f, 2)));
            assert(sophia_ss_frame_has_credit(&f) && !sophia_ss_image_ring_pixels(&t.img, 0));
            peer_status(&t.r.p, offer, m.resource_id, 2, 0); ring_steps(&t, 8); assert(app_events(&t, NULL, 0) == 1); ack_all(&t);
            assert(!sophia_ss_image_ring_pixels(&t.img, 0));
            ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, m.resource_id, 0, 0);
            ring_steps(&t, 8);
            assert(ring_info(&t, offer).adoptable);
        } else {
            int status;
            if (test == 3) { peer_status(&t.r.p, offer, m.resource_id, 2, 0); ring_steps(&t, 8); assert(app_events(&t, NULL, 0) == 1); ack_all(&t); }
            if (test == 4) assert(!frame_image_deliver(&t, &f, frame_image_outcome(&f, 1)));
            status = frame_image_deliver(&t, &f, frame_image_refused(&f, test == 2 ? 2 : 1));
            if (test == 2 || test == 4) assert(status == SOPHIA_9P_INVALID);
            assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_UNKNOWN);
            assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_FAILED);
        }
        ring_close(&t, fd);
    }
}

static void frame_image_transport_failure(void)
{
    unsigned closed;
    for (closed = 0; closed < 2; ++closed) {
        struct image_rig t;
        struct sophia_ss_frame_stream f = {0};
        struct sophia_sf_record r = image_frame_record();
        struct sophia_ss_ring_image m = frame_image(&r);
        uint64_t ticket, offer;
        int fd = frame_image_ready(&t, &f, &r);
        memset(sophia_ss_image_ring_pixels(&t.img, 0), 7, 8);
        t.r.p.policy = closed ? P_HOLD : P_ERROR;
        t.r.p.error = 22;
        assert(!sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &offer));
        ring_steps(&t, 16);
        if (closed) {
            assert(t.r.p.submit_held);
            peer_close(&t.r.p); ring_steps(&t, 8);
            assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_UNKNOWN);
        } else {
            assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_REFUSED);
            assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_REFUSED);
        }
        assert(!sophia_ss_frame_has_credit(&f));
        ring_close(&t, fd);
    }
}

static void frame_image_two_slots(void)
{
    struct image_rig t;
    struct sophia_ss_frame_stream f = {0};
    struct sophia_sf_record r = image_frame_record();
    struct sophia_ss_ring_image m = frame_image(&r);
    uint64_t ticket, offer, first = m.resource_id;
    unsigned generation, kind;
    int fd = frame_image_ready(&t, &f, &r);
    for (generation = 1; generation <= 2; ++generation) {
        m.resource_id = first + generation - 1;
        r.value.frame.commit.candidate.transaction = 20 + generation;
        r.value.frame.commit.candidate.candidate_generation = generation;
        r.value.frame.commit.candidate.placements[0].resource_id = m.resource_id;
        memset(sophia_ss_image_ring_pixels(&t.img, generation - 1), 7, 8);
        assert(!sophia_ss_frame_submit_ring(&t.img, &f, generation - 1, &m, &r, t.r.now, &ticket, &offer));
        assert(offer == generation);
        assert(settle_ticket(&t, ticket) == SOPHIA_SS_SUBMITTED);
        for (kind = 1; kind <= 2; ++kind) {
            struct sophia_sf_record outcome = frame_image_outcome(&f, (uint16_t)kind);
            outcome.value.frame.outcome.candidate.transaction = 20 + generation;
            outcome.value.frame.outcome.candidate.candidate_generation = generation;
            assert(!frame_image_deliver(&t, &f, outcome));
        }
    }
    ++m.resource_id;
    r.value.frame.commit.candidate.transaction = 23;
    r.value.frame.commit.candidate.candidate_generation = 3;
    r.value.frame.commit.candidate.placements[0].resource_id = m.resource_id;
    assert(sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &offer));
    assert(sophia_ss_frame_has_credit(&f) && t.img.ring.next_offer == 3);
    assert(!sophia_ss_image_ring_pixels(&t.img, 0) && !sophia_ss_image_ring_pixels(&t.img, 1));
    peer_status(&t.r.p, 1, first, 2, 0); ring_steps(&t, 8);
    assert(app_events(&t, NULL, 0) == 1); ack_all(&t);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, 1, 0, 2, first, 0, 0); ring_steps(&t, 8);
    assert(!sophia_ss_image_ring_forget(&t.img, 1));
    memset(sophia_ss_image_ring_pixels(&t.img, 0), 7, 8);
    assert(!sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &offer));
    assert(offer == 3 && ring_info(&t, offer).offer.slot == 0);
    assert(!sophia_ss_image_ring_pixels(&t.img, 1));
    ring_close(&t, fd);
}
