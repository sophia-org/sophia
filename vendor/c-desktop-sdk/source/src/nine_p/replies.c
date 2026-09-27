#include "internal.h"

static int qid(const uint8_t *b, struct sophia_9p_qid *q)
{
    if (b[0] != 0 && b[0] != 0x80)
        return SOPHIA_9P_INVALID;
    q->type = b[0];
    q->version = p9_u32(b + 1);
    q->path = p9_u64(b + 5);
    return 0;
}
/* Validate before any copy or state transition. Each request owns exactly one
 * bounded reply slot; even an empty reply retains that storage and its tag. */
static int decode(const struct sophia_9p_slot *s, const uint8_t *b, size_t n,
                  struct sophia_9p_reply *r)
{
    size_t i;
    uint32_t count;
    if (n < 7)
        return SOPHIA_9P_INVALID;
    memset(r, 0, sizeof(*r));
    r->type = b[4];
    b += 7;
    n -= 7;
    if (r->type == 7) {
        if (n != 4 || s->type == 100 || s->type == 108)
            return SOPHIA_9P_INVALID;
        r->error = p9_u32(b);
        return 0;
    }
    if (r->type != s->type + 1)
        return SOPHIA_9P_INVALID;
    switch (s->type) {
    case 100:
        if (n != 14 || p9_u16(b + 4) != 8 || memcmp(b + 6, "9P2000.L", 8))
            return SOPHIA_9P_INVALID;
        r->count = p9_u32(b);
        return 0;
    case 104:
        return n == 13 ? qid(b, &r->qid) : SOPHIA_9P_INVALID;
    case 110:
        if (n < 2)
            return SOPHIA_9P_INVALID;
        count = p9_u16(b);
        if (count > 16 || count > s->count || (!count && s->count) || n != 2 + 13 * (size_t)count)
            return SOPHIA_9P_INVALID;
        for (i = 0; i < count; i++)
            if (qid(b + 2 + 13 * i, &r->qid))
                return SOPHIA_9P_INVALID;
        r->count = count;
        r->data = b + 2;
        return 0;
    case 12:
        if (n != 17 || qid(b, &r->qid))
            return SOPHIA_9P_INVALID;
        r->iounit = p9_u32(b + 13);
        return 0;
    case 116:
        if (n < 4)
            return SOPHIA_9P_INVALID;
        count = p9_u32(b);
        if (count > s->count || count != n - 4)
            return SOPHIA_9P_INVALID;
        r->count = count;
        r->data = b + 4;
        return 0;
    case 118:
        if (n != 4 || p9_u32(b) > s->count)
            return SOPHIA_9P_INVALID;
        r->count = p9_u32(b);
        return 0;
    case 120:
    case 108:
        return n ? SOPHIA_9P_INVALID : 0;
    default:
        return SOPHIA_9P_INVALID;
    }
}
int p9_decode(struct sophia_9p_client *c, size_t i, struct sophia_9p_reply *r)
{
    int result = decode(&c->slots[i], p9_data(c, i), c->slots[i].bytes, r);
    r->handle.slot = (uint16_t)i;
    r->handle.serial = c->slots[i].serial;
    return result;
}
int p9_receive(struct sophia_9p_client *c)
{
    size_t i;
    uint16_t tag = p9_u16(c->rx + 5);
    struct sophia_9p_slot *s;
    struct sophia_9p_reply r;
    for (i = 0; i < 2 * c->capacity; i++)
        if (c->slots[i].state && c->slots[i].tag == tag)
            break;
    if (i == 2 * c->capacity)
        return SOPHIA_9P_INVALID;
    s = &c->slots[i];
    if (s->state != P9_SENT || decode(s, c->rx, c->rx_used, &r))
        return SOPHIA_9P_INVALID;
    if (s->type == 100) {
        if (r.count < 4096 || r.count > c->offered)
            return SOPHIA_9P_INVALID;
        c->msize = r.count;
        c->phase = 2;
    }
    if ((s->type == 104 || s->type == 110) &&
        (r.type == 7 || (s->type == 110 && r.count < s->count)))
        p9_free_fid(c, s->fid);
    if (s->type == 108) {
        struct sophia_9p_slot *old = &c->slots[s->old_slot];
        if (!old->flushing)
            return SOPHIA_9P_INVALID;
        old->flushing = 0;
        if (old->state == P9_SENT) {
            if (old->type == 104 || old->type == 110)
                p9_free_fid(c, old->fid);
            memset(old, 0, sizeof(*old));
        } else if (old->state == P9_HELD)
            memset(old, 0, sizeof(*old));
        else if (old->state != P9_DONE)
            return SOPHIA_9P_INVALID;
    }
    if (c->completed == UINT64_MAX)
        return SOPHIA_9P_INVALID;
    memcpy(p9_data(c, i), c->rx, c->rx_used);
    s->bytes = c->rx_used;
    s->completed = ++c->completed;
    s->state = P9_DONE;
    return 0;
}
