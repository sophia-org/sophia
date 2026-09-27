#include "internal.h"
#include <errno.h>
#include <sys/socket.h>

size_t sophia_9p_storage_bytes(uint32_t msize, uint16_t requests)
{
    size_t slots = (size_t)requests * 2 + 1;
    if (msize < 4096 || msize > 16u * 1024u * 1024u || !requests || requests > SOPHIA_9P_REQUESTS ||
        msize > SIZE_MAX / slots)
        return 0;
    return msize * slots;
}
int sophia_9p_init(struct sophia_9p_client *c, int fd, uint32_t msize, uint16_t requests,
                   uint16_t fids, void *storage, size_t bytes)
{
    size_t needed = sophia_9p_storage_bytes(msize, requests);
    uintptr_t a = (uintptr_t)c, b = (uintptr_t)storage;
    if (!c || fd < 0 || !storage || !needed || bytes < needed || !fids || fids > SOPHIA_9P_FIDS ||
        a > UINTPTR_MAX - sizeof(*c) || b > UINTPTR_MAX - needed ||
        !(a + sizeof(*c) <= b || b + needed <= a))
        return SOPHIA_9P_ARGUMENT;
    memset(c, 0, sizeof(*c));
    c->fd = fd;
    c->offered = c->msize = msize;
    c->capacity = requests;
    c->fid_limit = fids;
    c->storage = storage;
    c->rx = c->storage + 2 * (size_t)requests * msize;
    c->rx_needed = 4;
    return SOPHIA_9P_OK;
}
int p9_available(struct sophia_9p_client *c, int *slot)
{
    size_t i;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (c->phase != 2)
        return SOPHIA_9P_BUSY;
    for (i = 0; i < c->capacity; i++)
        if (!c->slots[i].state && !c->slots[c->capacity + i].state) {
            *slot = (int)i;
            return 0;
        }
    return SOPHIA_9P_BUSY;
}
int p9_begin(struct sophia_9p_client *c, uint8_t type, size_t body, int slot,
             struct sophia_9p_handle *h, uint8_t **out)
{
    size_t i;
    uint16_t tag;
    int used;
    struct sophia_9p_slot *s;
    uint8_t *b;
    if (!c || !h || slot < 0 || (size_t)slot >= 2 * c->capacity)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (c->slots[slot].state)
        return SOPHIA_9P_BUSY;
    if (body > c->msize - 7u || c->serial == UINT64_MAX)
        return SOPHIA_9P_ARGUMENT;
    tag = type == 100 ? UINT16_MAX : c->next_tag;
    if (type != 100)
        do {
            if (tag == UINT16_MAX)
                tag = 0;
            used = 0;
            for (i = 0; i < 2 * c->capacity; i++)
                if (c->slots[i].state && c->slots[i].tag == tag)
                    used = 1;
            if (used)
                tag++;
        } while (used);
    s = &c->slots[slot];
    memset(s, 0, sizeof(*s));
    s->serial = ++c->serial;
    s->tag = tag;
    s->type = type;
    s->state = P9_QUEUED;
    s->bytes = body + 7;
    s->fid = UINT32_MAX;
    b = p9_data(c, (size_t)slot);
    p9_put(b, s->bytes, 4);
    b[4] = type;
    p9_put(b + 5, tag, 2);
    h->slot = (uint16_t)slot;
    h->serial = s->serial;
    *out = b + 7;
    if (type != 100)
        c->next_tag = (uint16_t)(tag + 1);
    return 0;
}
int p9_fid(struct sophia_9p_client *c, uint32_t *fid)
{
    size_t i;
    uint32_t f = c->next_fid;
    int used;
    if (c->fid_count >= c->fid_limit)
        return SOPHIA_9P_BUSY;
    do {
        if (f == UINT32_MAX)
            f = 0;
        used = 0;
        for (i = 0; i < c->fid_count; i++)
            if (c->fids[i] == f)
                used = 1;
        if (used)
            f++;
    } while (used);
    *fid = f;
    return 0;
}
void p9_free_fid(struct sophia_9p_client *c, uint32_t f)
{
    size_t i;
    for (i = 0; i < c->fid_count; i++)
        if (c->fids[i] == f) {
            c->fids[i] = c->fids[--c->fid_count];
            return;
        }
}
static int output(struct sophia_9p_client *c, size_t budget)
{
    unsigned calls;
    for (calls = 0; calls < 32 && budget; calls++) {
        size_t i, best = 2 * c->capacity, amount;
        uint64_t serial = UINT64_MAX;
        struct sophia_9p_slot *s;
        ssize_t n;
        for (i = 0; i < 2 * c->capacity; i++)
            if (c->slots[i].state == P9_QUEUED && c->slots[i].serial < serial) {
                serial = c->slots[i].serial;
                best = i;
            }
        if (best == 2 * c->capacity)
            break;
        s = &c->slots[best];
        amount = s->bytes - s->sent;
        if (amount > budget)
            amount = budget;
        n = send(c->fd, p9_data(c, best) + s->sent, amount, MSG_DONTWAIT | MSG_NOSIGNAL);
        if (n < 0) {
            if (errno == EINTR)
                continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK)
                break;
            return c->terminal = SOPHIA_9P_IO;
        }
        if (!n)
            return c->terminal = SOPHIA_9P_IO;
        s->sent += (size_t)n;
        budget -= (size_t)n;
        if (s->sent == s->bytes)
            s->state = P9_SENT;
    }
    return 0;
}
static int closed_status(const struct sophia_9p_client *c)
{
    size_t i;
    /* A clean EOF cannot invalidate replies already validated in this batch.
     * Requests are still refused immediately; only completion draining remains. */
    if (c->terminal == SOPHIA_9P_CLOSED)
        for (i = 0; i < 2 * c->capacity; i++)
            if (c->slots[i].state == P9_DONE && !c->slots[i].consumed)
                return 0;
    return c->terminal;
}
int sophia_9p_service(struct sophia_9p_client *c, size_t budget)
{
    unsigned calls;
    int r;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return closed_status(c);
    r = output(c, budget);
    if (r)
        return r;
    for (calls = 0; calls < 32 && budget; calls++) {
        size_t amount;
        ssize_t n;
        /* Read exactly one frame, never beyond the negotiated bound. */
        amount = c->rx_needed - c->rx_used;
        if (amount > budget)
            amount = budget;
        n = recv(c->fd, c->rx + c->rx_used, amount, MSG_DONTWAIT);
        if (n < 0) {
            if (errno == EINTR)
                continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK)
                break;
            return c->terminal = SOPHIA_9P_IO;
        }
        if (!n) {
            c->terminal = c->rx_used ? SOPHIA_9P_INVALID : SOPHIA_9P_CLOSED;
            return closed_status(c);
        }
        c->rx_used += (size_t)n;
        budget -= (size_t)n;
        if (c->rx_used == 4) {
            c->rx_needed = p9_u32(c->rx);
            if (c->rx_needed < 7 || c->rx_needed > c->msize)
                return c->terminal = SOPHIA_9P_INVALID;
        }
        if (c->rx_used == c->rx_needed && c->rx_used >= 7) {
            r = p9_receive(c);
            if (r)
                return c->terminal = r;
            c->rx_used = 0;
            c->rx_needed = 4;
        }
    }
    return 0;
}
int sophia_9p_wants_write(const struct sophia_9p_client *c)
{
    size_t i;
    if (!c || c->terminal)
        return 0;
    for (i = 0; i < 2 * c->capacity; i++)
        if (c->slots[i].state == P9_QUEUED)
            return 1;
    return 0;
}
int sophia_9p_peek(struct sophia_9p_client *c, struct sophia_9p_reply *r)
{
    size_t i, best;
    uint64_t serial = UINT64_MAX;
    if (!c || !r)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal && c->terminal != SOPHIA_9P_CLOSED)
        return c->terminal;
    best = 2 * c->capacity;
    for (i = 0; i < 2 * c->capacity; i++)
        if (c->slots[i].state == P9_DONE && !c->slots[i].consumed &&
            c->slots[i].completed < serial) {
            best = i;
            serial = c->slots[i].completed;
        }
    return best == 2 * c->capacity ? (c->terminal ? c->terminal : SOPHIA_9P_AGAIN)
                                   : p9_decode(c, best, r);
}
int sophia_9p_consume(struct sophia_9p_client *c, struct sophia_9p_handle h)
{
    struct sophia_9p_slot *s;
    if (!c || h.slot >= 2 * c->capacity)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal && c->terminal != SOPHIA_9P_CLOSED)
        return c->terminal;
    s = &c->slots[h.slot];
    if (s->state != P9_DONE || s->serial != h.serial || s->consumed)
        return SOPHIA_9P_ARGUMENT;
    if (s->flushing) {
        s->consumed = 1;
        s->state = P9_HELD;
    } else
        memset(s, 0, sizeof(*s));
    return 0;
}
