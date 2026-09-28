#include "sophia_shell_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

/* Literal native envelopes and rows from spec/proposed/, independent of the
 * old framed codec and of the Rust SDK. Header offsets are included here. */
#define HEADER(n, k)                                                                               \
    [0] = (n) & 255, [1] = (n) >> 8, [4] = 1, [6] = (k) & 255, [7] = (k) >> 8, [8] = 9, [16] = 5,  \
    [32] = 11, [40] = 9, [48] = 17, [56] = 18
static const uint8_t descriptor_wire[108] = {
    HEADER(108, 273), [64] = 19,   [72] = 1, [74] = 4,  [77] = 2,   [78] = 0x23, [79] = 1,
    [80] = 2,         [84] = 0x23, [85] = 1, [88] = 21, [96] = 255, [97] = 255,  [100] = 21};
static const uint8_t tabs_wire[92] = {
    HEADER(92, 275), [64] = 3,   [68] = 255, [69] = 255,  [70] = 255,  [71] = 255, [72] = 255,
    [73] = 255,      [74] = 255, [75] = 255, [76] = 0x34, [77] = 0x12, [84] = 1};
static const uint8_t reference_wire[468] = {
    HEADER(468, 276), [64] = 19,   [72] = 20,   [82] = 255,   [83] = 255,   [84] = 1,
    [88] = 10,        [90] = 16,   [92] = 4,    [94] = 2,     [96] = 3,     [98] = 5,
    [100] = 1,        [102] = 8,   [104] = 2,   [108] = 0x44, [109] = 0x33, [110] = 0x22,
    [111] = 0x11,     [112] = 3,   [113] = 2,   [114] = 1,    [115] = 255,  [116] = 6,
    [117] = 5,        [118] = 4,   [119] = 255, [120] = 9,    [121] = 8,    [122] = 7,
    [123] = 255,      [124] = 12,  [125] = 11,  [126] = 10,   [127] = 255,  [128] = 15,
    [129] = 14,       [130] = 13,  [131] = 255, [132] = 4,    [136] = 'K',  [137] = 'e',
    [138] = 'y',      [139] = 's', [264] = 255, [265] = 255,  [268] = 1,    [272] = 'X',
    [336] = 2,        [340] = 'g', [341] = 'o'};
static const uint8_t launcher_wire[110] = {
    HEADER(110, 277), [64] = 19, [72] = 20,  [83] = 16,   [84] = 3,  [86] = 10,  [92] = 3,
    [93] = 2,         [94] = 1,  [95] = 255, [96] = 6,    [97] = 5,  [98] = 4,   [99] = 255,
    [100] = 9,        [101] = 8, [102] = 7,  [103] = 255, [104] = 1, [107] = 16, [108] = 29};
static struct sophia_sf_record descriptor(void)
{
    struct sophia_sf_record r = {
        {SOPHIA_SF_DESCRIPTOR_CANDIDATE, 9, 5, 0},
        .value.descriptor_candidate = {
            11, 9, 17, 18, 19, 1, 4, 512, 0x123, 2, {{0x123, 21}, {65535, 21}}}};
    return r;
}
static struct sophia_sf_record tabs(void)
{
    struct sophia_sf_record r = {{SOPHIA_SF_TABS_CANDIDATE, 9, 5, 0},
                                 .value.tabs_candidate = {11, 9, 17, 18, 3, tabs_wire + 68, 24}};
    return r;
}
static struct sophia_sf_record reference(void)
{
    struct sophia_sf_record r = {
        {SOPHIA_SF_REFERENCE_CANDIDATE, 9, 5, 0},
        .value.reference_candidate = {
            11,
            9,
            17,
            18,
            19,
            20,
            0,
            65535,
            1,
            {10,
             16,
             4,
             2,
             3,
             5,
             1,
             8,
             2,
             {0x11223344, 0xff010203, 0xff040506, 0xff070809, 0xff0a0b0c, 0xff0d0e0f},
             {(const uint8_t *)"Keys", 4}},
            reference_wire + 264,
            204}};
    return r;
}
static struct sophia_sf_record launcher(void)
{
    struct sophia_sf_record r = {
        {SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE, 9, 5, 0},
        .value.descriptor_launcher_candidate = {11,
                                                9,
                                                17,
                                                18,
                                                19,
                                                20,
                                                0,
                                                4096,
                                                3,
                                                10,
                                                {0, 0xff010203, 0xff040506, 0xff070809},
                                                {1, 4096, 29}}};
    return r;
}
static unsigned wire_refusals;
static void bad_wire(const uint8_t *b, size_t n)
{
    struct sophia_sf_record out, before;
    memset(&out, 0xa5, sizeof(out));
    memcpy(&before, &out, sizeof(out));
    assert(sophia_sf_decode(b, n, &out) == -1);
    assert(!memcmp(&out, &before, sizeof(out)));
    wire_refusals++;
}
static void bad_value(const struct sophia_sf_record *r)
{
    uint8_t b[52489], before[52489];
    size_t n = 12345;
    memset(b, 0xa5, sizeof(b));
    memcpy(before, b, sizeof(b));
    assert(sophia_sf_encode(b, sizeof(b), r, &n) == -1);
    assert(n == 12345 && !memcmp(b, before, sizeof(b)));
}
static size_t roundtrip(const struct sophia_sf_record *r)
{
    uint8_t b[52489], again[52489];
    size_t n, m;
    struct sophia_sf_record out;
    assert(!sophia_sf_encode(b, sizeof(b), r, &n));
    assert(!sophia_sf_decode(b, n, &out));
    assert(!sophia_sf_encode(again, sizeof(again), &out, &m));
    assert(n == m && !memcmp(b, again, n));
    return n;
}
static void literal(const struct sophia_sf_record *r, const uint8_t *expected, size_t n,
                    unsigned identities)
{
    uint8_t b[52489];
    size_t written;
    struct sophia_sf_record out, changed;
    memset(b, 0xa5, sizeof(b));
    assert(!sophia_sf_encode(b, sizeof(b), r, &written));
    assert(written == n && !memcmp(b, expected, n) && b[n] == 0xa5);
    assert(!sophia_sf_decode(expected, n, &out));
    assert(!sophia_sf_encode(b, sizeof(b), &out, &written));
    assert(written == n && !memcmp(b, expected, n));
    for (size_t i = 0; i < n; i++) {
        memcpy(b, expected, n);
        b[0] = (uint8_t)i;
        b[1] = (uint8_t)(i >> 8);
        bad_wire(b, i);
        memset(b, 0xa5, sizeof(b));
        written = 12345;
        assert(sophia_sf_encode(b, i, r, &written) == -4 && written == 12345);
        for (size_t j = 0; j < sizeof(b); j++)
            assert(b[j] == 0xa5);
    }
    memcpy(b, expected, n);
    b[0] = (uint8_t)(n + 1);
    b[1] = (uint8_t)((n + 1) >> 8);
    b[n] = 0;
    bad_wire(b, n + 1);
    for (unsigned i = 0; i < identities; i++) {
        memcpy(b, expected, n);
        memset(b + 32 + i * 8, 0, 8);
        bad_wire(b, n);
    }
    memcpy(b, expected, n);
    b[40] = 8;
    bad_wire(b, n);
    memcpy(b, expected, n);
    b[24] = 1;
    bad_wire(b, n);
    memcpy(b, expected, n);
    b[16] = 0;
    bad_wire(b, n);
    changed = *r;
    changed.header.epoch = 8;
    bad_value(&changed);
    changed = *r;
    changed.header.epoch = 0;
    bad_value(&changed);
    changed = *r;
    changed.header.sequence = 1;
    bad_value(&changed);
}
static void mutate_byte(const uint8_t *wire, size_t n, size_t at, uint8_t value)
{
    uint8_t b[52489];
    memcpy(b, wire, n);
    b[at] = value;
    bad_wire(b, n);
}
static void descriptor_rules(void)
{
    struct sophia_sf_record r = descriptor();
    literal(&r, descriptor_wire, sizeof(descriptor_wire), 5);
    for (unsigned edge = 0; edge <= 5; edge++) {
        r = descriptor();
        r.value.descriptor_candidate.reservation_edge = (uint16_t)edge;
        r.value.descriptor_candidate.reservation_thickness = edge ? 512 : 0;
        if (edge <= 4)
            assert(roundtrip(&r) == 108);
        else
            bad_value(&r);
    }
    r = descriptor();
    r.value.descriptor_candidate.visible = 0;
    bad_value(&r);
    {
        struct sophia_sf_record hidden = r;
        hidden.value.descriptor_candidate.selected_slot = 0;
        hidden.value.descriptor_candidate.reservation_edge = 0;
        hidden.value.descriptor_candidate.reservation_thickness = 0;
        bad_value(&hidden); /* Entries alone must prevent a hidden candidate. */
    }
    r.value.descriptor_candidate.entry_count = 0;
    bad_value(&r);
    r.value.descriptor_candidate.selected_slot = 0;
    bad_value(&r);
    r.value.descriptor_candidate.reservation_edge = 0;
    r.value.descriptor_candidate.reservation_thickness = 0;
    assert(roundtrip(&r) == 84);
    r.value.descriptor_candidate.visible = 1;
    bad_value(&r);
    r = descriptor();
    r.value.descriptor_candidate.selected_slot = 7;
    bad_value(&r);
    r = descriptor();
    r.value.descriptor_candidate.reservation_thickness = 513;
    bad_value(&r);
    r = descriptor();
    r.value.descriptor_candidate.reservation_thickness = 0;
    bad_value(&r);
    r = descriptor();
    r.value.descriptor_candidate.entry_count = 16;
    for (unsigned i = 0; i < 16; i++)
        r.value.descriptor_candidate.entries[i] =
            (struct sophia_sf_descriptor_candidate_entry){(uint16_t)(i + 1), UINT64_MAX};
    r.value.descriptor_candidate.selected_slot = 16;
    assert(roundtrip(&r) == 276);
    {
        uint8_t b[288];
        size_t n;
        assert(!sophia_sf_encode(b, sizeof(b), &r, &n) && n == 276);
        memset(b + 276, 0, 12);
        b[0] = 32;
        b[1] = 1;
        b[80] = 17;
        b[276] = 17;
        b[280] = 1;
        bad_wire(b, sizeof(b)); /* Complete seventeenth row, not just a short record. */
    }
    r.value.descriptor_candidate.entry_count = 17;
    bad_value(&r);
    r = descriptor();
    r.value.descriptor_candidate.entries[1].slot = r.value.descriptor_candidate.entries[0].slot;
    bad_value(&r);
    mutate_byte(descriptor_wire, 108, 72, 2);
    mutate_byte(descriptor_wire, 108, 74, 5);
    mutate_byte(descriptor_wire, 108, 82, 1);
    mutate_byte(descriptor_wire, 108, 86, 1);
    mutate_byte(descriptor_wire, 108, 98, 1);
    mutate_byte(descriptor_wire, 108, 100, 0);
    mutate_byte(descriptor_wire, 108, 80, 17);
}
static void tabs_rules(void)
{
    struct sophia_sf_record r = tabs(), out;
    uint8_t rows[1024 * 8], b[8268];
    uint64_t slot = 12345;
    size_t n;
    literal(&r, tabs_wire, sizeof(tabs_wire), 4);
    assert(!sophia_sf_decode(tabs_wire, 92, &out));
    assert(out.value.tabs_candidate.rows == tabs_wire + 68);
    assert(!sophia_sf_tab_order_at(&out.value.tabs_candidate, 0, &slot) && slot == UINT64_MAX);
    slot = 12345;
    assert(sophia_sf_tab_order_at(&out.value.tabs_candidate, SIZE_MAX, &slot) == -4 &&
           slot == 12345);
    assert(!sophia_sf_tab_order_at(&out.value.tabs_candidate, 1, &slot) && slot == 0x1234);
    for (unsigned i = 0; i < 1024; i++)
        assert(!sophia_sf_tab_order_encode(rows + i * 8, UINT64_MAX - i));
    r.value.tabs_candidate.rows = rows;
    r.value.tabs_candidate.rows_bytes = sizeof(rows);
    r.value.tabs_candidate.group_count = 1024;
    r.value.tabs_candidate.candidate_generation =
        UINT64_MAX; /* Owner rejects its reserved high bit. */
    assert(roundtrip(&r) == 8260);
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!memcmp(b + 68, rows, sizeof(rows))); /* Validation never sorts the borrowed view. */
    b[0] = 76;
    b[1] = 32;
    b[64] = 1;
    b[65] = 4;
    assert(!sophia_sf_tab_order_encode(b + 8260, 1));
    bad_wire(b, sizeof(b)); /* Complete 1025th distinct group. */
    assert(!sophia_sf_tab_order_encode(rows + 1023 * 8, UINT64_MAX));
    bad_value(&r);
    assert(!sophia_sf_tab_order_encode(rows + 1023 * 8, UINT64_MAX - 1023));
    r.value.tabs_candidate.group_count = 1025;
    bad_value(&r);
    r.value.tabs_candidate.group_count = 1024;
    r.value.tabs_candidate.rows_bytes--;
    bad_value(&r);
    r = tabs();
    r.value.tabs_candidate.rows = NULL;
    bad_value(&r);
    r.value.tabs_candidate.rows_bytes = 0;
    r.value.tabs_candidate.group_count = 0;
    assert(roundtrip(&r) == 68);
    memset(rows, 0, sizeof(rows));
    r = tabs();
    r.value.tabs_candidate.rows = rows;
    bad_value(&r);
    slot = 12345;
    assert(sophia_sf_tab_order_at(&r.value.tabs_candidate, 0, &slot) == -1 && slot == 12345);
    memset(rows, 0xa5, 8);
    assert(sophia_sf_tab_order_encode(rows, 0) == -1);
    for (unsigned i = 0; i < 8; i++)
        assert(rows[i] == 0xa5);
    mutate_byte(tabs_wire, 92, 66, 1);
    mutate_byte(tabs_wire, 92, 64, 4);
}
static void reference_rules(void)
{
    struct sophia_sf_record r = reference(), out;
    struct sophia_sf_reference_entry e, before;
    uint8_t rows[257 * 204], row[204];
    literal(&r, reference_wire, sizeof(reference_wire), 6);
    assert(!sophia_sf_decode(reference_wire, 468, &out));
    assert(out.value.reference_candidate.rows == reference_wire + 264);
    assert(out.value.reference_candidate.style.title.data == reference_wire + 136);
    assert(!sophia_sf_reference_entry_at(&out.value.reference_candidate, 0, &e));
    assert(e.slot == 65535 && e.key.size == 1 && e.key.data == reference_wire + 272 &&
           e.label.size == 2 && e.label.data == reference_wire + 340);
    assert(!sophia_sf_reference_entry_encode(row, &e));
    assert(!memcmp(row, reference_wire + 264, 204));
    memcpy(&before, &e, sizeof(e));
    row[2] = 1;
    assert(sophia_sf_reference_entry_decode(row, &e) == -1 && !memcmp(&e, &before, sizeof(e)));
    assert(sophia_sf_reference_entry_at(&out.value.reference_candidate, SIZE_MAX, &e) == -4 &&
           !memcmp(&e, &before, sizeof(e)));
    e.key.size = 0;
    memset(row, 0xa5, sizeof(row));
    assert(sophia_sf_reference_entry_encode(row, &e) == -1);
    for (unsigned i = 0; i < 204; i++)
        assert(row[i] == 0xa5);
    e = before;
    for (unsigned i = 0; i < 257; i++) {
        e.slot = (uint16_t)(65535 - i);
        assert(!sophia_sf_reference_entry_encode(rows + i * 204, &e));
    }
    r.value.reference_candidate.rows = rows;
    r.value.reference_candidate.rows_bytes = 256 * 204;
    r.value.reference_candidate.entry_count = 256;
    assert(roundtrip(&r) == 52488);
    {
        uint8_t b[52692];
        memcpy(b, reference_wire, 264);
        memcpy(b + 264, rows, sizeof(rows));
        b[0] = 212;
        b[1] = 205;
        b[84] = 1;
        b[85] = 1;
        bad_wire(b, sizeof(b)); /* Complete 257th distinct reference entry. */
    }
    memcpy(rows + 255 * 204, rows, 204);
    bad_value(&r);
    r.value.reference_candidate.entry_count = 257;
    r.value.reference_candidate.rows_bytes = sizeof(rows);
    bad_value(&r);
    r = reference();
    r.value.reference_candidate.rows_bytes--;
    bad_value(&r);
    r = reference();
    r.value.reference_candidate.rows = NULL;
    bad_value(&r);
    r.value.reference_candidate.rows_bytes = 0;
    r.value.reference_candidate.entry_count = 0;
    assert(roundtrip(&r) == 264); /* Empty projection and page are owner decisions. */
    r.value.reference_candidate.visible = 1;
    assert(roundtrip(&r) == 264);
    r.value.reference_candidate.visible = 2;
    bad_value(&r);
    const unsigned zero_pad[] = {86,  87,  106, 107, 134, 135, 263, 266,
                                 267, 270, 271, 335, 338, 339, 467};
    for (unsigned i = 0; i < sizeof(zero_pad) / sizeof(zero_pad[0]); i++)
        mutate_byte(reference_wire, 468, zero_pad[i], 1);
    mutate_byte(reference_wire, 468, 268, 0);
    mutate_byte(reference_wire, 468, 336, 0);
    mutate_byte(reference_wire, 468, 132, 0);
    mutate_byte(reference_wire, 468, 272, '\n');
    mutate_byte(reference_wire, 468, 340, 0x80);
    mutate_byte(reference_wire, 468, 136, 0);
    mutate_byte(reference_wire, 468, 80, 2);
    for (unsigned i = 0; i < 6; i++) {
        r = reference();
        r.value.reference_candidate.style.colors[i] &= 0x00ffffff;
        if (i)
            bad_value(&r);
        else
            assert(roundtrip(&r) == 468);
    }
}
static void style_bounds(void)
{
    struct sophia_sf_record r;
    /* Native field offsets, low, high; each endpoint and both excluded sides. */
    const unsigned bounds[][3] = {{56, 8, 32}, {58, 8, 48}, {60, 0, 64},  {62, 0, 32}, {64, 0, 64},
                                  {66, 0, 64}, {68, 0, 16}, {70, 0, 128}, {72, 1, 4}};
    for (unsigned i = 0; i < 9; i++) {
        uint8_t b[468];
        memcpy(b, reference_wire, sizeof(b));
        b[32 + bounds[i][0]] = (uint8_t)bounds[i][1];
        assert(!sophia_sf_decode(b, sizeof(b), &r));
        b[32 + bounds[i][0]] = (uint8_t)bounds[i][2];
        assert(!sophia_sf_decode(b, sizeof(b), &r));
        b[32 + bounds[i][0]] = (uint8_t)(bounds[i][2] + 1);
        bad_wire(b, sizeof(b));
        if (bounds[i][1]) {
            b[32 + bounds[i][0]] = (uint8_t)(bounds[i][1] - 1);
            bad_wire(b, sizeof(b));
        }
    }
}
static void reference_text_bounds(void)
{
    uint8_t text[129], row[204];
    struct sophia_sf_reference_entry e;
    struct sophia_sf_record r = reference();
    memset(text, 'x', sizeof(text));
    e = (struct sophia_sf_reference_entry){65535, {text, 64}, {text, 128}};
    assert(!sophia_sf_reference_entry_encode(row, &e));
    r.value.reference_candidate.rows = row;
    r.value.reference_candidate.style.title = (struct sophia_sf_text){text, 128};
    assert(roundtrip(&r) == 468);
    r.value.reference_candidate.style.title.size = 129;
    bad_value(&r);
    e.key.size = 65;
    assert(sophia_sf_reference_entry_encode(row, &e) == -1);
    e.key.size = 64;
    e.label.size = 129;
    assert(sophia_sf_reference_entry_encode(row, &e) == -1);
    const struct sophia_sf_text invalid[] = {{NULL, 0},
                                             {NULL, 1},
                                             {(const uint8_t *)"\n", 1},
                                             {(const uint8_t *)"\xc2\x85", 2},
                                             {(const uint8_t *)"\xe2\x80\xae", 3},
                                             {(const uint8_t *)"\xe2\x81\xa6", 3},
                                             {(const uint8_t *)"\xc0\xaf", 2},
                                             {(const uint8_t *)"\xed\xa0\x80", 3}};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        r = reference();
        r.value.reference_candidate.style.title = invalid[i];
        bad_value(&r);
        e = (struct sophia_sf_reference_entry){1, invalid[i], {text, 1}};
        assert(sophia_sf_reference_entry_encode(row, &e) == -1);
        e.key = (struct sophia_sf_text){text, 1};
        e.label = invalid[i];
        assert(sophia_sf_reference_entry_encode(row, &e) == -1);
    }
}
static void launcher_rules(void)
{
    struct sophia_sf_record r = launcher();
    literal(&r, launcher_wire, sizeof(launcher_wire), 6);
    r.value.descriptor_launcher_candidate.entry_count = 32;
    for (unsigned i = 0; i < 32; i++)
        r.value.descriptor_launcher_candidate.entries[i] = (uint16_t)(4096 - i);
    assert(roundtrip(&r) == 168);
    {
        uint8_t b[170];
        size_t n;
        assert(!sophia_sf_encode(b, sizeof(b), &r, &n) && n == 168);
        b[0] = 170;
        b[84] = 33;
        b[168] = 1;
        b[169] = 0;
        bad_wire(b, sizeof(b)); /* Complete 33rd distinct row. */
    }
    r.value.descriptor_launcher_candidate.entry_count = 33;
    bad_value(&r);
    r = launcher();
    r.value.descriptor_launcher_candidate.entries[2] = 1;
    bad_value(&r);
    r = launcher();
    r.value.descriptor_launcher_candidate.entries[2] = 4097;
    bad_value(&r);
    r = launcher();
    r.value.descriptor_launcher_candidate.entries[2] = 0;
    bad_value(&r);
    r = launcher();
    r.value.descriptor_launcher_candidate.selected = 2;
    bad_value(&r);
    r = launcher();
    r.value.descriptor_launcher_candidate.selected = 0;
    assert(roundtrip(&r) == 110);
    r.value.descriptor_launcher_candidate.entry_count = 0;
    assert(roundtrip(&r) == 104);
    r.value.descriptor_launcher_candidate.visible = 1;
    assert(roundtrip(&r) == 104);
    r.value.descriptor_launcher_candidate.font_size = 32;
    assert(roundtrip(&r) == 104);
    r.value.descriptor_launcher_candidate.font_size = 33;
    bad_value(&r);
    r.value.descriptor_launcher_candidate.font_size = 9;
    bad_value(&r);
    for (unsigned i = 0; i < 4; i++) {
        r = launcher();
        r.value.descriptor_launcher_candidate.colors[i] &= 0x00ffffff;
        if (i)
            bad_value(&r);
        else
            assert(roundtrip(&r) == 110);
    }
    mutate_byte(launcher_wire, 110, 80, 2);
    mutate_byte(launcher_wire, 110, 84, 33);
    mutate_byte(launcher_wire, 110, 86, 9);
    mutate_byte(launcher_wire, 110, 108, 1);
    mutate_byte(launcher_wire, 110, 106, 1);
}
static void receipts(void)
{
    struct sophia_sf_record r = {{SOPHIA_SF_SUBMITTED, 9, 0, 1}, .value.submitted = {5, 273}};
    for (uint16_t kind = 273; kind <= 278; kind++) {
        r.value.submitted.candidate_kind = kind;
        assert(roundtrip(&r) == 48);
    }
    r.value.submitted.candidate_kind = 279;
    bad_value(&r);
}
int main(void)
{
    descriptor_rules();
    tabs_rules();
    reference_rules();
    style_bounds();
    reference_text_bounds();
    launcher_rules();
    receipts();
    printf("descriptor candidates: 4 literal vectors, maxima, borrowed rows and %u malformed-wire "
           "refusals passed\n",
           wire_refusals);
    return 0;
}
