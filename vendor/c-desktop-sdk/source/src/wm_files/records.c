#include "internal.h"

static int header(const struct sophia_wf_header *v) {
  if (!v->epoch)
    return -1;
  if (v->kind == SOPHIA_WF_LIMITS || v->kind == SOPHIA_WF_SNAPSHOT)
    return !v->submission && !v->sequence ? 0 : -1;
  if (v->kind >= SOPHIA_WF_NEGOTIATED &&
      v->kind <= SOPHIA_WF_PRESENTATION_RECEIPT)
    return !v->submission && v->sequence ? 0 : -1;
  if (v->kind >= SOPHIA_WF_NEGOTIATE && v->kind <= SOPHIA_WF_SESSION_OPERATION)
    return v->submission && !v->sequence ? 0 : -1;
  return -1;
}
int sophia_wf_decode(const void *src, size_t size, uint64_t selected,
                     struct sophia_wf_record *out) {
  const uint8_t *p = src;
  struct sophia_wf_record r;
  size_t prefix, offset, i, section_bytes;
  if (!src || !out)
    return -4;
  if (size < SOPHIA_WF_HEADER_BYTES || size > SOPHIA_WF_MAX_RECORD ||
      wf_get(p, 4) != size || wf_get(p + 4, 2) != 1)
    return -1;
  memset(&r, 0, sizeof(r));
  r.header.kind = (uint16_t)wf_get(p + 6, 2);
  r.header.epoch = wf_get(p + 8, 8);
  r.header.submission = wf_get(p + 16, 8);
  r.header.sequence = wf_get(p + 24, 8);
  if (header(&r.header) ||
      wf_prefix_read(p + 32, size - 32, selected, &r, &prefix))
    return -1;
  offset = 32 + prefix;
  for (i = 0; i < r.section_count; ++i) {
    struct sophia_wf_section *s = &r.sections[i];
    if (size - offset < 16 || !wf_zero(p + offset + 2, 2) ||
        !wf_zero(p + offset + 12, 4))
      return -1;
    s->kind = (uint16_t)wf_get(p + offset, 2);
    s->count = (uint32_t)wf_get(p + offset + 4, 4);
    s->bytes = (size_t)wf_get(p + offset + 8, 4);
    offset += 16;
    if (s->bytes > size - offset)
      return -1;
    s->rows = p + offset;
    offset += s->bytes;
  }
  if (offset != size || wf_sections(&r, selected, &section_bytes))
    return -1;
  *out = r;
  return 0;
}
int sophia_wf_encode(void *dst, size_t capacity, uint64_t selected,
                     const struct sophia_wf_record *r, size_t *bytes) {
  uint8_t prefix[240], *p = dst;
  struct sophia_wf_record check;
  size_t size, prefix_bytes, checked_bytes, section_bytes, i, offset;
  if (!dst || !r || !bytes)
    return -4;
  if (header(&r->header) || wf_prefix_write(prefix, r, &prefix_bytes))
    return -1;
  memset(&check, 0, sizeof(check));
  check.header = r->header;
  if (wf_prefix_read(prefix, prefix_bytes, selected, &check, &checked_bytes) ||
      checked_bytes != prefix_bytes ||
      wf_sections(r, selected, &section_bytes) ||
      section_bytes > SOPHIA_WF_MAX_RECORD - 32 - prefix_bytes)
    return -1;
  size = 32 + prefix_bytes + section_bytes;
  if (capacity < size)
    return -4;
  wf_put(p, size, 4);
  wf_put(p + 4, 1, 2);
  wf_put(p + 6, r->header.kind, 2);
  wf_put(p + 8, r->header.epoch, 8);
  wf_put(p + 16, r->header.submission, 8);
  wf_put(p + 24, r->header.sequence, 8);
  memcpy(p + 32, prefix, prefix_bytes);
  offset = 32 + prefix_bytes;
  for (i = 0; i < r->section_count; ++i) {
    const struct sophia_wf_section *s = &r->sections[i];
    wf_put(p + offset, s->kind, 2);
    wf_put(p + offset + 2, 0, 2);
    wf_put(p + offset + 4, s->count, 4);
    wf_put(p + offset + 8, s->bytes, 4);
    wf_put(p + offset + 12, 0, 4);
    memcpy(p + offset + 16, s->rows, s->bytes);
    offset += 16 + s->bytes;
  }
  *bytes = size;
  return 0;
}
int sophia_wf_submit_encode(uint8_t dst[24], uint64_t epoch, uint64_t id,
                            uint32_t bytes) {
  if (!dst)
    return -4;
  if (!epoch || !id || bytes < 32 || bytes > SOPHIA_WF_MAX_RECORD)
    return -1;
  wf_put(dst, epoch, 8);
  wf_put(dst + 8, id, 8);
  wf_put(dst + 16, bytes, 4);
  wf_put(dst + 20, 0, 4);
  return 0;
}
int sophia_wf_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence) {
  if (!dst)
    return -4;
  if (!epoch || !sequence)
    return -1;
  wf_put(dst, epoch, 8);
  wf_put(dst + 8, sequence, 8);
  return 0;
}
