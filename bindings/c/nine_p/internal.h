#ifndef SOPHIA_NINE_P_INTERNAL_H
#define SOPHIA_NINE_P_INTERNAL_H
#include "../sophia_9p_client.h"
#include <string.h>
enum { P9_FREE, P9_QUEUED, P9_SENT, P9_DONE, P9_HELD };
static inline uint16_t p9_u16(const uint8_t *p) { return (uint16_t)(p[0] | (uint16_t)p[1] << 8); }
static inline uint32_t p9_u32(const uint8_t *p)
{
    return (uint32_t)p9_u16(p) | (uint32_t)p9_u16(p + 2) << 16;
}
static inline uint64_t p9_u64(const uint8_t *p)
{
    return (uint64_t)p9_u32(p) | (uint64_t)p9_u32(p + 4) << 32;
}
static inline void p9_put(uint8_t *p, uint64_t v, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        p[i] = (uint8_t)(v >> (8 * i));
}
static inline uint8_t *p9_data(struct sophia_9p_client *c, size_t i)
{
    return c->storage + i * c->offered;
}
int p9_begin(struct sophia_9p_client *, uint8_t type, size_t body, int slot,
             struct sophia_9p_handle *, uint8_t **);
int p9_available(struct sophia_9p_client *, int *);
int p9_fid(struct sophia_9p_client *, uint32_t *);
void p9_free_fid(struct sophia_9p_client *, uint32_t);
int p9_receive(struct sophia_9p_client *);
int p9_decode(struct sophia_9p_client *, size_t, struct sophia_9p_reply *);
#endif
