/* Independent C implementation against the same literal SFT corpus as Rust. */
#include "sophia_shell_frames.h"
#undef NDEBUG
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct vector {
    char name[40];
    uint8_t bytes[8193];
    size_t count;
};
static struct vector vectors[16];
static size_t count;
static void put(uint8_t *b, uint64_t v, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        b[i] = (uint8_t)(v >> (8 * i));
}
static unsigned hex(char c)
{
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return (unsigned)(c <= '9' ? c - '0' : c - 'a' + 10);
}
static enum sophia_sft_class class_of(const uint8_t *b)
{
    return b[7] ? SOPHIA_SFT_REQUEST : SOPHIA_SFT_EVENT;
}
static void load(void)
{
    char line[17000];
    FILE *f = fopen("spec/golden/sophia-frame-transactions-v1.tsv", "r");
    assert(f);
    while (fgets(line, sizeof line, f)) {
        char *name, *kind, *bytes, *end;
        size_t i, len;
        struct vector *v;
        if (line[0] == '#')
            continue;
        name = line;
        kind = strchr(name, '\t');
        assert(kind);
        *kind++ = 0;
        bytes = strchr(kind, '\t');
        assert(bytes);
        *bytes++ = 0;
        end = strchr(bytes, '\n');
        assert(end);
        *end = 0;
        len = strlen(bytes);
        assert(!(len % 2) && len / 2 <= 8192 && count < 16);
        v = &vectors[count++];
        assert(strlen(name) < sizeof v->name);
        strcpy(v->name, name);
        v->count = len / 2;
        for (i = 0; i < v->count; i++)
            v->bytes[i] = (uint8_t)(16 * hex(bytes[2 * i]) + hex(bytes[2 * i + 1]));
        assert((unsigned)atoi(kind) == (unsigned)v->bytes[6] + 256u * v->bytes[7]);
    }
    assert(!ferror(f));
    fclose(f);
    assert(count == 16);
}
static struct vector *named(const char *name)
{
    size_t i;
    for (i = 0; i < count; i++)
        if (!strcmp(vectors[i].name, name))
            return &vectors[i];
    abort();
}
static void refused(const uint8_t *b, size_t n, enum sophia_sft_class class)
{
    struct sophia_sft_record out, before;
    memset(&out, 0xa5, sizeof out);
    memcpy(&before, &out, sizeof out);
    assert(sophia_sft_decode(b, n, class, 13, 8192, &out) != 0);
    assert(!memcmp(&out, &before, sizeof out));
}
static void mutation(const char *name, size_t offset, uint64_t value, size_t n)
{
    struct vector v = *named(name);
    assert(offset + n <= v.count);
    put(v.bytes + offset, value, n);
    refused(v.bytes, v.count, class_of(v.bytes));
}
static void goldens_and_envelopes(void)
{
    size_t i, j, written;
    uint8_t encoded[8193];
    const uint16_t revisions[] = {1, 8, 9, 10, 11, 12, 14, UINT16_MAX};
    for (i = 0; i < count; i++) {
        const struct vector *v = &vectors[i];
        enum sophia_sft_class class = class_of(v->bytes);
        struct sophia_sft_record record;
        struct sophia_sf_record old;
        assert(sophia_sft_decode(v->bytes, v->count, class, 13, 8192, &record) == 0);
        assert(sophia_sft_record_bytes(&record, 13) == v->count);
        written = 0;
        assert(!sophia_sft_encode(encoded, sizeof encoded, 13, &record, &written));
        assert(written == v->count && !memcmp(encoded, v->bytes, written));
        assert(sophia_sf_decode(v->bytes, v->count, &old) != 0);
        assert(!sophia_sf_decode_revision(v->bytes, v->count, 13, &old));
        assert(!sophia_sf_encode_revision(encoded, sizeof encoded, 13, &old, &written));
        assert(written == v->count && !memcmp(encoded, v->bytes, written));
        for (j = 0; j < sizeof revisions / sizeof *revisions; j++) {
            assert(sophia_sft_decode(v->bytes, v->count, class, revisions[j], 8192, &record) != 0);
            assert(sophia_sft_encode(encoded, sizeof encoded, revisions[j], &record, &written) !=
                   0);
            assert(sophia_sf_decode_revision(v->bytes, v->count, revisions[j], &old) != 0);
        }
        assert(sophia_sft_decode(v->bytes, v->count, class, 13, v->count - 1, &record) != 0);
        refused(v->bytes, v->count,
                class == SOPHIA_SFT_REQUEST ? SOPHIA_SFT_EVENT : SOPHIA_SFT_REQUEST);
        for (j = 0; j < v->count; j++) {
            refused(v->bytes, j, class);
            if (j >= 32) {
                memcpy(encoded, v->bytes, j);
                put(encoded, j, 4);
                refused(encoded, j, class);
            }
        }
        memcpy(encoded, v->bytes, v->count);
        encoded[v->count] = 0;
        put(encoded, v->count + 1, 4);
        refused(encoded, v->count + 1, class);
        memset(encoded, 0, sizeof encoded);
        memcpy(encoded, v->bytes, v->count);
        put(encoded, sizeof encoded, 4);
        assert(sophia_sft_decode(encoded, sizeof encoded, class, 13, SIZE_MAX, &record) != 0);
        mutation(v->name, 4, 2, 2);
        mutation(v->name, 8, 2, 8);
        mutation(v->name, 32, 0, 8);
        /* Failure is atomic even with enough backing storage but too small a capacity. */
        memset(encoded, 0x5a, sizeof encoded);
        written = 123;
        assert(sophia_sft_encode(encoded, v->count - 1, 13, &record, &written) != 0);
        assert(written == 123);
        for (j = 0; j < sizeof encoded; j++)
            assert(encoded[j] == 0x5a);
    }
}
static void controls(void)
{
    size_t i, j;
    const char *names[] = {"open", "close", "opened", "closed"};
    for (i = 0; i < 4; i++)
        for (j = 40; j < 104; j += 8)
            mutation(names[i], j, 0, 8);
    mutation("opened", 104, 0, 4);
    mutation("opened", 104, 2, 4);
    mutation("opened", 104, UINT32_MAX, 4);
    mutation("opened", 108, 0, 4);
    mutation("opened", 108, 251, 4);
    mutation("opened", 108, UINT32_MAX, 4);
    mutation("closed", 106, 1, 1);
    for (i = 0; i < 3; i++) {
        uint16_t reason[] = {0, 13, UINT16_MAX};
        mutation("closed", 104, reason[i], 2);
        mutation("refused-commit", 90, reason[i], 2);
    }
    mutation("refused-commit", 92, 1, 1);
    mutation("refused-commit", 88, 262, 2);
    mutation("refused-commit", 80, 0, 8);
    mutation("refused-open", 80, 9, 8);
    mutation("refused-close", 80, 9, 8);
    for (i = 40; i <= 72; i += 8)
        mutation("refused-commit", i, 0, 8);
}
static void commits(void)
{
    const char *names[] = {"commit-retained", "commit-retained-retire", "commit-copy",
                           "commit-copy-retire"};
    const uint32_t sizes[] = {0, 1, 135, 167, 169, UINT32_MAX};
    const size_t mismatch[] = {232, 240, 304, 312, 320, 328, 336, 344};
    const size_t zero[] = {248, 256, 264, 272, 288, 296, 304, 360};
    struct vector v = *named("commit-retained-retire");
    size_t i, j;
    for (i = 0; i < 4; i++) {
        mutation(names[i], 44, 4, 2);
        mutation(names[i], 46, 1, 1);
        mutation(names[i], 112, 0, 2);
        mutation(names[i], 112, 2, 2);
        mutation(names[i], 112, UINT16_MAX, 2);
        mutation(names[i], 114, 33, 2);
        mutation(names[i], 116, 65, 2);
        mutation(names[i], 118, 1, 1);
        for (j = 0; j < sizeof sizes / sizeof *sizes; j++)
            mutation(names[i], 40, sizes[j], 4);
    }
    mutation("commit-copy", 280, 2, 8);
    mutation("commit-copy", 280, 3, 8);
    mutation("commit-copy", 280, UINT64_MAX, 8);
    for (i = 0; i < sizeof mismatch / sizeof *mismatch; i++)
        mutation("commit-copy", mismatch[i], 99, 8);
    for (i = 0; i < sizeof zero / sizeof *zero; i++)
        mutation("commit-copy", zero[i], 0, 8);
    mutation("commit-copy", 288, 25, 8);
    mutation("commit-copy", 216, 151, 4);
    mutation("commit-copy", 222, 'E', 1);
    mutation("commit-retained-retire", 216, 11, 8);
    put(v.bytes + 224, 23, 8);
    put(v.bytes + 232, 24, 8);
    refused(v.bytes, v.count, SOPHIA_SFT_REQUEST);
}
static void outcomes(void)
{
    const char *names[] = {"prepared", "presented-credit", "presented-closed", "rejected-credit",
                           "superseded-credit"};
    size_t i, j;
    mutation("prepared", 140, 1, 8);
    for (i = 0; i < 5; i++) {
        for (j = 108; j <= 132; j += 8)
            mutation(names[i], j, 0, 8);
        mutation(names[i], 80, 0, 2);
        mutation(names[i], 82, 13, 2);
    }
    mutation("presented-credit", 84, 0, 8);
    mutation("prepared", 84, 1, 8);
}
static void receipts(void)
{
    struct sophia_sf_record r, out;
    uint8_t b[48];
    size_t written;
    uint16_t kind, revision;
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_SF_SUBMITTED;
    r.header.epoch = 1;
    r.header.sequence = 13;
    r.value.submitted.submission_id = 12;
    for (kind = SOPHIA_SFT_OPEN; kind <= SOPHIA_SFT_COMMIT; kind++) {
        r.value.submitted.candidate_kind = kind;
        assert(!sophia_sf_encode_revision(b, sizeof b, 13, &r, &written));
        assert(written == 48);
        assert(!sophia_sf_decode_revision(b, written, 13, &out));
        assert(out.value.submitted.candidate_kind == kind);
        assert(sophia_sf_decode(b, written, &out) != 0);
        for (revision = 8; revision <= 14; revision++)
            if (revision != 13) {
                assert(sophia_sf_encode_revision(b, sizeof b, revision, &r, &written) != 0);
                assert(sophia_sf_decode_revision(b, 48, revision, &out) != 0);
            }
    }
}
static void classify(const char *input, const char *output)
{
    FILE *in = fopen(input, "r"), *out = fopen(output, "w");
    char line[17000];
    uint8_t bytes[8193], encoded[8193];
    assert(in && out);
    while (fgets(line, sizeof line, in)) {
        char *hexes = strchr(line, '\t'), *end;
        size_t i, n, written;
        enum sophia_sft_class class;
        struct sophia_sft_record r;
        int accepted;
        assert(hexes);
        *hexes++ = 0;
        end = strchr(hexes, '\n');
        assert(end);
        *end = 0;
        class = !strcmp(line, "request") ? SOPHIA_SFT_REQUEST : SOPHIA_SFT_EVENT;
        assert(!strcmp(line, "request") || !strcmp(line, "event"));
        n = strlen(hexes) / 2;
        assert(strlen(hexes) % 2 == 0 && n <= sizeof bytes);
        for (i = 0; i < n; i++)
            bytes[i] = (uint8_t)(16 * hex(hexes[2 * i]) + hex(hexes[2 * i + 1]));
        accepted = !sophia_sft_decode(bytes, n, class, 13, 8192, &r);
        if (accepted) {
            assert(!sophia_sft_encode(encoded, sizeof encoded, 13, &r, &written));
            assert(written == n && !memcmp(bytes, encoded, n));
        }
        assert(fprintf(out, "%d\n", accepted) > 0);
    }
    assert(!ferror(in));
    fclose(in);
    assert(!fclose(out));
}
static void maximum_rows(void)
{
    struct vector *v = named("commit-retained");
    struct sophia_sft_record r, decoded;
    uint8_t bytes[8192];
    size_t i, written;
    assert(!sophia_sft_decode(v->bytes, v->count, SOPHIA_SFT_REQUEST, 13, 8192, &r));
    r.value.commit.candidate.placement_count = 32;
    for (i = 1; i < 32; i++)
        r.value.commit.candidate.placements[i] = r.value.commit.candidate.placements[0];
    r.value.commit.candidate.target_count = 64;
    for (i = 0; i < 64; i++) {
        struct sophia_sf_content_target *t = &r.value.commit.candidate.targets[i];
        t->action_kind = 1;
        t->target_id = i + 1;
        t->target_generation = 1;
        t->action_id = i + 1;
        t->bounds_width = 2;
        t->bounds_height = 2;
    }
    assert(!sophia_sft_encode(bytes, sizeof bytes, 13, &r, &written));
    assert(written == 32 + 16 + 72 + 64 + 32 * 32 + 64 * 48);
    assert(!sophia_sft_decode(bytes, written, SOPHIA_SFT_REQUEST, 13, 8192, &decoded));
    assert(decoded.value.commit.candidate.target_count == 64);
    assert(decoded.value.commit.candidate.targets[63].target_id == 64);
    r.value.commit.candidate.targets[0].surface_index = 1;
    assert(sophia_sft_encode(bytes, sizeof bytes, 13, &r, &written));
    r.value.commit.candidate.targets[0].surface_index = 0;
    r.value.commit.candidate.target_count = 65;
    assert(sophia_sft_encode(bytes, sizeof bytes, 13, &r, &written));
}
int main(int argc, char **argv)
{
    if (argc == 3) {
        classify(argv[1], argv[2]);
        return 0;
    }
    assert(argc == 1);
    load();
    goldens_and_envelopes();
    controls();
    commits();
    outcomes();
    receipts();
    maximum_rows();
    puts("SFT: 16 goldens, truncation/envelope/bounds/profile/credit refusals OK");
    return 0;
}
