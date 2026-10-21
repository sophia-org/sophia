#include "internal.h"

static enum sophia_ic_refusal take_limits(const uint8_t *p, struct sophia_ic_limits *l)
{
    l->channel_generation = ic_get(p, 8);
    l->limits_generation = ic_get(p + 8, 8);
    l->max_resource_bytes = ic_get(p + 16, 8);
    l->max_sealed_bytes = ic_get(p + 24, 8);
    l->max_dmabuf_source_bytes = ic_get(p + 32, 8);
    l->max_renderer_bytes = ic_get(p + 40, 8);
    l->max_renderer_entries = (uint32_t)ic_get(p + 48, 4);
    l->max_offers = (uint32_t)ic_get(p + 52, 4);
    l->max_validation_jobs = (uint32_t)ic_get(p + 56, 4);
    l->max_capture_jobs = (uint32_t)ic_get(p + 60, 4);
    l->max_outputs = (uint32_t)ic_get(p + 64, 4);
    l->max_rows_per_output = (uint32_t)ic_get(p + 68, 4);
    l->max_width = (uint32_t)ic_get(p + 72, 4);
    l->max_height = (uint32_t)ic_get(p + 76, 4);
    l->validation_timeout_ms = (uint32_t)ic_get(p + 80, 4);
    l->capture_interest_timeout_ms = (uint32_t)ic_get(p + 84, 4);
    l->validation_chunk_bytes = (uint32_t)ic_get(p + 88, 4);
    l->max_packets_per_turn = (uint32_t)ic_get(p + 92, 4);
    if (!l->channel_generation || !l->limits_generation)
        return SOPHIA_IC_GENERATION;
    if (!ic_zero(p + 96, 8))
        return SOPHIA_IC_RESERVED;
    if (!l->max_resource_bytes || !l->max_width || !l->max_height || !l->validation_timeout_ms ||
        !l->capture_interest_timeout_ms || !l->validation_chunk_bytes || !l->max_packets_per_turn)
        return SOPHIA_IC_VALUE;
    if (!l->max_offers || l->max_offers > 4 || !l->max_outputs || l->max_outputs > 16 ||
        !l->max_rows_per_output || l->max_rows_per_output > 256)
        return SOPHIA_IC_CAPACITY;
    /* Sealed: validation jobs within offers and room for one maximum
     * resource, or both zero. */
    if (l->max_validation_jobs || l->max_sealed_bytes) {
        if (!l->max_validation_jobs || l->max_validation_jobs > l->max_offers ||
            l->max_sealed_bytes < l->max_resource_bytes)
            return SOPHIA_IC_CAPACITY;
    }
    /* DMA-BUF: all four reservations positive, or all four zero. */
    if (l->max_dmabuf_source_bytes || l->max_renderer_bytes || l->max_renderer_entries ||
        l->max_capture_jobs) {
        if (!l->max_dmabuf_source_bytes || !l->max_renderer_bytes || !l->max_renderer_entries ||
            !l->max_capture_jobs || l->max_capture_jobs > l->max_offers)
            return SOPHIA_IC_CAPACITY;
    }
    return SOPHIA_IC_OK;
}

static void put_limits(uint8_t *p, const struct sophia_ic_limits *l)
{
    ic_put(p, l->channel_generation, 8);
    ic_put(p + 8, l->limits_generation, 8);
    ic_put(p + 16, l->max_resource_bytes, 8);
    ic_put(p + 24, l->max_sealed_bytes, 8);
    ic_put(p + 32, l->max_dmabuf_source_bytes, 8);
    ic_put(p + 40, l->max_renderer_bytes, 8);
    ic_put(p + 48, l->max_renderer_entries, 4);
    ic_put(p + 52, l->max_offers, 4);
    ic_put(p + 56, l->max_validation_jobs, 4);
    ic_put(p + 60, l->max_capture_jobs, 4);
    ic_put(p + 64, l->max_outputs, 4);
    ic_put(p + 68, l->max_rows_per_output, 4);
    ic_put(p + 72, l->max_width, 4);
    ic_put(p + 76, l->max_height, 4);
    ic_put(p + 80, l->validation_timeout_ms, 4);
    ic_put(p + 84, l->capture_interest_timeout_ms, 4);
    ic_put(p + 88, l->validation_chunk_bytes, 4);
    ic_put(p + 92, l->max_packets_per_turn, 4);
    ic_put(p + 96, 0, 8);
}

static void take_row(const uint8_t *p, struct sophia_ic_format_row *r)
{
    r->output = ic_get(p, 8);
    r->device_major = (uint32_t)ic_get(p + 8, 4);
    r->device_minor = (uint32_t)ic_get(p + 12, 4);
    r->fourcc = (uint32_t)ic_get(p + 16, 4);
    r->flags = (uint32_t)ic_get(p + 20, 4);
    r->modifier = ic_get(p + 24, 8);
    r->max_width = (uint32_t)ic_get(p + 32, 4);
    r->max_height = (uint32_t)ic_get(p + 36, 4);
}

static void put_row(uint8_t *p, const struct sophia_ic_format_row *r)
{
    ic_put(p, r->output, 8);
    ic_put(p + 8, r->device_major, 4);
    ic_put(p + 12, r->device_minor, 4);
    ic_put(p + 16, r->fourcc, 4);
    ic_put(p + 20, r->flags, 4);
    ic_put(p + 24, r->modifier, 8);
    ic_put(p + 32, r->max_width, 4);
    ic_put(p + 36, r->max_height, 4);
}

/* Unsigned (output, device_major, device_minor, fourcc, modifier) order. */
static int row_before(const struct sophia_ic_format_row *a, const struct sophia_ic_format_row *b)
{
    if (a->output != b->output)
        return a->output < b->output;
    if (a->device_major != b->device_major)
        return a->device_major < b->device_major;
    if (a->device_minor != b->device_minor)
        return a->device_minor < b->device_minor;
    if (a->fourcc != b->fourcc)
        return a->fourcc < b->fourcc;
    return a->modifier < b->modifier;
}

static enum sophia_ic_refusal take_formats(const uint8_t *p, size_t n, struct sophia_ic_formats *f)
{
    struct sophia_ic_format_row previous, row;
    uint32_t i;
    f->channel_generation = ic_get(p + 32, 8);
    f->formats_generation = ic_get(p + 40, 8);
    f->limits_generation = ic_get(p + 48, 8);
    f->row_count = (uint32_t)ic_get(p + 56, 4);
    f->rows = p + SOPHIA_IC_FORMATS_HEADER_BYTES;
    if ((uint64_t)SOPHIA_IC_FORMATS_HEADER_BYTES + (uint64_t)SOPHIA_IC_FORMAT_ROW_BYTES * f->row_count !=
        n)
        return SOPHIA_IC_LENGTH;
    if (ic_get(p + 60, 4))
        return SOPHIA_IC_RESERVED;
    if (f->row_count > SOPHIA_IC_MAX_FORMAT_ROWS)
        return SOPHIA_IC_CAPACITY;
    if (!f->channel_generation || !f->formats_generation || !f->limits_generation)
        return SOPHIA_IC_GENERATION;
    for (i = 0; i < f->row_count; i++) {
        take_row(f->rows + SOPHIA_IC_FORMAT_ROW_BYTES * i, &row);
        if (!row.output)
            return SOPHIA_IC_IDENTITY;
        if (!row.fourcc)
            return SOPHIA_IC_FORMAT;
        if (!row.max_width || !row.max_height)
            return SOPHIA_IC_DIMENSIONS;
        if (row.flags != SOPHIA_IC_FORMAT_OPAQUE && row.flags != SOPHIA_IC_FORMAT_VALIDATED)
            return SOPHIA_IC_VALUE;
        if (i && !row_before(&previous, &row))
            return SOPHIA_IC_ORDER;
        previous = row;
    }
    return SOPHIA_IC_OK;
}

enum sophia_ic_refusal sophia_ic_format_row_at(const struct sophia_ic_formats *f, uint32_t i,
                                               struct sophia_ic_format_row *row)
{
    if (!f || !row || !f->rows || i >= f->row_count)
        return SOPHIA_IC_ARGUMENT;
    take_row(f->rows + SOPHIA_IC_FORMAT_ROW_BYTES * i, row);
    return SOPHIA_IC_OK;
}

static enum sophia_ic_refusal direction(const struct sophia_ic_header *h)
{
    switch (h->kind) {
    case SOPHIA_IC_IMAGE_BEGIN:
        return h->submission && !h->sequence ? SOPHIA_IC_OK : SOPHIA_IC_DIRECTION;
    case SOPHIA_IC_IMAGE_LIMITS:
    case SOPHIA_IC_IMAGE_FORMATS:
        return !h->submission && !h->sequence ? SOPHIA_IC_OK : SOPHIA_IC_DIRECTION;
    default:
        return !h->submission && h->sequence ? SOPHIA_IC_OK : SOPHIA_IC_DIRECTION;
    }
}

static size_t body_bytes(uint16_t kind)
{
    switch (kind) {
    case SOPHIA_IC_IMAGE_BEGIN:
    case SOPHIA_IC_IMAGE_ADMITTED:
        return SOPHIA_IC_METADATA_BYTES;
    case SOPHIA_IC_SOURCE_RELEASED:
        return 32;
    case SOPHIA_IC_CHANNEL_READY:
    case SOPHIA_IC_CHANNEL_CLOSED:
        return 16;
    case SOPHIA_IC_IMAGE_LIMITS:
        return SOPHIA_IC_LIMITS_BYTES - SOPHIA_IC_RECORD_HEADER_BYTES;
    default:
        return 0;
    }
}

enum sophia_ic_refusal sophia_ic_record_decode(const void *src, size_t n, struct sophia_ic_record *out)
{
    const uint8_t *p = src, *b;
    struct sophia_ic_record v;
    enum sophia_ic_refusal r;
    if (!src || !out)
        return SOPHIA_IC_ARGUMENT;
    memset(&v, 0, sizeof v);
    if (n < SOPHIA_IC_RECORD_HEADER_BYTES || ic_get(p, 4) != n)
        return SOPHIA_IC_LENGTH;
    if (ic_get(p + 4, 2) != SOPHIA_IC_WIRE_VERSION)
        return SOPHIA_IC_VERSION;
    v.header.kind = (uint16_t)ic_get(p + 6, 2);
    if (v.header.kind < SOPHIA_IC_IMAGE_BEGIN || v.header.kind > SOPHIA_IC_IMAGE_FORMATS)
        return SOPHIA_IC_KIND;
    v.header.epoch = ic_get(p + 8, 8);
    v.header.submission = ic_get(p + 16, 8);
    v.header.sequence = ic_get(p + 24, 8);
    if (!v.header.epoch)
        return SOPHIA_IC_EPOCH;
    if ((r = direction(&v.header)) != SOPHIA_IC_OK)
        return r;
    b = p + SOPHIA_IC_RECORD_HEADER_BYTES;
    if (v.header.kind == SOPHIA_IC_IMAGE_FORMATS) {
        if (n < SOPHIA_IC_FORMATS_HEADER_BYTES)
            return SOPHIA_IC_LENGTH;
        if ((r = take_formats(p, n, &v.value.formats)) != SOPHIA_IC_OK)
            return r;
        *out = v;
        return SOPHIA_IC_OK;
    }
    if (n != SOPHIA_IC_RECORD_HEADER_BYTES + body_bytes(v.header.kind))
        return SOPHIA_IC_LENGTH;
    switch (v.header.kind) {
    case SOPHIA_IC_IMAGE_BEGIN:
    case SOPHIA_IC_IMAGE_ADMITTED:
        ic_take_metadata(b, &v.value.image);
        r = sophia_ic_metadata_check(&v.value.image);
        break;
    case SOPHIA_IC_SOURCE_RELEASED: {
        struct sophia_ic_source_released *s = &v.value.released;
        s->offer = ic_get(b, 8);
        s->resource_id = ic_get(b + 8, 8);
        s->resource_generation = ic_get(b + 16, 8);
        s->disposition = (uint32_t)ic_get(b + 24, 4);
        r = !s->offer                                   ? SOPHIA_IC_OFFER
            : !s->resource_id || !s->resource_generation ? SOPHIA_IC_IDENTITY
            : s->disposition < SOPHIA_IC_RETAINED || s->disposition > SOPHIA_IC_DRAINED
                ? SOPHIA_IC_VALUE
            : ic_get(b + 28, 4) ? SOPHIA_IC_RESERVED
                                : SOPHIA_IC_OK;
        break;
    }
    case SOPHIA_IC_CHANNEL_READY:
    case SOPHIA_IC_CHANNEL_CLOSED: {
        struct sophia_ic_channel_state *c = &v.value.channel;
        uint32_t top = v.header.kind == SOPHIA_IC_CHANNEL_READY ? 4 : SOPHIA_IC_CLOSED_REVOKED;
        c->channel_generation = ic_get(b, 8);
        c->value = (uint32_t)ic_get(b + 8, 4);
        r = !c->channel_generation              ? SOPHIA_IC_GENERATION
            : !c->value || c->value > top       ? SOPHIA_IC_VALUE
            : ic_get(b + 12, 4) ? SOPHIA_IC_RESERVED
                                : SOPHIA_IC_OK;
        break;
    }
    default:
        r = take_limits(b, &v.value.limits);
        break;
    }
    if (r)
        return r;
    *out = v;
    return SOPHIA_IC_OK;
}

static void put_header(uint8_t *b, uint32_t total, const struct sophia_ic_header *h)
{
    ic_put(b, total, 4);
    ic_put(b + 4, SOPHIA_IC_WIRE_VERSION, 2);
    ic_put(b + 6, h->kind, 2);
    ic_put(b + 8, h->epoch, 8);
    ic_put(b + 16, h->submission, 8);
    ic_put(b + 24, h->sequence, 8);
}

enum sophia_ic_refusal sophia_ic_record_encode(void *dst, size_t cap, const struct sophia_ic_record *v,
                                               size_t *written)
{
    uint8_t b[SOPHIA_IC_LIMITS_BYTES], *body = b + SOPHIA_IC_RECORD_HEADER_BYTES;
    struct sophia_ic_record check;
    size_t n;
    enum sophia_ic_refusal r;
    if (!dst || !v || !written || v->header.kind < SOPHIA_IC_IMAGE_BEGIN ||
        v->header.kind > SOPHIA_IC_IMAGE_LIMITS)
        return SOPHIA_IC_ARGUMENT;
    n = SOPHIA_IC_RECORD_HEADER_BYTES + body_bytes(v->header.kind);
    if (cap < n)
        return SOPHIA_IC_ARGUMENT;
    memset(b, 0, sizeof b);
    put_header(b, (uint32_t)n, &v->header);
    switch (v->header.kind) {
    case SOPHIA_IC_IMAGE_BEGIN:
    case SOPHIA_IC_IMAGE_ADMITTED:
        ic_put_metadata(body, &v->value.image);
        break;
    case SOPHIA_IC_SOURCE_RELEASED:
        ic_put(body, v->value.released.offer, 8);
        ic_put(body + 8, v->value.released.resource_id, 8);
        ic_put(body + 16, v->value.released.resource_generation, 8);
        ic_put(body + 24, v->value.released.disposition, 4);
        break;
    case SOPHIA_IC_CHANNEL_READY:
    case SOPHIA_IC_CHANNEL_CLOSED:
        ic_put(body, v->value.channel.channel_generation, 8);
        ic_put(body + 8, v->value.channel.value, 4);
        break;
    default:
        put_limits(body, &v->value.limits);
        break;
    }
    if ((r = sophia_ic_record_decode(b, n, &check)) != SOPHIA_IC_OK)
        return r;
    memcpy(dst, b, n);
    *written = n;
    return SOPHIA_IC_OK;
}

enum sophia_ic_refusal sophia_ic_formats_encode(void *dst, size_t cap, const struct sophia_ic_header *h,
                                                const struct sophia_ic_formats *f,
                                                const struct sophia_ic_format_row *rows,
                                                size_t *written)
{
    struct sophia_ic_header header;
    struct sophia_ic_record check;
    uint8_t *b = dst;
    size_t n;
    uint32_t i;
    enum sophia_ic_refusal r;
    if (!dst || !h || !f || !written || (f->row_count && !rows) ||
        f->row_count > SOPHIA_IC_MAX_FORMAT_ROWS)
        return SOPHIA_IC_ARGUMENT;
    n = SOPHIA_IC_FORMATS_HEADER_BYTES + (size_t)SOPHIA_IC_FORMAT_ROW_BYTES * f->row_count;
    if (cap < n)
        return SOPHIA_IC_ARGUMENT;
    header = *h;
    header.kind = SOPHIA_IC_IMAGE_FORMATS;
    memset(b, 0, n);
    put_header(b, (uint32_t)n, &header);
    ic_put(b + 32, f->channel_generation, 8);
    ic_put(b + 40, f->formats_generation, 8);
    ic_put(b + 48, f->limits_generation, 8);
    ic_put(b + 56, f->row_count, 4);
    for (i = 0; i < f->row_count; i++)
        put_row(b + SOPHIA_IC_FORMATS_HEADER_BYTES + SOPHIA_IC_FORMAT_ROW_BYTES * i, &rows[i]);
    if ((r = sophia_ic_record_decode(b, n, &check)) != SOPHIA_IC_OK) {
        memset(b, 0, n);
        return r;
    }
    *written = n;
    return SOPHIA_IC_OK;
}
