/* Owned-ring codec against Sophia's pinned corpus,
 * spec/golden/sophia-owned-ring-v1.tsv. Shape decode and the registered
 * capacity (184320, the corpus header) are separate checks; the category
 * column is diagnostic only and is not mapped to SDK refusals. */
#include "sophia_owned_ring.h"
#undef NDEBUG
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CASES 36
#define CAPACITY 184320u
struct vector {
    char name[64], surface[16];
    int accepted;
    unsigned fds;
    uint8_t bytes[256];
    size_t count;
};
static struct vector vectors[CASES];
static size_t vector_count;

static unsigned hex(char c)
{
    return (unsigned)(c <= '9' ? c - '0' : c - 'a' + 10);
}

static void load(void)
{
    FILE *file = fopen("spec/golden/sophia-owned-ring-v1.tsv", "r");
    static char line[1024];
    int capacity_header = 0;
    assert(file);
    while (fgets(line, sizeof line, file)) {
        struct vector *v;
        char *field[6], *at = line;
        size_t i, length;
        if (line[0] == '#') {
            capacity_header |= strstr(line, "capacity of 184320 bytes") != NULL;
            continue;
        }
        for (i = 0; i < 6; i++) {
            field[i] = at;
            at = strpbrk(at, i == 5 ? "\n" : "\t");
            assert(at);
            *at++ = 0;
        }
        assert(vector_count < CASES);
        v = &vectors[vector_count++];
        assert(strlen(field[0]) < sizeof v->name && strlen(field[1]) < sizeof v->surface);
        strcpy(v->name, field[0]);
        strcpy(v->surface, field[1]);
        v->accepted = atoi(field[2]);
        v->fds = (unsigned)atoi(field[3]);
        length = strlen(field[4]);
        assert(length % 2 == 0 && length / 2 <= sizeof v->bytes);
        v->count = length / 2;
        for (i = 0; i < v->count; i++)
            v->bytes[i] = (uint8_t)(hex(field[4][2 * i]) * 16 + hex(field[4][2 * i + 1]));
        assert(v->accepted == !strcmp(field[5], "Accept"));
    }
    fclose(file);
    assert(vector_count == CASES && capacity_header);
}

/* Decode, then the contextual admission checks, then exact re-encoding. */
static int accepts(const struct vector *v)
{
    struct sophia_or_registration reg;
    struct sophia_or_offer offer;
    struct sophia_or_event event;
    uint8_t b[SOPHIA_OR_OFFER_BYTES];
    int kind = 0;
    if (!strcmp(v->surface, "source")) {
        if (v->fds != 1 || !sophia_or_is_source(v->bytes, v->count) ||
            sophia_or_source_decode(v->bytes, v->count, &reg) != SOPHIA_IC_OK)
            return 0;
        assert(sophia_or_source_encode(b, &reg) == SOPHIA_IC_OK && !memcmp(b, v->bytes, v->count));
        return 1;
    }
    if (v->fds)
        return 0;
    if (!strcmp(v->surface, "event")) {
        if (!sophia_or_is_event(v->bytes, v->count) ||
            sophia_or_event_decode(v->bytes, v->count, &event) != SOPHIA_IC_OK)
            return 0;
        assert(sophia_or_event_encode(b, &event) == SOPHIA_IC_OK && !memcmp(b, v->bytes, v->count));
        return 1;
    }
    assert(!strcmp(v->surface, "control"));
    if (sophia_or_control_decode(v->bytes, v->count, &kind, &reg, &offer) != SOPHIA_IC_OK)
        return 0;
    if (kind == 1) {
        assert(sophia_or_register_encode(b, &reg) == SOPHIA_IC_OK &&
               !memcmp(b, v->bytes, v->count));
        return 1;
    }
    assert(kind == 2 && sophia_or_offer_encode(b, &offer) == SOPHIA_IC_OK &&
           !memcmp(b, v->bytes, v->count));
    return (uint64_t)offer.width * offer.height * 4u <= CAPACITY;
}

static void geometry(void)
{
    size_t extent, bytes;
    assert(sophia_or_geometry(1, CAPACITY, &extent, &bytes) == SOPHIA_IC_OK);
    assert(extent == 4096 + 184320 && bytes == extent);
    assert(sophia_or_geometry(4, 4u << 20, &extent, &bytes) == SOPHIA_IC_OK);
    assert(extent == 4096 + (4u << 20) && bytes == 4 * extent);
    assert(sophia_or_geometry(2, 4, &extent, &bytes) == SOPHIA_IC_OK && extent == 8192 &&
           bytes == 16384);
    assert(sophia_or_geometry(0, 4, NULL, NULL) == SOPHIA_IC_CAPACITY);
    assert(sophia_or_geometry(5, 4, NULL, NULL) == SOPHIA_IC_CAPACITY);
    assert(sophia_or_geometry(1, 6, NULL, NULL) == SOPHIA_IC_CAPACITY);
    assert(sophia_or_geometry(1, (4u << 20) + 4, NULL, NULL) == SOPHIA_IC_CAPACITY);
}

/* Slot zero is valid, and every record reaching encode is canonical. */
static void encode_controls(void)
{
    struct sophia_or_identity id = {7, 3, 9, 1, 1};
    struct sophia_or_offer offer = {{7, 3, 9, 1, 1}, 1, 0, 2, 11, 1, 21, 1, 31, 1, 41, 1920, 24};
    struct sophia_or_event event;
    struct sophia_or_registration reg = {{7, 3, 9, 1, 1}, 1, CAPACITY};
    uint8_t b[SOPHIA_OR_OFFER_BYTES];
    assert(sophia_or_offer_encode(b, &offer) == SOPHIA_IC_OK);
    offer.slot_generation = 3;
    assert(sophia_or_offer_encode(b, &offer) == SOPHIA_IC_GENERATION);
    offer.slot_generation = 2;
    offer.identity.ring = 0;
    assert(sophia_or_offer_encode(b, &offer) == SOPHIA_IC_IDENTITY);
    reg.slots = 0;
    assert(sophia_or_register_encode(b, &reg) == SOPHIA_IC_CAPACITY);
    memset(&event, 0, sizeof event);
    event.kind = SOPHIA_OR_SLOT_RELEASED;
    event.identity = id;
    event.sequence = 5;
    event.offer = 1;
    event.slot_generation = 2;
    event.resource_id = event.resource_generation = 1;
    assert(sophia_or_event_encode(b, &event) == SOPHIA_IC_OK);
    event.changed_generation = 1;
    assert(sophia_or_event_encode(b, &event) == SOPHIA_IC_VALUE);
    event.reason = 2;
    assert(sophia_or_event_encode(b, &event) == SOPHIA_IC_OK);
    event.kind = SOPHIA_OR_RING_READY;
    assert(sophia_or_event_encode(b, &event) != SOPHIA_IC_OK);
    assert(!sophia_or_is_event(b, 4) && !sophia_or_is_event(NULL, 8));
}

int main(void)
{
    size_t i;
    load();
    for (i = 0; i < vector_count; i++)
        if (accepts(&vectors[i]) != vectors[i].accepted) {
            fprintf(stderr, "owned ring corpus: %s\n", vectors[i].name);
            return 1;
        }
    geometry();
    encode_controls();
    printf("owned ring codec: %zu corpus rows, capacity as admission, geometry passed\n",
           vector_count);
    return 0;
}
