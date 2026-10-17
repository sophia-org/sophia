#include "shell_session_peer.h"

static struct sophia_sf_record many[SOPHIA_SS_SLOTS];

static void admission_atomicity(void)
{
    struct rig r;
    struct sophia_ss_config config = rig_config;
    struct sophia_sf_record group[5];
    enum sophia_ss_outcome o;
    uint64_t first = 0;
    size_t i, used, room, fit;
    uint16_t queued;
    rig_start(&r, &config);
    group[0] = rig_request(1);
    /* No admission and no placeholder epoch before negotiation. */
    assert(sophia_ss_submit(&r.s, group, 1, &first) == SOPHIA_9P_BUSY && !first);
    rig_settle(&r);
    rig_run(&r, 4);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    rig_ack(&r);
    r.p.policy = P_HOLD;
    for (i = 0; i < 5; i++)
        group[i] = rig_request(i + 1);
    /* A record failing validation anywhere in a group admits nothing. */
    group[1].value.allocation_request.desired_width = 0;
    assert(sophia_ss_submit(&r.s, group, 3, &first) == SOPHIA_9P_INVALID && !first);
    assert(r.s.next_ticket == 1 && !r.s.queued && !r.s.flight && !r.s.queue_used);
    group[1] = rig_request(2);
    group[2].header.kind = SOPHIA_SF_NEGOTIATE;
    assert(sophia_ss_submit(&r.s, group, 3, &first) == SOPHIA_9P_ARGUMENT && !first);
    assert(r.s.next_ticket == 1 && !r.s.queued && !r.s.flight);
    assert(sophia_ss_outcome(&r.s, 1, &o, NULL) == SOPHIA_9P_ARGUMENT);
    group[2] = rig_request(3);
    assert(!sophia_ss_submit(&r.s, group, 3, &first) && first == 1);
    rig_run(&r, 6);
    /* The head is issued and held by the peer; two remain queued. */
    assert(r.p.submits == 2 && r.p.submit_held);
    assert(rig_outcome(&r, 1) == SOPHIA_SS_IN_FLIGHT);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_ADMITTED_LOCAL);
    assert(rig_outcome(&r, 3) == SOPHIA_SS_ADMITTED_LOCAL);
    /* Slot cap: two queued plus three exceeds four slots; nothing changes. */
    used = r.s.queue_used;
    assert(sophia_ss_submit(&r.s, group + 2, 3, &first) == SOPHIA_9P_BUSY && first == 1);
    assert(r.s.queued == 2 && r.s.queue_used == used && r.s.next_ticket == 4);
    assert(sophia_ss_outcome(&r.s, 4, &o, NULL) == SOPHIA_9P_ARGUMENT);
    /* A group larger than the configured queue can never be admitted. */
    assert(sophia_ss_submit(&r.s, group, 5, &first) == SOPHIA_9P_ARGUMENT);
    sophia_ss_close(&r.s);
    assert(rig_outcome(&r, 1) == SOPHIA_SS_UNKNOWN_DISCONNECTED);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_DROPPED_UNSENT);
    assert(rig_outcome(&r, 3) == SOPHIA_SS_DROPPED_UNSENT);
    rig_close(&r);

    /* Byte cap: slots to spare, bytes exhausted; nothing changes. */
    config.queue_slots = SOPHIA_SS_SLOTS;
    rig_ready(&r, &config);
    r.p.policy = P_HOLD;
    for (i = 0; i < SOPHIA_SS_SLOTS; i++)
        many[i] = rig_request(i + 1);
    assert(sophia_ss_record_bytes(&many[0]) == 160);
    assert(!sophia_ss_submit(&r.s, many, 50, &first) && first == 1);
    rig_run(&r, 4);
    used = r.s.queue_used;
    queued = r.s.queued;
    room = r.s.queue_capacity - used;
    fit = room / 160;
    assert(queued + fit + 1 <= SOPHIA_SS_SLOTS);
    assert(sophia_ss_submit(&r.s, many, fit + 1, &first) == SOPHIA_9P_BUSY && first == 1);
    assert(r.s.queue_used == used && r.s.queued == queued && r.s.next_ticket == 51);
    assert(!sophia_ss_submit(&r.s, many, fit, &first) && first == 51);
    assert(r.s.queue_used == used + 160 * fit);
    rig_close(&r);
}
static void reservation(void)
{
    struct rig r;
    struct sophia_ss_reservation v, w;
    struct sophia_sf_record group[3];
    uint64_t first = 0;
    size_t bytes;
    rig_ready(&r, &rig_config);
    r.p.policy = P_HOLD;
    group[0] = group[1] = group[2] = rig_request(9);
    bytes = sophia_ss_record_bytes(&group[0]);
    /* Cancellation restores capacity without consuming a ticket or sending. */
    assert(!sophia_ss_reserve(&r.s, 2, 2 * bytes, &v));
    assert(!sophia_ss_cancel(&r.s, &v));
    assert(!r.s.reserved && !r.s.queued && r.s.next_ticket == 1);
    assert(sophia_ss_cancel(&r.s, &v) == SOPHIA_9P_ARGUMENT);
    assert(!sophia_ss_submit(&r.s, group, 1, &first) && first == 1);
    rig_run(&r, 4);
    assert(!sophia_ss_reserve(&r.s, 2, 2 * bytes, &v));
    /* Reserved room is invisible to ordinary submissions, and single. */
    assert(sophia_ss_reserve(&r.s, 1, bytes, &w) == SOPHIA_9P_BUSY);
    assert(sophia_ss_submit(&r.s, group, 3, &first) == SOPHIA_9P_BUSY && first == 1);
    assert(!sophia_ss_submit(&r.s, group, 2, &first) && first == 2);
    assert(sophia_ss_submit(&r.s, group, 1, &first) == SOPHIA_9P_BUSY && first == 2);
    /* Commit refuses sizing and value errors without losing the room. */
    assert(sophia_ss_commit(&r.s, &v, group, 3, &first) == SOPHIA_9P_ARGUMENT);
    group[1].value.allocation_request.desired_width = 0;
    assert(sophia_ss_commit(&r.s, &v, group, 2, &first) == SOPHIA_9P_INVALID && first == 2);
    group[1] = group[0];
    assert(!sophia_ss_commit(&r.s, &v, group, 2, &first) && first == 4);
    assert(r.s.queued == 4 && !r.s.reserved);
    assert(sophia_ss_cancel(&r.s, &v) == SOPHIA_9P_ARGUMENT);
    assert(sophia_ss_reserve(&r.s, 1, bytes, &w) == SOPHIA_9P_BUSY);
    rig_close(&r);

    /* A reservation outliving the session admits nothing. */
    rig_ready(&r, &rig_config);
    assert(!sophia_ss_reserve(&r.s, 1, bytes, &v));
    sophia_ss_close(&r.s);
    assert(sophia_ss_commit(&r.s, &v, group, 1, &first) == SOPHIA_9P_CLOSED);
    assert(r.s.next_ticket == 1 && !r.s.reserved);
    rig_close(&r);
}
static void unknown_after_issued_submit(void)
{
    struct rig r;
    struct sophia_sf_record group[2];
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.policy = P_HOLD;
    group[0] = rig_request(1);
    group[1] = rig_request(2);
    assert(!sophia_ss_submit(&r.s, group, 2, &first) && first == 1);
    rig_run(&r, 6);
    assert(r.p.submits == 2 && r.p.submit_held);
    sophia_ss_close(&r.s);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
    assert(rig_outcome(&r, 1) == SOPHIA_SS_UNKNOWN_DISCONNECTED);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_DROPPED_UNSENT);
    assert(sophia_ss_submit(&r.s, group, 1, &first) == SOPHIA_9P_CLOSED);
    assert(!sophia_ss_poll_events(&r.s));
    rig_close(&r);
}
static void unknown_with_both_write_replies_held(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.hold_tx = 1;
    r.p.policy = P_HOLD;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    rig_run(&r, 6);
    assert(r.p.tx_held && r.p.submit_held && r.p.submits == 2);
    peer_close(&r.p);
    rig_run(&r, 2);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
    assert(rig_outcome(&r, first) == SOPHIA_SS_UNKNOWN_DISCONNECTED);
    rig_close(&r);
}
static void definitive_refusal(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    enum sophia_ss_outcome o;
    uint32_t error = 0;
    uint64_t first, second;
    rig_ready(&r, &rig_config);
    r.p.policy = P_ERROR;
    r.p.error = 22;
    r.p.once = 1;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    assert(rig_until(&r, first, 16) == SOPHIA_SS_REFUSED);
    assert(!sophia_ss_outcome(&r.s, first, &o, &error) && error == 22);
    /* Nothing was journaled; the session and a fresh transaction continue. */
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    assert(!sophia_ss_submit(&r.s, &request, 1, &second) && second == first + 1);
    assert(rig_until(&r, second, 32) == SOPHIA_SS_SUBMITTED);
    assert(!sophia_ss_outcome(&r.s, second, &o, &error) && !error);
    rig_close(&r);
}
static void submitted_is_sticky(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first;
    rig_ready(&r, &rig_config);
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    assert(rig_until(&r, first, 32) == SOPHIA_SS_SUBMITTED);
    peer_close(&r.p);
    rig_run(&r, 2);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
    assert(rig_outcome(&r, first) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}
/* The final Rread carries Submitted and EOF follows in the same pass. */
static void submitted_in_final_read(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.policy = P_SILENT;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    rig_run(&r, 6);
    assert(r.p.submits == 2 && r.p.event_held);
    assert(rig_outcome(&r, first) == SOPHIA_SS_IN_FLIGHT);
    peer_submitted(&r.p, rig_last_id(&r), SOPHIA_SF_ALLOCATION_REQUEST);
    peer_answer_events(&r.p);
    peer_close(&r.p);
    rig_step(&r);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
    assert(rig_outcome(&r, first) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}
/* Behind an undelivered event: validated in order, never raw-scanned. */
static void submitted_behind_event(int poison, int accepted)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    const struct sophia_sf_record *e;
    uint8_t bad[48] = {48, 0, 0, 0, 2, 0, 18, 0, 17};
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.policy = accepted ? P_SILENT : P_HOLD;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    rig_run(&r, 6);
    assert(r.p.submits == 2 && r.p.event_held);
    peer_app_event(&r.p);
    if (poison == 1) {
        /* Wrong envelope version, otherwise shaped like the next event. */
        bad[24] = (uint8_t)++r.p.sequence;
        peer_raw(&r.p, bad, sizeof(bad));
    } else if (poison == 2)
        r.p.sequence--; /* Submitted repeats the previous sequence. */
    peer_submitted(&r.p, rig_last_id(&r), SOPHIA_SF_ALLOCATION_REQUEST);
    peer_answer_events(&r.p);
    peer_close(&r.p);
    rig_step(&r);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
    assert(rig_outcome(&r, first) ==
           (poison && !accepted ? SOPHIA_SS_UNKNOWN_DISCONNECTED : SOPHIA_SS_SUBMITTED));
    /* The undelivered event is still delivered, in order. */
    assert(!sophia_ss_event(&r.s, &e) && e->header.kind == SOPHIA_SF_ALLOCATION_RESULT);
    rig_close(&r);
}
static void ealready(void)
{
    struct rig r;
    struct sophia_sf_record group[2];
    uint64_t first;
    /* Custody already observed: EALREADY confirms it. */
    rig_ready(&r, &rig_config);
    r.p.policy = P_CUSTODY;
    r.p.once = 1;
    group[0] = rig_request(1);
    group[1] = rig_request(2);
    assert(!sophia_ss_submit(&r.s, group, 1, &first));
    assert(rig_until(&r, first, 16) == SOPHIA_SS_SUBMITTED);
    assert(r.p.submit_held);
    peer_answer_submit(&r.p, 114);
    rig_ack(&r);
    rig_run(&r, 8);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    assert(rig_outcome(&r, first) == SOPHIA_SS_SUBMITTED);
    assert(!sophia_ss_submit(&r.s, group + 1, 1, &first));
    assert(rig_until(&r, first, 32) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);

    /* No observed custody: fail closed. */
    rig_ready(&r, &rig_config);
    r.p.policy = P_ERROR;
    r.p.error = 114;
    assert(!sophia_ss_submit(&r.s, group, 2, &first));
    rig_run(&r, 8);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && r.s.files.remote_error == 114);
    assert(rig_outcome(&r, first) == SOPHIA_SS_UNKNOWN_DISCONNECTED);
    assert(rig_outcome(&r, first + 1) == SOPHIA_SS_DROPPED_UNSENT);
    rig_close(&r);
}
static void eagain_waits_for_progress(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    const struct sophia_sf_record *e;
    uint8_t refused[24];
    unsigned i, writes;
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.policy = P_ERROR;
    r.p.error = 11;
    r.p.once = 1;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    for (i = 0; i < 16 && r.p.submits < 2; i++)
        rig_step(&r);
    assert(r.p.submits == 2);
    memcpy(refused, r.p.last_submit, 24);
    writes = r.p.tx_writes;
    /* The pass that sees EAGAIN queues no second Tsubmit. */
    assert(!sophia_ss_dispatch(&r.s, POLLIN, 65536, r.now));
    assert(r.s.files.submit_wait && !r.s.files.submit_op.active);
    assert(!(sophia_ss_poll_events(&r.s) & POLLOUT));
    assert(sophia_ss_timeout(&r.s, r.now) == 4);
    assert(rig_outcome(&r, first) == SOPHIA_SS_IN_FLIGHT);
    /* No progress and no elapsed backoff: no resend. */
    rig_run(&r, 8);
    r.now += 3;
    rig_run(&r, 2);
    assert(r.p.submits == 2);
    r.now += 1;
    assert(!sophia_ss_timeout(&r.s, r.now));
    rig_run(&r, 4);
    /* Same epoch and id, and the staged transaction was not rewritten. */
    assert(r.p.submits == 3 && !memcmp(refused, r.p.last_submit, 24));
    assert(r.p.tx_writes == writes);
    assert(rig_until(&r, first, 16) == SOPHIA_SS_SUBMITTED);
    assert(sophia_ss_timeout(&r.s, r.now) == -1);
    rig_ack(&r);

    /* Consuming an event is progress: the retry needs no clock. */
    r.p.policy = P_ERROR;
    r.p.once = 1;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    for (i = 0; i < 32 && r.p.submits < 4; i++)
        rig_step(&r);
    assert(r.p.submits == 4);
    memcpy(refused, r.p.last_submit, 24);
    writes = r.p.tx_writes;
    peer_app_event(&r.p);
    rig_run(&r, 6);
    assert(r.s.waiting && r.p.submits == 4);
    assert(!sophia_ss_event(&r.s, &e) && e->header.kind == SOPHIA_SF_ALLOCATION_RESULT);
    assert(!sophia_ss_consume(&r.s));
    /* Consume is not a dispatch pass: still nothing resent. */
    peer_pump(&r.p);
    assert(r.p.submits == 4);
    rig_run(&r, 4);
    assert(r.p.submits == 5 && !memcmp(refused, r.p.last_submit, 24));
    assert(r.p.tx_writes == writes);
    assert(rig_until(&r, first, 16) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}
/* Caller time near the top of the range, and running backwards. */
static void clock_rules(void)
{
    const uint64_t top = UINT64_MAX - 2;
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    struct sophia_ss_obligations o;
    unsigned i;
    uint64_t first;
    rig_start(&r, &rig_config);
    r.now = top;
    rig_settle(&r);
    rig_run(&r, 4);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    assert(!sophia_ss_obligations(&r.s, &o) && o.consumed == 2 && o.ack_due_ms == UINT64_MAX);
    assert(sophia_ss_dispatch(&r.s, POLLIN, 65536, top - 1) == SOPHIA_9P_ARGUMENT);
    assert(r.s.now_ms == top && sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    rig_ack(&r);
    r.p.policy = P_ERROR;
    r.p.error = 11;
    r.p.once = 1;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    for (i = 0; i < 16 && r.p.submits < 2; i++)
        rig_step(&r);
    assert(r.p.submits == 2);
    assert(!sophia_ss_dispatch(&r.s, POLLIN, 65536, r.now));
    /* The backoff deadline saturates instead of wrapping into the past. */
    assert(r.s.waiting && r.s.retry_at == UINT64_MAX);
    assert(sophia_ss_timeout(&r.s, r.now) == 2);
    r.now = UINT64_MAX - 1;
    rig_run(&r, 4);
    assert(r.p.submits == 2);
    r.now = UINT64_MAX;
    rig_run(&r, 4);
    assert(r.p.submits == 3);
    assert(rig_until(&r, first, 16) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}
static void stale_events(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.policy = P_HOLD;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    rig_run(&r, 6);
    assert(r.p.submits == 2 && r.p.event_held);
    r.p.event_error = 116;
    peer_answer_events(&r.p);
    rig_step(&r);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_STALE && r.s.files.stale);
    assert(r.s.files.remote_error == 116);
    assert(rig_outcome(&r, first) == SOPHIA_SS_UNKNOWN_DISCONNECTED);
    rig_close(&r);
}
static void reservation_is_bound_to_its_session(void)
{
    struct rig a, b;
    struct sophia_ss_reservation va, vb;
    struct sophia_sf_record record = rig_request(1);
    uint64_t first = 99;
    size_t bytes = sophia_ss_record_bytes(&record);
    rig_ready(&a, &rig_config);
    rig_ready(&b, &rig_config);
    assert(!sophia_ss_reserve(&a.s, 1, bytes, &va));
    assert(!sophia_ss_reserve(&b.s, 1, bytes, &vb));
    assert(va.serial == vb.serial);
    assert(sophia_ss_cancel(&b.s, &va) == SOPHIA_9P_ARGUMENT);
    assert(sophia_ss_commit(&b.s, &va, &record, 1, &first) == SOPHIA_9P_ARGUMENT);
    assert(first == 99 && a.s.reserved && b.s.reserved);
    assert(!sophia_ss_commit(&a.s, &va, &record, 1, &first) && first == 1);
    assert(!sophia_ss_cancel(&b.s, &vb));
    rig_close(&a);
    rig_close(&b);
}

static void old_and_unissued_tickets(void)
{
    struct rig r;
    struct sophia_sf_record request;
    enum sophia_ss_outcome o;
    uint64_t ticket;
    unsigned i;
    rig_ready(&r, &rig_config);
    assert(sophia_ss_outcome(&r.s, 0, &o, NULL) == SOPHIA_9P_ARGUMENT);
    assert(sophia_ss_outcome(&r.s, 1, &o, NULL) == SOPHIA_9P_ARGUMENT);
    for (i = 1; i <= SOPHIA_SS_OUTCOMES + 1; i++) {
        request = rig_request(i);
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket) && ticket == i);
        assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
        rig_ack(&r);
    }
    assert(rig_outcome(&r, 1) == SOPHIA_SS_UNAVAILABLE);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_SUBMITTED);
    assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED);
    assert(sophia_ss_outcome(&r.s, ticket + 1, &o, NULL) == SOPHIA_9P_ARGUMENT);
    assert(sophia_ss_outcome(&r.s, UINT64_MAX, &o, NULL) == SOPHIA_9P_ARGUMENT);
    sophia_ss_close(&r.s);
    assert(rig_outcome(&r, 1) == SOPHIA_SS_UNAVAILABLE);
    assert(rig_outcome(&r, 2) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}

static void reusable_probe_waits_for_negotiated_after_ack(void)
{
    struct rig r;
    rig_start(&r, &rig_config);
    r.p.reusable = 1;
    r.p.hold_negotiated = 1;
    rig_run(&r, 64);
    assert(r.s.files.submitted_sequence && !r.s.files.negotiated);
    /* A lower-level caller may acknowledge Submitted before Negotiated arrives. */
    assert(!sophia_sf_client_ack(&r.s.files));
    rig_run(&r, 16);
    assert(r.s.files.acked_sequence >= r.s.files.submitted_sequence);
    assert(!r.p.reuse_probes);
    peer_record(&r.p, &r.p.held_negotiated);
    rig_settle(&r);
    rig_ack(&r);
    assert(r.s.files.negotiated && r.p.reuse_probes == 1);
    assert(r.s.files.transaction_mode == 1);
    rig_close(&r);
}

static void reusable_transaction_writer(void)
{
    struct rig r;
    struct sophia_sf_record request;
    uint64_t ticket;
    unsigned i;
    rig_start(&r, &rig_config);
    r.p.reusable = 1;
    rig_settle(&r);
    rig_ack(&r);
    assert(r.s.files.transaction_mode == 1 && r.p.reuse_probes == 1);
    for (i = 0; i < 3; i++) {
        request = rig_request(i + 1);
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        assert(rig_until(&r, ticket, 64) == SOPHIA_SS_SUBMITTED);
        rig_ack(&r);
    }
    assert(r.p.tx_opens == 2 && r.p.tx_clunks == 1);
    r.p.policy = P_HOLD;
    request = rig_request(4);
    assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
    rig_run(&r, 12);
    assert(r.p.submit_held);
    peer_error(&r.p, r.p.submit_tag, 22);
    r.p.submit_held = 0;
    assert(rig_until(&r, ticket, 64) == SOPHIA_SS_REFUSED);
    rig_run(&r, 12);
    assert(r.p.reuse_probes == 2 && r.p.tx_opens == 3 && r.p.tx_clunks == 2);
    rig_close(&r);
    for (i = 0; i < 3; i++) {
        static const uint32_t errors[] = {5, 13, 116};
        rig_start(&r, &rig_config);
        r.p.reuse_lookup_error = errors[i];
        rig_settle(&r);
        assert(!sophia_ss_ack(&r.s));
        rig_run(&r, 16);
        assert(sophia_ss_state(&r.s) != SOPHIA_SS_READY);
        assert(r.p.tx_opens == 1 && r.p.reuse_probes == 1);
        rig_close(&r);
    }
}

static void paired_record_failure(void)
{
    static const uint32_t errors[] = {0, 11, 22, 116, 114};
    unsigned reuse, i;
    for (reuse = 0; reuse < 2; reuse++) {
        for (i = 0; i < sizeof(errors) / sizeof(errors[0]); i++) {
            struct rig r;
            struct sophia_sf_record request = rig_request(1);
            uint64_t ticket;
            rig_start(&r, &rig_config);
            r.p.reusable = (int)reuse;
            rig_settle(&r);
            rig_ack(&r);
            r.p.hold_tx = 1;
            r.p.policy = P_HOLD;
            assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
            rig_run(&r, 12);
            assert(r.p.tx_held && r.p.submit_held && r.p.submits == 2);
            peer_count(&r.p, r.p.tx_tag, r.p.tx_count - 1);
            r.p.tx_held = 0;
            rig_run(&r, 2);
            assert(rig_outcome(&r, ticket) == SOPHIA_SS_IN_FLIGHT);
            peer_answer_submit(&r.p, errors[i]);
            rig_run(&r, 12);
            assert(sophia_ss_state(&r.s) != SOPHIA_SS_READY);
            assert(rig_outcome(&r, ticket) ==
                   (!errors[i] ? SOPHIA_SS_SUBMITTED : errors[i] == 114 ?
                    SOPHIA_SS_UNKNOWN_DISCONNECTED : SOPHIA_SS_DROPPED_UNSENT));
            assert(r.p.submits == 2); /* No retry of the incomplete record. */
            rig_close(&r);
        }
    }
}

static void paired_submit_reply_before_record_reply(void)
{
    unsigned reuse;
    for (reuse = 0; reuse < 2; reuse++) {
        struct rig r;
        struct sophia_sf_record request = rig_request(1);
        uint64_t ticket;
        rig_start(&r, &rig_config);
        r.p.reusable = (int)reuse;
        rig_settle(&r);
        rig_ack(&r);
        r.p.hold_tx = 1;
        r.p.policy = P_HOLD;
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        rig_run(&r, 12);
        assert(r.p.tx_held && r.p.submit_held);
        peer_answer_submit(&r.p, 0);
        rig_run(&r, 2);
        assert(rig_outcome(&r, ticket) == SOPHIA_SS_IN_FLIGHT);
        peer_count(&r.p, r.p.tx_tag, r.p.tx_count);
        r.p.tx_held = 0;
        peer_submitted(&r.p, peer_get(r.p.last_submit + 8, 8),
                       (uint16_t)peer_get(r.p.staged + 6, 2));
        assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
        rig_close(&r);
    }
}

static void paired_failure_preserves_early_submitted(void)
{
    unsigned reuse;
    for (reuse = 0; reuse < 2; reuse++) {
        struct rig r;
        struct sophia_sf_record request = rig_request(1);
        uint64_t ticket;
        rig_start(&r, &rig_config);
        r.p.reusable = (int)reuse;
        rig_settle(&r);
        rig_ack(&r);
        r.p.hold_tx = 1;
        r.p.policy = P_HOLD;
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        rig_run(&r, 12);
        assert(r.p.tx_held && r.p.submit_held);
        peer_error(&r.p, r.p.tx_tag, 22);
        r.p.tx_held = 0;
        peer_answer_submit(&r.p, 11);
        peer_submitted(&r.p, peer_get(r.p.last_submit + 8, 8),
                       (uint16_t)peer_get(r.p.staged + 6, 2));
        peer_answer_events(&r.p);
        rig_run(&r, 12);
        assert(sophia_ss_state(&r.s) != SOPHIA_SS_READY);
        assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED);
        assert(r.p.submits == 2);
        rig_close(&r);
    }
}

static void only_final_fragment_can_pair(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t ticket;
    rig_ready(&r, &rig_config);
    rig_ack(&r);
    rig_run(&r, 8);
    r.s.files.iounit[1] = 64;
    r.p.hold_tx = 1;
    r.p.policy = P_HOLD;
    assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
    rig_run(&r, 8);
    assert(r.p.tx_held && r.p.tx_count == 64 && r.p.submits == 1);
    peer_count(&r.p, r.p.tx_tag, r.p.tx_count);
    r.p.tx_held = 0;
    rig_run(&r, 8);
    assert(r.p.tx_held && r.p.tx_count == 64 && r.p.submits == 1);
    peer_count(&r.p, r.p.tx_tag, r.p.tx_count);
    r.p.tx_held = 0;
    rig_run(&r, 8);
    assert(r.p.tx_held && r.p.tx_count == 32 && r.p.submit_held && r.p.submits == 2);
    peer_count(&r.p, r.p.tx_tag, r.p.tx_count);
    r.p.tx_held = 0;
    peer_answer_submit(&r.p, 0);
    peer_submitted(&r.p, peer_get(r.p.last_submit + 8, 8),
                   (uint16_t)peer_get(r.p.staged + 6, 2));
    assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
    rig_close(&r);
}

static void accepted_pair_survives_disconnect(void)
{
    unsigned reuse, record_replied;
    for (reuse = 0; reuse < 2; reuse++) {
        for (record_replied = 0; record_replied < 2; record_replied++) {
            struct rig r;
            struct sophia_sf_record request = rig_request(1);
            uint64_t ticket;
            rig_start(&r, &rig_config);
            r.p.reusable = (int)reuse;
            rig_settle(&r);
            rig_ack(&r);
            r.p.hold_tx = 1;
            r.p.policy = P_HOLD;
            assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
            rig_run(&r, 12);
            assert(r.p.tx_held && r.p.submit_held);
            if (record_replied) {
                peer_count(&r.p, r.p.tx_tag, r.p.tx_count);
                r.p.tx_held = 0;
            }
            peer_answer_submit(&r.p, 0);
            rig_run(&r, 2);
            assert(rig_outcome(&r, ticket) == SOPHIA_SS_IN_FLIGHT);
            peer_close(&r.p);
            rig_run(&r, 2);
            assert(rig_outcome(&r, ticket) == SOPHIA_SS_SUBMITTED);
            rig_close(&r);
        }
    }
}

static void paired_record_stale_keeps_revocation_classification(void)
{
    unsigned reuse;
    for (reuse = 0; reuse < 2; reuse++) {
        struct rig r;
        struct sophia_sf_record request = rig_request(1);
        uint64_t ticket;
        rig_start(&r, &rig_config);
        r.p.reusable = (int)reuse;
        rig_settle(&r);
        rig_ack(&r);
        r.p.hold_tx = 1;
        r.p.policy = P_HOLD;
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        rig_run(&r, 12);
        assert(r.p.tx_held && r.p.submit_held);
        peer_error(&r.p, r.p.tx_tag, 116);
        r.p.tx_held = 0;
        peer_answer_submit(&r.p, 11);
        rig_run(&r, 12);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_STALE);
        assert(r.s.files.stale && r.s.files.remote_error == 116);
        assert(rig_outcome(&r, ticket) == SOPHIA_SS_DROPPED_UNSENT);
        rig_close(&r);
    }
}

int main(void)
{
    paired_record_failure();
    paired_submit_reply_before_record_reply();
    paired_failure_preserves_early_submitted();
    only_final_fragment_can_pair();
    accepted_pair_survives_disconnect();
    paired_record_stale_keeps_revocation_classification();
    reusable_probe_waits_for_negotiated_after_ack();
    reusable_transaction_writer();
    old_and_unissued_tickets();
    admission_atomicity();
    reservation();
    reservation_is_bound_to_its_session();
    unknown_after_issued_submit();
    unknown_with_both_write_replies_held();
    definitive_refusal();
    submitted_is_sticky();
    submitted_in_final_read();
    submitted_behind_event(0, 0);
    submitted_behind_event(1, 0);
    submitted_behind_event(2, 0);
    submitted_behind_event(0, 1);
    submitted_behind_event(1, 1);
    submitted_behind_event(2, 1);
    ealready();
    eagain_waits_for_progress();
    clock_rules();
    stale_events();
    puts("shell session: admission, reservation, custody, refusal, EALREADY, EAGAIN, ESTALE "
         "passed");
    return 0;
}
