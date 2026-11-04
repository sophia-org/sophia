#include "internal.h"
#include "../sophia_shell_frames.h"

/* Shell r9 adds two values that the shared helpers keep refusing, so
 * sophia_sf_decode and sophia_sf_encode stay r<=8:
 *   ResourceStatus Rejected (status 3) with reason 13,
 *   image_channel_unavailable; other statuses keep their bounds.
 *   Submitted naming ImageBegin (0x7000), the receipt for an accepted
 *   ImageBegin candidate; the kind is carried, never relabeled.
 * Only these explicit entry points, given negotiated revision 9, accept
 * either value; r10 also admits ImageBegin receipts for optional DMA-BUF. Each one is validated by the legacy codec with one
 * legal stand-in value, then the r9 value is restored. */
#define STATUS_AT (SOPHIA_SF_HEADER_BYTES + 40)
#define REASON_AT (SOPHIA_SF_HEADER_BYTES + 42)
#define STATUS_BYTES (SOPHIA_SF_HEADER_BYTES + 56)
#define CANDIDATE_KIND_AT (SOPHIA_SF_HEADER_BYTES + 8)
#define SUBMITTED_BYTES (SOPHIA_SF_HEADER_BYTES + 16)
#define REJECTED 3

static int r9_rejection(uint16_t revision, uint16_t kind, uint16_t status, uint16_t reason)
{
    /* r10 retires sealed ingress, the only source of reason 13. */
    return revision == SOPHIA_SF_REVISION_IMAGE_CHANNEL && kind == SOPHIA_SF_RESOURCE_STATUS &&
           status == REJECTED && reason == SOPHIA_SF_REASON_IMAGE_CHANNEL_UNAVAILABLE;
}

static int extended_receipt(uint16_t revision, uint16_t kind, uint16_t candidate_kind)
{
    if (kind != SOPHIA_SF_SUBMITTED) return 0;
    if (revision == SOPHIA_SFT_REVISION)
        return candidate_kind == SOPHIA_SF_KIND_IMAGE_BEGIN ||
               (candidate_kind >= SOPHIA_SFT_OPEN && candidate_kind <= SOPHIA_SFT_COMMIT);
    return (revision == SOPHIA_SF_REVISION_IMAGE_CHANNEL ||
            revision == SOPHIA_SF_REVISION_OWNED_RING) &&
           candidate_kind == SOPHIA_SF_KIND_IMAGE_BEGIN;
}

/* Decode a copy with the field at `at` set to `stand_in`. */
static int decode_substituted(const uint8_t *b, size_t bytes, size_t at, uint16_t stand_in,
                              struct sophia_sf_record *out)
{
    uint8_t copy[STATUS_BYTES];
    memcpy(copy, b, bytes);
    sf_put(copy + at, stand_in, 2);
    return sophia_sf_decode(copy, bytes, out) ? -1 : 0;
}

int sophia_sf_decode_revision(const void *src, size_t bytes, uint16_t revision,
                              struct sophia_sf_record *out)
{
    const uint8_t *b = src;
    struct sophia_sf_record v;
    if (!src || !out || !revision ||
        (revision > SOPHIA_SF_REVISION_OWNED_RING && revision != SOPHIA_SFT_REVISION))
        return -4;
    if (revision == SOPHIA_SFT_REVISION && bytes >= SOPHIA_SF_HEADER_BYTES &&
        ((sf_get(b + 6, 2) >= SOPHIA_SFT_OPENED && sf_get(b + 6, 2) <= SOPHIA_SFT_OUTCOME) ||
         (sf_get(b + 6, 2) >= SOPHIA_SFT_OPEN && sf_get(b + 6, 2) <= SOPHIA_SFT_COMMIT))) {
        struct sophia_sft_record frame;
        if (sophia_sft_decode(src, bytes, sf_get(b + 6, 2) < 256 ? SOPHIA_SFT_EVENT : SOPHIA_SFT_REQUEST,
                              revision, SOPHIA_SFT_MAX_BYTES, &frame)) return -1;
        memset(&v, 0, sizeof(v));
        v.header = frame.header;
        v.value.frame = frame.value;
        *out = v;
        return 0;
    }
    if (bytes == STATUS_BYTES &&
        r9_rejection(revision, (uint16_t)sf_get(b + 6, 2), (uint16_t)sf_get(b + STATUS_AT, 2),
                     (uint16_t)sf_get(b + REASON_AT, 2))) {
        if (decode_substituted(b, bytes, REASON_AT, 0, &v))
            return -1;
        v.value.resource_status.reason = SOPHIA_SF_REASON_IMAGE_CHANNEL_UNAVAILABLE;
    } else if (bytes == SUBMITTED_BYTES &&
               extended_receipt(revision, (uint16_t)sf_get(b + 6, 2),
                          (uint16_t)sf_get(b + CANDIDATE_KIND_AT, 2))) {
        if (decode_substituted(b, bytes, CANDIDATE_KIND_AT, SOPHIA_SF_NEGOTIATE, &v))
            return -1;
        v.value.submitted.candidate_kind = (uint16_t)sf_get(b + CANDIDATE_KIND_AT, 2);
    } else {
        return sophia_sf_decode(src, bytes, out);
    }
    *out = v;
    return 0;
}

int sophia_sf_encode_revision(void *dst, size_t capacity, uint16_t revision,
                              const struct sophia_sf_record *r, size_t *written)
{
    struct sophia_sf_record copy;
    size_t at;
    uint16_t value;
    int status;
    if (!dst || !r || !written || !revision ||
        (revision > SOPHIA_SF_REVISION_OWNED_RING && revision != SOPHIA_SFT_REVISION))
        return -4;
    if (revision == SOPHIA_SFT_REVISION &&
        ((r->header.kind >= SOPHIA_SFT_OPENED && r->header.kind <= SOPHIA_SFT_OUTCOME) ||
         (r->header.kind >= SOPHIA_SFT_OPEN && r->header.kind <= SOPHIA_SFT_COMMIT))) {
        const struct sophia_sft_record frame = {r->header, r->value.frame};
        return sophia_sft_encode(dst, capacity, revision, &frame, written);
    }
    copy = *r;
    if (r->header.kind == SOPHIA_SF_RESOURCE_STATUS &&
        r9_rejection(revision, r->header.kind, r->value.resource_status.status,
                     r->value.resource_status.reason)) {
        copy.value.resource_status.reason = 0;
        at = REASON_AT;
        value = SOPHIA_SF_REASON_IMAGE_CHANNEL_UNAVAILABLE;
    } else if (r->header.kind == SOPHIA_SF_SUBMITTED &&
               extended_receipt(revision, r->header.kind, r->value.submitted.candidate_kind)) {
        copy.value.submitted.candidate_kind = SOPHIA_SF_NEGOTIATE;
        at = CANDIDATE_KIND_AT;
        value = r->value.submitted.candidate_kind;
    } else {
        return sophia_sf_encode(dst, capacity, r, written);
    }
    status = sophia_sf_encode(dst, capacity, &copy, written);
    if (!status)
        sf_put((uint8_t *)dst + at, value, 2);
    return status;
}
