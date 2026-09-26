#ifndef SOPHIA_SHELL_FILES_INTERNAL_H
#define SOPHIA_SHELL_FILES_INTERNAL_H
#include "../sophia_shell_files.h"
#include <limits.h>
#include <string.h>
static inline uint64_t sf_get(const uint8_t *p, size_t n)
{
    size_t i;
    uint64_t v = 0;
    for (i = 0; i < n; i++)
        v |= (uint64_t)p[i] << (i * 8);
    return v;
}
static inline int64_t sf_signed(const uint8_t *p, size_t n)
{
    uint64_t v = sf_get(p, n), sign = UINT64_C(1) << (n * 8 - 1);
    return v & sign ? -(int64_t)((sign * 2 - 1) - v) - 1 : (int64_t)v;
}
static inline void sf_put(uint8_t *p, uint64_t v, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        p[i] = (uint8_t)(v >> (i * 8));
}
static inline int sf_zero(const uint8_t *p, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        if (p[i])
            return 0;
    return 1;
}
static inline int sf_pair(uint64_t id, uint64_t gen) { return (!id) == (!gen); }
static inline int sf_scale(uint32_t n, uint32_t d)
{
    uint32_t a = n, b = d;
    if (!n || n > 32 || !d || d > 4)
        return 0;
    while (b) {
        uint32_t r = a % b;
        a = b;
        b = r;
    }
    return a == 1;
}
static inline int sf_rect(int32_t x, int32_t y, uint32_t w, uint32_t h)
{
    return x >= 0 && y >= 0 && w && h && (uint64_t)(uint32_t)x + w <= INT32_MAX &&
           (uint64_t)(uint32_t)y + h <= INT32_MAX;
}
int sf_validate(const struct sophia_sf_record *);
#include "fields.h"
#endif
