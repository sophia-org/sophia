#include "shell_frames_peer.h"

static void whole_request_and_retained_event(void)
{
    struct rig r;
    struct sophia_sf_record request = open_record(), answer = open_record();
    struct sophia_sft_record decoded;
    const struct sophia_sf_record *e;
    enum sophia_ss_outcome outcome;
    struct sophia_ss_reservation held;
    uint64_t ticket;
    uint32_t error;
    ready(&r);
    assert(sophia_ss_record_bytes(&request) == 104);
    r.p.policy = P_HOLD;
    assert(!sophia_ss_reserve(&r.s, 1, 104, &held));
    assert(!sophia_ss_cancel(&r.s, &held));
    assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
    rig_run(&r, 32);
    assert(!sophia_ss_outcome(&r.s, ticket, &outcome, &error) && outcome == SOPHIA_SS_IN_FLIGHT);
    assert(!sophia_sft_decode(r.p.staged, r.p.staged_used, SOPHIA_SFT_REQUEST, 13, 8192, &decoded));
    assert(decoded.header.kind == SOPHIA_SFT_OPEN && decoded.value.stream.transaction == 10);
    peer_answer_submit(&r.p, 0);
    peer_submitted(&r.p, decoded.header.submission, SOPHIA_SFT_OPEN);
    r.p.policy = P_ACCEPT;
    rig_run(&r, 16);
    assert(!sophia_ss_outcome(&r.s, ticket, &outcome, &error) && outcome == SOPHIA_SS_SUBMITTED);
    assert(sophia_ss_event(&r.s, &e) == SOPHIA_9P_AGAIN);
    answer.header.kind = SOPHIA_SFT_OPENED;
    answer.value.frame.stream.ttl_ms = 100;
    peer_record(&r.p, &answer);
    rig_run(&r, 8);
    assert(!sophia_ss_event(&r.s, &e));
    assert(e->header.kind == SOPHIA_SFT_OPENED && e->value.frame.stream.key.stream_id == 1);
    assert(sophia_ss_ack_limit(&r.s) == 3);
    assert(!sophia_ss_consume(&r.s));
    assert(sophia_ss_ack_limit(&r.s) == 4);
    rig_ack(&r);
    assert(r.p.acked == 4);
    assert(sophia_ss_event(&r.s, &e) == SOPHIA_9P_AGAIN);
    request.header.kind = SOPHIA_SFT_CLOSE;
    request.value.frame.stream.transaction = 11;
    assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
    rig_run(&r, 32);
    assert(!sophia_ss_outcome(&r.s, ticket, &outcome, &error) && outcome == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}

static void admission_refusals_preserve_the_queue(void)
{
    struct rig r;
    struct sophia_sf_record request = open_record();
    uint64_t ticket = 99, before;
    rig_ready(&r, &rig_config);
    assert(sophia_ss_submit(&r.s, &request, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    rig_close(&r);
    ready(&r);
    before = r.s.next_ticket;
    request.header.kind = SOPHIA_SFT_OPENED;
    assert(sophia_ss_submit(&r.s, &request, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    request = open_record(); request.value.frame.stream.key.connection_epoch++;
    assert(sophia_ss_submit(&r.s, &request, 1, &ticket) == SOPHIA_9P_INVALID);
    request = open_record(); r.s.files.limits.max_candidate_bytes = 64;
    assert(sophia_ss_submit(&r.s, &request, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(r.s.next_ticket == before && !r.s.queued && ticket == 99);
    rig_close(&r);
}

static void exact_negotiation(void)
{
    struct rig r;
    unsigned i;
    for (i = 0; i < 8; ++i) {
        struct sophia_sf_negotiated v = welcome;
        struct sophia_ss_config cfg = config;
        if (i == 0) v.selected_revision = 10;
        if (i == 1) v.capabilities &= ~SOPHIA_SFT_CAPABILITY;
        if (i == 2) v.capabilities &= ~SOPHIA_SF_CAP_OWNED_RING;
        if (i == 3) v.capabilities |= SOPHIA_SF_CAP_DMABUF_IMAGE;
        if (i == 4) v.limits_published = 0;
        if (i == 5) cfg.offer.required_capabilities &= ~SOPHIA_SFT_CAPABILITY;
        if (i == 6) v.capabilities |= UINT64_C(1) << 16;
        if (i == 7) v.capabilities |= UINT64_C(1) << 17;
        start_as(&r, &v, &cfg);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED);
        rig_close(&r);
    }
}

static void whole_retry_and_bounded_queue(void)
{
    struct rig r;
    struct sophia_sf_record request = open_record();
    uint8_t original[104], submit[24];
    uint64_t tickets[5], unchanged = 99;
    unsigned writes, submits;
    ready(&r);
    r.p.policy = P_HOLD;
    /* One item is handed off immediately; four more fill the local queue. */
    for (unsigned i = 0; i < 5; ++i) {
        request.value.frame.stream.transaction = 10 + i;
        assert(!sophia_ss_submit(&r.s, &request, 1, &tickets[i]));
    }
    assert(sophia_ss_submit(&r.s, &request, 1, &unchanged) == SOPHIA_9P_BUSY);
    assert(unchanged == 99);
    rig_run(&r, 32);
    assert(r.p.staged_used == sizeof original);
    memcpy(original, r.p.staged, sizeof original);
    memcpy(submit, r.p.last_submit, sizeof submit);
    writes = r.p.tx_writes; submits = r.p.submits;
    peer_answer_submit(&r.p, 11);
    rig_run(&r, 8);
    assert(r.p.submits == submits);
    r.now += 1000;
    rig_run(&r, 16);
    assert(r.p.submits == submits + 1 && r.p.tx_writes == writes);
    assert(!memcmp(original, r.p.staged, sizeof original));
    assert(!memcmp(submit, r.p.last_submit, sizeof submit));
    peer_answer_submit(&r.p, 0);
    peer_submitted(&r.p, peer_get(submit + 8, 8), SOPHIA_SFT_OPEN);
    r.p.policy = P_ACCEPT;
    for (unsigned i = 0; i < 5; ++i) {
        rig_run(&r, 32);
        rig_ack(&r);
    }
    for (unsigned i = 0; i < 5; ++i) {
        enum sophia_ss_outcome outcome;
        uint32_t error;
        assert(!sophia_ss_outcome(&r.s, tickets[i], &outcome, &error));
        assert(outcome == SOPHIA_SS_SUBMITTED);
    }
    rig_close(&r);
}

static void invalid_event_stops_before_later_valid_event(void)
{
    for (unsigned wrong_epoch = 0; wrong_epoch < 2; ++wrong_epoch) {
        struct rig r;
        struct sophia_sf_record answer = open_record();
        const struct sophia_sf_record *event;
        size_t offset;
        ready(&r);
        answer.header.kind = SOPHIA_SFT_OPENED;
        answer.value.frame.stream.ttl_ms = 100;
        offset = r.p.journal_used;
        peer_record(&r.p, &answer);
        if (wrong_epoch) peer_put(r.p.journal + offset + 8, PEER_EPOCH + 1, 8);
        else peer_put(r.p.journal + offset + 104, 2, 4);
        peer_record(&r.p, &answer);
        rig_run(&r, 16);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED);
        assert(sophia_ss_event(&r.s, &event) == SOPHIA_9P_INVALID);
        assert(sophia_ss_ack_limit(&r.s) == 2);
        rig_close(&r);
    }
}

static unsigned unhex(char c)
{
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return c <= '9' ? (unsigned)(c - '0') : (unsigned)(c - 'a' + 10);
}

static void complete_commits_use_the_same_lane(void)
{
    struct rig r;
    FILE *f = fopen("spec/golden/sophia-frame-transactions-v1.tsv", "r");
    char line[17000];
    unsigned count = 0;
    assert(f);
    ready(&r);
    while (fgets(line, sizeof line, f)) {
        uint8_t bytes[8192], encoded[8192];
        size_t n, i, written;
        uint64_t ticket;
        char *hex;
        struct sophia_sft_record frame;
        struct sophia_sf_record request = {0};
        if (strncmp(line, "commit-", 7)) continue;
        hex = strchr(line, '\t'); assert(hex);
        hex = strchr(hex + 1, '\t'); assert(hex); ++hex;
        n = strcspn(hex, "\n") / 2; assert(n <= sizeof bytes);
        for (i = 0; i < n; ++i) bytes[i] = (uint8_t)((unhex(hex[2*i]) << 4) | unhex(hex[2*i+1]));
        assert(!sophia_sft_decode(bytes, n, SOPHIA_SFT_REQUEST, 13, 8192, &frame));
        frame.header.epoch = frame.value.commit.candidate.grant_connection_epoch = PEER_EPOCH;
        if (frame.value.commit.has_image) frame.value.commit.image.identity.connection_epoch = PEER_EPOCH;
        request.header.kind = SOPHIA_SFT_COMMIT;
        request.value.frame = frame.value;
        r.p.policy = P_HOLD;
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        rig_run(&r, 32);
        frame.header.submission = peer_get(r.p.last_submit + 8, 8);
        assert(!sophia_sft_encode(encoded, sizeof encoded, 13, &frame, &written));
        assert(written == r.p.staged_used && !memcmp(encoded, r.p.staged, written));
        peer_answer_submit(&r.p, 0);
        peer_submitted(&r.p, frame.header.submission, SOPHIA_SFT_COMMIT);
        rig_run(&r, 16);
        rig_ack(&r);
        ++count;
    }
    assert(count == 4);
    fclose(f);
    rig_close(&r);
}

int main(void)
{
    for (unsigned i = 0; i < 4; ++i) {
        const enum sophia_sf_profile roles[] = {SOPHIA_SF_BAR, SOPHIA_SF_LAUNCHER, SOPHIA_SF_DOCK, SOPHIA_SF_DESCRIPTOR};
        const uint64_t caps[] = {0x48081, 0x489a0, 0x491a2, 0x48083};
        struct rig r;
        struct sophia_ss_config cfg = config;
        struct sophia_sf_negotiated v = welcome;
        cfg.profile = roles[i]; cfg.offer.required_capabilities = v.capabilities = caps[i];
        start_as(&r, &v, &cfg);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
        rig_close(&r);
    }
    whole_request_and_retained_event();
    admission_refusals_preserve_the_queue();
    exact_negotiation();
    whole_retry_and_bounded_queue();
    complete_commits_use_the_same_lane();
    invalid_event_stops_before_later_valid_event();
    puts("shell SFT session: whole custody, retained ACK, admission and negotiation OK");
    return 0;
}
