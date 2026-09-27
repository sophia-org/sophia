#include "roles_internal.h"

int sf_role_candidate_wire_check(const uint8_t *, size_t, int);

void sf_role_candidate_put(uint8_t *b, const struct sophia_sf_role_candidate *v, int native)
{
    const struct sophia_sf_candidate *c = &v->content;
    size_t i, at = native ? 108 : 88;
    sf_put_candidate(b, c);
    if (native) {
        sf_put(b + 72, v->opening, 8);
        sf_put(b + 80, v->catalog_generation, 8);
        sf_put(b + 88, v->state_revision, 8);
        sf_put(b + 96, v->selected, 2);
        sf_put(b + 98, c->surface_count, 2);
        sf_put(b + 100, c->placement_count, 2);
        sf_put(b + 102, c->target_count, 2);
        sf_put(b + 104, v->row_count, 2);
        sf_put(b + 106, 0, 2);
    } else {
        sf_put(b + 72, v->catalog_generation, 8);
        sf_put(b + 80, c->surface_count, 2);
        sf_put(b + 82, c->placement_count, 2);
        sf_put(b + 84, c->target_count, 2);
        sf_put(b + 86, 0, 2);
    }
    for (i = 0; i < c->surface_count; i++, at += 64)
        sf_put_content_surface(b + at, &c->surfaces[i]);
    for (i = 0; i < c->placement_count; i++, at += 32)
        sf_put_content_placement(b + at, &c->placements[i]);
    for (i = 0; i < c->target_count; i++, at += 48)
        sf_put_content_target(b + at, &c->targets[i]);
    if (native)
        for (i = 0; i < v->row_count; i++, at += 2)
            sf_put(b + at, v->rows[i], 2);
}
static int bytes_check(const struct sophia_sf_role_candidate *v, int native)
{
    uint8_t b[8192];
    const struct sophia_sf_candidate *c = &v->content;
    size_t n;
    if (c->surface_count > (native ? 1 : 8) || c->placement_count > 32 ||
        c->target_count > (native ? 32 : 64) || v->row_count > 32)
        return -1;
    n = (native ? 108 + 2u * v->row_count : 88) + 64u * c->surface_count +
        32u * c->placement_count + 48u * c->target_count;
    if (n + 32 > 8192)
        return -1;
    sf_role_candidate_put(b, v, native);
    return sf_role_candidate_wire_check(b, n, native);
}
int sf_role_candidate_check(const struct sophia_sf_role_candidate *v, int native)
{
    uint8_t seen[4097] = {0};
    size_t i;
    if (bytes_check(v, native))
        return -1;
    if (!native)
        return 0; /* Dock counts beyond field bounds are owner rules. */
    if (v->content.surface_count != 1 || !v->content.placement_count ||
        v->content.target_count != v->row_count || ((v->selected == 0) != (v->row_count == 0)))
        return -1;
    for (i = 0; i < v->row_count; i++) {
        if (seen[v->rows[i]])
            return -1;
        seen[v->rows[i]] = 1;
    }
    return v->selected && !seen[v->selected] ? -1 : 0;
}
int sophia_sf_role_candidate_validate_value(uint16_t kind, const struct sophia_sf_role_candidate *v)
{
    if (!v || (kind != 267 && kind != 270))
        return -4;
    return sf_role_candidate_check(v, kind == 267);
}
int sophia_sf_role_candidate_encode_bytes(void *dst, size_t capacity, uint16_t kind,
                                          const struct sophia_sf_role_candidate *v, size_t *written)
{
    size_t n;
    int native = kind == 267;
    if (!dst || !v || !written || (kind != 267 && kind != 270))
        return -4;
    if (bytes_check(v, native))
        return -1;
    n = (native ? 108 + 2u * v->row_count : 88) + 64u * v->content.surface_count +
        32u * v->content.placement_count + 48u * v->content.target_count;
    if (n > capacity)
        return -4;
    sf_role_candidate_put(dst, v, native);
    *written = n;
    return 0;
}
int sophia_sf_role_candidate_decode_bytes(uint16_t kind, const void *src, size_t bytes,
                                          struct sophia_sf_role_candidate *out)
{
    struct sophia_sf_role_candidate value;
    if (!src || !out || (kind != 267 && kind != 270))
        return -4;
    memset(&value, 0, sizeof(value));
    if (sf_role_candidate_take(src, bytes, &value, kind == 267))
        return -1;
    *out = value;
    return 0;
}
int sf_role_candidate_take(const uint8_t *b, size_t n, struct sophia_sf_role_candidate *v,
                           int native)
{
    uint8_t prefix[80], row[64];
    size_t i, at = native ? 108 : 88;
    struct sophia_sf_candidate *c = &v->content;
    if (sf_role_candidate_wire_check(b, n, native))
        return -1;
    memcpy(prefix, b, 72);
    memcpy(prefix + 72, b + (native ? 98 : 80), 6);
    memset(prefix + 78, 0, 2);
    if (sf_take_candidate(prefix, c))
        return -1;
    v->catalog_generation = sf_get(b + (native ? 80 : 72), 8);
    if (native) {
        v->opening = sf_get(b + 72, 8);
        v->state_revision = sf_get(b + 88, 8);
        v->selected = (uint16_t)sf_get(b + 96, 2);
        v->row_count = (uint16_t)sf_get(b + 104, 2);
    }
    for (i = 0; i < c->surface_count; i++, at += 64) {
        memcpy(row, b + at, 64);
        /* Base extraction shares scalar fields; the role-specific validator
         * has already checked the original native role=3 row. */
        if (native)
            sf_put(row + 24, 1, 2);
        if (sf_take_content_surface(row, &c->surfaces[i]))
            return -1;
        if (native)
            c->surfaces[i].role = 3;
    }
    for (i = 0; i < c->placement_count; i++, at += 32)
        if (sf_take_content_placement(b + at, &c->placements[i]))
            return -1;
    for (i = 0; i < c->target_count; i++, at += 48) {
        memcpy(row, b + at, 48);
        sf_put(row + 2, 1, 2);
        if (sf_take_content_target(row, &c->targets[i]))
            return -1;
        c->targets[i].action_kind = native ? 2 : 3;
    }
    if (native)
        for (i = 0; i < v->row_count; i++, at += 2)
            v->rows[i] = (uint16_t)sf_get(b + at, 2);
    return 0;
}
