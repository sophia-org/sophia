#include "internal.h"

static int text_size(const char *s, size_t *n)
{
    size_t i;
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    for (i = 0; i <= UINT16_MAX; i++)
        if (!s[i]) {
            *n = i;
            return 0;
        }
    return SOPHIA_9P_ARGUMENT;
}
static void text(uint8_t **b, const char *s, size_t n)
{
    p9_put(*b, n, 2);
    memcpy(*b + 2, s, n);
    *b += 2 + n;
}
int sophia_9p_version(struct sophia_9p_client *c, struct sophia_9p_handle *h)
{
    uint8_t *b;
    int r;
    if (!c || c->phase)
        return SOPHIA_9P_ARGUMENT;
    r = p9_begin(c, 100, 14, 0, h, &b);
    if (r)
        return r;
    p9_put(b, c->offered, 4);
    p9_put(b + 4, 8, 2);
    memcpy(b + 6, "9P2000.L", 8);
    c->phase = 1;
    return 0;
}
int sophia_9p_attach(struct sophia_9p_client *c, const char *u, const char *a,
                     struct sophia_9p_handle *h, uint32_t *fid)
{
    int slot, r;
    size_t un, an;
    uint32_t f;
    uint8_t *b;
    if (!h || !fid)
        return SOPHIA_9P_ARGUMENT;
    r = p9_available(c, &slot);
    if (r)
        return r;
    if (text_size(u, &un) || text_size(a, &an))
        return SOPHIA_9P_ARGUMENT;
    r = p9_fid(c, &f);
    if (r)
        return r;
    r = p9_begin(c, 104, 16 + un + an, slot, h, &b);
    if (r)
        return r;
    p9_put(b, f, 4);
    p9_put(b + 4, UINT32_MAX, 4);
    b += 8;
    text(&b, u, un);
    text(&b, a, an);
    p9_put(b, UINT32_MAX, 4);
    c->slots[slot].fid = f;
    c->fids[c->fid_count++] = f;
    c->next_fid = f + 1;
    *fid = f;
    return 0;
}
int sophia_9p_walk(struct sophia_9p_client *c, uint32_t from, const char *const *names,
                   size_t count, struct sophia_9p_handle *h, uint32_t *fid)
{
    int slot, r;
    size_t i, n[16], len = 10;
    uint32_t f;
    uint8_t *b;
    if (!h || !fid || count > 16 || (count && !names))
        return SOPHIA_9P_ARGUMENT;
    r = p9_available(c, &slot);
    if (r)
        return r;
    for (i = 0; i < count; i++) {
        if (text_size(names[i], &n[i]))
            return SOPHIA_9P_ARGUMENT;
        len += 2 + n[i];
    }
    r = p9_fid(c, &f);
    if (r)
        return r;
    r = p9_begin(c, 110, len, slot, h, &b);
    if (r)
        return r;
    p9_put(b, from, 4);
    p9_put(b + 4, f, 4);
    p9_put(b + 8, count, 2);
    b += 10;
    for (i = 0; i < count; i++)
        text(&b, names[i], n[i]);
    c->slots[slot].fid = f;
    c->slots[slot].count = (uint32_t)count;
    c->fids[c->fid_count++] = f;
    c->next_fid = f + 1;
    *fid = f;
    return 0;
}
int sophia_9p_lopen(struct sophia_9p_client *c, uint32_t fid, uint32_t flags,
                    struct sophia_9p_handle *h)
{
    int slot, r = p9_available(c, &slot);
    uint8_t *b;
    if (r)
        return r;
    r = p9_begin(c, 12, 8, slot, h, &b);
    if (r)
        return r;
    p9_put(b, fid, 4);
    p9_put(b + 4, flags, 4);
    return 0;
}
static int io_request(struct sophia_9p_client *c, uint8_t type, uint32_t fid, uint64_t offset,
                      const void *data, size_t count, struct sophia_9p_handle *h)
{
    int slot, r = p9_available(c, &slot);
    uint8_t *b;
    if (r)
        return r;
    if (type == 118 && (count > c->msize - 23u || (count && !data)))
        return SOPHIA_9P_ARGUMENT;
    if (type == 116 && count > c->msize - 11u)
        count = c->msize - 11u;
    r = p9_begin(c, type, 16 + (type == 118 ? count : 0), slot, h, &b);
    if (r)
        return r;
    p9_put(b, fid, 4);
    p9_put(b + 4, offset, 8);
    p9_put(b + 12, count, 4);
    if (type == 118 && count)
        memcpy(b + 16, data, count);
    c->slots[slot].count = (uint32_t)count;
    return 0;
}
int sophia_9p_read(struct sophia_9p_client *c, uint32_t f, uint64_t o, uint32_t n,
                   struct sophia_9p_handle *h)
{
    return io_request(c, 116, f, o, NULL, n, h);
}
int sophia_9p_write(struct sophia_9p_client *c, uint32_t f, uint64_t o, const void *b, size_t n,
                    struct sophia_9p_handle *h)
{
    return io_request(c, 118, f, o, b, n, h);
}
int sophia_9p_clunk(struct sophia_9p_client *c, uint32_t f, struct sophia_9p_handle *h)
{
    int slot, r = p9_available(c, &slot);
    uint8_t *b;
    if (r)
        return r;
    r = p9_begin(c, 120, 4, slot, h, &b);
    if (r)
        return r;
    p9_put(b, f, 4);
    p9_free_fid(c, f);
    return 0;
}
int sophia_9p_flush(struct sophia_9p_client *c, struct sophia_9p_handle old,
                    struct sophia_9p_handle *h)
{
    struct sophia_9p_slot *s;
    uint8_t *b;
    int r;
    size_t slot;
    if (!c || !h || old.slot >= c->capacity)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    s = &c->slots[old.slot];
    slot = c->capacity + old.slot;
    if (s->serial != old.serial || (s->state != P9_QUEUED && s->state != P9_SENT) || s->flushing ||
        s->type == 120 || s->type == 100)
        return SOPHIA_9P_ARGUMENT;
    /* The paired slot is reserved even when every ordinary slot is occupied. */
    r = p9_begin(c, 108, 2, (int)slot, h, &b);
    if (r)
        return r;
    p9_put(b, s->tag, 2);
    c->slots[slot].old_slot = old.slot;
    s->flushing = 1;
    return 0;
}
