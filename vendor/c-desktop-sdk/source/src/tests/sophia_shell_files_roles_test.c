#include "../sophia_shell_files.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Body literals transcribed from KDL, independently of the C encoders and
 * the Go oracle's vectors. Unlisted bytes are reserved or zero-valued. */
#define BINDING                                                                                    \
    [0] = 7, [8] = 17, [16] = 1, [24] = 2, [32] = 3, [40] = 1, [48] = 4, [56] = 1, [64] = 5,       \
    [72] = 6, [80] = 7, [88] = 8, [96] = 9, [104] = 10
#define EVENT BINDING, [112] = 11, [120] = 9
static const uint8_t catalog[688] = {
    [0] = 7,    [8] = 17,   [16] = 5,    [24] = 1, [26] = 1, [32] = 1, [34] = 1, [36] = 1,
    [40] = 'T', [428] = 12, [432] = 'r', 'e',      'g',      'i',      's',      't',
    'e',        'r',        'e',         'd',      ':',      'x'};
static const uint8_t indicators[152] = {
    [0] = 7, [34] = 1, [36] = 1, [50] = 1, [54] = 'L', [116] = 1, [120] = 'I'};
static const uint8_t opening[64] = {
    [0] = 7, [8] = 17, [16] = 1, [24] = 2, [32] = 3, [40] = 1, [48] = 5, [56] = 9};
static const uint8_t focus[112] = {BINDING};
static const uint8_t revoked[116] = {BINDING, [112] = 1};
static const uint8_t input[398] = {
    BINDING, [112] = 11, [120] = 10, [128] = 1, [136] = 1, [138] = 1, [142] = 'x'};
static const uint8_t native_outcome[136] = {EVENT, [128] = 1, [130] = 1, [132] = 1};
static const uint8_t closed[36] = {[0] = 7, [8] = 17, [16] = 1, [24] = 2, [32] = 1};
static const uint8_t allocation[92] = {[0] = 7,  [8] = 17, [16] = 1, [24] = 2,  [32] = 3, [40] = 1,
                                       [48] = 1, [72] = 1, [74] = 1, [76] = 64, [80] = 32};
static const uint8_t input_ack[132] = {EVENT, [128] = 1};
static const uint8_t activate[132] = {EVENT, [128] = 1, [130] = 1};
#define ACTION                                                                                     \
    [0] = 7, [8] = 17, [16] = 1, [24] = 3, [32] = 1, [40] = 6, [48] = 7, [56] = 8, [64] = 4,       \
    [72] = 1, [80] = 1, [88] = 1, [96] = 1, [104] = 11, [112] = 1, [120] = 5
static const uint8_t catalog_activate[128] = {ACTION};
static const uint8_t catalog_outcome[132] = {ACTION, [128] = 1};
static const uint8_t indicator_activate[56] = {[0] = 7};
static const uint8_t indicator_outcome[36] = {[0] = 7};
static const uint8_t native_candidate[254] = {
    [0] = 7,   [8] = 17,    [16] = 1,    [24] = 6,  [32] = 3,  [40] = 1,  [48] = 4,
    [56] = 1,  [64] = 8,    [72] = 2,    [80] = 5,  [88] = 9,  [96] = 1,  [98] = 1,
    [100] = 1, [102] = 1,   [104] = 1,   [108] = 4, [116] = 1, [124] = 1, [132] = 3,
    [134] = 1, [148] = 255, [149] = 255, [172] = 1, [180] = 1, [206] = 2, [208] = 1,
    [216] = 1, [224] = 1,   [240] = 2,   [244] = 1, [252] = 1};
static const uint8_t catalog_candidate[232] = {
    [0] = 7,   [8] = 17,  [16] = 1,  [24] = 6,  [32] = 3,    [40] = 1,    [48] = 4,
    [56] = 1,  [64] = 8,  [72] = 5,  [80] = 1,  [82] = 1,    [84] = 1,    [88] = 4,
    [96] = 1,  [104] = 1, [112] = 1, [114] = 1, [128] = 255, [129] = 255, [152] = 1,
    [160] = 1, [186] = 3, [188] = 1, [196] = 1, [204] = 1,   [220] = 2,   [224] = 1};
#undef ACTION
#undef EVENT
#undef BINDING
static const struct vector {
    uint16_t kind;
    const uint8_t *body;
    size_t size;
} vectors[] = {{3, catalog, sizeof(catalog)},
               {4, indicators, sizeof(indicators)},
               {38, opening, sizeof(opening)},
               {39, focus, sizeof(focus)},
               {40, revoked, sizeof(revoked)},
               {41, input, sizeof(input)},
               {42, native_outcome, sizeof(native_outcome)},
               {43, closed, sizeof(closed)},
               {44, catalog_outcome, sizeof(catalog_outcome)},
               {45, indicator_outcome, sizeof(indicator_outcome)},
               {266, allocation, sizeof(allocation)},
               {267, native_candidate, sizeof(native_candidate)},
               {268, input_ack, sizeof(input_ack)},
               {269, activate, sizeof(activate)},
               {270, catalog_candidate, sizeof(catalog_candidate)},
               {271, catalog_activate, sizeof(catalog_activate)},
               {272, indicator_activate, sizeof(indicator_activate)}};
static void put32(uint8_t *b, size_t n)
{
    size_t i;
    for (i = 0; i < 4; i++)
        b[i] = (uint8_t)(n >> (8 * i));
}
static size_t literal(uint8_t *b, const struct vector *v)
{
    memset(b, 0, 32);
    put32(b, 32 + v->size);
    b[4] = 1;
    b[6] = (uint8_t)v->kind;
    b[7] = (uint8_t)(v->kind >> 8);
    b[8] = 17;
    if (v->kind >= 256)
        b[16] = 9;
    else if (v->kind >= 16)
        b[24] = 1;
    memcpy(b + 32, v->body, v->size);
    return 32 + v->size;
}
static void vectors_and_truncations(void)
{
    uint8_t bytes[1024], encoded[1024];
    struct sophia_sf_record out, sentinel;
    size_t i, j, n, written;
    memset(&sentinel, 0xa5, sizeof(sentinel));
    for (i = 0; i < sizeof(vectors) / sizeof(vectors[0]); i++) {
        n = literal(bytes, &vectors[i]);
        if (sophia_sf_decode(bytes, n, &out)) {
            fprintf(stderr, "invalid role vector %u\n", vectors[i].kind);
            abort();
        }
        assert(!sophia_sf_encode(encoded, sizeof(encoded), &out, &written));
        assert(written == n && !memcmp(bytes, encoded, n));
        for (j = 32; j < n; j++) {
            struct sophia_sf_record failed = sentinel;
            put32(bytes, j);
            assert(sophia_sf_decode(bytes, j, &failed));
            assert(!memcmp(&failed, &sentinel, sizeof(failed)));
        }
        put32(bytes, n + 1);
        bytes[n] = 0;
        assert(sophia_sf_decode(bytes, n + 1, &out));
    }
}
static const struct vector *find_vector(unsigned kind)
{
    size_t i;
    for (i = 0; i < sizeof(vectors) / sizeof(vectors[0]); i++)
        if (vectors[i].kind == kind)
            return &vectors[i];
    abort();
}
static void refusal_controls(void)
{
    static const struct mutation {
        uint16_t kind, offset, value;
    } mutations[] = {
        {3, 24, 4097},  {3, 26, 2},    {3, 28, 1},     {3, 32, 0},     {3, 34, 2},
        {3, 36, 129},   {3, 38, 1},    {4, 32, 2},     {4, 34, 17},    {4, 36, 257},
        {4, 38, 1},     {4, 50, 33},   {4, 52, 1},     {38, 24, 0},    {39, 104, 0},
        {40, 112, 0},   {40, 112, 13}, {40, 114, 1},   {41, 136, 0},   {41, 136, 18},
        {41, 138, 257}, {41, 140, 1},  {42, 128, 0},   {42, 130, 0},   {42, 132, 0},
        {42, 134, 1},   {43, 32, 0},   {43, 34, 1},    {44, 112, 2},   {44, 114, 1},
        {44, 128, 6},   {44, 130, 1},  {45, 32, 4},    {266, 72, 0},   {266, 74, 5},
        {266, 84, 513}, {267, 98, 2},  {267, 100, 33}, {267, 102, 33}, {267, 104, 33},
        {267, 106, 1},  {268, 128, 0}, {268, 130, 1},  {269, 128, 3},  {269, 130, 4097},
        {270, 80, 9},   {270, 82, 33}, {270, 84, 65},  {270, 86, 1},   {271, 112, 0},
        {272, 0, 0}};
    uint8_t bytes[1024], encoded[1024], sentinel[1024];
    struct sophia_sf_record out;
    size_t i, n, written = 789;
    for (i = 0; i < sizeof(mutations) / sizeof(mutations[0]); i++) {
        const struct mutation *m = &mutations[i];
        n = literal(bytes, find_vector(m->kind));
        bytes[32 + m->offset] = (uint8_t)m->value;
        bytes[33 + m->offset] = (uint8_t)(m->value >> 8);
        assert(sophia_sf_decode(bytes, n, &out));
    }
    n = literal(bytes, find_vector(267));
    assert(!sophia_sf_decode(bytes, n, &out));
    out.value.role_candidate.content.surface_count = 2;
    memset(sentinel, 0xa5, sizeof(sentinel));
    memcpy(encoded, sentinel, sizeof(encoded));
    assert(sophia_sf_encode(encoded, sizeof(encoded), &out, &written));
    assert(written == 789 && !memcmp(encoded, sentinel, sizeof(encoded)));
}
static void maximum_catalog(void)
{
    const size_t n = 64u + 4096u * 656u;
    uint8_t *bytes = calloc(n, 1), *copy = malloc(n);
    struct sophia_sf_record record;
    struct sophia_sf_catalog_entry entry;
    size_t i, written;
    assert(bytes && copy);
    put32(bytes, n);
    bytes[4] = 1;
    bytes[6] = 3;
    bytes[8] = 17;
    bytes[32] = 1;
    bytes[40] = 17;
    bytes[48] = 1;
    bytes[57] = 16;
    for (i = 0; i < 4096; i++) {
        uint8_t *row = bytes + 64 + 656 * i;
        row[0] = (uint8_t)(i + 1);
        row[1] = (uint8_t)((i + 1) >> 8);
        row[4] = 1;
        row[8] = 'a';
    }
    assert(!sophia_sf_decode(bytes, n, &record));
    assert(record.value.catalog.rows == bytes + 64);
    assert(!sophia_sf_catalog_entry_at(&record.value.catalog, 4095, &entry));
    assert(entry.slot == 4096 && entry.label.size == 1 && entry.label.data[0] == 'a');
    assert(!sophia_sf_encode(copy, n, &record, &written));
    assert(written == n && !memcmp(bytes, copy, n));
    bytes[64 + 656] = 1;
    bytes[65 + 656] = 0;
    assert(sophia_sf_decode(bytes, n, &record));
    bytes[64 + 656] = 2;
    bytes[58] = 1;
    for (i = 0; i < 4096; i++) {
        uint8_t *row = bytes + 64 + 656 * i;
        char suffix[5];
        assert(snprintf(suffix, sizeof(suffix), "%04u", (unsigned)(4095 - i)) == 4);
        row[396] = 0;
        row[397] = 1;
        memcpy(row + 400, "registered:", 11);
        memset(row + 411, 'a', 241);
        memcpy(row + 652, suffix, 4);
    }
    assert(!sophia_sf_decode(bytes, n, &record));
    assert(!sophia_sf_encode(copy, n, &record, &written));
    assert(written == n && !memcmp(bytes, copy, n));
    memcpy(bytes + 64 + 4095 * 656 + 396, bytes + 64 + 396, 260);
    assert(sophia_sf_decode(bytes, n, &record));
    free(copy);
    free(bytes);
}
static void snapshot_rows(void)
{
    struct sophia_sf_catalog_entry c;
    struct sophia_sf_indicator_output_status s;
    struct sophia_sf_indicator_entry i;
    struct sophia_sf_record r;
    uint8_t cbytes[656], sbytes[46], ibytes[66], *maximum;
    size_t n = 32 + 40 + 16 * 46 + 256 * 66;
    assert(!sophia_sf_catalog_entry_decode(catalog + 32, &c));
    assert(!sophia_sf_catalog_entry_encode(cbytes, &c) && !memcmp(cbytes, catalog + 32, 656));
    assert(!sophia_sf_indicator_status_decode(indicators + 40, &s));
    assert(!sophia_sf_indicator_status_encode(sbytes, &s) && !memcmp(sbytes, indicators + 40, 46));
    assert(!sophia_sf_indicator_entry_decode(indicators + 86, &i));
    assert(!sophia_sf_indicator_entry_encode(ibytes, &i) && !memcmp(ibytes, indicators + 86, 66));
    maximum = calloc(n, 1);
    assert(maximum);
    put32(maximum, n);
    maximum[4] = 1;
    maximum[6] = 4;
    maximum[8] = 17;
    maximum[32] = 1;
    maximum[66] = 16;
    maximum[69] = 1;
    assert(!sophia_sf_decode(maximum, n, &r));
    assert(r.value.indicators.rows == maximum + 72 && r.value.indicators.indicator_count == 256);
    free(maximum);
}
static void candidate_layers(void)
{
    struct sophia_sf_role_candidate base, changed, decoded;
    uint8_t bytes[8192];
    size_t n, i;
    assert(!sophia_sf_role_candidate_decode_bytes(267, native_candidate, sizeof(native_candidate),
                                                  &base));
    for (i = 0; i < 5; i++) {
        changed = base;
        switch (i) {
        case 0:
            changed.content.surface_count = 0;
            break;
        case 1:
            changed.content.placement_count = 0;
            break;
        case 2:
            changed.row_count = 0;
            break;
        case 3:
            changed.selected = 2;
            break;
        case 4:
            changed.content.target_count = changed.row_count = 2;
            changed.content.targets[1] = changed.content.targets[0];
            changed.rows[1] = changed.rows[0];
            break;
        }
        assert(!sophia_sf_role_candidate_encode_bytes(bytes, sizeof(bytes), 267, &changed, &n));
        assert(!sophia_sf_role_candidate_decode_bytes(267, bytes, n, &decoded));
        assert(sophia_sf_role_candidate_validate_value(267, &decoded));
    }
    /* Target duplicates/overlaps belong to owner validation. */
    changed.rows[1] = 2;
    assert(!sophia_sf_role_candidate_validate_value(267, &changed));
    assert(!sophia_sf_role_candidate_decode_bytes(270, catalog_candidate, sizeof(catalog_candidate),
                                                  &changed));
    changed.content.surface_count = changed.content.placement_count = changed.content.target_count =
        0;
    assert(!sophia_sf_role_candidate_validate_value(270, &changed));
    assert(!sophia_sf_role_candidate_encode_bytes(bytes, sizeof(bytes), 270, &changed, &n));
    assert(!sophia_sf_role_candidate_decode_bytes(270, bytes, n, &decoded));
}
static void text_field(uint8_t *dst, size_t capacity, const uint8_t *text, size_t size)
{
    memset(dst, 0, capacity + 4);
    dst[0] = (uint8_t)size;
    memcpy(dst + 4, text, size);
}
static void maximum_candidates(void)
{
    const unsigned kinds[] = {267, 270};
    struct sophia_sf_record r, decoded;
    struct sophia_sf_role_candidate *v = &r.value.role_candidate;
    uint8_t bytes[8192];
    size_t k, i, n;
    for (k = 0; k < 2; k++) {
        n = literal(bytes, find_vector(kinds[k]));
        assert(!sophia_sf_decode(bytes, n, &r));
        v->content.surface_count = k ? 8 : 1;
        v->content.placement_count = 32;
        v->content.target_count = k ? 64 : 32;
        v->row_count = k ? 0 : 32;
        for (i = 1; i < v->content.surface_count; i++)
            v->content.surfaces[i] = v->content.surfaces[0];
        for (i = 1; i < 32; i++)
            v->content.placements[i] = v->content.placements[0];
        for (i = 1; i < v->content.target_count; i++)
            v->content.targets[i] = v->content.targets[0];
        for (i = 0; i < v->row_count; i++)
            v->rows[i] = (uint16_t)(i + 1);
        assert(!sophia_sf_encode(bytes, sizeof(bytes), &r, &n));
        assert(!sophia_sf_decode(bytes, n, &decoded));
    }
}
static void semantic_controls(void)
{
    static const uint8_t controls[][4] = {
        {0}, {0x7f}, {0xc2, 0x85}, {0xe2, 0x80, 0xaa}, {0xe2, 0x81, 0xa9}};
    static const uint8_t invalid[][4] = {
        {0xc0, 0x80}, {0xed, 0xa0, 0x80}, {0xf4, 0x90, 0x80, 0x80}, {0xe2, 0x82}, {0x80}};
    static const size_t control_sizes[] = {1, 1, 2, 3, 3}, invalid_sizes[] = {2, 3, 4, 2, 1};
    uint8_t bytes[1024];
    struct sophia_sf_record record;
    size_t i, j, n;
    for (i = 0; i < 5; i++) {
        n = literal(bytes, find_vector(3));
        text_field(bytes + 32 + 36, 128, controls[i], control_sizes[i]);
        assert(sophia_sf_decode(bytes, n, &record));
        n = literal(bytes, find_vector(41));
        text_field(bytes + 32 + 138, 256, controls[i], control_sizes[i]);
        assert(sophia_sf_decode(bytes, n, &record));
        n = literal(bytes, find_vector(4));
        text_field(bytes + 32 + 50, 32, controls[i], control_sizes[i]);
        assert(!sophia_sf_decode(bytes, n, &record));
        for (j = 0; j < 3; j++) {
            const unsigned kinds[] = {3, 4, 41};
            const size_t offsets[] = {36, 50, 138}, caps[] = {128, 32, 256};
            n = literal(bytes, find_vector(kinds[j]));
            text_field(bytes + 32 + offsets[j], caps[j], invalid[i], invalid_sizes[i]);
            assert(sophia_sf_decode(bytes, n, &record));
        }
    }
    for (i = 1; i <= 17; i++) {
        n = literal(bytes, find_vector(41));
        bytes[32 + 136] = (uint8_t)i;
        if (i != 1)
            memset(bytes + 32 + 138, 0, 260);
        if (i == 17)
            bytes[32 + 120] = 9;
        assert(!sophia_sf_decode(bytes, n, &record));
        bytes[32 + 120] = i == 17 ? 10 : 9;
        assert(sophia_sf_decode(bytes, n, &record));
    }
    for (i = 1; i <= 3; i++) {
        n = literal(bytes, find_vector(266));
        bytes[32 + 72] = (uint8_t)i;
        if (i != 1)
            bytes[32 + 56] = bytes[32 + 64] = 1;
        if (i == 3)
            memset(bytes + 32 + 76, 0, 16);
        assert(!sophia_sf_decode(bytes, n, &record));
        bytes[32 + 56] ^= 1;
        assert(sophia_sf_decode(bytes, n, &record));
    }
    {
        const char *identities[] = {"registered:", "desktop:", "unknown:x"};
        for (i = 0; i < 3; i++) {
            n = literal(bytes, find_vector(3));
            text_field(bytes + 32 + 428, 256, (const uint8_t *)identities[i],
                       strlen(identities[i]));
            assert(sophia_sf_decode(bytes, n, &record));
        }
    }
    for (i = 0; i < 2; i++) {
        n = literal(bytes, find_vector(i ? 42 : 269));
        bytes[32 + 120] = 10;
        assert(sophia_sf_decode(bytes, n, &record));
    }
}
static void catalog_identity_bijection(void)
{
    uint8_t bytes[64 + 3 * 656], encoded[sizeof(bytes)];
    struct sophia_sf_record r, decoded;
    size_t i, written;
    literal(bytes, find_vector(3));
    put32(bytes, sizeof(bytes));
    bytes[56] = 3;
    for (i = 1; i < 3; i++) {
        memcpy(bytes + 64 + i * 656, bytes + 64, 656);
        bytes[64 + i * 656] = (uint8_t)(i + 1);
        bytes[64 + i * 656 + 411] = (uint8_t)('x' + i);
    }
    assert(!sophia_sf_decode(bytes, sizeof(bytes), &r));
    assert(!sophia_sf_encode(encoded, sizeof(encoded), &r, &written));
    assert(written == sizeof(bytes) && !memcmp(bytes, encoded, sizeof(bytes)));
    /* Nonadjacent distinct slots must not name the same persistent identity.
     * r borrows these rows, so both encoding and decoding see this mutation. */
    bytes[64 + 2 * 656 + 411] = 'x';
    assert(sophia_sf_decode(bytes, sizeof(bytes), &decoded));
    assert(sophia_sf_encode(encoded, sizeof(encoded), &r, &written));
    /* Prefixes with different lengths are distinct names. */
    bytes[64 + 2 * 656 + 396] = 13;
    bytes[64 + 2 * 656 + 412] = 'y';
    assert(!sophia_sf_decode(bytes, sizeof(bytes), &r));
    /* A launcher catalog has no identities; repeated display labels are valid. */
    bytes[58] = 0;
    for (i = 0; i < 3; i++)
        memset(bytes + 64 + i * 656 + 396, 0, 260);
    assert(!sophia_sf_decode(bytes, sizeof(bytes), &r));
    assert(!sophia_sf_encode(encoded, sizeof(encoded), &r, &written));
}
int main(void)
{
    catalog_identity_bijection();
    vectors_and_truncations();
    refusal_controls();
    maximum_catalog();
    snapshot_rows();
    candidate_layers();
    maximum_candidates();
    semantic_controls();
    puts("shell role files: 17 independent KDL vectors and truncations passed");
    return 0;
}
