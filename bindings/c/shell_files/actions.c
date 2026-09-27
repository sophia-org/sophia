/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_frame_demand(const struct sophia_sf_frame_demand *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || !v->output_id ||
        !v->output_generation || !v->demand_id || v->reason < 1 || v->reason > 3)
        return -1;
    return 0;
}
void sf_put_frame_demand(uint8_t *b, const struct sophia_sf_frame_demand *v)
{
    memset(b, 0, 66);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->output_id, 8);
    sf_put(b + 32, (uint64_t)v->output_generation, 8);
    sf_put(b + 40, (uint64_t)v->allocation_id, 8);
    sf_put(b + 48, (uint64_t)v->allocation_generation, 8);
    sf_put(b + 56, (uint64_t)v->demand_id, 8);
    sf_put(b + 64, (uint64_t)v->reason, 2);
}
int sf_take_frame_demand(const uint8_t *b, struct sophia_sf_frame_demand *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->output_id = (uint64_t)sf_get(b + 24, 8);
    v->output_generation = (uint64_t)sf_get(b + 32, 8);
    v->allocation_id = (uint64_t)sf_get(b + 40, 8);
    v->allocation_generation = (uint64_t)sf_get(b + 48, 8);
    v->demand_id = (uint64_t)sf_get(b + 56, 8);
    v->reason = (uint16_t)sf_get(b + 64, 2);
    return sf_check_frame_demand(v);
}
int sf_check_frame_demand_cancel(const struct sophia_sf_frame_demand_cancel *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || !v->output_id ||
        !v->output_generation || !v->demand_id)
        return -1;
    return 0;
}
void sf_put_frame_demand_cancel(uint8_t *b, const struct sophia_sf_frame_demand_cancel *v)
{
    memset(b, 0, 56);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->output_id, 8);
    sf_put(b + 32, (uint64_t)v->output_generation, 8);
    sf_put(b + 40, (uint64_t)v->demand_id, 8);
    sf_put(b + 48, (uint64_t)v->permit_id, 8);
}
int sf_take_frame_demand_cancel(const uint8_t *b, struct sophia_sf_frame_demand_cancel *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->output_id = (uint64_t)sf_get(b + 24, 8);
    v->output_generation = (uint64_t)sf_get(b + 32, 8);
    v->demand_id = (uint64_t)sf_get(b + 40, 8);
    v->permit_id = (uint64_t)sf_get(b + 48, 8);
    return sf_check_frame_demand_cancel(v);
}
int sf_check_frame_permit(const struct sophia_sf_frame_permit *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || !v->output_id ||
        !v->output_generation || !v->demand_id || v->state < 1 || v->state > 4 || v->reason > 12 ||
        (v->state == 1 && v->ttl_ms > 250) || v->max_candidate_bytes > 8192)
        return -1;
    return 0;
}
void sf_put_frame_permit(uint8_t *b, const struct sophia_sf_frame_permit *v)
{
    memset(b, 0, 72);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->output_id, 8);
    sf_put(b + 32, (uint64_t)v->output_generation, 8);
    sf_put(b + 40, (uint64_t)v->demand_id, 8);
    sf_put(b + 48, (uint64_t)v->permit_id, 8);
    sf_put(b + 56, (uint64_t)v->state, 2);
    sf_put(b + 58, (uint64_t)v->reason, 2);
    sf_put(b + 60, (uint64_t)v->ttl_ms, 4);
    sf_put(b + 64, (uint64_t)v->max_candidate_bytes, 4);
}
int sf_take_frame_permit(const uint8_t *b, struct sophia_sf_frame_permit *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->output_id = (uint64_t)sf_get(b + 24, 8);
    v->output_generation = (uint64_t)sf_get(b + 32, 8);
    v->demand_id = (uint64_t)sf_get(b + 40, 8);
    v->permit_id = (uint64_t)sf_get(b + 48, 8);
    v->state = (uint16_t)sf_get(b + 56, 2);
    v->reason = (uint16_t)sf_get(b + 58, 2);
    v->ttl_ms = (uint32_t)sf_get(b + 60, 4);
    v->max_candidate_bytes = (uint32_t)sf_get(b + 64, 4);
    if (!sf_zero(b + 68, 4))
        return -1;
    return sf_check_frame_permit(v);
}
int sf_check_action(const struct sophia_sf_action *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || !v->output_id ||
        !v->output_generation || !v->candidate_generation || !v->presentation_epoch ||
        !v->interaction_generation || !v->allocation_id || !v->allocation_generation ||
        !v->event_id || v->kind < 1 || v->kind > 3 || v->reason > 12)
        return -1;
    return 0;
}
void sf_put_action(uint8_t *b, const struct sophia_sf_action *v)
{
    memset(b, 0, 120);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->output_id, 8);
    sf_put(b + 32, (uint64_t)v->output_generation, 8);
    sf_put(b + 40, (uint64_t)v->candidate_generation, 8);
    sf_put(b + 48, (uint64_t)v->presentation_epoch, 8);
    sf_put(b + 56, (uint64_t)v->interaction_generation, 8);
    sf_put(b + 64, (uint64_t)v->allocation_id, 8);
    sf_put(b + 72, (uint64_t)v->allocation_generation, 8);
    sf_put(b + 80, (uint64_t)v->target_id, 8);
    sf_put(b + 88, (uint64_t)v->target_generation, 8);
    sf_put(b + 96, (uint64_t)v->action_id, 8);
    sf_put(b + 104, (uint64_t)v->event_id, 8);
    sf_put(b + 112, (uint64_t)v->kind, 2);
    sf_put(b + 114, (uint64_t)v->reason, 2);
}
int sf_take_action(const uint8_t *b, struct sophia_sf_action *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->output_id = (uint64_t)sf_get(b + 24, 8);
    v->output_generation = (uint64_t)sf_get(b + 32, 8);
    v->candidate_generation = (uint64_t)sf_get(b + 40, 8);
    v->presentation_epoch = (uint64_t)sf_get(b + 48, 8);
    v->interaction_generation = (uint64_t)sf_get(b + 56, 8);
    v->allocation_id = (uint64_t)sf_get(b + 64, 8);
    v->allocation_generation = (uint64_t)sf_get(b + 72, 8);
    v->target_id = (uint64_t)sf_get(b + 80, 8);
    v->target_generation = (uint64_t)sf_get(b + 88, 8);
    v->action_id = (uint64_t)sf_get(b + 96, 8);
    v->event_id = (uint64_t)sf_get(b + 104, 8);
    v->kind = (uint16_t)sf_get(b + 112, 2);
    v->reason = (uint16_t)sf_get(b + 114, 2);
    if (!sf_zero(b + 116, 4))
        return -1;
    return sf_check_action(v);
}
int sf_check_action_ack(const struct sophia_sf_action_ack *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || !v->output_id ||
        !v->output_generation || !v->candidate_generation || !v->presentation_epoch ||
        !v->interaction_generation || !v->allocation_id || !v->allocation_generation ||
        !v->event_id || v->disposition < 1 || v->disposition > 2)
        return -1;
    return 0;
}
void sf_put_action_ack(uint8_t *b, const struct sophia_sf_action_ack *v)
{
    memset(b, 0, 120);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->output_id, 8);
    sf_put(b + 32, (uint64_t)v->output_generation, 8);
    sf_put(b + 40, (uint64_t)v->candidate_generation, 8);
    sf_put(b + 48, (uint64_t)v->presentation_epoch, 8);
    sf_put(b + 56, (uint64_t)v->interaction_generation, 8);
    sf_put(b + 64, (uint64_t)v->allocation_id, 8);
    sf_put(b + 72, (uint64_t)v->allocation_generation, 8);
    sf_put(b + 80, (uint64_t)v->target_id, 8);
    sf_put(b + 88, (uint64_t)v->target_generation, 8);
    sf_put(b + 96, (uint64_t)v->action_id, 8);
    sf_put(b + 104, (uint64_t)v->event_id, 8);
    sf_put(b + 112, (uint64_t)v->disposition, 2);
}
int sf_take_action_ack(const uint8_t *b, struct sophia_sf_action_ack *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->output_id = (uint64_t)sf_get(b + 24, 8);
    v->output_generation = (uint64_t)sf_get(b + 32, 8);
    v->candidate_generation = (uint64_t)sf_get(b + 40, 8);
    v->presentation_epoch = (uint64_t)sf_get(b + 48, 8);
    v->interaction_generation = (uint64_t)sf_get(b + 56, 8);
    v->allocation_id = (uint64_t)sf_get(b + 64, 8);
    v->allocation_generation = (uint64_t)sf_get(b + 72, 8);
    v->target_id = (uint64_t)sf_get(b + 80, 8);
    v->target_generation = (uint64_t)sf_get(b + 88, 8);
    v->action_id = (uint64_t)sf_get(b + 96, 8);
    v->event_id = (uint64_t)sf_get(b + 104, 8);
    v->disposition = (uint16_t)sf_get(b + 112, 2);
    if (!sf_zero(b + 114, 2))
        return -1;
    if (!sf_zero(b + 116, 4))
        return -1;
    return sf_check_action_ack(v);
}
int sf_check_candidate_outcome(const struct sophia_sf_candidate_outcome *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->candidate_generation || !v->output_id || !v->output_generation || v->kind < 1 ||
        v->kind > 4 || v->reason > 12)
        return -1;
    return 0;
}
void sf_put_candidate_outcome(uint8_t *b, const struct sophia_sf_candidate_outcome *v)
{
    memset(b, 0, 76);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->candidate_generation, 8);
    sf_put(b + 32, (uint64_t)v->output_id, 8);
    sf_put(b + 40, (uint64_t)v->output_generation, 8);
    sf_put(b + 48, (uint64_t)v->kind, 2);
    sf_put(b + 50, (uint64_t)v->reason, 2);
    sf_put(b + 52, (uint64_t)v->presentation_epoch, 8);
    sf_put(b + 60, (uint64_t)v->work_area_generation, 8);
    sf_put(b + 68, (uint64_t)v->wm_commit_generation, 8);
}
int sf_take_candidate_outcome(const uint8_t *b, struct sophia_sf_candidate_outcome *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->candidate_generation = (uint64_t)sf_get(b + 24, 8);
    v->output_id = (uint64_t)sf_get(b + 32, 8);
    v->output_generation = (uint64_t)sf_get(b + 40, 8);
    v->kind = (uint16_t)sf_get(b + 48, 2);
    v->reason = (uint16_t)sf_get(b + 50, 2);
    v->presentation_epoch = (uint64_t)sf_get(b + 52, 8);
    v->work_area_generation = (uint64_t)sf_get(b + 60, 8);
    v->wm_commit_generation = (uint64_t)sf_get(b + 68, 8);
    return sf_check_candidate_outcome(v);
}
