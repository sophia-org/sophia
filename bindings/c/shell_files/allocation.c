/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_allocation_request(const struct sophia_sf_allocation_request *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || !v->output_id ||
        !v->output_generation || !v->allocation_request_id || v->operation < 1 ||
        v->operation > 3 || v->role < 1 || v->role > 2 || v->edge < 1 || v->edge > 4 ||
        v->margin_top < -512 || v->margin_top > 512 || v->margin_right < -512 ||
        v->margin_right > 512 || v->margin_bottom < -512 || v->margin_bottom > 512 ||
        v->margin_left < -512 || v->margin_left > 512)
        return -1;
    return 0;
}
void sf_put_allocation_request(uint8_t *b, const struct sophia_sf_allocation_request *v)
{
    memset(b, 0, 128);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->output_id, 8);
    sf_put(b + 32, (uint64_t)v->output_generation, 8);
    sf_put(b + 40, (uint64_t)v->allocation_request_id, 8);
    sf_put(b + 48, (uint64_t)v->operation, 2);
    sf_put(b + 50, (uint64_t)v->role, 2);
    sf_put(b + 52, (uint64_t)v->edge, 2);
    sf_put(b + 56, (uint64_t)v->prior_id, 8);
    sf_put(b + 64, (uint64_t)v->prior_generation, 8);
    sf_put(b + 72, (uint64_t)v->parent_id, 8);
    sf_put(b + 80, (uint64_t)v->parent_generation, 8);
    sf_put(b + 88, (uint64_t)v->parent_presentation_epoch, 8);
    sf_put(b + 96, (uint64_t)v->anchor_x, 4);
    sf_put(b + 100, (uint64_t)v->anchor_y, 4);
    sf_put(b + 104, (uint64_t)v->anchor_width, 4);
    sf_put(b + 108, (uint64_t)v->anchor_height, 4);
    sf_put(b + 112, (uint64_t)v->desired_width, 4);
    sf_put(b + 116, (uint64_t)v->desired_height, 4);
    sf_put(b + 120, (uint64_t)v->margin_top, 2);
    sf_put(b + 122, (uint64_t)v->margin_right, 2);
    sf_put(b + 124, (uint64_t)v->margin_bottom, 2);
    sf_put(b + 126, (uint64_t)v->margin_left, 2);
}
int sf_take_allocation_request(const uint8_t *b, struct sophia_sf_allocation_request *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->output_id = (uint64_t)sf_get(b + 24, 8);
    v->output_generation = (uint64_t)sf_get(b + 32, 8);
    v->allocation_request_id = (uint64_t)sf_get(b + 40, 8);
    v->operation = (uint16_t)sf_get(b + 48, 2);
    v->role = (uint16_t)sf_get(b + 50, 2);
    v->edge = (uint16_t)sf_get(b + 52, 2);
    if (!sf_zero(b + 54, 2))
        return -1;
    v->prior_id = (uint64_t)sf_get(b + 56, 8);
    v->prior_generation = (uint64_t)sf_get(b + 64, 8);
    v->parent_id = (uint64_t)sf_get(b + 72, 8);
    v->parent_generation = (uint64_t)sf_get(b + 80, 8);
    v->parent_presentation_epoch = (uint64_t)sf_get(b + 88, 8);
    v->anchor_x = (int32_t)sf_signed(b + 96, 4);
    v->anchor_y = (int32_t)sf_signed(b + 100, 4);
    v->anchor_width = (uint32_t)sf_get(b + 104, 4);
    v->anchor_height = (uint32_t)sf_get(b + 108, 4);
    v->desired_width = (uint32_t)sf_get(b + 112, 4);
    v->desired_height = (uint32_t)sf_get(b + 116, 4);
    v->margin_top = (int16_t)sf_signed(b + 120, 2);
    v->margin_right = (int16_t)sf_signed(b + 122, 2);
    v->margin_bottom = (int16_t)sf_signed(b + 124, 2);
    v->margin_left = (int16_t)sf_signed(b + 126, 2);
    return sf_check_allocation_request(v);
}
int sf_check_allocation_result(const struct sophia_sf_allocation_result *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch || v->status < 1 ||
        v->status > 4 || v->reason > 12 || !v->output_id || !v->output_generation ||
        (v->status == 1 && v->allowed_reservation_extent > 512) ||
        v->margin_top < -512 || v->margin_top > 512 ||
        v->margin_right < -512 || v->margin_right > 512 || v->margin_bottom < -512 ||
        v->margin_bottom > 512 || v->margin_left < -512 || v->margin_left > 512)
        return -1;
    return 0;
}
void sf_put_allocation_result(uint8_t *b, const struct sophia_sf_allocation_result *v)
{
    memset(b, 0, 168);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->allocation_request_id, 8);
    sf_put(b + 32, (uint64_t)v->status, 2);
    sf_put(b + 34, (uint64_t)v->reason, 2);
    sf_put(b + 40, (uint64_t)v->output_id, 8);
    sf_put(b + 48, (uint64_t)v->output_generation, 8);
    sf_put(b + 56, (uint64_t)v->allocation_id, 8);
    sf_put(b + 64, (uint64_t)v->allocation_generation, 8);
    sf_put(b + 72, (uint64_t)v->parent_id, 8);
    sf_put(b + 80, (uint64_t)v->parent_generation, 8);
    sf_put(b + 88, (uint64_t)v->scale_generation, 8);
    sf_put(b + 96, (uint64_t)v->logical_x, 4);
    sf_put(b + 100, (uint64_t)v->logical_y, 4);
    sf_put(b + 104, (uint64_t)v->logical_width, 4);
    sf_put(b + 108, (uint64_t)v->logical_height, 4);
    sf_put(b + 112, (uint64_t)v->pixel_x, 4);
    sf_put(b + 116, (uint64_t)v->pixel_y, 4);
    sf_put(b + 120, (uint64_t)v->pixel_width, 4);
    sf_put(b + 124, (uint64_t)v->pixel_height, 4);
    sf_put(b + 128, (uint64_t)v->scale_numerator, 4);
    sf_put(b + 132, (uint64_t)v->scale_denominator, 4);
    sf_put(b + 136, (uint64_t)v->allowed_reservation_extent, 4);
    sf_put(b + 140, (uint64_t)v->margin_top, 2);
    sf_put(b + 142, (uint64_t)v->margin_right, 2);
    sf_put(b + 144, (uint64_t)v->margin_bottom, 2);
    sf_put(b + 146, (uint64_t)v->margin_left, 2);
    sf_put(b + 148, (uint64_t)v->acknowledged_anchor_x, 4);
    sf_put(b + 152, (uint64_t)v->acknowledged_anchor_y, 4);
    sf_put(b + 156, (uint64_t)v->acknowledged_anchor_width, 4);
    sf_put(b + 160, (uint64_t)v->acknowledged_anchor_height, 4);
}
int sf_take_allocation_result(const uint8_t *b, struct sophia_sf_allocation_result *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->allocation_request_id = (uint64_t)sf_get(b + 24, 8);
    v->status = (uint16_t)sf_get(b + 32, 2);
    v->reason = (uint16_t)sf_get(b + 34, 2);
    if (!sf_zero(b + 36, 4))
        return -1;
    v->output_id = (uint64_t)sf_get(b + 40, 8);
    v->output_generation = (uint64_t)sf_get(b + 48, 8);
    v->allocation_id = (uint64_t)sf_get(b + 56, 8);
    v->allocation_generation = (uint64_t)sf_get(b + 64, 8);
    v->parent_id = (uint64_t)sf_get(b + 72, 8);
    v->parent_generation = (uint64_t)sf_get(b + 80, 8);
    v->scale_generation = (uint64_t)sf_get(b + 88, 8);
    v->logical_x = (int32_t)sf_signed(b + 96, 4);
    v->logical_y = (int32_t)sf_signed(b + 100, 4);
    v->logical_width = (uint32_t)sf_get(b + 104, 4);
    v->logical_height = (uint32_t)sf_get(b + 108, 4);
    v->pixel_x = (int32_t)sf_signed(b + 112, 4);
    v->pixel_y = (int32_t)sf_signed(b + 116, 4);
    v->pixel_width = (uint32_t)sf_get(b + 120, 4);
    v->pixel_height = (uint32_t)sf_get(b + 124, 4);
    v->scale_numerator = (uint32_t)sf_get(b + 128, 4);
    v->scale_denominator = (uint32_t)sf_get(b + 132, 4);
    v->allowed_reservation_extent = (uint32_t)sf_get(b + 136, 4);
    v->margin_top = (int16_t)sf_signed(b + 140, 2);
    v->margin_right = (int16_t)sf_signed(b + 142, 2);
    v->margin_bottom = (int16_t)sf_signed(b + 144, 2);
    v->margin_left = (int16_t)sf_signed(b + 146, 2);
    v->acknowledged_anchor_x = (int32_t)sf_signed(b + 148, 4);
    v->acknowledged_anchor_y = (int32_t)sf_signed(b + 152, 4);
    v->acknowledged_anchor_width = (uint32_t)sf_get(b + 156, 4);
    v->acknowledged_anchor_height = (uint32_t)sf_get(b + 160, 4);
    if (!sf_zero(b + 164, 4))
        return -1;
    return sf_check_allocation_result(v);
}
