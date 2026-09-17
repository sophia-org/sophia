#ifndef SOPHIA_SHELL_TEXT_H
#define SOPHIA_SHELL_TEXT_H
#include <stddef.h>
#include <stdint.h>

/* Same text contract as the Rust catalog codec: strict Unicode scalar UTF-8,
 * no controls or bidi formatting controls. Reject, never replace or truncate. */
static inline int shell_text_valid(const uint8_t *p, size_t length)
{
    size_t i = 0;
    while (i < length) {
        uint32_t cp = p[i++], minimum = 0;
        unsigned tail = 0;
        if (cp < 0x80u) {
            tail = 0;
        } else if (cp >= 0xc2u && cp <= 0xdfu) {
            cp &= 0x1fu; tail = 1; minimum = 0x80u;
        } else if (cp >= 0xe0u && cp <= 0xefu) {
            cp &= 0x0fu; tail = 2; minimum = 0x800u;
        } else if (cp >= 0xf0u && cp <= 0xf4u) {
            cp &= 7u; tail = 3; minimum = 0x10000u;
        } else {
            return 0;
        }
        if (tail > length - i)
            return 0;
        for (unsigned j = 0; j < tail; ++j) {
            uint8_t next = p[i++];
            if ((next & 0xc0u) != 0x80u)
                return 0;
            cp = (cp << 6) | (next & 0x3fu);
        }
        if (cp < minimum || cp > 0x10ffffu || (cp >= 0xd800u && cp <= 0xdfffu) ||
            cp < 0x20u || (cp >= 0x7fu && cp <= 0x9fu) ||
            (cp >= 0x202au && cp <= 0x202eu) || (cp >= 0x2066u && cp <= 0x2069u))
            return 0;
    }
    return 1;
}

#endif
