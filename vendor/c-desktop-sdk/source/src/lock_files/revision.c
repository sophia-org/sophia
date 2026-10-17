#include "../sophia_lock_files.h"
#include <string.h>

/* Lock r2 adds Submitted receipts naming ImageBegin (0x7000), the receipt
 * for an accepted ImageBegin candidate; the kind is carried, never
 * relabeled. sophia_lf_decode and sophia_lf_encode stay r1 and refuse it.
 * These entry points, given negotiated revision 2, validate the record with
 * the r1 codec and one legal stand-in kind, then restore 0x7000. */
#define CANDIDATE_KIND_AT (SOPHIA_LF_HEADER_BYTES + 8)
#define SUBMITTED_BYTES (SOPHIA_LF_HEADER_BYTES + 16)

static uint16_t get16(const uint8_t *p) {
  return (uint16_t)(p[0] | (unsigned)p[1] << 8);
}
static void put16(uint8_t *p, uint16_t v) {
  p[0] = (uint8_t)v;
  p[1] = (uint8_t)(v >> 8);
}
static int r2_receipt(uint16_t revision, uint16_t kind,
                      uint16_t candidate_kind) {
  return revision >= SOPHIA_LF_REVISION_IMAGE_CHANNEL &&
         kind == SOPHIA_LF_SUBMITTED &&
         candidate_kind == SOPHIA_LF_KIND_IMAGE_BEGIN;
}

int sophia_lf_decode_revision(const void *src, size_t bytes,
                              uint16_t revision, struct sophia_lf_record *out) {
  const uint8_t *b = src;
  uint8_t copy[SUBMITTED_BYTES];
  struct sophia_lf_record v;
  if (!src || !out || !revision ||
      revision > SOPHIA_LF_REVISION_IMAGE_CHANNEL)
    return -4;
  if (bytes != SUBMITTED_BYTES ||
      !r2_receipt(revision, get16(b + 6), get16(b + CANDIDATE_KIND_AT)))
    return sophia_lf_decode(src, bytes, out);
  memcpy(copy, b, sizeof copy);
  put16(copy + CANDIDATE_KIND_AT, SOPHIA_LF_NEGOTIATE);
  if (sophia_lf_decode(copy, sizeof copy, &v))
    return -1;
  v.value.submitted.kind = SOPHIA_LF_KIND_IMAGE_BEGIN;
  *out = v;
  return 0;
}

int sophia_lf_encode_revision(void *dst, size_t capacity, uint16_t revision,
                              const struct sophia_lf_record *record,
                              size_t *bytes) {
  struct sophia_lf_record copy;
  int status;
  if (!dst || !record || !bytes || !revision ||
      revision > SOPHIA_LF_REVISION_IMAGE_CHANNEL)
    return -4;
  if (!r2_receipt(revision, record->header.kind,
                  record->header.kind == SOPHIA_LF_SUBMITTED
                      ? record->value.submitted.kind
                      : 0))
    return sophia_lf_encode(dst, capacity, record, bytes);
  copy = *record;
  copy.value.submitted.kind = SOPHIA_LF_NEGOTIATE;
  status = sophia_lf_encode(dst, capacity, &copy, bytes);
  if (!status)
    put16((uint8_t *)dst + CANDIDATE_KIND_AT, SOPHIA_LF_KIND_IMAGE_BEGIN);
  return status;
}
