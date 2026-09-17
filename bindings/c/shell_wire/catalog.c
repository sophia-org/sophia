#include "../sophia_shell_catalog.h"
#include "fields.h"

#include <string.h>

static int overlap(const void *a, size_t n, const void *b, size_t m)
{
    uintptr_t x = (uintptr_t)a, y = (uintptr_t)b;
    if (n > UINTPTR_MAX - x || m > UINTPTR_MAX - y)
        return 1;
    return x < y + m && y < x + n;
}

int sophia_shell_catalog_init(struct sophia_shell_catalog *c,
                             const struct sophia_shell_welcome *welcome,
                             struct sophia_shell_catalog_entry *first,
                             struct sophia_shell_catalog_entry *second,
                             size_t capacity)
{
    if (!c || !welcome || !first || !second || !capacity ||
        capacity > SOPHIA_SHELL_CATALOG_MAX_ENTRIES ||
        !welcome->connection_epoch || welcome->revision < 4u ||
        welcome->revision > SOPHIA_SHELL_WIRE_MAX_REVISION ||
        !(welcome->capabilities & SOPHIA_SHELL_CAP_APPLICATION_CATALOG))
        return SOPHIA_SHELL_CATALOG_ARGUMENT;
    size_t bytes = capacity * sizeof(*first);
    if (overlap(first, bytes, second, bytes) || overlap(first, bytes, c, sizeof(*c)) ||
        overlap(second, bytes, c, sizeof(*c)) || overlap(welcome, sizeof(*welcome), c, sizeof(*c)))
        return SOPHIA_SHELL_CATALOG_ARGUMENT;
    *c = (struct sophia_shell_catalog){
        .entries = {first, second}, .capacity = capacity,
        .connection_epoch = welcome->connection_epoch
    };
    return SOPHIA_SHELL_CATALOG_PENDING;
}

/* Same text contract as the Rust catalog codec: strict Unicode scalar UTF-8,
 * no controls or bidi formatting controls. Reject, never replace or truncate. */
static int text_valid(const uint8_t *p, size_t length)
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

static int entry(struct sophia_shell_catalog *c, const uint8_t *p, size_t bytes)
{
    if (bytes < 24u || c->received >= c->expected)
        return 0;
    struct sophia_shell_catalog_entry value = {0};
    value.slot = shell_get16(p + 16);
    value.available = shell_get16(p + 18);
    value.label_bytes = shell_get16(p + 20);
    if (!value.slot || value.slot > SOPHIA_SHELL_CATALOG_MAX_ENTRIES ||
        value.available > 1u || !value.label_bytes ||
        value.label_bytes > SOPHIA_SHELL_CATALOG_LABEL_BYTES ||
        value.label_bytes > bytes - 24u)
        return 0;
    size_t keyword_offset = 22u + value.label_bytes;
    value.keywords_bytes = shell_get16(p + keyword_offset);
    if (value.keywords_bytes > SOPHIA_SHELL_CATALOG_KEYWORDS_BYTES ||
        keyword_offset + 2u + value.keywords_bytes != bytes ||
        !text_valid(p + 22, value.label_bytes) ||
        !text_valid(p + keyword_offset + 2u, value.keywords_bytes))
        return 0;
    unsigned index = value.slot - 1u;
    uint64_t bit = UINT64_C(1) << (index % 64u);
    if (c->seen[index / 64u] & bit)
        return 0;
    memcpy(value.label, p + 22, value.label_bytes);
    memcpy(value.keywords, p + keyword_offset + 2u, value.keywords_bytes);
    c->entries[1u - c->active][c->received++] = value;
    c->seen[index / 64u] |= bit;
    return 1;
}

int sophia_shell_catalog_accept(struct sophia_shell_catalog *c,
                               const struct sophia_shell_frame *frame)
{
    if (!c || !frame || !c->entries[0] || !c->entries[1] || !c->capacity)
        return SOPHIA_SHELL_CATALOG_ARGUMENT;
    if (c->failed)
        return SOPHIA_SHELL_CATALOG_INVALID;
    if (frame->kind < 114u || frame->kind > 116u)
        return SOPHIA_SHELL_CATALOG_UNRELATED;
    const uint8_t *p = frame->payload;
    size_t bytes = frame->payload_bytes;
    if (!p || bytes < 16u || bytes > SOPHIA_SHELL_MAX_PAYLOAD_BYTES ||
        !frame->transaction || shell_get64(p) != c->connection_epoch)
        goto invalid;
    uint64_t generation = shell_get64(p + 8);
    if (!generation)
        goto invalid;
    if (frame->kind == 114u) {
        if (c->assembling || generation <= c->generation || bytes != 20u ||
            shell_get16(p + 18) || shell_get16(p + 16) > c->capacity)
            goto invalid;
        c->assembling = 1;
        c->pending_generation = generation;
        c->transaction = frame->transaction;
        c->expected = shell_get16(p + 16);
        c->received = 0;
        memset(c->seen, 0, sizeof(c->seen));
        return SOPHIA_SHELL_CATALOG_PENDING;
    }
    if (!c->assembling || generation != c->pending_generation || frame->transaction != c->transaction)
        goto invalid;
    if (frame->kind == 115u) {
        if (!entry(c, p, bytes))
            goto invalid;
        return SOPHIA_SHELL_CATALOG_PENDING;
    }
    if (bytes != 16u || c->received != c->expected)
        goto invalid;
    c->active = 1u - c->active;
    c->generation = generation;
    c->count = c->received;
    c->assembling = 0;
    return SOPHIA_SHELL_CATALOG_COMMITTED;
invalid:
    c->failed = 1;
    return SOPHIA_SHELL_CATALOG_INVALID;
}

const struct sophia_shell_catalog_entry *sophia_shell_catalog_entries(
    const struct sophia_shell_catalog *c, size_t *count, uint64_t *generation)
{
    if (!c || !count || !generation || !c->generation)
        return NULL;
    *count = c->count;
    *generation = c->generation;
    return c->entries[c->active];
}
