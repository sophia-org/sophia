#include "descriptors_internal.h"

static int entry_check(const struct sophia_sf_reference_entry *v)
{
    return !v->slot || !v->key.size || !v->label.size || sf_launcher_text_check(v->key, 64) ||
                   sf_launcher_text_check(v->label, 128)
               ? -1
               : 0;
}
int sophia_sf_reference_entry_encode(uint8_t b[204], const struct sophia_sf_reference_entry *v)
{
    if (!b || !v)
        return -4;
    if (entry_check(v))
        return -1;
    sf_put(b, v->slot, 2);
    sf_put(b + 2, 0, 2);
    sf_text_put(b + 4, 64, v->key);
    sf_text_put(b + 72, 128, v->label);
    return 0;
}
int sophia_sf_reference_entry_decode(const uint8_t b[204], struct sophia_sf_reference_entry *out)
{
    struct sophia_sf_reference_entry v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.slot = (uint16_t)sf_get(b, 2);
    if (!sf_zero(b + 2, 2) || sf_text_take(b + 4, 64, &v.key) ||
        sf_text_take(b + 72, 128, &v.label) || entry_check(&v))
        return -1;
    *out = v;
    return 0;
}
int sophia_sf_reference_entry_at(const struct sophia_sf_reference_candidate *v, size_t i,
                                 struct sophia_sf_reference_entry *out)
{
    if (!v || !out || !v->rows || v->entry_count > 256 || i >= v->entry_count ||
        v->rows_bytes != (size_t)v->entry_count * 204)
        return -4;
    return sophia_sf_reference_entry_decode(v->rows + i * 204, out);
}
static int style_check(const struct sophia_sf_reference_style *v)
{
    size_t i;
    if (v->body_size < 8 || v->body_size > 32 || v->title_size < 8 || v->title_size > 48 ||
        v->padding > 64 || v->row_gap > 32 || v->key_gap > 64 || v->column_gap > 64 ||
        v->border > 16 || v->margin > 128 || v->columns < 1 || v->columns > 4 || !v->title.size ||
        sf_launcher_text_check(v->title, 128))
        return -1;
    for (i = 1; i < 6; i++)
        if (v->colors[i] >> 24 != 255)
            return -1;
    return 0;
}
int sf_reference_candidate_check(const struct sophia_sf_reference_candidate *v, uint64_t epoch)
{
    uint8_t seen[8192] = {0};
    size_t i;
    if (!v->transaction || v->connection_epoch != epoch || !v->catalog_generation ||
        !v->request_generation || !v->candidate_generation || !v->output_id || v->visible > 1 ||
        v->entry_count > 256 || v->rows_bytes != (size_t)v->entry_count * 204 ||
        (v->rows_bytes && !v->rows) || style_check(&v->style))
        return -1;
    for (i = 0; i < v->entry_count; i++) {
        struct sophia_sf_reference_entry e;
        unsigned byte, mask;
        if (sophia_sf_reference_entry_at(v, i, &e))
            return -1;
        byte = e.slot / 8;
        mask = 1u << (e.slot % 8);
        if (seen[byte] & mask)
            return -1;
        seen[byte] |= (uint8_t)mask;
    }
    return 0;
}
void sf_reference_candidate_put(uint8_t *b, const struct sophia_sf_reference_candidate *v)
{
    const struct sophia_sf_reference_style *s = &v->style;
    size_t i;
    sf_put(b, v->transaction, 8);
    sf_put(b + 8, v->connection_epoch, 8);
    sf_put(b + 16, v->catalog_generation, 8);
    sf_put(b + 24, v->request_generation, 8);
    sf_put(b + 32, v->candidate_generation, 8);
    sf_put(b + 40, v->output_id, 8);
    sf_put(b + 48, v->visible, 2);
    sf_put(b + 50, v->page, 2);
    sf_put(b + 52, v->entry_count, 2);
    sf_put(b + 54, 0, 2);
    sf_put(b + 56, s->body_size, 2);
    sf_put(b + 58, s->title_size, 2);
    sf_put(b + 60, s->padding, 2);
    sf_put(b + 62, s->row_gap, 2);
    sf_put(b + 64, s->key_gap, 2);
    sf_put(b + 66, s->column_gap, 2);
    sf_put(b + 68, s->border, 2);
    sf_put(b + 70, s->margin, 2);
    sf_put(b + 72, s->columns, 2);
    sf_put(b + 74, 0, 2);
    for (i = 0; i < 6; i++)
        sf_put(b + 76 + 4 * i, s->colors[i], 4);
    sf_text_put(b + 100, 128, s->title);
    if (v->rows_bytes)
        memcpy(b + 232, v->rows, v->rows_bytes);
}
int sf_reference_candidate_take(const uint8_t *b, size_t n, struct sophia_sf_reference_candidate *v)
{
    struct sophia_sf_reference_style *s = &v->style;
    size_t i;
    if (n < 232 || !sf_zero(b + 54, 2) || !sf_zero(b + 74, 2))
        return -1;
    v->transaction = sf_get(b, 8);
    v->connection_epoch = sf_get(b + 8, 8);
    v->catalog_generation = sf_get(b + 16, 8);
    v->request_generation = sf_get(b + 24, 8);
    v->candidate_generation = sf_get(b + 32, 8);
    v->output_id = sf_get(b + 40, 8);
    v->visible = (uint16_t)sf_get(b + 48, 2);
    v->page = (uint16_t)sf_get(b + 50, 2);
    v->entry_count = (uint16_t)sf_get(b + 52, 2);
    s->body_size = (uint16_t)sf_get(b + 56, 2);
    s->title_size = (uint16_t)sf_get(b + 58, 2);
    s->padding = (uint16_t)sf_get(b + 60, 2);
    s->row_gap = (uint16_t)sf_get(b + 62, 2);
    s->key_gap = (uint16_t)sf_get(b + 64, 2);
    s->column_gap = (uint16_t)sf_get(b + 66, 2);
    s->border = (uint16_t)sf_get(b + 68, 2);
    s->margin = (uint16_t)sf_get(b + 70, 2);
    s->columns = (uint16_t)sf_get(b + 72, 2);
    for (i = 0; i < 6; i++)
        s->colors[i] = (uint32_t)sf_get(b + 76 + 4 * i, 4);
    if (sf_text_take(b + 100, 128, &s->title))
        return -1;
    v->rows = b + 232;
    v->rows_bytes = n - 232;
    return 0;
}
