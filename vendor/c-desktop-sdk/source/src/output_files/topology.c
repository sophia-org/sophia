#include "internal.h"

#define PREFIX 24u
#define HEAD 104u
#define MODE 24u
#define GROUP 84u

/* Strict UTF-8: no overlong forms, surrogates or values above U+10FFFF. */
static int utf8(const uint8_t *p, size_t n) {
  size_t i = 0;
  while (i < n) {
    uint8_t c = p[i];
    size_t extra, k;
    uint32_t v;
    if (c < 0x80) {
      ++i;
      continue;
    }
    if (c >= 0xc2 && c <= 0xdf) {
      extra = 1;
      v = c & 0x1f;
    } else if (c >= 0xe0 && c <= 0xef) {
      extra = 2;
      v = c & 0x0f;
    } else if (c >= 0xf0 && c <= 0xf4) {
      extra = 3;
      v = c & 0x07;
    } else
      return 0;
    if (n - i <= extra)
      return 0;
    for (k = 1; k <= extra; ++k) {
      if ((p[i + k] & 0xc0) != 0x80)
        return 0;
      v = (v << 6) | (p[i + k] & 0x3f);
    }
    if ((extra == 2 && (v < 0x800 || (v >= 0xd800 && v <= 0xdfff))) ||
        (extra == 3 && (v < 0x10000 || v > 0x10ffff)))
      return 0;
    i += extra + 1;
  }
  return 1;
}
static const struct sophia_of_head *find_head(const struct sophia_of_topology *t,
                                              uint64_t head) {
  uint16_t i;
  for (i = 0; i < t->head_count; ++i)
    if (t->heads[i].head == head)
      return &t->heads[i];
  return NULL;
}
static int head_grouped(const struct sophia_of_topology *t, uint64_t head) {
  uint16_t g, m;
  for (g = 0; g < t->group_count; ++g)
    for (m = 0; m < t->groups[g].member_count; ++m)
      if (t->groups[g].members[m].head == head)
        return 1;
  return 0;
}
/* Wire bounds plus the published snapshot invariants in
 * spec/sophia-output-files.md. A disabled head's current mode is not checked
 * against its table; snapshot groups may overlap (proposals may not). */
int sophia_of_topology_valid(const struct sophia_of_topology *t) {
  uint32_t covered = 0;
  uint16_t i, j, k;
  if (!t || !t->topology_epoch || !t->primary_output || !t->head_count ||
      t->head_count > SOPHIA_OF_MAX_HEADS || !t->group_count ||
      t->group_count > SOPHIA_OF_MAX_GROUPS || !t->mode_count ||
      t->mode_count > SOPHIA_OF_MAX_MODES)
    return 0;
  for (i = 0; i < t->head_count; ++i) {
    const struct sophia_of_head *h = &t->heads[i];
    int current = 0;
    if (!h->head || !h->generation || (h->flags & ~7u) || !h->transforms ||
        (h->transforms & ~0xffu) || !h->label_bytes ||
        h->label_bytes > SOPHIA_OF_MAX_LABEL ||
        !utf8((const uint8_t *)h->label, h->label_bytes) || !h->mode_count ||
        h->mode_count > SOPHIA_OF_MAX_MODES_PER_HEAD ||
        h->first_mode != covered || h->mode_count > t->mode_count - covered)
      return 0;
    covered += h->mode_count;
    for (j = 0; j < i; ++j)
      if (t->heads[j].head == h->head)
        return 0;
    for (j = h->first_mode; j < covered; ++j) {
      const struct sophia_of_mode *m = &t->modes[j];
      if (!m->mode || m->width <= 0 || m->height <= 0 ||
          !m->refresh_millihz || m->preferred > 1)
        return 0;
      for (k = h->first_mode; k < j; ++k)
        if (t->modes[k].mode == m->mode)
          return 0;
      current |= h->current_mode && m->mode == h->current_mode;
    }
    if ((h->flags & SOPHIA_OF_HEAD_ENABLED) &&
        (!(h->flags & SOPHIA_OF_HEAD_CONNECTED) || !current))
      return 0;
  }
  if (covered != t->mode_count)
    return 0;
  for (i = 0; i < t->group_count; ++i) {
    const struct sophia_of_group *g = &t->groups[i];
    if (!g->output || !g->generation || g->x < 0 || g->y < 0 ||
        g->width <= 0 || g->height <= 0 || !g->member_count ||
        g->member_count > SOPHIA_OF_MAX_MEMBERS)
      return 0;
    for (j = 0; j < i; ++j)
      if (t->groups[j].output == g->output)
        return 0;
    for (j = 0; j < g->member_count; ++j) {
      uint64_t head = g->members[j].head;
      uint16_t earlier_group, earlier;
      if (!find_head(t, head) || g->members[j].mapping < SOPHIA_OF_FIT ||
          g->members[j].mapping > SOPHIA_OF_EXACT)
        return 0;
      /* A head belongs to at most one group. */
      for (earlier_group = 0; earlier_group <= i; ++earlier_group) {
        const struct sophia_of_group *e = &t->groups[earlier_group];
        uint16_t end = earlier_group == i ? j : e->member_count;
        for (earlier = 0; earlier < end; ++earlier)
          if (e->members[earlier].head == head)
            return 0;
      }
    }
  }
  for (i = 0; i < t->group_count && t->groups[i].output != t->primary_output;
       ++i)
    ;
  if (i == t->group_count)
    return 0;
  for (i = 0; i < t->head_count; ++i)
    if ((t->heads[i].flags & SOPHIA_OF_HEAD_ENABLED) &&
        !head_grouped(t, t->heads[i].head))
      return 0;
  return 1;
}
int sophia_of_topology_decode(const void *src, size_t size,
                              struct sophia_of_header *header,
                              struct sophia_of_topology *out) {
  const uint8_t *p = src, *row;
  struct sophia_of_header h;
  uint16_t heads, groups, modes, i;
  if (!src || !header || !out)
    return -4;
  if (size < SOPHIA_OF_HEADER_BYTES + PREFIX || size > SOPHIA_OF_MAX_RECORD ||
      of_get(p, 4) != size || of_get(p + 4, 2) != 1)
    return -1;
  h.kind = (uint16_t)of_get(p + 6, 2);
  h.epoch = of_get(p + 8, 8);
  h.submission = of_get(p + 16, 8);
  h.sequence = of_get(p + 24, 8);
  p += 32;
  heads = (uint16_t)of_get(p + 16, 2);
  groups = (uint16_t)of_get(p + 18, 2);
  modes = (uint16_t)of_get(p + 20, 2);
  /* Counts and exact length are checked before any row is read. */
  if (h.kind != SOPHIA_OF_TOPOLOGY || of_header(&h) || !heads ||
      heads > SOPHIA_OF_MAX_HEADS || !groups || groups > SOPHIA_OF_MAX_GROUPS ||
      !modes || modes > SOPHIA_OF_MAX_MODES || !of_zero(p + 22, 2) ||
      size != 32u + PREFIX + (size_t)heads * HEAD + (size_t)modes * MODE +
                  (size_t)groups * GROUP)
    return -1;
  /* The table is too large to stage twice; a failed decode clears it. */
  memset(out, 0, sizeof(*out));
  out->topology_epoch = of_get(p, 8);
  out->primary_output = of_get(p + 8, 8);
  out->head_count = heads;
  out->group_count = groups;
  out->mode_count = modes;
  row = p + PREFIX;
  for (i = 0; i < heads; ++i, row += HEAD) {
    struct sophia_of_head *d = &out->heads[i];
    d->head = of_get(row, 8);
    d->generation = of_get(row + 8, 8);
    d->flags = (uint16_t)of_get(row + 16, 2);
    d->transforms = (uint16_t)of_get(row + 18, 2);
    d->label_bytes = (uint16_t)of_get(row + 20, 2);
    d->mode_count = (uint16_t)of_get(row + 22, 2);
    d->current_mode = of_get(row + 24, 8);
    d->first_mode = (uint16_t)of_get(row + 32, 2);
    if (!of_zero(row + 34, 6) || !d->label_bytes ||
        d->label_bytes > SOPHIA_OF_MAX_LABEL ||
        !of_zero(row + 40 + d->label_bytes, SOPHIA_OF_MAX_LABEL - d->label_bytes))
      goto invalid;
    memcpy(d->label, row + 40, d->label_bytes);
  }
  for (i = 0; i < modes; ++i, row += MODE) {
    struct sophia_of_mode *m = &out->modes[i];
    m->mode = of_get(row, 8);
    m->width = of_signed(row + 8);
    m->height = of_signed(row + 12);
    m->refresh_millihz = (uint32_t)of_get(row + 16, 4);
    m->preferred = (uint16_t)of_get(row + 20, 2);
    if (!of_zero(row + 22, 2))
      goto invalid;
  }
  for (i = 0; i < groups; ++i, row += GROUP) {
    struct sophia_of_group *g = &out->groups[i];
    g->output = of_get(row, 8);
    g->generation = of_get(row + 8, 8);
    g->x = of_signed(row + 16);
    g->y = of_signed(row + 20);
    g->width = of_signed(row + 24);
    g->height = of_signed(row + 28);
    g->member_count = (uint16_t)of_get(row + 32, 2);
    if (!of_zero(row + 34, 2) ||
        of_members_read(row + 36, g->member_count, g->members))
      goto invalid;
  }
  if (!sophia_of_topology_valid(out))
    goto invalid;
  *header = h;
  return 0;
invalid:
  memset(out, 0, sizeof(*out));
  return -1;
}
int sophia_of_topology_encode(void *dst, size_t capacity, uint64_t epoch,
                              const struct sophia_of_topology *t,
                              size_t *bytes) {
  uint8_t *p = dst, *row;
  struct sophia_of_header h;
  size_t size;
  uint16_t i;
  if (!dst || !t || !bytes)
    return -4;
  if (!epoch || !sophia_of_topology_valid(t))
    return -1;
  size = 32u + PREFIX + (size_t)t->head_count * HEAD +
         (size_t)t->mode_count * MODE + (size_t)t->group_count * GROUP;
  if (capacity < size)
    return -4;
  memset(p, 0, size);
  h.kind = SOPHIA_OF_TOPOLOGY;
  h.epoch = epoch;
  h.submission = h.sequence = 0;
  of_header_write(p, &h, size);
  p += 32;
  of_put(p, t->topology_epoch, 8);
  of_put(p + 8, t->primary_output, 8);
  of_put(p + 16, t->head_count, 2);
  of_put(p + 18, t->group_count, 2);
  of_put(p + 20, t->mode_count, 2);
  row = p + PREFIX;
  for (i = 0; i < t->head_count; ++i, row += HEAD) {
    const struct sophia_of_head *d = &t->heads[i];
    of_put(row, d->head, 8);
    of_put(row + 8, d->generation, 8);
    of_put(row + 16, d->flags, 2);
    of_put(row + 18, d->transforms, 2);
    of_put(row + 20, d->label_bytes, 2);
    of_put(row + 22, d->mode_count, 2);
    of_put(row + 24, d->current_mode, 8);
    of_put(row + 32, d->first_mode, 2);
    memcpy(row + 40, d->label, d->label_bytes);
  }
  for (i = 0; i < t->mode_count; ++i, row += MODE) {
    const struct sophia_of_mode *m = &t->modes[i];
    of_put(row, m->mode, 8);
    of_put(row + 8, (uint32_t)m->width, 4);
    of_put(row + 12, (uint32_t)m->height, 4);
    of_put(row + 16, m->refresh_millihz, 4);
    of_put(row + 20, m->preferred, 2);
  }
  for (i = 0; i < t->group_count; ++i, row += GROUP) {
    const struct sophia_of_group *g = &t->groups[i];
    of_put(row, g->output, 8);
    of_put(row + 8, g->generation, 8);
    of_put(row + 16, (uint32_t)g->x, 4);
    of_put(row + 20, (uint32_t)g->y, 4);
    of_put(row + 24, (uint32_t)g->width, 4);
    of_put(row + 28, (uint32_t)g->height, 4);
    of_put(row + 32, g->member_count, 2);
    of_members_write(row + 36, g->member_count, g->members);
  }
  *bytes = size;
  return 0;
}
