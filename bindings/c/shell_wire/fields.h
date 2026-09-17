#ifndef SOPHIA_SHELL_WIRE_FIELDS_H
#define SOPHIA_SHELL_WIRE_FIELDS_H

#include "../sophia_shell_wire.h"

static inline uint16_t shell_get16(const uint8_t *p)
{
    return (uint16_t)((uint16_t)p[0] | (uint16_t)p[1] << 8);
}

static inline uint32_t shell_get32(const uint8_t *p)
{
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

static inline uint64_t shell_get64(const uint8_t *p)
{
    return (uint64_t)shell_get32(p) | (uint64_t)shell_get32(p + 4) << 32;
}

static inline void shell_put16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
}

static inline void shell_put32(uint8_t *p, uint32_t v)
{
    for (unsigned i = 0; i < 4; ++i)
        p[i] = (uint8_t)(v >> (8 * i));
}

static inline void shell_put64(uint8_t *p, uint64_t v)
{
    for (unsigned i = 0; i < 8; ++i)
        p[i] = (uint8_t)(v >> (8 * i));
}

/* -1 unknown, 0 Session->shell, 1 shell->Session. Explicit published inventory. */
static inline int shell_direction(uint16_t kind)
{
    switch (kind) {
    case 96: case 99: case 102: case 107: case 112: case 118: case 121:
    case 163: case 165: case 167: case 168: case 169: case 170: case 172:
    case 173: case 174: case 176: case 178: case 180: case 185:
        return 1;
    case 97: case 98: case 100: case 101: case 103: case 104: case 105:
    case 106: case 108: case 109: case 110: case 111: case 113: case 114:
    case 115: case 116: case 117: case 119: case 120: case 122: case 160:
    case 161: case 162: case 164: case 166: case 171: case 175: case 177:
    case 179: case 181: case 182: case 183: case 184: case 186:
        return 0;
    default:
        return -1;
    }
}

static inline int shell_transaction_valid(uint16_t kind, uint64_t tx)
{
    return ((kind == 96 || kind == 97 || kind == 160 || kind == 161) ? tx == 0 : tx != 0);
}

int shell_header(const uint8_t *src, size_t capacity, size_t *total);

#endif
