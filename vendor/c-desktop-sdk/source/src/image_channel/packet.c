#include "internal.h"

void ic_take_metadata(const uint8_t *p, struct sophia_ic_metadata *m)
{
    m->offer = ic_get(p, 8);
    m->resource_id = ic_get(p + 8, 8);
    m->resource_generation = ic_get(p + 16, 8);
    m->admission_epoch = ic_get(p + 24, 8);
    m->output = ic_get(p + 32, 8);
    m->allocation = ic_get(p + 40, 8);
    m->topology_generation = ic_get(p + 48, 8);
    m->source_kind = (uint32_t)ic_get(p + 56, 4);
    m->source_format = (uint32_t)ic_get(p + 60, 4);
    m->width = (uint32_t)ic_get(p + 64, 4);
    m->height = (uint32_t)ic_get(p + 68, 4);
    m->stride = (uint32_t)ic_get(p + 72, 4);
    m->plane_count = (uint16_t)ic_get(p + 76, 2);
    m->descriptor_count = (uint16_t)ic_get(p + 78, 2);
    m->logical_bytes = ic_get(p + 80, 8);
    m->modifier = ic_get(p + 88, 8);
}
void ic_put_metadata(uint8_t *p, const struct sophia_ic_metadata *m)
{
    ic_put(p, m->offer, 8);
    ic_put(p + 8, m->resource_id, 8);
    ic_put(p + 16, m->resource_generation, 8);
    ic_put(p + 24, m->admission_epoch, 8);
    ic_put(p + 32, m->output, 8);
    ic_put(p + 40, m->allocation, 8);
    ic_put(p + 48, m->topology_generation, 8);
    ic_put(p + 56, m->source_kind, 4);
    ic_put(p + 60, m->source_format, 4);
    ic_put(p + 64, m->width, 4);
    ic_put(p + 68, m->height, 4);
    ic_put(p + 72, m->stride, 4);
    ic_put(p + 76, m->plane_count, 2);
    ic_put(p + 78, m->descriptor_count, 2);
    ic_put(p + 80, m->logical_bytes, 8);
    ic_put(p + 88, m->modifier, 8);
}

enum sophia_ic_refusal sophia_ic_metadata_check(const struct sophia_ic_metadata *m)
{
    uint64_t pixels;
    int sealed;
    if (!m)
        return SOPHIA_IC_ARGUMENT;
    if (!m->offer)
        return SOPHIA_IC_OFFER;
    if (!m->resource_id || !m->resource_generation || !m->admission_epoch || !m->output ||
        !m->allocation || !m->topology_generation)
        return SOPHIA_IC_IDENTITY;
    if (m->source_kind != SOPHIA_IC_SOURCE_SEALED && m->source_kind != SOPHIA_IC_SOURCE_DMABUF)
        return SOPHIA_IC_SOURCE_KIND;
    sealed = m->source_kind == SOPHIA_IC_SOURCE_SEALED;
    if (sealed ? m->source_format != SOPHIA_IC_SEALED_BGRA8_PREMULTIPLIED : !m->source_format)
        return SOPHIA_IC_FORMAT;
    if (sealed ? m->plane_count != 1 : !m->plane_count || m->plane_count > SOPHIA_IC_PLANES)
        return SOPHIA_IC_PLANE_COUNT;
    if (m->descriptor_count != m->plane_count)
        return SOPHIA_IC_FD_COUNT;
    if (!m->width || !m->height)
        return SOPHIA_IC_DIMENSIONS;
    if (m->stride != (sealed ? (uint64_t)m->width * 4 : 0))
        return SOPHIA_IC_STRIDE;
    pixels = (uint64_t)m->width * m->height;
    if (pixels > UINT64_MAX / 4 || m->logical_bytes != pixels * 4)
        return SOPHIA_IC_BYTE_LENGTH;
    if (sealed && m->modifier)
        return SOPHIA_IC_MODIFIER;
    return SOPHIA_IC_OK;
}

enum sophia_ic_refusal sophia_ic_metadata_admissible(const struct sophia_ic_metadata *m,
                                                     const struct sophia_ic_expect *e)
{
    enum sophia_ic_refusal r = sophia_ic_metadata_check(m);
    if (r)
        return r;
    if (!e)
        return SOPHIA_IC_ARGUMENT;
    if (!(e->source_kinds & (m->source_kind == SOPHIA_IC_SOURCE_SEALED ? SOPHIA_IC_KIND_BIT_SEALED
                                                                        : SOPHIA_IC_KIND_BIT_DMABUF)))
        return SOPHIA_IC_SOURCE_KIND;
    if (m->width > e->max_width || m->height > e->max_height)
        return SOPHIA_IC_DIMENSIONS;
    if (m->logical_bytes > e->max_resource_bytes)
        return SOPHIA_IC_BYTE_LENGTH;
    return SOPHIA_IC_OK;
}

int sophia_ic_metadata_equal(const struct sophia_ic_metadata *a, const struct sophia_ic_metadata *b)
{
    uint8_t x[SOPHIA_IC_METADATA_BYTES], y[SOPHIA_IC_METADATA_BYTES];
    if (!a || !b)
        return 0;
    ic_put_metadata(x, a);
    ic_put_metadata(y, b);
    return !memcmp(x, y, sizeof x);
}

static int expect_valid(const struct sophia_ic_expect *e)
{
    return e && (e->role == SOPHIA_IC_ROLE_SHELL || e->role == SOPHIA_IC_ROLE_LOCK) &&
           e->connection_epoch && e->channel_generation && e->source_kinds &&
           !(e->source_kinds & ~3u) && e->max_width && e->max_height && e->max_resource_bytes;
}

static enum sophia_ic_refusal planes(const uint8_t *p, struct sophia_ic_packet *out)
{
    const struct sophia_ic_metadata *m = &out->metadata;
    unsigned i;
    for (i = 0; i < SOPHIA_IC_PLANES; i++) {
        const uint8_t *row = p + 16 * i;
        if (i >= m->plane_count) {
            if (!ic_zero(row, 16))
                return SOPHIA_IC_RESERVED;
            continue;
        }
        if (ic_get(row + 14, 2))
            return SOPHIA_IC_RESERVED;
        out->planes[i].offset = ic_get(row, 8);
        out->planes[i].stride = (uint32_t)ic_get(row + 8, 4);
        out->planes[i].fd_index = (uint16_t)ic_get(row + 12, 2);
    }
    for (i = 0; i < m->plane_count; i++) {
        const struct sophia_ic_plane *pl = &out->planes[i];
        /* DMA-BUF offsets and strides belong to the import validator. */
        if (pl->fd_index != i)
            return SOPHIA_IC_PLANE_LAYOUT;
        if (m->source_kind == SOPHIA_IC_SOURCE_SEALED && (pl->offset || pl->stride != m->stride))
            return SOPHIA_IC_PLANE_LAYOUT;
    }
    return SOPHIA_IC_OK;
}

enum sophia_ic_refusal sophia_ic_packet_decode(const void *src, size_t n, unsigned fds,
                                               const struct sophia_ic_expect *e,
                                               struct sophia_ic_packet *out)
{
    const uint8_t *p = src;
    struct sophia_ic_packet v;
    enum sophia_ic_refusal r;
    if (!src || !out || !expect_valid(e))
        return SOPHIA_IC_ARGUMENT;
    memset(&v, 0, sizeof v);
    if (n < SOPHIA_IC_HELLO_BYTES || ic_get(p, 4) != n)
        return SOPHIA_IC_LENGTH;
    if (ic_get(p + 4, 2) != SOPHIA_IC_WIRE_VERSION)
        return SOPHIA_IC_VERSION;
    v.kind = (uint16_t)ic_get(p + 6, 2);
    if (v.kind != SOPHIA_IC_HELLO && v.kind != SOPHIA_IC_SOURCE)
        return SOPHIA_IC_KIND;
    if (n != (v.kind == SOPHIA_IC_HELLO ? SOPHIA_IC_HELLO_BYTES : SOPHIA_IC_SOURCE_BYTES))
        return SOPHIA_IC_LENGTH;
    v.role = (uint16_t)ic_get(p + 8, 2);
    if (v.role != e->role)
        return SOPHIA_IC_ROLE;
    if (ic_get(p + 12, 4))
        return SOPHIA_IC_RESERVED;
    v.connection_epoch = ic_get(p + 16, 8);
    if (v.connection_epoch != e->connection_epoch)
        return SOPHIA_IC_EPOCH;
    v.channel_generation = ic_get(p + 24, 8);
    if (v.channel_generation != e->channel_generation)
        return SOPHIA_IC_GENERATION;
    v.descriptor_count = (uint16_t)ic_get(p + 10, 2);
    if (v.descriptor_count != fds ||
        (v.kind == SOPHIA_IC_HELLO ? fds != 0 : !fds || fds > SOPHIA_IC_MAX_FDS))
        return SOPHIA_IC_FD_COUNT;
    v.offer = ic_get(p + 32, 8);
    if (v.kind == SOPHIA_IC_HELLO) {
        if (v.offer)
            return SOPHIA_IC_OFFER;
        *out = v;
        return SOPHIA_IC_OK;
    }
    ic_take_metadata(p + 40, &v.metadata);
    if (!v.offer || v.offer != v.metadata.offer)
        return SOPHIA_IC_OFFER;
    if ((r = sophia_ic_metadata_check(&v.metadata)) != SOPHIA_IC_OK)
        return r;
    if (v.metadata.descriptor_count != v.descriptor_count)
        return SOPHIA_IC_FD_COUNT;
    if ((r = planes(p + 136, &v)) != SOPHIA_IC_OK)
        return r;
    if ((r = sophia_ic_metadata_admissible(&v.metadata, e)) != SOPHIA_IC_OK)
        return r;
    *out = v;
    return SOPHIA_IC_OK;
}

static void put_header(uint8_t *b, uint32_t total, uint16_t kind, uint16_t count,
                       const struct sophia_ic_expect *e, uint64_t offer)
{
    ic_put(b, total, 4);
    ic_put(b + 4, SOPHIA_IC_WIRE_VERSION, 2);
    ic_put(b + 6, kind, 2);
    ic_put(b + 8, e->role, 2);
    ic_put(b + 10, count, 2);
    ic_put(b + 12, 0, 4);
    ic_put(b + 16, e->connection_epoch, 8);
    ic_put(b + 24, e->channel_generation, 8);
    ic_put(b + 32, offer, 8);
}

enum sophia_ic_refusal sophia_ic_hello_encode(uint8_t dst[SOPHIA_IC_HELLO_BYTES],
                                              const struct sophia_ic_expect *e)
{
    struct sophia_ic_packet check;
    if (!dst || !expect_valid(e))
        return SOPHIA_IC_ARGUMENT;
    put_header(dst, SOPHIA_IC_HELLO_BYTES, SOPHIA_IC_HELLO, 0, e, 0);
    return sophia_ic_packet_decode(dst, SOPHIA_IC_HELLO_BYTES, 0, e, &check);
}

enum sophia_ic_refusal sophia_ic_source_encode(uint8_t dst[SOPHIA_IC_SOURCE_BYTES],
                                               const struct sophia_ic_expect *e,
                                               const struct sophia_ic_metadata *m,
                                               const struct sophia_ic_plane pl[SOPHIA_IC_PLANES])
{
    struct sophia_ic_packet check;
    uint8_t b[SOPHIA_IC_SOURCE_BYTES];
    unsigned i;
    enum sophia_ic_refusal r;
    if (!dst || !m || !pl || !expect_valid(e))
        return SOPHIA_IC_ARGUMENT;
    memset(b, 0, sizeof b);
    put_header(b, SOPHIA_IC_SOURCE_BYTES, SOPHIA_IC_SOURCE, m->descriptor_count, e, m->offer);
    ic_put_metadata(b + 40, m);
    for (i = 0; i < m->plane_count && i < SOPHIA_IC_PLANES; i++) {
        ic_put(b + 136 + 16 * i, pl[i].offset, 8);
        ic_put(b + 144 + 16 * i, pl[i].stride, 4);
        ic_put(b + 148 + 16 * i, pl[i].fd_index, 2);
    }
    r = sophia_ic_packet_decode(b, sizeof b, m->descriptor_count, e, &check);
    if (r == SOPHIA_IC_OK)
        memcpy(dst, b, sizeof b);
    return r;
}
