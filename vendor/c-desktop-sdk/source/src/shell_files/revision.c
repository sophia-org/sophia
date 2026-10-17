#include "internal.h"

/* Shell r9 admits ResourceStatus Rejected (status 3) with reason 13,
 * image_channel_unavailable. The shared reason helpers keep their <= 12
 * bounds, so sophia_sf_decode and sophia_sf_encode still refuse reason 13.
 * Only these explicit entry points, given a negotiated revision of at least
 * 9, accept it, and only on that one status. */
#define STATUS_AT (SOPHIA_SF_HEADER_BYTES + 40)
#define REASON_AT (SOPHIA_SF_HEADER_BYTES + 42)
#define STATUS_BYTES (SOPHIA_SF_HEADER_BYTES + 56)
#define REJECTED 3

static int r9_rejection(uint16_t revision, uint16_t kind, uint16_t status, uint16_t reason)
{
    return revision >= SOPHIA_SF_REVISION_IMAGE_CHANNEL && kind == SOPHIA_SF_RESOURCE_STATUS &&
           status == REJECTED && reason == SOPHIA_SF_REASON_IMAGE_CHANNEL_UNAVAILABLE;
}

int sophia_sf_decode_revision(const void *src, size_t bytes, uint16_t revision,
                              struct sophia_sf_record *out)
{
    const uint8_t *b = src;
    uint8_t copy[STATUS_BYTES];
    if (!src || !out || !revision || revision > SOPHIA_SF_REVISION_IMAGE_CHANNEL)
        return -4;
    if (bytes != STATUS_BYTES ||
        !r9_rejection(revision, (uint16_t)sf_get(b + 6, 2), (uint16_t)sf_get(b + STATUS_AT, 2),
                      (uint16_t)sf_get(b + REASON_AT, 2)))
        return sophia_sf_decode(src, bytes, out);
    memcpy(copy, b, sizeof copy);
    sf_put(copy + REASON_AT, 0, 2);
    if (sophia_sf_decode(copy, sizeof copy, out))
        return -1;
    out->value.resource_status.reason = SOPHIA_SF_REASON_IMAGE_CHANNEL_UNAVAILABLE;
    return 0;
}

int sophia_sf_encode_revision(void *dst, size_t capacity, uint16_t revision,
                              const struct sophia_sf_record *r, size_t *written)
{
    struct sophia_sf_record copy;
    int status;
    if (!dst || !r || !written || !revision || revision > SOPHIA_SF_REVISION_IMAGE_CHANNEL)
        return -4;
    if (r->header.kind != SOPHIA_SF_RESOURCE_STATUS ||
        !r9_rejection(revision, r->header.kind, r->value.resource_status.status,
                      r->value.resource_status.reason))
        return sophia_sf_encode(dst, capacity, r, written);
    copy = *r;
    copy.value.resource_status.reason = 0;
    status = sophia_sf_encode(dst, capacity, &copy, written);
    if (!status)
        sf_put((uint8_t *)dst + REASON_AT, SOPHIA_SF_REASON_IMAGE_CHANNEL_UNAVAILABLE, 2);
    return status;
}
