#include "shell_session_peer.h"

static uint8_t transaction[65536], tab_rows[8192], reference_rows[256 * 204];
static uint8_t expected[3][65536];
static struct sophia_sf_record candidates[3];
static size_t lengths[3];
static const struct sophia_ss_config config = {
    {1, 8, 1 | 4 | 8 | 16}, SOPHIA_SF_DESCRIPTOR, 4096, 4, 4 * 65536, NULL, 0};

static void records(void)
{
    struct sophia_sf_reference_entry entry = {
        1, {(const uint8_t *)"k", 1}, {(const uint8_t *)"label", 5}};
    for (unsigned i = 0; i < 1024; i++)
        assert(!sophia_sf_tab_order_encode(tab_rows + i * 8, i + 1));
    for (unsigned i = 0; i < 256; i++) {
        entry.slot = (uint16_t)(i + 1);
        assert(!sophia_sf_reference_entry_encode(reference_rows + i * 204, &entry));
    }
    memset(candidates, 0, sizeof(candidates));
    candidates[0].header.kind = SOPHIA_SF_TABS_CANDIDATE;
    candidates[0].value.tabs_candidate =
        (struct sophia_sf_tabs_candidate){1, PEER_EPOCH, 2, 3, 1024, tab_rows, sizeof(tab_rows)};
    candidates[1].header.kind = SOPHIA_SF_REFERENCE_CANDIDATE;
    candidates[1].value.reference_candidate = (struct sophia_sf_reference_candidate){
        2, PEER_EPOCH, 2, 3, 4, 5, 1, 0, 256,
        {10, 16, 4, 2, 3, 5, 1, 8, 2,
         {0, 0xff010203, 0xff040506, 0xff070809, 0xff0a0b0c, 0xff0d0e0f},
         {(const uint8_t *)"Keys", 4}},
        reference_rows, sizeof(reference_rows)};
    candidates[2] = candidates[1];
    candidates[2].value.reference_candidate.transaction = 3;
    for (unsigned i = 0; i < 3; i++) {
        struct sophia_sf_record v = candidates[i];
        v.header.epoch = PEER_EPOCH;
        v.header.submission = i + 2;
        assert(!sophia_sf_encode(expected[i], sizeof(expected[i]), &v, &lengths[i]));
        assert(lengths[i] == (i ? 52488u : 8260u));
        assert(sophia_sf_record_bytes(&v) == lengths[i]);
        /* Session sizing assigns header context; the body epoch is not 1. */
        assert(sophia_ss_record_bytes(&candidates[i]) == lengths[i]);
    }
    assert(!sophia_ss_record_bytes(NULL) && !sophia_sf_record_bytes(NULL));
    struct sophia_sf_record bad = candidates[0];
    bad.value.tabs_candidate.connection_epoch = 0;
    assert(!sophia_ss_record_bytes(&bad));
    bad = candidates[1];
    bad.value.reference_candidate.rows_bytes--;
    assert(!sophia_ss_record_bytes(&bad));
}
static void ready(struct rig *r, size_t capacity, size_t queue_bytes)
{
    struct sophia_ss_config c = config;
    size_t bytes;
    c.queue_bytes = queue_bytes;
    bytes = sophia_ss_storage_bytes(c.msize, c.queue_bytes);
    memset(r, 0, sizeof(*r));
    assert(bytes && !socketpair(AF_UNIX, SOCK_STREAM, 0, r->fd));
    r->storage = malloc(bytes);
    assert(r->storage);
    r->p.fd = r->fd[1];
    r->p.require_custody_ack = r->p.descriptor = 1;
    r->now = 1000;
    assert(!sophia_ss_open_fd_staging(&r->s, r->fd[0], &c, r->storage, bytes,
                                     capacity ? transaction : NULL, capacity));
    rig_settle(r);
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_READY);
    rig_ack(r);
    r->s.files.iounit[1] = 127;
    r->p.policy = P_HOLD;
}
static void group_custody_and_copy(void)
{
    struct rig r;
    uint64_t ticket = 0;
    records();
    ready(&r, 52488, config.queue_bytes);
    assert(!sophia_ss_submit(&r.s, candidates, 3, &ticket) && ticket == 1);
    /* Neither queue compaction nor mutation of the borrowed source may
     * change an admitted record, including the one being fragmented. */
    memset(tab_rows, 0xcc, sizeof(tab_rows));
    memset(reference_rows, 0xcc, sizeof(reference_rows));
    for (unsigned i = 0; i < 3; i++) {
        rig_run(&r, 1024);
        assert(r.p.submit_held && r.p.submits == i + 2);
        assert(r.p.staged_used == lengths[i] && !memcmp(r.p.staged, expected[i], lengths[i]));
        assert(rig_outcome(&r, i + 1) == SOPHIA_SS_IN_FLIGHT);
        peer_submitted(&r.p, i + 2, candidates[i].header.kind);
        peer_answer_submit(&r.p, 0);
        rig_run(&r, 16);
        assert(rig_outcome(&r, i + 1) == SOPHIA_SS_SUBMITTED);
        assert(r.p.submits == i + 2 && !r.p.transaction_busy);
        if (i < 2)
            assert(rig_outcome(&r, i + 2) == SOPHIA_SS_ADMITTED_LOCAL);
        rig_ack(&r);
    }
    assert(!r.s.queued && !r.s.queue_used && !r.s.flight);
    rig_close(&r);
}
static void atomicity_and_reservations(void)
{
    struct rig r;
    struct sophia_ss_reservation reservation;
    uint64_t ticket = 77;
    size_t sum;
    records();
    sum = lengths[0] + lengths[1] + lengths[2];
    ready(&r, sizeof(transaction), sum);
    candidates[2].value.reference_candidate.connection_epoch++;
    assert(sophia_ss_submit(&r.s, candidates, 3, &ticket) == SOPHIA_9P_INVALID);
    assert(ticket == 77 && r.s.next_ticket == 1 && !r.s.queue_used && !r.s.queued && !r.s.flight);
    candidates[2].value.reference_candidate.connection_epoch--;
    assert(!sophia_ss_reserve(&r.s, 3, sum - 1, &reservation));
    assert(sophia_ss_commit(&r.s, &reservation, candidates, 3, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(r.s.reserved && r.s.next_ticket == 1 && ticket == 77 && !r.s.queue_used);
    assert(!sophia_ss_cancel(&r.s, &reservation));
    assert(!sophia_ss_reserve(&r.s, 3, sum, &reservation));
    assert(sophia_ss_submit(&r.s, candidates, 1, &ticket) == SOPHIA_9P_BUSY);
    assert(!sophia_ss_commit(&r.s, &reservation, candidates, 3, &ticket) && ticket == 1);
    assert(!r.s.reserved);
    rig_run(&r, 1024);
    assert(r.p.submit_held);
    sophia_ss_close(&r.s);
    assert(rig_outcome(&r, 1) == SOPHIA_SS_UNKNOWN_DISCONNECTED);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_DROPPED_UNSENT);
    assert(rig_outcome(&r, 3) == SOPHIA_SS_DROPPED_UNSENT);
    rig_close(&r);
    ready(&r, sizeof(transaction), sum - 1);
    assert(sophia_ss_submit(&r.s, candidates, 3, &ticket) == SOPHIA_9P_BUSY);
    assert(r.s.next_ticket == 1 && !r.s.queue_used);
    rig_close(&r);
}
static void insufficient_transaction(void)
{
    const size_t capacity[] = {0, 8260, 52487};
    struct rig r;
    uint64_t ticket = 77;
    records();
    for (unsigned i = 0; i < 3; i++) {
        ready(&r, capacity[i], config.queue_bytes);
        assert(sophia_ss_submit(&r.s, candidates, 3, &ticket) == SOPHIA_9P_ARGUMENT);
        assert(ticket == 77 && r.s.next_ticket == 1 && !r.s.queue_used && !r.s.flight);
        rig_run(&r, 32);
        assert(r.p.submits == 1);
        rig_close(&r);
    }
    ready(&r, sizeof(transaction), 8192);
    assert(sophia_ss_submit(&r.s, candidates, 1, &ticket) == SOPHIA_9P_ARGUMENT);
    assert(r.s.next_ticket == 1 && !r.s.queue_used);
    rig_close(&r);
}
static void paced_retry(void)
{
    struct rig r;
    uint64_t ticket;
    unsigned writes;
    records();
    ready(&r, sizeof(transaction), config.queue_bytes);
    r.p.policy = P_ERROR;
    r.p.error = 11;
    assert(!sophia_ss_submit(&r.s, candidates + 1, 2, &ticket) && ticket == 1);
    rig_run(&r, 1024);
    assert(r.s.waiting && r.p.submits == 2 && r.s.files.next_submission == 3);
    writes = r.p.tx_writes;
    uint8_t frozen[52488];
    memcpy(frozen, r.p.staged, sizeof(frozen));
    rig_run(&r, 32);
    assert(r.p.submits == 2);
    r.p.policy = P_HOLD;
    r.now += 4;
    rig_run(&r, 32);
    assert(r.p.submit_held && r.p.submits == 3 && rig_last_id(&r) == 2);
    assert(r.p.tx_writes == writes && !memcmp(r.p.staged, frozen, sizeof(frozen)));
    assert(rig_outcome(&r, 1) == SOPHIA_SS_IN_FLIGHT);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_ADMITTED_LOCAL);
    peer_submitted(&r.p, 2, SOPHIA_SF_REFERENCE_CANDIDATE);
    peer_answer_submit(&r.p, 0);
    rig_run(&r, 16);
    sophia_ss_close(&r.s);
    assert(rig_outcome(&r, 1) == SOPHIA_SS_SUBMITTED);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_DROPPED_UNSENT);
    rig_close(&r);
}
static void buffer_refusals(void)
{
    static struct sophia_ss session, before;
    struct sophia_ss_config c = config;
    size_t bytes = sophia_ss_storage_bytes(c.msize, c.queue_bytes);
    uint8_t *storage = malloc(bytes);
    void *bad[] = {NULL, transaction, transaction, transaction, &session,
                  storage, storage + bytes - 8192, (void *)(uintptr_t)(UINTPTR_MAX - 4096)};
    size_t caps[] = {8192, 0, 8191, 65537, 8192, 8192, 8192, 8192};
    assert(storage);
    memset(&session, 0xa5, sizeof(session));
    before = session;
    for (unsigned i = 0; i < sizeof(caps) / sizeof(caps[0]); i++) {
        assert(sophia_ss_open_fd_staging(&session, -1, &c, storage, bytes, bad[i], caps[i]) ==
               SOPHIA_9P_ARGUMENT);
        assert(!memcmp(&session, &before, sizeof(session)));
    }
    void *objects[] = {&session, storage + bytes - 296, transaction + 200};
    for (unsigned i = 0; i < 3; i++) {
        c.object_storage = objects[i];
        c.object_capacity = 296;
        assert(sophia_ss_open_fd_staging(&session, -1, &c, storage, bytes, transaction,
                                         sizeof(transaction)) == SOPHIA_9P_ARGUMENT);
        assert(!memcmp(&session, &before, sizeof(session)));
    }
    free(storage);
}
int main(void)
{
    buffer_refusals();
    insufficient_transaction();
    group_custody_and_copy();
    atomicity_and_reservations();
    paced_retry();
    puts("shell session caller-owned staging tests passed");
    return 0;
}
