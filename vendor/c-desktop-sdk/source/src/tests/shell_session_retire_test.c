#include "shell_session_peer.h"

#define CAPS (UINT64_C(0x8081) | SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT)
static const struct sophia_ss_config config = {{12, 12, CAPS}, SOPHIA_SF_BAR, 4096, 4, 8192,
    NULL, 0};
static const struct sophia_sf_negotiated welcome = {12, PEER_EPOCH, CAPS | 2, 16, 128, 16, 1};

static void ready(struct rig *r)
{
    rig_start(r, &config);
    r->p.welcome_override = &welcome;
    rig_settle(r);
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_READY);
    rig_ack(r);
}

static struct sophia_sf_record candidate(void)
{
    struct sophia_sf_record r;
    assert(!sophia_sf_decode(vector_Candidate, sizeof vector_Candidate, &r));
    r.value.candidate.grant_connection_epoch = PEER_EPOCH;
    return r;
}

static void refused_admission_is_atomic(void)
{
    struct rig r;
    struct sophia_sf_record c = candidate();
    struct sophia_sf_candidate_retire intent = {91, 92, 93};
    struct sophia_ss_reservation reserve;
    uint64_t ticket = 77;
    size_t bytes = sophia_ss_retiring_candidate_bytes(&c, &intent);
    ready(&r);
    assert(bytes == sizeof vector_Candidate + 24);
    r.s.files.welcome.capabilities &= ~SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT;
    assert(sophia_ss_submit_retiring_candidate(&r.s, &c, &intent, &ticket) == SOPHIA_9P_ARGUMENT);
    r.s.files.welcome.capabilities |= SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT;
    r.s.files.welcome.selected_revision = 13;
    assert(sophia_ss_submit_retiring_candidate(&r.s, &c, &intent, &ticket) == SOPHIA_9P_ARGUMENT);
    r.s.files.welcome.selected_revision = 12;
    intent.transaction = c.value.candidate.transaction;
    assert(!sophia_ss_retiring_candidate_bytes(&c, &intent));
    assert(sophia_ss_submit_retiring_candidate(&r.s, &c, &intent, &ticket) == SOPHIA_9P_ARGUMENT);
    intent.transaction = 91;
    intent.resource_id = c.value.candidate.placements[0].resource_id;
    intent.resource_generation = c.value.candidate.placements[0].resource_generation;
    assert(!sophia_ss_retiring_candidate_bytes(&c, &intent));
    intent.resource_id = 92;
    intent.resource_generation = 93;
    assert(!sophia_ss_reserve(&r.s, 4, r.s.queue_capacity, &reserve));
    assert(sophia_ss_submit_retiring_candidate(&r.s, &c, &intent, &ticket) == SOPHIA_9P_BUSY);
    assert(ticket == 77 && r.s.next_ticket == 1 && !r.s.queued && !r.s.queue_used);
    assert(!sophia_ss_cancel(&r.s, &reserve));
    rig_close(&r);
}

static void retry_preserves_the_whole_intent(void)
{
    struct rig r;
    struct sophia_sf_record c = candidate(), decoded;
    struct sophia_sf_candidate_retire intent = {91, 92, 93}, observed;
    uint8_t staged[8192], submit[24];
    uint64_t ticket;
    size_t bytes;
    unsigned i, writes;
    int present;
    ready(&r);
    r.p.policy = P_ERROR;
    r.p.error = 11;
    r.p.once = 1;
    assert(!sophia_ss_submit_retiring_candidate(&r.s, &c, &intent, &ticket));
    for (i = 0; i < 16 && r.p.submits < 2; ++i)
        rig_step(&r);
    assert(r.p.submits == 2);
    bytes = (size_t)peer_get(r.p.staged, 4);
    assert(bytes == sizeof vector_Candidate + 24);
    memcpy(staged, r.p.staged, bytes);
    memcpy(submit, r.p.last_submit, sizeof submit);
    writes = r.p.tx_writes;
    assert(!sophia_sf_candidate_decode_with_retire(staged, bytes, 1, &decoded, &observed, &present));
    assert(present && observed.transaction == 91 && observed.resource_id == 92 &&
        observed.resource_generation == 93);
    assert(!sophia_ss_dispatch(&r.s, POLLIN, 65536, r.now));
    rig_run(&r, 8);
    assert(r.p.submits == 2 && r.s.files.submit_wait);
    r.now += 4;
    rig_run(&r, 4);
    assert(r.p.submits == 3 && r.p.tx_writes == writes);
    assert(!memcmp(staged, r.p.staged, bytes) && !memcmp(submit, r.p.last_submit, sizeof submit));
    assert(rig_until(&r, ticket, 16) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}

static void exact_welcome(void)
{
    const struct { uint16_t revision; uint64_t required, granted; int valid; } cases[] = {
        {12, CAPS, CAPS | 2, 1},
        {10, CAPS, CAPS | 2, 0}, {11, CAPS, CAPS | 2, 0}, {13, CAPS, CAPS | 2, 0},
        {12, CAPS, CAPS | 2 | (UINT64_C(1) << 9), 0},
        {12, CAPS, CAPS & ~SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT, 0},
        {12, CAPS & ~SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT, CAPS | 2, 0},
        {10, CAPS & ~SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT, CAPS | 2, 0}
    };
    size_t i;
    for (i = 0; i < sizeof cases / sizeof cases[0]; ++i) {
        struct rig r;
        struct sophia_ss_config offered = config;
        struct sophia_sf_negotiated selected = welcome;
        offered.offer.minimum_revision = 10;
        offered.offer.maximum_revision = 13;
        offered.offer.required_capabilities = cases[i].required;
        selected.selected_revision = cases[i].revision;
        selected.capabilities = cases[i].granted;
        rig_start(&r, &offered);
        r.p.welcome_override = &selected;
        rig_settle(&r);
        assert(sophia_ss_state(&r.s) == (cases[i].valid ? SOPHIA_SS_READY : SOPHIA_SS_FAILED));
        rig_close(&r);
    }
}

int main(void)
{
    exact_welcome();
    refused_admission_is_atomic();
    retry_preserves_the_whole_intent();
    return 0;
}
