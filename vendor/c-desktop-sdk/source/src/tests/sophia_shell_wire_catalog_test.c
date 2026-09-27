#include "../sophia_shell_catalog.h"
#include "../shell_wire/fields.h"

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct fixture {
    struct sophia_shell_catalog catalog;
    struct sophia_shell_catalog_entry first[4], second[4];
};

static struct sophia_shell_welcome welcome(void)
{
    return (struct sophia_shell_welcome){
        .revision = 6, .connection_epoch = 5,
        .capabilities = SOPHIA_SHELL_CAP_APPLICATION_CATALOG
    };
}

static void init(struct fixture *f)
{
    struct sophia_shell_welcome w = welcome();
    assert(sophia_shell_catalog_init(&f->catalog, &w, f->first, f->second, 4) == 0);
}

static int control(struct fixture *f, uint16_t kind, uint64_t generation, uint16_t count)
{
    uint8_t payload[20] = {0};
    shell_put64(payload, 5);
    shell_put64(payload + 8, generation);
    shell_put16(payload + 16, count);
    struct sophia_shell_frame frame = {kind, 1, payload, kind == 114 ? 20 : 16};
    return sophia_shell_catalog_accept(&f->catalog, &frame);
}

static int row(struct fixture *f, uint64_t generation, uint16_t slot,
               const uint8_t *label, size_t length, uint16_t available)
{
    uint8_t payload[512] = {0};
    assert(length <= 256);
    shell_put64(payload, 5);
    shell_put64(payload + 8, generation);
    shell_put16(payload + 16, slot);
    shell_put16(payload + 18, available);
    shell_put16(payload + 20, (uint16_t)length);
    memcpy(payload + 22, label, length);
    struct sophia_shell_frame frame = {115, 1, payload, 24 + length};
    return sophia_shell_catalog_accept(&f->catalog, &frame);
}

static void commit_one(struct fixture *f, uint64_t generation)
{
    assert(control(f, 114, generation, 1) == SOPHIA_SHELL_CATALOG_PENDING);
    assert(row(f, generation, 1, (const uint8_t *)"Editor", 6, 1) == SOPHIA_SHELL_CATALOG_PENDING);
    assert(control(f, 116, generation, 0) == SOPHIA_SHELL_CATALOG_COMMITTED);
}

static void atomic_and_interleaved(void)
{
    struct fixture f;
    init(&f);
    size_t count = 999;
    uint64_t generation = 999;
    assert(!sophia_shell_catalog_entries(&f.catalog, &count, &generation));
    assert(count == 999 && generation == 999);
    commit_one(&f, 1);
    const struct sophia_shell_catalog_entry *old = sophia_shell_catalog_entries(&f.catalog, &count, &generation);
    assert(old && count == 1 && generation == 1 && !strcmp(old[0].label, "Editor"));
    assert(control(&f, 114, 2, 2) == SOPHIA_SHELL_CATALOG_PENDING);
    static const uint8_t unicode[] = "\xd0\xa2\xd0\xb5\xd1\x81\xd1\x82 \xf0\x9f\x94\xa7";
    assert(row(&f, 2, 4096, unicode, sizeof(unicode) - 1, 1) == SOPHIA_SHELL_CATALOG_PENDING);
    struct sophia_shell_frame other = {.kind = 171};
    assert(sophia_shell_catalog_accept(&f.catalog, &other) == SOPHIA_SHELL_CATALOG_UNRELATED);
    assert(sophia_shell_catalog_entries(&f.catalog, &count, &generation) == old);
    assert(count == 1 && generation == 1 && !strcmp(old[0].label, "Editor"));
    assert(row(&f, 2, 64, (const uint8_t *)"Off", 3, 0) == SOPHIA_SHELL_CATALOG_PENDING);
    assert(control(&f, 116, 2, 0) == SOPHIA_SHELL_CATALOG_COMMITTED);
    const struct sophia_shell_catalog_entry *next = sophia_shell_catalog_entries(&f.catalog, &count, &generation);
    assert(next != old && count == 2 && generation == 2);
    assert(next[0].slot == 4096 && next[1].slot == 64 && next[1].available == 0);
    assert(next[0].label_bytes == sizeof(unicode) - 1 && !strcmp(next[0].label, (const char *)unicode));
    assert(control(&f, 114, 3, 0) == SOPHIA_SHELL_CATALOG_PENDING);
    assert(control(&f, 116, 3, 0) == SOPHIA_SHELL_CATALOG_COMMITTED);
    assert(sophia_shell_catalog_entries(&f.catalog, &count, &generation));
    assert(count == 0 && generation == 3);
}

static void refused_transfer_preserves_committed(void)
{
    for (unsigned mode = 0; mode < 10; ++mode) {
        struct fixture f;
        init(&f);
        commit_one(&f, 1);
        uint8_t p[20] = {0};
        shell_put64(p, 5); shell_put64(p + 8, 2); shell_put16(p + 16, 1);
        struct sophia_shell_frame frame = {114, 1, p, sizeof(p)};
        int result;
        if (mode < 5) {
            if (mode == 0) shell_put64(p, 6);
            if (mode == 1) shell_put64(p + 8, 1);
            if (mode == 2) frame.transaction = 0;
            if (mode == 3) shell_put16(p + 16, 5);
            if (mode == 4) shell_put16(p + 18, 1);
            result = sophia_shell_catalog_accept(&f.catalog, &frame);
        } else {
            assert(sophia_shell_catalog_accept(&f.catalog, &frame) == 0);
            if (mode == 5) result = control(&f, 116, 2, 0); /* missing row */
            else if (mode == 6) result = control(&f, 114, 3, 0); /* overlapping begin */
            else if (mode == 7) result = row(&f, 3, 1, (const uint8_t *)"A", 1, 1);
            else {
                assert(row(&f, 2, 1, (const uint8_t *)"A", 1, 1) == 0);
                frame.kind = 116; frame.payload_bytes = 16;
                if (mode == 8) frame.transaction = 2;
                else frame.payload_bytes = 17;
                result = sophia_shell_catalog_accept(&f.catalog, &frame);
            }
        }
        assert(result == SOPHIA_SHELL_CATALOG_INVALID);
        assert(control(&f, 114, 3, 0) == SOPHIA_SHELL_CATALOG_INVALID);
        size_t count = 0; uint64_t generation = 0;
        const struct sophia_shell_catalog_entry *entries = sophia_shell_catalog_entries(&f.catalog, &count, &generation);
        assert(entries && count == 1 && generation == 1 && !strcmp(entries[0].label, "Editor"));
    }
}

static void malformed_rows(void)
{
    static const uint8_t *bad[] = {
        (const uint8_t *)"", (const uint8_t *)"\x80", (const uint8_t *)"\xc0\xaf",
        (const uint8_t *)"\xed\xa0\x80", (const uint8_t *)"\xf4\x90\x80\x80",
        (const uint8_t *)"\xe2\x82", (const uint8_t *)"\xc2\x80",
        (const uint8_t *)"\n", (const uint8_t *)"\xe2\x80\xae", (const uint8_t *)"\xe2\x81\xa6"
    };
    for (size_t i = 0; i < sizeof(bad)/sizeof(bad[0]); ++i) {
        struct fixture f; init(&f);
        assert(control(&f, 114, 1, 1) == 0);
        assert(row(&f, 1, 1, bad[i], strlen((const char *)bad[i]), 1) == SOPHIA_SHELL_CATALOG_INVALID);
    }
    for (unsigned mode = 0; mode < 5; ++mode) {
        struct fixture f; init(&f);
        assert(control(&f, 114, 1, 2) == 0);
        const uint8_t embedded_nul[] = {'a', 0, 'b'};
        uint8_t long_label[129]; memset(long_label, 'a', sizeof(long_label));
        int result;
        if (mode == 0) result = row(&f, 1, 0, (const uint8_t *)"A", 1, 1);
        else if (mode == 1) result = row(&f, 1, 1, long_label, sizeof(long_label), 1);
        else if (mode == 2) result = row(&f, 1, 1, embedded_nul, sizeof(embedded_nul), 1);
        else if (mode == 3) result = row(&f, 1, 1, (const uint8_t *)"A", 1, 2);
        else {
            assert(row(&f, 1, 65, (const uint8_t *)"A", 1, 1) == 0);
            result = row(&f, 1, 65, (const uint8_t *)"B", 1, 1);
        }
        assert(result == SOPHIA_SHELL_CATALOG_INVALID);
    }
}

static unsigned nibble(char c)
{
    if (c >= '0' && c <= '9') return (unsigned)(c - '0');
    assert(c >= 'a' && c <= 'f');
    return (unsigned)(c - 'a') + 10;
}

static void entry_size_boundaries(void)
{
    uint8_t p[24 + 128 + 256] = {0};
    shell_put64(p, 5); shell_put64(p + 8, 1);
    shell_put16(p + 16, 1); shell_put16(p + 18, 1);
    shell_put16(p + 20, 128);
    memset(p + 22, 'L', 128);
    shell_put16(p + 150, 256);
    memset(p + 152, 'K', 256);
    struct sophia_shell_frame frame = {115, 1, p, sizeof(p)};
    struct fixture f; init(&f);
    assert(control(&f, 114, 1, 1) == 0);
    assert(sophia_shell_catalog_accept(&f.catalog, &frame) == 0);
    assert(control(&f, 116, 1, 0) == SOPHIA_SHELL_CATALOG_COMMITTED);
    size_t count = 0; uint64_t generation = 0;
    const struct sophia_shell_catalog_entry *entries = sophia_shell_catalog_entries(&f.catalog, &count, &generation);
    assert(count == 1 && generation == 1);
    assert(strlen(entries[0].label) == 128 && strlen(entries[0].keywords) == 256);
    for (size_t bytes = 0; bytes < sizeof(p); ++bytes) {
        init(&f); assert(control(&f, 114, 1, 1) == 0);
        frame.payload_bytes = bytes;
        assert(sophia_shell_catalog_accept(&f.catalog, &frame) == SOPHIA_SHELL_CATALOG_INVALID);
    }
    frame.payload_bytes = sizeof(p);
    init(&f); assert(control(&f, 114, 1, 1) == 0);
    p[sizeof(p) - 1] = 0x80;
    assert(sophia_shell_catalog_accept(&f.catalog, &frame) == SOPHIA_SHELL_CATALOG_INVALID);
}

static void rust_golden_catalog(const char *path)
{
    FILE *file = fopen(path, "r"); assert(file);
    struct fixture f; init(&f);
    char line[2048]; unsigned frames = 0;
    while (fgets(line, sizeof(line), file)) {
        if (strncmp(line, "catalog|", 8)) continue;
        char *hex = line + 8;
        size_t length = strcspn(hex, "\r\n");
        uint8_t bytes[1024];
        assert(length % 2 == 0 && length / 2 <= sizeof(bytes));
        for (size_t i = 0; i < length / 2; ++i)
            bytes[i] = (uint8_t)(16 * nibble(hex[i * 2]) + nibble(hex[i * 2 + 1]));
        struct sophia_shell_frame frame;
        assert(sophia_shell_frame_decode(bytes, length / 2, &frame) == SOPHIA_SHELL_OK);
        assert(sophia_shell_catalog_accept(&f.catalog, &frame) ==
               (frame.kind == 116 ? SOPHIA_SHELL_CATALOG_COMMITTED : SOPHIA_SHELL_CATALOG_PENDING));
        ++frames;
    }
    assert(!ferror(file)); fclose(file);
    size_t count = 0; uint64_t generation = 0;
    const struct sophia_shell_catalog_entry *entries = sophia_shell_catalog_entries(&f.catalog, &count, &generation);
    assert(frames == 5 && entries && count == 3 && generation == 7);
    assert(entries[0].slot == 1 && entries[0].available == 1 && !strcmp(entries[0].label, "Application 1"));
    assert(entries[1].slot == 2 && entries[1].available == 0 && !strcmp(entries[1].keywords, "editor terminal"));
    assert(entries[2].slot == 3 && entries[2].available == 1);
}

int main(int argc, char **argv)
{
    assert(argc == 2);
    struct fixture f; struct sophia_shell_welcome w = welcome();
    assert(sophia_shell_catalog_init(&f.catalog, &w, f.first, f.first, 4) == SOPHIA_SHELL_CATALOG_ARGUMENT);
    assert(sophia_shell_catalog_init(&f.catalog, &w, f.first, f.second, 0) == SOPHIA_SHELL_CATALOG_ARGUMENT);
    w.capabilities = 0;
    assert(sophia_shell_catalog_init(&f.catalog, &w, f.first, f.second, 4) == SOPHIA_SHELL_CATALOG_ARGUMENT);
    atomic_and_interleaved();
    refused_transfer_preserves_committed();
    malformed_rows();
    entry_size_boundaries();
    rust_golden_catalog(argv[1]);
    puts("sophia_shell_catalog groups=6 status=pass native=false");
    return 0;
}
