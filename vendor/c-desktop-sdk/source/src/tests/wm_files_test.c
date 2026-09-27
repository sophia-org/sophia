#include "../sophia_wm_files.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Test construction is independent of the SDK encoder. Each complete vector
 * names literal wire offsets from the pinned KDL. No live Session claim. */
static void put(uint8_t *p, uint64_t value, size_t width) {
  size_t i;
  for (i = 0; i < width; ++i) {
    p[i] = (uint8_t)value;
    value >>= 8;
  }
}
static size_t record(uint8_t *p, uint16_t kind, size_t body) {
  size_t n = 32 + body;
  memset(p, 0, n);
  put(p, n, 4);
  put(p + 4, 1, 2);
  put(p + 6, kind, 2);
  put(p + 8, 7, 8);
  if (kind >= 256)
    put(p + 16, 3, 8);
  else if (kind >= 16)
    put(p + 24, 5, 8);
  return n;
}
static void reject(const uint8_t *p, size_t n, uint64_t caps) {
  struct sophia_wf_record out, sentinel;
  memset(&sentinel, 0xa5, sizeof(sentinel));
  out = sentinel;
  assert(sophia_wf_decode(p, n, caps, &out) == -1);
  assert(!memcmp(&out, &sentinel, sizeof(out)));
}
static void roundtrip(const uint8_t *p, size_t n, uint64_t caps) {
  struct sophia_wf_record r;
  uint8_t encoded[4096], sentinel[4096], bad[4096];
  size_t used, i;
  assert(n <= sizeof(encoded));
  assert(!sophia_wf_decode(p, n, caps, &r));
  assert(!sophia_wf_encode(encoded, sizeof(encoded), caps, &r, &used));
  assert(used == n && !memcmp(encoded, p, n));
  memset(sentinel, 0xa5, sizeof(sentinel));
  memcpy(encoded, sentinel, sizeof(encoded));
  used = 909;
  assert(sophia_wf_encode(encoded, n - 1, caps, &r, &used) == -4);
  assert(used == 909 && !memcmp(encoded, sentinel, sizeof(encoded)));
  for (i = 0; i < n; ++i)
    reject(p, i, caps);
  memcpy(bad, p, n);
  bad[4] = 2;
  reject(bad, n, caps);
  memcpy(bad, p, n);
  memset(bad + 8, 0, 8);
  reject(bad, n, caps);
  memcpy(bad, p, n);
  bad[6] = 0;
  bad[7] = 0;
  reject(bad, n, caps);
  memcpy(bad, p, n);
  bad[n] = 0;
  put(bad, n + 1, 4);
  reject(bad, n + 1, caps);
  memcpy(bad, p, n);
  if (r.header.kind < 16)
    bad[24] = 1;
  else if (r.header.kind < 256)
    bad[16] = 1;
  else
    bad[24] = 1;
  reject(bad, n, caps);
}
static void scalar_vectors(void) {
  uint8_t p[512];
  size_t n, i;
  uint16_t k;
  struct sophia_wf_record r;
  n = record(p, 1, 32);
  put(p + 32, UINT64_MAX, 8);
  put(p + 40, 1048576, 4);
  put(p + 44, 1048576, 4);
  put(p + 48, 64, 2);
  put(p + 50, 32, 2);
  put(p + 52, 12000, 4);
  put(p + 56, 4000, 4);
  put(p + 60, 1, 2);
  roundtrip(p, n, 0);
  p[62] = 1;
  reject(p, n, 0);
  p[62] = 0;
  p[40] = 1;
  reject(p, n, 0);
  n = record(p, 256, 16);
  put(p + 32, 1, 8);
  put(p + 40, 2, 8);
  roundtrip(p, n, 0);
  p[40] = 1;
  reject(p, n, 0);
  n = record(p, 16, 8);
  put(p + 32, UINT64_MAX, 8);
  roundtrip(p, n, 0);
  n = record(p, 17, 16);
  put(p + 32, 3, 8);
  put(p + 40, 262, 2);
  roundtrip(p, n, 0);
  put(p + 40, 264, 2);
  reject(p, n, 0);
  for (k = 18; k <= 20; ++k) {
    n = record(p, k, 48);
    put(p + 32, 91, 8);
    put(p + 40, 4, 8);
    for (i = 0; i < 32; ++i)
      p[48 + i] = (uint8_t)i;
    roundtrip(p, n, UINT64_MAX);
    reject(p, n, 0);
  }
  for (k = 257; k <= 259; ++k) {
    n = record(p, k, 56);
    put(p + 32, 91, 8);
    put(p + 40, 4, 8);
    p[48] = 1;
    put(p + 80, 2, 2);
    roundtrip(p, n, UINT64_MAX);
    p[82] = 1;
    reject(p, n, UINT64_MAX);
    p[82] = 0;
    p[80] = 4;
    reject(p, n, UINT64_MAX);
  }
  n = record(p, 260, 48);
  put(p + 32, 100, 8);
  put(p + 40, 12, 8);
  put(p + 48, 3, 2);
  put(p + 52, 1, 4);
  put(p + 56, 0xaabbcc, 4);
  put(p + 60, 2, 4);
  put(p + 64, 0xffffff, 4);
  put(p + 68, 0x102030, 4);
  roundtrip(p, n, UINT64_MAX);
  p[59] = 0xff;
  reject(p, n, UINT64_MAX);
  n = record(p, 261, 32);
  put(p + 32, 15, 8);
  put(p + 40, 2, 2);
  put(p + 48, 10, 8);
  put(p + 56, 20, 8);
  roundtrip(p, n, UINT64_MAX);
  reject(p, n, 0);
  put(p + 56, 10, 8);
  reject(p, n, UINT64_MAX);
  n = record(p, 263, 32);
  put(p + 32, 100, 8);
  put(p + 40, 27, 8);
  put(p + 48, 81, 8);
  roundtrip(p, n, UINT64_MAX);
  put(p + 56, UINT32_MAX, 4);
  put(p + 60, 1, 4);
  reject(p, n, UINT64_MAX);
  put(p + 56, 0, 4);
  roundtrip(p, n, UINT64_MAX);
  for (k = 21; k <= 24; k += 3) {
    n = record(p, k, 24);
    put(p + 32, 100, 8);
    put(p + 40, 12, 8);
    put(p + 48, 5, 2);
    roundtrip(p, n, 0);
    p[50] = 1;
    reject(p, n, 0);
  }
  n = record(p, 23, 32);
  put(p + 32, 100, 8);
  put(p + 40, 27, 8);
  put(p + 48, 3, 8);
  put(p + 56, 1, 2);
  put(p + 58, 1, 2);
  roundtrip(p, n, 0);
  p[56] = 2;
  reject(p, n, 0);
  p[58] = 0;
  roundtrip(p, n, 0);
  assert(!sophia_wf_decode(p, n, 0, &r));
  assert(r.value.projection_outcome.transaction == 100 &&
         r.value.projection_outcome.request_id == 27);
  n = record(p, 25, 48);
  for (i = 0; i < 5; ++i)
    put(p + 32 + i * 8, i + 1, 8);
  put(p + 72, 3, 2);
  roundtrip(p, n, 0);
}
static size_t cycle(uint8_t *p, unsigned cause, size_t bytes) {
  size_t i, n = record(p, 22, 48 + 16 + bytes);
  for (i = 0; i < 5; ++i)
    put(p + 32 + i * 8, i + 1, 8);
  put(p + 72, cause, 2);
  put(p + 74, 2, 2);
  put(p + 80, 10, 8);
  put(p + 88, 20, 8);
  return n;
}
static void cycle_vectors(void) {
  uint8_t p[512];
  size_t n, i;
  struct sophia_wf_record r;
  n = cycle(p, 0, 0);
  roundtrip(p, n, 0);
  put(p + 88, 10, 8);
  reject(p, n, UINT64_MAX);
  n = cycle(p, 1, 16);
  put(p + 96, 7, 8);
  put(p + 104, 8, 8);
  roundtrip(p, n, UINT64_MAX);
  reject(p, n, 0);
  n = cycle(p, 2, 8);
  put(p + 100, 9, 4);
  roundtrip(p, n, 0); /* Index zero is present. */
  memset(p + 96, 0, 8);
  reject(p, n, UINT64_MAX);
  n = cycle(p, 3, 16);
  put(p + 96, 20, 8);
  roundtrip(p, n, UINT64_MAX);
  put(p + 96, 30, 8);
  reject(p, n, UINT64_MAX);
  n = cycle(p, 4, 32);
  put(p + 96, 2, 2);
  put(p + 98, 1, 2);
  put(p + 104, 1, 4);
  put(p + 108, 9, 4);
  put(p + 112, UINT32_C(0xfffffffe), 4);
  put(p + 116, UINT32_C(0x80000000), 4);
  put(p + 120, 300, 4);
  put(p + 124, 200, 4);
  roundtrip(p, n, UINT64_MAX);
  assert(!sophia_wf_decode(p, n, UINT64_MAX, &r));
  assert(r.value.cycle.value.interaction.x == -2 &&
         r.value.cycle.value.interaction.y == INT32_MIN);
  p[98] = 4;
  p[100] = 2;
  memset(p + 120, 0, 8);
  roundtrip(p, n, UINT64_MAX);
  memset(p + 112, 0, 8);
  reject(p, n, UINT64_MAX);
  p[96] = 4;
  roundtrip(p, n, UINT64_MAX);
  n = cycle(p, 5, 32);
  put(p + 96, 1, 8);
  put(p + 104, 2, 8);
  put(p + 112, 10, 8);
  put(p + 120, 3, 8);
  roundtrip(p, n, UINT64_MAX);
  n = cycle(p, 6, 64);
  for (i = 0; i < 6; ++i)
    put(p + 96 + i * 8, i + 1, 8);
  put(p + 120, 20, 8);
  roundtrip(p, n, UINT64_MAX);
  put(p + 144, 19, 8);
  reject(p, n, UINT64_MAX);
  put(p + 152, 2, 8);
  roundtrip(p, n, UINT64_MAX);
}
static void sections(void) {
  uint8_t p[4096], encoded[4096], unchanged[4096];
  struct sophia_wf_record r;
  size_t n, used;
  n = record(p, 2, 32 + 16 + 56);
  put(p + 32, 71, 8);
  put(p + 40, 9, 8);
  put(p + 48, 10, 8);
  put(p + 56, 1, 2);
  put(p + 64, 1, 2);
  put(p + 68, 1, 4);
  put(p + 72, 56, 4);
  put(p + 80, 10, 8);
  put(p + 88, 1, 8);
  put(p + 112, 640, 4);
  put(p + 116, 480, 4);
  put(p + 128, 640, 4);
  put(p + 132, 480, 4);
  roundtrip(p, n, 0);
  assert(!sophia_wf_decode(p, n, 0, &r));
  assert(r.sections[0].rows == p + 80 && r.sections[0].bytes == 56);
  memset(unchanged, 0xa5, sizeof(unchanged));
  memcpy(encoded, unchanged, sizeof(encoded));
  r.sections[0].count = UINT32_MAX;
  used = 912;
  assert(sophia_wf_encode(encoded, sizeof(encoded), 0, &r, &used) == -1);
  assert(used == 912 && !memcmp(encoded, unchanged, sizeof(encoded)));
  p[48] = 11;
  reject(p, n, UINT64_MAX);
  p[48] = 10;
  p[66] = 1;
  reject(p, n, UINT64_MAX);
  p[66] = 0;
  put(p + 72, UINT32_MAX, 4);
  reject(p, n, UINT64_MAX);
  put(p + 72, 56, 4);
  n = record(p, 262, 40 + 16 + 24 + 16 + 32);
  put(p + 32, 71, 8);
  put(p + 40, 7, 8);
  put(p + 48, 9, 8);
  put(p + 56, 10, 8);
  put(p + 64, 2, 2);
  put(p + 72, 1, 2);
  put(p + 76, 1, 4);
  put(p + 80, 24, 4);
  put(p + 88, 10, 8);
  put(p + 112, 65288, 2);
  put(p + 116, 1, 4);
  put(p + 120, 32, 4);
  put(p + 128, 10, 8);
  put(p + 136, 1, 8);
  put(p + 144, 7, 8);
  put(p + 152, 99, 8);
  roundtrip(p, n, UINT64_MAX);
  reject(p, n, SOPHIA_WF_CAP_OUTPUT_LAUNCH_CONTEXT);
  reject(p, n, SOPHIA_WF_CAP_LAUNCH_ORIGIN);
  put(p + 112, 1, 2);
  reject(p, n, UINT64_MAX);
}
static void control_writes(void) {
  uint8_t p[24], old[24];
  memset(old, 0xa5, sizeof(old));
  memcpy(p, old, sizeof(p));
  assert(sophia_wf_submit_encode(p, 0, 1, 32) == -1 &&
         !memcmp(p, old, sizeof(p)));
  assert(sophia_wf_submit_encode(p, 7, 1, 1048577) == -1 &&
         !memcmp(p, old, sizeof(p)));
  assert(sophia_wf_ack_encode(p, 7, 0) == -1 && !memcmp(p, old, sizeof(p)));
  assert(!sophia_wf_submit_encode(p, 7, 81, 1048576));
  assert(p[0] == 7 && p[8] == 81 && p[18] == 16 && !p[20]);
  assert(!sophia_wf_ack_encode(p, 7, UINT64_MAX));
  assert(p[0] == 7 && p[8] == 255 && p[15] == 255);
}
static unsigned hex(char c) {
  assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
  return c <= '9' ? (unsigned)(c - '0') : (unsigned)(c - 'a' + 10);
}
#define ROW(name, checks)                                                      \
  do {                                                                         \
    if (!strcmp(line, #name)) {                                                \
      struct sophia_wf_##name v, sentinel;                                     \
      memset(&sentinel, 0xa5, sizeof(sentinel));                               \
      assert(!sophia_wf_##name##_decode(wire, count, &v));                     \
      assert(checks);                                                          \
      assert(!sophia_wf_##name##_encode(out, sizeof(out), &v));                \
      assert(!memcmp(wire, out, count));                                       \
      for (j = 0; j < count; ++j) {                                            \
        v = sentinel;                                                          \
        assert(sophia_wf_##name##_decode(wire, j, &v));                        \
        assert(!memcmp(&v, &sentinel, sizeof(v)));                             \
      }                                                                        \
      ++seen;                                                                  \
      goto row_done;                                                           \
    }                                                                          \
  } while (0)
static void golden_rows(void) {
  FILE *file = fopen("spec/golden/sophia-wm-v1.records", "r");
  char line[1024], *value, *end;
  uint8_t wire[512], out[512];
  size_t count, i, j, seen = 0;
  assert(file);
  while (fgets(line, sizeof(line), file)) {
    if (line[0] == '#')
      continue;
    value = strchr(line, '|');
    assert(value);
    *value++ = 0;
    end = strchr(value, '\n');
    assert(end);
    *end = 0;
    count = strlen(value) / 2;
    assert(count && count < sizeof(wire) && strlen(value) == count * 2);
    for (i = 0; i < count; ++i)
      wire[i] = (uint8_t)(hex(value[i * 2]) * 16 + hex(value[i * 2 + 1]));
    ROW(snapshot_output, v.output == 1 && v.focus_index == 3 &&
                             v.width == 1920 && v.work_y == 24);
    ROW(snapshot_surface, v.surface_index == 3 && v.state_generation == 7 &&
                              v.capability_bits == 31 && v.width == 800);
    ROW(snapshot_action,
        v.action == 5 && v.name_len == 10 && !memcmp(v.name, "focus-next", 10));
    ROW(snapshot_session_operation,
        v.operation == 11 && v.slot == 1 && v.target_bits == 1);
    ROW(projection_output,
        v.output == 1 && v.placement_count == 1 && v.focus_index == 3);
    ROW(projection_placement, v.surface_index == 3 && v.state_generation == 7 &&
                                  v.requested_height == 600 &&
                                  v.transform == 1);
    ROW(projection_indicator, v.indicator == 7 && v.action == 5 &&
                                  v.label_len == 3 &&
                                  !memcmp(v.label, "web", 3));
    ROW(projection_output_status, v.output == 1 && v.focus_bits == 1 &&
                                      v.layout_len == 4 &&
                                      !memcmp(v.layout, "Tall", 4));
    ROW(snapshot_surface_classification, v.surface_index == 3 &&
                                             v.surface_generation == 1 &&
                                             v.classification == 2);
    ROW(projection_launch_context, v.surface_index == 3 &&
                                       v.surface_generation == 1 &&
                                       v.epoch == 1 && v.token == 1);
    ROW(projection_output_launch_context,
        v.output == 1 && v.generation == 1 && v.epoch == 1 && v.token == 1);
    ROW(snapshot_launch_origin, v.surface_index == 3 &&
                                    v.surface_generation == 1 && v.epoch == 1 &&
                                    v.token == 1);
    ROW(snapshot_output_policy_key,
        v.output == 1 && v.generation == 1 && v.policy_key == 1);
    ROW(projection_tab_group, v.output == 1 && v.group == 1 && v.width == 1 &&
                                  v.member_count == 1 && v.focused == 1);
    ROW(projection_tab_member, v.output == 1 && v.group == 1 &&
                                   v.surface_index == 1 &&
                                   v.surface_generation == 1);
    ROW(projection_translation_group, v.output == 1 && v.group == 1 &&
                                          v.x == 1 && v.y == 0 &&
                                          v.member_count == 1);
    ROW(projection_translation_member, v.output == 1 && v.group == 1 &&
                                           v.surface_index == 1 &&
                                           v.surface_generation == 1);
    ROW(projection_presentation, v.generation == 1 && v.keyboard_output == 1 &&
                                     v.binding_count == 1 &&
                                     v.instance_count == 1);
    ROW(projection_presentation_output, v.output == 1 && v.generation == 1 &&
                                            v.width == 1280 &&
                                            v.height == 720 && v.mode == 2);
    ROW(projection_surface_instance,
        v.id == 2 && v.source_index == 1 && v.width == 320 &&
            v.opacity_millis == 1000 && v.action == 5);
    ROW(projection_presentation_region, v.id == 1 && v.width == 1280 &&
                                            v.clip_height == 720 &&
                                            v.role == 1 && v.action == 0);
    ROW(projection_presentation_binding,
        v.action == 5 && v.keycode == 28 && v.modifiers == 0);
    assert(!"unrecognized golden row");
  row_done:;
  }
  assert(!ferror(file) && seen == 22);
  assert(!fclose(file));
}
#undef ROW
int main(void) {
  scalar_vectors();
  cycle_vectors();
  sections();
  control_writes();
  golden_rows();
  puts("wm_files_test: literal bodies, causes, sections and malformed controls "
       "passed");
  return 0;
}
