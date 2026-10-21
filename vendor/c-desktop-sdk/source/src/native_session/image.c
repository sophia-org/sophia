#include "internal.h"
#include "../sophia_shell_image.h"
#include "../image_producer/internal.h"
#include "../shell_session/internal.h"

/* A free registry entry, or NULL; *duplicate when the identity is held. */
static struct sophia_ns_resource *free_entry(struct sophia_ns *n, uint64_t id, uint64_t generation,
                                             int *duplicate)
{
    struct sophia_ns_resource *slot = NULL;
    unsigned i;
    *duplicate = 0;
    for (i = 0; i < SOPHIA_NS_RESOURCES; i++) {
        struct sophia_ns_resource *entry = &n->resources[i];
        if (entry->state == SOPHIA_NS_RESOURCE_FREE) {
            if (!slot)
                slot = entry;
        } else if (entry->id == id && entry->generation == generation)
            *duplicate = 1;
    }
    return slot;
}

static void adopted(struct sophia_ns_resource *slot, uint64_t id, uint64_t generation,
                    uint64_t offer)
{
    memset(slot, 0, sizeof(*slot));
    slot->id = id;
    slot->generation = generation;
    slot->image_offer = offer;
    slot->transaction = offer;
    slot->state = SOPHIA_NS_RESOURCE_ACCEPTED;
}

/* r10: only Accepted plus the exact successful SlotReleased is adoptable. */
static int adopt_ring(struct sophia_ns *n, struct sophia_ss_image *image, uint64_t offer)
{
    struct sophia_ss_ring_offer_info info;
    struct sophia_ns_resource *slot;
    const struct sophia_or_offer *o = &info.offer;
    int duplicate;
    if (sophia_ss_image_ring_info(image, offer, &info) != SOPHIA_IPR_OK || !info.adoptable ||
        o->identity.connection_epoch != n->limits.grant_connection_epoch ||
        o->identity.content_epoch != n->limits.grant_content_epoch ||
        o->width > n->limits.max_width_px || o->height > n->limits.max_height_px ||
        (uint64_t)o->width * o->height * 4u > n->limits.max_resource_bytes)
        return SOPHIA_9P_INVALID;
    slot = free_entry(n, o->resource_id, o->resource_generation, &duplicate);
    if (duplicate)
        return SOPHIA_9P_INVALID;
    if (!slot)
        return SOPHIA_9P_BUSY;
    /* As for an ISB, consumption follows every fallible check. */
    if (sophia_ss_image_ring_forget(image, offer) != SOPHIA_IPR_OK)
        return SOPHIA_9P_INVALID;
    adopted(slot, o->resource_id, o->resource_generation, offer);
    return 0;
}

int sophia_ns_adopt_image(struct sophia_ns *n, struct sophia_ss_image *image, uint64_t offer)
{
    const struct sophia_sf_limits *limits;
    const struct sophia_ip_slot *source;
    struct sophia_ns_resource *slot = NULL;
    struct sophia_ic_expect expect;
    struct sophia_ic_metadata metadata;
    unsigned i;
    int r = ns_gate(n);
    if (r)
        return r;
    if (!image || !offer || image->session != n->ss || n->ss->image != image)
        return SOPHIA_9P_ARGUMENT;
    if (n->ss->files.profile != SOPHIA_SF_LAUNCHER)
        return SOPHIA_9P_INVALID;
    limits = sophia_ss_limits(n->ss);
    if (!limits || !ns_grant(n, limits->grant_connection_epoch, limits->grant_content_epoch))
        return SOPHIA_9P_INVALID;
    if (ss_ring_mode(image))
        return adopt_ring(n, image, offer);
    source = ip_slot(&image->producer, offer);
    if (image->producer.config.connection_epoch != n->limits.grant_connection_epoch ||
        !source || source->state != SOPHIA_IPO_SETTLED || source->status != SOPHIA_IPS_ACCEPTED ||
        source->disposition != SOPHIA_IC_RETAINED ||
        source->metadata.admission_epoch != n->limits.grant_content_epoch)
        return SOPHIA_9P_INVALID;
    metadata = source->metadata;
    expect = image->producer.expect;
    expect.max_width = n->limits.max_width_px;
    expect.max_height = n->limits.max_height_px;
    expect.max_resource_bytes = n->limits.max_resource_bytes;
    if (sophia_ic_metadata_admissible(&metadata, &expect) != SOPHIA_IC_OK)
        return SOPHIA_9P_INVALID;
    for (i = 0; i < SOPHIA_NS_RESOURCES; i++) {
        struct sophia_ns_resource *entry = &n->resources[i];
        if (entry->state == SOPHIA_NS_RESOURCE_FREE) {
            if (!slot)
                slot = entry;
        } else if (entry->id == metadata.resource_id && entry->generation == metadata.resource_generation)
            return SOPHIA_9P_INVALID;
    }
    if (!slot)
        return SOPHIA_9P_BUSY;
    /* Everything fallible precedes this consumption; no operation below can
     * fail or allocate. The offer cannot later recreate a released resource. */
    if (sophia_ip_forget(&image->producer, offer) != SOPHIA_IPR_OK)
        return SOPHIA_9P_INVALID;
    memset(slot, 0, sizeof(*slot));
    slot->id = metadata.resource_id;
    slot->generation = metadata.resource_generation;
    slot->image_offer = offer;
    slot->transaction = offer;
    slot->state = SOPHIA_NS_RESOURCE_ACCEPTED;
    return 0;
}
