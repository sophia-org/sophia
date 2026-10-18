#include "internal.h"
#include "../sophia_shell_image.h"
#include "../image_producer/internal.h"

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
    source = ip_slot(&image->producer, offer);
    if (!limits || !ns_grant(n, limits->grant_connection_epoch, limits->grant_content_epoch) ||
        image->producer.config.connection_epoch != n->limits.grant_connection_epoch ||
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
