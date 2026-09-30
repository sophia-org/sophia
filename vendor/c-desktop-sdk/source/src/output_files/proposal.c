#include "internal.h"

#define PREFIX 24u
#define TARGET 32u
#define GROUP 76u
#define MEMBER 12u

int of_members_read(const uint8_t *p, uint16_t count,
                    struct sophia_of_member *out) {
  uint16_t i;
  if (!count || count > SOPHIA_OF_MAX_MEMBERS)
    return -1;
  for (i = 0; i < count; ++i) {
    const uint8_t *m = p + i * MEMBER;
    out[i].head = of_get(m, 8);
    out[i].mapping = (uint16_t)of_get(m + 8, 2);
    if (out[i].mapping < SOPHIA_OF_FIT || out[i].mapping > SOPHIA_OF_EXACT ||
        !of_zero(m + 10, 2))
      return -1;
  }
  return of_zero(p + count * MEMBER, (SOPHIA_OF_MAX_MEMBERS - count) * MEMBER)
             ? 0
             : -1;
}
void of_members_write(uint8_t *p, uint16_t count,
                      const struct sophia_of_member *v) {
  uint16_t i;
  memset(p, 0, SOPHIA_OF_MAX_MEMBERS * MEMBER);
  for (i = 0; i < count && i < SOPHIA_OF_MAX_MEMBERS; ++i) {
    of_put(p + i * MEMBER, v[i].head, 8);
    of_put(p + i * MEMBER + 8, v[i].mapping, 2);
  }
}
/* Structure only: identities, generations, geometry and the primary index
 * reach the topology owner, which keeps its specific refusals. */
int of_proposal_read(const uint8_t *p, size_t n,
                     struct sophia_of_proposal *v) {
  uint16_t i;
  if (n < PREFIX)
    return -1;
  v->transaction = of_get(p, 8);
  v->base_topology_epoch = of_get(p + 8, 8);
  v->intent = (uint16_t)of_get(p + 16, 2);
  v->primary_group_index = (uint16_t)of_get(p + 18, 2);
  v->head_count = (uint16_t)of_get(p + 20, 2);
  v->group_count = (uint16_t)of_get(p + 22, 2);
  if (!v->transaction || !v->base_topology_epoch ||
      (v->intent != SOPHIA_OF_VALIDATE_ONLY && v->intent != SOPHIA_OF_APPLY) ||
      !v->head_count || v->head_count > SOPHIA_OF_MAX_HEADS ||
      !v->group_count || v->group_count > SOPHIA_OF_MAX_GROUPS ||
      n != PREFIX + v->head_count * TARGET + v->group_count * GROUP)
    return -1;
  p += PREFIX;
  for (i = 0; i < v->head_count; ++i, p += TARGET) {
    struct sophia_of_head_target *t = &v->heads[i];
    t->head = of_get(p, 8);
    t->generation = of_get(p + 8, 8);
    t->mode = of_get(p + 16, 8);
    t->transform = (uint16_t)of_get(p + 24, 2);
    t->vrr = (uint16_t)of_get(p + 26, 2);
    if (t->transform < SOPHIA_OF_NORMAL || t->transform > SOPHIA_OF_FLIPPED270 ||
        t->vrr < SOPHIA_OF_VRR_DISABLED || t->vrr > SOPHIA_OF_VRR_ALWAYS ||
        !of_zero(p + 28, 4))
      return -1;
  }
  for (i = 0; i < v->group_count; ++i, p += GROUP) {
    struct sophia_of_proposal_group *g = &v->groups[i];
    g->output = of_get(p, 8);
    g->x = of_signed(p + 8);
    g->y = of_signed(p + 12);
    g->width = of_signed(p + 16);
    g->height = of_signed(p + 20);
    g->member_count = (uint16_t)of_get(p + 24, 2);
    if (!of_zero(p + 26, 2) ||
        of_members_read(p + 28, g->member_count, g->members))
      return -1;
  }
  return 0;
}
int of_proposal_write(uint8_t *p, size_t n, const struct sophia_of_proposal *v,
                      size_t *size) {
  uint16_t i;
  size_t total;
  if (!v->head_count || v->head_count > SOPHIA_OF_MAX_HEADS ||
      !v->group_count || v->group_count > SOPHIA_OF_MAX_GROUPS)
    return -1;
  for (i = 0; i < v->group_count; ++i)
    if (!v->groups[i].member_count ||
        v->groups[i].member_count > SOPHIA_OF_MAX_MEMBERS)
      return -1;
  total = PREFIX + v->head_count * TARGET + v->group_count * GROUP;
  if (n < total)
    return -4;
  memset(p, 0, total);
  of_put(p, v->transaction, 8);
  of_put(p + 8, v->base_topology_epoch, 8);
  of_put(p + 16, v->intent, 2);
  of_put(p + 18, v->primary_group_index, 2);
  of_put(p + 20, v->head_count, 2);
  of_put(p + 22, v->group_count, 2);
  p += PREFIX;
  for (i = 0; i < v->head_count; ++i, p += TARGET) {
    of_put(p, v->heads[i].head, 8);
    of_put(p + 8, v->heads[i].generation, 8);
    of_put(p + 16, v->heads[i].mode, 8);
    of_put(p + 24, v->heads[i].transform, 2);
    of_put(p + 26, v->heads[i].vrr, 2);
  }
  for (i = 0; i < v->group_count; ++i, p += GROUP) {
    const struct sophia_of_proposal_group *g = &v->groups[i];
    of_put(p, g->output, 8);
    of_put(p + 8, (uint32_t)g->x, 4);
    of_put(p + 12, (uint32_t)g->y, 4);
    of_put(p + 16, (uint32_t)g->width, 4);
    of_put(p + 20, (uint32_t)g->height, 4);
    of_put(p + 24, g->member_count, 2);
    of_members_write(p + 28, g->member_count, g->members);
  }
  *size = total;
  return 0;
}
