/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_resource_begin(const struct sophia_sf_resource_begin *v)
{
    if (!v->transaction || v->slot > 3 || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->resource_id || !v->resource_generation || !v->width_px || v->width_px > 8192 ||
        !v->height_px || v->height_px > 4096 || v->rendered_scale_numerator < 1 ||
        v->rendered_scale_numerator > 32 || v->rendered_scale_denominator < 1 ||
        v->rendered_scale_denominator > 4 || v->pixel_format != 1)
        return -1;
    return 0;
}
void sf_put_resource_begin(uint8_t *b, const struct sophia_sf_resource_begin *v)
{
    memset(b, 0, 80);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->slot, 2);
    sf_put(b + 16, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 24, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 32, (uint64_t)v->resource_id, 8);
    sf_put(b + 40, (uint64_t)v->resource_generation, 8);
    sf_put(b + 48, (uint64_t)v->width_px, 4);
    sf_put(b + 52, (uint64_t)v->height_px, 4);
    sf_put(b + 56, (uint64_t)v->rendered_scale_numerator, 4);
    sf_put(b + 60, (uint64_t)v->rendered_scale_denominator, 4);
    sf_put(b + 64, (uint64_t)v->pixel_format, 2);
    sf_put(b + 68, (uint64_t)v->chunk_count, 4);
    sf_put(b + 72, (uint64_t)v->total_bytes, 8);
}
int sf_take_resource_begin(const uint8_t *b, struct sophia_sf_resource_begin *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->slot = (uint16_t)sf_get(b + 8, 2);
    if (!sf_zero(b + 10, 6))
        return -1;
    v->grant_connection_epoch = (uint64_t)sf_get(b + 16, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 24, 8);
    v->resource_id = (uint64_t)sf_get(b + 32, 8);
    v->resource_generation = (uint64_t)sf_get(b + 40, 8);
    v->width_px = (uint32_t)sf_get(b + 48, 4);
    v->height_px = (uint32_t)sf_get(b + 52, 4);
    v->rendered_scale_numerator = (uint32_t)sf_get(b + 56, 4);
    v->rendered_scale_denominator = (uint32_t)sf_get(b + 60, 4);
    v->pixel_format = (uint16_t)sf_get(b + 64, 2);
    if (!sf_zero(b + 66, 2))
        return -1;
    v->chunk_count = (uint32_t)sf_get(b + 68, 4);
    v->total_bytes = (uint64_t)sf_get(b + 72, 8);
    return sf_check_resource_begin(v);
}
int sf_check_resource_end(const struct sophia_sf_resource_end *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->resource_id || !v->resource_generation || !v->total_bytes || v->total_bytes > 4194304 ||
        !v->chunk_count || v->chunk_count > 4096)
        return -1;
    return 0;
}
void sf_put_resource_end(uint8_t *b, const struct sophia_sf_resource_end *v)
{
    memset(b, 0, 56);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->resource_id, 8);
    sf_put(b + 32, (uint64_t)v->resource_generation, 8);
    sf_put(b + 40, (uint64_t)v->total_bytes, 8);
    sf_put(b + 48, (uint64_t)v->chunk_count, 4);
}
int sf_take_resource_end(const uint8_t *b, struct sophia_sf_resource_end *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->resource_id = (uint64_t)sf_get(b + 24, 8);
    v->resource_generation = (uint64_t)sf_get(b + 32, 8);
    v->total_bytes = (uint64_t)sf_get(b + 40, 8);
    v->chunk_count = (uint32_t)sf_get(b + 48, 4);
    if (!sf_zero(b + 52, 4))
        return -1;
    return sf_check_resource_end(v);
}
int sf_check_resource_cancel(const struct sophia_sf_resource_cancel *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->resource_id || !v->resource_generation)
        return -1;
    return 0;
}
void sf_put_resource_cancel(uint8_t *b, const struct sophia_sf_resource_cancel *v)
{
    memset(b, 0, 40);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->resource_id, 8);
    sf_put(b + 32, (uint64_t)v->resource_generation, 8);
}
int sf_take_resource_cancel(const uint8_t *b, struct sophia_sf_resource_cancel *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->resource_id = (uint64_t)sf_get(b + 24, 8);
    v->resource_generation = (uint64_t)sf_get(b + 32, 8);
    return sf_check_resource_cancel(v);
}
int sf_check_resource_retire(const struct sophia_sf_resource_retire *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->resource_id || !v->resource_generation)
        return -1;
    return 0;
}
void sf_put_resource_retire(uint8_t *b, const struct sophia_sf_resource_retire *v)
{
    memset(b, 0, 40);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->resource_id, 8);
    sf_put(b + 32, (uint64_t)v->resource_generation, 8);
}
int sf_take_resource_retire(const uint8_t *b, struct sophia_sf_resource_retire *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->resource_id = (uint64_t)sf_get(b + 24, 8);
    v->resource_generation = (uint64_t)sf_get(b + 32, 8);
    return sf_check_resource_retire(v);
}
int sf_check_resource_status(const struct sophia_sf_resource_status *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->resource_id || !v->resource_generation || v->status < 1 || v->status > 4 ||
        v->reason > 12 || v->admitted_bytes > 4194304)
        return -1;
    return 0;
}
void sf_put_resource_status(uint8_t *b, const struct sophia_sf_resource_status *v)
{
    memset(b, 0, 56);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->resource_id, 8);
    sf_put(b + 32, (uint64_t)v->resource_generation, 8);
    sf_put(b + 40, (uint64_t)v->status, 2);
    sf_put(b + 42, (uint64_t)v->reason, 2);
    sf_put(b + 44, (uint64_t)v->next_ordinal, 4);
    sf_put(b + 48, (uint64_t)v->admitted_bytes, 8);
}
int sf_take_resource_status(const uint8_t *b, struct sophia_sf_resource_status *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->resource_id = (uint64_t)sf_get(b + 24, 8);
    v->resource_generation = (uint64_t)sf_get(b + 32, 8);
    v->status = (uint16_t)sf_get(b + 40, 2);
    v->reason = (uint16_t)sf_get(b + 42, 2);
    v->next_ordinal = (uint32_t)sf_get(b + 44, 4);
    v->admitted_bytes = (uint64_t)sf_get(b + 48, 8);
    return sf_check_resource_status(v);
}
int sf_check_resource_released(const struct sophia_sf_resource_released *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->resource_id || !v->resource_generation || v->reason > 12)
        return -1;
    return 0;
}
void sf_put_resource_released(uint8_t *b, const struct sophia_sf_resource_released *v)
{
    memset(b, 0, 42);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->resource_id, 8);
    sf_put(b + 32, (uint64_t)v->resource_generation, 8);
    sf_put(b + 40, (uint64_t)v->reason, 2);
}
int sf_take_resource_released(const uint8_t *b, struct sophia_sf_resource_released *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->resource_id = (uint64_t)sf_get(b + 24, 8);
    v->resource_generation = (uint64_t)sf_get(b + 32, 8);
    v->reason = (uint16_t)sf_get(b + 40, 2);
    return sf_check_resource_released(v);
}
