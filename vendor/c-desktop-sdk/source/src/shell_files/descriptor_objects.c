#include "descriptors_internal.h"

size_t sf_descriptor_object_size(const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTORS:
        return 64 + 196u * r->value.descriptors.descriptor_count;
    case SOPHIA_SF_TABS:
        return 32 + 24u * r->value.tabs.group_count + 196u * r->value.tabs.entry_count;
    case SOPHIA_SF_SHORTCUTS:
        return 32 + 408u * r->value.shortcuts.entry_count;
    default:
        return 0;
    }
}
static int insert_slot(uint8_t seen[8192], uint16_t slot)
{
    unsigned byte = slot / 8, mask = 1u << (slot % 8);
    if (!slot || (seen[byte] & mask))
        return -1;
    seen[byte] |= (uint8_t)mask;
    return 0;
}
static int descriptors_check(const struct sophia_sf_descriptors *v, uint64_t epoch)
{
    uint8_t seen[8192] = {0};
    size_t i;
    if (!v->transaction || v->connection_epoch != epoch || !v->snapshot_generation ||
        !v->output_id || !v->output_generation || !v->broker_epoch || !v->broker_revocation_epoch ||
        v->descriptor_count > 16 || v->rows_bytes != (size_t)v->descriptor_count * 196 ||
        (v->rows_bytes && !v->rows))
        return -1;
    for (i = 0; i < v->descriptor_count; i++) {
        struct sophia_sf_descriptor_entry e;
        if (sophia_sf_descriptor_entry_at(v, i, &e) || e.action_recipient_epoch != epoch ||
            e.action_issuer_epoch != v->broker_epoch ||
            e.action_issuer_revocation_epoch != v->broker_revocation_epoch ||
            insert_slot(seen, e.slot))
            return -1;
    }
    return 0;
}
static int tabs_check(const struct sophia_sf_tabs *v, uint64_t epoch)
{
    uint8_t seen[8192] = {0};
    size_t i, j, at = 0;
    if (!v->transaction || v->connection_epoch != epoch || !v->generation ||
        v->group_count > 1024 || v->entry_count > 2048 ||
        v->rows_bytes != (size_t)v->group_count * 24 + (size_t)v->entry_count * 196 ||
        (v->rows_bytes && !v->rows) ||
        sf_descriptor_group_order_unique(v->rows, v->group_count, 24))
        return -1;
    for (i = 0; i < v->group_count; i++) {
        struct sophia_sf_tab_group g;
        int selected = 0;
        if (sophia_sf_tab_group_at(v, i, &g) || g.entry_count > v->entry_count - at)
            return -1;
        for (j = 0; j < g.entry_count; j++) {
            struct sophia_sf_descriptor_entry e;
            if (sophia_sf_tab_entry_at(v, at + j, &e) || e.action_recipient_epoch != epoch ||
                insert_slot(seen, e.slot))
                return -1;
            if (e.slot == g.selected_slot)
                selected = 1;
        }
        if (g.entry_count && !selected)
            return -1;
        at += g.entry_count;
    }
    return at == v->entry_count ? 0 : -1;
}
static int shortcuts_check(const struct sophia_sf_shortcuts *v, uint64_t epoch)
{
    uint8_t seen[8192] = {0};
    size_t i;
    if (!v->transaction || v->connection_epoch != epoch || !v->generation || v->entry_count > 256 ||
        v->rows_bytes != (size_t)v->entry_count * 408 || (v->rows_bytes && !v->rows))
        return -1;
    for (i = 0; i < v->entry_count; i++) {
        struct sophia_sf_shortcut_entry e;
        if (sophia_sf_shortcut_entry_at(v, i, &e) || insert_slot(seen, e.slot))
            return -1;
    }
    return 0;
}
int sf_descriptor_object_check(const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTORS:
        return descriptors_check(&r->value.descriptors, r->header.epoch);
    case SOPHIA_SF_TABS:
        return tabs_check(&r->value.tabs, r->header.epoch);
    case SOPHIA_SF_SHORTCUTS:
        return shortcuts_check(&r->value.shortcuts, r->header.epoch);
    default:
        return -1;
    }
}
void sf_descriptor_object_put(uint8_t *b, const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTORS: {
        const struct sophia_sf_descriptors *v = &r->value.descriptors;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->snapshot_generation, 8);
        sf_put(b + 24, v->output_id, 8);
        sf_put(b + 32, v->output_generation, 8);
        sf_put(b + 40, v->broker_epoch, 8);
        sf_put(b + 48, v->broker_revocation_epoch, 8);
        sf_put(b + 56, v->descriptor_count, 2);
        memset(b + 58, 0, 6);
        if (v->rows_bytes)
            memcpy(b + 64, v->rows, v->rows_bytes);
        break;
    }
    case SOPHIA_SF_TABS: {
        const struct sophia_sf_tabs *v = &r->value.tabs;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->generation, 8);
        sf_put(b + 24, v->group_count, 2);
        sf_put(b + 26, v->entry_count, 2);
        sf_put(b + 28, 0, 4);
        if (v->rows_bytes)
            memcpy(b + 32, v->rows, v->rows_bytes);
        break;
    }
    case SOPHIA_SF_SHORTCUTS: {
        const struct sophia_sf_shortcuts *v = &r->value.shortcuts;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->generation, 8);
        sf_put(b + 24, v->entry_count, 2);
        memset(b + 26, 0, 6);
        if (v->rows_bytes)
            memcpy(b + 32, v->rows, v->rows_bytes);
        break;
    }
    }
}
int sf_descriptor_object_take(const uint8_t *b, size_t n, struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTORS: {
        struct sophia_sf_descriptors *v = &r->value.descriptors;
        if (n < 64 || !sf_zero(b + 58, 6))
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->snapshot_generation = sf_get(b + 16, 8);
        v->output_id = sf_get(b + 24, 8);
        v->output_generation = sf_get(b + 32, 8);
        v->broker_epoch = sf_get(b + 40, 8);
        v->broker_revocation_epoch = sf_get(b + 48, 8);
        v->descriptor_count = (uint16_t)sf_get(b + 56, 2);
        v->rows = b + 64;
        v->rows_bytes = n - 64;
        return 0;
    }
    case SOPHIA_SF_TABS: {
        struct sophia_sf_tabs *v = &r->value.tabs;
        if (n < 32 || !sf_zero(b + 28, 4))
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->generation = sf_get(b + 16, 8);
        v->group_count = (uint16_t)sf_get(b + 24, 2);
        v->entry_count = (uint16_t)sf_get(b + 26, 2);
        v->rows = b + 32;
        v->rows_bytes = n - 32;
        return 0;
    }
    case SOPHIA_SF_SHORTCUTS: {
        struct sophia_sf_shortcuts *v = &r->value.shortcuts;
        if (n < 32 || !sf_zero(b + 26, 6))
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->generation = sf_get(b + 16, 8);
        v->entry_count = (uint16_t)sf_get(b + 24, 2);
        v->rows = b + 32;
        v->rows_bytes = n - 32;
        return 0;
    }
    default:
        return -1;
    }
}
