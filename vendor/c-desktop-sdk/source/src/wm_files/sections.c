#include "internal.h"

/* Section layout and disclosure are file-codec rules. Scene membership,
 * geometry, cross-row policy constraints and resource authority belong to
 * Session's semantic validators; a codec-valid candidate is not admitted. */
int wf_sections(const struct sophia_wf_record *r, uint64_t selected,
                size_t *bytes) {
  uint16_t family = r->header.kind, previous = 0;
  size_t total = 0, i, j;
  int array = family == SOPHIA_WF_SNAPSHOT || family == SOPHIA_WF_PROJECTION ||
              family == SOPHIA_WF_CONFIGURATION;
  if (r->section_count > SOPHIA_WF_MAX_SECTIONS || (!array && r->section_count))
    return -1;
  if ((family == SOPHIA_WF_SNAPSHOT || family == SOPHIA_WF_PROJECTION) &&
      (!r->section_count || r->sections[0].kind != 1))
    return -1;
  for (i = 0; i < r->section_count; ++i) {
    const struct sophia_wf_section *s = &r->sections[i];
    size_t width;
    uint32_t maximum;
    uint64_t required;
    if (!s->rows || !s->kind || s->kind <= previous || !s->count ||
        wf_row_layout(family, s->kind, &width, &maximum, &required) ||
        s->count > maximum || (selected & required) != required ||
        s->bytes != (size_t)s->count * width ||
        s->bytes > SOPHIA_WF_MAX_RECORD - 16 ||
        total > SOPHIA_WF_MAX_RECORD - 16 - s->bytes)
      return -1;
    for (j = 0; j < s->count; ++j)
      if (wf_row_reserved(family, s->kind, s->rows + j * width))
        return -1;
    total += 16 + s->bytes;
    previous = s->kind;
  }
  if (family == SOPHIA_WF_SNAPSHOT) {
    const struct sophia_wf_section *s = &r->sections[0];
    int active = 0;
    for (i = 0; i < s->count; ++i)
      if (wf_get(s->rows + i * SOPHIA_WF_SNAPSHOT_OUTPUT_BYTES, 8) ==
          r->value.snapshot.active_output)
        active = 1;
    if (!active)
      return -1;
  }
  *bytes = total;
  return 0;
}
