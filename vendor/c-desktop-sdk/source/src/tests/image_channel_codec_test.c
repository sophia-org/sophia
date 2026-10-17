/* Public image-channel codec against Sophia's pinned corpus,
 * spec/golden/sophia-image-channel-v1.tsv, plus the shell r9 reason-13 seam. */
#include "sophia_image_channel.h"
#include "sophia_lock_files.h"
#include "sophia_shell_files.h"
#undef NDEBUG
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CASES 54
struct vector {
    char name[64], family[16], category[32];
    int accepted;
    unsigned fds;
    uint8_t bytes[2048];
    size_t count;
};
static struct vector vectors[CASES];
static size_t vector_count;

static unsigned hex(char c)
{
    return (unsigned)(c <= '9' ? c - '0' : c - 'a' + 10);
}

static void load(void)
{
    FILE *file = fopen("spec/golden/sophia-image-channel-v1.tsv", "r");
    static char line[8192];
    assert(file);
    while (fgets(line, sizeof line, file)) {
        struct vector *v;
        char *field[6], *at = line;
        size_t i, length;
        if (line[0] == '#')
            continue;
        for (i = 0; i < 6; i++) {
            field[i] = at;
            at = strpbrk(at, i == 5 ? "\n" : "\t");
            assert(at);
            *at++ = 0;
        }
        assert(vector_count < CASES);
        v = &vectors[vector_count++];
        assert(strlen(field[0]) < sizeof v->name && strlen(field[1]) < sizeof v->family &&
               strlen(field[5]) < sizeof v->category);
        strcpy(v->name, field[0]);
        strcpy(v->family, field[1]);
        strcpy(v->category, field[5]);
        v->accepted = atoi(field[2]);
        v->fds = (unsigned)atoi(field[3]);
        length = strlen(field[4]);
        assert(length % 2 == 0 && length / 2 <= sizeof v->bytes);
        v->count = length / 2;
        for (i = 0; i < v->count; i++)
            v->bytes[i] = (uint8_t)(hex(field[4][2 * i]) * 16 + hex(field[4][2 * i + 1]));
        assert(v->accepted == !strcmp(v->category, "Accept"));
    }
    fclose(file);
    assert(vector_count == CASES);
}

static const struct vector *named(const char *name)
{
    size_t i;
    for (i = 0; i < vector_count; i++)
        if (!strcmp(vectors[i].name, name))
            return &vectors[i];
    assert(!"missing vector");
    return NULL;
}

static enum sophia_ic_refusal category(const char *c)
{
    static const struct {
        const char *name;
        enum sophia_ic_refusal refusal;
    } map[] = {
        {"Accept", SOPHIA_IC_OK}, {"Length", SOPHIA_IC_LENGTH}, {"Version", SOPHIA_IC_VERSION},
        {"Role", SOPHIA_IC_ROLE}, {"Reserved", SOPHIA_IC_RESERVED}, {"Epoch", SOPHIA_IC_EPOCH},
        {"Generation", SOPHIA_IC_GENERATION}, {"FdCount", SOPHIA_IC_FD_COUNT},
        {"Offer", SOPHIA_IC_OFFER}, {"SourceKind", SOPHIA_IC_SOURCE_KIND},
        {"PlaneCount", SOPHIA_IC_PLANE_COUNT}, {"PlaneLayout", SOPHIA_IC_PLANE_LAYOUT},
        {"Dimensions", SOPHIA_IC_DIMENSIONS}, {"Stride", SOPHIA_IC_STRIDE},
        {"ByteLength", SOPHIA_IC_BYTE_LENGTH}, {"Modifier", SOPHIA_IC_MODIFIER},
        {"DiscoveryFields", SOPHIA_IC_DISCOVERY_FIELDS}, {"Decimal", SOPHIA_IC_DECIMAL},
        {"Endpoint", SOPHIA_IC_ENDPOINT}, {"Constants", SOPHIA_IC_CONSTANTS},
        {"Coherence", SOPHIA_IC_COHERENCE}, {"Capacity", SOPHIA_IC_CAPACITY},
        {"FormatsUnsupported", SOPHIA_IC_FORMATS_UNSUPPORTED},
    };
    size_t i;
    for (i = 0; i < sizeof map / sizeof map[0]; i++)
        if (!strcmp(map[i].name, c))
            return map[i].refusal;
    fprintf(stderr, "unknown category %s\n", c);
    abort();
}

struct set {
    struct sophia_ic_discovery discovery;
    struct sophia_ic_record limits, formats;
    struct sophia_ic_role_bounds role;
    struct sophia_ic_expect expect;
};
static struct set shell, lock;

static void fixture(struct set *s, const char *prefix)
{
    char name[64];
    const struct vector *v;
    snprintf(name, sizeof name, "%s-discovery", prefix);
    v = named(name);
    assert(sophia_ic_discovery_decode(v->bytes, v->count, &s->discovery) == SOPHIA_IC_OK);
    snprintf(name, sizeof name, "%s-limits", prefix);
    v = named(name);
    assert(sophia_ic_record_decode(v->bytes, v->count, &s->limits) == SOPHIA_IC_OK);
    snprintf(name, sizeof name, "%s-sealed-formats", prefix);
    v = named(name);
    assert(sophia_ic_record_decode(v->bytes, v->count, &s->formats) == SOPHIA_IC_OK);
    /* Role ceilings the fixture limits sit within: 4 MiB images, 4096 square,
     * 16 outputs and four transfer slots. */
    s->role.max_resource_bytes = 4194304;
    s->role.max_width = s->role.max_height = 4096;
    s->role.max_outputs = 16;
    s->role.transfer_slots = 4;
    assert(sophia_ic_discovery_coherent(&s->discovery, &s->limits, &s->formats, &s->role,
                                        &s->expect) == SOPHIA_IC_OK);
}

static enum sophia_ic_refusal classify(const struct vector *v, const uint8_t *b, size_t n)
{
    const struct set *s = !strncmp(v->name, "lock-", 5) ? &lock : &shell;
    struct sophia_ic_expect expect;
    struct sophia_ic_packet packet;
    struct sophia_ic_record record;
    struct sophia_ic_discovery discovery;
    enum sophia_ic_refusal r;
    if (!strcmp(v->family, "channel"))
        return sophia_ic_packet_decode(b, n, v->fds, &s->expect, &packet);
    if (!strcmp(v->family, "discovery")) {
        if ((r = sophia_ic_discovery_decode(b, n, &discovery)) != SOPHIA_IC_OK)
            return r;
        return sophia_ic_discovery_coherent(&discovery, &s->limits, &s->formats, &s->role, &expect);
    }
    assert(!strcmp(v->family, "record"));
    if ((r = sophia_ic_record_decode(b, n, &record)) != SOPHIA_IC_OK)
        return r;
    if (record.header.epoch != s->discovery.connection_epoch)
        return SOPHIA_IC_EPOCH;
    if (record.header.kind == SOPHIA_IC_IMAGE_LIMITS)
        return sophia_ic_discovery_coherent(&s->discovery, &record, &s->formats, &s->role, &expect);
    if (record.header.kind == SOPHIA_IC_IMAGE_FORMATS)
        return sophia_ic_discovery_coherent(&s->discovery, &s->limits, &record, &s->role, &expect);
    return SOPHIA_IC_OK;
}

/* Decode then encode must reproduce the exact accepted bytes. */
static void round_trip(const struct vector *v)
{
    const struct set *s = !strncmp(v->name, "lock-", 5) ? &lock : &shell;
    uint8_t out[SOPHIA_IC_MAX_FORMATS_BYTES];
    size_t written = 0;
    if (!strcmp(v->family, "channel")) {
        struct sophia_ic_packet p;
        assert(sophia_ic_packet_decode(v->bytes, v->count, v->fds, &s->expect, &p) == SOPHIA_IC_OK);
        if (p.kind == SOPHIA_IC_HELLO)
            assert(sophia_ic_hello_encode(out, &s->expect) == SOPHIA_IC_OK);
        else
            assert(sophia_ic_source_encode(out, &s->expect, &p.metadata, p.planes) == SOPHIA_IC_OK);
        assert(!memcmp(out, v->bytes, v->count));
    } else if (!strcmp(v->family, "record")) {
        struct sophia_ic_record r;
        struct sophia_ic_format_row rows[1];
        assert(sophia_ic_record_decode(v->bytes, v->count, &r) == SOPHIA_IC_OK);
        if (r.header.kind == SOPHIA_IC_IMAGE_FORMATS) {
            assert(r.value.formats.row_count <= 1);
            if (r.value.formats.row_count)
                assert(sophia_ic_format_row_at(&r.value.formats, 0, rows) == SOPHIA_IC_OK);
            assert(sophia_ic_formats_encode(out, sizeof out, &r.header, &r.value.formats, rows,
                                            &written) == SOPHIA_IC_OK);
        } else {
            assert(sophia_ic_record_encode(out, sizeof out, &r, &written) == SOPHIA_IC_OK);
        }
        assert(written == v->count && !memcmp(out, v->bytes, v->count));
    }
}

static void corpus(void)
{
    size_t i, j;
    for (i = 0; i < vector_count; i++) {
        const struct vector *v = &vectors[i];
        enum sophia_ic_refusal want = category(v->category), got = classify(v, v->bytes, v->count);
        uint8_t longer[2049];
        if (got != want) {
            fprintf(stderr, "%s: %s, expected %s\n", v->name, sophia_ic_refusal_name(got),
                    sophia_ic_refusal_name(want));
            abort();
        }
        if (!v->accepted)
            continue;
        round_trip(v);
        for (j = 0; j < v->count; j++)
            assert(classify(v, v->bytes, j) != SOPHIA_IC_OK);
        memcpy(longer, v->bytes, v->count);
        longer[v->count] = 0;
        assert(classify(v, longer, v->count + 1) != SOPHIA_IC_OK);
    }
}

static void put(uint8_t *p, uint64_t v, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        p[i] = (uint8_t)(v >> (8 * i));
}

/* Sealed sources refuse everything the corpus does not already pin. */
static void sealed_controls(void)
{
    const struct vector *v = named("shell-sealed-source");
    struct sophia_ic_packet p;
    struct sophia_ic_expect narrow = shell.expect;
    uint8_t b[SOPHIA_IC_SOURCE_BYTES];
    memcpy(b, v->bytes, sizeof b);
    put(b + 152, 1, 1); /* unused plane row 1 */
    assert(sophia_ic_packet_decode(b, sizeof b, 1, &shell.expect, &p) == SOPHIA_IC_RESERVED);
    memcpy(b, v->bytes, sizeof b);
    put(b + 144, 4, 4); /* plane stride differs from metadata stride */
    assert(sophia_ic_packet_decode(b, sizeof b, 1, &shell.expect, &p) == SOPHIA_IC_PLANE_LAYOUT);
    memcpy(b, v->bytes, sizeof b);
    put(b + 100, 2, 4); /* sealed format other than premultiplied BGRA8 */
    assert(sophia_ic_packet_decode(b, sizeof b, 1, &shell.expect, &p) == SOPHIA_IC_FORMAT);
    memcpy(b, v->bytes, sizeof b);
    put(b + 6, 3, 2);
    assert(sophia_ic_packet_decode(b, sizeof b, 1, &shell.expect, &p) == SOPHIA_IC_KIND);
    memcpy(b, v->bytes, sizeof b);
    put(b + 48, 0, 8); /* zero resource ID */
    assert(sophia_ic_packet_decode(b, sizeof b, 1, &shell.expect, &p) == SOPHIA_IC_IDENTITY);
    assert(sophia_ic_packet_decode(v->bytes, sizeof b, 0, &shell.expect, &p) == SOPHIA_IC_FD_COUNT);
    assert(sophia_ic_packet_decode(v->bytes, sizeof b, 1, &lock.expect, &p) == SOPHIA_IC_ROLE);
    narrow.max_width = 1;
    assert(sophia_ic_packet_decode(v->bytes, sizeof b, 1, &narrow, &p) == SOPHIA_IC_DIMENSIONS);
    narrow = shell.expect;
    narrow.max_resource_bytes = 7;
    assert(sophia_ic_packet_decode(v->bytes, sizeof b, 1, &narrow, &p) == SOPHIA_IC_BYTE_LENGTH);
    narrow = shell.expect;
    narrow.source_kinds = SOPHIA_IC_KIND_BIT_DMABUF;
    assert(sophia_ic_packet_decode(v->bytes, sizeof b, 1, &narrow, &p) == SOPHIA_IC_SOURCE_KIND);
    /* Overflowing logical size is refused, not wrapped. */
    assert(sophia_ic_packet_decode(v->bytes, sizeof b, 1, &shell.expect, &p) == SOPHIA_IC_OK);
    p.metadata.width = p.metadata.height = UINT32_MAX;
    p.metadata.stride = 0;
    p.metadata.source_kind = SOPHIA_IC_SOURCE_DMABUF;
    p.metadata.source_format = 0x34325258;
    p.metadata.logical_bytes = (uint64_t)UINT32_MAX * UINT32_MAX * 4;
    assert(sophia_ic_metadata_check(&p.metadata) == SOPHIA_IC_BYTE_LENGTH);
}

/* DMA-BUF shape: multi-plane rows, zero metadata stride, any modifier. */
static void dmabuf_shape(void)
{
    struct sophia_ic_expect e = shell.expect;
    struct sophia_ic_metadata m;
    struct sophia_ic_plane planes[SOPHIA_IC_PLANES];
    struct sophia_ic_packet p;
    uint8_t b[SOPHIA_IC_SOURCE_BYTES];
    memset(&m, 0, sizeof m);
    memset(planes, 0, sizeof planes);
    m.offer = 3, m.resource_id = 4, m.resource_generation = 5, m.admission_epoch = 6;
    m.output = 7, m.allocation = 8, m.topology_generation = 9;
    m.source_kind = SOPHIA_IC_SOURCE_DMABUF, m.source_format = 0x34325258;
    m.width = 2, m.height = 2, m.plane_count = m.descriptor_count = 2;
    m.logical_bytes = 16, m.modifier = 0x00ffffffffffffffu;
    planes[0].stride = 8;
    planes[1].offset = 4096, planes[1].stride = 8, planes[1].fd_index = 1;
    assert(sophia_ic_source_encode(b, &e, &m, planes) == SOPHIA_IC_SOURCE_KIND);
    e.source_kinds = SOPHIA_IC_KIND_BIT_SEALED | SOPHIA_IC_KIND_BIT_DMABUF;
    assert(sophia_ic_source_encode(b, &e, &m, planes) == SOPHIA_IC_OK);
    assert(sophia_ic_packet_decode(b, sizeof b, 2, &e, &p) == SOPHIA_IC_OK);
    assert(sophia_ic_metadata_equal(&p.metadata, &m) && p.planes[1].offset == 4096);
    b[10] = 1; /* header count 1 with one SCM_RIGHTS fd, metadata says 2 */
    assert(sophia_ic_packet_decode(b, sizeof b, 1, &e, &p) == SOPHIA_IC_FD_COUNT);
    planes[1].fd_index = 0;
    assert(sophia_ic_source_encode(b, &e, &m, planes) == SOPHIA_IC_PLANE_LAYOUT);
    /* Zero DMA-BUF plane stride is wire-valid. No import acceptance is
     * implied; checked import validation judges it. */
    planes[1].fd_index = 1, planes[1].stride = 0;
    assert(sophia_ic_source_encode(b, &e, &m, planes) == SOPHIA_IC_OK);
    planes[1].stride = 8, m.stride = 8;
    assert(sophia_ic_source_encode(b, &e, &m, planes) == SOPHIA_IC_STRIDE);
    m.stride = 0, m.plane_count = m.descriptor_count = 5;
    assert(sophia_ic_source_encode(b, &e, &m, planes) == SOPHIA_IC_FD_COUNT);
    assert(sophia_ic_metadata_check(&m) == SOPHIA_IC_PLANE_COUNT);
    m.plane_count = 2, m.descriptor_count = 1;
    assert(sophia_ic_metadata_check(&m) == SOPHIA_IC_FD_COUNT);
}

/* Records: direction, dispositions, close reasons, limits pairs, row order. */
static void record_controls(void)
{
    struct sophia_ic_record r, base;
    uint8_t b[SOPHIA_IC_MAX_FORMATS_BYTES];
    struct sophia_ic_format_row rows[3];
    struct sophia_ic_formats f;
    struct sophia_ic_expect expect;
    size_t n;
    unsigned i;
    const struct vector *v = named("source-released");
    assert(sophia_ic_record_decode(v->bytes, v->count, &base) == SOPHIA_IC_OK);
    for (i = 0; i <= 6; i++) {
        r = base;
        r.value.released.disposition = i;
        assert(sophia_ic_record_encode(b, sizeof b, &r, &n) ==
               (i >= 1 && i <= 5 ? SOPHIA_IC_OK : SOPHIA_IC_VALUE));
    }
    r = base;
    r.header.submission = 1;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_DIRECTION);
    v = named("channel-closed");
    assert(sophia_ic_record_decode(v->bytes, v->count, &base) == SOPHIA_IC_OK);
    for (i = 0; i <= 5; i++) {
        r = base;
        r.value.channel.value = i;
        assert(sophia_ic_record_encode(b, sizeof b, &r, &n) ==
               (i >= 1 && i <= 4 ? SOPHIA_IC_OK : SOPHIA_IC_VALUE));
    }
    r.header.kind = SOPHIA_IC_CHANNEL_READY;
    r.value.channel.value = 4;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_VALUE);
    v = named("image-begin");
    memcpy(b, v->bytes, v->count);
    put(b + 32, 0, 8);
    assert(sophia_ic_record_decode(b, v->count, &r) == SOPHIA_IC_OFFER);
    memcpy(b, v->bytes, v->count);
    put(b + 24, 1, 8);
    assert(sophia_ic_record_decode(b, v->count, &r) == SOPHIA_IC_DIRECTION);
    put(b + 6, 0x7007, 2);
    assert(sophia_ic_record_decode(b, v->count, &r) == SOPHIA_IC_KIND);

    /* Limits: a DMA-BUF grant needs all four reservations; sealed needs room
     * for one maximum resource. */
    base = shell.limits;
    r = base;
    r.value.limits.max_capture_jobs = 1;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_CAPACITY);
    r.value.limits.max_dmabuf_source_bytes = r.value.limits.max_renderer_bytes = 1;
    r.value.limits.max_renderer_entries = 1;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_OK);
    r.value.limits.max_capture_jobs = r.value.limits.max_offers + 1;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_CAPACITY);
    r.value.limits.max_capture_jobs = 0;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_CAPACITY);
    r.value.limits.max_capture_jobs = 1;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_OK);
    assert(sophia_ic_record_decode(b, n, &r) == SOPHIA_IC_OK);
    /* Granted DMA capacity that discovery does not advertise is incoherent. */
    assert(sophia_ic_discovery_coherent(&shell.discovery, &r, &shell.formats, &shell.role,
                                        &expect) == SOPHIA_IC_CAPACITY);
    r = base;
    r.value.limits.max_sealed_bytes = r.value.limits.max_resource_bytes - 1;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_CAPACITY);
    r = base;
    r.value.limits.max_offers = 5;
    assert(sophia_ic_record_encode(b, sizeof b, &r, &n) == SOPHIA_IC_CAPACITY);
    {
        struct sophia_ic_role_bounds small = shell.role;
        small.transfer_slots = 3;
        assert(sophia_ic_discovery_coherent(&shell.discovery, &shell.limits, &shell.formats,
                                            &small, &expect) == SOPHIA_IC_CAPACITY);
        small = shell.role;
        small.max_resource_bytes = shell.limits.value.limits.max_resource_bytes - 1;
        assert(sophia_ic_discovery_coherent(&shell.discovery, &shell.limits, &shell.formats,
                                            &small, &expect) == SOPHIA_IC_CAPACITY);
    }

    /* Format rows: strict key order, exactly one flag, nonzero fields. */
    memset(rows, 0, sizeof rows);
    for (i = 0; i < 3; i++) {
        rows[i].output = 1, rows[i].device_major = 226, rows[i].fourcc = 0x34325258;
        rows[i].flags = SOPHIA_IC_FORMAT_OPAQUE, rows[i].max_width = rows[i].max_height = 64;
        rows[i].modifier = i;
    }
    f = shell.formats.value.formats;
    f.row_count = 3;
    assert(sophia_ic_formats_encode(b, sizeof b, &shell.formats.header, &f, rows, &n) ==
           SOPHIA_IC_OK);
    assert(n == 64 + 3 * 40);
    rows[2].modifier = 1;
    assert(sophia_ic_formats_encode(b, sizeof b, &shell.formats.header, &f, rows, &n) ==
           SOPHIA_IC_ORDER);
    rows[2].modifier = 2, rows[1].flags = 3;
    assert(sophia_ic_formats_encode(b, sizeof b, &shell.formats.header, &f, rows, &n) ==
           SOPHIA_IC_VALUE);
    rows[1].flags = SOPHIA_IC_FORMAT_VALIDATED, rows[0].fourcc = 0;
    assert(sophia_ic_formats_encode(b, sizeof b, &shell.formats.header, &f, rows, &n) ==
           SOPHIA_IC_FORMAT);
    rows[0].fourcc = 0x34325258;
    assert(sophia_ic_formats_encode(b, sizeof b, &shell.formats.header, &f, rows, &n) ==
           SOPHIA_IC_OK);
    assert(sophia_ic_record_decode(b, n, &r) == SOPHIA_IC_OK);
    {
        struct sophia_ic_discovery d = shell.discovery;
        struct sophia_ic_record l = shell.limits;
        d.source_kinds = 3;
        l.value.limits.max_capture_jobs = l.value.limits.max_renderer_entries = 1;
        l.value.limits.max_dmabuf_source_bytes = l.value.limits.max_renderer_bytes = 1;
        assert(sophia_ic_discovery_coherent(&d, &l, &r, &shell.role, &expect) == SOPHIA_IC_OK);
        l.value.limits.max_rows_per_output = 2;
        assert(sophia_ic_discovery_coherent(&d, &l, &r, &shell.role, &expect) ==
               SOPHIA_IC_CAPACITY);
        l.value.limits.max_rows_per_output = 3;
        l.value.limits.max_width = 63;
        assert(sophia_ic_discovery_coherent(&d, &l, &r, &shell.role, &expect) ==
               SOPHIA_IC_CAPACITY);
    }
}

static void coherence_controls(void)
{
    struct sophia_ic_expect expect;
    struct sophia_ic_discovery d;
    size_t size = SOPHIA_IC_MAX_FORMATS_BYTES + SOPHIA_IC_FORMAT_ROW_BYTES;
    uint8_t *large = calloc(1, size);
    const struct vector *f = named("shell-sealed-formats");
    struct sophia_ic_record r;
    unsigned k;
    for (k = 0; k < 4; k++) {
        d = shell.discovery;
        if (k == 0)
            d.channel_generation++;
        if (k == 1)
            d.limits_generation++;
        if (k == 2)
            d.formats_generation++;
        if (k == 3)
            d.connection_epoch++;
        assert(sophia_ic_discovery_coherent(&d, &shell.limits, &shell.formats, &shell.role,
                                            &expect) == SOPHIA_IC_COHERENCE);
    }
    for (k = 0; k < 2; k++) {
        struct sophia_ic_record l = shell.limits;
        if (k == 0)
            l.value.limits.channel_generation++;
        else
            l.value.limits.limits_generation++;
        assert(sophia_ic_discovery_coherent(&shell.discovery, &l, &shell.formats, &shell.role,
                                            &expect) == SOPHIA_IC_COHERENCE);
    }
    /* 4097 rows with an exact length is a capacity refusal, not a length one. */
    assert(large);
    memcpy(large, f->bytes, 64);
    put(large, size, 4);
    put(large + 56, SOPHIA_IC_MAX_FORMAT_ROWS + 1, 4);
    assert(sophia_ic_record_decode(large, size, &r) == SOPHIA_IC_CAPACITY);
    free(large);
}

static void discovery_controls(void)
{
    const struct vector *v = named("shell-discovery");
    struct sophia_ic_discovery d;
    uint8_t b[SOPHIA_IC_DISCOVERY_MAX + 1];
    char *at;
    memcpy(b, v->bytes, v->count);
    b[0] = 'S';
    assert(sophia_ic_discovery_decode(b, v->count, &d) == SOPHIA_IC_VERSION);
    memcpy(b, v->bytes, v->count);
    b[v->count] = 0;
    at = strstr((char *)b, "source_kinds=1");
    assert(at);
    at[13] = '4';
    assert(sophia_ic_discovery_decode(b, v->count, &d) == SOPHIA_IC_SOURCE_KIND);
    memcpy(b, v->bytes, v->count);
    at = strstr((char *)b, "role=1");
    assert(at);
    at[5] = '3';
    assert(sophia_ic_discovery_decode(b, v->count, &d) == SOPHIA_IC_ROLE);
    memcpy(b, v->bytes, v->count);
    at = strstr((char *)b, "endpoint=image.sock");
    assert(at);
    memcpy(at + 9, "..", 2);
    assert(sophia_ic_discovery_decode(b, v->count, &d) == SOPHIA_IC_OK);
    memcpy(at + 9, "../image.s", 10);
    assert(sophia_ic_discovery_decode(b, v->count, &d) == SOPHIA_IC_ENDPOINT);
    assert(sophia_ic_discovery_decode(NULL, 0, &d) == SOPHIA_IC_ARGUMENT);
    assert(sophia_ic_record_decode(NULL, 0, &shell.limits) == SOPHIA_IC_ARGUMENT);
    assert(sophia_ic_record_decode(NULL, 136, &shell.limits) == SOPHIA_IC_ARGUMENT);
    assert(sophia_ic_discovery_decode(NULL, v->count, &d) == SOPHIA_IC_ARGUMENT);
    assert(sophia_ic_discovery_decode(v->bytes, v->count, NULL) == SOPHIA_IC_ARGUMENT);
    assert(sophia_ic_discovery_decode(v->bytes, 0, &d) == SOPHIA_IC_DISCOVERY_FIELDS);
    assert(sophia_ic_discovery_decode(v->bytes, v->count, &d) == SOPHIA_IC_OK);
    assert(!strcmp(d.endpoint, "image.sock") && d.max_offers == 4 && d.formats_generation == 9);
}

/* The role codecs still refuse the new kinds; shell reason 13 needs r9. */
static void role_codecs(void)
{
    struct sophia_sf_record r, back;
    struct sophia_lf_record lr;
    uint8_t b[128];
    size_t n;
    const struct vector *v = named("image-begin");
    assert(sophia_sf_decode(v->bytes, v->count, &r) != 0);
    assert(sophia_sf_decode_revision(v->bytes, v->count, 9, &r) != 0);
    assert(sophia_lf_decode(v->bytes, v->count, &lr) != 0);
    assert(SOPHIA_LF_REASON_IMAGE_CHANNEL_UNAVAILABLE == 9);

    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_SF_RESOURCE_STATUS;
    r.header.epoch = 1, r.header.sequence = 1;
    r.value.resource_status.transaction = 2;
    r.value.resource_status.grant_connection_epoch = 1;
    r.value.resource_status.grant_content_epoch = 1;
    r.value.resource_status.resource_id = 3;
    r.value.resource_status.resource_generation = 1;
    r.value.resource_status.status = 3;
    r.value.resource_status.reason = 12;
    assert(sophia_sf_encode(b, sizeof b, &r, &n) == 0 && n == 88);
    r.value.resource_status.reason = 13;
    assert(sophia_sf_encode(b, sizeof b, &r, &n) != 0);
    assert(sophia_sf_encode_revision(b, sizeof b, 8, &r, &n) != 0);
    assert(sophia_sf_encode_revision(b, sizeof b, 9, &r, &n) == 0 && n == 88);
    assert(b[32 + 42] == 13 && b[32 + 43] == 0);
    assert(sophia_sf_decode(b, n, &back) != 0);
    assert(sophia_sf_decode_revision(b, n, 8, &back) != 0);
    assert(sophia_sf_decode_revision(b, n, 9, &back) == 0);
    assert(back.value.resource_status.reason == 13 && back.value.resource_status.status == 3);
    assert(sophia_sf_decode_revision(b, n, 0, &back) == -4);
    assert(sophia_sf_decode_revision(b, n, 10, &back) == -4);
    assert(sophia_sf_encode_revision(b, sizeof b, 10, &r, &n) == -4);
    /* Only Rejected carries reason 13; 14 stays out of range at r9. */
    r.value.resource_status.status = 4;
    assert(sophia_sf_encode_revision(b, sizeof b, 9, &r, &n) != 0);
    r.value.resource_status.status = 3;
    r.value.resource_status.reason = 14;
    assert(sophia_sf_encode_revision(b, sizeof b, 9, &r, &n) != 0);
    /* Released keeps its own <= 12 bound at every revision. */
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_SF_RESOURCE_RELEASED;
    r.header.epoch = 1, r.header.sequence = 1;
    r.value.resource_released.transaction = 2;
    r.value.resource_released.grant_connection_epoch = 1;
    r.value.resource_released.grant_content_epoch = 1;
    r.value.resource_released.resource_id = 3;
    r.value.resource_released.resource_generation = 1;
    r.value.resource_released.reason = 13;
    assert(sophia_sf_encode_revision(b, sizeof b, 9, &r, &n) != 0);
    r.value.resource_released.reason = 12;
    assert(sophia_sf_encode_revision(b, sizeof b, 9, &r, &n) == 0);
    b[32 + 40] = 13;
    assert(sophia_sf_decode_revision(b, n, 9, &back) != 0);
}

int main(void)
{
    load();
    fixture(&shell, "shell");
    fixture(&lock, "lock");
    assert(shell.expect.role == SOPHIA_IC_ROLE_SHELL && lock.expect.role == SOPHIA_IC_ROLE_LOCK);
    corpus();
    sealed_controls();
    dmabuf_shape();
    record_controls();
    discovery_controls();
    coherence_controls();
    role_codecs();
    puts("image channel codec: 54 corpus vectors, controls and r9 reason seam passed");
    return 0;
}
