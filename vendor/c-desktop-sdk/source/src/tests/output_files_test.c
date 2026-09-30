/* Output file codec checks against literal bytes written from
 * spec/sophia-output-files-v1.kdl, independent of the encoders. */
#include "sophia_output_files.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static size_t hex(uint8_t *out, size_t capacity, const char *text) {
  size_t n = 0;
  while (*text) {
    unsigned value;
    if (*text == ' ' || *text == '\n') {
      ++text;
      continue;
    }
    assert(sscanf(text, "%2x", &value) == 1 && n < capacity);
    out[n++] = (uint8_t)value;
    text += 2;
  }
  return n;
}
static void put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (8 * i));
}
static void header(uint8_t *p, uint32_t size, uint16_t kind, uint64_t sub,
                   uint64_t seq) {
  put(p, size, 4);
  put(p + 4, 1, 2);
  put(p + 6, kind, 2);
  put(p + 8, 9, 8);
  put(p + 16, sub, 8);
  put(p + 24, seq, 8);
}
static int decodes(const uint8_t *p, size_t n) {
  struct sophia_of_record r;
  return sophia_of_decode(p, n, &r) == 0;
}
/* Encoding the decoded value must reproduce the literal exactly. */
static void round_trip(const uint8_t *p, size_t n) {
  struct sophia_of_record r;
  uint8_t out[SOPHIA_OF_MAX_CANDIDATE];
  size_t bytes = 0;
  assert(!sophia_of_decode(p, n, &r));
  assert(!sophia_of_encode(out, sizeof(out), &r, &bytes));
  assert(bytes == n && !memcmp(out, p, n));
  assert(sophia_of_encode(out, n - 1, &r, &bytes) == -4);
}

static void limits(void) {
  uint8_t b[128];
  struct sophia_of_record r;
  size_t n = hex(b, sizeof(b),
                 "48000000 0100 0100 0900000000000000 0000000000000000"
                 "0000000000000000"
                 "0100 1000 1000 8000 0400 4000 0008 0000"
                 "40000000 00400000 f8060000 e02e0000 d0070000 00100000");
  assert(n == SOPHIA_OF_LIMITS_RECORD);
  assert(!sophia_of_decode(b, n, &r));
  assert(r.header.kind == SOPHIA_OF_LIMITS && r.header.epoch == 9);
  assert(r.value.limits.journal_records == 64 &&
         r.value.limits.journal_bytes == 16384 &&
         r.value.limits.staging_bytes == 1784 &&
         r.value.limits.assembly_timeout_ms == 12000 &&
         r.value.limits.ack_timeout_ms == 2000 &&
         r.value.limits.max_domain_transactions == 4096);
  round_trip(b, n);
  /* Each fixed bound, the reserved field, and every resource's interval. */
  {
    static const struct {
      size_t at, width;
      uint64_t value;
    } bad[] = {{32, 2, 2},     {34, 2, 17},    {44, 2, 5},     {46, 2, 1},
               {48, 4, 7},     {48, 4, 65},    {52, 4, 2047},  {52, 4, 16385},
               {56, 4, 1783},  {60, 4, 0},     {60, 4, 12001}, {64, 4, 0},
               {64, 4, 2001},  {68, 4, 0},     {68, 4, 4097},  {36, 2, 127}};
    size_t i;
    for (i = 0; i < sizeof(bad) / sizeof(bad[0]); ++i) {
      uint8_t c[128];
      memcpy(c, b, n);
      put(c + bad[i].at, bad[i].value, bad[i].width);
      assert(!decodes(c, n));
    }
  }
  /* Minimum resources remain valid. */
  put(b + 48, 8, 4);
  put(b + 52, 2048, 4);
  put(b + 60, 1, 4);
  put(b + 64, 1, 4);
  put(b + 68, 1, 4);
  assert(decodes(b, n));
}

static void events(void) {
  uint8_t b[128], c[128];
  struct sophia_of_record r;
  size_t n;
  /* Negotiated: revision 1, observe|configure, fixed bounds. */
  n = hex(b, sizeof(b),
          "38000000 0100 1000 0900000000000000 0000000000000000"
          "0100000000000000 0100 000000000000 0300000000000000"
          "1000 1000 8000 0400");
  assert(!sophia_of_decode(b, n, &r) && r.value.granted_capabilities == 3);
  round_trip(b, n);
  memcpy(c, b, n);
  put(c + 40, 2, 8); /* configure without observe */
  assert(!decodes(c, n));
  memcpy(c, b, n);
  put(c + 40, 7, 8); /* unknown granted bit */
  assert(!decodes(c, n));
  memcpy(c, b, n);
  c[35] = 1; /* reserved */
  assert(!decodes(c, n));
  memcpy(c, b, n);
  put(c + 54, 5, 2); /* members per group */
  assert(!decodes(c, n));
  /* Refused: observation required. */
  n = hex(b, sizeof(b),
          "28000000 0100 1100 0900000000000000 0000000000000000"
          "0200000000000000 0200 000000000000");
  assert(!sophia_of_decode(b, n, &r) &&
         r.value.refusal == SOPHIA_OF_OBSERVATION_REQUIRED);
  round_trip(b, n);
  put(b + 32, 3, 2);
  assert(!decodes(b, n));
  /* Submitted: submission 5, Proposal. */
  n = hex(b, sizeof(b),
          "30000000 0100 1200 0900000000000000 0000000000000000"
          "0300000000000000 0500000000000000 0101 000000000000");
  assert(!sophia_of_decode(b, n, &r) && r.value.submitted.submission == 5 &&
         r.value.submitted.kind == SOPHIA_OF_PROPOSAL);
  round_trip(b, n);
  memcpy(c, b, n);
  put(c + 40, 258, 2);
  assert(!decodes(c, n));
  memcpy(c, b, n);
  put(c + 32, 0, 8);
  assert(!decodes(c, n));
  /* ObjectPublished: Topology epoch 7 at Qid path 90. */
  n = hex(b, sizeof(b),
          "38000000 0100 1300 0900000000000000 0000000000000000"
          "0400000000000000 0200 000000000000 0700000000000000"
          "5a00000000000000");
  assert(!sophia_of_decode(b, n, &r) && r.value.published.topology_epoch == 7 &&
         r.value.published.qid_path == 90);
  round_trip(b, n);
  memcpy(c, b, n);
  put(c + 32, 1, 2);
  assert(!decodes(c, n));
  memcpy(c, b, n);
  put(c + 48, 0, 8);
  assert(!decodes(c, n));
  /* Outcome: transaction 11 committed; an unknown reason is preserved. */
  n = hex(b, sizeof(b),
          "38000000 0100 2000 0900000000000000 0000000000000000"
          "0500000000000000 0b00000000000000 0700000000000000"
          "0200 ffff 00000000");
  assert(!sophia_of_decode(b, n, &r) && r.value.outcome.transaction == 11 &&
         r.value.outcome.outcome == SOPHIA_OF_COMMITTED &&
         r.value.outcome.reason == 0xffff);
  round_trip(b, n);
  memcpy(c, b, n);
  put(c + 48, 7, 2);
  assert(!decodes(c, n));
  memcpy(c, b, n);
  c[55] = 1;
  assert(!decodes(c, n));
}

static void identities(void) {
  uint8_t b[128];
  size_t n = hex(b, sizeof(b),
                 "30000000 0100 0001 0900000000000000 0100000000000000"
                 "0000000000000000 0100 0100 00000000 0300000000000000");
  struct sophia_of_record r;
  assert(!sophia_of_decode(b, n, &r) &&
         r.value.negotiate.capabilities == 3);
  round_trip(b, n);
  /* Unknown requested bits and revision ranges reach negotiation. */
  put(b + 40, UINT64_MAX, 8);
  put(b + 32, 7, 2);
  put(b + 34, 2, 2);
  assert(decodes(b, n));
  put(b + 36, 1, 4);
  assert(!decodes(b, n));
  put(b + 36, 0, 4);
  put(b + 24, 1, 8); /* candidate with a sequence */
  assert(!decodes(b, n));
  put(b + 24, 0, 8);
  put(b + 16, 0, 8); /* candidate without submission */
  assert(!decodes(b, n));
  put(b + 16, 1, 8);
  put(b + 8, 0, 8); /* zero epoch */
  assert(!decodes(b, n));
  put(b + 8, 9, 8);
  put(b + 4, 2, 2); /* version */
  assert(!decodes(b, n));
  put(b + 4, 1, 2);
  put(b + 6, 258, 2); /* unknown kind */
  assert(!decodes(b, n));
  put(b + 6, 256, 2);
  put(b, 47, 4); /* declared length */
  assert(!decodes(b, n));
  put(b, 48, 4);
  assert(!decodes(b, n - 1) && decodes(b, n));
  /* Objects carry neither identity; events need a sequence. */
  header(b, 72, SOPHIA_OF_LIMITS, 0, 1);
  assert(!decodes(b, 72));
  header(b, 56, SOPHIA_OF_OUTCOME, 1, 1);
  assert(!decodes(b, 56));
}

static size_t proposal(uint8_t *b, size_t capacity) {
  return hex(b, capacity,
             "a4000000 0100 0101 0900000000000000 0500000000000000"
             "0000000000000000"
             "0b00000000000000 0700000000000000 0200 0000 0100 0100"
             "0300000000000000 0400000000000000 0600000000000000"
             "0200 0200 00000000"
             "0200000000000000 00000000 00000000 d0020000 00050000 0100 0000"
             "0300000000000000 0100 0000"
             "000000000000000000000000 000000000000000000000000"
             "000000000000000000000000");
}

static void proposals(void) {
  uint8_t b[SOPHIA_OF_MAX_CANDIDATE + 16], c[SOPHIA_OF_MAX_CANDIDATE + 16];
  struct sophia_of_record r;
  size_t n = proposal(b, sizeof(b)), bytes;
  assert(n == 32 + 24 + 32 + 76);
  assert(!sophia_of_decode(b, n, &r));
  assert(r.value.proposal.transaction == 11 &&
         r.value.proposal.intent == SOPHIA_OF_APPLY &&
         r.value.proposal.heads[0].transform == SOPHIA_OF_ROTATE90 &&
         r.value.proposal.groups[0].height == 1280 &&
         r.value.proposal.groups[0].members[0].mapping == SOPHIA_OF_FIT);
  round_trip(b, n);
  /* Owner semantics: unknown heads, negative geometry, new output, zero
   * generation and a bad primary index are well-formed records. */
  memcpy(c, b, n);
  put(c + 56, 0, 8);
  put(c + 64, 0, 8);
  put(c + 88, 0, 8);
  put(c + 96, (uint32_t)-5, 4);
  put(c + 50, 9, 2);
  assert(!sophia_of_decode(c, n, &r) && r.value.proposal.groups[0].x == -5);
  {
    static const struct {
      size_t at, width;
      uint64_t value;
    } bad[] = {{32, 8, 0},  {40, 8, 0},  {48, 2, 0},   {48, 2, 3},
               {52, 2, 0},  {52, 2, 17}, {54, 2, 2},   {80, 2, 0},
               {80, 2, 9},  {82, 2, 0},  {82, 2, 4},   {84, 1, 1},
               {112, 2, 0}, {112, 2, 5}, {114, 1, 1},  {124, 2, 0},
               {124, 2, 4}, {126, 1, 1}, {128, 1, 1},  {163, 1, 1}};
    size_t i;
    for (i = 0; i < sizeof(bad) / sizeof(bad[0]); ++i) {
      memcpy(c, b, n);
      put(c + bad[i].at, bad[i].value, bad[i].width);
      assert(!decodes(c, n));
    }
  }
  /* The largest proposal is exactly the candidate bound. */
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_OF_PROPOSAL;
  r.header.epoch = 9;
  r.header.submission = 1;
  r.value.proposal.transaction = r.value.proposal.base_topology_epoch = 1;
  r.value.proposal.intent = SOPHIA_OF_VALIDATE_ONLY;
  r.value.proposal.head_count = r.value.proposal.group_count = 16;
  {
    unsigned i;
    for (i = 0; i < 16; ++i) {
      r.value.proposal.heads[i].transform = SOPHIA_OF_NORMAL;
      r.value.proposal.heads[i].vrr = SOPHIA_OF_VRR_DISABLED;
      r.value.proposal.groups[i].member_count = 4;
      r.value.proposal.groups[i].members[0].mapping = SOPHIA_OF_COVER;
      r.value.proposal.groups[i].members[1].mapping = SOPHIA_OF_COVER;
      r.value.proposal.groups[i].members[2].mapping = SOPHIA_OF_COVER;
      r.value.proposal.groups[i].members[3].mapping = SOPHIA_OF_COVER;
    }
  }
  assert(!sophia_of_encode(c, sizeof(c), &r, &bytes) &&
         bytes == SOPHIA_OF_MAX_CANDIDATE);
  assert(decodes(c, bytes));
  r.value.proposal.head_count = 17;
  assert(sophia_of_encode(c, sizeof(c), &r, &bytes) == -1);
  /* A candidate beyond the bound is refused even with a valid length word. */
  memset(c, 0, sizeof(c));
  header(c, SOPHIA_OF_MAX_CANDIDATE + 1, SOPHIA_OF_NEGOTIATE, 1, 0);
  assert(!decodes(c, SOPHIA_OF_MAX_CANDIDATE + 1));
  /* Topology goes through its own decoder. */
  header(c, 56, SOPHIA_OF_TOPOLOGY, 0, 0);
  assert(!decodes(c, 56));
}

/* One enabled head with one mode in one group: 32 + 24 + 104 + 24 + 84. */
static size_t topology(uint8_t *b) {
  size_t n = hex(b, 64,
                 "0c010000 0100 0200 0900000000000000 0000000000000000"
                 "0000000000000000"
                 "0700000000000000 0200000000000000 0100 0100 0100 0000");
  n += hex(b + n, 104,
           "0300000000000000 0400000000000000 0700 ff00 0400 0100"
           "0500000000000000 0000 000000000000 44502d31");
  memset(b + n, 0, 60);
  n += 60;
  n += hex(b + n, 24, "0500000000000000 80070000 38040000 60ea0000 0100 0000");
  n += hex(b + n, 84,
           "0200000000000000 0600000000000000 00000000 00000000"
           "80070000 38040000 0100 0000 0300000000000000 0300 0000");
  memset(b + n, 0, 36);
  return n + 36;
}

static struct sophia_of_topology t, u;

static int topology_decodes(const uint8_t *b, size_t n) {
  struct sophia_of_header h;
  return sophia_of_topology_decode(b, n, &h, &u) == 0;
}

static void topologies(void) {
  uint8_t b[SOPHIA_OF_MAX_TOPOLOGY], c[SOPHIA_OF_MAX_TOPOLOGY];
  struct sophia_of_header h;
  size_t n = topology(b), bytes;
  assert(n == 268);
  assert(!sophia_of_topology_decode(b, n, &h, &t));
  assert(h.kind == SOPHIA_OF_TOPOLOGY && h.epoch == 9);
  assert(t.topology_epoch == 7 && t.primary_output == 2 &&
         t.heads[0].flags == 7 && !strcmp(t.heads[0].label, "DP-1") &&
         t.heads[0].label_bytes == 4 && t.modes[0].width == 1920 &&
         t.groups[0].members[0].mapping == SOPHIA_OF_EXACT);
  assert(!sophia_of_topology_encode(c, sizeof(c), 9, &t, &bytes) &&
         bytes == n && !memcmp(c, b, n));
  assert(sophia_of_topology_encode(c, n - 1, 9, &t, &bytes) == -4);
  {
    struct sophia_of_record r;
    assert(sophia_of_decode(b, n, &r) == -1);
  }
  /* Structural refusals: counts, length, flags, transforms, label, modes. */
  {
    static const struct {
      size_t at, width;
      uint64_t value;
    } bad[] = {{32, 8, 0},   {40, 8, 3},   {48, 2, 0},   {50, 2, 2},
               {52, 2, 2},   {54, 2, 1},   {56, 8, 0},   {64, 8, 0},
               {72, 2, 8},   {74, 2, 0},   {74, 2, 256}, {76, 2, 0},
               {76, 2, 65},  {78, 2, 2},   {80, 8, 6},   {88, 2, 1},
               {90, 1, 1},   {100, 1, 1},  {160, 8, 0},  {168, 4, 0},
               {172, 4, 0},  {176, 4, 0},  {180, 2, 2},  {182, 1, 1},
               {184, 8, 0},  {192, 8, 0},  {200, 4, (uint32_t)-1},
               {208, 4, 0},  {216, 2, 0},  {216, 2, 5},  {218, 1, 1},
               {220, 8, 4},  {228, 2, 0},  {232, 1, 1},  {240, 1, 1}};
    size_t i;
    for (i = 0; i < sizeof(bad) / sizeof(bad[0]); ++i) {
      memcpy(c, b, n);
      put(c + bad[i].at, bad[i].value, bad[i].width);
      assert(!topology_decodes(c, n));
      assert(u.head_count == 0); /* cleared on failure */
    }
  }
  memcpy(c, b, n);
  c[96] = 0xc0; /* overlong UTF-8 */
  c[97] = 0x80;
  assert(!topology_decodes(c, n));
  memcpy(c, b, n);
  c[96] = 0xed; /* surrogate */
  c[97] = 0xa0;
  c[98] = 0x80;
  assert(!topology_decodes(c, n));
  memcpy(c, b, n);
  put(c, n - 1, 4);
  assert(!topology_decodes(c, n - 1));
  /* A disabled head may name any current mode, even outside its table. */
  memcpy(c, b, n);
  put(c + 72, 1, 2);
  put(c + 80, 99, 8);
  assert(topology_decodes(c, n) && u.heads[0].current_mode == 99);
  /* An enabled head must be connected with a mode of its own. */
  memcpy(&u, &t, sizeof(u));
  u.heads[0].flags = SOPHIA_OF_HEAD_ENABLED;
  assert(!sophia_of_topology_valid(&u));
  memcpy(&u, &t, sizeof(u));
  u.heads[0].current_mode = 0;
  assert(!sophia_of_topology_valid(&u));
}

/* Two heads in two overlapping groups is a valid snapshot; one head in two
 * groups, an absent primary and an ungrouped enabled head are not. */
static void invariants(void) {
  uint8_t b[SOPHIA_OF_MAX_TOPOLOGY];
  struct sophia_of_header h;
  size_t bytes;
  memcpy(&u, &t, sizeof(u));
  u.head_count = 2;
  u.mode_count = 2;
  u.heads[1] = t.heads[0];
  u.heads[1].head = 8;
  u.heads[1].first_mode = 1;
  u.heads[1].current_mode = 12;
  u.modes[1] = t.modes[0];
  u.modes[1].mode = 12;
  u.group_count = 2;
  u.groups[1] = t.groups[0];
  u.groups[1].output = 3;
  u.groups[1].members[0].head = 8;
  assert(sophia_of_topology_valid(&u));
  assert(!sophia_of_topology_encode(b, sizeof(b), 9, &u, &bytes));
  memset(&t, 0, sizeof(t));
  assert(!sophia_of_topology_decode(b, bytes, &h, &t) && t.head_count == 2 &&
         t.heads[1].first_mode == 1);
  u.groups[1].members[0].head = 3;
  assert(!sophia_of_topology_valid(&u));
  u.groups[1].members[0].head = 8;
  u.groups[1].member_count = 2;
  u.groups[1].members[1].head = 8;
  u.groups[1].members[1].mapping = SOPHIA_OF_FIT;
  assert(!sophia_of_topology_valid(&u));
  u.groups[1].member_count = 1;
  u.groups[1].output = 2;
  assert(!sophia_of_topology_valid(&u));
  u.groups[1].output = 3;
  u.primary_output = 4;
  assert(!sophia_of_topology_valid(&u));
  u.primary_output = 2;
  u.group_count = 1; /* head 8 is enabled but ungrouped */
  assert(!sophia_of_topology_valid(&u));
  u.heads[1].flags = SOPHIA_OF_HEAD_CONNECTED;
  assert(sophia_of_topology_valid(&u));
  u.heads[1].first_mode = 0;
  assert(!sophia_of_topology_valid(&u));
  u.heads[1].first_mode = 1;
  u.modes[1].mode = 5; /* the same id on another head is permitted */
  assert(sophia_of_topology_valid(&u));
  u.heads[1].mode_count = 2;
  u.mode_count = 3;
  u.modes[2] = u.modes[1]; /* duplicate within one head */
  assert(!sophia_of_topology_valid(&u));
  assert(sophia_of_topology_encode(b, sizeof(b), 9, &u, &bytes) == -1);
  assert(sophia_of_topology_encode(b, sizeof(b), 0, &t, &bytes) == -1);
}

static void maximum_topology(void) {
  static uint8_t b[SOPHIA_OF_MAX_TOPOLOGY];
  struct sophia_of_header h;
  size_t bytes;
  unsigned i, m;
  memset(&u, 0, sizeof(u));
  u.topology_epoch = 1;
  u.primary_output = 1;
  u.head_count = u.group_count = 16;
  u.mode_count = 2048;
  for (i = 0; i < 16; ++i) {
    u.heads[i].head = i + 1;
    u.heads[i].generation = 1;
    u.heads[i].flags = SOPHIA_OF_HEAD_CONNECTED | SOPHIA_OF_HEAD_ENABLED;
    u.heads[i].transforms = 1;
    u.heads[i].label_bytes = 64;
    memset(u.heads[i].label, 'a', 64);
    u.heads[i].mode_count = 128;
    u.heads[i].first_mode = (uint16_t)(i * 128);
    u.heads[i].current_mode = 1;
    for (m = 0; m < 128; ++m) {
      struct sophia_of_mode *mode = &u.modes[i * 128 + m];
      mode->mode = m + 1;
      mode->width = mode->height = 64;
      mode->refresh_millihz = 60000;
    }
    u.groups[i].output = i + 1;
    u.groups[i].generation = 1;
    u.groups[i].width = u.groups[i].height = 64;
    u.groups[i].member_count = 1;
    u.groups[i].members[0].head = i + 1;
    u.groups[i].members[0].mapping = SOPHIA_OF_EXACT;
  }
  assert(!sophia_of_topology_encode(b, sizeof(b), 9, &u, &bytes) &&
         bytes == SOPHIA_OF_MAX_TOPOLOGY);
  assert(!sophia_of_topology_decode(b, bytes, &h, &t) &&
         t.heads[15].label_bytes == 64 && t.modes[2047].mode == 128);
}

static void controls(void) {
  uint8_t s[24], a[16], expect[24];
  assert(!sophia_of_submit_encode(s, 9, 5, 164));
  hex(expect, sizeof(expect),
      "0900000000000000 0500000000000000 a4000000 00000000");
  assert(!memcmp(s, expect, 24));
  assert(sophia_of_submit_encode(s, 9, 5, 47) == -1);
  assert(sophia_of_submit_encode(s, 9, 5, 1785) == -1);
  assert(sophia_of_submit_encode(s, 0, 5, 48) == -1);
  assert(sophia_of_submit_encode(s, 9, 0, 48) == -1);
  assert(!sophia_of_ack_encode(a, 9, 3));
  hex(expect, sizeof(expect), "0900000000000000 0300000000000000");
  assert(!memcmp(a, expect, 16));
  assert(sophia_of_ack_encode(a, 9, 0) == -1);
  assert(sophia_of_decode(NULL, 0, NULL) == -4);
}

int main(void) {
  limits();
  events();
  identities();
  proposals();
  topologies();
  invariants();
  maximum_topology();
  controls();
  puts("output_files_test: ok");
  return 0;
}
