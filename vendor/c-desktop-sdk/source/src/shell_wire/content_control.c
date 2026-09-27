#include "../sophia_shell_content_control.h"
#include "fields.h"

static int grant_valid(struct sophia_shell_content_grant v)
{
    return v.connection_epoch && v.content_grant_epoch;
}
static int id_valid(struct sophia_shell_content_id v, int nullable)
{
    return (v.id && v.generation) || (nullable && !v.id && !v.generation);
}
static void put_grant(uint8_t *p, struct sophia_shell_content_grant v)
{
    shell_put64(p,v.connection_epoch); shell_put64(p+8,v.content_grant_epoch);
}
static void put_id(uint8_t *p, struct sophia_shell_content_id v)
{
    shell_put64(p,v.id); shell_put64(p+8,v.generation);
}
int sophia_shell_candidate_end_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_candidate_end *v, size_t *bytes)
{
    if (!dst || !v || !bytes) return SOPHIA_SHELL_ARGUMENT;
    if (!tx || !grant_valid(v->grant) || !v->generation || v->surface_count>8 ||
        v->placement_count>32 || v->target_count>64) return SOPHIA_SHELL_INVALID;
    uint8_t p[40]={0}; put_grant(p,v->grant); shell_put64(p+16,v->generation);
    shell_put32(p+24,v->surface_count); shell_put32(p+28,v->placement_count);
    shell_put32(p+32,v->target_count);
    return sophia_shell_frame_encode(dst,capacity,174,tx,p,sizeof(p),bytes);
}
int sophia_shell_frame_demand_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_frame_demand *v, size_t *bytes)
{
    if (!dst || !v || !bytes) return SOPHIA_SHELL_ARGUMENT;
    if (!tx || !grant_valid(v->grant) || !id_valid(v->output,0) ||
        !id_valid(v->allocation,1) || !v->demand_id || v->reason<1 || v->reason>3)
        return SOPHIA_SHELL_INVALID;
    uint8_t p[58]={0}; put_grant(p,v->grant); put_id(p+16,v->output); put_id(p+32,v->allocation);
    shell_put64(p+48,v->demand_id); shell_put16(p+56,v->reason);
    return sophia_shell_frame_encode(dst,capacity,176,tx,p,sizeof(p),bytes);
}
int sophia_shell_demand_cancel_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_demand_cancel *v, size_t *bytes)
{
    if (!dst || !v || !bytes) return SOPHIA_SHELL_ARGUMENT;
    if (!tx || !grant_valid(v->grant) || !id_valid(v->output,0) || !v->demand_id)
        return SOPHIA_SHELL_INVALID;
    uint8_t p[48]={0}; put_grant(p,v->grant); put_id(p+16,v->output);
    shell_put64(p+32,v->demand_id); shell_put64(p+40,v->permit_id);
    return sophia_shell_frame_encode(dst,capacity,178,tx,p,sizeof(p),bytes);
}
int sophia_shell_content_action_ack_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_content_action_ack *v, size_t *bytes)
{
    if (!dst || !v || !bytes) return SOPHIA_SHELL_ARGUMENT;
    const struct sophia_shell_content_action_identity *a=&v->identity;
    if (!tx || !grant_valid(v->grant) || !id_valid(a->output,0) || !id_valid(a->allocation,0) ||
        !a->candidate_generation || !a->presentation_epoch || !a->interaction_generation ||
        !a->event_id || v->disposition<1 || v->disposition>2 ||
        !((a->target_id && a->target_generation && a->action_id) ||
          (!a->target_id && !a->target_generation && !a->action_id))) return SOPHIA_SHELL_INVALID;
    uint8_t p[112]={0}; put_grant(p,v->grant); put_id(p+16,a->output);
    shell_put64(p+32,a->candidate_generation); shell_put64(p+40,a->presentation_epoch);
    shell_put64(p+48,a->interaction_generation); put_id(p+56,a->allocation);
    shell_put64(p+72,a->target_id); shell_put64(p+80,a->target_generation);
    shell_put64(p+88,a->action_id); shell_put64(p+96,a->event_id); shell_put16(p+104,v->disposition);
    return sophia_shell_frame_encode(dst,capacity,180,tx,p,sizeof(p),bytes);
}
