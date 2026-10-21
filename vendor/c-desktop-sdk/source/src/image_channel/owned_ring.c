/* Owned-ring records: u32 total length, four magic bytes, u64 words. */
#include "internal.h"
#include "../sophia_owned_ring.h"

static const uint8_t CONTROL[4] = {'D', '4', 'C', '1'};
static const uint8_t SOURCE[4] = {'D', '4', 'F', '1'};
static const uint8_t EVENT[4] = {'D', '4', 'E', '1'};

static int magic(const void *b, size_t n, const uint8_t m[4])
{
    return b && n >= 8 && !memcmp((const uint8_t *)b + 4, m, 4);
}

int sophia_or_is_event(const void *b, size_t n)
{
    return magic(b, n, EVENT);
}

int sophia_or_is_source(const void *b, size_t n)
{
    return magic(b, n, SOURCE);
}

static void put_words(uint8_t *dst, const uint8_t m[4], const uint64_t *w, size_t count)
{
    size_t i;
    ic_put(dst, 8 + 8 * count, 4);
    memcpy(dst + 4, m, 4);
    for (i = 0; i < count; i++)
        ic_put(dst + 8 + 8 * i, w[i], 8);
}

static enum sophia_ic_refusal get_words(const void *src, size_t n, const uint8_t m[4],
                                        uint64_t *w, size_t count)
{
    const uint8_t *b = src;
    size_t i;
    if (!src)
        return SOPHIA_IC_ARGUMENT;
    if (n != 8 + 8 * count || ic_get(b, 4) != n)
        return SOPHIA_IC_LENGTH;
    if (memcmp(b + 4, m, 4))
        return SOPHIA_IC_KIND;
    for (i = 0; i < count; i++)
        w[i] = ic_get(b + 8 + 8 * i, 8);
    return SOPHIA_IC_OK;
}

enum sophia_ic_refusal sophia_or_geometry(uint32_t slots, uint64_t capacity, size_t *extent,
                                          size_t *source_bytes)
{
    size_t e;
    if (!slots || slots > SOPHIA_OR_MAX_SLOTS || !capacity || capacity > SOPHIA_OR_MAX_CAPACITY ||
        capacity % 4)
        return SOPHIA_IC_CAPACITY;
    e = SOPHIA_OR_SLOT_HEADER_BYTES + (size_t)(capacity + 4095u) / 4096u * 4096u;
    if (extent)
        *extent = e;
    if (source_bytes)
        *source_bytes = e * slots;
    return SOPHIA_IC_OK;
}

static void identity_words(uint64_t *w, uint64_t kind, const struct sophia_or_identity *id)
{
    w[0] = kind;
    w[1] = id->connection_epoch;
    w[2] = id->content_epoch;
    w[3] = id->channel_generation;
    w[4] = id->ring;
    w[5] = id->ring_generation;
}

static enum sophia_ic_refusal identity(const uint64_t *w, struct sophia_or_identity *id)
{
    size_t i;
    for (i = 1; i < 6; i++)
        if (!w[i])
            return SOPHIA_IC_IDENTITY;
    id->connection_epoch = w[1];
    id->content_epoch = w[2];
    id->channel_generation = w[3];
    id->ring = w[4];
    id->ring_generation = w[5];
    return SOPHIA_IC_OK;
}

int sophia_or_identity_equal(const struct sophia_or_identity *a, const struct sophia_or_identity *b)
{
    return a && b && a->connection_epoch == b->connection_epoch &&
           a->content_epoch == b->content_epoch &&
           a->channel_generation == b->channel_generation && a->ring == b->ring &&
           a->ring_generation == b->ring_generation;
}

static enum sophia_ic_refusal registration(const uint64_t *w, struct sophia_or_registration *out)
{
    struct sophia_or_registration v;
    enum sophia_ic_refusal r;
    if (w[0] != 1)
        return SOPHIA_IC_KIND;
    if ((r = identity(w, &v.identity)) != SOPHIA_IC_OK)
        return r;
    if (w[6] > SOPHIA_OR_MAX_SLOTS || sophia_or_geometry((uint32_t)w[6], w[7], NULL, NULL))
        return SOPHIA_IC_CAPACITY;
    v.slots = (uint32_t)w[6];
    v.capacity = w[7];
    if (out)
        *out = v;
    return SOPHIA_IC_OK;
}

static enum sophia_ic_refusal offer(const uint64_t *w, struct sophia_or_offer *out)
{
    struct sophia_or_offer v;
    enum sophia_ic_refusal r;
    size_t i;
    if (w[0] != 2)
        return SOPHIA_IC_KIND;
    if ((r = identity(w, &v.identity)) != SOPHIA_IC_OK)
        return r;
    if (!w[6])
        return SOPHIA_IC_OFFER;
    if (w[7] >= SOPHIA_OR_MAX_SLOTS)
        return SOPHIA_IC_VALUE;
    if (!w[8] || w[8] % 2)
        return SOPHIA_IC_GENERATION;
    for (i = 9; i < 16; i++)
        if (!w[i])
            return SOPHIA_IC_IDENTITY;
    /* Shape bounds only; the registered capacity is the caller's check. */
    if (!w[16] || !w[17] || w[16] > UINT32_MAX || w[17] > UINT32_MAX ||
        w[16] * w[17] > SOPHIA_OR_MAX_CAPACITY / 4)
        return SOPHIA_IC_DIMENSIONS;
    v.offer = w[6];
    v.slot = (uint32_t)w[7];
    v.slot_generation = w[8];
    v.resource_id = w[9];
    v.resource_generation = w[10];
    v.allocation_id = w[11];
    v.allocation_generation = w[12];
    v.output_id = w[13];
    v.output_generation = w[14];
    v.topology_generation = w[15];
    v.width = (uint32_t)w[16];
    v.height = (uint32_t)w[17];
    if (out)
        *out = v;
    return SOPHIA_IC_OK;
}

static enum sophia_ic_refusal event(const uint64_t *w, struct sophia_or_event *out)
{
    struct sophia_or_event v;
    enum sophia_ic_refusal r;
    if (!w[0] || w[0] > SOPHIA_OR_SLOT_RELEASED)
        return SOPHIA_IC_KIND;
    if ((r = identity(w, &v.identity)) != SOPHIA_IC_OK)
        return r;
    if (!w[6])
        return SOPHIA_IC_ORDER;
    if (w[12] > UINT16_MAX || w[13] > 1)
        return SOPHIA_IC_VALUE;
    if (w[0] == SOPHIA_OR_SLOT_RELEASED) {
        if (w[13] && !w[12])
            return SOPHIA_IC_VALUE;
        if (!w[7])
            return SOPHIA_IC_OFFER;
        if (w[8] >= SOPHIA_OR_MAX_SLOTS)
            return SOPHIA_IC_VALUE;
        if (!w[9] || w[9] % 2)
            return SOPHIA_IC_GENERATION;
        if (!w[10] || !w[11])
            return SOPHIA_IC_IDENTITY;
    } else {
        if ((w[0] == SOPHIA_OR_RING_REFUSED) != (w[12] != 0))
            return SOPHIA_IC_VALUE;
        if (w[7] || w[8] || w[9] || w[10] || w[11] || w[13])
            return SOPHIA_IC_RESERVED;
    }
    v.kind = (enum sophia_or_event_kind)w[0];
    v.sequence = w[6];
    v.offer = w[7];
    v.slot = (uint32_t)w[8];
    v.slot_generation = w[9];
    v.resource_id = w[10];
    v.resource_generation = w[11];
    v.reason = (uint16_t)w[12];
    v.changed_generation = (uint8_t)w[13];
    if (out)
        *out = v;
    return SOPHIA_IC_OK;
}

static enum sophia_ic_refusal registration_words(uint64_t w[8],
                                                 const struct sophia_or_registration *v)
{
    if (!v)
        return SOPHIA_IC_ARGUMENT;
    identity_words(w, 1, &v->identity);
    w[6] = v->slots;
    w[7] = v->capacity;
    return registration(w, NULL);
}

enum sophia_ic_refusal sophia_or_register_encode(uint8_t dst[SOPHIA_OR_REGISTER_BYTES],
                                                 const struct sophia_or_registration *v)
{
    uint64_t w[8];
    enum sophia_ic_refusal r;
    if (!dst)
        return SOPHIA_IC_ARGUMENT;
    if ((r = registration_words(w, v)) == SOPHIA_IC_OK)
        put_words(dst, CONTROL, w, 8);
    return r;
}

enum sophia_ic_refusal sophia_or_source_encode(uint8_t dst[SOPHIA_OR_SOURCE_BYTES],
                                               const struct sophia_or_registration *v)
{
    uint64_t w[8];
    enum sophia_ic_refusal r;
    if (!dst)
        return SOPHIA_IC_ARGUMENT;
    if ((r = registration_words(w, v)) == SOPHIA_IC_OK)
        put_words(dst, SOURCE, w, 8);
    return r;
}

enum sophia_ic_refusal sophia_or_offer_encode(uint8_t dst[SOPHIA_OR_OFFER_BYTES],
                                              const struct sophia_or_offer *v)
{
    uint64_t w[18];
    enum sophia_ic_refusal r;
    if (!dst || !v)
        return SOPHIA_IC_ARGUMENT;
    identity_words(w, 2, &v->identity);
    w[6] = v->offer;
    w[7] = v->slot;
    w[8] = v->slot_generation;
    w[9] = v->resource_id;
    w[10] = v->resource_generation;
    w[11] = v->allocation_id;
    w[12] = v->allocation_generation;
    w[13] = v->output_id;
    w[14] = v->output_generation;
    w[15] = v->topology_generation;
    w[16] = v->width;
    w[17] = v->height;
    if ((r = offer(w, NULL)) == SOPHIA_IC_OK)
        put_words(dst, CONTROL, w, 18);
    return r;
}

enum sophia_ic_refusal sophia_or_event_encode(uint8_t dst[SOPHIA_OR_EVENT_BYTES],
                                              const struct sophia_or_event *v)
{
    uint64_t w[14];
    enum sophia_ic_refusal r;
    if (!dst || !v)
        return SOPHIA_IC_ARGUMENT;
    identity_words(w, (uint64_t)v->kind, &v->identity);
    w[6] = v->sequence;
    w[7] = v->offer;
    w[8] = v->slot;
    w[9] = v->slot_generation;
    w[10] = v->resource_id;
    w[11] = v->resource_generation;
    w[12] = v->reason;
    w[13] = v->changed_generation;
    if ((r = event(w, NULL)) == SOPHIA_IC_OK)
        put_words(dst, EVENT, w, 14);
    return r;
}

enum sophia_ic_refusal sophia_or_control_decode(const void *src, size_t n, int *kind,
                                                struct sophia_or_registration *reg,
                                                struct sophia_or_offer *off)
{
    uint64_t w[18];
    enum sophia_ic_refusal r;
    if (!kind || !reg || !off)
        return SOPHIA_IC_ARGUMENT;
    if (n == SOPHIA_OR_REGISTER_BYTES) {
        if ((r = get_words(src, n, CONTROL, w, 8)) == SOPHIA_IC_OK &&
            (r = registration(w, reg)) == SOPHIA_IC_OK)
            *kind = 1;
        return r;
    }
    if ((r = get_words(src, n, CONTROL, w, 18)) == SOPHIA_IC_OK && (r = offer(w, off)) == SOPHIA_IC_OK)
        *kind = 2;
    return r;
}

enum sophia_ic_refusal sophia_or_source_decode(const void *src, size_t n,
                                               struct sophia_or_registration *out)
{
    uint64_t w[8];
    enum sophia_ic_refusal r;
    if (!out)
        return SOPHIA_IC_ARGUMENT;
    if ((r = get_words(src, n, SOURCE, w, 8)) != SOPHIA_IC_OK)
        return r;
    return registration(w, out);
}

enum sophia_ic_refusal sophia_or_event_decode(const void *src, size_t n, struct sophia_or_event *out)
{
    uint64_t w[14];
    enum sophia_ic_refusal r;
    if (!out)
        return SOPHIA_IC_ARGUMENT;
    if ((r = get_words(src, n, EVENT, w, 14)) != SOPHIA_IC_OK)
        return r;
    return event(w, out);
}
