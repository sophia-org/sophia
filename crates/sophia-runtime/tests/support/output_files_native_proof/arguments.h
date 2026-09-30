/* Argument and layout parsing for the output files native proof peer. Pure:
 * no I/O, no session and no allocation. Included once, by the peer, so its
 * definitions are static in that translation unit.
 *
 *   --stage=STAGE [--deadline-ms=N] --a-topology-epoch=E
 *   --a-heads=H --a-groups=G --a-primary=P [--b-heads=H --b-groups=G
 *   --b-primary=P]
 *
 *   STAGE  validate | reject | commit-restore | apply-await-termination
 *   H      HEAD:MODE:TRANSFORM:VRR[,...]            1..16 enabled heads
 *   G      OUTPUT@X,Y,WxH=HEAD/MAP[+HEAD/MAP][;...]  1..16 groups, 1..4 members
 *   P      zero-based index of the primary group
 *
 * IDs are strict nonzero decimal u64 without leading zeros. OUTPUT names an
 * existing output: this proof never requests allocation. Each argument
 * appears at most once. */
#ifndef OUTPUT_FILES_NATIVE_PROOF_ARGUMENTS_H
#define OUTPUT_FILES_NATIVE_PROOF_ARGUMENTS_H
#include "sophia_output_files.h"
#include <stdint.h>
#include <string.h>

#define PROOF_DEFAULT_DEADLINE_MS 30000u
#define PROOF_MAX_DEADLINE_MS 120000u
#define PROOF_MAX_ARGUMENT_BYTES 4096u

enum proof_stage { VALIDATE, REJECT, COMMIT_RESTORE, APPLY_AWAIT_TERMINATION };

struct layout_head {
  uint64_t head, mode;
  uint16_t transform, vrr;
};
struct layout_group {
  uint64_t output;
  int32_t x, y, width, height;
  unsigned member_count;
  struct sophia_of_member members[SOPHIA_OF_MAX_MEMBERS];
};
struct layout {
  unsigned head_count, group_count, primary;
  struct layout_head heads[SOPHIA_OF_MAX_HEADS];
  struct layout_group groups[SOPHIA_OF_MAX_GROUPS];
};
struct proof_arguments {
  enum proof_stage stage;
  const char *stage_name; /* NULL until --stage parsed */
  const char *layout_name; /* "A" or "B" for a layout refusal */
  uint64_t deadline_ms, a_epoch;
  struct layout a, b;
  int have_b;
};

/* Strict nonzero decimal u64: digits only, no leading zero, overflow
 * refused. */
static int parse_id(const char *text, size_t n, uint64_t *out) {
  uint64_t value = 0;
  size_t i;
  if (!n || text[0] == '0')
    return -1;
  for (i = 0; i < n; ++i) {
    unsigned digit;
    if (text[i] < '0' || text[i] > '9')
      return -1;
    digit = (unsigned)(text[i] - '0');
    if (value > (UINT64_MAX - digit) / 10)
      return -1;
    value = value * 10 + digit;
  }
  *out = value;
  return 0;
}
/* Zero-based group index, strict decimal, below the group bound. */
static int parse_index(const char *text, unsigned *out) {
  uint64_t value;
  if (!strcmp(text, "0")) {
    *out = 0;
    return 0;
  }
  if (parse_id(text, strlen(text), &value) || value >= SOPHIA_OF_MAX_GROUPS)
    return -1;
  *out = (unsigned)value;
  return 0;
}
/* Decimal i32 with an optional leading minus; the owner judges the value. */
static int parse_i32(const char *text, size_t n, int32_t *out) {
  int64_t value = 0;
  size_t i = 0;
  int negative = 0;
  if (n && text[0] == '-') {
    negative = 1;
    i = 1;
  }
  if (i == n)
    return -1;
  for (; i < n; ++i) {
    if (text[i] < '0' || text[i] > '9')
      return -1;
    value = value * 10 + (text[i] - '0');
    if (value > (int64_t)INT32_MAX + negative)
      return -1;
  }
  *out = (int32_t)(negative ? -value : value);
  return 0;
}
static int parse_word(const char *text, size_t n, const char *const *words,
                      unsigned count, uint16_t *out) {
  unsigned i;
  for (i = 0; i < count; ++i)
    if (strlen(words[i]) == n && !memcmp(text, words[i], n)) {
      *out = (uint16_t)(i + 1);
      return 0;
    }
  return -1;
}
static const char *const proof_transforms[] = {
    "normal",  "90",         "180",         "270",
    "flipped", "flipped-90", "flipped-180", "flipped-270"};
static const char *const proof_vrrs[] = {"disabled", "automatic", "always"};
static const char *const proof_mappings[] = {"fit", "cover", "exact"};

/* Split [start, end) at the first separator; returns the field length and
 * sets *next past the separator, or NULL when there was none. */
static size_t field(const char *start, const char *end, char separator,
                    const char **next) {
  const char *p = start;
  while (p < end && *p != separator)
    ++p;
  *next = p < end ? p + 1 : NULL;
  return (size_t)(p - start);
}
static const char *parse_heads(const char *text, struct layout *layout) {
  const char *p = text, *end = text + strlen(text);
  if (!*text)
    return "empty head list";
  while (p) {
    const char *entry_end, *q, *next;
    struct layout_head *h;
    size_t n = field(p, end, ',', &next);
    unsigned i;
    if (layout->head_count == SOPHIA_OF_MAX_HEADS)
      return "more than 16 heads";
    h = &layout->heads[layout->head_count++];
    entry_end = p + n;
    n = field(p, entry_end, ':', &q);
    if (!q || parse_id(p, n, &h->head))
      return "bad head id";
    p = q;
    n = field(p, entry_end, ':', &q);
    if (!q || parse_id(p, n, &h->mode))
      return "bad mode id";
    p = q;
    n = field(p, entry_end, ':', &q);
    if (!q || parse_word(p, n, proof_transforms, 8, &h->transform))
      return "bad transform";
    if (parse_word(q, (size_t)(entry_end - q), proof_vrrs, 3, &h->vrr))
      return "bad vrr";
    for (i = 0; i + 1 < layout->head_count; ++i)
      if (layout->heads[i].head == h->head)
        return "duplicate head";
    p = next;
  }
  return NULL;
}
static const char *parse_members(const char *p, const char *end,
                                 struct layout_group *g) {
  while (p) {
    const char *next, *slash;
    size_t n = field(p, end, '+', &next);
    const char *entry_end = p + n;
    struct sophia_of_member *m;
    if (g->member_count == SOPHIA_OF_MAX_MEMBERS)
      return "more than 4 members";
    m = &g->members[g->member_count++];
    n = field(p, entry_end, '/', &slash);
    if (!slash || parse_id(p, n, &m->head))
      return "bad member head";
    if (parse_word(slash, (size_t)(entry_end - slash), proof_mappings, 3,
                   &m->mapping))
      return "bad mapping";
    p = next;
  }
  return NULL;
}
static const char *parse_groups(const char *text, struct layout *layout) {
  const char *p = text, *end = text + strlen(text);
  if (!*text)
    return "empty group list";
  while (p) {
    const char *next, *entry_end, *q, *reason;
    struct layout_group *g;
    size_t n = field(p, end, ';', &next);
    unsigned i;
    if (layout->group_count == SOPHIA_OF_MAX_GROUPS)
      return "more than 16 groups";
    g = &layout->groups[layout->group_count++];
    entry_end = p + n;
    n = field(p, entry_end, '@', &q);
    if (!q || parse_id(p, n, &g->output))
      return "bad output id";
    p = q;
    n = field(p, entry_end, ',', &q);
    if (!q || parse_i32(p, n, &g->x))
      return "bad x";
    p = q;
    n = field(p, entry_end, ',', &q);
    if (!q || parse_i32(p, n, &g->y))
      return "bad y";
    p = q;
    n = field(p, entry_end, 'x', &q);
    if (!q || parse_i32(p, n, &g->width))
      return "bad width";
    p = q;
    n = field(p, entry_end, '=', &q);
    if (!q || parse_i32(p, n, &g->height))
      return "bad height";
    if ((reason = parse_members(q, entry_end, g)))
      return reason;
    for (i = 0; i + 1 < layout->group_count; ++i)
      if (layout->groups[i].output == g->output)
        return "duplicate output";
    p = next;
  }
  return NULL;
}
/* Each listed head is in exactly one group and each member is listed. */
static const char *check_layout(const struct layout *layout) {
  unsigned h, g, m, seen;
  if (layout->primary >= layout->group_count)
    return "primary index out of range";
  for (h = 0; h < layout->head_count; ++h) {
    for (seen = 0, g = 0; g < layout->group_count; ++g)
      for (m = 0; m < layout->groups[g].member_count; ++m)
        seen += layout->groups[g].members[m].head == layout->heads[h].head;
    if (seen != 1)
      return "each head must be in exactly one group";
  }
  for (g = 0; g < layout->group_count; ++g)
    for (m = 0; m < layout->groups[g].member_count; ++m) {
      for (h = 0; h < layout->head_count; ++h)
        if (layout->heads[h].head == layout->groups[g].members[m].head)
          break;
      if (h == layout->head_count)
        return "group member is not a listed head";
    }
  return NULL;
}
/* Equal on every field a revision-1 topology can show: heads and modes,
 * outputs, geometry, membership, mappings and primary. */
static int same_visible(const struct layout *a, const struct layout *b) {
  unsigned i, j;
  if (a->head_count != b->head_count || a->group_count != b->group_count ||
      a->groups[a->primary].output != b->groups[b->primary].output)
    return 0;
  for (i = 0; i < a->head_count; ++i) {
    for (j = 0; j < b->head_count; ++j)
      if (b->heads[j].head == a->heads[i].head)
        break;
    if (j == b->head_count || b->heads[j].mode != a->heads[i].mode)
      return 0;
  }
  for (i = 0; i < a->group_count; ++i) {
    const struct layout_group *x = &a->groups[i], *y = NULL;
    for (j = 0; j < b->group_count; ++j)
      if (b->groups[j].output == x->output)
        y = &b->groups[j];
    if (!y || x->x != y->x || x->y != y->y || x->width != y->width ||
        x->height != y->height || x->member_count != y->member_count)
      return 0;
    for (j = 0; j < x->member_count; ++j) {
      unsigned k;
      for (k = 0; k < y->member_count; ++k)
        if (y->members[k].head == x->members[j].head &&
            y->members[k].mapping == x->members[j].mapping)
          break;
      if (k == y->member_count)
        return 0;
    }
  }
  return 1;
}
static const char *parse_layout(const char *heads, const char *groups,
                                const char *primary, struct layout *layout) {
  const char *reason;
  if ((reason = parse_heads(heads, layout)) ||
      (reason = parse_groups(groups, layout)))
    return reason;
  if (parse_index(primary, &layout->primary))
    return "bad primary index";
  return check_layout(layout);
}

/* NULL on success, else a refusal reason. out must be zeroed. */
static const char *proof_parse_arguments(int argc, char **argv,
                                         struct proof_arguments *out) {
  static const struct {
    const char *name;
    enum proof_stage stage;
  } stages[] = {{"validate", VALIDATE},
                {"reject", REJECT},
                {"commit-restore", COMMIT_RESTORE},
                {"apply-await-termination", APPLY_AWAIT_TERMINATION}};
  enum { STAGE, DEADLINE, EPOCH, A_HEADS, A_GROUPS, A_PRIMARY, B_HEADS,
         B_GROUPS, B_PRIMARY, KEYS };
  static const char *const keys[KEYS] = {
      "--stage",   "--deadline-ms", "--a-topology-epoch",
      "--a-heads", "--a-groups",    "--a-primary",
      "--b-heads", "--b-groups",    "--b-primary"};
  const char *values[KEYS] = {NULL};
  const char *reason;
  unsigned k;
  int i;
  out->deadline_ms = PROOF_DEFAULT_DEADLINE_MS;
  if (argc < 6 || argc > 10)
    return "expected 5 to 9 arguments";
  for (i = 1; i < argc; ++i) {
    const char *equals = strchr(argv[i], '=');
    size_t key = equals ? (size_t)(equals - argv[i]) : strlen(argv[i]);
    if (strlen(argv[i]) > PROOF_MAX_ARGUMENT_BYTES)
      return "argument longer than 4096 bytes";
    for (k = 0; k < KEYS; ++k)
      if (strlen(keys[k]) == key && !memcmp(argv[i], keys[k], key))
        break;
    if (k == KEYS || !equals)
      return "unknown argument";
    if (values[k])
      return "repeated argument";
    values[k] = equals + 1;
  }
  if (values[STAGE]) {
    for (k = 0; k < sizeof(stages) / sizeof(stages[0]); ++k)
      if (!strcmp(values[STAGE], stages[k].name)) {
        out->stage = stages[k].stage;
        out->stage_name = stages[k].name;
      }
    if (!out->stage_name)
      return "unknown stage";
  }
  if (!values[STAGE] || !values[EPOCH] || !values[A_HEADS] ||
      !values[A_GROUPS] || !values[A_PRIMARY])
    return "--stage and all four --a-* arguments are required";
  if (values[DEADLINE] &&
      (parse_id(values[DEADLINE], strlen(values[DEADLINE]), &out->deadline_ms) ||
       out->deadline_ms > PROOF_MAX_DEADLINE_MS))
    return "--deadline-ms must be 1..120000";
  if (parse_id(values[EPOCH], strlen(values[EPOCH]), &out->a_epoch))
    return "bad --a-topology-epoch";
  out->layout_name = "A";
  if ((reason = parse_layout(values[A_HEADS], values[A_GROUPS],
                             values[A_PRIMARY], &out->a)))
    return reason;
  out->have_b = values[B_HEADS] || values[B_GROUPS] || values[B_PRIMARY];
  if (out->have_b) {
    out->layout_name = "B";
    if (!values[B_HEADS] || !values[B_GROUPS] || !values[B_PRIMARY])
      return "--b-heads, --b-groups and --b-primary go together";
    if ((reason = parse_layout(values[B_HEADS], values[B_GROUPS],
                               values[B_PRIMARY], &out->b)))
      return reason;
  }
  out->layout_name = NULL;
  if ((out->stage == REJECT) == out->have_b)
    return out->stage == REJECT ? "reject takes layout A only"
                                : "this stage requires layout B";
  if (out->have_b && same_visible(&out->a, &out->b))
    return "layout B must differ from A in a verifiable field";
  return NULL;
}
#endif
