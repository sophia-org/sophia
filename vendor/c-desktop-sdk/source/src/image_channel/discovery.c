#include "internal.h"

static const char *const names[] = {
    "ok", "length", "version", "kind", "role", "reserved", "epoch", "generation",
    "fd_count", "offer", "identity", "source_kind", "format", "plane_count", "plane_layout",
    "dimensions", "stride", "byte_length", "modifier", "direction", "value", "order",
    "discovery_fields", "decimal", "endpoint", "constants", "coherence", "capacity",
    "formats_unsupported", "argument"};

const char *sophia_ic_refusal_name(enum sophia_ic_refusal r)
{
    return (unsigned)r < sizeof names / sizeof names[0] ? names[r] : "unknown";
}

#define HEADER_LINE "sophia-image-channel version=1\n"
enum { FIELD_U16, FIELD_U32, FIELD_U64, FIELD_ENDPOINT };
static const struct field {
    const char *key;
    int type;
} fields[] = {
    {"role", FIELD_U16},
    {"connection_epoch", FIELD_U64},
    {"channel_generation", FIELD_U64},
    {"endpoint", FIELD_ENDPOINT},
    {"source_kinds", FIELD_U32},
    {"max_packet_bytes", FIELD_U32},
    {"max_fds", FIELD_U32},
    {"max_offers", FIELD_U32},
    {"max_packets_per_turn", FIELD_U32},
    {"hello_timeout_ms", FIELD_U32},
    {"validation_timeout_ms", FIELD_U32},
    {"validation_chunk_bytes", FIELD_U32},
    {"capture_interest_timeout_ms", FIELD_U32},
    {"formats_generation", FIELD_U64},
    {"limits_generation", FIELD_U64},
};
#define FIELD_COUNT (sizeof fields / sizeof fields[0])

/* Canonical unsigned decimal, checked against the destination width. */
static int decimal(const uint8_t *s, size_t n, uint64_t max, uint64_t *out)
{
    uint64_t v = 0;
    size_t i;
    if (!n || (n > 1 && s[0] == '0'))
        return 0;
    for (i = 0; i < n; i++) {
        unsigned d;
        if (s[i] < '0' || s[i] > '9')
            return 0;
        d = (unsigned)(s[i] - '0');
        if (v > (max - d) / 10)
            return 0;
        v = v * 10 + d;
    }
    *out = v;
    return 1;
}

static int endpoint(const uint8_t *s, size_t n)
{
    size_t i;
    if (!n || n > SOPHIA_IC_ENDPOINT_MAX || (n == 1 && s[0] == '.') ||
        (n == 2 && s[0] == '.' && s[1] == '.'))
        return 0;
    for (i = 0; i < n; i++) {
        uint8_t c = s[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
              c == '_' || c == '-' || c == '.'))
            return 0;
    }
    return 1;
}

static void store(struct sophia_ic_discovery *d, size_t i, uint64_t v)
{
    uint32_t *u32[] = {NULL, NULL, NULL, NULL, &d->source_kinds, &d->max_packet_bytes,
                       &d->max_fds, &d->max_offers, &d->max_packets_per_turn,
                       &d->hello_timeout_ms, &d->validation_timeout_ms,
                       &d->validation_chunk_bytes, &d->capture_interest_timeout_ms};
    if (i == 0)
        d->role = (uint16_t)v;
    else if (i == 1)
        d->connection_epoch = v;
    else if (i == 2)
        d->channel_generation = v;
    else if (i == 13)
        d->formats_generation = v;
    else if (i == 14)
        d->limits_generation = v;
    else
        *u32[i] = (uint32_t)v;
}

enum sophia_ic_refusal sophia_ic_discovery_decode(const void *src, size_t n,
                                                  struct sophia_ic_discovery *out)
{
    const uint8_t *p = src, *at, *end;
    struct sophia_ic_discovery d;
    size_t i, header = sizeof HEADER_LINE - 1;
    if (!src || !out)
        return SOPHIA_IC_ARGUMENT;
    memset(&d, 0, sizeof d);
    if (n > SOPHIA_IC_DISCOVERY_MAX || n < header || p[n - 1] != '\n')
        return SOPHIA_IC_DISCOVERY_FIELDS;
    if (memcmp(p, HEADER_LINE, header))
        return SOPHIA_IC_VERSION;
    at = p + header;
    end = p + n;
    for (i = 0; i < FIELD_COUNT; i++) {
        const uint8_t *line_end = memchr(at, '\n', (size_t)(end - at)), *value;
        size_t key = strlen(fields[i].key), length;
        uint64_t v, max;
        if (!line_end || (size_t)(line_end - at) < key + 1 || memcmp(at, fields[i].key, key) ||
            at[key] != '=')
            return SOPHIA_IC_DISCOVERY_FIELDS;
        value = at + key + 1;
        length = (size_t)(line_end - value);
        if (fields[i].type == FIELD_ENDPOINT) {
            if (!endpoint(value, length))
                return SOPHIA_IC_ENDPOINT;
            memcpy(d.endpoint, value, length);
            d.endpoint[length] = 0;
        } else {
            max = fields[i].type == FIELD_U16   ? UINT16_MAX
                  : fields[i].type == FIELD_U32 ? UINT32_MAX
                                                : UINT64_MAX;
            if (!decimal(value, length, max, &v))
                return SOPHIA_IC_DECIMAL;
            store(&d, i, v);
        }
        at = line_end + 1;
    }
    if (at != end)
        return SOPHIA_IC_DISCOVERY_FIELDS;
    if (d.role != SOPHIA_IC_ROLE_SHELL && d.role != SOPHIA_IC_ROLE_LOCK)
        return SOPHIA_IC_ROLE;
    if (!d.connection_epoch)
        return SOPHIA_IC_EPOCH;
    if (!d.channel_generation || !d.formats_generation || !d.limits_generation)
        return SOPHIA_IC_GENERATION;
    if (!d.source_kinds || d.source_kinds & ~3u)
        return SOPHIA_IC_SOURCE_KIND;
    if (d.max_packet_bytes != SOPHIA_IC_SOURCE_BYTES || d.max_fds != SOPHIA_IC_MAX_FDS)
        return SOPHIA_IC_CONSTANTS;
    if (!d.max_offers || d.max_offers > 4)
        return SOPHIA_IC_CAPACITY;
    if (!d.max_packets_per_turn || !d.hello_timeout_ms || !d.validation_timeout_ms ||
        !d.validation_chunk_bytes || !d.capture_interest_timeout_ms)
        return SOPHIA_IC_VALUE;
    *out = d;
    return SOPHIA_IC_OK;
}

static enum sophia_ic_refusal format_bounds(const struct sophia_ic_formats *f,
                                            const struct sophia_ic_limits *l)
{
    struct sophia_ic_format_row row;
    uint64_t output = 0;
    uint32_t i, outputs = 0, per_output = 0;
    for (i = 0; i < f->row_count; i++) {
        sophia_ic_format_row_at(f, i, &row);
        if (!i || row.output != output) {
            output = row.output;
            per_output = 0;
            if (++outputs > l->max_outputs)
                return SOPHIA_IC_CAPACITY;
        }
        if (++per_output > l->max_rows_per_output || row.max_width > l->max_width ||
            row.max_height > l->max_height)
            return SOPHIA_IC_CAPACITY;
    }
    return SOPHIA_IC_OK;
}

enum sophia_ic_refusal sophia_ic_discovery_coherent(const struct sophia_ic_discovery *d,
                                                    const struct sophia_ic_record *lr,
                                                    const struct sophia_ic_record *fr,
                                                    const struct sophia_ic_role_bounds *role,
                                                    struct sophia_ic_expect *expect)
{
    const struct sophia_ic_limits *l;
    const struct sophia_ic_formats *f;
    int sealed, dmabuf;
    enum sophia_ic_refusal r;
    if (!d || !lr || !fr || !role || !expect || lr->header.kind != SOPHIA_IC_IMAGE_LIMITS ||
        fr->header.kind != SOPHIA_IC_IMAGE_FORMATS)
        return SOPHIA_IC_ARGUMENT;
    l = &lr->value.limits;
    f = &fr->value.formats;
    if (lr->header.epoch != d->connection_epoch || fr->header.epoch != d->connection_epoch ||
        l->channel_generation != d->channel_generation ||
        f->channel_generation != d->channel_generation ||
        l->limits_generation != d->limits_generation ||
        f->limits_generation != d->limits_generation ||
        f->formats_generation != d->formats_generation || l->max_offers != d->max_offers ||
        l->validation_timeout_ms != d->validation_timeout_ms ||
        l->capture_interest_timeout_ms != d->capture_interest_timeout_ms ||
        l->validation_chunk_bytes != d->validation_chunk_bytes ||
        l->max_packets_per_turn != d->max_packets_per_turn)
        return SOPHIA_IC_COHERENCE;
    sealed = (d->source_kinds & SOPHIA_IC_KIND_BIT_SEALED) != 0;
    dmabuf = (d->source_kinds & SOPHIA_IC_KIND_BIT_DMABUF) != 0;
    if (f->row_count && !dmabuf)
        return SOPHIA_IC_FORMATS_UNSUPPORTED;
    if (sealed != (l->max_validation_jobs != 0) || dmabuf != (l->max_capture_jobs != 0))
        return SOPHIA_IC_CAPACITY;
    if (l->max_resource_bytes > role->max_resource_bytes || l->max_width > role->max_width ||
        l->max_height > role->max_height || l->max_outputs > role->max_outputs ||
        l->max_offers != (role->transfer_slots < 4 ? role->transfer_slots : 4u))
        return SOPHIA_IC_CAPACITY;
    if ((r = format_bounds(f, l)) != SOPHIA_IC_OK)
        return r;
    memset(expect, 0, sizeof *expect);
    expect->role = d->role;
    expect->connection_epoch = d->connection_epoch;
    expect->channel_generation = d->channel_generation;
    expect->source_kinds = d->source_kinds;
    expect->max_width = l->max_width;
    expect->max_height = l->max_height;
    expect->max_resource_bytes = l->max_resource_bytes;
    return SOPHIA_IC_OK;
}
