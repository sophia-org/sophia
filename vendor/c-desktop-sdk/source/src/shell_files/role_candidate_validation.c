#include "roles_internal.h"

static int words(const uint8_t *b, size_t n)
{
    size_t i;
    for (i = 0; i < n; i += 8)
        if (!sf_get(b + i, 8))
            return 0;
    return 1;
}
static int margins_valid(const uint8_t *b)
{
    size_t i;
    for (i = 0; i < 8; i += 2) {
        int64_t value = sf_signed(b + i, 2);
        if (value < -512 || value > 512)
            return 0;
    }
    return 1;
}
int sf_role_candidate_wire_check(const uint8_t *b, size_t n, int native)
{
    size_t i, at = native ? 108 : 88, counts = native ? 98 : 80;
    size_t ns, np, nt, nr = 0;
    if (n < at || n + 32 > 8192 || !words(b, 80))
        return -1;
    ns = sf_get(b + counts, 2);
    np = sf_get(b + counts + 2, 2);
    nt = sf_get(b + counts + 4, 2);
    if (ns > (native ? 1 : 8) || np > 32 || nt > (native ? 32 : 64))
        return -1;
    if (native) {
        nr = sf_get(b + 104, 2);
        if (nr > 32 || !sf_get(b + 80, 8) || !sf_get(b + 88, 8) || sf_get(b + 96, 2) > 4096 ||
            !sf_zero(b + 106, 2))
            return -1;
    } else if (!sf_zero(b + 86, 2))
        return -1;
    if (n != at + 64 * ns + 32 * np + 48 * nt + 2 * nr)
        return -1;
    for (i = 0; i < ns; i++, at += 64) {
        const uint8_t *s = b + at;
        if (!words(s, 24) || sf_get(s + 24, 2) != (native ? 3u : 1u) || sf_get(s + 26, 2) < 1 ||
            sf_get(s + 26, 2) > 4 || !margins_valid(s + 28) || sf_get(s + 36, 4) > 512 ||
            (native && sf_get(s + 36, 4)) || sf_get(s + 40, 2) != 65535 || !sf_zero(s + 42, 22))
            return -1;
    }
    for (i = 0; i < np; i++, at += 32) {
        const uint8_t *p = b + at;
        if (!words(p, 16) || sf_get(p + 16, 2) > 7 || (native && sf_get(p + 16, 2)) ||
            !sf_zero(p + 18, 2) || sf_signed(p + 20, 4) < 0 || sf_signed(p + 24, 4) < 0 ||
            !sf_zero(p + 28, 4))
            return -1;
    }
    for (i = 0; i < nt; i++, at += 48) {
        const uint8_t *t = b + at;
        if (sf_get(t, 2) > 7 || (native && sf_get(t, 2)) ||
            sf_get(t + 2, 2) != (native ? 2u : 3u) || !sf_get(t + 4, 8) || !sf_get(t + 12, 8) ||
            !sf_get(t + 20, 8) || sf_get(t + 20, 8) > 4096 || sf_signed(t + 28, 4) < 0 ||
            sf_signed(t + 32, 4) < 0 || !sf_get(t + 36, 4) || !sf_get(t + 40, 4) ||
            !sf_zero(t + 44, 4) || sf_get(t + 28, 4) + sf_get(t + 36, 4) > INT32_MAX ||
            sf_get(t + 32, 4) + sf_get(t + 40, 4) > INT32_MAX)
            return -1;
    }
    for (i = 0; i < nr; i++, at += 2)
        if (!sf_get(b + at, 2) || sf_get(b + at, 2) > 4096)
            return -1;
    return 0;
}
int sf_native_allocation_rules(const uint8_t *b)
{
    uint64_t prior = sf_get(b + 56, 8), gen = sf_get(b + 64, 8), op = sf_get(b + 72, 2);
    if (!margins_valid(b + 84) || !sf_pair(prior, gen) || ((op == 1) != (prior == 0)))
        return -1;
    if (op == 3)
        return sf_zero(b + 76, 16) ? 0 : -1;
    return sf_get(b + 76, 4) && sf_get(b + 80, 4) ? 0 : -1;
}
int sf_native_input_rules(const uint8_t *b)
{
    struct sophia_sf_text text;
    uint64_t kind = sf_get(b + 136, 2), state = sf_get(b + 120, 8), binding = sf_get(b + 96, 8);
    if (sf_text_take(b + 138, 256, &text) || ((kind == 1) != (text.size != 0)) ||
        sf_launcher_text_check(text, 256))
        return -1;
    return (kind == 17 ? state == binding : state > binding) ? 0 : -1;
}
