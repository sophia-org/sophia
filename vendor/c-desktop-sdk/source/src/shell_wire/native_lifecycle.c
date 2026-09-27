#include "native_lifecycle.h"
#include <stdlib.h>

int sophia_shell_native_lifecycle_new(const struct sophia_shell_frame *limits,
    uint64_t catalog, struct sophia_shell_outbox *outbox, struct sophia_shell_native_lifecycle **out)
{
    if (!catalog || !outbox || !outbox->max_records || !out) return SOPHIA_SHELL_ARGUMENT;
    struct sophia_shell_content_limits l;
    int r=sophia_shell_content_limits_decode(limits,&l);
    if (r!=SOPHIA_SHELL_OK) return r;
    if (outbox->max_bytes>l.max_input_queue_bytes || outbox->max_records>l.max_control_records ||
        outbox->max_records-outbox->reserve_records<2 || l.max_frame_payload<124) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_native_lifecycle *o=calloc(1,sizeof(*o));
    if (!o) return SOPHIA_SHELL_BUSY;
    o->limits=l; o->outbox=outbox; o->catalog=catalog; *out=o; return SOPHIA_SHELL_OK;
}
int sophia_shell_native_lifecycle_catalog(struct sophia_shell_native_lifecycle *o, uint64_t generation)
{
    if (!o || !generation || generation<=o->catalog) return SOPHIA_SHELL_INVALID;
    if (o->response.active || o->in_callback) return SOPHIA_SHELL_BUSY;
    o->catalog=generation; o->focus_valid=0; return SOPHIA_SHELL_OK;
}
int sophia_shell_native_lifecycle_offer(struct sophia_shell_native_lifecycle *o,
    const struct sophia_shell_native_candidate *begin, const struct sophia_shell_native_chunk *chunk, uint64_t *tx)
{
    if (!o || !begin || !chunk || !tx || !*tx || *tx>UINT64_MAX-2) return SOPHIA_SHELL_ARGUMENT;
    if (o->pending.valid || o->response.active || o->in_callback) return SOPHIA_SHELL_BUSY;
    if (!native_current(o) || o->generation==UINT64_MAX || begin->candidate_generation || chunk->candidate_generation ||
        !native_grant_equal(begin->grant,native_grant(o)) || !native_grant_equal(chunk->grant,begin->grant) ||
        begin->opening!=o->opening.opening || begin->catalog_generation!=o->catalog || begin->state_revision!=o->revision ||
        !native_id_equal(begin->output,o->opening.output) || chunk->ordinal || chunk->surface_count!=1 ||
        chunk->placement_count!=begin->placement_count || chunk->target_count!=begin->row_count ||
        chunk->placement_count>32 || chunk->target_count>32 ||
        chunk->placement_count>o->limits.max_candidate_placements || chunk->target_count>o->limits.max_candidate_targets)
        return SOPHIA_SHELL_INVALID;
    for (unsigned i=0; i<4; ++i)
        if (chunk->surface.margins[i]<-(int)o->limits.max_margin_logical ||
            chunk->surface.margins[i]>(int)o->limits.max_margin_logical) return SOPHIA_SHELL_INVALID;
    for (unsigned i=0; i<chunk->target_count; ++i) {
        if (chunk->targets[i].slot!=begin->rows[i]) return SOPHIA_SHELL_INVALID;
        for (unsigned j=0; j<i; ++j) if (chunk->targets[i].id==chunk->targets[j].id) return SOPHIA_SHELL_INVALID;
    }
    size_t data=40+108+2*begin->row_count+64+32*chunk->placement_count+48*chunk->target_count;
    if (data>o->limits.max_candidate_bytes) return SOPHIA_SHELL_INVALID;
    struct native_scene scene={.begin=*begin,.chunk=*chunk,.begin_tx=*tx,.valid=1};
    scene.begin.candidate_generation=scene.chunk.candidate_generation=o->generation+1;
    uint8_t a[196],b[2688]; size_t an,bn;
    int r=sophia_shell_native_candidate_encode(a,sizeof(a),*tx,&scene.begin,&an);
    if (r!=SOPHIA_SHELL_OK) return r;
    r=sophia_shell_native_chunk_encode(b,sizeof(b),*tx+1,&scene.chunk,&bn);
    if (r!=SOPHIA_SHELL_OK) return r;
    if (an-24>o->limits.max_frame_payload || bn-24>o->limits.max_frame_payload) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_outbound_frame pair[2]={{a,an,SOPHIA_SHELL_OUTBOUND_BULK},{b,bn,SOPHIA_SHELL_OUTBOUND_BULK}};
    r=sophia_shell_outbox_push(o->outbox,pair,2);
    if (r!=SOPHIA_SHELL_OK) return r;
    o->pending=scene; ++o->generation; *tx+=2; return SOPHIA_SHELL_OK;
}
int sophia_shell_native_lifecycle_pump(struct sophia_shell_native_lifecycle *o, uint64_t *tx)
{
    if (!o || !tx || !*tx || *tx==UINT64_MAX) return SOPHIA_SHELL_ARGUMENT;
    if (o->response.active || o->in_callback) return SOPHIA_SHELL_BUSY;
    if (!o->pending.valid || o->pending.end_queued) return SOPHIA_SHELL_AGAIN;
    struct sophia_shell_candidate_end end={o->limits.grant,o->pending.begin.candidate_generation,
        1,o->pending.begin.placement_count,o->pending.begin.row_count};
    uint8_t bytes[64]; size_t length;
    int r=sophia_shell_candidate_end_encode(bytes,sizeof(bytes),*tx,&end,&length);
    if (r!=SOPHIA_SHELL_OK) return r;
    struct sophia_shell_outbound_frame frame={bytes,length,SOPHIA_SHELL_OUTBOUND_CONTROL};
    r=sophia_shell_outbox_push(o->outbox,&frame,1);
    if (r==SOPHIA_SHELL_OK) {o->pending.end_queued=1; ++*tx;}
    return r;
}
static int outcome(struct sophia_shell_native_lifecycle *o, const struct sophia_shell_frame *f)
{
    struct sophia_shell_content_feedback feedback;
    int r=sophia_shell_content_feedback_decode(f,&feedback);
    if (r!=SOPHIA_SHELL_OK) return r;
    struct sophia_shell_candidate_outcome *v=&feedback.value.candidate;
    const struct sophia_shell_native_candidate *b=&o->pending.begin;
    if (!o->pending.valid || feedback.grant.connection_epoch!=o->limits.grant.connection_epoch ||
        feedback.grant.content_grant_epoch!=o->limits.grant.content_grant_epoch || f->transaction!=o->pending.begin_tx ||
        v->generation!=b->candidate_generation || v->output.id!=b->output.id || v->output.generation!=b->output.generation)
        return SOPHIA_SHELL_INVALID;
    if (v->kind==1) {
        if (!o->pending.end_queued || o->pending.prepared) return SOPHIA_SHELL_INVALID;
        o->pending.prepared=1;
    } else {
        if (v->kind==2) {
            if (!o->pending.prepared) return SOPHIA_SHELL_INVALID;
            if (native_current(o) && b->opening==o->opening.opening && b->catalog_generation==o->catalog) {
                o->shown=o->pending; o->shown.epoch=v->presentation_epoch; o->focus_valid=0;
            }
        }
        o->pending=(struct native_scene){0};
    }
    return SOPHIA_SHELL_OK;
}
static int focus(struct sophia_shell_native_lifecycle *o, const struct sophia_shell_native_binding *v)
{
    const struct native_scene *s=&o->shown;
    if (!native_current(o) || !s->valid || !native_grant_equal(v->grant,native_grant(o)) ||
        v->opening!=o->opening.opening || v->catalog_generation!=o->catalog ||
        !native_id_equal(v->output,s->begin.output) || !native_id_equal(v->allocation,s->chunk.surface.allocation) ||
        v->candidate_generation!=s->begin.candidate_generation || v->presentation_epoch!=s->epoch ||
        v->interaction_generation!=s->begin.interaction_generation || v->state_revision!=s->begin.state_revision ||
        v->state_revision!=o->revision || v->focus_lease<=o->last_lease) return SOPHIA_SHELL_INVALID;
    o->focus=*v; o->last_lease=v->focus_lease; o->focus_valid=1; return SOPHIA_SHELL_OK;
}
int sophia_shell_native_lifecycle_receive(struct sophia_shell_native_lifecycle *o,
    const struct sophia_shell_frame *f, uint64_t *tx, sophia_shell_native_edit apply, void *user)
{
    if (!o || !f || !f->payload || !tx) return SOPHIA_SHELL_ARGUMENT;
    if (f->payload_bytes>o->limits.max_frame_payload) return SOPHIA_SHELL_INVALID;
    if (o->in_callback) return SOPHIA_SHELL_BUSY;
    if (o->response.active) return native_retry_response(o,f);
    if (f->kind==175) return outcome(o,f);
    if (f->kind==179) return native_receive_action(o,f,tx);
    if (f->kind==193) return native_receive_input(o,f,tx,apply,user);
    struct sophia_shell_native_message message;
    int r=sophia_shell_native_launcher_decode(f,&message);
    if (r!=SOPHIA_SHELL_OK) return r;
    switch (f->kind) {
    case 187: {
        struct sophia_shell_native_opening v=message.value.opening;
        if (o->open || !native_grant_equal(v.grant,native_grant(o)) || v.catalog_generation!=o->catalog ||
            v.opening<=o->last_opening) return SOPHIA_SHELL_INVALID;
        o->opening=v; o->last_opening=v.opening; o->revision=v.state_revision;
        o->open=1; o->dismissed=0; o->shown=(struct native_scene){0}; o->focus_valid=0;
        return SOPHIA_SHELL_OK;
    }
    case 191: return focus(o,&message.value.focus);
    case 192:
        if (!native_binding_equal(&message.value.revoked.binding,&o->focus)) return SOPHIA_SHELL_INVALID;
        o->focus_valid=0; return SOPHIA_SHELL_OK;
    case 196: {
        const struct sophia_shell_native_activation *a=&message.value.outcome.activation;
        if (!o->activation_pending || f->transaction!=o->activation_tx ||
            !native_event_equal(&a->event,&o->activation.event) || a->cause!=o->activation.cause || a->slot!=o->activation.slot)
            return SOPHIA_SHELL_INVALID;
        o->activation_pending=0;
        if (message.value.outcome.status==1 && o->open && a->event.binding.opening==o->opening.opening) {
            o->dismissed=1; o->focus_valid=0;
        }
        return SOPHIA_SHELL_OK;
    }
    case 197:
        if (!o->open || !native_grant_equal(message.value.closed.grant,native_grant(o)) ||
            message.value.closed.opening!=o->opening.opening) return SOPHIA_SHELL_INVALID;
        o->open=0; o->focus_valid=0; o->shown=(struct native_scene){0}; return SOPHIA_SHELL_OK;
    default: return SOPHIA_SHELL_INVALID;
    }
}
int sophia_shell_native_lifecycle_inspect(const struct sophia_shell_native_lifecycle *o,
                                        struct sophia_shell_native_lifecycle_snapshot *out)
{
    if (!o || !out) return SOPHIA_SHELL_ARGUMENT;
    *out=(struct sophia_shell_native_lifecycle_snapshot){o->open,o->shown.valid,o->focus_valid,o->pending.valid,
        o->activation_pending,o->dismissed,o->opening,o->revision,o->shown.begin.candidate_generation,o->shown.epoch,
        o->focus,o->shown.begin.selected}; return SOPHIA_SHELL_OK;
}
void sophia_shell_native_lifecycle_dispose(struct sophia_shell_native_lifecycle *o) {free(o);}
