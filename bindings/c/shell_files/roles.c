#include "roles_internal.h"

size_t sf_role_size(const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case 3:
        return 32 + (size_t)r->value.catalog.entry_count * 656;
    case 4:
        return 40 + (size_t)r->value.indicators.status_count * 46 +
               (size_t)r->value.indicators.indicator_count * 66;
    case 38:
        return 64;
    case 39:
        return 112;
    case 40:
        return 116;
    case 41:
        return 398;
    case 42:
        return 136;
    case 43:
        return 36;
    case 44:
        return 132;
    case 45:
        return 36;
    case 266:
        return 92;
    case 267:
    case 270: {
        const struct sophia_sf_role_candidate *v = &r->value.role_candidate;
        return (r->header.kind == 267 ? 108 + 2u * v->row_count : 88) +
               64u * v->content.surface_count + 32u * v->content.placement_count +
               48u * v->content.target_count;
    }
    case 268:
    case 269:
        return 132;
    case 271:
        return 128;
    case 272:
        return 56;
    default:
        return 0;
    }
}
int sf_role_check(const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case 3:
        return sf_catalog_check(&r->value.catalog);
    case 4:
        return sf_indicators_check(&r->value.indicators);
    case 267:
    case 270:
        return sf_role_candidate_check(&r->value.role_candidate, r->header.kind == 267);
    default:
        return sf_native_check(r);
    }
}
void sf_role_put(uint8_t *b, const struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case 3:
        sf_catalog_put(b, &r->value.catalog);
        break;
    case 4:
        sf_indicators_put(b, &r->value.indicators);
        break;
    case 267:
    case 270:
        sf_role_candidate_put(b, &r->value.role_candidate, r->header.kind == 267);
        break;
    default:
        sf_native_put(b, r);
        break;
    }
}
int sf_role_take(const uint8_t *b, size_t n, struct sophia_sf_record *r)
{
    switch (r->header.kind) {
    case 3:
        return sf_catalog_take(b, n, &r->value.catalog);
    case 4:
        return sf_indicators_take(b, n, &r->value.indicators);
    case 267:
    case 270:
        return sf_role_candidate_take(b, n, &r->value.role_candidate, r->header.kind == 267);
    default:
        return sf_native_take(b, n, r);
    }
}
