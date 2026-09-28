#include "shell_session_peer.h"

/* This tests transaction storage against a scripted transport, not descriptor
 * authorization. The descriptor role and production export are separate gates. */
struct file_rig {
    struct sophia_sf_client client;
    struct sophia_9p_client wire;
    struct peer peer;
    int fd[2];
    void *storage;
};
static uint8_t transaction[52488], rows[256 * 204], encoded[52488], expected[52488];
static const struct sophia_sf_negotiate offer = {1, 8, 1 | 4 | 8 | 16};
static void wire_open(struct file_rig *r)
{
    size_t bytes = sophia_9p_storage_bytes(4096, 8);
    memset(r, 0, sizeof(*r));
    assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, r->fd));
    r->storage = malloc(bytes);
    assert(r->storage);
    assert(!sophia_9p_init(&r->wire, r->fd[0], 4096, 8, 32, r->storage, bytes));
    r->peer.fd = r->fd[1];
    r->peer.require_custody_ack = 1;
    r->peer.descriptor = 1;
}
static void close_rig(struct file_rig *r)
{
    close(r->fd[0]);
    close(r->fd[1]);
    free(r->storage);
}
static void run(struct file_rig *r, unsigned passes, int ack)
{
    for (unsigned i = 0; i < passes; i++) {
        const struct sophia_sf_record *event;
        int status;
        assert(!sophia_sf_client_service(&r->client, 65536));
        peer_pump(&r->peer);
        while (!sophia_sf_client_event(&r->client, &event))
            assert(!sophia_sf_client_event_consume(&r->client));
        if (ack) {
            status = sophia_sf_client_ack(&r->client);
            assert(!status || status == SOPHIA_9P_BUSY);
        }
    }
}
static void ready(struct file_rig *r, size_t capacity)
{
    struct sophia_sf_buffers buffers = {NULL, 0, capacity ? transaction : NULL, capacity};
    wire_open(r);
    memset(transaction, 0xa5, sizeof(transaction));
    assert(!sophia_sf_client_init_buffers(&r->client, &r->wire, offer, SOPHIA_SF_DESCRIPTOR, &buffers));
    run(r, 128, 1);
    assert(sophia_sf_client_ready(&r->client) && !r->client.submit_stage &&
           r->client.next_submission == 2 && r->peer.submits == 1);
}
static struct sophia_sf_record tabs(void)
{
    struct sophia_sf_record v = {
        {SOPHIA_SF_TABS_CANDIDATE, PEER_EPOCH, 1, 0},
        .value.tabs_candidate = {1, PEER_EPOCH, 2, 3, 1024, rows, 1024 * 8}};
    for (unsigned i = 0; i < 1024; i++)
        assert(!sophia_sf_tab_order_encode(rows + i * 8, i + 1));
    return v;
}
static struct sophia_sf_record reference(void)
{
    struct sophia_sf_record v = {
        {SOPHIA_SF_REFERENCE_CANDIDATE, PEER_EPOCH, 1, 0},
        .value.reference_candidate = {
            1,
            PEER_EPOCH,
            2,
            3,
            4,
            5,
            1,
            0,
            256,
            {10,
             16,
             4,
             2,
             3,
             5,
             1,
             8,
             2,
             {0, 0xff010203, 0xff040506, 0xff070809, 0xff0a0b0c, 0xff0d0e0f},
             {(const uint8_t *)"Keys", 4}},
            rows,
            256 * 204}};
    struct sophia_sf_reference_entry e = {
        1, {(const uint8_t *)"k", 1}, {(const uint8_t *)"label", 5}};
    for (unsigned i = 0; i < 256; i++) {
        e.slot = (uint16_t)(i + 1);
        assert(!sophia_sf_reference_entry_encode(rows + i * 204, &e));
    }
    return v;
}
static void maximum_roundtrip(int reference_kind, int as_bytes)
{
    struct file_rig r;
    struct sophia_sf_record v = reference_kind ? reference() : tabs();
    enum sophia_sf_submission stage;
    size_t n;
    uint64_t id;
    ready(&r, sizeof(transaction));
    /* Force many writes; the peer also keeps the submit reply outstanding. */
    r.client.iounit[1] = 127;
    r.peer.policy = P_HOLD;
    v.header.submission = 2;
    assert(!sophia_sf_encode(expected, sizeof(expected), &v, &n));
    assert(n == (reference_kind ? 52488u : 8260u));
    memcpy(encoded, expected, n);
    memset(encoded + 16, 0, 8);
    if (as_bytes)
        assert(!sophia_sf_client_submit_bytes(&r.client, encoded, n));
    else
        assert(!sophia_sf_client_submit(&r.client, &v));
    memset(rows, 0xcc, sizeof(rows));
    memset(encoded, 0xcc, sizeof(encoded));
    run(&r, 1024, 1);
    assert(r.peer.submit_held && r.peer.submits == 2 && r.peer.staged_used == n);
    assert(!memcmp(r.peer.staged, expected, n));
    assert(!sophia_sf_client_submission(&r.client, &id, &stage) && id == 2 &&
           stage == SOPHIA_SF_SUBMISSION_ISSUED);
    /* Submission cannot overwrite the outstanding transaction. */
    assert(sophia_sf_client_submit(&r.client, &v) == SOPHIA_9P_BUSY);
    assert(!memcmp(r.client.tx_storage, expected, n));
    peer_submitted(&r.peer, 2, v.header.kind);
    peer_answer_submit(&r.peer, 0);
    run(&r, 16, 0);
    assert(!sophia_sf_client_submission(&r.client, &id, &stage) &&
           stage == SOPHIA_SF_SUBMISSION_CUSTODIED);
    assert(r.client.submit_stage == 6 && r.peer.acked == 2 && !r.peer.transaction_busy);
    run(&r, 16, 1);
    assert(!r.client.submit_stage && !r.peer.transaction_busy);
    close_rig(&r);
}
static void capacity_and_retry(void)
{
    struct file_rig r;
    struct sophia_sf_record v = tabs();
    size_t n;
    ready(&r, 0);
    assert(sophia_sf_client_submit(&r.client, &v) == SOPHIA_9P_ARGUMENT);
    assert(!r.client.submit_stage && r.client.next_submission == 2 && r.peer.submits == 1);
    close_rig(&r);
    v = reference();
    ready(&r, sizeof(transaction) - 1);
    assert(sophia_sf_client_submit(&r.client, &v) == SOPHIA_9P_ARGUMENT);
    assert(!r.client.submit_stage && r.client.next_submission == 2 && r.peer.submits == 1);
    close_rig(&r);
    ready(&r, sizeof(transaction));
    v.header.submission = 2;
    assert(!sophia_sf_encode(expected, sizeof(expected), &v, &n));
    memcpy(encoded, expected, n);
    memset(encoded + 16, 0, 8);
    encoded[86] = 1; /* Reserved reference-candidate padding. */
    assert(sophia_sf_client_submit_bytes(&r.client, encoded, n) == SOPHIA_9P_INVALID);
    assert(!r.client.submit_stage && r.client.next_submission == 2 && r.peer.submits == 1);
    encoded[86] = 0;
    encoded[8]++; /* A different attach epoch cannot consume an id either. */
    assert(sophia_sf_client_submit_bytes(&r.client, encoded, n) == SOPHIA_9P_ARGUMENT);
    assert(!r.client.submit_stage && r.client.next_submission == 2 && r.peer.submits == 1);
    r.peer.policy = P_ERROR;
    r.peer.error = 11;
    r.peer.once = 1;
    assert(!sophia_sf_client_submit(&r.client, &v));
    run(&r, 128, 1);
    assert(r.client.submit_wait && r.peer.submits == 2 && r.client.next_submission == 3);
    assert(!memcmp(r.client.tx_storage, expected, n));
    run(&r, 32, 1);
    assert(r.peer.submits == 2); /* No service-driven replay on EAGAIN. */
    assert(!sophia_sf_client_submit_retry(&r.client));
    run(&r, 32, 1);
    assert(r.peer.submits == 3 && peer_get(r.peer.last_submit + 8, 8) == 2);
    assert(!r.client.submit_stage && r.client.submitted && !r.peer.transaction_busy);
    close_rig(&r);
}
static void invalid_storage(void)
{
    struct file_rig r;
    struct sophia_sf_client before;
    struct sophia_9p_client wire_before;
    wire_open(&r);
    memset(&r.client, 0xa5, sizeof(r.client));
    memcpy(&before, &r.client, sizeof(before));
    memcpy(&wire_before, &r.wire, sizeof(wire_before));
    const struct sophia_sf_buffers invalid[] = {{NULL, 0, NULL, 8192},
                                                {NULL, 0, transaction, 0},
                                                {NULL, 0, transaction, 8191},
                                                {NULL, 0, transaction, SOPHIA_SF_MAX_RECORD + 1u},
                                                {NULL, 0, &r.client, 8192},
                                                {NULL, 0, &r.wire, 8192},
                                                {NULL, 0, r.storage, 8192},
                                                {transaction + 8191, 296, transaction, 8192},
                                                {NULL, 0, (void *)(UINTPTR_MAX - 8191), 8192}};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        assert(sophia_sf_client_init_buffers(&r.client, &r.wire, offer, SOPHIA_SF_BAR,
                                             &invalid[i]) == SOPHIA_9P_ARGUMENT);
        assert(!memcmp(&r.client, &before, sizeof(before)) &&
               !memcmp(&r.wire, &wire_before, sizeof(wire_before)));
        peer_pump(&r.peer);
        assert(!r.peer.fid_count && !r.peer.submits);
    }
    close_rig(&r);
}
int main(void)
{
    invalid_storage();
    for (int kind = 0; kind < 2; kind++)
        for (int as_bytes = 0; as_bytes < 2; as_bytes++)
            maximum_roundtrip(kind, as_bytes);
    capacity_and_retry();
    puts("shell staging: caller-owned maxima, custody barrier, same-id retry and buffer refusals "
         "passed");
    return 0;
}
