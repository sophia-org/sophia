/* Native snapshot rows from the separately pinned descriptor proposal. */
#include "descriptors_internal.h"

static int descriptor_entry_check(const struct sophia_sf_descriptor_entry *v)
{
    if (!v->slot || v->trust_level > 3 || v->attention > 2 || v->label_present > 1 ||
        v->label_redacted > 1 || !v->generation || !v->action_token || !v->action_issuer_epoch ||
        !v->action_issuer_revocation_epoch || !v->action_recipient_epoch ||
        v->action_target_slot != v->slot || v->action_target_generation != v->generation ||
        sf_descriptor_text_check(v->label, 128))
        return -1;
    if (v->label_present)
        return v->label.size ? 0 : -1;
    return v->label.size || v->label_redacted ? -1 : 0;
}
int sophia_sf_descriptor_entry_encode(uint8_t b[196], const struct sophia_sf_descriptor_entry *v)
{
    if (!b || !v)
        return -4;
    if (descriptor_entry_check(v))
        return -1;
    sf_put(b, v->slot, 2);
    sf_put(b + 2, v->trust_level, 2);
    sf_put(b + 4, v->attention, 2);
    sf_put(b + 6, v->label_present, 2);
    sf_put(b + 8, v->label_redacted, 2);
    sf_put(b + 10, 0, 2);
    sf_put(b + 12, v->generation, 8);
    sf_put(b + 20, v->action_token, 8);
    sf_put(b + 28, v->action_issuer_epoch, 8);
    sf_put(b + 36, v->action_issuer_revocation_epoch, 8);
    sf_put(b + 44, v->action_recipient_epoch, 8);
    sf_put(b + 52, v->action_target_slot, 2);
    sf_put(b + 54, 0, 2);
    sf_put(b + 56, v->action_target_generation, 8);
    sf_text_put(b + 64, 128, v->label);
    return 0;
}
int sophia_sf_descriptor_entry_decode(const uint8_t b[196], struct sophia_sf_descriptor_entry *out)
{
    struct sophia_sf_descriptor_entry v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.slot = (uint16_t)sf_get(b, 2);
    v.trust_level = (uint16_t)sf_get(b + 2, 2);
    v.attention = (uint16_t)sf_get(b + 4, 2);
    v.label_present = (uint16_t)sf_get(b + 6, 2);
    v.label_redacted = (uint16_t)sf_get(b + 8, 2);
    v.generation = sf_get(b + 12, 8);
    v.action_token = sf_get(b + 20, 8);
    v.action_issuer_epoch = sf_get(b + 28, 8);
    v.action_issuer_revocation_epoch = sf_get(b + 36, 8);
    v.action_recipient_epoch = sf_get(b + 44, 8);
    v.action_target_slot = (uint16_t)sf_get(b + 52, 2);
    v.action_target_generation = sf_get(b + 56, 8);
    if (!sf_zero(b + 10, 2) || !sf_zero(b + 54, 2) || sf_text_take(b + 64, 128, &v.label) ||
        descriptor_entry_check(&v))
        return -1;
    *out = v;
    return 0;
}
int sophia_sf_descriptor_entry_at(const struct sophia_sf_descriptors *v, size_t i,
                                  struct sophia_sf_descriptor_entry *out)
{
    if (!v || !out || !v->rows || v->descriptor_count > 16 || i >= v->descriptor_count ||
        v->rows_bytes != (size_t)v->descriptor_count * 196)
        return -4;
    return sophia_sf_descriptor_entry_decode(v->rows + i * 196, out);
}
static int group_check(const struct sophia_sf_tab_group *v)
{
    return !v->group_slot || !v->output_id || v->focused > 1 || v->entry_count > 2048 ||
                   ((v->entry_count == 0) != (v->selected_slot == 0))
               ? -1
               : 0;
}
int sophia_sf_tab_group_encode(uint8_t b[24], const struct sophia_sf_tab_group *v)
{
    if (!b || !v)
        return -4;
    if (group_check(v))
        return -1;
    sf_put(b, v->group_slot, 8);
    sf_put(b + 8, v->output_id, 8);
    sf_put(b + 16, v->selected_slot, 2);
    sf_put(b + 18, v->focused, 2);
    sf_put(b + 20, v->entry_count, 2);
    sf_put(b + 22, 0, 2);
    return 0;
}
int sophia_sf_tab_group_decode(const uint8_t b[24], struct sophia_sf_tab_group *out)
{
    struct sophia_sf_tab_group v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.group_slot = sf_get(b, 8);
    v.output_id = sf_get(b + 8, 8);
    v.selected_slot = (uint16_t)sf_get(b + 16, 2);
    v.focused = (uint16_t)sf_get(b + 18, 2);
    v.entry_count = (uint16_t)sf_get(b + 20, 2);
    if (!sf_zero(b + 22, 2) || group_check(&v))
        return -1;
    *out = v;
    return 0;
}
static int tabs_view(const struct sophia_sf_tabs *v)
{
    return !v || !v->rows || v->group_count > 1024 || v->entry_count > 2048 ||
                   v->rows_bytes != (size_t)v->group_count * 24 + (size_t)v->entry_count * 196
               ? -4
               : 0;
}
int sophia_sf_tab_group_at(const struct sophia_sf_tabs *v, size_t i,
                           struct sophia_sf_tab_group *out)
{
    if (tabs_view(v) || !out || i >= v->group_count)
        return -4;
    return sophia_sf_tab_group_decode(v->rows + i * 24, out);
}
int sophia_sf_tab_entry_at(const struct sophia_sf_tabs *v, size_t i,
                           struct sophia_sf_descriptor_entry *out)
{
    if (tabs_view(v) || !out || i >= v->entry_count)
        return -4;
    return sophia_sf_descriptor_entry_decode(v->rows + (size_t)v->group_count * 24 + i * 196, out);
}
static int shortcut_check(const struct sophia_sf_shortcut_entry *v)
{
    return !v->slot || v->label_present > 1 || v->group_present > 1 || !v->chord.size ||
                   !v->action.size || ((v->label.size != 0) != (v->label_present != 0)) ||
                   ((v->group.size != 0) != (v->group_present != 0)) ||
                   sf_launcher_text_check(v->chord, 64) || sf_launcher_text_check(v->action, 128) ||
                   sf_launcher_text_check(v->label, 128) || sf_launcher_text_check(v->group, 64)
               ? -1
               : 0;
}
int sophia_sf_shortcut_entry_encode(uint8_t b[408], const struct sophia_sf_shortcut_entry *v)
{
    if (!b || !v)
        return -4;
    if (shortcut_check(v))
        return -1;
    sf_put(b, v->slot, 2);
    sf_put(b + 2, v->label_present, 2);
    sf_put(b + 4, v->group_present, 2);
    sf_put(b + 6, 0, 2);
    sf_text_put(b + 8, 64, v->chord);
    sf_text_put(b + 76, 128, v->action);
    sf_text_put(b + 208, 128, v->label);
    sf_text_put(b + 340, 64, v->group);
    return 0;
}
int sophia_sf_shortcut_entry_decode(const uint8_t b[408], struct sophia_sf_shortcut_entry *out)
{
    struct sophia_sf_shortcut_entry v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.slot = (uint16_t)sf_get(b, 2);
    v.label_present = (uint16_t)sf_get(b + 2, 2);
    v.group_present = (uint16_t)sf_get(b + 4, 2);
    if (!sf_zero(b + 6, 2) || sf_text_take(b + 8, 64, &v.chord) ||
        sf_text_take(b + 76, 128, &v.action) || sf_text_take(b + 208, 128, &v.label) ||
        sf_text_take(b + 340, 64, &v.group) || shortcut_check(&v))
        return -1;
    *out = v;
    return 0;
}
int sophia_sf_shortcut_entry_at(const struct sophia_sf_shortcuts *v, size_t i,
                                struct sophia_sf_shortcut_entry *out)
{
    if (!v || !out || !v->rows || v->entry_count > 256 || i >= v->entry_count ||
        v->rows_bytes != (size_t)v->entry_count * 408)
        return -4;
    return sophia_sf_shortcut_entry_decode(v->rows + i * 408, out);
}
