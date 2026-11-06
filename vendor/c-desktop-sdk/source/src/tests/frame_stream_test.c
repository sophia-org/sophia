#include "shell_frames_peer.h"
#include "../sophia_shell_frame_stream.h"

static struct sophia_sf_record commit_record(uint64_t generation)
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
        if (strncmp(line, "commit-retained\t", 16)) continue;
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
    v.value.commit.candidate.transaction = 20 + generation;
    v.value.commit.candidate.grant_connection_epoch = PEER_EPOCH;
    v.value.commit.candidate.grant_content_epoch = 1;
    v.value.commit.candidate.output_id = v.value.commit.candidate.output_generation = 1;
    v.value.commit.candidate.facts_generation = v.value.commit.candidate.pacing_permit = 1;
    v.value.commit.candidate.surfaces[0].allocation_id = v.value.commit.candidate.surfaces[0].allocation_generation = 1;
    v.value.commit.candidate.candidate_generation = generation;
    r.header.kind = SOPHIA_SFT_COMMIT; r.value.frame = v.value;
    return r;
}
static int deliver(struct rig *r, struct sophia_ss_frame_stream *f, struct sophia_sf_record record)
{
    const struct sophia_sf_record *event;
    int status;
    peer_record(&r->p, &record);
    rig_run(r, 16);
    assert(!sophia_ss_event(&r->s, &event));
    status = sophia_ss_frame_observe(f, event);
    assert(!sophia_ss_consume(&r->s)); rig_ack(r);
    return status;
}
static struct sophia_sf_record opened(void)
{
    struct sophia_sf_record r = open_record();
    r.header.kind = SOPHIA_SFT_OPENED; r.value.frame.stream.ttl_ms = 100;
    return r;
}
static struct sophia_sf_record closed(uint64_t transaction)
{
    struct sophia_sf_record r = open_record();
    r.header.kind = SOPHIA_SFT_CLOSED;
    r.value.frame.stream.transaction = transaction; r.value.frame.stream.reason = 1;
    return r;
}
static struct sophia_sf_record outcome(uint64_t generation, uint16_t kind, uint64_t credit)
{
    struct sophia_sf_record r = {0};
    struct sophia_sft_outcome *v = &r.value.frame.outcome;
    r.header.kind = SOPHIA_SFT_OUTCOME;
    v->candidate = (struct sophia_sf_candidate_outcome){20+generation, PEER_EPOCH, 1, generation, 1, 1,
                                                     kind, kind <= 2 ? 0 : 1, kind == 2, 0, 0};
    v->stream_id = v->allocation_id = v->allocation_generation = v->facts_generation = 1;
    v->credit_expires_msec = credit;
    return r;
}
static struct sophia_sf_record refused(uint16_t kind, uint64_t transaction, uint64_t generation)
{
    struct sophia_sf_record r = {0}; r.header.kind = SOPHIA_SFT_REFUSED;
    r.value.frame.refused = (struct sophia_sft_refused){transaction, PEER_EPOCH, 1, 1, 1, 1, generation, kind, 1};
    return r;
}
static void accepted(struct rig *r, uint64_t ticket)
{
    enum sophia_ss_outcome result;
    uint32_t error;
    rig_run(r, 32); rig_ack(r);
    assert(!sophia_ss_outcome(&r->s, ticket, &result, &error) && result == SOPHIA_SS_SUBMITTED);
}
static void owner_ready(struct rig *r, struct sophia_ss_frame_stream *f)
{
    struct sophia_sf_record open = open_record();
    uint64_t ticket;
    memset(f, 0, sizeof(*f)); ready(r);
    assert(!sophia_ss_frame_open(&r->s, f, 10, &open.value.frame.stream.key, &ticket));
    assert(f->state == SOPHIA_SS_FRAME_OPENING && !sophia_ss_frame_has_credit(f));
    accepted(r, ticket); assert(!sophia_ss_frame_has_credit(f));
    assert(!deliver(r, f, opened())); assert(sophia_ss_frame_has_credit(f));
}
static void submit_one(struct rig *r, struct sophia_ss_frame_stream *f)
{
    struct sophia_sf_record record = commit_record(1);
    uint64_t ticket;
    assert(!sophia_ss_frame_submit(&r->s, f, &record, &ticket)); accepted(r, ticket);
    assert(!sophia_ss_frame_has_credit(f));
}
static void credit_once(void)
{
    struct rig r; struct sophia_ss_frame_stream f;
    struct sophia_sf_record record; uint64_t ticket;
    owner_ready(&r, &f); submit_one(&r, &f);
    record = commit_record(2); assert(sophia_ss_frame_submit(&r.s, &f, &record, &ticket));
    assert(!deliver(&r, &f, outcome(1, 1, 0))); assert(!sophia_ss_frame_has_credit(&f));
    assert(!deliver(&r, &f, outcome(1, 2, 123))); assert(sophia_ss_frame_has_credit(&f));
    record = commit_record(1); assert(sophia_ss_frame_submit(&r.s, &f, &record, &ticket));
    assert(deliver(&r, &f, outcome(1, 2, 123)) == SOPHIA_9P_INVALID);
    assert(f.state == SOPHIA_SS_FRAME_UNKNOWN && !sophia_ss_frame_has_credit(&f)); rig_close(&r);
}
static void drain_orders(void)
{
    for (unsigned close_first = 0; close_first < 2; ++close_first) {
        struct rig r; struct sophia_ss_frame_stream f; struct sophia_sft_key key;
        uint64_t ticket;
        owner_ready(&r, &f); submit_one(&r, &f); key = f.key; ++key.stream_id;
        assert(!sophia_ss_frame_close(&r.s, &f, 30, &ticket)); accepted(&r, ticket);
        assert(!deliver(&r, &f, close_first ? closed(30) : outcome(1, 3, 123)));
        assert(!sophia_ss_frame_drained(&f));
        assert(sophia_ss_frame_open(&r.s, &f, 31, &key, &ticket));
        assert(!deliver(&r, &f, close_first ? outcome(1, 3, 0) : closed(30)));
        assert(sophia_ss_frame_drained(&f));
        assert(sophia_ss_frame_open(&r.s, &f, 31, &f.key, &ticket));
        assert(!sophia_ss_frame_open(&r.s, &f, 31, &key, &ticket)); rig_close(&r);
    }
}
static void automatic_close_race(void)
{
    struct rig r; struct sophia_ss_frame_stream f; uint64_t ticket;
    owner_ready(&r, &f);
    assert(!sophia_ss_frame_close(&r.s, &f, 30, &ticket)); accepted(&r, ticket);
    assert(!deliver(&r, &f, closed(10))); assert(!sophia_ss_frame_drained(&f));
    assert(!deliver(&r, &f, refused(SOPHIA_SFT_CLOSE, 30, 0))); assert(sophia_ss_frame_drained(&f));
    rig_close(&r);
}
static void refused_frame_and_close(void)
{
    struct rig r; struct sophia_ss_frame_stream f; uint64_t ticket;
    struct sophia_sf_record record;
    owner_ready(&r, &f); submit_one(&r, &f);
    assert(!deliver(&r, &f, refused(SOPHIA_SFT_COMMIT, 21, 1))); assert(sophia_ss_frame_has_credit(&f));
    record = commit_record(1); assert(sophia_ss_frame_submit(&r.s, &f, &record, &ticket));
    record = commit_record(2); assert(!sophia_ss_frame_submit(&r.s, &f, &record, &ticket)); accepted(&r, ticket);
    assert(!deliver(&r, &f, outcome(2, 3, 0))); assert(f.state == SOPHIA_SS_FRAME_CLOSING);
    assert(!sophia_ss_frame_close(&r.s, &f, 30, &ticket)); accepted(&r, ticket);
    assert(!deliver(&r, &f, refused(SOPHIA_SFT_CLOSE, 30, 0))); assert(!sophia_ss_frame_has_credit(&f));
    assert(!sophia_ss_frame_close(&r.s, &f, 31, &ticket)); accepted(&r, ticket);
    assert(!deliver(&r, &f, closed(31))); assert(sophia_ss_frame_drained(&f)); rig_close(&r);
}
static void wrong_tuples(void)
{
    for (unsigned field = 0; field < 9; ++field) {
        struct rig r; struct sophia_ss_frame_stream f;
        struct sophia_sf_record record = outcome(1, 3, 123);
        struct sophia_sft_outcome *v = &record.value.frame.outcome;
        uint64_t *words[] = {&v->candidate.transaction, &v->candidate.grant_content_epoch,
            &v->candidate.candidate_generation, &v->candidate.output_id, &v->candidate.output_generation,
            &v->stream_id, &v->allocation_id, &v->allocation_generation, &v->facts_generation};
        owner_ready(&r, &f); submit_one(&r, &f); ++*words[field];
        assert(deliver(&r, &f, record) == SOPHIA_9P_INVALID);
        assert(f.state == SOPHIA_SS_FRAME_UNKNOWN && !sophia_ss_frame_has_credit(&f));
        assert(!sophia_ss_frame_drained(&f));
        assert(deliver(&r, &f, outcome(1, 3, 0)) == SOPHIA_9P_INVALID); rig_close(&r);
    }
}
static void queue_refusal(void)
{
    struct rig r; struct sophia_ss_frame_stream f;
    struct sophia_ss_reservation reservation;
    struct sophia_sf_record record = commit_record(1);
    uint64_t ticket = 99;
    owner_ready(&r, &f);
    assert(!sophia_ss_reserve(&r.s, 4, 8192, &reservation));
    assert(sophia_ss_frame_submit(&r.s, &f, &record, &ticket));
    assert(sophia_ss_frame_close(&r.s, &f, 30, &ticket));
    assert(ticket == 99 && sophia_ss_frame_has_credit(&f) && f.last_candidate == 0);
    assert(!sophia_ss_cancel(&r.s, &reservation));
    assert(!sophia_ss_frame_submit(&r.s, &f, &record, &ticket)); rig_close(&r);
}
static void opening_refusal_and_disconnect(void)
{
    struct rig r; struct sophia_ss_frame_stream f = {0};
    struct sophia_sf_record record = open_record(); uint64_t ticket;
    ready(&r);
    assert(!sophia_ss_frame_open(&r.s, &f, 10, &record.value.frame.stream.key, &ticket)); accepted(&r, ticket);
    assert(!deliver(&r, &f, refused(SOPHIA_SFT_OPEN, 10, 0))); assert(sophia_ss_frame_drained(&f));
    assert(!sophia_ss_frame_open(&r.s, &f, 11, &record.value.frame.stream.key, &ticket));
    sophia_ss_frame_disconnect(&f); assert(!sophia_ss_frame_drained(&f));
    { struct sophia_sf_record commit = commit_record(1);
      assert(sophia_ss_frame_submit(&r.s, &f, &commit, &ticket) == SOPHIA_9P_ARGUMENT); }
    ++record.value.frame.stream.key.stream_id;
    assert(sophia_ss_frame_open(&r.s, &f, 12, &record.value.frame.stream.key, &ticket)); rig_close(&r);
}
static void invalid_after_prepared(void)
{
    for (unsigned refusal = 0; refusal < 2; ++refusal) {
        struct rig r; struct sophia_ss_frame_stream f;
        owner_ready(&r, &f); submit_one(&r, &f);
        assert(!deliver(&r, &f, outcome(1, 1, 0)));
        assert(deliver(&r, &f, refusal ? refused(SOPHIA_SFT_COMMIT, 21, 1) : outcome(1, 1, 0)) == SOPHIA_9P_INVALID);
        assert(f.state == SOPHIA_SS_FRAME_UNKNOWN); rig_close(&r);
    }
}
static void local_binding_and_open_capacity(void)
{
    struct rig r; struct sophia_ss_frame_stream f;
    uint64_t ticket = 99;
    owner_ready(&r, &f);
    for (unsigned field = 0; field < 8; ++field) {
        struct sophia_sf_record record = commit_record(1);
        struct sophia_sf_candidate *c = &record.value.frame.commit.candidate;
        uint64_t *words[] = {&c->grant_content_epoch, &c->output_id, &c->output_generation,
            &c->facts_generation, &c->pacing_permit, &c->surfaces[0].allocation_id,
            &c->surfaces[0].allocation_generation};
        if (field == 7) c->surface_count = 0; else ++*words[field];
        assert(sophia_ss_frame_submit(&r.s, &f, &record, &ticket));
        assert(sophia_ss_frame_has_credit(&f) && ticket == 99);
    }
    assert(sophia_ss_frame_close(&r.s, &f, 10, &ticket));
    submit_one(&r, &f); rig_close(&r);
    {
        struct sophia_ss_reservation reserved;
        struct sophia_sft_key key = open_record().value.frame.stream.key;
        memset(&f, 0, sizeof f); ready(&r);
        assert(!sophia_ss_reserve(&r.s, 4, 8192, &reserved));
        assert(sophia_ss_frame_open(&r.s, &f, 10, &key, &ticket));
        assert(sophia_ss_frame_drained(&f) && !f.key.connection_epoch);
        assert(!sophia_ss_cancel(&r.s, &reserved));
        assert(!sophia_ss_frame_open(&r.s, &f, 10, &key, &ticket)); accepted(&r, ticket);
        assert(!deliver(&r, &f, opened())); assert(!deliver(&r, &f, closed(10)));
        ++key.stream_id; ++key.content_epoch;
        assert(sophia_ss_frame_open(&r.s, &f, 11, &key, &ticket));
        assert(sophia_ss_frame_drained(&f)); rig_close(&r);
    }
}
static void no_credit_after_closed(void)
{
    struct rig r; struct sophia_ss_frame_stream f;
    owner_ready(&r, &f); submit_one(&r, &f);
    assert(!deliver(&r, &f, closed(10)));
    assert(deliver(&r, &f, outcome(1, 3, 100)) == SOPHIA_9P_INVALID);
    assert(!sophia_ss_frame_has_credit(&f) && !sophia_ss_frame_drained(&f)); rig_close(&r);
    owner_ready(&r, &f);
    assert(deliver(&r, &f, opened()) == SOPHIA_9P_INVALID);
    assert(f.state == SOPHIA_SS_FRAME_UNKNOWN); rig_close(&r);
}
static void terminal_only_profile(void)
{
    for (unsigned prepared = 0; prepared < 2; ++prepared) {
        struct rig r; struct sophia_ss_frame_stream f = {0}; uint64_t ticket;
        struct sophia_sf_negotiated v = welcome;
        struct sophia_ss_config cfg = config;
        cfg.offer.required_capabilities |= SOPHIA_SFT_TERMINAL_ONLY_CAPABILITY;
        v.capabilities |= SOPHIA_SFT_TERMINAL_ONLY_CAPABILITY;
        start_as(&r, &v, &cfg);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
        rig_ack(&r);
        struct sophia_sf_record request = open_record();
        assert(!sophia_ss_frame_open(&r.s, &f, 10, &request.value.frame.stream.key, &ticket));
        accepted(&r, ticket);
        assert(!deliver(&r, &f, opened()));
        submit_one(&r, &f);
        if (prepared) {
            const struct sophia_sf_record *event;
            struct sophia_sf_record record = outcome(1, 1, 0);
            peer_record(&r.p, &record);
            rig_run(&r, 16);
            assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED);
            assert(sophia_ss_event(&r.s, &event) == SOPHIA_9P_INVALID);
            assert(!sophia_ss_frame_has_credit(&f));
        } else {
            assert(!deliver(&r, &f, outcome(1, 2, 123)));
            assert(sophia_ss_frame_has_credit(&f));
        }
        rig_close(&r);
    }
}

int main(void)
{
    terminal_only_profile();
    credit_once(); drain_orders(); automatic_close_race(); refused_frame_and_close(); wrong_tuples();
    queue_refusal(); opening_refusal_and_disconnect(); invalid_after_prepared();
    local_binding_and_open_capacity(); no_credit_after_closed();
    puts("SFT stream: one credit, exact terminals, close races, refusal and unknown custody OK");
    return 0;
}
