#include "roles_internal.h"

static int entry_check(const struct sophia_sf_catalog_entry *v)
{
    if (v->slot < 1 || v->slot > 4096 || v->available > 1 || !v->label.size ||
        sf_launcher_text_check(v->label, 128) || sf_launcher_text_check(v->keywords, 256) ||
        sf_launcher_text_check(v->identity, 256))
        return -1;
    if (v->identity.size &&
        !(v->identity.size > 11 && !memcmp(v->identity.data, "registered:", 11)) &&
        !(v->identity.size > 8 && !memcmp(v->identity.data, "desktop:", 8)))
        return -1;
    return 0;
}
int sophia_sf_catalog_entry_encode(uint8_t b[656], const struct sophia_sf_catalog_entry *v)
{
    if (!b || !v)
        return -4;
    if (entry_check(v))
        return -1;
    sf_put(b, v->slot, 2);
    sf_put(b + 2, v->available, 2);
    sf_text_put(b + 4, 128, v->label);
    sf_text_put(b + 136, 256, v->keywords);
    sf_text_put(b + 396, 256, v->identity);
    return 0;
}
int sophia_sf_catalog_entry_decode(const uint8_t b[656], struct sophia_sf_catalog_entry *out)
{
    struct sophia_sf_catalog_entry v;
    if (!b || !out)
        return -4;
    memset(&v, 0, sizeof(v));
    v.slot = (uint16_t)sf_get(b, 2);
    v.available = (uint16_t)sf_get(b + 2, 2);
    if (sf_text_take(b + 4, 128, &v.label) || sf_text_take(b + 136, 256, &v.keywords) ||
        sf_text_take(b + 396, 256, &v.identity) || entry_check(&v))
        return -1;
    *out = v;
    return 0;
}
int sophia_sf_catalog_entry_at(const struct sophia_sf_catalog *v, size_t i,
                               struct sophia_sf_catalog_entry *out)
{
    if (!v || !out || !v->rows || i >= v->entry_count || v->entry_count > 4096 ||
        v->rows_bytes != (size_t)v->entry_count * 656)
        return -4;
    return sophia_sf_catalog_entry_decode(v->rows + i * 656, out);
}
int sf_catalog_check(const struct sophia_sf_catalog *v)
{
    uint8_t seen[4097] = {0};
    size_t i;
    if (!v->transaction || !v->connection_epoch || !v->generation || v->entry_count > 4096 ||
        v->identities_present > 1 || v->rows_bytes != (size_t)v->entry_count * 656 ||
        (v->rows_bytes && !v->rows))
        return -1;
    for (i = 0; i < v->entry_count; i++) {
        struct sophia_sf_catalog_entry e;
        if (sophia_sf_catalog_entry_at(v, i, &e) || seen[e.slot] ||
            (!!e.identity.size) != (!!v->identities_present))
            return -1;
        seen[e.slot] = 1;
    }
    return 0;
}
void sf_catalog_put(uint8_t *b, const struct sophia_sf_catalog *v)
{
    sf_put(b, v->transaction, 8);
    sf_put(b + 8, v->connection_epoch, 8);
    sf_put(b + 16, v->generation, 8);
    sf_put(b + 24, v->entry_count, 2);
    sf_put(b + 26, v->identities_present, 2);
    sf_put(b + 28, 0, 4);
    if (v->rows_bytes)
        memcpy(b + 32, v->rows, v->rows_bytes);
}
int sf_catalog_take(const uint8_t *b, size_t n, struct sophia_sf_catalog *v)
{
    if (n < 32 || !sf_zero(b + 28, 4))
        return -1;
    v->transaction = sf_get(b, 8);
    v->connection_epoch = sf_get(b + 8, 8);
    v->generation = sf_get(b + 16, 8);
    v->entry_count = (uint16_t)sf_get(b + 24, 2);
    v->identities_present = (uint16_t)sf_get(b + 26, 2);
    v->rows = b + 32;
    v->rows_bytes = n - 32;
    return sf_catalog_check(v);
}
