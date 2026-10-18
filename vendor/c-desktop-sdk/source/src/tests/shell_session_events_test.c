#include "shell_session_peer.h"

static const struct sophia_sf_record *next_event(struct rig *r, uint64_t sequence)
{
    const struct sophia_sf_record *e = NULL;
    unsigned i;
    for (i = 0; i < 8 && sophia_ss_event(&r->s, &e); i++)
        rig_step(r);
    assert(e && e->header.sequence == sequence);
    return e;
}
static int fetch_status(struct rig *r, uint16_t kind, uint64_t generation, uint64_t qid,
                        const struct sophia_sf_record **o)
{
    unsigned i;
    int status = SOPHIA_9P_BUSY;
    *o = NULL;
    assert(!sophia_ss_object(&r->s, kind, generation, qid));
    for (i = 0; i < 64 && status == SOPHIA_9P_BUSY; i++) {
        rig_step(r);
        status = sophia_ss_object_result(&r->s, o);
    }
    return status;
}
static const struct sophia_sf_record *fetch(struct rig *r, uint16_t kind, uint64_t generation,
                                            uint64_t qid)
{
    const struct sophia_sf_record *o;
    assert(!fetch_status(r, kind, generation, qid, &o) && o && o->header.kind == kind);
    return o;
}
static void ack(struct rig *r, uint64_t sequence)
{
    assert(!sophia_ss_ack(&r->s));
    rig_run(r, 4);
    assert(r->p.acked == sequence && r->s.files.acked_sequence == sequence);
}
/* Negotiation leaves sequences 1 (Submitted) and 2 (Negotiated) consumed. */
static void ack_bounded_by_undelivered_events(void)
{
    struct rig r;
    struct sophia_ss_obligations o;
    rig_ready_unacked(&r, &rig_config);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    assert(!sophia_ss_obligations(&r.s, &o) && o.consumed == 2 && !o.acked);
    assert(o.ack_due_ms == 1000 + SOPHIA_SS_ACK_PROGRESS_MS && !o.objects && !o.blocked);
    peer_app_event(&r.p);
    peer_app_event(&r.p);
    next_event(&r, 3);
    /* Delivered but not consumed: not acknowledgeable. */
    assert(sophia_ss_ack_limit(&r.s) == 2);
    ack(&r, 2);
    assert(!sophia_ss_obligations(&r.s, &o) && !o.ack_due_ms);
    assert(!sophia_ss_consume(&r.s));
    assert(sophia_ss_ack_limit(&r.s) == 3);
    next_event(&r, 4);
    assert(sophia_ss_ack_limit(&r.s) == 3);
    ack(&r, 3);
    assert(!sophia_ss_consume(&r.s));
    ack(&r, 4);
    assert(!sophia_ss_ack(&r.s) && !r.s.files.ack_op.active);
    rig_close(&r);
}
static void ack_bounded_by_unfetched_object(void)
{
    struct rig r;
    struct sophia_ss_obligations o;
    const struct sophia_sf_record *object;
    rig_ready(&r, &rig_config);
    r.p.outputs_generation = 4;
    peer_app_event(&r.p);
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    peer_app_event(&r.p);
    next_event(&r, 3);
    assert(!sophia_ss_consume(&r.s));
    assert(next_event(&r, 4)->header.kind == SOPHIA_SF_OBJECT_PUBLISHED);
    assert(!sophia_ss_consume(&r.s));
    next_event(&r, 5);
    assert(!sophia_ss_consume(&r.s));
    /* Consumed through 5, but the announcement at 4 is owed a fetch. */
    assert(sophia_ss_ack_limit(&r.s) == 3);
    assert(!sophia_ss_obligations(&r.s, &o) && o.consumed == 5 && o.ack_limit == 3);
    assert(o.objects == 1u << (SOPHIA_SF_OUTPUTS - 1) && o.blocked);
    ack(&r, 3);
    /* A fetch that is not the announced object leaves it owed. */
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 0, 0);
    assert(object->value.outputs.facts_generation == 4);
    assert(sophia_ss_ack_limit(&r.s) == 3);
    r.p.outputs_generation = 5;
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    assert(object->value.outputs.facts_generation == 5);
    assert(sophia_ss_ack_limit(&r.s) == 5);
    assert(!sophia_ss_obligations(&r.s, &o) && !o.objects && !o.blocked);
    ack(&r, 5);
    rig_close(&r);
}
/* A newer announcement of a feed must not move the ack bound past the first
 * unfetched one; only fetching the newest object discharges both. */
static void superseded_announcement_keeps_bound(void)
{
    struct rig r;
    const struct sophia_sf_record *object;
    uint64_t sequence;
    rig_ready(&r, &rig_config);
    r.p.outputs_generation = 5;
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    peer_app_event(&r.p);
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 6, OUTPUTS_QID);
    peer_app_event(&r.p);
    for (sequence = 3; sequence <= 6; sequence++) {
        next_event(&r, sequence);
        assert(!sophia_ss_consume(&r.s));
    }
    assert(sophia_ss_ack_limit(&r.s) == 2);
    ack(&r, 2);
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    assert(object->value.outputs.facts_generation == 5);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    r.p.outputs_generation = 6;
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 6, OUTPUTS_QID);
    assert(object->value.outputs.facts_generation == 6);
    assert(sophia_ss_ack_limit(&r.s) == 6);
    ack(&r, 6);
    rig_close(&r);
}
/* Only EOF may follow the exact record, even when bytes arrive across reads. */
static void object_eof_probe(void)
{
    struct rig r;
    const struct sophia_sf_record *object;
    rig_ready(&r, &rig_config);
    r.p.outputs_generation = 1;
    /* Exact object: the probe past it reads EOF. */
    assert(fetch(&r, SOPHIA_SF_OUTPUTS, 1, OUTPUTS_QID)->value.outputs.facts_generation == 1);
    /* Positive short reads are not EOF: 16 reads of 7 bytes, then the probe. */
    r.p.object_read_max = 7;
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 1, OUTPUTS_QID);
    assert(object->value.outputs.facts_generation == 1);
    /* The exact record, then one trailing byte in a later read: refused. */
    r.p.object_trailing = 1;
    r.p.object_read_max = 112;
    assert(fetch_status(&r, SOPHIA_SF_OUTPUTS, 1, OUTPUTS_QID, &object) == SOPHIA_9P_INVALID);
    r.p.object_read_max = 7;
    assert(fetch_status(&r, SOPHIA_SF_OUTPUTS, 1, OUTPUTS_QID, &object) == SOPHIA_9P_INVALID);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY && !r.s.files.stale);
    rig_close(&r);
}
/* Peek the announcement, fetch and verify its object, then consume it. */
static void fetch_then_consume(void)
{
    struct rig r;
    const struct sophia_sf_record *object;
    struct sophia_ss_obligations o;
    rig_ready(&r, &rig_config);
    r.p.outputs_generation = 5;
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    peer_app_event(&r.p);
    next_event(&r, 3);
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    assert(object->value.outputs.facts_generation == 5);
    /* A failed fetch does not overwrite the verified record. */
    r.p.object_trailing = 1;
    assert(fetch_status(&r, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID, &object) == SOPHIA_9P_INVALID);
    r.p.object_trailing = 0;
    assert(next_event(&r, 3)->header.kind == SOPHIA_SF_OBJECT_PUBLISHED);
    assert(!sophia_ss_consume(&r.s));
    assert(sophia_ss_ack_limit(&r.s) == 3);
    next_event(&r, 4);
    assert(!sophia_ss_consume(&r.s));
    assert(sophia_ss_ack_limit(&r.s) == 4);
    assert(!sophia_ss_obligations(&r.s, &o) && !o.objects && !o.blocked);
    ack(&r, 4);
    rig_close(&r);
}
/* An older announcement is owed; the newer one is fetched before it is
 * consumed. The earliest bound holds until that consume discharges both. */
static void superseded_fetch_then_consume(void)
{
    struct rig r;
    const struct sophia_sf_record *object;
    rig_ready(&r, &rig_config);
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 6, OUTPUTS_QID);
    next_event(&r, 3);
    assert(!sophia_ss_consume(&r.s));
    assert(sophia_ss_ack_limit(&r.s) == 2);
    assert(next_event(&r, 4)->header.kind == SOPHIA_SF_OBJECT_PUBLISHED);
    r.p.outputs_generation = 6;
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 6, OUTPUTS_QID);
    assert(object->value.outputs.facts_generation == 6);
    /* Not the latest consumed announcement: the older hold stays. */
    assert(sophia_ss_ack_limit(&r.s) == 2);
    assert(!sophia_ss_consume(&r.s));
    assert(sophia_ss_ack_limit(&r.s) == 4);
    ack(&r, 4);
    rig_close(&r);
}
static void poll_reports_queued_output(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first;
    rig_ready(&r, &rig_config);
    rig_run(&r, 4);
    assert(sophia_ss_poll_fd(&r.s) == r.fd[0]);
    assert(sophia_ss_poll_events(&r.s) == POLLIN && !sophia_9p_wants_write(&r.s.wire));
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    /* Admission queued the transaction write without sending it. */
    assert(sophia_ss_poll_events(&r.s) == (POLLIN | POLLOUT));
    assert(!sophia_ss_dispatch(&r.s, POLLOUT, 65536, r.now));
    assert(sophia_ss_poll_events(&r.s) == POLLIN);
    assert(rig_until(&r, first, 32) == SOPHIA_SS_SUBMITTED);
    /* Quiescent once the transaction is reopened and the events read is held. */
    rig_ack(&r);
    rig_run(&r, 12);
    assert(r.s.files.submit_stage == 0 && r.p.event_held);
    assert(sophia_ss_poll_events(&r.s) == POLLIN);
    sophia_ss_close(&r.s);
    assert(!sophia_ss_poll_events(&r.s));
    rig_close(&r);
}
/* One 1x1 resource: the vector limits allow exactly one 4-byte chunk. */
/* ESTALE on an object fid fails that fetch; on events it ends the session. */
static void object_stale_is_not_terminal(void)
{
    struct rig r;
    const struct sophia_sf_record *object = NULL;
    unsigned i;
    int status = SOPHIA_9P_BUSY;
    rig_ready(&r, &rig_config);
    r.p.object_error = 116;
    assert(!sophia_ss_object(&r.s, SOPHIA_SF_OUTPUTS, 0, 0));
    for (i = 0; i < 16 && status == SOPHIA_9P_BUSY; i++) {
        rig_step(&r);
        status = sophia_ss_object_result(&r.s, &object);
    }
    assert(status == SOPHIA_9P_INVALID);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    assert(!r.s.files.stale && r.s.files.remote_error == 116);
    r.p.object_error = 0;
    r.p.outputs_generation = 1;
    object = fetch(&r, SOPHIA_SF_OUTPUTS, 0, 0);
    assert(object->value.outputs.facts_generation == 1);
    r.p.event_error = 116;
    peer_answer_events(&r.p);
    rig_step(&r);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_STALE && r.s.files.stale);
    rig_close(&r);
}
static void negotiation_refusal(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint16_t reason = 0;
    uint64_t denied = 0, first = 0;
    rig_start(&r, &rig_config);
    r.p.refuse_negotiation = 1;
    assert(sophia_ss_refusal(&r.s, &reason, &denied) == SOPHIA_9P_AGAIN);
    rig_settle(&r);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_REFUSED_NEGOTIATION);
    assert(!sophia_ss_refusal(&r.s, &reason, &denied) && reason == 4 && denied == 0x10);
    assert(sophia_ss_submit(&r.s, &request, 1, &first) == SOPHIA_9P_INVALID && !first);
    assert(!sophia_ss_epoch(&r.s) && !sophia_ss_limits(&r.s) && !sophia_ss_poll_events(&r.s));
    rig_close(&r);
}
static void outcome_eviction(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    enum sophia_ss_outcome o;
    uint32_t error = 1;
    uint64_t ticket, i;
    rig_ready(&r, &rig_config);
    for (i = 1; i <= SOPHIA_SS_OUTCOMES + 2; i++) {
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket) && ticket == i);
        assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
        rig_ack(&r);
    }
    /* Evicted tickets report unavailable; they never alias a later one. */
    assert(!sophia_ss_outcome(&r.s, 1, &o, &error) && o == SOPHIA_SS_UNAVAILABLE && !error);
    assert(!sophia_ss_outcome(&r.s, 2, &o, NULL) && o == SOPHIA_SS_UNAVAILABLE);
    assert(!sophia_ss_outcome(&r.s, 3, &o, NULL) && o == SOPHIA_SS_SUBMITTED);
    assert(sophia_ss_outcome(&r.s, SOPHIA_SS_OUTCOMES + 3, &o, NULL) == SOPHIA_9P_ARGUMENT);
    assert(sophia_ss_outcome(&r.s, 0, &o, NULL) == SOPHIA_9P_ARGUMENT);
    rig_close(&r);
}
/* Accepted custody occupies the server's single transaction slot until its
 * Submitted is acknowledged, even if the application queues nothing else. */
static void transaction_reopen_waits_for_custody_ack(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t ticket;
    rig_ready(&r, &rig_config);
    ack(&r, 2);
    r.p.require_custody_ack = 1;
    assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
    assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
    rig_run(&r, 16); /* Deliberately defer the caller's cumulative ack. */
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY);
    assert(!r.p.transaction_busy && r.p.acked == 2);
    ack(&r, 3);
    request = rig_request(2);
    assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
    assert(rig_until(&r, ticket, 32) == SOPHIA_SS_SUBMITTED);
    assert(!r.p.transaction_busy);
    rig_close(&r);
}
static void transaction_reopen_waits_for_object_hold(void)
{
    struct rig r;
    struct sophia_sf_record request = rig_request(1);
    uint64_t first, second;
    rig_ready(&r, &rig_config);
    r.p.require_custody_ack = 1;
    r.p.outputs_generation = 5;
    peer_published(&r.p, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID);
    next_event(&r, 3);
    assert(!sophia_ss_consume(&r.s));
    assert(!sophia_ss_submit(&r.s, &request, 1, &first));
    request = rig_request(2);
    assert(!sophia_ss_submit(&r.s, &request, 1, &second));
    assert(rig_until(&r, first, 32) == SOPHIA_SS_SUBMITTED);
    assert(sophia_ss_ack_limit(&r.s) == 2);
    rig_ack(&r); /* Cumulative ack cannot cross the unfetched publication. */
    rig_run(&r, 16);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY && !r.p.transaction_busy);
    assert(rig_outcome(&r, second) == SOPHIA_SS_ADMITTED_LOCAL);
    assert(fetch(&r, SOPHIA_SF_OUTPUTS, 5, OUTPUTS_QID)->value.outputs.facts_generation == 5);
    assert(sophia_ss_ack_limit(&r.s) == 4);
    rig_ack(&r);
    assert(rig_until(&r, second, 32) == SOPHIA_SS_SUBMITTED);
    assert(!r.p.transaction_busy);
    rig_close(&r);
}
static void unsupported_descriptor_publications_cannot_be_acknowledged(void)
{
    for (uint16_t kind = SOPHIA_SF_DESCRIPTORS; kind <= SOPHIA_SF_SHORTCUTS; kind++) {
        struct rig r;
        rig_ready(&r, &rig_config);
        peer_published(&r.p, kind, 1, 1234);
        rig_run(&r, 8);
        assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED);
        assert(sophia_ss_ack_limit(&r.s) == 2 && r.s.files.consumed_sequence == 2);
        assert(sophia_ss_ack(&r.s) == SOPHIA_9P_INVALID && r.p.acked == 2);
        rig_close(&r);
    }
    struct rig r;
    struct sophia_sf_record event = {{SOPHIA_SF_DESCRIPTOR_OUTCOME, PEER_EPOCH, 0, 1},
        .value.descriptor_outcome = {1, PEER_EPOCH, 1, 0, 1}};
    rig_ready(&r, &rig_config);
    peer_record(&r.p, &event);
    rig_run(&r, 8);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED);
    assert(sophia_ss_ack_limit(&r.s) == 2 && r.p.acked == 2);
    rig_close(&r);
}
static void cumulative_ack_coalesces_consumed_burst(void)
{
    struct rig r;
    unsigned before, i;
    rig_ready(&r, &rig_config);
    before = r.p.acks;
    for (i = 0; i < 3; i++)
        peer_app_event(&r.p);
    for (i = 0; i < 3; i++) {
        next_event(&r, 3 + i);
        assert(!sophia_ss_consume(&r.s));
    }
    /* Explicit scheduling remains explicit: consuming never sends an ACK. */
    assert(r.p.acks == before);
    ack(&r, 5);
    assert(r.p.acks == before + 1);
    rig_close(&r);
}

int main(void)
{
    cumulative_ack_coalesces_consumed_burst();
    ack_bounded_by_undelivered_events();
    ack_bounded_by_unfetched_object();
    superseded_announcement_keeps_bound();
    object_eof_probe();
    fetch_then_consume();
    superseded_fetch_then_consume();
    poll_reports_queued_output();
    object_stale_is_not_terminal();
    negotiation_refusal();
    outcome_eviction();
    transaction_reopen_waits_for_custody_ack();
    transaction_reopen_waits_for_object_hold();
    unsupported_descriptor_publications_cannot_be_acknowledged();
    puts("shell session: ack bounds, object obligations, poll, node ESTALE, refusal, "
         "outcome ring passed");
    return 0;
}
