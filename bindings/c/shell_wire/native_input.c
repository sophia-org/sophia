#include "native_lifecycle.h"
#include <string.h>

static int reserve_response(struct sophia_shell_native_lifecycle *o, const struct sophia_shell_frame *f,
    uint64_t *tx, unsigned count, const size_t *lengths)
{
    if (!*tx || *tx>UINT64_MAX-count || f->payload_bytes>sizeof(o->response.source)) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_outbox_reservation ticket;
    int r=sophia_shell_outbox_reserve(o->outbox,lengths,count,&ticket);
    if (r!=SOPHIA_SHELL_OK) return r;
    o->response=(struct native_response){.active=1,.source_kind=f->kind,.source_tx=f->transaction,
        .source_bytes=f->payload_bytes,.ticket=ticket,.count=count};
    memcpy(o->response.source,f->payload,f->payload_bytes);
    for (unsigned i=0; i<count; ++i) o->response.lengths[i]=lengths[i];
    *tx+=count;
    return SOPHIA_SHELL_OK;
}
int native_retry_response(struct sophia_shell_native_lifecycle *o, const struct sophia_shell_frame *f)
{
    struct native_response *r=&o->response;
    if (f->kind!=r->source_kind || f->transaction!=r->source_tx || f->payload_bytes!=r->source_bytes ||
        memcmp(f->payload,r->source,r->source_bytes)) return SOPHIA_SHELL_BUSY;
    struct sophia_shell_outbound_frame frames[2];
    for (unsigned i=0; i<r->count; ++i)
        frames[i]=(struct sophia_shell_outbound_frame){r->frames[i],r->lengths[i],SOPHIA_SHELL_OUTBOUND_CONTROL};
    int result=sophia_shell_outbox_commit(o->outbox,r->ticket,frames,r->count);
    if (result!=SOPHIA_SHELL_OK) return result;
    if (r->activation_tx) {
        o->activation=r->activation; o->activation_tx=r->activation_tx; o->activation_pending=1;
    }
    *r=(struct native_response){0};
    return SOPHIA_SHELL_OK;
}
int native_receive_input(struct sophia_shell_native_lifecycle *o, const struct sophia_shell_frame *f,
    uint64_t *tx, sophia_shell_native_edit apply, void *user)
{
    struct sophia_shell_native_message message;
    int r=sophia_shell_native_launcher_decode(f,&message);
    if (r!=SOPHIA_SHELL_OK) return r;
    const struct sophia_shell_native_input *v=&message.value.input;
    int accept=v->kind==17;
    if (!native_current(o) || !o->focus_valid || !native_binding_equal(&v->event.binding,&o->focus) ||
        v->event.event_id<=o->last_event || v->issued_mono_usec<o->last_issued ||
        (!accept && (o->revision==UINT64_MAX || v->event.state_revision!=o->revision+1))) return SOPHIA_SHELL_INVALID;
    int activate=accept && !o->activation_pending && o->shown.begin.selected && v->event.state_revision==o->revision;
    unsigned count=activate?2:1;
    if (!*tx || *tx>UINT64_MAX-count) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_native_ack ack={v->event,1};
    uint8_t consumed[256],refused[256],activation[256]; size_t lengths[2],refused_length;
    r=sophia_shell_native_ack_encode(consumed,sizeof(consumed),*tx,&ack,&lengths[0]);
    if (r!=SOPHIA_SHELL_OK) return r;
    ack.disposition=2;
    r=sophia_shell_native_ack_encode(refused,sizeof(refused),*tx,&ack,&refused_length);
    if (r!=SOPHIA_SHELL_OK || refused_length!=lengths[0]) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_native_activation a={v->event,1,o->shown.begin.selected};
    uint64_t activation_tx=*tx+1;
    if (activate) {
        r=sophia_shell_native_activation_encode(activation,sizeof(activation),activation_tx,&a,&lengths[1]);
        if (r!=SOPHIA_SHELL_OK) return r;
    }
    r=reserve_response(o,f,tx,count,lengths);
    if (r!=SOPHIA_SHELL_OK) return r;
    /* Callback cannot repeat after this ownership point. Both possible ACK
     * encodings already exist; choosing its outcome cannot allocate or fail. */
    int applied=activate;
    if (!accept) {
        o->in_callback=1;
        applied=apply && apply(user,v->kind,v->text,v->text_bytes);
        o->in_callback=0; o->revision=v->event.state_revision;
    }
    memcpy(o->response.frames[0],applied?consumed:refused,lengths[0]);
    if (activate) {
        memcpy(o->response.frames[1],activation,lengths[1]);
        o->response.activation=a; o->response.activation_tx=activation_tx;
    }
    o->last_event=v->event.event_id; o->last_issued=v->issued_mono_usec;
    return native_retry_response(o,f);
}
static int action_matches(const struct sophia_shell_native_lifecycle *o,
                          const struct sophia_shell_content_action_identity *a)
{
    const struct native_scene *s=&o->shown;
    return s->valid && a->output.id==s->begin.output.id && a->output.generation==s->begin.output.generation &&
        a->candidate_generation==s->begin.candidate_generation && a->presentation_epoch==s->epoch &&
        a->interaction_generation==s->begin.interaction_generation &&
        a->allocation.id==s->chunk.surface.allocation.id && a->allocation.generation==s->chunk.surface.allocation.generation;
}
int native_receive_action(struct sophia_shell_native_lifecycle *o, const struct sophia_shell_frame *f, uint64_t *tx)
{
    struct sophia_shell_content_feedback feedback;
    int r=sophia_shell_content_feedback_decode(f,&feedback);
    if (r!=SOPHIA_SHELL_OK) return r;
    const struct sophia_shell_content_action *v=&feedback.value.action;
    const struct sophia_shell_content_action_identity *id=&v->identity;
    if (feedback.grant.connection_epoch!=o->limits.grant.connection_epoch ||
        feedback.grant.content_grant_epoch!=o->limits.grant.content_grant_epoch) return SOPHIA_SHELL_INVALID;
    /* Cancellation names an old receipt, not a fresh action or ACK obligation.
     * It cannot erase an already owned activation/outcome. */
    if (v->kind==3) return SOPHIA_SHELL_OK;
    if (!native_current(o) || !action_matches(o,id) || id->event_id<=o->last_action) return SOPHIA_SHELL_INVALID;
    uint16_t slot=0;
    for (unsigned i=0; i<o->shown.chunk.target_count; ++i) {
        const struct sophia_shell_native_target *t=&o->shown.chunk.targets[i];
        if (t->id==id->target_id && t->generation==id->target_generation && t->slot==id->action_id) slot=t->slot;
    }
    int activate=v->kind==1 && slot && o->focus_valid && o->focus.state_revision==o->revision && !o->activation_pending;
    unsigned count=activate?2:1;
    if (!*tx || *tx>UINT64_MAX-count) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_content_action_ack ack={feedback.grant,*id,(uint16_t)((activate || v->kind==2)?1:2)};
    uint8_t ack_bytes[256],activation[256]; size_t lengths[2];
    r=sophia_shell_content_action_ack_encode(ack_bytes,sizeof(ack_bytes),*tx,&ack,&lengths[0]);
    if (r!=SOPHIA_SHELL_OK) return r;
    struct sophia_shell_native_activation a={{o->focus,id->event_id,o->revision},2,slot};
    uint64_t activation_tx=*tx+1;
    if (activate) {
        r=sophia_shell_native_activation_encode(activation,sizeof(activation),activation_tx,&a,&lengths[1]);
        if (r!=SOPHIA_SHELL_OK) return r;
    }
    r=reserve_response(o,f,tx,count,lengths);
    if (r!=SOPHIA_SHELL_OK) return r;
    memcpy(o->response.frames[0],ack_bytes,lengths[0]);
    if (activate) {
        memcpy(o->response.frames[1],activation,lengths[1]);
        o->response.activation=a; o->response.activation_tx=activation_tx;
    }
    o->last_action=id->event_id;
    if (v->kind==2) {o->dismissed=1; o->focus_valid=0;}
    return native_retry_response(o,f);
}
