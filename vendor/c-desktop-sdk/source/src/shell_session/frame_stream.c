#include "internal.h"
#include "../sophia_shell_frame_stream.h"
#include "../sophia_shell_frames.h"
#include <string.h>

static int same_key(const struct sophia_sft_key *a, const struct sophia_sft_key *b)
{
    return a->connection_epoch == b->connection_epoch && a->content_epoch == b->content_epoch &&
           a->output_id == b->output_id && a->output_generation == b->output_generation &&
           a->allocation_id == b->allocation_id && a->allocation_generation == b->allocation_generation &&
           a->facts_generation == b->facts_generation && a->stream_id == b->stream_id;
}
int sophia_ss_frame_has_credit(const struct sophia_ss_frame_stream *f)
{
    return f && f->state == SOPHIA_SS_FRAME_OPEN && f->credit;
}
int sophia_ss_frame_drained(const struct sophia_ss_frame_stream *f)
{
    return f && f->state == SOPHIA_SS_FRAME_CLOSED;
}
void sophia_ss_frame_disconnect(struct sophia_ss_frame_stream *f)
{
    if (f) { f->state = SOPHIA_SS_FRAME_UNKNOWN; f->credit = 0; }
}
int sophia_ss_frame_open(struct sophia_ss *s, struct sophia_ss_frame_stream *f,
                        uint64_t transaction, const struct sophia_sft_key *key, uint64_t *ticket)
{
    struct sophia_sf_record r = {0};
    int status;
    if (!s || !f || !key || !ticket || f->state != SOPHIA_SS_FRAME_CLOSED ||
        key->stream_id <= f->last_stream ||
        (f->key.connection_epoch && (key->connection_epoch != f->key.connection_epoch ||
                                    key->content_epoch != f->key.content_epoch)))
        return SOPHIA_9P_ARGUMENT;
    r.header.kind = SOPHIA_SFT_OPEN;
    r.value.frame.stream.transaction = transaction;
    r.value.frame.stream.key = *key;
    status = sophia_ss_submit(s, &r, 1, ticket);
    if (status) return status;
    f->state = SOPHIA_SS_FRAME_OPENING;
    f->key = *key;
    f->opening = transaction;
    f->closing = 0;
    f->credit = f->closed_notice = 0;
    return 0;
}
int ss_frame_check(const struct sophia_ss_frame_stream *f, const struct sophia_sf_record *r)
{
    const struct sophia_sf_candidate *c;
    const struct sophia_sft_key *k;
    if (!f || !r || f->state != SOPHIA_SS_FRAME_OPEN || r->header.kind != SOPHIA_SFT_COMMIT)
        return SOPHIA_9P_ARGUMENT;
    c = &r->value.frame.commit.candidate; k = &f->key;
    if (!k->connection_epoch || c->grant_connection_epoch != k->connection_epoch ||
        c->grant_content_epoch != k->content_epoch || c->output_id != k->output_id ||
        c->output_generation != k->output_generation || c->facts_generation != k->facts_generation ||
        c->pacing_permit != k->stream_id || c->surface_count != 1 ||
        c->surfaces[0].allocation_id != k->allocation_id ||
        c->surfaces[0].allocation_generation != k->allocation_generation ||
        c->candidate_generation <= f->last_candidate) return SOPHIA_9P_ARGUMENT;
    return !sophia_ss_frame_has_credit(f) || f->pending_transaction ? SOPHIA_9P_BUSY : 0;
}
void ss_frame_submitted(struct sophia_ss_frame_stream *f, const struct sophia_sf_record *r)
{
    const struct sophia_sf_candidate *c = &r->value.frame.commit.candidate;
    f->last_candidate = f->pending_generation = c->candidate_generation;
    f->pending_transaction = c->transaction;
    f->prepared = f->credit = 0;
}
int sophia_ss_frame_submit(struct sophia_ss *s, struct sophia_ss_frame_stream *f,
                          const struct sophia_sf_record *r, uint64_t *ticket)
{
    int status;
    if (!s || !ticket) return SOPHIA_9P_ARGUMENT;
    status = ss_frame_check(f, r);
    if (status) return status;
    status = sophia_ss_submit(s, r, 1, ticket);
    if (!status) ss_frame_submitted(f, r);
    return status;
}
int sophia_ss_frame_close(struct sophia_ss *s, struct sophia_ss_frame_stream *f,
                         uint64_t transaction, uint64_t *ticket)
{
    struct sophia_sf_record r = {0};
    int status;
    if (!s || !f || !ticket ||
        (f->state != SOPHIA_SS_FRAME_OPEN && f->state != SOPHIA_SS_FRAME_CLOSING) ||
        f->closing || f->closed_notice || transaction == f->opening) return SOPHIA_9P_ARGUMENT;
    r.header.kind = SOPHIA_SFT_CLOSE;
    r.value.frame.stream.transaction = transaction;
    r.value.frame.stream.key = f->key;
    status = sophia_ss_submit(s, &r, 1, ticket);
    if (status) return status;
    f->state = SOPHIA_SS_FRAME_CLOSING;
    f->credit = 0;
    f->closing = transaction;
    return 0;
}
static void finish_drain(struct sophia_ss_frame_stream *f)
{
    if (f->closed_notice && !f->pending_transaction && !f->closing) f->state = SOPHIA_SS_FRAME_CLOSED;
}
static int refused(struct sophia_ss_frame_stream *f, const struct sophia_sft_refused *v)
{
    const struct sophia_sft_key *k = &f->key;
    if (v->connection_epoch != k->connection_epoch || v->content_epoch != k->content_epoch ||
        v->output_id != k->output_id || v->output_generation != k->output_generation ||
        v->stream_id != k->stream_id) return -1;
    switch (v->request_kind) {
    case SOPHIA_SFT_OPEN:
        if (f->state != SOPHIA_SS_FRAME_OPENING || v->transaction != f->opening) return -1;
        f->state = SOPHIA_SS_FRAME_CLOSED;
        return 0;
    case SOPHIA_SFT_CLOSE:
        if (f->state != SOPHIA_SS_FRAME_CLOSING || v->transaction != f->closing) return -1;
        f->closing = 0;
        finish_drain(f);
        return 0;
    case SOPHIA_SFT_COMMIT:
        if (!f->pending_transaction || v->transaction != f->pending_transaction ||
            v->candidate_generation != f->pending_generation || f->prepared) return -1;
        f->pending_transaction = f->pending_generation = 0;
        /* The refused request spent no host credit: undo only our local reservation. */
        f->credit = f->state == SOPHIA_SS_FRAME_OPEN;
        finish_drain(f);
        return 0;
    default: return -1;
    }
}
static int outcome(struct sophia_ss_frame_stream *f, const struct sophia_sft_outcome *v)
{
    const struct sophia_sft_key *k = &f->key;
    const struct sophia_sf_candidate_outcome *c = &v->candidate;
    if (c->grant_connection_epoch != k->connection_epoch || c->grant_content_epoch != k->content_epoch ||
        c->output_id != k->output_id || c->output_generation != k->output_generation ||
        v->stream_id != k->stream_id || v->allocation_id != k->allocation_id ||
        v->allocation_generation != k->allocation_generation || v->facts_generation != k->facts_generation ||
        !f->pending_transaction || c->transaction != f->pending_transaction ||
        c->candidate_generation != f->pending_generation) return -1;
    if (c->kind == 1) {
        if (f->prepared) return -1;
        f->prepared = 1;
    } else {
        if (f->closed_notice && v->credit_expires_msec) return -1;
        f->pending_transaction = f->pending_generation = 0;
        f->prepared = 0;
        f->credit = f->state == SOPHIA_SS_FRAME_OPEN && v->credit_expires_msec != 0;
        if (!f->credit) f->state = SOPHIA_SS_FRAME_CLOSING;
        finish_drain(f);
    }
    return 0;
}
int sophia_ss_frame_observe(struct sophia_ss_frame_stream *f, const struct sophia_sf_record *r)
{
    struct sophia_sft_record value;
    const struct sophia_sft_stream *v;
    int status = -1;
    if (!f || !r) return SOPHIA_9P_ARGUMENT;
    if (f->state == SOPHIA_SS_FRAME_UNKNOWN || !f->key.connection_epoch ||
        r->header.epoch != f->key.connection_epoch || r->header.kind < SOPHIA_SFT_OPENED ||
        r->header.kind > SOPHIA_SFT_OUTCOME) goto fail;
    value.header = r->header;
    /* Validate the active event without copying the candidate-sized union. */
    if (r->header.kind == SOPHIA_SFT_OUTCOME) value.value.outcome = r->value.frame.outcome;
    else if (r->header.kind == SOPHIA_SFT_REFUSED) value.value.refused = r->value.frame.refused;
    else value.value.stream = r->value.frame.stream;
    if (!sophia_sft_record_bytes(&value, SOPHIA_SFT_REVISION)) goto fail;
    v = &r->value.frame.stream;
    switch (r->header.kind) {
    case SOPHIA_SFT_OPENED:
        if (f->state == SOPHIA_SS_FRAME_OPENING && same_key(&f->key, &v->key) && v->transaction == f->opening) {
            f->last_stream = f->key.stream_id;
            f->state = SOPHIA_SS_FRAME_OPEN;
            f->credit = 1;
            status = 0;
        }
        break;
    case SOPHIA_SFT_CLOSED:
        if ((f->state == SOPHIA_SS_FRAME_OPEN || f->state == SOPHIA_SS_FRAME_CLOSING) &&
            !f->closed_notice && same_key(&f->key, &v->key) &&
            (v->transaction == f->opening || v->transaction == f->closing)) {
            f->closed_notice = 1;
            if (v->transaction == f->closing) f->closing = 0;
            f->credit = 0;
            f->state = SOPHIA_SS_FRAME_CLOSING;
            finish_drain(f);
            status = 0;
        }
        break;
    case SOPHIA_SFT_REFUSED: status = refused(f, &r->value.frame.refused); break;
    case SOPHIA_SFT_OUTCOME: status = outcome(f, &r->value.frame.outcome); break;
    }
    if (!status) return 0;
fail:
    sophia_ss_frame_disconnect(f);
    return SOPHIA_9P_INVALID;
}
