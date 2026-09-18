#include "../sophia_shell_content_feedback.h"
#include "fields.h"

static struct sophia_shell_content_id get_id(const uint8_t *p)
{
    return (struct sophia_shell_content_id){shell_get64(p), shell_get64(p+8)};
}
static int valid_id(struct sophia_shell_content_id v, int nullable)
{
    return (v.id && v.generation) || (nullable && !v.id && !v.generation);
}
static int scale(uint32_t n, uint32_t d)
{
    if (!n || n>32 || !d || d>4) return 0;
    while (d) {uint32_t next=n%d; n=d; d=next;}
    return n==1;
}
static int32_t signed32(uint32_t v)
{
    return v<=INT32_MAX ? (int32_t)v : (int32_t)((int64_t)v-INT64_C(4294967296));
}
static int16_t signed16(uint16_t v)
{
    return v<=INT16_MAX ? (int16_t)v : (int16_t)((int32_t)v-65536);
}
static struct sophia_shell_content_rect rect(const uint8_t *p)
{
    return (struct sophia_shell_content_rect){signed32(shell_get32(p)),
        signed32(shell_get32(p+4)),shell_get32(p+8),shell_get32(p+12)};
}
static int facts(const uint8_t *p, size_t bytes, struct sophia_shell_output_facts *v)
{
    if (bytes<32 || shell_get32(p+28)) return 0;
    v->generation=shell_get64(p+16); v->count=shell_get32(p+24);
    if (!v->generation || v->count>16 || bytes!=32+40*v->count) return 0;
    for (uint32_t i=0; i<v->count; ++i) {
        const uint8_t *q=p+32+40*i;
        struct sophia_shell_output_fact *o=&v->outputs[i];
        o->output=get_id(q); o->local_width=shell_get32(q+16); o->local_height=shell_get32(q+20);
        o->scale_numerator=shell_get32(q+24); o->scale_denominator=shell_get32(q+28);
        o->scale_generation=shell_get64(q+32);
        if (!valid_id(o->output,0) || !o->local_width || !o->local_height ||
            !scale(o->scale_numerator,o->scale_denominator) || !o->scale_generation) return 0;
        for (uint32_t j=0; j<i; ++j) if (v->outputs[j].output.id==o->output.id) return 0;
    }
    return 1;
}
static int allocation(const uint8_t *p, size_t bytes, struct sophia_shell_allocation_result *v)
{
    if (bytes!=160 || shell_get32(p+28) || shell_get32(p+156)) return 0;
    v->request_id=shell_get64(p+16); v->status=shell_get16(p+24); v->reason=shell_get16(p+26);
    v->output=get_id(p+32); v->allocation=get_id(p+48); v->parent=get_id(p+64);
    v->scale_generation=shell_get64(p+80); v->logical=rect(p+88); v->pixel=rect(p+104);
    v->scale_numerator=shell_get32(p+120); v->scale_denominator=shell_get32(p+124);
    v->allowed_reservation_extent=shell_get32(p+128);
    for (unsigned i=0; i<4; ++i) {
        v->margins[i]=signed16(shell_get16(p+132+2*i));
        if (v->margins[i]<-512 || v->margins[i]>512) return 0;
    }
    v->acknowledged_anchor=rect(p+140);
    if (v->status<1 || v->status>4 || v->reason>12 ||
        (!v->request_id)!=(v->status==4) || !valid_id(v->output,0) ||
        !valid_id(v->allocation,v->status==2) || !valid_id(v->parent,1)) return 0;
    if (v->status==1) {
        if (v->reason || !scale(v->scale_numerator,v->scale_denominator) ||
            !v->scale_generation || !v->logical.width || !v->logical.height ||
            !v->pixel.width || !v->pixel.height || v->allowed_reservation_extent>512) return 0;
    } else if (v->status==2 || v->status==3) {
        /* All geometry, scale, margins and anchor bytes are zero on these outcomes. */
        for (size_t i=80; i<156; ++i) if (p[i]) return 0;
    }
    return 1;
}
static int candidate(const uint8_t *p, size_t bytes, struct sophia_shell_candidate_outcome *v)
{
    if (bytes!=68) return 0;
    v->generation=shell_get64(p+16); v->output=get_id(p+24);
    v->kind=shell_get16(p+40); v->reason=shell_get16(p+42);
    v->presentation_epoch=shell_get64(p+44); v->work_area_generation=shell_get64(p+52);
    v->wm_commit_generation=shell_get64(p+60);
    return v->generation && valid_id(v->output,0) && v->kind>=1 && v->kind<=4 &&
        v->reason<=12 && (!!v->presentation_epoch)==(v->kind==2) && (v->kind>2 || !v->reason);
}
static int permit(const uint8_t *p, size_t bytes, struct sophia_shell_frame_permit *v)
{
    if (bytes!=64 || shell_get32(p+60)) return 0;
    v->output=get_id(p+16); v->demand_id=shell_get64(p+32); v->permit_id=shell_get64(p+40);
    v->state=shell_get16(p+48); v->reason=shell_get16(p+50);
    v->ttl_ms=shell_get32(p+52); v->max_candidate_bytes=shell_get32(p+56);
    if (!valid_id(v->output,0) || !v->demand_id || v->state<1 || v->state>4 ||
        v->reason>12 || v->max_candidate_bytes>8192) return 0;
    return v->state!=1 || (v->permit_id && v->ttl_ms && v->ttl_ms<=250 &&
        v->max_candidate_bytes && !v->reason);
}
static int action(const uint8_t *p, size_t bytes, struct sophia_shell_content_action *v)
{
    if (bytes!=112 || shell_get32(p+108)) return 0;
    struct sophia_shell_content_action_identity *a=&v->identity;
    a->output=get_id(p+16); a->candidate_generation=shell_get64(p+32);
    a->presentation_epoch=shell_get64(p+40); a->interaction_generation=shell_get64(p+48);
    a->allocation=get_id(p+56); a->target_id=shell_get64(p+72); a->target_generation=shell_get64(p+80);
    a->action_id=shell_get64(p+88); a->event_id=shell_get64(p+96);
    v->kind=shell_get16(p+104); v->reason=shell_get16(p+106);
    if (!valid_id(a->output,0) || !valid_id(a->allocation,0) || !a->candidate_generation ||
        !a->presentation_epoch || !a->interaction_generation || !a->event_id ||
        v->kind<1 || v->kind>3 || v->reason>12) return 0;
    if (v->kind==1) return a->target_id && a->target_generation && a->action_id && !v->reason;
    if (v->kind==2) return !a->target_id && !a->target_generation && !a->action_id && !v->reason;
    return 1;
}
int sophia_shell_content_feedback_decode(const struct sophia_shell_frame *f,
                                        struct sophia_shell_content_feedback *out)
{
    if (!f || !f->payload || !out) return SOPHIA_SHELL_ARGUMENT;
    if (!f->transaction || f->payload_bytes<16) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_content_feedback v={0};
    v.kind=f->kind; v.transaction=f->transaction;
    v.grant=(struct sophia_shell_content_grant){shell_get64(f->payload),shell_get64(f->payload+8)};
    if (!v.grant.connection_epoch || !v.grant.content_grant_epoch) return SOPHIA_SHELL_INVALID;
    int valid=0;
    switch (f->kind) {
    case 162: valid=facts(f->payload,f->payload_bytes,&v.value.facts); break;
    case 164: valid=allocation(f->payload,f->payload_bytes,&v.value.allocation); break;
    case 175: valid=candidate(f->payload,f->payload_bytes,&v.value.candidate); break;
    case 177: valid=permit(f->payload,f->payload_bytes,&v.value.permit); break;
    case 179: valid=action(f->payload,f->payload_bytes,&v.value.action); break;
    default: break;
    }
    if (!valid) return SOPHIA_SHELL_INVALID;
    *out=v;
    return SOPHIA_SHELL_OK;
}
