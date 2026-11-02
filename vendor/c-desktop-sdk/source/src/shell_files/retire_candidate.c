#include "internal.h"

#define CANDIDATE_LIMIT 8192u
#define FLAGS_AT (SOPHIA_SF_HEADER_BYTES + 78u)
#define PREFIX_BYTES (SOPHIA_SF_HEADER_BYTES + 80u)
#define RETIRE_BYTES 24u

int sophia_sf_candidate_encode_with_retire(void *dst, size_t capacity,
    const struct sophia_sf_record *record, const struct sophia_sf_candidate_retire *retire,
    size_t *written)
{
    size_t bytes, bounded;
    uint8_t *b = dst;
    int result;
    if (!dst || !record || !retire || !written)
        return -4;
    if (record->header.kind != SOPHIA_SF_CANDIDATE || !retire->transaction ||
        !retire->resource_id || !retire->resource_generation)
        return -1;
    bounded = capacity < CANDIDATE_LIMIT ? capacity : CANDIDATE_LIMIT;
    if (bounded < RETIRE_BYTES)
        return -4;
    result = sophia_sf_encode(dst, bounded - RETIRE_BYTES, record, &bytes);
    if (result)
        return result;
    sf_put(b + FLAGS_AT, 1, 2);
    sf_put(b + bytes, retire->transaction, 8);
    sf_put(b + bytes + 8, retire->resource_id, 8);
    sf_put(b + bytes + 16, retire->resource_generation, 8);
    sf_put(b, bytes + RETIRE_BYTES, 4);
    *written = bytes + RETIRE_BYTES;
    return 0;
}

int sophia_sf_candidate_decode_with_retire(const void *src, size_t bytes, int enabled,
    struct sophia_sf_record *out, struct sophia_sf_candidate_retire *retire, int *has_retire)
{
    const uint8_t *b = src;
    uint8_t copy[CANDIDATE_LIMIT];
    struct sophia_sf_candidate_retire intent;
    size_t plain;
    uint16_t flags;
    if (!src || !out || !retire || !has_retire)
        return -4;
    if (bytes < PREFIX_BYTES || bytes > CANDIDATE_LIMIT || sf_get(b, 4) != bytes ||
        sf_get(b + 6, 2) != SOPHIA_SF_CANDIDATE)
        return -1;
    flags = (uint16_t)sf_get(b + FLAGS_AT, 2);
    if (!flags) {
        if (sophia_sf_decode(src, bytes, out))
            return -1;
        memset(retire, 0, sizeof(*retire));
        *has_retire = 0;
        return 0;
    }
    if (flags != 1 || !enabled || bytes < PREFIX_BYTES + RETIRE_BYTES)
        return -1;
    plain = bytes - RETIRE_BYTES;
    intent.transaction = sf_get(b + plain, 8);
    intent.resource_id = sf_get(b + plain + 8, 8);
    intent.resource_generation = sf_get(b + plain + 16, 8);
    if (!intent.transaction || !intent.resource_id || !intent.resource_generation)
        return -1;
    /* Reuse the ordinary codec's row, length and cross-field validation. */
    memcpy(copy, b, plain);
    sf_put(copy, plain, 4);
    sf_put(copy + FLAGS_AT, 0, 2);
    if (sophia_sf_decode(copy, plain, out))
        return -1;
    *retire = intent;
    *has_retire = 1;
    return 0;
}
