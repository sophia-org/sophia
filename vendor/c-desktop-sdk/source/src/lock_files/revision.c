#include "../sophia_lock_files.h"
#include <string.h>

/* Lock r2 adds Submitted receipts naming ImageBegin (0x7000), the receipt
 * for an accepted ImageBegin candidate; the kind is carried, never
 * relabeled. sophia_lf_decode and sophia_lf_encode stay r1 and refuse it.
 * These entry points, given negotiated revision 2, validate the record with
 * the r1 codec and one legal stand-in kind, then restore 0x7000.
 * Negotiated is handled the same way: the r1 codec checks the record with
 * selection 1 and the r1 grants, then the selection and image bits are
 * restored and checked against each other. */
#define CANDIDATE_KIND_AT (SOPHIA_LF_HEADER_BYTES + 8)
#define SUBMITTED_BYTES (SOPHIA_LF_HEADER_BYTES + 16)
#define SELECTED_AT SOPHIA_LF_HEADER_BYTES
#define GRANTED_AT (SOPHIA_LF_HEADER_BYTES + 8)
#define NEGOTIATED_BYTES (SOPHIA_LF_HEADER_BYTES + 16)
#define R1_GRANTS ((uint64_t)(SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS))
#define IMAGE_GRANTS ((uint64_t)(SOPHIA_LF_CAP_IMAGE_SEALED | SOPHIA_LF_CAP_IMAGE_DMABUF))

static uint16_t get16(const uint8_t *p) {
  return (uint16_t)(p[0] | (unsigned)p[1] << 8);
}
static void put16(uint8_t *p, uint16_t v) {
  p[0] = (uint8_t)v;
  p[1] = (uint8_t)(v >> 8);
}
static uint64_t get64(const uint8_t *p) {
  uint64_t v = 0;
  int i;
  for (i = 7; i >= 0; --i)
    v = v << 8 | p[i];
  return v;
}
static void put64(uint8_t *p, uint64_t v) {
  int i;
  for (i = 0; i < 8; ++i)
    p[i] = (uint8_t)(v >> (8 * i));
}
/* A selection within the bound, with image bits only when it is 2. */
static int selection_valid(uint16_t revision, uint16_t selected,
                           uint64_t granted) {
  return selected >= 1 && selected <= revision &&
         !(granted & ~(R1_GRANTS | IMAGE_GRANTS)) &&
         (selected == SOPHIA_LF_REVISION_IMAGE_CHANNEL || !(granted & IMAGE_GRANTS));
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
  if (bytes == NEGOTIATED_BYTES && get16(b + 6) == SOPHIA_LF_NEGOTIATED) {
    uint16_t selected = get16(b + SELECTED_AT);
    uint64_t granted = get64(b + GRANTED_AT);
    if (!selection_valid(revision, selected, granted))
      return -1;
    memcpy(copy, b, sizeof copy);
    put16(copy + SELECTED_AT, SOPHIA_LF_REVISION);
    put64(copy + GRANTED_AT, granted & R1_GRANTS);
    if (sophia_lf_decode(copy, sizeof copy, &v))
      return -1;
    v.value.negotiated.granted_capabilities = granted;
    v.value.negotiated.selected_revision = selected;
    *out = v;
    return 0;
  }
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
  if (record->header.kind == SOPHIA_LF_NEGOTIATED) {
    const struct sophia_lf_negotiated *n = &record->value.negotiated;
    uint16_t selected = n->selected_revision ? n->selected_revision : SOPHIA_LF_REVISION;
    if (!selection_valid(revision, selected, n->granted_capabilities))
      return -1;
    copy = *record;
    copy.value.negotiated.granted_capabilities &= R1_GRANTS;
    copy.value.negotiated.selected_revision = SOPHIA_LF_REVISION;
    status = sophia_lf_encode(dst, capacity, &copy, bytes);
    if (!status) {
      put16((uint8_t *)dst + SELECTED_AT, selected);
      put64((uint8_t *)dst + GRANTED_AT, n->granted_capabilities);
    }
    return status;
  }
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
