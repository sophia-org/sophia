#ifndef SOPHIA_IMAGE_CHANNEL_INTERNAL_H
#define SOPHIA_IMAGE_CHANNEL_INTERNAL_H
#include "../sophia_image_channel.h"
#include <string.h>
static inline uint64_t ic_get(const uint8_t *p, size_t n)
{
    size_t i;
    uint64_t v = 0;
    for (i = 0; i < n; i++)
        v |= (uint64_t)p[i] << (i * 8);
    return v;
}
static inline void ic_put(uint8_t *p, uint64_t v, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        p[i] = (uint8_t)(v >> (i * 8));
}
static inline int ic_zero(const uint8_t *p, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        if (p[i])
            return 0;
    return 1;
}
void ic_take_metadata(const uint8_t *, struct sophia_ic_metadata *);
void ic_put_metadata(uint8_t *, const struct sophia_ic_metadata *);
#endif
