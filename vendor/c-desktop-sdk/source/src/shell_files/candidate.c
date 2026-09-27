/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_content_surface(const struct sophia_sf_content_surface *v)
{
    if (!v->allocation_id || !v->allocation_generation || !v->scale_generation || v->role < 1 ||
        v->role > 2 || v->edge < 1 || v->edge > 4 || v->margin_top < -512 || v->margin_top > 512 ||
        v->margin_right < -512 || v->margin_right > 512 || v->margin_bottom < -512 ||
        v->margin_bottom > 512 || v->margin_left < -512 || v->margin_left > 512 ||
        v->reservation_extent > 512)
        return -1;
    return 0;
}
void sf_put_content_surface(uint8_t *b, const struct sophia_sf_content_surface *v)
{
    memset(b, 0, 64);
    sf_put(b + 0, (uint64_t)v->allocation_id, 8);
    sf_put(b + 8, (uint64_t)v->allocation_generation, 8);
    sf_put(b + 16, (uint64_t)v->scale_generation, 8);
    sf_put(b + 24, (uint64_t)v->role, 2);
    sf_put(b + 26, (uint64_t)v->edge, 2);
    sf_put(b + 28, (uint64_t)v->margin_top, 2);
    sf_put(b + 30, (uint64_t)v->margin_right, 2);
    sf_put(b + 32, (uint64_t)v->margin_bottom, 2);
    sf_put(b + 34, (uint64_t)v->margin_left, 2);
    sf_put(b + 36, (uint64_t)v->reservation_extent, 4);
    sf_put(b + 40, (uint64_t)v->parent_surface_index, 2);
    sf_put(b + 44, (uint64_t)v->anchor_x, 4);
    sf_put(b + 48, (uint64_t)v->anchor_y, 4);
    sf_put(b + 52, (uint64_t)v->anchor_width, 4);
    sf_put(b + 56, (uint64_t)v->anchor_height, 4);
}
int sf_take_content_surface(const uint8_t *b, struct sophia_sf_content_surface *v)
{
    v->allocation_id = (uint64_t)sf_get(b + 0, 8);
    v->allocation_generation = (uint64_t)sf_get(b + 8, 8);
    v->scale_generation = (uint64_t)sf_get(b + 16, 8);
    v->role = (uint16_t)sf_get(b + 24, 2);
    v->edge = (uint16_t)sf_get(b + 26, 2);
    v->margin_top = (int16_t)sf_signed(b + 28, 2);
    v->margin_right = (int16_t)sf_signed(b + 30, 2);
    v->margin_bottom = (int16_t)sf_signed(b + 32, 2);
    v->margin_left = (int16_t)sf_signed(b + 34, 2);
    v->reservation_extent = (uint32_t)sf_get(b + 36, 4);
    v->parent_surface_index = (uint16_t)sf_get(b + 40, 2);
    if (!sf_zero(b + 42, 2))
        return -1;
    v->anchor_x = (int32_t)sf_signed(b + 44, 4);
    v->anchor_y = (int32_t)sf_signed(b + 48, 4);
    v->anchor_width = (uint32_t)sf_get(b + 52, 4);
    v->anchor_height = (uint32_t)sf_get(b + 56, 4);
    if (!sf_zero(b + 60, 4))
        return -1;
    return sf_check_content_surface(v);
}
int sf_check_content_placement(const struct sophia_sf_content_placement *v)
{
    if (!v->resource_id || !v->resource_generation || v->surface_index > 7 ||
        v->destination_x_px < 0 || v->destination_y_px < 0)
        return -1;
    return 0;
}
void sf_put_content_placement(uint8_t *b, const struct sophia_sf_content_placement *v)
{
    memset(b, 0, 32);
    sf_put(b + 0, (uint64_t)v->resource_id, 8);
    sf_put(b + 8, (uint64_t)v->resource_generation, 8);
    sf_put(b + 16, (uint64_t)v->surface_index, 2);
    sf_put(b + 20, (uint64_t)v->destination_x_px, 4);
    sf_put(b + 24, (uint64_t)v->destination_y_px, 4);
}
int sf_take_content_placement(const uint8_t *b, struct sophia_sf_content_placement *v)
{
    v->resource_id = (uint64_t)sf_get(b + 0, 8);
    v->resource_generation = (uint64_t)sf_get(b + 8, 8);
    v->surface_index = (uint16_t)sf_get(b + 16, 2);
    if (!sf_zero(b + 18, 2))
        return -1;
    v->destination_x_px = (int32_t)sf_signed(b + 20, 4);
    v->destination_y_px = (int32_t)sf_signed(b + 24, 4);
    if (!sf_zero(b + 28, 4))
        return -1;
    return sf_check_content_placement(v);
}
int sf_check_content_target(const struct sophia_sf_content_target *v)
{
    if (v->surface_index > 7 || v->action_kind != 1 || !v->target_id || !v->target_generation ||
        !v->action_id || v->bounds_x < 0 || v->bounds_y < 0 || !v->bounds_width ||
        !v->bounds_height)
        return -1;
    return 0;
}
void sf_put_content_target(uint8_t *b, const struct sophia_sf_content_target *v)
{
    memset(b, 0, 48);
    sf_put(b + 0, (uint64_t)v->surface_index, 2);
    sf_put(b + 2, (uint64_t)v->action_kind, 2);
    sf_put(b + 4, (uint64_t)v->target_id, 8);
    sf_put(b + 12, (uint64_t)v->target_generation, 8);
    sf_put(b + 20, (uint64_t)v->action_id, 8);
    sf_put(b + 28, (uint64_t)v->bounds_x, 4);
    sf_put(b + 32, (uint64_t)v->bounds_y, 4);
    sf_put(b + 36, (uint64_t)v->bounds_width, 4);
    sf_put(b + 40, (uint64_t)v->bounds_height, 4);
}
int sf_take_content_target(const uint8_t *b, struct sophia_sf_content_target *v)
{
    v->surface_index = (uint16_t)sf_get(b + 0, 2);
    v->action_kind = (uint16_t)sf_get(b + 2, 2);
    v->target_id = (uint64_t)sf_get(b + 4, 8);
    v->target_generation = (uint64_t)sf_get(b + 12, 8);
    v->action_id = (uint64_t)sf_get(b + 20, 8);
    v->bounds_x = (int32_t)sf_signed(b + 28, 4);
    v->bounds_y = (int32_t)sf_signed(b + 32, 4);
    v->bounds_width = (uint32_t)sf_get(b + 36, 4);
    v->bounds_height = (uint32_t)sf_get(b + 40, 4);
    if (!sf_zero(b + 44, 4))
        return -1;
    return sf_check_content_target(v);
}
int sf_check_candidate(const struct sophia_sf_candidate *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->candidate_generation || !v->output_id || !v->output_generation ||
        !v->facts_generation || !v->pacing_permit || !v->interaction_generation ||
        v->surface_count > 8 || v->placement_count > 32 || v->target_count > 64)
        return -1;
    return 0;
}
void sf_put_candidate(uint8_t *b, const struct sophia_sf_candidate *v)
{
    memset(b, 0, 80);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->candidate_generation, 8);
    sf_put(b + 32, (uint64_t)v->output_id, 8);
    sf_put(b + 40, (uint64_t)v->output_generation, 8);
    sf_put(b + 48, (uint64_t)v->facts_generation, 8);
    sf_put(b + 56, (uint64_t)v->pacing_permit, 8);
    sf_put(b + 64, (uint64_t)v->interaction_generation, 8);
    sf_put(b + 72, (uint64_t)v->surface_count, 2);
    sf_put(b + 74, (uint64_t)v->placement_count, 2);
    sf_put(b + 76, (uint64_t)v->target_count, 2);
}
int sf_take_candidate(const uint8_t *b, struct sophia_sf_candidate *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->candidate_generation = (uint64_t)sf_get(b + 24, 8);
    v->output_id = (uint64_t)sf_get(b + 32, 8);
    v->output_generation = (uint64_t)sf_get(b + 40, 8);
    v->facts_generation = (uint64_t)sf_get(b + 48, 8);
    v->pacing_permit = (uint64_t)sf_get(b + 56, 8);
    v->interaction_generation = (uint64_t)sf_get(b + 64, 8);
    v->surface_count = (uint16_t)sf_get(b + 72, 2);
    v->placement_count = (uint16_t)sf_get(b + 74, 2);
    v->target_count = (uint16_t)sf_get(b + 76, 2);
    if (!sf_zero(b + 78, 2))
        return -1;
    return sf_check_candidate(v);
}
