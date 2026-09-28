#include "sophia_shell_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

/* Native file vectors authored from the native file KDL. No IPC or Rust codec. */
#define GENERATION UINT64_C(0x0102030405060708)
#define HEADER(n, k)                                                                               \
    [0] = (n) & 255, [1] = (n) >> 8, [4] = 1, [6] = (k), [8] = 9, [32] = 11, [40] = 9, [48] = 17
#define ENTRY(at, lo, hi, issuer, revoke)                                                          \
    [at] = (lo), [at + 1] = (hi), [at + 12] = 8, [at + 13] = 7, [at + 14] = 6, [at + 15] = 5,      \
    [at + 16] = 4, [at + 17] = 3, [at + 18] = 2, [at + 19] = 1, [at + 20] = 22,                    \
    [at + 28] = (issuer), [at + 36] = (revoke), [at + 44] = 9, [at + 52] = (lo), [at + 53] = (hi), \
    [at + 56] = 8, [at + 57] = 7, [at + 58] = 6, [at + 59] = 5, [at + 60] = 4, [at + 61] = 3,      \
    [at + 62] = 2, [at + 63] = 1
#define LABEL(at)                                                                                  \
    [at + 2] = 3, [at + 4] = 2, [at + 6] = 1, [at + 8] = 1, [at + 64] = 3, [at + 68] = 'a',        \
          [at + 69] = 0xc3, [at + 70] = 0xa9
static const uint8_t descriptors_wire[292] = {HEADER(292, 5),
                                              [56] = 18,
                                              [64] = 19,
                                              [72] = 20,
                                              [80] = 21,
                                              [88] = 1,
                                              ENTRY(96, 0x23, 1, 20, 21),
                                              LABEL(96)};
static const uint8_t tabs_wire[504] = {HEADER(504, 6), [56] = 2,
                                       [58] = 2,       [64] = 255,
                                       [65] = 255,     [66] = 255,
                                       [67] = 255,     [68] = 255,
                                       [69] = 255,     [70] = 255,
                                       [71] = 255,     [72] = 18,
                                       [80] = 0x23,    [81] = 1,
                                       [82] = 1,       [84] = 1,
                                       [88] = 2,       [96] = 19,
                                       [104] = 255,    [105] = 255,
                                       [108] = 1,      ENTRY(112, 0x23, 1, 20, 21),
                                       LABEL(112),     ENTRY(308, 255, 255, 30, 31)};
static const uint8_t shortcuts_wire[472] = {
    HEADER(472, 7), [56] = 1,   [64] = 255,  [65] = 255,  [66] = 1,    [72] = 3,    [76] = 'M',
    [77] = '-',     [78] = '1', [140] = 5,   [144] = 'f', [145] = 'o', [146] = 'c', [147] = 'u',
    [148] = 's',    [272] = 3,  [276] = 'O', [277] = 'n', [278] = 'e'};
static struct sophia_sf_descriptor_entry entry(uint16_t slot)
{
    struct sophia_sf_descriptor_entry e = {slot,
                                           3,
                                           2,
                                           1,
                                           1,
                                           GENERATION,
                                           22,
                                           20,
                                           21,
                                           9,
                                           slot,
                                           GENERATION,
                                           {(const uint8_t *)"a\xc3\xa9", 3}};
    return e;
}
static struct sophia_sf_record descriptors(void)
{
    struct sophia_sf_record r = {
        {SOPHIA_SF_DESCRIPTORS, 9, 0, 0},
        .value.descriptors = {11, 9, 17, 18, 19, 20, 21, 1, descriptors_wire + 96, 196}};
    return r;
}
static struct sophia_sf_record tabs(void)
{
    struct sophia_sf_record r = {{SOPHIA_SF_TABS, 9, 0, 0},
                                 .value.tabs = {11, 9, 17, 2, 2, tabs_wire + 64, 440}};
    return r;
}
static struct sophia_sf_shortcut_entry shortcut(uint16_t slot)
{
    struct sophia_sf_shortcut_entry e = {slot,
                                         1,
                                         0,
                                         {(const uint8_t *)"M-1", 3},
                                         {(const uint8_t *)"focus", 5},
                                         {(const uint8_t *)"One", 3},
                                         {NULL, 0}};
    return e;
}
static struct sophia_sf_record shortcuts(void)
{
    struct sophia_sf_record r = {{SOPHIA_SF_SHORTCUTS, 9, 0, 0},
                                 .value.shortcuts = {11, 9, 17, 1, shortcuts_wire + 64, 408}};
    return r;
}
static unsigned wire_refusals;
static void bad_wire(const uint8_t *b, size_t n)
{
    struct sophia_sf_record out, before;
    memset(&out, 0xa5, sizeof(out));
    memcpy(&before, &out, sizeof(out));
    assert(sophia_sf_decode(b, n, &out) == -1 && !memcmp(&out, &before, sizeof(out)));
    wire_refusals++;
}
static void bad_value(const struct sophia_sf_record *r)
{
    static uint8_t b[1048577], before[1048577];
    size_t n = 12345;
    memset(b, 0xa5, sizeof(b));
    memcpy(before, b, sizeof(b));
    assert(sophia_sf_encode(b, sizeof(b), r, &n) == -1);
    assert(n == 12345 && !memcmp(b, before, sizeof(b)));
}
static size_t roundtrip(const struct sophia_sf_record *r)
{
    static uint8_t b[1048577], again[1048577];
    struct sophia_sf_record out;
    size_t n, m;
    assert(!sophia_sf_encode(b, sizeof(b), r, &n));
    assert(!sophia_sf_decode(b, n, &out));
    assert(!sophia_sf_encode(again, sizeof(again), &out, &m));
    assert(n == m && !memcmp(b, again, n));
    return n;
}
static void literal(const struct sophia_sf_record *r, const uint8_t *wire, size_t n,
                    unsigned identities)
{
    uint8_t b[1024];
    struct sophia_sf_record out, invalid;
    size_t written;
    memset(b, 0xa5, sizeof(b));
    assert(!sophia_sf_encode(b, sizeof(b), r, &written));
    assert(written == n && !memcmp(b, wire, n) && b[n] == 0xa5);
    assert(!sophia_sf_decode(wire, n, &out));
    assert(!sophia_sf_encode(b, sizeof(b), &out, &written));
    assert(written == n && !memcmp(b, wire, n));
    for (size_t i = 0; i < n; i++) {
        memcpy(b, wire, n);
        b[0] = (uint8_t)i;
        b[1] = (uint8_t)(i >> 8);
        bad_wire(b, i);
        memset(b, 0xa5, sizeof(b));
        written = 12345;
        assert(sophia_sf_encode(b, i, r, &written) == -4 && written == 12345);
        for (size_t j = 0; j < sizeof(b); j++)
            assert(b[j] == 0xa5);
    }
    memcpy(b, wire, n);
    b[0] = (uint8_t)(n + 1);
    b[1] = (uint8_t)((n + 1) >> 8);
    b[n] = 0;
    bad_wire(b, n + 1);
    for (unsigned i = 0; i < identities; i++) {
        memcpy(b, wire, n);
        memset(b + 32 + i * 8, 0, 8);
        bad_wire(b, n);
    }
    memcpy(b, wire, n);
    b[40] = 8;
    bad_wire(b, n);
    memcpy(b, wire, n);
    b[16] = 1;
    bad_wire(b, n);
    memcpy(b, wire, n);
    b[24] = 1;
    bad_wire(b, n);
    invalid = *r;
    invalid.header.epoch = 8;
    bad_value(&invalid);
    invalid = *r;
    invalid.header.submission = 1;
    bad_value(&invalid);
    invalid = *r;
    invalid.header.sequence = 1;
    bad_value(&invalid);
}
static void mutate_byte(const uint8_t *wire, size_t n, size_t at, uint8_t value)
{
    uint8_t b[1024];
    assert(n <= sizeof(b));
    memcpy(b, wire, n);
    b[at] = value;
    bad_wire(b, n);
}
static void descriptor_row_tests(void)
{
    struct sophia_sf_descriptor_entry e = entry(0x123), out, before;
    uint8_t b[196], untouched[196], text[129];
    assert(!sophia_sf_descriptor_entry_encode(b, &e));
    assert(!memcmp(b, descriptors_wire + 96, sizeof(b)));
    assert(!sophia_sf_descriptor_entry_decode(descriptors_wire + 96, &out));
    assert(out.label.data == descriptors_wire + 164 && out.label.size == 3);
    before = out;
    b[10] = 1;
    assert(sophia_sf_descriptor_entry_decode(b, &out) == -1 && !memcmp(&out, &before, sizeof(out)));
    memset(untouched, 0xa5, sizeof(untouched));
    /* Descriptor labels preserve bidi characters; shortcut text does not. */
    const struct sophia_sf_text allowed[] = {{(const uint8_t *)"\xe2\x80\xae", 3},
                                             {(const uint8_t *)"\xe2\x81\xa6", 3}};
    for (unsigned i = 0; i < 2; i++) {
        e = entry(1);
        e.label = allowed[i];
        assert(!sophia_sf_descriptor_entry_encode(b, &e));
        assert(!sophia_sf_descriptor_entry_decode(b, &out));
    }
    const struct sophia_sf_text invalid[] = {{NULL, 0},
                                             {NULL, 1},
                                             {(const uint8_t *)"\n", 1},
                                             {(const uint8_t *)"\xc2\x85", 2},
                                             {(const uint8_t *)"\xc0\xaf", 2},
                                             {(const uint8_t *)"\xed\xa0\x80", 3},
                                             {(const uint8_t *)"\xf4\x90\x80\x80", 4},
                                             {(const uint8_t *)"\xc3", 1}};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        e = entry(1);
        e.label = invalid[i];
        memcpy(b, untouched, sizeof(b));
        assert(sophia_sf_descriptor_entry_encode(b, &e) == -1 && !memcmp(b, untouched, sizeof(b)));
    }
    e = entry(1);
    memset(text, 'x', sizeof(text));
    e.label = (struct sophia_sf_text){text, 128};
    assert(!sophia_sf_descriptor_entry_encode(b, &e));
    e.label.size = 129;
    assert(sophia_sf_descriptor_entry_encode(b, &e) == -1);
    e = entry(1);
    e.label_present = 0;
    assert(sophia_sf_descriptor_entry_encode(b, &e) == -1);
    e.label = (struct sophia_sf_text){NULL, 0};
    assert(sophia_sf_descriptor_entry_encode(b, &e) == -1);
    e.label_redacted = 0;
    assert(!sophia_sf_descriptor_entry_encode(b, &e));
    assert(!sophia_sf_descriptor_entry_decode(b, &out));
    assert(!out.label_present && !out.label_redacted && !out.label.size);
    e = entry(1);
    e.action_target_slot = 2;
    assert(sophia_sf_descriptor_entry_encode(b, &e) == -1);
    e = entry(1);
    e.action_target_generation++;
    assert(sophia_sf_descriptor_entry_encode(b, &e) == -1);
}
static void descriptor_snapshots(void)
{
    struct sophia_sf_record r = descriptors(), out;
    struct sophia_sf_descriptor_entry e = entry(1), got, before;
    uint8_t rows[17 * 196];
    literal(&r, descriptors_wire, sizeof(descriptors_wire), 7);
    assert(!sophia_sf_decode(descriptors_wire, 292, &out));
    assert(out.value.descriptors.rows == descriptors_wire + 96);
    assert(!sophia_sf_descriptor_entry_at(&out.value.descriptors, 0, &got));
    before = got;
    assert(sophia_sf_descriptor_entry_at(&out.value.descriptors, SIZE_MAX, &got) == -4 &&
           !memcmp(&got, &before, sizeof(got)));
    for (unsigned i = 0; i < 17; i++) {
        e = entry((uint16_t)(65535 - i));
        assert(!sophia_sf_descriptor_entry_encode(rows + i * 196, &e));
    }
    r.value.descriptors.rows = rows;
    r.value.descriptors.rows_bytes = 16 * 196;
    r.value.descriptors.descriptor_count = 16;
    assert(roundtrip(&r) == 3232);
    r.value.descriptors.rows_bytes = sizeof(rows);
    r.value.descriptors.descriptor_count = 17;
    bad_value(&r);
    r.value.descriptors.rows_bytes = 16 * 196;
    r.value.descriptors.descriptor_count = 16;
    memcpy(rows + 15 * 196, rows, 196);
    bad_value(&r);
    r = descriptors();
    r.value.descriptors.rows = NULL;
    bad_value(&r);
    r.value.descriptors.rows_bytes = 0;
    r.value.descriptors.descriptor_count = 0;
    assert(roundtrip(&r) == 96);
    const unsigned pad[] = {90, 91, 92, 93, 94, 95, 106, 107, 150, 151, 162, 163, 291};
    for (unsigned i = 0; i < sizeof(pad) / sizeof(pad[0]); i++)
        mutate_byte(descriptors_wire, 292, pad[i], 1);
    mutate_byte(descriptors_wire, 292, 98, 4);
    mutate_byte(descriptors_wire, 292, 100, 3);
    mutate_byte(descriptors_wire, 292, 102, 2);
    mutate_byte(descriptors_wire, 292, 104, 2);
    mutate_byte(descriptors_wire, 292, 108, 0); /* Generation now differs from action target. */
    mutate_byte(descriptors_wire, 292, 124, 30);
    mutate_byte(descriptors_wire, 292, 132, 31);
    mutate_byte(descriptors_wire, 292, 140, 10);
    mutate_byte(descriptors_wire, 292, 148, 0x24);
    mutate_byte(descriptors_wire, 292, 152, 0);
    mutate_byte(descriptors_wire, 292, 164, '\n');
    const unsigned nonzero[] = {116, 124, 132, 140};
    for (unsigned i = 0; i < 4; i++)
        mutate_byte(descriptors_wire, 292, nonzero[i], 0);
}
static void tab_snapshots(void)
{
    static uint8_t rows[1025 * 24 + 2049 * 196];
    struct sophia_sf_record r = tabs(), out;
    struct sophia_sf_tab_group group = {UINT64_MAX, 18, 0x123, 1, 1}, g, before;
    struct sophia_sf_descriptor_entry e, got;
    uint8_t b[24], two_groups[440];
    literal(&r, tabs_wire, sizeof(tabs_wire), 3);
    assert(!sophia_sf_tab_group_encode(b, &group) && !memcmp(b, tabs_wire + 64, 24));
    assert(!sophia_sf_decode(tabs_wire, 504, &out));
    assert(out.value.tabs.rows == tabs_wire + 64);
    assert(!sophia_sf_tab_group_at(&out.value.tabs, 1, &g) && g.group_slot == 2 &&
           g.selected_slot == 65535);
    before = g;
    assert(sophia_sf_tab_group_at(&out.value.tabs, SIZE_MAX, &g) == -4 &&
           !memcmp(&g, &before, sizeof(g)));
    assert(!sophia_sf_tab_entry_at(&out.value.tabs, 1, &got));
    assert(got.slot == 65535 && got.action_issuer_epoch == 30 && !got.label_present);
    /* A duplicate across groups remains invalid even when each selection
     * correctly names that group's entry. */
    memcpy(two_groups, tabs_wire + 64, sizeof(two_groups));
    g = (struct sophia_sf_tab_group){2, 19, 0x123, 0, 1};
    assert(!sophia_sf_tab_group_encode(two_groups + 24, &g));
    e = entry(0x123);
    e.action_issuer_epoch = 30;
    e.action_issuer_revocation_epoch = 31;
    assert(!sophia_sf_descriptor_entry_encode(two_groups + 48 + 196, &e));
    r.value.tabs.rows = two_groups;
    bad_value(&r);
    r = tabs();
    memcpy(two_groups, tabs_wire + 64, sizeof(two_groups));
    r.value.tabs.rows = two_groups;
    two_groups[48 + 196 + 44] = 10;
    bad_value(&r); /* Per-entry recipient belongs to this connection. */
    /* One group may hold all 2048 descriptors; there is no standalone 16-row cap. */
    group = (struct sophia_sf_tab_group){1, 18, 2048, 1, 2048};
    assert(!sophia_sf_tab_group_encode(rows, &group));
    for (unsigned i = 0; i < 2048; i++) {
        e = entry((uint16_t)(i + 1));
        assert(!sophia_sf_descriptor_entry_encode(rows + 24 + i * 196, &e));
    }
    r = tabs();
    r.value.tabs.rows = rows;
    r.value.tabs.group_count = 1;
    r.value.tabs.entry_count = 2048;
    r.value.tabs.rows_bytes = 24 + 2048 * 196;
    assert(roundtrip(&r) == 401496);
    /* Both global maxima at once, with descriptor slots distinct across groups. */
    for (unsigned i = 0; i < 1024; i++) {
        group = (struct sophia_sf_tab_group){UINT64_MAX - i, 18, (uint16_t)(2 * i + 1),
                                             (uint16_t)(i % 2), 2};
        assert(!sophia_sf_tab_group_encode(rows + i * 24, &group));
    }
    for (unsigned i = 0; i < 2048; i++) {
        e = entry((uint16_t)(i + 1));
        e.action_issuer_epoch = 20 + i;
        e.action_issuer_revocation_epoch = 21 + i;
        assert(!sophia_sf_descriptor_entry_encode(rows + 1024 * 24 + i * 196, &e));
    }
    r.value.tabs.group_count = 1024;
    r.value.tabs.rows_bytes = 1024 * 24 + 2048 * 196;
    assert(roundtrip(&r) == 426048);
    r.value.tabs.entry_count = 2049;
    r.value.tabs.rows_bytes += 196;
    bad_value(&r);
    /* A complete 2049-row view with a matching group sum fails the global cap
     * independently of group partition validation. Both groups fit their cap. */
    group = (struct sophia_sf_tab_group){1, 18, 1, 1, 1024};
    assert(!sophia_sf_tab_group_encode(rows, &group));
    group = (struct sophia_sf_tab_group){2, 19, 1025, 0, 1025};
    assert(!sophia_sf_tab_group_encode(rows + 24, &group));
    for (unsigned i = 0; i < 2049; i++) {
        e = entry((uint16_t)(i + 1));
        assert(!sophia_sf_descriptor_entry_encode(rows + 48 + i * 196, &e));
    }
    r.value.tabs.group_count = 2;
    r.value.tabs.rows_bytes = 48 + 2049 * 196;
    bad_value(&r);
    /* Complete, otherwise valid 1025 empty groups. */
    for (unsigned i = 0; i < 1025; i++) {
        group = (struct sophia_sf_tab_group){i + 1, 18, 0, 0, 0};
        assert(!sophia_sf_tab_group_encode(rows + i * 24, &group));
    }
    r.value.tabs.group_count = 1025;
    r.value.tabs.entry_count = 0;
    r.value.tabs.rows_bytes = 1025 * 24;
    bad_value(&r);
    r.value.tabs.group_count = 1024;
    r.value.tabs.rows_bytes = 1024 * 24;
    assert(roundtrip(&r) == 24640);
    memcpy(rows + 1023 * 24, rows, 24);
    bad_value(&r);
    r = tabs();
    r.value.tabs.rows = NULL;
    bad_value(&r);
    r.value.tabs.rows_bytes = 0;
    r.value.tabs.group_count = 0;
    r.value.tabs.entry_count = 0;
    assert(roundtrip(&r) == 64);
    const unsigned pads[] = {60,  61,  62,  63,  86,  87,  110, 111, 122, 123, 166,
                             167, 178, 179, 307, 318, 319, 362, 363, 374, 375, 503};
    for (unsigned i = 0; i < sizeof(pads) / sizeof(pads[0]); i++)
        mutate_byte(tabs_wire, 504, pads[i], 1);
    mutate_byte(tabs_wire, 504, 82, 2);
    mutate_byte(tabs_wire, 504, 84, 2);
    mutate_byte(tabs_wire, 504, 108, 0);
    mutate_byte(tabs_wire, 504, 80, 0x24);
    mutate_byte(tabs_wire, 504, 104, 0);
    mutate_byte(tabs_wire, 504, 352, 10);
    mutate_byte(tabs_wire, 504, 56, 1);
    mutate_byte(tabs_wire, 504, 58, 1);
}
static void shortcut_rows_and_snapshots(void)
{
    struct sophia_sf_record r = shortcuts(), out;
    struct sophia_sf_shortcut_entry e = shortcut(65535), got, before;
    uint8_t b[408], text[129];
    static uint8_t rows[257 * 408];
    literal(&r, shortcuts_wire, sizeof(shortcuts_wire), 3);
    assert(!sophia_sf_shortcut_entry_encode(b, &e) && !memcmp(b, shortcuts_wire + 64, 408));
    assert(!sophia_sf_decode(shortcuts_wire, 472, &out));
    assert(out.value.shortcuts.rows == shortcuts_wire + 64);
    assert(!sophia_sf_shortcut_entry_at(&out.value.shortcuts, 0, &got));
    assert(got.chord.data == shortcuts_wire + 76 && got.label_present == 1 &&
           got.group_present == 0 && !got.group.size);
    before = got;
    assert(sophia_sf_shortcut_entry_at(&out.value.shortcuts, SIZE_MAX, &got) == -4 &&
           !memcmp(&got, &before, sizeof(got)));
    b[6] = 1;
    assert(sophia_sf_shortcut_entry_decode(b, &got) == -1 && !memcmp(&got, &before, sizeof(got)));
    e.group_present = 1;
    assert(sophia_sf_shortcut_entry_encode(b, &e) == -1);
    e.group = (struct sophia_sf_text){(const uint8_t *)"g", 1};
    assert(!sophia_sf_shortcut_entry_encode(b, &e));
    e.label_present = 0;
    assert(sophia_sf_shortcut_entry_encode(b, &e) == -1);
    e.label = (struct sophia_sf_text){NULL, 0};
    assert(!sophia_sf_shortcut_entry_encode(b, &e));
    e = shortcut(1);
    memset(text, 'x', sizeof(text));
    e.chord = (struct sophia_sf_text){text, 64};
    e.action = (struct sophia_sf_text){text, 128};
    e.label = (struct sophia_sf_text){text, 128};
    e.group_present = 1;
    e.group = (struct sophia_sf_text){text, 64};
    assert(!sophia_sf_shortcut_entry_encode(b, &e));
    for (unsigned i = 0; i < 257; i++) {
        e.slot = (uint16_t)(65535 - i);
        assert(!sophia_sf_shortcut_entry_encode(rows + i * 408, &e));
    }
    r.value.shortcuts.rows = rows;
    r.value.shortcuts.rows_bytes = 256 * 408;
    r.value.shortcuts.entry_count = 256;
    assert(roundtrip(&r) == 104512);
    r.value.shortcuts.entry_count = 257;
    r.value.shortcuts.rows_bytes = sizeof(rows);
    bad_value(&r);
    r.value.shortcuts.entry_count = 256;
    r.value.shortcuts.rows_bytes = 256 * 408;
    memcpy(rows + 255 * 408, rows, 408);
    bad_value(&r);
    r = shortcuts();
    r.value.shortcuts.rows = NULL;
    bad_value(&r);
    r.value.shortcuts.rows_bytes = 0;
    r.value.shortcuts.entry_count = 0;
    assert(roundtrip(&r) == 64);
    const unsigned pads[] = {58,  59,  60,  61,  62,  63,  70,  71,  74,  75,
                             139, 142, 143, 271, 274, 275, 403, 406, 407, 471};
    for (unsigned i = 0; i < sizeof(pads) / sizeof(pads[0]); i++)
        mutate_byte(shortcuts_wire, 472, pads[i], 1);
    mutate_byte(shortcuts_wire, 472, 66, 2);
    mutate_byte(shortcuts_wire, 472, 68, 1);
    mutate_byte(shortcuts_wire, 472, 72, 0);
    mutate_byte(shortcuts_wire, 472, 140, 0);
    mutate_byte(shortcuts_wire, 472, 272, 0);
    mutate_byte(shortcuts_wire, 472, 76, '\n');
    mutate_byte(shortcuts_wire, 472, 144, 0x80);
    mutate_byte(shortcuts_wire, 472, 276, 0);
    const struct sophia_sf_text invalid[] = {{NULL, 1},
                                             {(const uint8_t *)"\xc2\x85", 2},
                                             {(const uint8_t *)"\xe2\x80\xae", 3},
                                             {(const uint8_t *)"\xe2\x81\xa6", 3},
                                             {(const uint8_t *)"\xc0\xaf", 2}};
    for (unsigned field = 0; field < 4; field++) {
        for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
            e = shortcut(1);
            e.group_present = 1;
            e.group = (struct sophia_sf_text){text, 1};
            switch (field) {
            case 0:
                e.chord = invalid[i];
                break;
            case 1:
                e.action = invalid[i];
                break;
            case 2:
                e.label = invalid[i];
                break;
            default:
                e.group = invalid[i];
                break;
            }
            memset(b, 0xa5, sizeof(b));
            assert(sophia_sf_shortcut_entry_encode(b, &e) == -1);
            for (unsigned j = 0; j < sizeof(b); j++)
                assert(b[j] == 0xa5);
        }
        e = shortcut(1);
        e.group_present = 1;
        e.group = (struct sophia_sf_text){text, 1};
        switch (field) {
        case 0:
            e.chord = (struct sophia_sf_text){text, 65};
            break;
        case 1:
            e.action = (struct sophia_sf_text){text, 129};
            break;
        case 2:
            e.label = (struct sophia_sf_text){text, 129};
            break;
        default:
            e.group = (struct sophia_sf_text){text, 65};
            break;
        }
        assert(sophia_sf_shortcut_entry_encode(b, &e) == -1);
    }
}
static void publications(void)
{
    struct sophia_sf_record r = {{SOPHIA_SF_OBJECT_PUBLISHED, 9, 0, 1},
                                 .value.object_published = {5, 1, 1234}};
    for (uint16_t k = 5; k <= 7; k++) {
        r.value.object_published.object_kind = k;
        assert(roundtrip(&r) == 56);
    }
    r.value.object_published.object_kind = 8;
    bad_value(&r);
}
int main(void)
{
    descriptor_row_tests();
    descriptor_snapshots();
    tab_snapshots();
    shortcut_rows_and_snapshots();
    publications();
    printf("descriptor objects: 3 literal vectors, row/context validation, maxima and %u "
           "malformed-wire refusals passed\n",
           wire_refusals);
    return 0;
}
