/* Native variable-length candidates from spec/proposed/descriptor-layout.kdl. */
#include "descriptors_internal.h"

size_t sf_descriptor_candidate_size(const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_CANDIDATE:
        return 52 + 12u * r->value.descriptor_candidate.entry_count;
    case SOPHIA_SF_TABS_CANDIDATE:
        return 36 + 8u * r->value.tabs_candidate.group_count;
    case SOPHIA_SF_REFERENCE_CANDIDATE:
        return 232 + 204u * r->value.reference_candidate.entry_count;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE:
        return 72 + 2u * r->value.descriptor_launcher_candidate.entry_count;
    default:
        return 0;
    }
}
int sophia_sf_tab_order_encode(uint8_t dst[8], uint64_t slot)
{
    if (!dst)
        return -4;
    if (!slot)
        return -1;
    sf_put(dst, slot, 8);
    return 0;
}
int sophia_sf_tab_order_at(const struct sophia_sf_tabs_candidate *v, size_t i, uint64_t *out)
{
    uint64_t slot;
    if (!v || !out || !v->rows || v->group_count > 1024 || i >= v->group_count ||
        v->rows_bytes != (size_t)v->group_count * 8)
        return -4;
    slot = sf_get(v->rows + i * 8, 8);
    if (!slot)
        return -1;
    *out = slot;
    return 0;
}
static void sift(uint64_t *order, size_t root, size_t count)
{
    while (root * 2 + 1 < count) {
        size_t child = root * 2 + 1;
        uint64_t saved;
        if (child + 1 < count && order[child] < order[child + 1])
            child++;
        if (order[root] >= order[child])
            return;
        saved = order[root];
        order[root] = order[child];
        order[child] = saved;
        root = child;
    }
}
int sf_descriptor_group_order_unique(const uint8_t *rows, size_t count, size_t stride)
{
    uint64_t order[1024];
    size_t i;
    /* Fixed scratch and O(n log n) work even for adversarial u64 group IDs.
     * Sorting this copy must never change the application's requested order. */
    if (count > 1024 || (count && !rows) || (stride != 8 && stride != 24))
        return -1;
    for (i = 0; i < count; i++) {
        order[i] = sf_get(rows + i * stride, 8);
        if (!order[i])
            return -1;
    }
    for (i = count / 2; i > 0; i--)
        sift(order, i - 1, count);
    for (i = count; i > 1; i--) {
        uint64_t saved = order[0];
        order[0] = order[i - 1];
        order[i - 1] = saved;
        sift(order, 0, i - 1);
    }
    for (i = 1; i < count; i++)
        if (order[i - 1] == order[i])
            return -1;
    return 0;
}
static int descriptor_check(const struct sophia_sf_descriptor_candidate *v, uint64_t epoch)
{
    size_t i, j;
    int selected = 0;
    if (!v->transaction || v->connection_epoch != epoch || !v->snapshot_generation ||
        !v->candidate_generation || !v->output_id || v->visible > 1 || v->entry_count > 16 ||
        (!!v->visible) != (v->entry_count != 0) || v->reservation_edge > 4 ||
        v->reservation_thickness > 512 ||
        ((v->reservation_edge == 0) != (v->reservation_thickness == 0)) ||
        (!v->visible && (v->selected_slot || v->reservation_edge)))
        return -1;
    for (i = 0; i < v->entry_count; i++) {
        if (!v->entries[i].slot || !v->entries[i].generation)
            return -1;
        if (v->entries[i].slot == v->selected_slot)
            selected = 1;
        for (j = 0; j < i; j++)
            if (v->entries[i].slot == v->entries[j].slot)
                return -1;
    }
    return v->visible && !selected ? -1 : 0;
}
static int launcher_check(const struct sophia_sf_descriptor_launcher_candidate *v, uint64_t epoch)
{
    size_t i, j;
    int selected = !v->selected;
    if (!v->transaction || v->connection_epoch != epoch || !v->catalog_generation ||
        !v->request_generation || !v->candidate_generation || !v->output_id || v->visible > 1 ||
        v->selected > 4096 || v->entry_count > 32 || v->font_size < 10 || v->font_size > 32)
        return -1;
    for (i = 1; i < 4; i++)
        if (v->colors[i] >> 24 != 255)
            return -1;
    for (i = 0; i < v->entry_count; i++) {
        if (!v->entries[i] || v->entries[i] > 4096)
            return -1;
        if (v->entries[i] == v->selected)
            selected = 1;
        for (j = 0; j < i; j++)
            if (v->entries[i] == v->entries[j])
                return -1;
    }
    /* Visibility and selection are intentionally not coupled in this family. */
    return selected ? 0 : -1;
}
int sf_descriptor_candidate_check(const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_CANDIDATE:
        return descriptor_check(&r->value.descriptor_candidate, r->header.epoch);
    case SOPHIA_SF_TABS_CANDIDATE: {
        const struct sophia_sf_tabs_candidate *v = &r->value.tabs_candidate;
        if (!v->transaction || v->connection_epoch != r->header.epoch || !v->snapshot_generation ||
            !v->candidate_generation || v->group_count > 1024 ||
            v->rows_bytes != (size_t)v->group_count * 8 || (v->rows_bytes && !v->rows))
            return -1;
        return sf_descriptor_group_order_unique(v->rows, v->group_count, 8);
    }
    case SOPHIA_SF_REFERENCE_CANDIDATE:
        return sf_reference_candidate_check(&r->value.reference_candidate, r->header.epoch);
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE:
        return launcher_check(&r->value.descriptor_launcher_candidate, r->header.epoch);
    default:
        return -1;
    }
}
void sf_descriptor_candidate_put(uint8_t *b, const struct sophia_sf_record *r)
{
    size_t i;
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_CANDIDATE: {
        const struct sophia_sf_descriptor_candidate *v = &r->value.descriptor_candidate;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->snapshot_generation, 8);
        sf_put(b + 24, v->candidate_generation, 8);
        sf_put(b + 32, v->output_id, 8);
        sf_put(b + 40, v->visible, 2);
        sf_put(b + 42, v->reservation_edge, 2);
        sf_put(b + 44, v->reservation_thickness, 2);
        sf_put(b + 46, v->selected_slot, 2);
        sf_put(b + 48, v->entry_count, 2);
        sf_put(b + 50, 0, 2);
        for (i = 0; i < v->entry_count; i++) {
            sf_put(b + 52 + 12 * i, v->entries[i].slot, 2);
            sf_put(b + 54 + 12 * i, 0, 2);
            sf_put(b + 56 + 12 * i, v->entries[i].generation, 8);
        }
        break;
    }
    case SOPHIA_SF_TABS_CANDIDATE: {
        const struct sophia_sf_tabs_candidate *v = &r->value.tabs_candidate;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->snapshot_generation, 8);
        sf_put(b + 24, v->candidate_generation, 8);
        sf_put(b + 32, v->group_count, 2);
        sf_put(b + 34, 0, 2);
        if (v->rows_bytes)
            memcpy(b + 36, v->rows, v->rows_bytes);
        break;
    }
    case SOPHIA_SF_REFERENCE_CANDIDATE:
        sf_reference_candidate_put(b, &r->value.reference_candidate);
        break;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE: {
        const struct sophia_sf_descriptor_launcher_candidate *v =
            &r->value.descriptor_launcher_candidate;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->catalog_generation, 8);
        sf_put(b + 24, v->request_generation, 8);
        sf_put(b + 32, v->candidate_generation, 8);
        sf_put(b + 40, v->output_id, 8);
        sf_put(b + 48, v->visible, 2);
        sf_put(b + 50, v->selected, 2);
        sf_put(b + 52, v->entry_count, 2);
        sf_put(b + 54, v->font_size, 2);
        for (i = 0; i < 4; i++)
            sf_put(b + 56 + 4 * i, v->colors[i], 4);
        for (i = 0; i < v->entry_count; i++)
            sf_put(b + 72 + 2 * i, v->entries[i], 2);
        break;
    }
    }
}
int sf_descriptor_candidate_take(const uint8_t *b, size_t n, struct sophia_sf_record *r)
{
    size_t i;
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_CANDIDATE: {
        struct sophia_sf_descriptor_candidate *v = &r->value.descriptor_candidate;
        if (n < 52 || !sf_zero(b + 50, 2))
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->snapshot_generation = sf_get(b + 16, 8);
        v->candidate_generation = sf_get(b + 24, 8);
        v->output_id = sf_get(b + 32, 8);
        v->visible = (uint16_t)sf_get(b + 40, 2);
        v->reservation_edge = (uint16_t)sf_get(b + 42, 2);
        v->reservation_thickness = (uint16_t)sf_get(b + 44, 2);
        v->selected_slot = (uint16_t)sf_get(b + 46, 2);
        v->entry_count = (uint16_t)sf_get(b + 48, 2);
        if (v->entry_count > 16 || n != sf_descriptor_candidate_size(r))
            return -1;
        for (i = 0; i < v->entry_count; i++) {
            if (!sf_zero(b + 54 + 12 * i, 2))
                return -1;
            v->entries[i].slot = (uint16_t)sf_get(b + 52 + 12 * i, 2);
            v->entries[i].generation = sf_get(b + 56 + 12 * i, 8);
        }
        return 0;
    }
    case SOPHIA_SF_TABS_CANDIDATE: {
        struct sophia_sf_tabs_candidate *v = &r->value.tabs_candidate;
        if (n < 36 || !sf_zero(b + 34, 2))
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->snapshot_generation = sf_get(b + 16, 8);
        v->candidate_generation = sf_get(b + 24, 8);
        v->group_count = (uint16_t)sf_get(b + 32, 2);
        v->rows = b + 36;
        v->rows_bytes = n - 36;
        return 0;
    }
    case SOPHIA_SF_REFERENCE_CANDIDATE:
        return sf_reference_candidate_take(b, n, &r->value.reference_candidate);
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE: {
        struct sophia_sf_descriptor_launcher_candidate *v = &r->value.descriptor_launcher_candidate;
        if (n < 72)
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->catalog_generation = sf_get(b + 16, 8);
        v->request_generation = sf_get(b + 24, 8);
        v->candidate_generation = sf_get(b + 32, 8);
        v->output_id = sf_get(b + 40, 8);
        v->visible = (uint16_t)sf_get(b + 48, 2);
        v->selected = (uint16_t)sf_get(b + 50, 2);
        v->entry_count = (uint16_t)sf_get(b + 52, 2);
        v->font_size = (uint16_t)sf_get(b + 54, 2);
        if (v->entry_count > 32 || n != sf_descriptor_candidate_size(r))
            return -1;
        for (i = 0; i < 4; i++)
            v->colors[i] = (uint32_t)sf_get(b + 56 + 4 * i, 4);
        for (i = 0; i < v->entry_count; i++)
            v->entries[i] = (uint16_t)sf_get(b + 72 + 2 * i, 2);
        return 0;
    }
    default:
        return -1;
    }
}
