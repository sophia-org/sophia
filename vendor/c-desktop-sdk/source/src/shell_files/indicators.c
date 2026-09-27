#include "roles_internal.h"

int sophia_sf_indicator_status_encode(uint8_t b[46],
                                      const struct sophia_sf_indicator_output_status *v)
{
    if (!b || !v)
        return -4;
    if (sf_text_check(v->layout, 32))
        return -1;
    sf_put(b, v->output_id, 8);
    sf_put(b + 8, v->focus_bits, 2);
    sf_text_put(b + 10, 32, v->layout);
    return 0;
}
int sophia_sf_indicator_status_decode(const uint8_t b[46],
                                      struct sophia_sf_indicator_output_status *out)
{
    struct sophia_sf_indicator_output_status v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.output_id = sf_get(b, 8);
    v.focus_bits = (uint16_t)sf_get(b + 8, 2);
    if (sf_text_take(b + 10, 32, &v.layout))
        return -1;
    *out = v;
    return 0;
}
int sophia_sf_indicator_entry_encode(uint8_t b[66], const struct sophia_sf_indicator_entry *v)
{
    if (!b || !v)
        return -4;
    if (sf_text_check(v->label, 32))
        return -1;
    sf_put(b, v->output_id, 8);
    sf_put(b + 8, v->indicator, 8);
    sf_put(b + 16, v->action, 8);
    sf_put(b + 24, v->slot, 4);
    sf_put(b + 28, v->state_bits, 2);
    sf_text_put(b + 30, 32, v->label);
    return 0;
}
int sophia_sf_indicator_entry_decode(const uint8_t b[66], struct sophia_sf_indicator_entry *out)
{
    struct sophia_sf_indicator_entry v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.output_id = sf_get(b, 8);
    v.indicator = sf_get(b + 8, 8);
    v.action = sf_get(b + 16, 8);
    v.slot = (uint32_t)sf_get(b + 24, 4);
    v.state_bits = (uint16_t)sf_get(b + 28, 2);
    if (sf_text_take(b + 30, 32, &v.label))
        return -1;
    *out = v;
    return 0;
}
int sf_indicators_check(const struct sophia_sf_indicators *v)
{
    size_t i, at = 0;
    if (!v->transaction || v->active_output_present > 1 ||
        (!!v->active_output_id) != (!!v->active_output_present) || v->status_count > 16 ||
        v->indicator_count > 256 ||
        v->rows_bytes != (size_t)v->status_count * 46 + (size_t)v->indicator_count * 66 ||
        (v->rows_bytes && !v->rows))
        return -1;
    for (i = 0; i < v->status_count; i++, at += 46) {
        struct sophia_sf_indicator_output_status row;
        if (sophia_sf_indicator_status_decode(v->rows + at, &row))
            return -1;
    }
    for (i = 0; i < v->indicator_count; i++, at += 66) {
        struct sophia_sf_indicator_entry row;
        if (sophia_sf_indicator_entry_decode(v->rows + at, &row))
            return -1;
    }
    return 0;
}
void sf_indicators_put(uint8_t *b, const struct sophia_sf_indicators *v)
{
    sf_put(b, v->transaction, 8);
    sf_put(b + 8, v->connection_epoch, 8);
    sf_put(b + 16, v->generation, 8);
    sf_put(b + 24, v->active_output_id, 8);
    sf_put(b + 32, v->active_output_present, 2);
    sf_put(b + 34, v->status_count, 2);
    sf_put(b + 36, v->indicator_count, 2);
    sf_put(b + 38, 0, 2);
    if (v->rows_bytes)
        memcpy(b + 40, v->rows, v->rows_bytes);
}
int sf_indicators_take(const uint8_t *b, size_t n, struct sophia_sf_indicators *v)
{
    if (n < 40 || !sf_zero(b + 38, 2))
        return -1;
    v->transaction = sf_get(b, 8);
    v->connection_epoch = sf_get(b + 8, 8);
    v->generation = sf_get(b + 16, 8);
    v->active_output_id = sf_get(b + 24, 8);
    v->active_output_present = (uint16_t)sf_get(b + 32, 2);
    v->status_count = (uint16_t)sf_get(b + 34, 2);
    v->indicator_count = (uint16_t)sf_get(b + 36, 2);
    v->rows = b + 40;
    v->rows_bytes = n - 40;
    return sf_indicators_check(v);
}
