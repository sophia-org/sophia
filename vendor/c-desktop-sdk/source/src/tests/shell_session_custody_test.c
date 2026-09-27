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
static void dropped_when_only_staged(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.hold_tx = 1;
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    rig_run(&r, 6);
    assert(r.p.tx_held && r.p.submits == 1);
    peer_close(&r.p);
    rig_run(&r, 2);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_CLOSED);
    assert(rig_outcome(&r, first) == SOPHIA_SS_DROPPED_UNSENT);
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
static void submitted_behind_event(int poison)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    const struct sophia_sf_record *e;
    uint8_t bad[48] = {48, 0, 0, 0, 2, 0, 18, 0, 17};
    uint64_t first;
    rig_ready(&r, &rig_config);
    r.p.policy = P_SILENT;
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
           (poison ? SOPHIA_SS_UNKNOWN_DISCONNECTED : SOPHIA_SS_SUBMITTED));
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
int main(void)
{
    admission_atomicity();
    reservation();
    unknown_after_issued_submit();
    dropped_when_only_staged();
    definitive_refusal();
    submitted_is_sticky();
    submitted_in_final_read();
    submitted_behind_event(0);
    submitted_behind_event(1);
    submitted_behind_event(2);
    ealready();
    eagain_waits_for_progress();
    clock_rules();
    stale_events();
    puts("shell session: admission, reservation, custody, refusal, EALREADY, EAGAIN, ESTALE "
         "passed");
    return 0;
}
