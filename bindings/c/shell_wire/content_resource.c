#include "../sophia_shell_content_resource.h"
#include "fields.h"
#include <string.h>

static int key_valid(const struct sophia_shell_resource_key *v)
{
    return v->grant.connection_epoch && v->grant.content_grant_epoch &&
        v->resource.id && v->resource.generation;
}
static void put_key(uint8_t *p, const struct sophia_shell_resource_key *v)
{
    shell_put64(p, v->grant.connection_epoch);
    shell_put64(p+8, v->grant.content_grant_epoch);
    shell_put64(p+16, v->resource.id);
    shell_put64(p+24, v->resource.generation);
}
static int scale_valid(uint32_t n, uint32_t d)
{
    if (!n || n > 32 || !d || d > 4) return 0;
    while (d) { uint32_t next = n%d; n = d; d = next; }
    return n == 1;
}
int sophia_shell_resource_reply_decode(const struct sophia_shell_frame *f,
                                      struct sophia_shell_resource_reply *out)
{
    if (!f || !f->payload || !out) return SOPHIA_SHELL_ARGUMENT;
    if (!f->transaction || (f->kind != 166 && f->kind != 171) ||
        f->payload_bytes != (f->kind == 166 ? 48u : 34u)) return SOPHIA_SHELL_INVALID;
    const uint8_t *p = f->payload;
    struct sophia_shell_resource_reply v = {0};
    v.kind = f->kind; v.transaction = f->transaction;
    v.key.grant = (struct sophia_shell_content_grant){shell_get64(p), shell_get64(p+8)};
    v.key.resource = (struct sophia_shell_content_id){shell_get64(p+16), shell_get64(p+24)};
    if (!key_valid(&v.key)) return SOPHIA_SHELL_INVALID;
    if (f->kind == 166) {
        v.status = shell_get16(p+32); v.reason = shell_get16(p+34);
        v.next_ordinal = shell_get32(p+36); v.admitted_bytes = shell_get64(p+40);
        if (v.status < 1 || v.status > 4 || v.admitted_bytes > SOPHIA_SHELL_RESOURCE_MAX_BYTES ||
            (v.status <= 2 && v.reason)) return SOPHIA_SHELL_INVALID;
    } else {
        v.reason = shell_get16(p+32);
    }
    if (v.reason > 12) return SOPHIA_SHELL_INVALID;
    *out = v;
    return SOPHIA_SHELL_OK;
}
int sophia_shell_resource_begin_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_resource_begin *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    if (!key_valid(&v->key) || !v->width || v->width > 8192 || !v->height || v->height > 4096 ||
        v->pixel_format != 1 || !scale_valid(v->scale_numerator, v->scale_denominator))
        return SOPHIA_SHELL_INVALID;
    uint32_t row = 4*v->width;
    uint64_t total = (uint64_t)row*v->height;
    uint32_t rows = SOPHIA_SHELL_RESOURCE_MAX_CHUNK_BYTES/row;
    if (total > SOPHIA_SHELL_RESOURCE_MAX_BYTES || total != v->total_bytes || !rows ||
        v->chunk_count != (v->height+rows-1)/rows) return SOPHIA_SHELL_INVALID;
    uint8_t p[64] = {0};
    put_key(p, &v->key);
    shell_put32(p+32, v->width); shell_put32(p+36, v->height);
    shell_put32(p+40, v->scale_numerator); shell_put32(p+44, v->scale_denominator);
    shell_put16(p+48, v->pixel_format); shell_put32(p+52, v->chunk_count);
    shell_put64(p+56, v->total_bytes);
    return sophia_shell_frame_encode(dst, capacity, 165, tx, p, sizeof(p), frame_bytes);
}
int sophia_shell_resource_chunk_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_resource_chunk *v, size_t *frame_bytes)
{
    if (!dst || !frame_bytes || !v || !v->bytes) return SOPHIA_SHELL_ARGUMENT;
    if (!tx || !key_valid(&v->key) || !v->byte_count ||
        v->byte_count > SOPHIA_SHELL_RESOURCE_MAX_CHUNK_BYTES ||
        v->offset > SOPHIA_SHELL_RESOURCE_MAX_BYTES-v->byte_count)
        return SOPHIA_SHELL_INVALID;
    size_t total = SOPHIA_SHELL_HEADER_BYTES+48+v->byte_count;
    if (capacity < total) return SOPHIA_SHELL_INVALID;
    uint8_t p[48] = {0};
    put_key(p, &v->key); shell_put32(p+32, v->ordinal);
    shell_put32(p+36, (uint32_t)v->byte_count); shell_put64(p+40, v->offset);
    /* All arguments and capacity are checked before the first destination write.
     * No payload-sized temporary/allocation or fallible work after the header. */
    size_t prefix;
    int result = sophia_shell_frame_encode(dst, capacity, 167, tx, p, sizeof(p), &prefix);
    if (result != SOPHIA_SHELL_OK) return result;
    memcpy(dst+prefix, v->bytes, v->byte_count);
    shell_put32(dst+16, (uint32_t)(48+v->byte_count));
    *frame_bytes = total;
    return SOPHIA_SHELL_OK;
}
int sophia_shell_resource_end_encode(uint8_t *dst, size_t capacity, uint64_t tx,
    const struct sophia_shell_resource_end *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    if (!key_valid(&v->key) || !v->total_bytes || v->total_bytes > SOPHIA_SHELL_RESOURCE_MAX_BYTES ||
        !v->chunk_count || v->chunk_count > 4096) return SOPHIA_SHELL_INVALID;
    uint8_t p[48] = {0}; put_key(p, &v->key);
    shell_put64(p+32, v->total_bytes); shell_put32(p+40, v->chunk_count);
    return sophia_shell_frame_encode(dst, capacity, 168, tx, p, sizeof(p), frame_bytes);
}
int sophia_shell_resource_control_encode(uint8_t *dst, size_t capacity, uint16_t kind,
    uint64_t tx, const struct sophia_shell_resource_key *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    if ((kind != 169 && kind != 170) || !key_valid(v)) return SOPHIA_SHELL_INVALID;
    uint8_t p[32]; put_key(p, v);
    return sophia_shell_frame_encode(dst, capacity, kind, tx, p, sizeof(p), frame_bytes);
}
