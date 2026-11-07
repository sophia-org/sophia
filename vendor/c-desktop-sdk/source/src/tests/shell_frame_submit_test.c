#include "shell_frames_peer.h"

static struct sophia_sf_record commit_record(void)
{
    FILE *f = fopen("spec/golden/sophia-frame-transactions-v1.tsv", "r");
    char line[17000];
    uint8_t bytes[8192];
    struct sophia_sft_record frame;
    struct sophia_sf_record request = {0};
    assert(f);
    while (fgets(line, sizeof line, f)) {
        char *hex;
        size_t n;
        if (strncmp(line, "commit-copy-retire\t", 19)) continue;
        hex = strchr(line, '\t'); assert(hex);
        hex = strchr(hex + 1, '\t'); assert(hex); ++hex;
        n = strcspn(hex, "\n") / 2; assert(n <= sizeof bytes);
        for (size_t i = 0; i < n; ++i) {
            unsigned v;
            assert(sscanf(hex + 2*i, "%2x", &v) == 1);
            bytes[i] = (uint8_t)v;
        }
        assert(!sophia_sft_decode(bytes, n, SOPHIA_SFT_REQUEST, 13, 8192, &frame));
        fclose(f);
        frame.header.epoch = frame.value.commit.candidate.grant_connection_epoch = PEER_EPOCH;
        frame.value.commit.image.identity.connection_epoch = PEER_EPOCH;
        request.header.kind = SOPHIA_SFT_COMMIT;
        request.value.frame = frame.value;
        return request;
    }
    abort();
}

static void connected(struct rig *r, unsigned direct, uint32_t lookup, uint32_t open, uint32_t iounit)
{
    start_frame_options(r, &welcome, &config, direct, lookup, open, iounit);
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_READY);
    assert(!sophia_ss_ack(&r->s));
    rig_run(r, 64);
}

static uint64_t issue(struct rig *r)
{
    struct sophia_sf_record request = commit_record();
    uint8_t expected[8192];
    size_t written;
    uint64_t ticket;
    unsigned tx = r->p.tx_writes, submits = r->p.submits;
    r->p.policy = P_HOLD;
    assert(!sophia_ss_submit(&r->s, &request, 1, &ticket));
    rig_run(r, 32);
    assert(r->p.frame_writes == 1 && r->p.tx_writes == tx && r->p.submits == submits);
    request.header.epoch = PEER_EPOCH;
    request.header.submission = peer_get(r->p.last_submit + 8, 8);
    assert(!sophia_sf_encode_revision(expected, sizeof expected, 13, &request, &written));
    assert(written == r->p.staged_used && !memcmp(expected, r->p.staged, written));
    assert(rig_outcome(r, ticket) == SOPHIA_SS_IN_FLIGHT);
    return ticket;
}

static void replies_and_retry(void)
{
    for (unsigned journal_first = 0; journal_first < 2; ++journal_first) {
        struct rig r;
        uint64_t ticket, submission;
        uint8_t original[8192];
        size_t bytes;
        connected(&r, 1, 0, 0, 0);
        ticket = issue(&r);
        bytes = r.p.staged_used;
        memcpy(original, r.p.staged, bytes);
        submission = peer_get(r.p.last_submit + 8, 8);
        peer_answer_submit(&r.p, 11);
        rig_run(&r, 8);
        assert(r.p.frame_writes == 1);
        r.now += 1000;
        rig_run(&r, 16);
        assert(r.p.frame_writes == 2 && r.p.staged_used == bytes && !memcmp(original, r.p.staged, bytes));
        if (journal_first) {
            peer_submitted(&r.p, submission, SOPHIA_SFT_COMMIT);
            rig_run(&r, 8);
            assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED);
            peer_answer_submit(&r.p, 0);
        } else {
            peer_answer_submit(&r.p, 0);
            rig_run(&r, 8);
            assert(rig_outcome(&r, ticket) == SOPHIA_SS_IN_FLIGHT);
            peer_submitted(&r.p, submission, SOPHIA_SFT_COMMIT);
        }
        rig_run(&r, 16);
        assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED);
        assert(!r.s.files.terminal);
        rig_ack(&r);
        assert(r.s.files.submit_stage == 0);
        rig_close(&r);
    }
}

static void fallback_and_discovery(void)
{
    for (unsigned small = 0; small < 2; ++small) {
        struct rig r;
        struct sophia_sf_record request = commit_record();
        uint64_t ticket;
        unsigned tx, submits;
        connected(&r, small ? 1 : 2, 0, 0, small ? 32 : 0);
        tx = r.p.tx_writes; submits = r.p.submits;
        r.p.policy = P_HOLD;
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        rig_run(&r, 32);
        assert(r.p.frame_writes == 0 && r.p.tx_writes == tx + 1 && r.p.submits == submits + 1);
        rig_close(&r);
    }
    {
        struct rig r;
        struct sophia_ss_config cfg = config;
        struct sophia_sf_record request = commit_record();
        uint64_t ticket;
        unsigned tx, submits;
        cfg.msize = 4096;
        request.value.frame.commit.candidate.placement_count = 32;
        request.value.frame.commit.candidate.target_count = 64;
        for (unsigned i = 1; i < 32; ++i)
            request.value.frame.commit.candidate.placements[i] = request.value.frame.commit.candidate.placements[0];
        for (unsigned i = 0; i < 64; ++i) {
            struct sophia_sf_content_target *target = &request.value.frame.commit.candidate.targets[i];
            target->surface_index = 0; target->action_kind = 1;
            target->target_id = i + 1; target->target_generation = 1; target->action_id = 1;
            target->bounds_width = target->bounds_height = 1;
        }
        start_frame_options(&r, &welcome, &cfg, 1, 0, 0, 0);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
        rig_ack(&r);
        tx = r.p.tx_writes; submits = r.p.submits;
        r.p.policy = P_HOLD;
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        rig_run(&r, 32);
        assert(!r.p.frame_writes && r.p.tx_writes > tx + 1 && r.p.submits == submits + 1);
        rig_close(&r);
    }
    for (unsigned open = 0; open < 2; ++open) {
        const uint32_t errors[] = {5, 13, 116};
        for (unsigned i = 0; i < 3; ++i) {
            struct rig r;
            connected(&r, 1, open ? 0 : errors[i], open ? errors[i] : 0, 0);
            assert(sophia_ss_state(&r.s) == (errors[i] == 116 ? SOPHIA_SS_STALE : SOPHIA_SS_FAILED) && r.s.files.remote_error == errors[i]);
            assert(!r.p.frame_writes);
            rig_close(&r);
        }
    }
}

static void refused_and_uncertain(void)
{
    const uint32_t errors[] = {22, 116, 114, 0};
    for (unsigned i = 0; i < 4; ++i) {
        struct rig r;
        uint64_t ticket;
        unsigned tx, submits;
        connected(&r, 1, 0, 0, 0);
        ticket = issue(&r);
        tx = r.p.tx_writes; submits = r.p.submits;
        if (errors[i]) peer_answer_submit(&r.p, errors[i]);
        else {
            r.p.submit_held = 0;
            peer_count(&r.p, r.p.submit_tag, (uint32_t)r.p.staged_used - 1);
        }
        rig_run(&r, 32);
        assert(rig_outcome(&r, ticket) == (i == 0 ? SOPHIA_SS_REFUSED : SOPHIA_SS_UNKNOWN_DISCONNECTED));
        assert(r.p.frame_writes == 1 && r.p.tx_writes == tx && r.p.submits == submits);
        rig_close(&r);
    }
}

static void disconnect_preserves_only_known_custody(void)
{
    for (unsigned accepted = 0; accepted < 2; ++accepted) {
        struct rig r;
        uint64_t ticket;
        connected(&r, 1, 0, 0, 0);
        ticket = issue(&r);
        if (accepted) {
            peer_submitted(&r.p, peer_get(r.p.last_submit + 8, 8), SOPHIA_SFT_COMMIT);
            rig_run(&r, 8);
            assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED);
        }
        peer_close(&r.p);
        rig_run(&r, 16);
        assert(rig_outcome(&r, ticket) == (accepted ? SOPHIA_SS_SUBMITTED : SOPHIA_SS_UNKNOWN_DISCONNECTED));
        rig_close(&r);
    }
}

int main(void)
{
    disconnect_preserves_only_known_custody();
    replies_and_retry();
    fallback_and_discovery();
    refused_and_uncertain();
    puts("frame-submit: complete writes, fallback, retry and custody refusals passed");
    return 0;
}
