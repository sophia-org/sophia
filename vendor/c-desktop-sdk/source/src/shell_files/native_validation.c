#include "roles_internal.h"

static int words_nonzero(const uint8_t *b, size_t end)
{
    size_t at;
    for (at = 0; at < end; at += 8)
        if (!sf_get(b + at, 8))
            return 0;
    return 1;
}
static int range(const uint8_t *b, unsigned lo, unsigned hi)
{
    uint64_t value = sf_get(b, 2);
    return value >= lo && value <= hi;
}
int sf_native_input_rules(const uint8_t *);
int sf_native_allocation_rules(const uint8_t *);
static int activation(const uint8_t *b)
{
    return words_nonzero(b, 128) && range(b + 128, 1, 2) && range(b + 130, 1, 4096) &&
           sf_get(b + 120, 8) == sf_get(b + 96, 8);
}
static int catalog_activation(const uint8_t *b)
{
    return words_nonzero(b, 112) && sf_get(b + 96, 8) <= 4096 && sf_get(b + 112, 2) == 1 &&
           sf_zero(b + 114, 6) && sf_get(b + 120, 8);
}
int sf_native_wire_check(unsigned kind, const uint8_t *b, size_t n)
{
    switch (kind) {
    case 38:
        return n == 64 && words_nonzero(b, 64) ? 0 : -1;
    case 39:
        return n == 112 && words_nonzero(b, 112) ? 0 : -1;
    case 40:
        return n == 116 && words_nonzero(b, 112) && range(b + 112, 1, 12) && sf_zero(b + 114, 2)
                   ? 0
                   : -1;
    case 41:
        return n == 398 && words_nonzero(b, 136) && range(b + 136, 1, 17) ? sf_native_input_rules(b)
                                                                          : -1;
    case 42:
        return n == 136 && activation(b) && range(b + 132, 1, 5) && range(b + 134, 0, 12) &&
                       ((sf_get(b + 132, 2) == 1) == (sf_get(b + 134, 2) == 0))
                   ? 0
                   : -1;
    case 43:
        return n == 36 && words_nonzero(b, 32) && range(b + 32, 1, 12) && sf_zero(b + 34, 2) ? 0
                                                                                             : -1;
    case 44:
        return n == 132 && catalog_activation(b) && range(b + 128, 1, 5) && sf_zero(b + 130, 2)
                   ? 0
                   : -1;
    case 45:
        return n == 36 && sf_get(b, 8) && range(b + 32, 0, 3) ? 0 : -1;
    case 266:
        return n == 92 && words_nonzero(b, 56) && range(b + 72, 1, 3) && range(b + 74, 1, 4)
                   ? sf_native_allocation_rules(b)
                   : -1;
    case 268:
        return n == 132 && words_nonzero(b, 128) && range(b + 128, 1, 2) && sf_zero(b + 130, 2)
                   ? 0
                   : -1;
    case 269:
        return n == 132 && activation(b) ? 0 : -1;
    case 271:
        return n == 128 && catalog_activation(b) ? 0 : -1;
    case 272:
        return n == 56 && sf_get(b, 8) ? 0 : -1;
    default:
        return -1;
    }
}
